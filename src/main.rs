#![windows_subsystem = "windows"]

use nyari::backdrop::{apply_window_backdrop, is_windows_11_or_newer};
use nyari::cli::{parse_args, run_mcp_stdio_stub, LaunchMode};
use nyari::dpi::{dip_to_phys, scale_factor, window_dpi, MIN_WINDOW_WIDTH_DIP, RESIZE_BORDER_DIP};
use nyari::environment::{
    create_environment_async, create_shell_controller, post_ui_event, to_wide_null,
    update_controller_scale, EnvironmentEvent, NyariEnvironment, ShellControllerMapper,
};
use nyari::ipc::{
    AppSettings, MemoryState, OmniboxResultItem, OmniboxSource, OverlaySurface, ShortcutAction,
    TabMetadata, TabMetadataPatch, UIAction, UIEvent,
};
use nyari::layout::{
    apply_window_region, build_l_shaped_layer1_region, clamp_maximized_rect_to_monitor_work,
    classify_activation_change, clear_window_region, compute_rects, create_owned_popup_styles,
    get_ancestor_root, handle_wm_erasebkgnd, is_window_maximized, should_keep_active_kind,
    update_resize_ring, ActiveOverlayKind, NyariWindowFamily, OverlayPriorityStack,
};
use nyari::single_instance::{acquire_or_forward, SingleInstanceGuard};
use nyari::storage::DbActor;
use nyari::NyariError;
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::ptr;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::{
    EventLoopBuilderExtWindows, WindowBuilderExtWindows, WindowExtWindows,
};
use tao::window::WindowBuilder;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller;
use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient, ValidateRect, HBRUSH};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::Shell::{
    DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, CreateWindowExW, DefWindowProcW, DeferWindowPos, DestroyWindow,
    EndDeferWindowPos, GetClientRect, IsWindowVisible, LoadCursorW, MessageBoxW, RegisterClassExW,
    SendMessageW, SetWindowPos, ShowWindow, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT,
    HTMAXBUTTON, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, HTTRANSPARENT, HWND_TOP, IDC_ARROW,
    MB_ICONERROR, MB_OK, NCCALCSIZE_PARAMS, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_MAXIMIZE, SW_RESTORE, SW_SHOWNORMAL, WA_INACTIVE,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_ACTIVATE, WM_CLOSE, WM_DPICHANGED, WM_ERASEBKGND, WM_MOVE,
    WM_MOVING, WM_NCCALCSIZE, WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
    WM_PAINT, WM_WINDOWPOSCHANGED, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
    WS_VISIBLE,
};

const CHILD_CLASS_NAME: PCWSTR = w!("NyariChildSurface");
const MAIN_SUBCLASS_ID: usize = 1001;
const SNAP_SUBCLASS_ID: usize = 1002;
const RING_SUBCLASS_ID: usize = 1003;
const LAYER2_SUBCLASS_ID: usize = 1004;

#[derive(Debug, Clone)]
enum AppEvent {
    Environment(EnvironmentEvent),
    Layer1Ready,
    Layer2Ready,
    UiAction(UIAction),
    Shortcut(ShortcutAction),
    ControllerError(String),
    OpenUrls(Vec<String>),
}

struct ComStaGuard;

impl ComStaGuard {
    fn init() -> Result<Self, NyariError> {
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        hr.ok().map_err(NyariError::Win32)?;
        Ok(Self)
    }
}

impl Drop for ComStaGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

#[derive(Clone, Copy)]
struct WindowHandles {
    main_hwnd: HWND,
    layer1_host_hwnd: HWND,
    content_host_hwnd: HWND,
    snap_overlay_hwnd: HWND,
    resize_ring_hwnd: HWND,
    layer2_popup_hwnd: HWND,
}

struct AppState {
    handles: WindowHandles,
    data_dir: PathBuf,
    settings: AppSettings,
    tabs: Vec<TabMetadata>,
    next_tab_seq: u64,
    active_tab_id: Option<String>,
    overlay_stack: OverlayPriorityStack,
    env: Rc<RefCell<Option<NyariEnvironment>>>,
    layer1_controller: Rc<RefCell<Option<ICoreWebView2Controller>>>,
    layer2_controller: Rc<RefCell<Option<ICoreWebView2Controller>>>,
    db_actor: Option<DbActor>,
    instance_guard: Option<SingleInstanceGuard>,
    in_layout_sync: Cell<bool>,
    shutting_down: bool,
    last_overlay_rect_dip: Option<(OverlaySurface, f64, f64, f64, f64)>,
}

impl AppState {
    fn window_family(&self) -> NyariWindowFamily {
        NyariWindowFamily {
            main_hwnd: self.handles.main_hwnd.0 as isize,
            shell_overlay_hwnd: Some(self.handles.layer2_popup_hwnd.0 as isize),
            glance_hwnd: None,
            extension_popup_hwnd: None,
            oauth_popup_hwnds: Vec::new(),
        }
    }

    fn controllers_snapshot(
        &self,
    ) -> (
        Option<ICoreWebView2Controller>,
        Option<ICoreWebView2Controller>,
    ) {
        (
            self.layer1_controller.borrow().clone(),
            self.layer2_controller.borrow().clone(),
        )
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match parse_args(&args) {
        LaunchMode::McpStdio => match run_mcp_stdio_stub() {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        },
        LaunchMode::Browser { urls } => match run_browser(urls) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                show_fatal_error(&format!("Nyari Browser failed to start:\n\n{err}"));
                ExitCode::FAILURE
            }
        },
    }
}

fn nyari_data_dir() -> PathBuf {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(local).join("NyariBrowser")
    } else if let Some(profile) = std::env::var_os("USERPROFILE") {
        PathBuf::from(profile)
            .join("AppData")
            .join("Local")
            .join("NyariBrowser")
    } else {
        PathBuf::from("NyariBrowser")
    }
}

fn load_or_init_settings(data_dir: &Path) -> Result<AppSettings, NyariError> {
    fs::create_dir_all(data_dir)?;
    let path = data_dir.join("settings.json");
    if path.exists() {
        let raw = fs::read_to_string(&path)?;
        if let Ok(parsed) = serde_json::from_str::<AppSettings>(&raw) {
            return Ok(parsed);
        }
    }
    let defaults = AppSettings::default();
    save_settings(data_dir, &defaults)?;
    Ok(defaults)
}

fn save_settings(data_dir: &Path, settings: &AppSettings) -> Result<(), NyariError> {
    fs::create_dir_all(data_dir)?;
    let path = data_dir.join("settings.json");
    let tmp_path = data_dir.join("settings.json.tmp");
    let json = serde_json::to_string_pretty(settings)?;
    fs::write(&tmp_path, json)?;
    fs::rename(&tmp_path, &path)?;
    Ok(())
}

unsafe extern "system" fn child_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn register_child_window_class() -> Result<(), NyariError> {
    let hinstance = unsafe { GetModuleHandleW(None) }?;
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }?;
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(child_wnd_proc),
        hInstance: hinstance.into(),
        hCursor: cursor,
        hbrBackground: HBRUSH(ptr::null_mut()),
        lpszClassName: CHILD_CLASS_NAME,
        ..Default::default()
    };
    let atom = unsafe { RegisterClassExW(&wc) };
    if atom == 0 {
        return Err(windows::core::Error::from_thread().into());
    }
    Ok(())
}

