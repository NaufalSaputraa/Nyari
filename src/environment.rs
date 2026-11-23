use crate::dpi::scale_factor;
use crate::ipc::{ShortcutAction, UIAction, UIEvent, V8HeapProfile};
use crate::NyariError;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use include_dir::{include_dir, Dir};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::ptr;
use std::rc::Rc;
use tao::event_loop::EventLoopProxy;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2EnvironmentWithOptions, GetAvailableCoreWebView2BrowserVersionString,
    ICoreWebView2, ICoreWebView2Controller, ICoreWebView2Controller2, ICoreWebView2Controller3,
    ICoreWebView2Environment, ICoreWebView2Environment10, ICoreWebView2Environment5,
    ICoreWebView2EnvironmentOptions, ICoreWebView2Settings3, ICoreWebView2Settings4,
    ICoreWebView2Settings5, ICoreWebView2Settings6, ICoreWebView2Settings9, ICoreWebView2_22,
    COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS, COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED,
    COREWEBVIEW2_COLOR, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
    COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
};
use webview2_com::{
    take_pwstr, AcceleratorKeyPressedEventHandler, BrowserProcessExitedEventHandler,
    CoreWebView2CustomSchemeRegistration, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewBrowserVersionAvailableEventHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler,
};
use windows::core::{w, Interface, BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{E_FAIL, HWND, RECT};
use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_ESCAPE, VK_SHIFT};
use windows::Win32::UI::Shell::SHCreateMemStream;

static UI_DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ui/dist");

const NYARI_SHELL_PREFIX: &str = "nyari://shell";

const DISABLE_SERVICE_WORKER_JS: &str = r#"
if (location.protocol === "nyari:" && "serviceWorker" in navigator) {
  try {
    Object.defineProperty(navigator, "serviceWorker", {
      value: undefined,
      configurable: false,
      writable: false
    });
  } catch (_) {}
}
"#;

/// Asynchronous lifecycle events emitted by [`NyariEnvironment`].
///
/// All variants are `Send` so they can be dispatched through `tao::event_loop::EventLoopProxy`
/// while keeping COM STA interfaces strictly pinned to the main UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentEvent {
    Ready,
    CreateFailed(String),
    RuntimeUpdateAvailable { version: String },
    BrowserProcessCrashed { reason: String, recovering: bool },
}

/// Owned handle to the initialized WebView2 environment and its registered environment-level events.
pub struct NyariEnvironment {
    env: ICoreWebView2Environment,
    version_token: i64,
    process_exited_token: Option<i64>,
}

impl NyariEnvironment {
    /// Returns a reference to the underlying `ICoreWebView2Environment`.
    pub fn raw(&self) -> &ICoreWebView2Environment {
        &self.env
    }
}

impl Drop for NyariEnvironment {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .env
                .remove_NewBrowserVersionAvailable(self.version_token);
            if let Some(token) = self.process_exited_token {
                if let Ok(env5) = self.env.cast::<ICoreWebView2Environment5>() {
                    let _ = env5.remove_BrowserProcessExited(token);
                }
            }
        }
    }
}

/// Callbacks mapping WebView2 controller events into the application's `EventLoopProxy<E>`.
#[derive(Clone, Copy)]
pub struct ShellControllerMapper<E: 'static> {
    pub on_ready: fn() -> E,
    pub on_action: fn(UIAction) -> E,
    pub on_shortcut: fn(ShortcutAction) -> E,
    pub on_error: fn(String) -> E,
}

