use crate::NyariError;
use windows::core::{s, w};
use windows::Win32::Foundation::{E_INVALIDARG, HWND};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMSBT_TRANSIENTWINDOW,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::UI::Controls::MARGINS;

/// Minimum Windows 11 build number (`22000`, Windows 11 21H2 — introduces `DWMWA_WINDOW_CORNER_PREFERENCE`).
pub const WIN11_MIN_BUILD: u32 = 22000;

/// Minimum Windows 11 22H2 build number (`22621` — introduces `DWMWA_SYSTEMBACKDROP_TYPE`).
pub const WIN11_22H2_MIN_BUILD: u32 = 22621;

/// Declarative DWM frame margin and Win11 backdrop attributes for a window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackdropConfig {
    pub margins: MARGINS,
    pub system_backdrop: Option<DWM_SYSTEMBACKDROP_TYPE>,
    pub corner_preference: Option<DWM_WINDOW_CORNER_PREFERENCE>,
}

/// Queries the true OS build number via `ntdll!RtlGetVersion` (unaffected by version-lie shims).
pub fn windows_build_number() -> Option<u32> {
    type FarProcFn = unsafe extern "system" fn() -> isize;
    type RtlGetVersionFn = unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32;

    let ntdll = unsafe { GetModuleHandleW(w!("ntdll.dll")) }.ok()?;
    let proc = unsafe { GetProcAddress(ntdll, s!("RtlGetVersion")) }?;
    let rtl_get_version: RtlGetVersionFn =
        unsafe { std::mem::transmute::<FarProcFn, RtlGetVersionFn>(proc) };

    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    let status = unsafe { rtl_get_version(&raw mut info) };
    if status != 0 {
        return None;
    }
    if info.dwMajorVersion >= 10 {
        Some(info.dwBuildNumber)
    } else {
        Some(0)
    }
}

/// Returns `true` when running on Windows 11 (`build >= 22000`).
pub fn is_windows_11_or_newer() -> bool {
    windows_build_number().is_some_and(|build| build >= WIN11_MIN_BUILD)
}

/// Computes the DWM backdrop and frame extension configuration for a specific OS `build_number`.
pub const fn compute_backdrop_config_for_build(
    build_number: u32,
    is_transient: bool,
) -> BackdropConfig {
    if build_number >= WIN11_MIN_BUILD {
        BackdropConfig {
            margins: MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            },
            // `DWMWA_SYSTEMBACKDROP_TYPE` was introduced in Windows 11 22H2 (build 22621).
            system_backdrop: if build_number >= WIN11_22H2_MIN_BUILD {
                Some(if is_transient {
                    DWMSBT_TRANSIENTWINDOW
                } else {
                    DWMSBT_MAINWINDOW
                })
            } else {
                None
            },
            // `DWMWA_WINDOW_CORNER_PREFERENCE` is supported starting in Windows 11 21H2 (build 22000).
            corner_preference: if is_transient {
                Some(DWMWCP_ROUND)
            } else {
                None
            },
        }
    } else {
        // Win10 frameless windows require a 1px top frame extension to preserve shadow/border integrity.
        BackdropConfig {
            margins: MARGINS {
                cxLeftWidth: 0,
                cxRightWidth: 0,
                cyTopHeight: 1,
                cyBottomHeight: 0,
            },
            system_backdrop: None,
            corner_preference: None,
        }
    }
}

/// Computes the DWM backdrop and frame extension configuration for Win11 vs Win10.
pub const fn compute_backdrop_config(is_win11: bool, is_transient: bool) -> BackdropConfig {
    let build = if is_win11 { WIN11_22H2_MIN_BUILD } else { 0 };
    compute_backdrop_config_for_build(build, is_transient)
}

/// Applies Mica/Acrylic backdrop attributes for an explicit OS `build_number`.
pub fn apply_window_backdrop_for_build(
    hwnd: HWND,
    build_number: u32,
    is_transient: bool,
) -> Result<(), NyariError> {
    if hwnd.is_invalid() {
        return Err(NyariError::InvalidInput(
            "HWND must not be null".to_string(),
        ));
    }

    let config = compute_backdrop_config_for_build(build_number, is_transient);
    unsafe { DwmExtendFrameIntoClientArea(hwnd, &config.margins) }?;

    // Apply Win11 21H2+ corner preference before 22H2+ system backdrop so older Win11 never loses rounded corners.
    if let Some(corners) = config.corner_preference {
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const corners).cast(),
                std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            )
        }?;
    }

    if let Some(backdrop) = config.system_backdrop {
        let res = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                (&raw const backdrop).cast(),
                std::mem::size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
            )
        };
        if let Err(err) = res {
            if err.code() != E_INVALIDARG {
                return Err(err.into());
            }
        }
    }

    Ok(())
}