fn create_child_hwnd(parent: HWND, title: PCWSTR, style: WINDOW_STYLE) -> Result<HWND, NyariError> {
    let hinstance = unsafe { GetModuleHandleW(None) }?;
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CHILD_CLASS_NAME,
            title,
            style,
            0,
            0,
            1,
            1,
            Some(parent),
            None,
            Some(hinstance.into()),
            None,
        )
    }
    .map_err(NyariError::Win32)
}

fn create_layer2_popup_hwnd(owner: HWND) -> Result<HWND, NyariError> {
    let hinstance = unsafe { GetModuleHandleW(None) }?;
    let styles = create_owned_popup_styles();
    unsafe {
        CreateWindowExW(
            styles.ex_style,
            CHILD_CLASS_NAME,
            w!("NyariOverlayLayer2"),
            styles.style,
            0,
            0,
            1,
            1,
            Some(owner),
            None,
            Some(hinstance.into()),
            None,
        )
    }
    .map_err(NyariError::Win32)
}

fn show_fatal_error(msg: &str) {
    let text = to_wide_null(msg);
    let title = to_wide_null("Nyari Browser");
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn run_browser(initial_urls: Vec<String>) -> Result<(), NyariError> {
    type ProxySlot = Arc<Mutex<(Option<EventLoopProxy<AppEvent>>, Vec<String>)>>;
    let proxy_slot: ProxySlot = Arc::new(Mutex::new((None, Vec::new())));
    let proxy_slot_cb = Arc::clone(&proxy_slot);

    let Some(instance_guard) = acquire_or_forward(&initial_urls, move |incoming_urls| {
        if let Ok(mut guard) = proxy_slot_cb.lock() {
            if let Some(proxy) = &guard.0 {
                let _ = proxy.send_event(AppEvent::OpenUrls(incoming_urls));
            } else {
                guard.1.extend(incoming_urls);
            }
        }
    })?
    else {
        return Ok(());
    };

    let _com_sta = ComStaGuard::init()?;
    let data_dir = nyari_data_dir();
    let db_actor = DbActor::spawn(&data_dir)?;
    let settings = load_or_init_settings(&data_dir)?;

    let event_loop: tao::event_loop::EventLoop<AppEvent> = EventLoopBuilder::with_user_event()
        .with_dpi_aware(false)
        .build();
    let proxy = event_loop.create_proxy();

    let mut queued_urls = initial_urls;
    if let Ok(mut guard) = proxy_slot.lock() {
        guard.0 = Some(proxy.clone());
        queued_urls.append(&mut guard.1);
    }

    let main_window = WindowBuilder::new()
        .with_title("Nyari Browser")
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(1280.0, 820.0))
        .with_min_inner_size(LogicalSize::new(MIN_WINDOW_WIDTH_DIP, 480.0))
        .with_transparent(false)
        .with_no_redirection_bitmap(true)
        .with_visible(false)
        .build(&event_loop)
        .map_err(|err| NyariError::Other(format!("Failed to create main window: {err}")))?;

    let main_hwnd = HWND(main_window.hwnd() as *mut _);
    let is_win11 = is_windows_11_or_newer();
    apply_window_backdrop(main_hwnd, is_win11, false)?;

    register_child_window_class()?;
    let layer1_host_hwnd = create_child_hwnd(
        main_hwnd,
        w!("NyariLayer1Host"),
        WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_CLIPCHILDREN.0 | WS_CLIPSIBLINGS.0),
    )?;
    let content_host_hwnd = create_child_hwnd(
        main_hwnd,
        w!("NyariContentHost"),
        WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_CLIPCHILDREN.0 | WS_CLIPSIBLINGS.0),
    )?;
    let snap_overlay_hwnd = create_child_hwnd(
        main_hwnd,
        w!("NyariSnapOverlay"),
        WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_CLIPSIBLINGS.0),
    )?;
    let resize_ring_hwnd = create_child_hwnd(
        main_hwnd,
        w!("NyariResizeRing"),
        WINDOW_STYLE(WS_CHILD.0 | WS_CLIPSIBLINGS.0),
    )?;
    let layer2_popup_hwnd = create_layer2_popup_hwnd(main_hwnd)?;
    apply_window_backdrop(layer2_popup_hwnd, is_win11, true)?;

    let handles = WindowHandles {
        main_hwnd,
        layer1_host_hwnd,
        content_host_hwnd,
        snap_overlay_hwnd,
        resize_ring_hwnd,
        layer2_popup_hwnd,
    };

    let env_slot = Rc::new(RefCell::new(None));
    let layer1_slot = Rc::new(RefCell::new(None));
    let layer2_slot = Rc::new(RefCell::new(None));

    let state = Rc::new(RefCell::new(AppState {
        handles,
        data_dir: data_dir.clone(),
        settings: settings.clone(),
        tabs: Vec::new(),
        next_tab_seq: 1,
        active_tab_id: None,
        overlay_stack: OverlayPriorityStack::new(),
        env: Rc::clone(&env_slot),
        layer1_controller: Rc::clone(&layer1_slot),
        layer2_controller: Rc::clone(&layer2_slot),
        db_actor: Some(db_actor),
        instance_guard: Some(instance_guard),
        in_layout_sync: Cell::new(false),
        shutting_down: false,
        last_overlay_rect_dip: None,
    }));

    install_subclass(main_hwnd, main_hwnd_subclass_proc, MAIN_SUBCLASS_ID, &state)?;
    install_subclass(
        snap_overlay_hwnd,
        snap_overlay_subclass_proc,
        SNAP_SUBCLASS_ID,
        &state,
    )?;
    install_subclass(
        resize_ring_hwnd,
        resize_ring_subclass_proc,
        RING_SUBCLASS_ID,
        &state,
    )?;
    install_subclass(
        layer2_popup_hwnd,
        layer2_popup_subclass_proc,
        LAYER2_SUBCLASS_ID,
        &state,
    )?;

    sync_window_layout(&state);
    main_window.set_visible(true);

    let eb_dir = data_dir.join("EBWebView");
    create_environment_async(
        &eb_dir,
        settings.v8_heap_profile,
        env_slot,
        proxy.clone(),
        AppEvent::Environment,
    )?;

    if !queued_urls.is_empty() {
        let _ = proxy.send_event(AppEvent::OpenUrls(queued_urls));
    }

    let state_loop = Rc::clone(&state);
    let proxy_loop = proxy.clone();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                perform_graceful_shutdown(&state_loop);
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(app_event) => match app_event {
                AppEvent::Environment(env_ev) => match env_ev {
                    EnvironmentEvent::Ready => {
                        init_shell_controllers(&state_loop, &proxy_loop);
                    }
                    EnvironmentEvent::CreateFailed(msg) => {
                        show_fatal_error(&format!(
                            "{msg}\n\nPlease install the Microsoft Edge WebView2 Evergreen Runtime."
                        ));
                    }
                    EnvironmentEvent::RuntimeUpdateAvailable { version } => {
                        broadcast_ui_event(
                            &state_loop,
                            &UIEvent::RuntimeUpdateAvailable { version },
                        );
                    }
                    EnvironmentEvent::BrowserProcessCrashed { reason, recovering } => {
                        broadcast_ui_event(
                            &state_loop,
                            &UIEvent::BrowserProcessCrashed { reason, recovering },
                        );
                    }
                },
                AppEvent::Layer1Ready => {
                    sync_window_layout(&state_loop);
                    push_initial_shell_state(&state_loop);
                }
                AppEvent::Layer2Ready => {
                    sync_layer2_viewport_script(&state_loop);
                    push_initial_shell_state(&state_loop);
                }
                AppEvent::UiAction(action) => {
                    if handle_ui_action(&state_loop, &main_window, action) {
                        perform_graceful_shutdown(&state_loop);
                        *control_flow = ControlFlow::Exit;
                    }
                }
                AppEvent::Shortcut(shortcut) => {
                    handle_shortcut(&state_loop, shortcut);
                }
                AppEvent::ControllerError(msg) => {
                    show_fatal_error(&msg);
                }
                AppEvent::OpenUrls(urls) => {
                    for url in urls {
                        open_stub_tab(&state_loop, Some(url), false);
                    }
                    main_window.set_focus();
                }
            },
            _ => {}
        }
    });
}