/// Generates a 24-byte CSPRNG nonce via Win32 `BCryptGenRandom` encoded as 32-char unpadded base64url.
pub fn generate_csp_nonce() -> Result<String, NyariError> {
    let mut bytes = [0u8; 24];
    let status = unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    status.ok().map_err(NyariError::Win32)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Maps a relative asset path extension to its static MIME content-type string.
pub fn mime_type_for_path(path: &str) -> &'static str {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    match file_name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("json") => "application/json; charset=utf-8",
        Some("woff2") => "font/woff2",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// Resolves an embedded asset from `ui/dist` for `nyari://shell/...` URIs, rejecting traversal.
pub fn resolve_nyari_asset(request_uri: &str) -> Option<(&'static [u8], &'static str)> {
    let rest = request_uri.strip_prefix(NYARI_SHELL_PREFIX)?;
    if !rest.is_empty()
        && !rest.starts_with('/')
        && !rest.starts_with('?')
        && !rest.starts_with('#')
    {
        return None;
    }

    let path_part = rest.split(['?', '#']).next().unwrap_or("");
    let rel = path_part.trim_start_matches('/');
    if rel.contains('\\') || rel.contains('\0') {
        return None;
    }

    let lower = rel.to_ascii_lowercase();
    if lower.contains("%2e")
        || lower.contains("%5c")
        || lower.contains("%2f")
        || lower.contains("%00")
    {
        return None;
    }

    if rel.split('/').any(|seg| seg == "." || seg == "..") {
        return None;
    }

    let normalized = if rel.is_empty() { "index.html" } else { rel };
    let file = UI_DIST.get_file(normalized)?;
    Some((file.contents(), mime_type_for_path(normalized)))
}

/// Injects `nonce="{nonce}"` onto `<script` tags inside served `index.html`.
pub fn rewrite_html_with_nonce(html: &str, nonce: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() + 64);
    let mut cursor = 0usize;

    while let Some(rel_idx) = lower[cursor..].find("<script") {
        let tag_start = cursor + rel_idx;
        out.push_str(&html[cursor..tag_start]);

        let after_tag_name = tag_start + "<script".len();
        let next_byte = html.as_bytes().get(after_tag_name).copied();
        let is_script_tag = matches!(next_byte, Some(b'>' | b' ' | b'\t' | b'\r' | b'\n'));

        if !is_script_tag {
            out.push_str(&html[tag_start..after_tag_name]);
            cursor = after_tag_name;
            continue;
        }

        out.push_str("<script");
        if let Some(gt_rel) = html[after_tag_name..].find('>') {
            let tag_end = after_tag_name + gt_rel;
            let attrs = &lower[after_tag_name..tag_end];
            if !attrs.contains("nonce=") {
                out.push_str(&format!(r#" nonce="{nonce}""#));
            }
            out.push_str(&html[after_tag_name..=tag_end]);
            cursor = tag_end + 1;
        } else {
            cursor = after_tag_name;
        }
    }

    out.push_str(&html[cursor..]);
    out
}

/// Builds strict security response headers for `nyari://shell/` responses.
pub fn build_nyari_response_headers(nonce: &str, mime: &str) -> String {
    format!(
        "Content-Type: {mime}\r\n\
         Content-Security-Policy: default-src 'self' nyari://shell; script-src 'self' 'nonce-{nonce}'; style-src 'self' 'unsafe-inline' nyari://shell; img-src 'self' data: https:; connect-src 'none'; object-src 'none'; frame-ancestors 'none'\r\n\
         X-Frame-Options: DENY\r\n\
         Access-Control-Allow-Origin: nyari://shell\r\n\
         Cache-Control: no-store"
    )
}

/// Builds additional Chromium browser arguments for the selected `V8HeapProfile`.
pub fn build_browser_arguments(v8_heap_profile: V8HeapProfile) -> String {
    match v8_heap_profile {
        V8HeapProfile::Eco => "--js-flags=--max-old-space-size=512".to_string(),
        V8HeapProfile::Balanced => "--js-flags=--max-old-space-size=1024".to_string(),
        V8HeapProfile::Unlimited => String::new(),
    }
}

/// Creates WebView2 environment options with `nyari://` registered as a secure scheme.
pub fn create_environment_options(
    v8_heap_profile: V8HeapProfile,
) -> Result<ICoreWebView2EnvironmentOptions, NyariError> {
    let scheme = CoreWebView2CustomSchemeRegistration::new("nyari".to_string());
    unsafe {
        scheme.set_treat_as_secure(true);
        scheme.set_has_authority_component(true);
        scheme.set_allowed_origins(vec!["nyari://shell".to_string()]);
    }

    let options = CoreWebView2EnvironmentOptions::default();
    let args = build_browser_arguments(v8_heap_profile);
    unsafe {
        if !args.is_empty() {
            options.set_additional_browser_arguments(args);
        }
        options.set_exclusive_user_data_folder_access(true);
        options.set_is_custom_crash_reporting_enabled(true);
        options.set_are_browser_extensions_enabled(true);
        options.set_scheme_registrations(vec![Some(scheme.into())]);
    }

    Ok(options.into())
}

/// Validates that `uri` belongs strictly to the `nyari://shell/` origin.
pub fn is_allowed_shell_navigation(uri: &str) -> bool {
    uri.starts_with("nyari://shell/")
}

/// Determines whether `NavigationCompleted` should fire `on_ready()` once for `nyari://shell/`.
pub fn should_fire_shell_ready(already_fired: &mut bool, is_success: bool, uri: &str) -> bool {
    if *already_fired || !is_success || !is_allowed_shell_navigation(uri) {
        return false;
    }
    *already_fired = true;
    true
}

/// Validates `WebMessageReceived` `Source()` starts with `nyari://shell/` and parses `UIAction`.
pub fn parse_shell_web_message(source: &str, payload: &str) -> Result<UIAction, NyariError> {
    if !is_allowed_shell_navigation(source) {
        return Err(NyariError::InvalidInput(format!(
            "Rejected WebMessage from untrusted origin: {source}"
        )));
    }
    serde_json::from_str::<UIAction>(payload).map_err(NyariError::from)
}

/// Encodes a Rust string slice as a null-terminated UTF-16 `Vec<u16>` for Win32/COM APIs.
pub fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Initializes the shared WebView2 environment asynchronously without blocking the STA loop.
pub fn create_environment_async<E: 'static>(
    user_data_dir: &Path,
    v8_heap_profile: V8HeapProfile,
    env_slot: Rc<RefCell<Option<NyariEnvironment>>>,
    proxy: EventLoopProxy<E>,
    map_event: fn(EnvironmentEvent) -> E,
) -> Result<(), NyariError> {
    std::fs::create_dir_all(user_data_dir)?;
    let options = create_environment_options(v8_heap_profile)?;
    let user_data_w = to_wide_null(&user_data_dir.to_string_lossy());

    let proxy_ready = proxy.clone();
    let handler =
        CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |result, env_opt| {
            if let Err(err) = result {
                let _ = proxy_ready.send_event(map_event(EnvironmentEvent::CreateFailed(format!(
                    "WebView2 environment creation failed: {err}"
                ))));
                return Ok(());
            }

            let Some(env) = env_opt else {
                let _ = proxy_ready.send_event(map_event(EnvironmentEvent::CreateFailed(
                    "WebView2 returned a null environment".to_string(),
                )));
                return Ok(());
            };

            let mut version_token = 0i64;
            let proxy_ver = proxy_ready.clone();
            let ver_handler =
                NewBrowserVersionAvailableEventHandler::create(Box::new(move |env_opt, _| {
                    let version = env_opt
                        .and_then(|e| {
                            let mut pwstr = PWSTR(ptr::null_mut());
                            unsafe { e.BrowserVersionString(&mut pwstr) }
                                .ok()
                                .map(|()| take_pwstr(pwstr))
                        })
                        .or_else(|| {
                            let mut pwstr = PWSTR(ptr::null_mut());
                            unsafe {
                                GetAvailableCoreWebView2BrowserVersionString(
                                    PCWSTR::null(),
                                    &mut pwstr,
                                )
                            }
                            .ok()
                            .map(|()| take_pwstr(pwstr))
                        })
                        .unwrap_or_else(|| "new".to_string());
                    let _ =
                        proxy_ver.send_event(map_event(EnvironmentEvent::RuntimeUpdateAvailable {
                            version,
                        }));
                    Ok(())
                }));
            unsafe {
                env.add_NewBrowserVersionAvailable(&ver_handler, &mut version_token)?;
            }

            let process_exited_token = if let Ok(env5) = env.cast::<ICoreWebView2Environment5>() {
                let mut token = 0i64;
                let proxy_crash = proxy_ready.clone();
                let exit_handler =
                    BrowserProcessExitedEventHandler::create(Box::new(move |_, args_opt| {
                        let recovering = args_opt
                            .and_then(|args| {
                                let mut kind = Default::default();
                                unsafe { args.BrowserProcessExitKind(&mut kind) }
                                    .ok()
                                    .map(|()| kind == COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED)
                            })
                            .unwrap_or(false);
                        let reason = if recovering {
                            "WebView2 browser process exited unexpectedly".to_string()
                        } else {
                            "WebView2 browser process exited".to_string()
                        };
                        let _ = proxy_crash.send_event(map_event(
                            EnvironmentEvent::BrowserProcessCrashed { reason, recovering },
                        ));
                        Ok(())
                    }));
                if unsafe { env5.add_BrowserProcessExited(&exit_handler, &mut token) }.is_ok() {
                    Some(token)
                } else {
                    None
                }
            } else {
                None
            };

            *env_slot.borrow_mut() = Some(NyariEnvironment {
                env,
                version_token,
                process_exited_token,
            });
            let _ = proxy_ready.send_event(map_event(EnvironmentEvent::Ready));
            Ok(())
        }));

    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            PCWSTR(user_data_w.as_ptr()),
            &options,
            &handler,
        )
    }
    .map_err(NyariError::Win32)
}

