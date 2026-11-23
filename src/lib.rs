use std::fmt;

pub mod backdrop;
pub mod cli;
pub mod dpi;
pub mod environment;
pub mod ipc;
pub mod layout;
pub mod single_instance;
pub mod storage;

/// Unified error type shared across Nyari Browser subsystems.
#[derive(Debug)]
pub enum NyariError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Win32(windows::core::Error),
    Sqlite(rusqlite::Error),
    InvalidInput(String),
    Other(String),
}

impl fmt::Display for NyariError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON error: {err}"),
            Self::Win32(err) => write!(f, "Win32 error: {err}"),
            Self::Sqlite(err) => write!(f, "SQLite error: {err}"),
            Self::InvalidInput(msg) => write!(f, "Invalid input: {msg}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for NyariError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Json(err) => Some(err),
            Self::Win32(err) => Some(err),
            Self::Sqlite(err) => Some(err),
            Self::InvalidInput(_) | Self::Other(_) => None,
        }
    }
}

impl From<std::io::Error> for NyariError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for NyariError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl From<windows::core::Error> for NyariError {
    fn from(err: windows::core::Error) -> Self {
        Self::Win32(err)
    }
}

impl From<rusqlite::Error> for NyariError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

#[cfg(test)]
mod tests {
    use super::NyariError;
    use std::error::Error;

    #[test]
    fn nyari_error_conversions_preserve_source_and_display() {
        let io_err: NyariError =
            std::io::Error::new(std::io::ErrorKind::NotFound, "missing").into();
        assert!(io_err.source().is_some());
        assert!(io_err.to_string().contains("missing"));

        let json_err: NyariError = serde_json::from_str::<serde_json::Value>("{bad")
            .unwrap_err()
            .into();
        assert!(json_err.source().is_some());

        let win_err: NyariError =
            windows::core::Error::from_hresult(windows::core::HRESULT(-1)).into();
        assert!(win_err.source().is_some());

        let sqlite_err: NyariError = rusqlite::Error::InvalidQuery.into();
        assert!(sqlite_err.source().is_some());

        let invalid = NyariError::InvalidInput("bad arg".into());
        assert!(invalid.source().is_none());
        assert_eq!(invalid.to_string(), "Invalid input: bad arg");
    }

    #[test]
    fn embedded_manifest_enables_per_monitor_v2_dpi_awareness() {
        use windows::Win32::UI::HiDpi::{
            AreDpiAwarenessContextsEqual, GetThreadDpiAwarenessContext,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        let is_pmv2 = unsafe {
            AreDpiAwarenessContextsEqual(
                GetThreadDpiAwarenessContext(),
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            )
            .as_bool()
        };
        assert!(
            is_pmv2,
            "expected PerMonitorV2 DPI awareness from embedded app.manifest"
        );
    }

    #[test]
    fn webview2_loader_links_statically() {
        let mut cmp = 1;
        unsafe {
            webview2_com::Microsoft::Web::WebView2::Win32::CompareBrowserVersions(
                windows::core::w!("1.0.0"),
                windows::core::w!("1.0.0"),
                &mut cmp,
            )
        }
        .expect("CompareBrowserVersions from WebView2LoaderStatic should succeed");
        assert_eq!(cmp, 0);
    }
}
