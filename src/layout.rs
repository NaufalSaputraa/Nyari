use crate::dpi::{
    dip_to_phys, EXT_POPUP_MAX_H_DIP, EXT_POPUP_MAX_W_DIP, EXT_POPUP_MIN_H_DIP,
    EXT_POPUP_MIN_W_DIP, RESIZE_BORDER_DIP, SIDEBAR_COLLAPSED_DIP, SIDEBAR_EXPANDED_DIP,
    SNAP_ZONE_HEIGHT_DIP, SNAP_ZONE_WIDTH_DIP, TOPBAR_HEIGHT_DIP, VERTICAL_CAPTION_HEIGHT_DIP,
};
use crate::ipc::{AppSettings, LayoutMode, OverlaySurface, RectDip};
use crate::NyariError;
use std::collections::VecDeque;
use windows::Win32::Foundation::{HWND, LRESULT, RECT};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, GetMonitorInfoW, MonitorFromWindow, SetWindowRgn,
    HRGN, MONITORINFO, MONITOR_DEFAULTTONEAREST, RGN_COMBINE_MODE, RGN_DIFF, RGN_ERROR, RGN_OR,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, IsZoomed, ShowWindow, GA_ROOT, SW_HIDE, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE,
    WINDOW_STYLE, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_POPUP,
};

/// Physical-pixel rectangle relative to the parent client area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhysRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl PhysRect {
    /// Creates a physical-pixel rectangle from origin and dimensions.
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns the exclusive right X coordinate (`x + width`).
    pub const fn right(&self) -> i32 {
        self.x + self.width
    }

    /// Returns the exclusive bottom Y coordinate (`y + height`).
    pub const fn bottom(&self) -> i32 {
        self.y + self.height
    }

    /// Returns `true` if either dimension is non-positive.
    pub const fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// Returns `true` when two non-empty rectangles overlap in area.
    pub const fn intersects(&self, other: &Self) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

/// Non-overlapping physical geometry for Layer 1 shell chrome, Content Host, and Snap Zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowLayoutRects {
    pub layer1_visible: bool,
    pub layer1_bounds: PhysRect,
    pub layer1_sidebar: Option<PhysRect>,
    pub layer1_caption: Option<PhysRect>,
    pub content_bounds: PhysRect,
    pub snap_zone: Option<PhysRect>,
    pub resize_border_phys: i32,
}

/// Internal Layer 2 overlay identity distinguishing Find-in-Page from Command Palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveOverlayKind {
    Modal,
    CommandPalette,
    FindInPage,
    ReadingMode,
    HoverSidebar,
}

impl ActiveOverlayKind {
    /// Maps the internal overlay kind to its IPC [`OverlaySurface`] for `SET_OVERLAY_BOUNDS`.
    pub const fn to_ipc_surface(self) -> OverlaySurface {
        match self {
            Self::Modal => OverlaySurface::Modal,
            Self::CommandPalette | Self::FindInPage => OverlaySurface::CommandPalette,
            Self::ReadingMode => OverlaySurface::ReadingMode,
            Self::HoverSidebar => OverlaySurface::HoverSidebar,
        }
    }
}

/// Priority-ordered single-surface controller for the Layer 2 owned popup window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverlayPriorityStack {
    modal_queue: VecDeque<String>,
    pub command_palette_open: bool,
    pub find_in_page_open: bool,
    pub reading_mode_open: bool,
    pub hover_sidebar_open: bool,
}

impl OverlayPriorityStack {
    /// Creates an empty priority stack with no active surfaces.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueues a Priority 1 modal request ID in FIFO order if not already present.
    pub fn push_modal(&mut self, request_id: impl Into<String>) {
        let req = request_id.into();
        if !self.modal_queue.iter().any(|existing| existing == &req) {
            self.modal_queue.push_back(req);
        }
    }

    /// Removes a resolved modal request ID from the FIFO queue.
    pub fn resolve_modal(&mut self, request_id: &str) -> bool {
        if let Some(pos) = self.modal_queue.iter().position(|id| id == request_id) {
            self.modal_queue.remove(pos);
            true
        } else {
            false
        }
    }

    /// Pops and returns the oldest pending modal request ID.
    pub fn pop_front_modal(&mut self) -> Option<String> {
        self.modal_queue.pop_front()
    }

    /// Returns the front-most pending modal request ID without removing it.
    pub fn active_modal(&self) -> Option<&str> {
        self.modal_queue.front().map(String::as_str)
    }

    /// Returns the number of queued Priority 1 modal requests.
    pub fn modal_count(&self) -> usize {
        self.modal_queue.len()
    }

    /// Resolves the highest-priority active overlay kind (`Modal > CommandPalette > FindInPage > ReadingMode > HoverSidebar`).
    pub fn active_kind(&self) -> Option<ActiveOverlayKind> {
        if !self.modal_queue.is_empty() {
            Some(ActiveOverlayKind::Modal)
        } else if self.command_palette_open {
            Some(ActiveOverlayKind::CommandPalette)
        } else if self.find_in_page_open {
            Some(ActiveOverlayKind::FindInPage)
        } else if self.reading_mode_open {
            Some(ActiveOverlayKind::ReadingMode)
        } else if self.hover_sidebar_open {
            Some(ActiveOverlayKind::HoverSidebar)
        } else {
            None
        }
    }