/// Attaches the in-memory `nyari://shell/*` resource handler with per-response CSP nonces.
pub fn attach_nyari_scheme_handler(
    webview: &ICoreWebView2,
    env: &ICoreWebView2Environment,
) -> Result<i64, NyariError> {
    if let Ok(wv22) = webview.cast::<ICoreWebView2_22>() {
        unsafe {
            wv22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                w!("nyari://shell/*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )?;
        }
    } else {
        unsafe {
            webview.AddWebResourceRequestedFilter(
                w!("nyari://shell/*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            )?;
        }
    }

    let sw_script_w = to_wide_null(DISABLE_SERVICE_WORKER_JS);
    unsafe {
        let _ = webview.AddScriptToExecuteOnDocumentCreated(PCWSTR(sw_script_w.as_ptr()), None);
    }

    let env_clone = env.clone();
    let mut token = 0i64;
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, args_opt| {
        let Some(args) = args_opt else {
            return Ok(());
        };
        let request = unsafe { args.Request() }?;
        let mut uri_pwstr = PWSTR(ptr::null_mut());
        unsafe { request.Uri(&mut uri_pwstr) }?;
        let uri = take_pwstr(uri_pwstr);

        if let Some((bytes, mime)) = resolve_nyari_asset(&uri) {
            let nonce = generate_csp_nonce().map_err(|_| windows::core::Error::from(E_FAIL))?;
            let owned_body: Vec<u8>;
            let slice: &[u8] = if mime.starts_with("text/html") {
                let html =
                    std::str::from_utf8(bytes).map_err(|_| windows::core::Error::from(E_FAIL))?;
                owned_body = rewrite_html_with_nonce(html, &nonce).into_bytes();
                &owned_body
            } else {
                bytes
            };

            let stream = unsafe { SHCreateMemStream(Some(slice)) }
                .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            let headers = build_nyari_response_headers(&nonce, mime);
            let headers_w = to_wide_null(&headers);
            let response = unsafe {
                env_clone.CreateWebResourceResponse(
                    &stream,
                    200,
                    w!("OK"),
                    PCWSTR(headers_w.as_ptr()),
                )
            }?;
            unsafe { args.SetResponse(&response) }?;
        } else {
            let response = unsafe {
                env_clone.CreateWebResourceResponse(
                    None,
                    404,
                    w!("Not Found"),
                    w!("Cache-Control: no-store\r\nX-Frame-Options: DENY"),
                )
            }?;
            unsafe { args.SetResponse(&response) }?;
        }

        Ok(())
    }));

    unsafe {
        webview.add_WebResourceRequested(&handler, &mut token)?;
    }
    Ok(token)
}

/// Creates a transparent (`A = 0`) Shell UI controller on the `default` profile (`ProfileName = nullptr`).
#[allow(clippy::too_many_arguments)]
pub fn create_shell_controller<E: 'static>(
    env: &ICoreWebView2Environment,
    parent_hwnd: HWND,
    initial_bounds: RECT,
    initial_visible: bool,
    initial_dpi: u32,
    shell_uri: &'static str,
    controller_slot: Rc<RefCell<Option<ICoreWebView2Controller>>>,
    proxy: EventLoopProxy<E>,
    mapper: ShellControllerMapper<E>,
) -> Result<(), NyariError> {
    let env_clone = env.clone();
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |result, controller_opt| {
            if let Err(err) = result {
                let _ = proxy.send_event((mapper.on_error)(format!(
                    "WebView2 controller creation failed: {err}"
                )));
                return Ok(());
            }

            let Some(controller) = controller_opt else {
                let _ = proxy.send_event((mapper.on_error)(
                    "WebView2 returned null controller".to_string(),
                ));
                return Ok(());
            };

            if let Ok(controller2) = controller.cast::<ICoreWebView2Controller2>() {
                unsafe {
                    let _ = controller2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                        A: 0,
                        R: 0,
                        G: 0,
                        B: 0,
                    });
                }
            }

            if let Ok(controller3) = controller.cast::<ICoreWebView2Controller3>() {
                unsafe {
                    let _ = controller3.SetBoundsMode(COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS);
                    let _ = controller3.SetRasterizationScale(scale_factor(initial_dpi));
                }
            }

            unsafe {
                let _ = controller.SetBounds(initial_bounds);
                let _ = controller.SetIsVisible(initial_visible);
            }

            let webview = unsafe { controller.CoreWebView2() }?;
            if let Ok(settings) = unsafe { webview.Settings() } {
                unsafe {
                    let _ = settings.SetIsWebMessageEnabled(true);
                    let _ = settings.SetAreHostObjectsAllowed(false);
                    let _ = settings.SetIsStatusBarEnabled(false);
                    let _ = settings.SetAreDefaultScriptDialogsEnabled(false);
                    let _ = settings.SetAreDefaultContextMenusEnabled(false);
                    let _ = settings.SetIsZoomControlEnabled(false);
                    let _ = settings.SetIsBuiltInErrorPageEnabled(false);
                }
                if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
                    unsafe {
                        let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
                    }
                }
                if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
                    unsafe {
                        let _ = s4.SetIsGeneralAutofillEnabled(false);
                        let _ = s4.SetIsPasswordAutosaveEnabled(false);
                    }
                }
                if let Ok(s5) = settings.cast::<ICoreWebView2Settings5>() {
                    unsafe {
                        let _ = s5.SetIsPinchZoomEnabled(false);
                    }
                }
                if let Ok(s6) = settings.cast::<ICoreWebView2Settings6>() {
                    unsafe {
                        let _ = s6.SetIsSwipeNavigationEnabled(false);
                    }
                }
                if let Ok(s9) = settings.cast::<ICoreWebView2Settings9>() {
                    unsafe {
                        let _ = s9.SetIsNonClientRegionSupportEnabled(true);
                    }
                }
            }

            if let Err(err) = attach_nyari_scheme_handler(&webview, &env_clone) {
                let _ = proxy.send_event((mapper.on_error)(format!(
                    "Failed to attach nyari:// scheme handler: {err}"
                )));
                return Ok(());
            }

            let mut nav_token = 0i64;
            let nav_handler = NavigationStartingEventHandler::create(Box::new(|_, args_opt| {
                let Some(args) = args_opt else {
                    return Ok(());
                };
                let mut uri_pwstr = PWSTR(ptr::null_mut());
                unsafe { args.Uri(&mut uri_pwstr) }?;
                let uri = take_pwstr(uri_pwstr);
                if !is_allowed_shell_navigation(&uri) {
                    unsafe { args.SetCancel(true) }?;
                }
                Ok(())
            }));
            unsafe {
                let _ = webview.add_NavigationStarting(&nav_handler, &mut nav_token);
            }

            let mut msg_token = 0i64;
            let proxy_msg = proxy.clone();
            let msg_handler =
                WebMessageReceivedEventHandler::create(Box::new(move |_, args_opt| {
                    let Some(args) = args_opt else {
                        return Ok(());
                    };
                    let mut src_pwstr = PWSTR(ptr::null_mut());
                    unsafe { args.Source(&mut src_pwstr) }?;
                    let source = take_pwstr(src_pwstr);

                    let mut msg_pwstr = PWSTR(ptr::null_mut());
                    let payload =
                        if unsafe { args.TryGetWebMessageAsString(&mut msg_pwstr) }.is_ok() {
                            take_pwstr(msg_pwstr)
                        } else {
                            unsafe { args.WebMessageAsJson(&mut msg_pwstr) }?;
                            take_pwstr(msg_pwstr)
                        };

                    if let Ok(action) = parse_shell_web_message(&source, &payload) {
                        let _ = proxy_msg.send_event((mapper.on_action)(action));
                    }
                    Ok(())
                }));
            unsafe {
                let _ = webview.add_WebMessageReceived(&msg_handler, &mut msg_token);
            }

            let mut accel_token = 0i64;
            let proxy_accel = proxy.clone();
            let accel_handler =
                AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args_opt| {
                    let Some(args) = args_opt else {
                        return Ok(());
                    };
                    let mut kind = Default::default();
                    unsafe { args.KeyEventKind(&mut kind) }?;
                    if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                        && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
                    {
                        return Ok(());
                    }

                    let mut vkey = 0u32;
                    unsafe { args.VirtualKey(&mut vkey) }?;
                    let ctrl = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) } < 0;
                    let shift = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0;

                    if ctrl && !shift && (vkey == u32::from(b'K') || vkey == u32::from(b'L')) {
                        unsafe { args.SetHandled(true) }?;
                        let _ = proxy_accel
                            .send_event((mapper.on_shortcut)(ShortcutAction::OpenCommandPalette));
                    } else if ctrl && !shift && vkey == u32::from(b'F') {
                        unsafe { args.SetHandled(true) }?;
                        let _ = proxy_accel
                            .send_event((mapper.on_shortcut)(ShortcutAction::OpenFindInPage));
                    } else if ctrl && !shift && vkey == u32::from(b'S') {
                        unsafe { args.SetHandled(true) }?;
                        let _ = proxy_accel.send_event((mapper.on_action)(UIAction::ToggleZen));
                    } else if !ctrl && !shift && vkey == u32::from(VK_ESCAPE.0) {
                        unsafe { args.SetHandled(true) }?;
                        let _ = proxy_accel
                            .send_event((mapper.on_shortcut)(ShortcutAction::CloseActiveOverlay));
                    }
                    Ok(())
                }));
            unsafe {
                let _ = controller.add_AcceleratorKeyPressed(&accel_handler, &mut accel_token);
            }

            *controller_slot.borrow_mut() = Some(controller);

            let mut nav_completed_token = 0i64;
            let fired_ready = Rc::new(Cell::new(false));
            let proxy_ready = proxy.clone();
            let nav_completed_handler =
                NavigationCompletedEventHandler::create(Box::new(move |webview_opt, args_opt| {
                    let is_success = args_opt
                        .and_then(|args| {
                            let mut ok = BOOL(0);
                            unsafe { args.IsSuccess(&mut ok) }
                                .ok()
                                .map(|()| ok.as_bool())
                        })
                        .unwrap_or(false);
                    let source = webview_opt
                        .and_then(|wv| {
                            let mut pwstr = PWSTR(ptr::null_mut());
                            unsafe { wv.Source(&mut pwstr) }
                                .ok()
                                .map(|()| take_pwstr(pwstr))
                        })
                        .unwrap_or_default();

                    let mut fired_flag = fired_ready.get();
                    if should_fire_shell_ready(&mut fired_flag, is_success, &source) {
                        fired_ready.set(fired_flag);
                        let _ = proxy_ready.send_event((mapper.on_ready)());
                    } else if !fired_flag && !is_success {
                        let _ = proxy_ready.send_event((mapper.on_error)(format!(
                            "Shell UI navigation failed for {source}"
                        )));
                    }
                    Ok(())
                }));
            unsafe {
                let _ = webview
                    .add_NavigationCompleted(&nav_completed_handler, &mut nav_completed_token);
            }

            let uri_w = to_wide_null(shell_uri);
            unsafe {
                let _ = webview.Navigate(PCWSTR(uri_w.as_ptr()));
            }
            Ok(())
        },
    ));

    if let Ok(env10) = env.cast::<ICoreWebView2Environment10>() {
        let ctrl_opts = unsafe { env10.CreateCoreWebView2ControllerOptions() }?;
        // Keep ProfileName = nullptr (default profile) for < 500 ms Shell UI startup.
        unsafe {
            ctrl_opts.SetIsInPrivateModeEnabled(false)?;
            env10.CreateCoreWebView2ControllerWithOptions(parent_hwnd, &ctrl_opts, &handler)?;
        }
    } else {
        unsafe {
            env.CreateCoreWebView2Controller(parent_hwnd, &handler)?;
        }
    }

    Ok(())
}