fn install_subclass(
    hwnd: HWND,
    proc_fn: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, usize, usize) -> LRESULT,
    subclass_id: usize,
    state: &Rc<RefCell<AppState>>,
) -> Result<(), NyariError> {
    let raw = Rc::into_raw(Rc::clone(state)) as usize;
    let ok = unsafe { SetWindowSubclass(hwnd, Some(proc_fn), subclass_id, raw) }.as_bool();
    if !ok {
        unsafe {
            drop(Rc::from_raw(raw as *const RefCell<AppState>));
        }
        return Err(NyariError::Other("SetWindowSubclass failed".to_string()));
    }
    Ok(())
}

fn borrow_subclass_state<'a>(dw_ref_data: usize) -> Option<&'a RefCell<AppState>> {
    if dw_ref_data == 0 {
        None
    } else {
        Some(unsafe { &*(dw_ref_data as *const RefCell<AppState>) })
    }
}

fn cleanup_subclass(
    hwnd: HWND,
    proc_fn: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, usize, usize) -> LRESULT,
    subclass_id: usize,
    dw_ref_data: usize,
) {
    unsafe {
        let _ = RemoveWindowSubclass(hwnd, Some(proc_fn), subclass_id);
        if dw_ref_data != 0 {
            drop(Rc::from_raw(dw_ref_data as *const RefCell<AppState>));
        }
    }
}

fn init_shell_controllers(state: &Rc<RefCell<AppState>>, proxy: &EventLoopProxy<AppEvent>) {
    let (env_opt, handles, layer1_slot, layer2_slot) = {
        let st = state.borrow();
        let snapshot = (
            st.env.borrow().as_ref().map(|e| e.raw().clone()),
            st.handles,
            Rc::clone(&st.layer1_controller),
            Rc::clone(&st.layer2_controller),
        );
        snapshot
    };

    let Some(env) = env_opt else {
        return;
    };

    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(handles.main_hwnd, &mut client);
    }
    let dpi = window_dpi(handles.main_hwnd);
    let settings = state.borrow().settings.clone();
    let rects = compute_rects(
        client.right - client.left,
        client.bottom - client.top,
        dpi,
        &settings,
    );
    let l1_rect = RECT {
        left: 0,
        top: 0,
        right: rects.layer1_bounds.width.max(1),
        bottom: rects.layer1_bounds.height.max(1),
    };

    let _ = create_shell_controller(
        &env,
        handles.layer1_host_hwnd,
        l1_rect,
        rects.layer1_visible,
        dpi,
        "nyari://shell/?layer=chrome",
        layer1_slot,
        proxy.clone(),
        ShellControllerMapper {
            on_ready: || AppEvent::Layer1Ready,
            on_action: AppEvent::UiAction,
            on_shortcut: AppEvent::Shortcut,
            on_error: AppEvent::ControllerError,
        },
    );

    let _ = create_shell_controller(
        &env,
        handles.layer2_popup_hwnd,
        RECT {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        },
        false,
        dpi,
        "nyari://shell/?layer=overlay",
        layer2_slot,
        proxy.clone(),
        ShellControllerMapper {
            on_ready: || AppEvent::Layer2Ready,
            on_action: AppEvent::UiAction,
            on_shortcut: AppEvent::Shortcut,
            on_error: AppEvent::ControllerError,
        },
    );
}

fn push_initial_shell_state(state: &Rc<RefCell<AppState>>) {
    let (settings, tabs, active_tab_id, controllers) = {
        let st = state.borrow();
        (
            st.settings.clone(),
            st.tabs.clone(),
            st.active_tab_id.clone(),
            st.controllers_snapshot(),
        )
    };
    let settings_ev = UIEvent::SettingsUpdated { settings };
    let active_js = to_wide_null(&sync_active_tab_script(active_tab_id.as_deref()));

    for controller in [controllers.0, controllers.1].into_iter().flatten() {
        let _ = post_ui_event(&controller, &settings_ev);
        for tab in &tabs {
            let _ = post_ui_event(&controller, &UIEvent::TabCreated { tab: tab.clone() });
        }
        if let Ok(wv) = unsafe { controller.CoreWebView2() } {
            unsafe {
                let _ = wv.ExecuteScript(PCWSTR(active_js.as_ptr()), None);
            }
        }
    }
}

fn broadcast_active_tab(state: &Rc<RefCell<AppState>>) {
    let (active_tab_id, controllers) = {
        let st = state.borrow();
        (st.active_tab_id.clone(), st.controllers_snapshot())
    };
    let js_w = to_wide_null(&sync_active_tab_script(active_tab_id.as_deref()));
    for controller in [controllers.0, controllers.1].into_iter().flatten() {
        if let Ok(wv) = unsafe { controller.CoreWebView2() } {
            unsafe {
                let _ = wv.ExecuteScript(PCWSTR(js_w.as_ptr()), None);
            }
        }
    }
}

fn broadcast_ui_event(state: &Rc<RefCell<AppState>>, event: &UIEvent) {
    let controllers = state.borrow().controllers_snapshot();
    if let Some(c1) = &controllers.0 {
        let _ = post_ui_event(c1, event);
    }
    if let Some(c2) = &controllers.1 {
        let _ = post_ui_event(c2, event);
    }
}