    /// Resolves the highest-priority active surface (`Modal > CommandPalette/FindInPage > ReadingMode > HoverSidebar`).
    pub fn active_surface(&self) -> Option<OverlaySurface> {
        self.active_kind().map(ActiveOverlayKind::to_ipc_surface)
    }

    /// Applies an activation guard disposition, dismissing transient surfaces while preserving modals and reading mode.
    pub fn apply_activation(&mut self, action: ActivationAction) -> ActivationEffects {
        let effects = evaluate_activation_effects(action);
        if effects.dismiss_command_palette {
            self.command_palette_open = false;
        }
        if effects.dismiss_hover_sidebar {
            self.hover_sidebar_open = false;
        }
        effects
    }
}

/// Top-level HWND family owned by a Nyari browser window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NyariWindowFamily {
    pub main_hwnd: isize,
    pub shell_overlay_hwnd: Option<isize>,
    pub glance_hwnd: Option<isize>,
    pub extension_popup_hwnd: Option<isize>,
    pub oauth_popup_hwnds: Vec<isize>,
}

impl NyariWindowFamily {
    /// Returns `true` if `root_hwnd` matches any owned popup or OAuth popup window in this family.
    pub fn contains_owned_popup(&self, root_hwnd: isize) -> bool {
        self.shell_overlay_hwnd == Some(root_hwnd)
            || self.glance_hwnd == Some(root_hwnd)
            || self.extension_popup_hwnd == Some(root_hwnd)
            || self.oauth_popup_hwnds.contains(&root_hwnd)
    }
}

/// Disposition returned by the `WM_ACTIVATE` window-family guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationAction {
    KeepAll,
    DismissTransientToMain,
    HideTransientOutsideApp,
}

/// Explicit per-surface retention and dismissal flags derived from [`ActivationAction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivationEffects {
    pub action: ActivationAction,
    pub dismiss_command_palette: bool,
    pub dismiss_hover_sidebar: bool,
    pub dismiss_glance: bool,
    pub dismiss_extension_popup: bool,
    pub keep_modal: bool,
    pub keep_find_in_page: bool,
    pub keep_reading_mode: bool,
    pub keep_oauth_popups: bool,
}

/// Win32 style pair used when creating owned `WS_POPUP` Layer 2 / Glance / Extension windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnedPopupStyles {
    pub style: WINDOW_STYLE,
    pub ex_style: WINDOW_EX_STYLE,
}

/// RAII wrapper ensuring intermediate GDI `HRGN` handles are deleted on drop unless ownership is transferred.
struct RgnGuard(HRGN);

impl RgnGuard {
    fn create_rect(left: i32, top: i32, right: i32, bottom: i32) -> Result<Self, NyariError> {
        let hrgn = unsafe { CreateRectRgn(left, top, right, bottom) };
        if hrgn.is_invalid() {
            return Err(win32_last_error_or("CreateRectRgn failed"));
        }
        Ok(Self(hrgn))
    }

    fn empty() -> Result<Self, NyariError> {
        Self::create_rect(0, 0, 0, 0)
    }

    fn as_raw(&self) -> HRGN {
        self.0
    }

    fn into_raw(mut self) -> HRGN {
        let raw = self.0;
        self.0 = HRGN::default();
        raw
    }
}

impl Drop for RgnGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = DeleteObject(self.0.into());
            }
        }
    }
}

fn win32_last_error_or(fallback: &str) -> NyariError {
    let err = windows::core::Error::from_thread();
    if err.code().is_ok() {
        NyariError::Other(fallback.to_string())
    } else {
        NyariError::Win32(err)
    }
}

fn combine_regions(
    src1: &RgnGuard,
    src2: &RgnGuard,
    mode: RGN_COMBINE_MODE,
) -> Result<RgnGuard, NyariError> {
    let dst = RgnGuard::empty()?;
    let region_type = unsafe {
        CombineRgn(
            Some(dst.as_raw()),
            Some(src1.as_raw()),
            Some(src2.as_raw()),
            mode,
        )
    };
    if region_type == RGN_ERROR {
        return Err(win32_last_error_or("CombineRgn failed"));
    }
    Ok(dst)
}

