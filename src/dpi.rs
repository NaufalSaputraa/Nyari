use windows::Win32::Foundation::HWND;
use windows::Win32::UI::HiDpi::GetDpiForWindow;

/// Standard Windows 100% baseline DPI.
pub const BASE_DPI: u32 = 96;

/// Minimum main window width in DIP to guarantee Win11 1/3 Snap Layout compatibility.
pub const MIN_WINDOW_WIDTH_DIP: f64 = 330.0;

/// Expanded vertical sidebar width in DIP.
pub const SIDEBAR_EXPANDED_DIP: f64 = 230.0;

/// Collapsed vertical favicon-rail sidebar width in DIP.
pub const SIDEBAR_COLLAPSED_DIP: f64 = 48.0;

/// Top caption strip height in DIP for vertical layout modes.
pub const VERTICAL_CAPTION_HEIGHT_DIP: f64 = 32.0;

/// Classic horizontal top bar height in DIP.
pub const TOPBAR_HEIGHT_DIP: f64 = 42.0;

/// Thickness in DIP of the dynamic resize border ring around restored windows.
pub const RESIZE_BORDER_DIP: f64 = 6.0;

/// Width in DIP of the native Win32 Snap Maximize (`HTMAXBUTTON`) overlay zone.
pub const SNAP_ZONE_WIDTH_DIP: f64 = 46.0;

/// Height in DIP of the native Win32 Snap Maximize (`HTMAXBUTTON`) overlay zone.
pub const SNAP_ZONE_HEIGHT_DIP: f64 = 32.0;

/// Default width in DIP of the uniform Find-in-Page overlay box in Layer 2.
pub const FIND_OVERLAY_WIDTH_DIP: f64 = 340.0;

/// Default height in DIP of the uniform Find-in-Page overlay box in Layer 2.
pub const FIND_OVERLAY_HEIGHT_DIP: f64 = 44.0;

/// Minimum width in DIP for owned extension action popups.
pub const EXT_POPUP_MIN_W_DIP: f64 = 200.0;

/// Minimum height in DIP for owned extension action popups.
pub const EXT_POPUP_MIN_H_DIP: f64 = 150.0;

/// Maximum width in DIP for owned extension action popups.
pub const EXT_POPUP_MAX_W_DIP: f64 = 800.0;

/// Maximum height in DIP for owned extension action popups.
pub const EXT_POPUP_MAX_H_DIP: f64 = 600.0;

/// Computes the Per-Monitor DPI V2 scale factor (`dpi / 96.0`), clamping below 96 DPI to `1.0`.
pub fn scale_factor(dpi: u32) -> f64 {
    f64::from(dpi.max(BASE_DPI)) / f64::from(BASE_DPI)
}

/// Converts a Device-Independent Pixel (DIP) measurement into rounded physical pixels for `dpi`.
pub fn dip_to_phys(dip: f64, dpi: u32) -> i32 {
    (dip * scale_factor(dpi)).round() as i32
}

/// Queries the window's effective Per-Monitor DPI via `GetDpiForWindow`, falling back to `96`.
pub fn window_dpi(hwnd: HWND) -> u32 {
    if hwnd.is_invalid() {
        return BASE_DPI;
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        BASE_DPI
    } else {
        dpi
    }
}

#[cfg(test)]
mod tests {
    use super::{dip_to_phys, scale_factor, window_dpi, BASE_DPI};
    use windows::Win32::Foundation::HWND;

    #[test]
    fn scale_factor_handles_standard_and_zero_dpis() {
        assert!((scale_factor(0) - 1.0).abs() < f64::EPSILON);
        assert!((scale_factor(96) - 1.0).abs() < f64::EPSILON);
        assert!((scale_factor(120) - 1.25).abs() < f64::EPSILON);
        assert!((scale_factor(144) - 1.5).abs() < f64::EPSILON);
        assert!((scale_factor(192) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dip_to_phys_scales_across_96_120_144_and_192_dpi() {
        // 96 DPI (100%)
        assert_eq!(dip_to_phys(230.0, 96), 230);
        assert_eq!(dip_to_phys(32.0, 96), 32);
        assert_eq!(dip_to_phys(6.0, 96), 6);

        // 120 DPI (125%)
        assert_eq!(dip_to_phys(230.0, 120), 288);
        assert_eq!(dip_to_phys(48.0, 120), 60);
        assert_eq!(dip_to_phys(32.0, 120), 40);
        assert_eq!(dip_to_phys(6.0, 120), 8);

        // 144 DPI (150%)
        assert_eq!(dip_to_phys(230.0, 144), 345);
        assert_eq!(dip_to_phys(48.0, 144), 72);
        assert_eq!(dip_to_phys(32.0, 144), 48);
        assert_eq!(dip_to_phys(42.0, 144), 63);
        assert_eq!(dip_to_phys(46.0, 144), 69);
        assert_eq!(dip_to_phys(6.0, 144), 9);

        // 192 DPI (200%)
        assert_eq!(dip_to_phys(230.0, 192), 460);
        assert_eq!(dip_to_phys(48.0, 192), 96);
        assert_eq!(dip_to_phys(32.0, 192), 64);
        assert_eq!(dip_to_phys(6.0, 192), 12);
    }

    #[test]
    fn window_dpi_falls_back_to_base_dpi_on_null_hwnd() {
        assert_eq!(window_dpi(HWND::default()), BASE_DPI);
    }
}