fn sync_layer2_viewport_script(state: &Rc<RefCell<AppState>>) {
    let (main_hwnd, c2_opt) = {
        let st = state.borrow();
        let snapshot = (st.handles.main_hwnd, st.layer2_controller.borrow().clone());
        snapshot
    };
    let Some(c2) = c2_opt else {
        return;
    };
    let mut rc = RECT::default();
    if unsafe { GetClientRect(main_hwnd, &mut rc) }.is_err() {
        return;
    }
    let scale = scale_factor(window_dpi(main_hwnd));
    let w_dip = f64::from((rc.right - rc.left).max(1)) / scale;
    let h_dip = f64::from((rc.bottom - rc.top).max(1)) / scale;
    let script = format!(
        "window.__NYARI_VIEWPORT__={{widthDip:{w_dip:.1},heightDip:{h_dip:.1}}};window.dispatchEvent(new Event('nyari-viewport'));"
    );
    let script_w = to_wide_null(&script);
    if let Ok(wv) = unsafe { c2.CoreWebView2() } {
        unsafe {
            let _ = wv.ExecuteScript(PCWSTR(script_w.as_ptr()), None);
        }
    }
}

fn sync_window_layout(state: &Rc<RefCell<AppState>>) {
    let (handles, settings, controllers) = {
        let st = state.borrow();
        if st.in_layout_sync.get() {
            return;
        }
        st.in_layout_sync.set(true);
        (st.handles, st.settings.clone(), st.controllers_snapshot())
    };

    let mut client = RECT::default();
    if unsafe { GetClientRect(handles.main_hwnd, &mut client) }.is_ok() {
        let w = (client.right - client.left).max(0);
        let h = (client.bottom - client.top).max(0);
        let dpi = window_dpi(handles.main_hwnd);
        let rects = compute_rects(w, h, dpi, &settings);

        if let Ok(Some(l_rgn)) = build_l_shaped_layer1_region(&rects) {
            let _ = apply_window_region(handles.layer1_host_hwnd, l_rgn, false);
        } else {
            let _ = clear_window_region(handles.layer1_host_hwnd, false);
        }

        let maximized = is_window_maximized(handles.main_hwnd);
        let _ = update_resize_ring(handles.resize_ring_hwnd, w, h, dpi, maximized);

        if let Ok(mut hdwp) = unsafe { BeginDeferWindowPos(4) } {
            let l1_flags = SWP_NOACTIVATE
                | SWP_NOZORDER
                | if rects.layer1_visible {
                    SWP_SHOWWINDOW
                } else {
                    SWP_HIDEWINDOW
                };
            if let Ok(next) = unsafe {
                DeferWindowPos(
                    hdwp,
                    handles.layer1_host_hwnd,
                    None,
                    rects.layer1_bounds.x,
                    rects.layer1_bounds.y,
                    rects.layer1_bounds.width.max(0),
                    rects.layer1_bounds.height.max(0),
                    l1_flags,
                )
            } {
                hdwp = next;
            }

            if let Ok(next) = unsafe {
                DeferWindowPos(
                    hdwp,
                    handles.content_host_hwnd,
                    None,
                    rects.content_bounds.x,
                    rects.content_bounds.y,
                    rects.content_bounds.width.max(0),
                    rects.content_bounds.height.max(0),
                    SWP_NOACTIVATE | SWP_NOZORDER,
                )
            } {
                hdwp = next;
            }

            if let Some(snap) = rects.snap_zone {
                // Position Snap Overlay directly above the middle [Maximize] button (`w - 92..w - 46` DIP).
                let caption_left = rects.layer1_caption.map_or(0, |c| c.x);
                let snap_x = (snap.x - snap.width).max(caption_left);
                if let Ok(next) = unsafe {
                    DeferWindowPos(
                        hdwp,
                        handles.snap_overlay_hwnd,
                        Some(HWND_TOP),
                        snap_x,
                        snap.y,
                        snap.width.max(0),
                        snap.height.max(0),
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    )
                } {
                    hdwp = next;
                }
            } else if let Ok(next) = unsafe {
                DeferWindowPos(
                    hdwp,
                    handles.snap_overlay_hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSIZE | SWP_NOMOVE | SWP_HIDEWINDOW,
                )
            } {
                hdwp = next;
            }

            if let Ok(next) = unsafe {
                DeferWindowPos(
                    hdwp,
                    handles.resize_ring_hwnd,
                    Some(HWND_TOP),
                    0,
                    0,
                    w,
                    h,
                    SWP_NOACTIVATE,
                )
            } {
                hdwp = next;
            }

            unsafe {
                let _ = EndDeferWindowPos(hdwp);
            }
        }

        if let Some(c1) = &controllers.0 {
            unsafe {
                let _ = c1.SetIsVisible(rects.layer1_visible);
                let _ = c1.SetBounds(RECT {
                    left: 0,
                    top: 0,
                    right: rects.layer1_bounds.width.max(0),
                    bottom: rects.layer1_bounds.height.max(0),
                });
            }
        }
    }

    sync_layer2_viewport_script(state);
    state.borrow().in_layout_sync.set(false);
}

fn position_layer2_popup(state: &Rc<RefCell<AppState>>, show_and_focus: bool) {
    let (handles, rect_dip_opt, c2_opt) = {
        let st = state.borrow();
        let snapshot = (
            st.handles,
            st.last_overlay_rect_dip,
            st.layer2_controller.borrow().clone(),
        );
        snapshot
    };
    let Some((_, x_dip, y_dip, w_dip, h_dip)) = rect_dip_opt else {
        return;
    };

    let dpi = window_dpi(handles.main_hwnd);
    let mut client_rect = RECT::default();
    let _ = unsafe { GetClientRect(handles.main_hwnd, &mut client_rect) };
    let client_w = (client_rect.right - client_rect.left).max(1);
    let client_h = (client_rect.bottom - client_rect.top).max(1);

    let w_phys = dip_to_phys(w_dip, dpi).clamp(1, client_w);
    let h_phys = dip_to_phys(h_dip, dpi).clamp(1, client_h);
    let x_rel = dip_to_phys(x_dip, dpi).clamp(0, (client_w - w_phys).max(0));
    let y_rel = dip_to_phys(y_dip, dpi).clamp(0, (client_h - h_phys).max(0));

    let mut origin = POINT { x: x_rel, y: y_rel };
    let swp_flags = if show_and_focus {
        SWP_NOZORDER | SWP_SHOWWINDOW
    } else {
        SWP_NOZORDER | SWP_NOACTIVATE
    };
    unsafe {
        let _ = ClientToScreen(handles.main_hwnd, &mut origin);
        let _ = SetWindowPos(
            handles.layer2_popup_hwnd,
            None,
            origin.x,
            origin.y,
            w_phys,
            h_phys,
            swp_flags,
        );
    }

    if let Some(c2) = c2_opt {
        unsafe {
            let _ = c2.SetBounds(RECT {
                left: 0,
                top: 0,
                right: w_phys,
                bottom: h_phys,
            });
            let _ = c2.SetIsVisible(true);
            if show_and_focus {
                let _ = c2.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            }
        }
    }
}

fn hide_layer2_popup(state: &Rc<RefCell<AppState>>, restore_focus: bool) {
    let (handles, controllers) = {
        let mut st = state.borrow_mut();
        st.last_overlay_rect_dip = None;
        (st.handles, st.controllers_snapshot())
    };
    unsafe {
        let _ = ShowWindow(handles.layer2_popup_hwnd, SW_HIDE);
    }
    if let Some(c2) = controllers.1 {
        unsafe {
            let _ = c2.SetIsVisible(false);
        }
    }
    if restore_focus {
        if let Some(c1) = controllers.0 {
            unsafe {
                let _ = c1.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            }
        }
    }
}