/// Computes non-overlapping physical pixel bounds for Layer 1, Content Host, and Snap Zone.
pub fn compute_rects(
    client_w_phys: i32,
    client_h_phys: i32,
    dpi: u32,
    settings: &AppSettings,
) -> WindowLayoutRects {
    let w = client_w_phys.max(0);
    let h = client_h_phys.max(0);
    let resize_border_phys = dip_to_phys(RESIZE_BORDER_DIP, dpi).max(0);

    if settings.zen_mode {
        return WindowLayoutRects {
            layer1_visible: false,
            layer1_bounds: PhysRect::new(0, 0, 0, 0),
            layer1_sidebar: None,
            layer1_caption: None,
            content_bounds: PhysRect::new(0, 0, w, h),
            snap_zone: None,
            resize_border_phys,
        };
    }

    let snap_zone_for = |caption_h: i32| {
        let snap_w = dip_to_phys(SNAP_ZONE_WIDTH_DIP, dpi).clamp(0, w);
        let snap_h = dip_to_phys(SNAP_ZONE_HEIGHT_DIP, dpi).clamp(0, caption_h);
        Some(PhysRect::new((w - snap_w).max(0), 0, snap_w, snap_h))
    };

    match settings.layout_mode {
        LayoutMode::VerticalExpanded | LayoutMode::VerticalCollapsed => {
            let sidebar_dip = match settings.layout_mode {
                LayoutMode::VerticalExpanded => SIDEBAR_EXPANDED_DIP,
                LayoutMode::VerticalCollapsed => SIDEBAR_COLLAPSED_DIP,
                LayoutMode::Topbar => unreachable!(),
            };
            let sidebar_w = dip_to_phys(sidebar_dip, dpi).clamp(0, w);
            let caption_h = dip_to_phys(VERTICAL_CAPTION_HEIGHT_DIP, dpi).clamp(0, h);

            WindowLayoutRects {
                layer1_visible: true,
                layer1_bounds: PhysRect::new(0, 0, w, h),
                layer1_sidebar: Some(PhysRect::new(0, 0, sidebar_w, h)),
                layer1_caption: Some(PhysRect::new(sidebar_w, 0, w - sidebar_w, caption_h)),
                content_bounds: PhysRect::new(sidebar_w, caption_h, w - sidebar_w, h - caption_h),
                snap_zone: snap_zone_for(caption_h),
                resize_border_phys,
            }
        }
        LayoutMode::Topbar => {
            let topbar_h = dip_to_phys(TOPBAR_HEIGHT_DIP, dpi).clamp(0, h);
            let topbar_rect = PhysRect::new(0, 0, w, topbar_h);

            WindowLayoutRects {
                layer1_visible: true,
                layer1_bounds: topbar_rect,
                layer1_sidebar: None,
                layer1_caption: Some(topbar_rect),
                content_bounds: PhysRect::new(0, topbar_h, w, h - topbar_h),
                snap_zone: snap_zone_for(topbar_h),
                resize_border_phys,
            }
        }
    }
}

/// Builds an L-shaped combined `HRGN` (`RGN_OR`) for vertical layouts so Layer 1 does not overlap Content Host.
pub fn build_l_shaped_layer1_region(rects: &WindowLayoutRects) -> Result<Option<HRGN>, NyariError> {
    if !rects.layer1_visible {
        return Ok(None);
    }
    let (Some(sidebar), Some(caption)) = (rects.layer1_sidebar, rects.layer1_caption) else {
        return Ok(None);
    };
    let sidebar_rgn =
        RgnGuard::create_rect(sidebar.x, sidebar.y, sidebar.right(), sidebar.bottom())?;
    let caption_rgn =
        RgnGuard::create_rect(caption.x, caption.y, caption.right(), caption.bottom())?;
    let combined = combine_regions(&sidebar_rgn, &caption_rgn, RGN_OR)?;
    Ok(Some(combined.into_raw()))
}

/// Builds a hollow resize-border ring `HRGN` (`outer - inner`, `RGN_DIFF`) of thickness `border_phys`.
pub fn build_resize_ring_region(
    w_phys: i32,
    h_phys: i32,
    border_phys: i32,
) -> Result<HRGN, NyariError> {
    let w = w_phys.max(0);
    let h = h_phys.max(0);
    let b = border_phys.max(0);

    let outer = RgnGuard::create_rect(0, 0, w, h)?;
    let inner_left = b.min(w);
    let inner_top = b.min(h);
    let inner_right = (w - b).max(inner_left);
    let inner_bottom = (h - b).max(inner_top);
    let inner = RgnGuard::create_rect(inner_left, inner_top, inner_right, inner_bottom)?;
    let ring = combine_regions(&outer, &inner, RGN_DIFF)?;
    Ok(ring.into_raw())
}

/// Applies `hrgn` to `hwnd` via `SetWindowRgn`, freeing `hrgn` ONLY when `SetWindowRgn` returns `0`.
pub fn apply_window_region(hwnd: HWND, hrgn: HRGN, redraw: bool) -> Result<(), NyariError> {
    if hrgn.is_invalid() {
        return Err(NyariError::InvalidInput(
            "HRGN must not be null".to_string(),
        ));
    }
    if hwnd.is_invalid() {
        unsafe {
            let _ = DeleteObject(hrgn.into());
        }
        return Err(NyariError::InvalidInput(
            "HWND must not be null".to_string(),
        ));
    }

    let applied = unsafe { SetWindowRgn(hwnd, Some(hrgn), redraw) };
    if applied == 0 {
        let err = win32_last_error_or("SetWindowRgn failed");
        unsafe {
            let _ = DeleteObject(hrgn.into());
        }
        return Err(err);
    }

    // Windows owns `hrgn` on non-zero `SetWindowRgn` return; never call `DeleteObject` here.
    Ok(())
}

/// Hides the dynamic resize border ring when maximized/fullscreen, or updates its `HRGN` and shows `SW_SHOWNOACTIVATE`.
pub fn update_resize_ring(
    ring_hwnd: HWND,
    w_phys: i32,
    h_phys: i32,
    dpi: u32,
    is_maximized_or_fullscreen: bool,
) -> Result<(), NyariError> {
    if ring_hwnd.is_invalid() {
        return Err(NyariError::InvalidInput(
            "Resize ring HWND must not be null".to_string(),
        ));
    }
    if is_maximized_or_fullscreen {
        unsafe {
            let _ = ShowWindow(ring_hwnd, SW_HIDE);
        }
        return Ok(());
    }

    let border_phys = dip_to_phys(RESIZE_BORDER_DIP, dpi);
    let hrgn = build_resize_ring_region(w_phys, h_phys, border_phys)?;
    apply_window_region(ring_hwnd, hrgn, false)?;
    unsafe {
        let _ = ShowWindow(ring_hwnd, SW_SHOWNOACTIVATE);
    }
    Ok(())
}