/// Applies Mica/Acrylic backdrop attributes on Windows 11 or 1px top border extension on Windows 10.
pub fn apply_window_backdrop(
    hwnd: HWND,
    is_win11: bool,
    is_transient: bool,
) -> Result<(), NyariError> {
    let build_number = if is_win11 {
        windows_build_number()
            .unwrap_or(WIN11_MIN_BUILD)
            .max(WIN11_MIN_BUILD)
    } else {
        0
    };
    apply_window_backdrop_for_build(hwnd, build_number, is_transient)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_window_backdrop, apply_window_backdrop_for_build, compute_backdrop_config,
        compute_backdrop_config_for_build, is_windows_11_or_newer, windows_build_number,
        WIN11_22H2_MIN_BUILD, WIN11_MIN_BUILD,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DWMSBT_MAINWINDOW, DWMSBT_TRANSIENTWINDOW, DWMWCP_ROUND};

    #[test]
    fn compute_backdrop_config_handles_win11_main_win11_transient_and_win10() {
        let win11_main = compute_backdrop_config(true, false);
        assert_eq!(win11_main.margins.cxLeftWidth, -1);
        assert_eq!(win11_main.margins.cyTopHeight, -1);
        assert_eq!(win11_main.system_backdrop, Some(DWMSBT_MAINWINDOW));
        assert_eq!(win11_main.corner_preference, None);

        let win11_popup = compute_backdrop_config(true, true);
        assert_eq!(win11_popup.margins.cxLeftWidth, -1);
        assert_eq!(win11_popup.system_backdrop, Some(DWMSBT_TRANSIENTWINDOW));
        assert_eq!(win11_popup.corner_preference, Some(DWMWCP_ROUND));

        let win10 = compute_backdrop_config(false, false);
        assert_eq!(win10.margins.cxLeftWidth, 0);
        assert_eq!(win10.margins.cxRightWidth, 0);
        assert_eq!(win10.margins.cyTopHeight, 1);
        assert_eq!(win10.margins.cyBottomHeight, 0);
        assert_eq!(win10.system_backdrop, None);
        assert_eq!(win10.corner_preference, None);
    }

    #[test]
    fn compute_backdrop_config_for_build_gates_system_backdrop_at_22621_and_corners_at_22000() {
        // Win10 build 19045 -> 1px top frame extension only
        let win10 = compute_backdrop_config_for_build(19045, true);
        assert_eq!(win10.margins.cxLeftWidth, 0);
        assert_eq!(win10.margins.cyTopHeight, 1);
        assert_eq!(win10.system_backdrop, None);
        assert_eq!(win10.corner_preference, None);

        // Win11 21H2 build 22000 -> -1 margins + DWMWCP_ROUND for transient, no DWMWA_SYSTEMBACKDROP_TYPE
        let win11_21h2_transient = compute_backdrop_config_for_build(WIN11_MIN_BUILD, true);
        assert_eq!(win11_21h2_transient.margins.cxLeftWidth, -1);
        assert_eq!(win11_21h2_transient.margins.cyTopHeight, -1);
        assert_eq!(win11_21h2_transient.corner_preference, Some(DWMWCP_ROUND));
        assert_eq!(win11_21h2_transient.system_backdrop, None);

        let win11_21h2_main = compute_backdrop_config_for_build(WIN11_MIN_BUILD, false);
        assert_eq!(win11_21h2_main.corner_preference, None);
        assert_eq!(win11_21h2_main.system_backdrop, None);

        // Win11 22H2 build 22621 -> both corner_preference and system_backdrop set
        let win11_22h2_transient = compute_backdrop_config_for_build(WIN11_22H2_MIN_BUILD, true);
        assert_eq!(win11_22h2_transient.margins.cxLeftWidth, -1);
        assert_eq!(win11_22h2_transient.corner_preference, Some(DWMWCP_ROUND));
        assert_eq!(
            win11_22h2_transient.system_backdrop,
            Some(DWMSBT_TRANSIENTWINDOW)
        );

        let win11_22h2_main = compute_backdrop_config_for_build(WIN11_22H2_MIN_BUILD, false);
        assert_eq!(win11_22h2_main.corner_preference, None);
        assert_eq!(win11_22h2_main.system_backdrop, Some(DWMSBT_MAINWINDOW));
    }

    #[test]
    fn apply_window_backdrop_rejects_null_hwnd_and_queries_os_build() {
        assert!(apply_window_backdrop(HWND::default(), true, false).is_err());
        assert!(apply_window_backdrop_for_build(HWND::default(), 22621, true).is_err());
        let build = windows_build_number().expect("RtlGetVersion should succeed on Windows");
        assert!(build > 0);
        assert_eq!(is_windows_11_or_newer(), build >= WIN11_MIN_BUILD);
    }

    #[test]
    fn apply_window_backdrop_succeeds_on_real_top_level_hwnd() {
        use crate::layout::create_owned_popup_styles;
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow};

        let styles = create_owned_popup_styles();
        let hwnd = unsafe {
            CreateWindowExW(
                styles.ex_style,
                w!("STATIC"),
                w!("nyari-backdrop-test"),
                styles.style,
                0,
                0,
                320,
                240,
                None,
                None,
                None,
                None,
            )
        }
        .expect("CreateWindowExW popup should succeed");

        let is_win11 = is_windows_11_or_newer();
        let res_main = apply_window_backdrop(hwnd, is_win11, false);
        let res_transient = apply_window_backdrop(hwnd, is_win11, true);
        let res_win10_fallback = apply_window_backdrop(hwnd, false, false);
        let res_21h2 = if is_win11 {
            apply_window_backdrop_for_build(hwnd, WIN11_MIN_BUILD, true)
        } else {
            Ok(())
        };

        unsafe {
            let _ = DestroyWindow(hwnd);
        }

        res_main.expect("apply_window_backdrop main window should succeed");
        res_transient.expect("apply_window_backdrop transient popup should succeed");
        res_win10_fallback.expect("apply_window_backdrop Win10 path should succeed");
        res_21h2.expect("apply_window_backdrop_for_build 21H2 path should succeed");
    }
}