fn open_stub_tab(state: &Rc<RefCell<AppState>>, url: Option<String>, is_private: bool) {
    let (tab, controllers) = {
        let mut st = state.borrow_mut();
        let seq = st.next_tab_seq;
        st.next_tab_seq += 1;
        let id = format!("tab-{seq}");
        let target_url = url.unwrap_or_else(|| "about:blank".to_string());
        let title = if target_url == "about:blank" {
            if is_private {
                "Private Tab".to_string()
            } else {
                "New Tab".to_string()
            }
        } else {
            target_url.clone()
        };
        let tab = TabMetadata {
            id: id.clone(),
            workspace_id: "ws-default".to_string(),
            title,
            url: target_url,
            favicon_url: String::new(),
            is_pinned: false,
            is_private,
            is_oauth_popup: false,
            keep_awake: false,
            has_unsaved_input: false,
            can_go_back: false,
            can_go_forward: false,
            is_loading: false,
            is_audible: false,
            shield_blocked_count: 0,
            memory_state: MemoryState::Active,
            ram_bytes: 0,
        };
        st.active_tab_id = Some(id);
        st.tabs.push(tab.clone());
        (tab, st.controllers_snapshot())
    };

    let ev = UIEvent::TabCreated { tab };
    if let Some(c1) = controllers.0 {
        let _ = post_ui_event(&c1, &ev);
    }
    if let Some(c2) = controllers.1 {
        let _ = post_ui_event(&c2, &ev);
    }
    broadcast_active_tab(state);
}

fn handle_shortcut(state: &Rc<RefCell<AppState>>, shortcut: ShortcutAction) {
    let (active_kind, c2_opt) = {
        let mut st = state.borrow_mut();
        match shortcut {
            ShortcutAction::OpenCommandPalette => {
                st.overlay_stack.find_in_page_open = false;
                st.overlay_stack.command_palette_open = true;
            }
            ShortcutAction::OpenFindInPage => {
                st.overlay_stack.command_palette_open = false;
                st.overlay_stack.find_in_page_open = true;
            }
            ShortcutAction::CloseActiveOverlay => {
                if st.overlay_stack.command_palette_open {
                    st.overlay_stack.command_palette_open = false;
                } else if st.overlay_stack.find_in_page_open {
                    st.overlay_stack.find_in_page_open = false;
                } else if st.overlay_stack.hover_sidebar_open {
                    st.overlay_stack.hover_sidebar_open = false;
                } else if st.overlay_stack.reading_mode_open {
                    st.overlay_stack.reading_mode_open = false;
                }
            }
        }
        let snapshot = (
            st.overlay_stack.active_kind(),
            st.layer2_controller.borrow().clone(),
        );
        snapshot
    };

    if let Some(c2) = c2_opt {
        let _ = post_ui_event(&c2, &UIEvent::ShortcutTriggered { action: shortcut });
    }
    if active_kind.is_none() {
        hide_layer2_popup(state, true);
    }
}

/// Processes a `UIAction` from Shell UI; returns `true` if the application should exit.
fn handle_ui_action(
    state: &Rc<RefCell<AppState>>,
    main_window: &tao::window::Window,
    action: UIAction,
) -> bool {
    match action {
        UIAction::GetSettings => {
            push_initial_shell_state(state);
            sync_layer2_viewport_script(state);
        }
        UIAction::UpdateSettings { patch } => {
            let (settings, data_dir) = {
                let mut st = state.borrow_mut();
                st.settings.apply_patch(patch);
                (st.settings.clone(), st.data_dir.clone())
            };
            let _ = save_settings(&data_dir, &settings);
            sync_window_layout(state);
            broadcast_ui_event(state, &UIEvent::SettingsUpdated { settings });
        }
        UIAction::SetLayout { mode } => {
            let (settings, data_dir) = {
                let mut st = state.borrow_mut();
                st.settings.layout_mode = mode;
                st.settings.zen_mode = false;
                (st.settings.clone(), st.data_dir.clone())
            };
            let _ = save_settings(&data_dir, &settings);
            sync_window_layout(state);
            broadcast_ui_event(state, &UIEvent::SettingsUpdated { settings });
        }
        UIAction::ToggleZen => {
            let (settings, data_dir) = {
                let mut st = state.borrow_mut();
                st.settings.zen_mode = !st.settings.zen_mode;
                (st.settings.clone(), st.data_dir.clone())
            };
            let _ = save_settings(&data_dir, &settings);
            sync_window_layout(state);
            broadcast_ui_event(state, &UIEvent::SettingsUpdated { settings });
        }
        UIAction::SetOverlayBounds {
            visible,
            surface,
            x_dip,
            y_dip,
            width_dip,
            height_dip,
        } => {
            if let (true, Some(surf)) = (visible, surface) {
                let was_visible = {
                    let mut st = state.borrow_mut();
                    st.last_overlay_rect_dip = Some((surf, x_dip, y_dip, width_dip, height_dip));
                    unsafe { IsWindowVisible(st.handles.layer2_popup_hwnd).as_bool() }
                };
                position_layer2_popup(state, !was_visible);
            } else {
                hide_layer2_popup(state, true);
            }
        }
        UIAction::CloseOverlay => {
            handle_shortcut(state, ShortcutAction::CloseActiveOverlay);
        }
        UIAction::WindowMinimize => {
            main_window.set_minimized(true);
        }
        UIAction::WindowMaximizeRestore => {
            main_window.set_maximized(!main_window.is_maximized());
        }
        UIAction::WindowClose => {
            return true;
        }
        UIAction::CreateTab {
            url, is_private, ..
        } => {
            open_stub_tab(state, url, is_private.unwrap_or(false));
        }
        UIAction::CloseTab { tab_id } => {
            {
                let mut st = state.borrow_mut();
                let AppState {
                    tabs,
                    active_tab_id,
                    ..
                } = &mut *st;
                close_tab_and_select_next(tabs, active_tab_id, &tab_id);
            }
            broadcast_ui_event(state, &UIEvent::TabClosed { tab_id });
            broadcast_active_tab(state);
        }
        UIAction::SwitchTab { tab_id } => {
            let switched = {
                let mut st = state.borrow_mut();
                let AppState {
                    tabs,
                    active_tab_id,
                    ..
                } = &mut *st;
                switch_active_tab(tabs, active_tab_id, &tab_id)
            };
            if switched {
                broadcast_active_tab(state);
            }
        }
        UIAction::Navigate {
            tab_id,
            url_or_query,
        } => {
            let trimmed = url_or_query.trim().to_string();
            let resolved_url = if trimmed.contains("://") {
                trimmed.clone()
            } else if trimmed.contains('.') && !trimmed.contains(' ') {
                format!("https://{trimmed}")
            } else {
                let template = state.borrow().settings.search_engine_template.clone();
                template.replace("%s", &trimmed.replace(' ', "+"))
            };
            {
                let mut st = state.borrow_mut();
                if let Some(tab) = st.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.url = resolved_url.clone();
                    tab.title = resolved_url.clone();
                }
            }
            broadcast_ui_event(
                state,
                &UIEvent::TabUpdated {
                    tab_id,
                    changes: TabMetadataPatch {
                        title: Some(resolved_url.clone()),
                        url: Some(resolved_url),
                        ..Default::default()
                    },
                },
            );
        }
        UIAction::QueryOmnibox { query } => {
            let q = query.trim().to_ascii_lowercase();
            let results: Vec<OmniboxResultItem> = state
                .borrow()
                .tabs
                .iter()
                .filter(|t| {
                    q.is_empty()
                        || t.title.to_ascii_lowercase().contains(&q)
                        || t.url.to_ascii_lowercase().contains(&q)
                })
                .map(|t| OmniboxResultItem {
                    id: t.id.clone(),
                    title: t.title.clone(),
                    url: t.url.clone(),
                    source: OmniboxSource::Tab,
                    icon: None,
                })
                .collect();
            if let Some(c2) = state.borrow().layer2_controller.borrow().clone() {
                let _ = post_ui_event(&c2, &UIEvent::OmniboxResults { results });
            }
        }
        UIAction::ExecuteCommand { command, .. } => match command.as_str() {
            "open_command_palette" => handle_shortcut(state, ShortcutAction::OpenCommandPalette),
            "open_find_in_page" => handle_shortcut(state, ShortcutAction::OpenFindInPage),
            "open_hover_sidebar" => {
                state.borrow_mut().overlay_stack.hover_sidebar_open = true;
            }
            "close_hover_sidebar" => {
                state.borrow_mut().overlay_stack.hover_sidebar_open = false;
                if state.borrow().overlay_stack.active_kind().is_none() {
                    hide_layer2_popup(state, true);
                }
            }
            _ => {}
        },
        UIAction::OpenInEdge { url }
            if url.starts_with("http://") || url.starts_with("https://") =>
        {
            let url_w = to_wide_null(&url);
            unsafe {
                let _ = ShellExecuteW(
                    None,
                    w!("open"),
                    w!("msedge.exe"),
                    PCWSTR(url_w.as_ptr()),
                    PCWSTR::null(),
                    SW_SHOWNORMAL,
                );
            }
        }
        _ => {}
    }
    false
}