/// Anchors an extension popup below `anchor` and clamps dimensions to `[200x150, 800x600]` DIP.
pub fn clamp_extension_popup_rect(
    anchor: RectDip,
    requested_w_dip: f64,
    requested_h_dip: f64,
) -> RectDip {
    let width = if requested_w_dip.is_finite() {
        requested_w_dip.clamp(EXT_POPUP_MIN_W_DIP, EXT_POPUP_MAX_W_DIP)
    } else {
        EXT_POPUP_MIN_W_DIP
    };
    let height = if requested_h_dip.is_finite() {
        requested_h_dip.clamp(EXT_POPUP_MIN_H_DIP, EXT_POPUP_MAX_H_DIP)
    } else {
        EXT_POPUP_MIN_H_DIP
    };
    let x = if anchor.x.is_finite() { anchor.x } else { 0.0 };
    let y = if anchor.y.is_finite() { anchor.y } else { 0.0 };
    let anchor_h = if anchor.height.is_finite() {
        anchor.height.max(0.0)
    } else {
        0.0
    };

    RectDip {
        x,
        y: y + anchor_h,
        width,
        height,
    }
}

/// Classifies a `WM_ACTIVATE(WA_INACTIVE)` transition using the gaining window's `GA_ROOT` ancestor.
pub fn classify_activation_change(
    gaining_root: Option<isize>,
    family: &NyariWindowFamily,
    _active_surface: Option<OverlaySurface>,
) -> ActivationAction {
    let Some(root) = gaining_root.filter(|&hwnd| hwnd != 0) else {
        return ActivationAction::HideTransientOutsideApp;
    };

    if family.contains_owned_popup(root) {
        ActivationAction::KeepAll
    } else if root == family.main_hwnd {
        ActivationAction::DismissTransientToMain
    } else {
        ActivationAction::HideTransientOutsideApp
    }
}

/// Maps an [`ActivationAction`] to explicit transient dismissal and persistent surface retention flags.
pub const fn evaluate_activation_effects(action: ActivationAction) -> ActivationEffects {
    match action {
        ActivationAction::KeepAll => ActivationEffects {
            action,
            dismiss_command_palette: false,
            dismiss_hover_sidebar: false,
            dismiss_glance: false,
            dismiss_extension_popup: false,
            keep_modal: true,
            keep_find_in_page: true,
            keep_reading_mode: true,
            keep_oauth_popups: true,
        },
        ActivationAction::DismissTransientToMain | ActivationAction::HideTransientOutsideApp => {
            ActivationEffects {
                action,
                dismiss_command_palette: true,
                dismiss_hover_sidebar: true,
                dismiss_glance: true,
                dismiss_extension_popup: true,
                keep_modal: true,
                keep_find_in_page: true,
                keep_reading_mode: true,
                keep_oauth_popups: true,
            }
        }
    }
}

/// Returns `true` if `kind` remains open under `action` (`Modal`, `ReadingMode`, and `FindInPage` stay open).
pub const fn should_keep_active_kind(kind: ActiveOverlayKind, action: ActivationAction) -> bool {
    match action {
        ActivationAction::KeepAll => true,
        ActivationAction::DismissTransientToMain | ActivationAction::HideTransientOutsideApp => {
            matches!(
                kind,
                ActiveOverlayKind::Modal
                    | ActiveOverlayKind::ReadingMode
                    | ActiveOverlayKind::FindInPage
            )
        }
    }
}

/// Returns `true` if `surface` remains open under `action` (`Modal` and `ReadingMode` stay open;
/// use [`should_keep_active_kind`] when distinguishing `FindInPage` from `CommandPalette`).
pub const fn should_keep_surface_open(surface: OverlaySurface, action: ActivationAction) -> bool {
    match action {
        ActivationAction::KeepAll => true,
        ActivationAction::DismissTransientToMain | ActivationAction::HideTransientOutsideApp => {
            matches!(surface, OverlaySurface::Modal | OverlaySurface::ReadingMode)
        }
    }
}