/// Pushes a typed [`UIEvent`] JSON payload to a Shell UI WebView2 controller.
pub fn post_ui_event(
    controller: &ICoreWebView2Controller,
    event: &UIEvent,
) -> Result<(), NyariError> {
    let json = serde_json::to_string(event)?;
    let json_w = to_wide_null(&json);
    let webview = unsafe { controller.CoreWebView2() }?;
    unsafe { webview.PostWebMessageAsJson(PCWSTR(json_w.as_ptr())) }.map_err(NyariError::Win32)
}

/// Updates `RasterizationScale` on a WebView2 controller after a Per-Monitor `WM_DPICHANGED` event.
pub fn update_controller_scale(controller: &ICoreWebView2Controller, dpi: u32) {
    if let Ok(controller3) = controller.cast::<ICoreWebView2Controller3>() {
        unsafe {
            let _ = controller3.SetRasterizationScale(scale_factor(dpi));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::LayoutMode;
    use std::ptr;
    use webview2_com::take_pwstr;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2CustomSchemeRegistration, ICoreWebView2EnvironmentOptions2,
        ICoreWebView2EnvironmentOptions3, ICoreWebView2EnvironmentOptions4,
        ICoreWebView2EnvironmentOptions6,
    };
    use windows::core::{Interface, BOOL, PWSTR};
    use windows::Win32::System::Com::CoTaskMemFree;

    #[test]
    fn generate_csp_nonce_produces_unique_32_char_base64url_strings() {
        let a = generate_csp_nonce().expect("first nonce");
        let b = generate_csp_nonce().expect("second nonce");
        assert_eq!(a.len(), 32);
        assert_eq!(b.len(), 32);
        assert_ne!(a, b);
        assert!(a
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_'));
    }

    #[test]
    fn build_nyari_response_headers_sets_strict_csp_cors_and_frame_options() {
        let headers = build_nyari_response_headers("abcNonce123", "text/html; charset=utf-8");
        assert!(headers.contains("Content-Type: text/html; charset=utf-8"));
        assert!(headers.contains("Access-Control-Allow-Origin: nyari://shell"));
        assert!(headers.contains("X-Frame-Options: DENY"));
        assert!(headers.contains("Cache-Control: no-store"));
        assert!(headers.contains("script-src 'self' 'nonce-abcNonce123'"));
        assert!(headers.contains("connect-src 'none'"));
        assert!(headers.contains("object-src 'none'"));
        assert!(headers.contains("frame-ancestors 'none'"));
    }

    #[test]
    fn rewrite_html_with_nonce_injects_nonce_into_script_tags() {
        let raw = r#"<!doctype html><html><head><script type="module" crossorigin src="/assets/app.js"></script></head><body><script>console.log(1)</script></body></html>"#;
        let rewritten = rewrite_html_with_nonce(raw, "nonceXYZ");
        assert!(rewritten.contains(r#"<script nonce="nonceXYZ" type="module""#));
        assert!(rewritten.contains(r#"<script nonce="nonceXYZ">console.log(1)</script>"#));
    }

    #[test]
    fn resolve_nyari_asset_serves_root_and_built_bundle_and_rejects_traversal() {
        let (index_bytes, index_mime) =
            resolve_nyari_asset("nyari://shell/?layer=chrome").expect("root should resolve");
        assert_eq!(index_mime, "text/html; charset=utf-8");
        let html = std::str::from_utf8(index_bytes).expect("index.html should be UTF-8");
        assert!(html.contains("/assets/"));

        let js_start = html
            .find("/assets/")
            .expect("Vite JS asset path in index.html");
        let js_end = html[js_start..]
            .find('"')
            .expect("closing quote after asset path")
            + js_start;
        let js_uri = format!("nyari://shell{}", &html[js_start..js_end]);
        let (js_bytes, js_mime) =
            resolve_nyari_asset(&js_uri).expect("built JS bundle asset should resolve");
        assert_eq!(js_mime, "application/javascript; charset=utf-8");
        assert!(!js_bytes.is_empty());

        let (slash_bytes, slash_mime) =
            resolve_nyari_asset("nyari://shell/").expect("bare root should resolve");
        assert_eq!(slash_mime, "text/html; charset=utf-8");
        assert_eq!(index_bytes, slash_bytes);

        assert!(resolve_nyari_asset("nyari://shell/../Cargo.toml").is_none());
        assert!(resolve_nyari_asset("nyari://shell/..\\Cargo.toml").is_none());
        assert!(resolve_nyari_asset("nyari://shell/%2e%2e/Cargo.toml").is_none());
        assert!(resolve_nyari_asset("nyari://evil/index.html").is_none());
        assert!(resolve_nyari_asset("https://example.com/").is_none());
    }

    #[test]
    fn mime_type_for_path_maps_web_assets_accurately() {
        assert_eq!(mime_type_for_path("index.html"), "text/html; charset=utf-8");
        assert_eq!(
            mime_type_for_path("assets/index-123.js"),
            "application/javascript; charset=utf-8"
        );
        assert_eq!(
            mime_type_for_path("assets/style.css"),
            "text/css; charset=utf-8"
        );
        assert_eq!(mime_type_for_path("icon.svg"), "image/svg+xml");
        assert_eq!(mime_type_for_path("logo.png"), "image/png");
    }

    #[test]
    fn build_browser_arguments_matches_v8_heap_profiles() {
        assert_eq!(
            build_browser_arguments(V8HeapProfile::Eco),
            "--js-flags=--max-old-space-size=512"
        );
        assert_eq!(
            build_browser_arguments(V8HeapProfile::Balanced),
            "--js-flags=--max-old-space-size=1024"
        );
        let unlimited = build_browser_arguments(V8HeapProfile::Unlimited);
        assert!(!unlimited.contains("max-old-space-size"));
    }

    #[test]
    fn create_environment_options_configures_security_flags_and_nyari_scheme() {
        let opts = create_environment_options(V8HeapProfile::Eco)
            .expect("create_environment_options should succeed");

        let mut args_pwstr = PWSTR(ptr::null_mut());
        unsafe { opts.AdditionalBrowserArguments(&mut args_pwstr) }.unwrap();
        assert_eq!(
            take_pwstr(args_pwstr),
            "--js-flags=--max-old-space-size=512"
        );

        let opts2: ICoreWebView2EnvironmentOptions2 = opts.cast().unwrap();
        let mut exclusive = BOOL(0);
        unsafe { opts2.ExclusiveUserDataFolderAccess(&mut exclusive) }.unwrap();
        assert!(exclusive.as_bool());

        let opts3: ICoreWebView2EnvironmentOptions3 = opts.cast().unwrap();
        let mut crash_reporting = BOOL(0);
        unsafe { opts3.IsCustomCrashReportingEnabled(&mut crash_reporting) }.unwrap();
        assert!(crash_reporting.as_bool());

        let opts6: ICoreWebView2EnvironmentOptions6 = opts.cast().unwrap();
        let mut extensions = BOOL(0);
        unsafe { opts6.AreBrowserExtensionsEnabled(&mut extensions) }.unwrap();
        assert!(extensions.as_bool());

        let opts4: ICoreWebView2EnvironmentOptions4 = opts.cast().unwrap();
        let mut count = 0u32;
        let mut raw_schemes: *mut Option<ICoreWebView2CustomSchemeRegistration> = ptr::null_mut();
        unsafe { opts4.GetCustomSchemeRegistrations(&mut count, &mut raw_schemes) }.unwrap();
        assert_eq!(count, 1);
        assert!(!raw_schemes.is_null());

        let scheme = unsafe { ptr::read(raw_schemes) }.expect("registered scheme");
        unsafe { CoTaskMemFree(Some(raw_schemes.cast())) };

        let mut name = PWSTR(ptr::null_mut());
        unsafe { scheme.SchemeName(&mut name) }.unwrap();
        assert_eq!(take_pwstr(name), "nyari");

        let mut secure = BOOL(0);
        unsafe { scheme.TreatAsSecure(&mut secure) }.unwrap();
        assert!(secure.as_bool());

        let mut authority = BOOL(0);
        unsafe { scheme.HasAuthorityComponent(&mut authority) }.unwrap();
        assert!(authority.as_bool());

        let mut origin_count = 0u32;
        let mut raw_origins = ptr::null_mut();
        unsafe { scheme.GetAllowedOrigins(&mut origin_count, &mut raw_origins) }.unwrap();
        assert_eq!(origin_count, 1);
        let mut origin_pwstr = PWSTR(ptr::null_mut());
        unsafe { ptr::swap(&mut origin_pwstr, raw_origins) };
        let origin = take_pwstr(origin_pwstr);
        unsafe { CoTaskMemFree(Some(raw_origins.cast())) };
        assert_eq!(origin, "nyari://shell");
    }

    #[test]
    fn parse_shell_web_message_enforces_nyari_shell_origin() {
        assert!(is_allowed_shell_navigation("nyari://shell/?layer=chrome"));
        assert!(!is_allowed_shell_navigation("https://example.com/"));
        assert!(!is_allowed_shell_navigation("nyari://other/"));

        let ok = parse_shell_web_message(
            "nyari://shell/?layer=chrome",
            r#"{"type":"SET_LAYOUT","mode":"topbar"}"#,
        )
        .expect("valid nyari://shell/ message should parse");
        assert_eq!(
            ok,
            UIAction::SetLayout {
                mode: LayoutMode::Topbar
            }
        );

        assert!(
            parse_shell_web_message("https://evil.example.com/", r#"{"type":"WINDOW_CLOSE"}"#)
                .is_err()
        );
    }

    #[test]
    fn should_fire_shell_ready_gates_on_first_successful_nyari_shell_navigation() {
        let mut fired = false;
        assert!(!should_fire_shell_ready(
            &mut fired,
            false,
            "nyari://shell/?layer=chrome"
        ));
        assert!(!fired);

        assert!(!should_fire_shell_ready(
            &mut fired,
            true,
            "https://example.com/"
        ));
        assert!(!fired);

        assert!(should_fire_shell_ready(
            &mut fired,
            true,
            "nyari://shell/?layer=chrome"
        ));
        assert!(fired);

        assert!(!should_fire_shell_ready(
            &mut fired,
            true,
            "nyari://shell/?layer=chrome"
        ));
    }
}