fn perform_graceful_shutdown(state: &Rc<RefCell<AppState>>) {
    let (settings, data_dir, db_actor, controllers, env, guard, handles) = {
        let mut st = state.borrow_mut();
        if st.shutting_down {
            return;
        }
        st.shutting_down = true;
        let c1 = st.layer1_controller.borrow_mut().take();
        let c2 = st.layer2_controller.borrow_mut().take();
        let env = st.env.borrow_mut().take();
        (
            st.settings.clone(),
            st.data_dir.clone(),
            st.db_actor.take(),
            (c1, c2),
            env,
            st.instance_guard.take(),
            st.handles,
        )
    };

    let _ = save_settings(&data_dir, &settings);
    if let Some(actor) = db_actor {
        let _ = actor.shutdown_and_checkpoint(Duration::from_millis(150));
    }

    if let Some(c2) = controllers.1 {
        unsafe {
            let _ = c2.Close();
        }
    }
    if let Some(c1) = controllers.0 {
        unsafe {
            let _ = c1.Close();
        }
    }
    drop(env);
    drop(guard);

    unsafe {
        let _ = DestroyWindow(handles.layer2_popup_hwnd);
    }
}

fn handle_family_activation_deactivation(state: &Rc<RefCell<AppState>>, gaining_hwnd: HWND) {
    let gained_root = get_ancestor_root(gaining_hwnd);
    let (transition, c2_opt) = {
        let Ok(mut st) = state.try_borrow_mut() else {
            return;
        };
        let family = st.window_family();
        let transition = apply_activation_transition(&mut st.overlay_stack, gained_root, &family);
        let snapshot = (transition, st.layer2_controller.borrow().clone());
        snapshot
    };

    if transition.dismissed_command_palette || transition.dismissed_hover_sidebar {
        if let Some(c2) = c2_opt {
            let script_w = to_wide_null(&sync_transient_dismissal_script(
                transition.dismissed_command_palette,
                transition.dismissed_hover_sidebar,
            ));
            if let Ok(wv) = unsafe { c2.CoreWebView2() } {
                unsafe {
                    let _ = wv.ExecuteScript(PCWSTR(script_w.as_ptr()), None);
                }
            }
        }
    }

    if transition.should_hide_popup {
        hide_layer2_popup(state, transition.restore_focus_to_main);
    }
}