/// Clamps a rectangle against a monitor work area without introducing inverted bounds.
pub fn clamp_rect_to_work_area(rect: RECT, work: RECT) -> RECT {
    let max_right = work.right.max(work.left);
    let max_bottom = work.bottom.max(work.top);
    let left = rect.left.clamp(work.left, max_right);
    let top = rect.top.clamp(work.top, max_bottom);
    let right = rect.right.clamp(left, max_right);
    let bottom = rect.bottom.clamp(top, max_bottom);
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

/// Clamps `rect` (from `WM_NCCALCSIZE`) to the nearest monitor's `rcWork` to prevent ~9px frameless maximize clipping.
pub fn clamp_maximized_rect_to_monitor_work(hwnd: HWND, rect: &mut RECT) -> bool {
    if hwnd.is_invalid() {
        return false;
    }
    let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if hmon.is_invalid() {
        return false;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { !GetMonitorInfoW(hmon, &raw mut info).as_bool() } {
        return false;
    }
    *rect = clamp_rect_to_work_area(*rect, info.rcWork);
    true
}

/// Returns `true` when `hwnd` is currently in the Win32 zoomed (maximized) state.
pub fn is_window_maximized(hwnd: HWND) -> bool {
    !hwnd.is_invalid() && unsafe { IsZoomed(hwnd).as_bool() }
}

/// Returns `LRESULT(1)` for `WM_ERASEBKGND` on `WS_EX_NOREDIRECTIONBITMAP` windows to suppress GDI erase flicker.
pub const fn handle_wm_erasebkgnd() -> LRESULT {
    LRESULT(1)
}

/// Resolves the top-level `GA_ROOT` ancestor of `hwnd` for the `WM_ACTIVATE` window-family guard.
pub fn get_ancestor_root(hwnd: HWND) -> Option<isize> {
    if hwnd.is_invalid() {
        return None;
    }
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    if root.is_invalid() {
        None
    } else {
        Some(root.0 as isize)
    }
}

/// Returns the standard Win32 styles for owned `WS_POPUP` floating surfaces (`Layer 2`, `Glance`, `ExtensionPopup`).
pub const fn create_owned_popup_styles() -> OwnedPopupStyles {
    OwnedPopupStyles {
        style: WINDOW_STYLE(WS_POPUP.0 | WS_CLIPCHILDREN.0 | WS_CLIPSIBLINGS.0),
        ex_style: WINDOW_EX_STYLE(WS_EX_NOREDIRECTIONBITMAP.0 | WS_EX_TOOLWINDOW.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::LayoutMode;
    use windows::Win32::Graphics::Gdi::{DeleteObject, PtInRegion};
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_POPUP,
    };

    #[test]
    fn compute_rects_scales_all_layout_modes_at_150_percent_dpi() {
        let mut settings = AppSettings {
            layout_mode: LayoutMode::VerticalExpanded,
            zen_mode: false,
            ..AppSettings::default()
        };

        // 150% DPI = 144 DPI (1.5x scale)
        let expanded = compute_rects(1500, 900, 144, &settings);
        assert!(expanded.layer1_visible);
        assert_eq!(expanded.layer1_bounds, PhysRect::new(0, 0, 1500, 900));
        assert_eq!(expanded.layer1_sidebar, Some(PhysRect::new(0, 0, 345, 900)));
        assert_eq!(
            expanded.layer1_caption,
            Some(PhysRect::new(345, 0, 1155, 48))
        );
        assert_eq!(expanded.content_bounds, PhysRect::new(345, 48, 1155, 852));
        assert_eq!(
            expanded.snap_zone,
            Some(PhysRect::new(1500 - 69, 0, 69, 48))
        );
        assert_eq!(expanded.resize_border_phys, 9);
        assert!(!expanded
            .content_bounds
            .intersects(&expanded.layer1_sidebar.unwrap()));
        assert!(!expanded
            .content_bounds
            .intersects(&expanded.layer1_caption.unwrap()));
        assert!(!expanded
            .content_bounds
            .intersects(&expanded.snap_zone.unwrap()));

        // Vertical collapsed: 48 * 1.5 = 72px sidebar, 32 * 1.5 = 48px caption
        settings.layout_mode = LayoutMode::VerticalCollapsed;
        let collapsed = compute_rects(1500, 900, 144, &settings);
        assert!(collapsed.layer1_visible);
        assert_eq!(collapsed.layer1_sidebar, Some(PhysRect::new(0, 0, 72, 900)));
        assert_eq!(
            collapsed.layer1_caption,
            Some(PhysRect::new(72, 0, 1428, 48))
        );
        assert_eq!(collapsed.content_bounds, PhysRect::new(72, 48, 1428, 852));
        assert_eq!(
            collapsed.snap_zone,
            Some(PhysRect::new(1500 - 69, 0, 69, 48))
        );
        assert!(!collapsed
            .content_bounds
            .intersects(&collapsed.layer1_sidebar.unwrap()));
        assert!(!collapsed
            .content_bounds
            .intersects(&collapsed.layer1_caption.unwrap()));

        // Topbar: 42 * 1.5 = 63px topbar
        settings.layout_mode = LayoutMode::Topbar;
        let topbar = compute_rects(1500, 900, 144, &settings);
        assert!(topbar.layer1_visible);
        assert_eq!(topbar.layer1_bounds, PhysRect::new(0, 0, 1500, 63));
        assert_eq!(topbar.layer1_sidebar, None);
        assert_eq!(topbar.layer1_caption, Some(PhysRect::new(0, 0, 1500, 63)));
        assert_eq!(topbar.content_bounds, PhysRect::new(0, 63, 1500, 837));
        assert_eq!(topbar.snap_zone, Some(PhysRect::new(1500 - 69, 0, 69, 48)));
        assert!(!topbar
            .content_bounds
            .intersects(&topbar.layer1_caption.unwrap()));
        assert!(!topbar.content_bounds.intersects(&topbar.snap_zone.unwrap()));

        // Zen mode hides Layer 1 and Snap Zone, giving full canvas to Content Host
        settings.zen_mode = true;
        let zen = compute_rects(1500, 900, 144, &settings);
        assert!(!zen.layer1_visible);
        assert_eq!(zen.layer1_sidebar, None);
        assert_eq!(zen.layer1_caption, None);
        assert_eq!(zen.snap_zone, None);
        assert_eq!(zen.content_bounds, PhysRect::new(0, 0, 1500, 900));
    }

    #[test]
    fn compute_rects_clamps_non_negative_on_tiny_or_zero_client_area() {
        let settings = AppSettings::default();
        let zero = compute_rects(0, 0, 144, &settings);
        assert_eq!(zero.content_bounds, PhysRect::new(0, 0, 0, 0));

        let tiny = compute_rects(100, 20, 144, &settings);
        assert!(tiny.content_bounds.width >= 0);
        assert!(tiny.content_bounds.height >= 0);
        assert!(tiny.snap_zone.unwrap().width >= 0);
        assert!(tiny.snap_zone.unwrap().height >= 0);
    }

    #[test]
    fn l_shaped_layer1_region_and_resize_ring_regions_match_geometry_and_cleanup_cleanly() {
        let expanded = compute_rects(
            1200,
            800,
            144,
            &AppSettings {
                layout_mode: LayoutMode::VerticalExpanded,
                zen_mode: false,
                ..AppSettings::default()
            },
        );
        let l_rgn = build_l_shaped_layer1_region(&expanded)
            .expect("build_l_shaped_layer1_region should succeed")
            .expect("vertical mode must return an L-shaped HRGN");
        assert!(!l_rgn.is_invalid());
        unsafe {
            assert!(PtInRegion(l_rgn, 100, 400).as_bool());
            assert!(PtInRegion(l_rgn, 800, 20).as_bool());
            assert!(!PtInRegion(l_rgn, 500, 200).as_bool());
            assert!(DeleteObject(l_rgn.into()).as_bool());
        }

        let topbar = compute_rects(
            1200,
            800,
            144,
            &AppSettings {
                layout_mode: LayoutMode::Topbar,
                zen_mode: false,
                ..AppSettings::default()
            },
        );
        assert!(build_l_shaped_layer1_region(&topbar).unwrap().is_none());

        let zen = compute_rects(
            1200,
            800,
            144,
            &AppSettings {
                zen_mode: true,
                ..AppSettings::default()
            },
        );
        assert!(build_l_shaped_layer1_region(&zen).unwrap().is_none());

        let ring_rgn = build_resize_ring_region(1200, 800, 9).expect("resize ring should build");
        assert!(!ring_rgn.is_invalid());
        unsafe {
            assert!(PtInRegion(ring_rgn, 4, 4).as_bool());
            assert!(PtInRegion(ring_rgn, 600, 4).as_bool());
            assert!(PtInRegion(ring_rgn, 1195, 400).as_bool());
            assert!(PtInRegion(ring_rgn, 600, 795).as_bool());
            assert!(!PtInRegion(ring_rgn, 600, 400).as_bool());
            assert!(DeleteObject(ring_rgn.into()).as_bool());
        }

        // Verify failure cleanup path in apply_window_region / update_resize_ring on invalid HWND.
        let invalid_target_rgn = build_resize_ring_region(100, 100, 6).unwrap();
        assert!(apply_window_region(HWND::default(), invalid_target_rgn, false).is_err());
        assert!(apply_window_region(HWND::default(), HRGN::default(), false).is_err());
        assert!(update_resize_ring(HWND::default(), 100, 100, 96, false).is_err());
    }

    #[test]
    fn clamp_extension_popup_rect_enforces_200x150_to_800x600_bounds() {
        let anchor = RectDip {
            x: 16.0,
            y: 44.0,
            width: 28.0,
            height: 28.0,
        };

        let too_small = clamp_extension_popup_rect(anchor, 50.0, 20.0);
        assert_eq!(too_small.x, 16.0);
        assert_eq!(too_small.y, 72.0);
        assert_eq!(too_small.width, 200.0);
        assert_eq!(too_small.height, 150.0);

        let too_large = clamp_extension_popup_rect(anchor, 1400.0, 950.0);
        assert_eq!(too_large.width, 800.0);
        assert_eq!(too_large.height, 600.0);

        let in_range = clamp_extension_popup_rect(anchor, 360.0, 420.0);
        assert_eq!(in_range.width, 360.0);
        assert_eq!(in_range.height, 420.0);

        let non_finite = clamp_extension_popup_rect(anchor, f64::NAN, f64::INFINITY);
        assert_eq!(non_finite.width, 200.0);
        assert_eq!(non_finite.height, 150.0);
    }

    #[test]
    fn overlay_priority_stack_enforces_modal_fifo_and_strict_precedence() {
        let mut stack = OverlayPriorityStack::new();
        assert_eq!(stack.active_kind(), None);
        assert_eq!(stack.active_surface(), None);

        stack.hover_sidebar_open = true;
        assert_eq!(stack.active_kind(), Some(ActiveOverlayKind::HoverSidebar));
        assert_eq!(stack.active_surface(), Some(OverlaySurface::HoverSidebar));

        stack.reading_mode_open = true;
        assert_eq!(stack.active_kind(), Some(ActiveOverlayKind::ReadingMode));
        assert_eq!(stack.active_surface(), Some(OverlaySurface::ReadingMode));

        stack.find_in_page_open = true;
        assert_eq!(stack.active_kind(), Some(ActiveOverlayKind::FindInPage));
        assert_eq!(stack.active_surface(), Some(OverlaySurface::CommandPalette));

        // CommandPalette preempts FindInPage while preserving find_in_page_open underneath.
        stack.command_palette_open = true;
        assert_eq!(stack.active_kind(), Some(ActiveOverlayKind::CommandPalette));
        assert_eq!(stack.active_surface(), Some(OverlaySurface::CommandPalette));

        stack.push_modal("perm-1");
        stack.push_modal("perm-1"); // duplicate ignored
        stack.push_modal("dialog-2");
        assert_eq!(stack.active_kind(), Some(ActiveOverlayKind::Modal));
        assert_eq!(stack.active_surface(), Some(OverlaySurface::Modal));
        assert_eq!(stack.active_modal(), Some("perm-1"));
        assert_eq!(stack.modal_count(), 2);

        assert_eq!(stack.pop_front_modal().as_deref(), Some("perm-1"));
        assert_eq!(stack.active_modal(), Some("dialog-2"));
        assert_eq!(stack.active_kind(), Some(ActiveOverlayKind::Modal));
        assert_eq!(stack.active_surface(), Some(OverlaySurface::Modal));

        assert!(stack.resolve_modal("dialog-2"));
        assert!(!stack.resolve_modal("missing"));
        assert_eq!(stack.active_kind(), Some(ActiveOverlayKind::CommandPalette));
        assert_eq!(stack.active_surface(), Some(OverlaySurface::CommandPalette));

        stack.command_palette_open = false;
        assert_eq!(stack.active_kind(), Some(ActiveOverlayKind::FindInPage));
        assert_eq!(stack.active_surface(), Some(OverlaySurface::CommandPalette));

        stack.find_in_page_open = false;
        assert_eq!(stack.active_kind(), Some(ActiveOverlayKind::ReadingMode));
        assert_eq!(stack.active_surface(), Some(OverlaySurface::ReadingMode));

        stack.reading_mode_open = false;
        assert_eq!(stack.active_kind(), Some(ActiveOverlayKind::HoverSidebar));
        assert_eq!(stack.active_surface(), Some(OverlaySurface::HoverSidebar));
    }

    #[test]
    fn test_activation_guard_preserves_owned_and_oauth_popups() {
        let family = NyariWindowFamily {
            main_hwnd: 0x1000,
            shell_overlay_hwnd: Some(0x2000),
            glance_hwnd: Some(0x3000),
            extension_popup_hwnd: Some(0x4000),
            oauth_popup_hwnds: vec![0x5001, 0x5002],
        };

        // Clicking inside any owned popup or OAuth popup keeps everything intact.
        assert_eq!(
            classify_activation_change(Some(0x2000), &family, Some(OverlaySurface::CommandPalette)),
            ActivationAction::KeepAll
        );
        assert_eq!(
            classify_activation_change(Some(0x3000), &family, None),
            ActivationAction::KeepAll
        );
        assert_eq!(
            classify_activation_change(Some(0x4000), &family, None),
            ActivationAction::KeepAll
        );
        assert_eq!(
            classify_activation_change(Some(0x5002), &family, Some(OverlaySurface::Modal)),
            ActivationAction::KeepAll
        );

        // Clicking back to main_hwnd dismisses transient overlays while keeping Modal/ReadingMode/OAuth open.
        let to_main = classify_activation_change(
            Some(family.main_hwnd),
            &family,
            Some(OverlaySurface::CommandPalette),
        );
        assert_eq!(to_main, ActivationAction::DismissTransientToMain);

        let mut stack = OverlayPriorityStack {
            command_palette_open: true,
            find_in_page_open: true,
            reading_mode_open: true,
            hover_sidebar_open: true,
            ..OverlayPriorityStack::new()
        };
        stack.push_modal("perm-req-1");

        let effects = stack.apply_activation(to_main);
        assert!(effects.dismiss_command_palette);
        assert!(effects.dismiss_hover_sidebar);
        assert!(effects.dismiss_glance);
        assert!(effects.dismiss_extension_popup);
        assert!(effects.keep_modal);
        assert!(effects.keep_find_in_page);
        assert!(effects.keep_reading_mode);
        assert!(effects.keep_oauth_popups);
        assert!(!stack.command_palette_open);
        assert!(!stack.hover_sidebar_open);
        assert!(stack.find_in_page_open);
        assert!(stack.reading_mode_open);
        assert_eq!(stack.active_modal(), Some("perm-req-1"));
        assert!(should_keep_active_kind(ActiveOverlayKind::Modal, to_main));
        assert!(should_keep_active_kind(
            ActiveOverlayKind::FindInPage,
            to_main
        ));
        assert!(should_keep_active_kind(
            ActiveOverlayKind::ReadingMode,
            to_main
        ));
        assert!(!should_keep_active_kind(
            ActiveOverlayKind::CommandPalette,
            to_main
        ));
        assert!(!should_keep_active_kind(
            ActiveOverlayKind::HoverSidebar,
            to_main
        ));
        assert!(should_keep_surface_open(OverlaySurface::Modal, to_main));
        assert!(should_keep_surface_open(
            OverlaySurface::ReadingMode,
            to_main
        ));
        assert!(!should_keep_surface_open(
            OverlaySurface::CommandPalette,
            to_main
        ));
        assert!(!should_keep_surface_open(
            OverlaySurface::HoverSidebar,
            to_main
        ));

        // Verify apply_activation + should_keep_active_kind agree when FindInPage is the active surface.
        let mut find_stack = OverlayPriorityStack {
            find_in_page_open: true,
            command_palette_open: true,
            hover_sidebar_open: true,
            ..OverlayPriorityStack::new()
        };
        let find_effects = find_stack.apply_activation(to_main);
        assert!(find_effects.keep_find_in_page);
        assert_eq!(
            find_stack.active_kind(),
            Some(ActiveOverlayKind::FindInPage)
        );
        assert_eq!(
            find_stack.active_surface(),
            Some(OverlaySurface::CommandPalette)
        );
        assert!(should_keep_active_kind(
            find_stack.active_kind().unwrap(),
            to_main
        ));

        // Moving focus outside Nyari (Alt+Tab or None) hides transient overlays while preserving Modal, FindInPage & OAuth.
        let outside =
            classify_activation_change(Some(0x9999), &family, Some(OverlaySurface::CommandPalette));
        assert_eq!(outside, ActivationAction::HideTransientOutsideApp);
        assert_eq!(
            classify_activation_change(None, &family, Some(OverlaySurface::HoverSidebar)),
            ActivationAction::HideTransientOutsideApp
        );
        let outside_effects = evaluate_activation_effects(outside);
        assert!(outside_effects.dismiss_command_palette);
        assert!(outside_effects.dismiss_glance);
        assert!(outside_effects.keep_modal);
        assert!(outside_effects.keep_find_in_page);
        assert!(outside_effects.keep_oauth_popups);
        assert!(should_keep_active_kind(
            ActiveOverlayKind::FindInPage,
            outside
        ));
        assert!(!should_keep_active_kind(
            ActiveOverlayKind::CommandPalette,
            outside
        ));
    }

    #[test]
    fn win32_helpers_provide_expected_constants_and_work_area_clamping() {
        assert_eq!(handle_wm_erasebkgnd(), LRESULT(1));
        assert_eq!(get_ancestor_root(HWND::default()), None);
        assert!(!is_window_maximized(HWND::default()));

        let styles = create_owned_popup_styles();
        assert_eq!(
            styles.style,
            WINDOW_STYLE(WS_POPUP.0 | WS_CLIPCHILDREN.0 | WS_CLIPSIBLINGS.0)
        );
        assert_eq!(
            styles.ex_style,
            WINDOW_EX_STYLE(WS_EX_NOREDIRECTIONBITMAP.0 | WS_EX_TOOLWINDOW.0)
        );

        let proposed = RECT {
            left: -8,
            top: -8,
            right: 1928,
            bottom: 1048,
        };
        let work = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        assert_eq!(clamp_rect_to_work_area(proposed, work), work);
        let mut dummy_rect = proposed;
        assert!(!clamp_maximized_rect_to_monitor_work(
            HWND::default(),
            &mut dummy_rect
        ));
    }

    #[test]
    fn real_hwnd_win32_boundaries_apply_region_resolve_ancestor_and_clamp_work_area() {
        use crate::dpi::window_dpi;
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_CHILD, WS_OVERLAPPEDWINDOW,
        };

        struct HwndGuard(HWND);
        impl Drop for HwndGuard {
            fn drop(&mut self) {
                if !self.0.is_invalid() {
                    unsafe {
                        let _ = DestroyWindow(self.0);
                    }
                }
            }
        }

        let parent = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("nyari-test-parent"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                640,
                480,
                None,
                None,
                None,
                None,
            )
        }
        .expect("CreateWindowExW parent should succeed");
        let parent_guard = HwndGuard(parent);

        let child = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("nyari-test-child"),
                WS_CHILD,
                0,
                0,
                640,
                480,
                Some(parent_guard.0),
                None,
                None,
                None,
            )
        }
        .expect("CreateWindowExW child should succeed");

        assert!(window_dpi(parent_guard.0) >= 96);
        assert_eq!(get_ancestor_root(child), Some(parent_guard.0 .0 as isize));
        assert_eq!(
            get_ancestor_root(parent_guard.0),
            Some(parent_guard.0 .0 as isize)
        );

        let ring_rgn = build_resize_ring_region(640, 480, 6).unwrap();
        // Windows takes ownership of `ring_rgn` on success; do not DeleteObject afterwards.
        apply_window_region(parent_guard.0, ring_rgn, false)
            .expect("SetWindowRgn should accept valid HWND and region");

        update_resize_ring(child, 640, 480, 144, false)
            .expect("update_resize_ring restored should succeed");
        update_resize_ring(child, 640, 480, 144, true)
            .expect("update_resize_ring maximized hide should succeed");

        let mut oversized = RECT {
            left: -50,
            top: -50,
            right: 50_000,
            bottom: 50_000,
        };
        assert!(clamp_maximized_rect_to_monitor_work(
            parent_guard.0,
            &mut oversized
        ));
        assert!(oversized.right > oversized.left && oversized.right < 50_000);
        assert!(oversized.bottom > oversized.top && oversized.bottom < 50_000);
    }
}