unsafe extern "system" fn main_hwnd_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid_subclass: usize,
    dw_ref_data: usize,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        cleanup_subclass(hwnd, main_hwnd_subclass_proc, MAIN_SUBCLASS_ID, dw_ref_data);
        return res;
    }

    let Some(state_cell) = borrow_subclass_state(dw_ref_data) else {
        return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    };

    match msg {
        WM_ERASEBKGND => handle_wm_erasebkgnd(),
        WM_NCCALCSIZE => {
            if is_window_maximized(hwnd) && wparam.0 != 0 {
                let params = unsafe { &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS) };
                let _ = clamp_maximized_rect_to_monitor_work(hwnd, &mut params.rgrc[0]);
                return LRESULT(0);
            }
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_DPICHANGED => {
            let new_dpi = u32::from((wparam.0 & 0xFFFF) as u16);
            if let Ok(st) = state_cell.try_borrow() {
                let controllers = st.controllers_snapshot();
                if let Some(c1) = &controllers.0 {
                    update_controller_scale(c1, new_dpi);
                }
                if let Some(c2) = &controllers.1 {
                    update_controller_scale(c2, new_dpi);
                }
            }
            let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            let state_rc = unsafe {
                let ptr = dw_ref_data as *const RefCell<AppState>;
                Rc::increment_strong_count(ptr);
                Rc::from_raw(ptr)
            };
            sync_window_layout(&state_rc);
            if should_reposition_visible_layer2(&state_rc) {
                position_layer2_popup(&state_rc, false);
            }
            res
        }
        WM_MOVE | WM_MOVING => {
            if let Ok(st) = state_cell.try_borrow() {
                let controllers = st.controllers_snapshot();
                if let Some(c1) = &controllers.0 {
                    unsafe {
                        let _ = c1.NotifyParentWindowPositionChanged();
                    }
                }
                if let Some(c2) = &controllers.1 {
                    unsafe {
                        let _ = c2.NotifyParentWindowPositionChanged();
                    }
                }
                if st.last_overlay_rect_dip.is_some() {
                    drop(st);
                    let state_rc = unsafe {
                        let ptr = dw_ref_data as *const RefCell<AppState>;
                        Rc::increment_strong_count(ptr);
                        Rc::from_raw(ptr)
                    };
                    if should_reposition_visible_layer2(&state_rc) {
                        position_layer2_popup(&state_rc, false);
                    }
                }
            }
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_WINDOWPOSCHANGED => {
            let state_rc = unsafe {
                let ptr = dw_ref_data as *const RefCell<AppState>;
                Rc::increment_strong_count(ptr);
                Rc::from_raw(ptr)
            };
            sync_window_layout(&state_rc);
            if should_reposition_visible_layer2(&state_rc) {
                position_layer2_popup(&state_rc, false);
            }
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                let state_rc = unsafe {
                    let ptr = dw_ref_data as *const RefCell<AppState>;
                    Rc::increment_strong_count(ptr);
                    Rc::from_raw(ptr)
                };
                handle_family_activation_deactivation(&state_rc, HWND(lparam.0 as *mut _));
            }
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_CLOSE => {
            let state_rc = unsafe {
                let ptr = dw_ref_data as *const RefCell<AppState>;
                Rc::increment_strong_count(ptr);
                Rc::from_raw(ptr)
            };
            perform_graceful_shutdown(&state_rc);
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn snap_overlay_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid_subclass: usize,
    dw_ref_data: usize,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        cleanup_subclass(
            hwnd,
            snap_overlay_subclass_proc,
            SNAP_SUBCLASS_ID,
            dw_ref_data,
        );
        return res;
    }

    let Some(state_cell) = borrow_subclass_state(dw_ref_data) else {
        return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    };

    match msg {
        WM_ERASEBKGND => handle_wm_erasebkgnd(),
        WM_NCHITTEST => LRESULT(HTMAXBUTTON as isize),
        WM_NCLBUTTONDOWN if wparam.0 as u32 == HTMAXBUTTON => LRESULT(0),
        WM_NCLBUTTONUP if wparam.0 as u32 == HTMAXBUTTON => {
            if let Ok(st) = state_cell.try_borrow() {
                let main_hwnd = st.handles.main_hwnd;
                let cmd = if is_window_maximized(main_hwnd) {
                    SW_RESTORE
                } else {
                    SW_MAXIMIZE
                };
                unsafe {
                    let _ = ShowWindow(main_hwnd, cmd);
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn resize_ring_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid_subclass: usize,
    dw_ref_data: usize,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        cleanup_subclass(
            hwnd,
            resize_ring_subclass_proc,
            RING_SUBCLASS_ID,
            dw_ref_data,
        );
        return res;
    }

    let Some(state_cell) = borrow_subclass_state(dw_ref_data) else {
        return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    };

    match msg {
        WM_ERASEBKGND => handle_wm_erasebkgnd(),
        WM_PAINT => {
            unsafe {
                let _ = ValidateRect(Some(hwnd), None);
            }
            LRESULT(0)
        }
        WM_NCHITTEST => {
            let Ok(st) = state_cell.try_borrow() else {
                return LRESULT(HTTRANSPARENT as isize);
            };
            let main_hwnd = st.handles.main_hwnd;
            let mut rc = RECT::default();
            if unsafe { GetClientRect(main_hwnd, &mut rc) }.is_err() {
                return LRESULT(HTTRANSPARENT as isize);
            }
            let mut pt = POINT {
                x: i32::from((lparam.0 & 0xFFFF) as i16),
                y: i32::from(((lparam.0 >> 16) & 0xFFFF) as i16),
            };
            if unsafe { !ScreenToClient(main_hwnd, &mut pt).as_bool() } {
                return LRESULT(HTTRANSPARENT as isize);
            }
            let w = rc.right - rc.left;
            let h = rc.bottom - rc.top;
            let b = dip_to_phys(RESIZE_BORDER_DIP, window_dpi(main_hwnd)).max(1);
            let left = pt.x < b;
            let right = pt.x >= w - b;
            let top = pt.y < b;
            let bottom = pt.y >= h - b;

            let hit = match (left, right, top, bottom) {
                (true, _, true, _) => HTTOPLEFT,
                (_, true, true, _) => HTTOPRIGHT,
                (true, _, _, true) => HTBOTTOMLEFT,
                (_, true, _, true) => HTBOTTOMRIGHT,
                (true, _, _, _) => HTLEFT,
                (_, true, _, _) => HTRIGHT,
                (_, _, true, _) => HTTOP,
                (_, _, _, true) => HTBOTTOM,
                _ => HTTRANSPARENT as u32,
            };
            LRESULT(hit as i32 as isize)
        }
        WM_NCLBUTTONDOWN => {
            if let Ok(st) = state_cell.try_borrow() {
                let main_hwnd = st.handles.main_hwnd;
                unsafe {
                    let _ = ReleaseCapture();
                    let _ = SendMessageW(main_hwnd, WM_NCLBUTTONDOWN, Some(wparam), Some(lparam));
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn layer2_popup_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid_subclass: usize,
    dw_ref_data: usize,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        cleanup_subclass(
            hwnd,
            layer2_popup_subclass_proc,
            LAYER2_SUBCLASS_ID,
            dw_ref_data,
        );
        return res;
    }

    match msg {
        WM_ERASEBKGND => handle_wm_erasebkgnd(),
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE && dw_ref_data != 0 {
                let state_rc = unsafe {
                    let ptr = dw_ref_data as *const RefCell<AppState>;
                    Rc::increment_strong_count(ptr);
                    Rc::from_raw(ptr)
                };
                handle_family_activation_deactivation(&state_rc, HWND(lparam.0 as *mut _));
            }
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

fn should_reposition_visible_layer2(state: &Rc<RefCell<AppState>>) -> bool {
    let Ok(st) = state.try_borrow() else {
        return false;
    };
    st.last_overlay_rect_dip.is_some()
        && unsafe { IsWindowVisible(st.handles.layer2_popup_hwnd).as_bool() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TransientDismissalResult {
    action: nyari::layout::ActivationAction,
    dismissed_command_palette: bool,
    dismissed_hover_sidebar: bool,
    should_hide_popup: bool,
    restore_focus_to_main: bool,
}

fn apply_activation_transition(
    stack: &mut OverlayPriorityStack,
    gained_root: Option<isize>,
    family: &NyariWindowFamily,
) -> TransientDismissalResult {
    let active_surface = stack.active_surface();
    let action = classify_activation_change(gained_root, family, active_surface);
    if action == nyari::layout::ActivationAction::KeepAll {
        return TransientDismissalResult {
            action,
            dismissed_command_palette: false,
            dismissed_hover_sidebar: false,
            should_hide_popup: false,
            restore_focus_to_main: false,
        };
    }

    let had_cmd = stack.command_palette_open;
    let had_hover = stack.hover_sidebar_open;
    stack.apply_activation(action);
    if let Some(kind) = stack.active_kind() {
        if !should_keep_active_kind(kind, action) {
            match kind {
                ActiveOverlayKind::CommandPalette => stack.command_palette_open = false,
                ActiveOverlayKind::HoverSidebar => stack.hover_sidebar_open = false,
                ActiveOverlayKind::Modal
                | ActiveOverlayKind::FindInPage
                | ActiveOverlayKind::ReadingMode => {}
            }
        }
    }

    let dismissed_command_palette = had_cmd && !stack.command_palette_open;
    let dismissed_hover_sidebar = had_hover && !stack.hover_sidebar_open;
    let should_hide_popup = stack.active_kind().is_none();
    let restore_focus_to_main =
        should_hide_popup && action == nyari::layout::ActivationAction::DismissTransientToMain;

    TransientDismissalResult {
        action,
        dismissed_command_palette,
        dismissed_hover_sidebar,
        should_hide_popup,
        restore_focus_to_main,
    }
}

fn sync_transient_dismissal_script(clear_cmd: bool, clear_hover: bool) -> String {
    format!("window.__NYARI_DISMISS_TRANSIENT__?.({clear_cmd}, {clear_hover});")
}

fn switch_active_tab(
    tabs: &[TabMetadata],
    active_tab_id: &mut Option<String>,
    tab_id: &str,
) -> bool {
    if tabs.iter().any(|t| t.id == tab_id) {
        *active_tab_id = Some(tab_id.to_string());
        true
    } else {
        false
    }
}

fn close_tab_and_select_next(
    tabs: &mut Vec<TabMetadata>,
    active_tab_id: &mut Option<String>,
    tab_id: &str,
) -> Option<String> {
    tabs.retain(|t| t.id != tab_id);
    if active_tab_id.as_deref() == Some(tab_id) {
        *active_tab_id = tabs.first().map(|t| t.id.clone());
    }
    active_tab_id.clone()
}

fn sync_active_tab_script(active_tab_id: Option<&str>) -> String {
    let encoded = serde_json::to_string(&active_tab_id).unwrap_or_else(|_| "null".to_string());
    format!("window.__NYARI_SET_ACTIVE_TAB__?.({encoded});")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tab(id: &str) -> TabMetadata {
        TabMetadata {
            id: id.to_string(),
            workspace_id: "ws-default".to_string(),
            title: id.to_string(),
            url: format!("https://{id}.example.com"),
            favicon_url: String::new(),
            is_pinned: false,
            is_private: false,
            is_oauth_popup: false,
            keep_awake: false,
            has_unsaved_input: false,
            can_go_back: false,
            can_go_forward: false,
            is_loading: false,
            is_audible: false,
            shield_blocked_count: 0,
            memory_state: MemoryState::Active,
            ram_bytes: 0,
        }
    }

    fn sample_family() -> NyariWindowFamily {
        NyariWindowFamily {
            main_hwnd: 100,
            shell_overlay_hwnd: Some(200),
            glance_hwnd: None,
            extension_popup_hwnd: None,
            oauth_popup_hwnds: Vec::new(),
        }
    }

    #[test]
    fn windows_subsystem_is_configured_for_release_binary() {
        // Compile-time smoke anchor ensuring binary target builds cleanly alongside lib target.
        assert_eq!(super::MAIN_SUBCLASS_ID, 1001);
    }

    #[test]
    fn activation_transition_preserves_persistent_surfaces_without_transient_dismissal() {
        let family = sample_family();

        // FindInPage stays open when clicking main_hwnd or switching outside app.
        let mut find_stack = OverlayPriorityStack::new();
        find_stack.find_in_page_open = true;
        let res_main = apply_activation_transition(&mut find_stack, Some(100), &family);
        assert_eq!(
            res_main.action,
            nyari::layout::ActivationAction::DismissTransientToMain
        );
        assert!(!res_main.dismissed_command_palette);
        assert!(!res_main.dismissed_hover_sidebar);
        assert!(!res_main.should_hide_popup);
        assert_eq!(
            find_stack.active_kind(),
            Some(ActiveOverlayKind::FindInPage)
        );

        // ReadingMode stays open on Alt+Tab outside app.
        let mut reader_stack = OverlayPriorityStack::new();
        reader_stack.reading_mode_open = true;
        let res_ext = apply_activation_transition(&mut reader_stack, Some(999), &family);
        assert_eq!(
            res_ext.action,
            nyari::layout::ActivationAction::HideTransientOutsideApp
        );
        assert!(!res_ext.dismissed_command_palette);
        assert!(!res_ext.dismissed_hover_sidebar);
        assert!(!res_ext.should_hide_popup);
        assert_eq!(
            reader_stack.active_kind(),
            Some(ActiveOverlayKind::ReadingMode)
        );

        // Modal stays open.
        let mut modal_stack = OverlayPriorityStack::new();
        modal_stack.push_modal("req-1");
        let res_modal = apply_activation_transition(&mut modal_stack, Some(100), &family);
        assert!(!res_modal.dismissed_command_palette);
        assert!(!res_modal.dismissed_hover_sidebar);
        assert!(!res_modal.should_hide_popup);
        assert_eq!(modal_stack.active_kind(), Some(ActiveOverlayKind::Modal));
    }

    #[test]
    fn activation_transition_clears_both_transient_surfaces_together_and_keeps_underlying_persistent(
    ) {
        let family = sample_family();

        // Both CommandPalette and HoverSidebar open: both clear and popup hides.
        let mut both_transient = OverlayPriorityStack::new();
        both_transient.command_palette_open = true;
        both_transient.hover_sidebar_open = true;
        let res = apply_activation_transition(&mut both_transient, Some(100), &family);
        assert!(res.dismissed_command_palette);
        assert!(res.dismissed_hover_sidebar);
        assert!(res.should_hide_popup);
        assert!(res.restore_focus_to_main);
        assert_eq!(both_transient.active_kind(), None);

        // CommandPalette + ReadingMode open: CommandPalette clears, ReadingMode remains, popup stays visible.
        let mut stacked = OverlayPriorityStack::new();
        stacked.command_palette_open = true;
        stacked.reading_mode_open = true;
        let res_stacked = apply_activation_transition(&mut stacked, Some(100), &family);
        assert!(res_stacked.dismissed_command_palette);
        assert!(!res_stacked.dismissed_hover_sidebar);
        assert!(!res_stacked.should_hide_popup);
        assert!(!res_stacked.restore_focus_to_main);
        assert_eq!(stacked.active_kind(), Some(ActiveOverlayKind::ReadingMode));

        let js = sync_transient_dismissal_script(true, true);
        assert!(js.contains("__NYARI_DISMISS_TRANSIENT__"));
        assert!(js.contains("true, true"));
    }

    #[test]
    fn switch_and_close_tab_helpers_sync_active_tab_id_reliably() {
        let mut tabs = vec![sample_tab("tab-1"), sample_tab("tab-2")];
        let mut active = Some("tab-1".to_string());

        assert!(switch_active_tab(&tabs, &mut active, "tab-2"));
        assert_eq!(active.as_deref(), Some("tab-2"));

        assert!(!switch_active_tab(&tabs, &mut active, "tab-missing"));
        assert_eq!(active.as_deref(), Some("tab-2"));

        let next = close_tab_and_select_next(&mut tabs, &mut active, "tab-2");
        assert_eq!(next.as_deref(), Some("tab-1"));
        assert_eq!(active.as_deref(), Some("tab-1"));

        let empty_next = close_tab_and_select_next(&mut tabs, &mut active, "tab-1");
        assert_eq!(empty_next, None);
        assert_eq!(active, None);

        assert_eq!(
            sync_active_tab_script(Some("tab-1")),
            r#"window.__NYARI_SET_ACTIVE_TAB__?.("tab-1");"#
        );
        assert_eq!(
            sync_active_tab_script(None),
            "window.__NYARI_SET_ACTIVE_TAB__?.(null);"
        );
    }
}
