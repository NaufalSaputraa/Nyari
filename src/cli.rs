/// Parsed startup mode for `nyari.exe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    Browser { urls: Vec<String> },
    McpStdio,
}

/// Parses command-line arguments (`args[0]` is expected to be the executable path).
///
/// Returns `LaunchMode::McpStdio` if `--mcp-stdio` is present anywhere in `args[1..]`.
/// Otherwise collects non-flag URL/path arguments into `LaunchMode::Browser { urls }`.
pub fn parse_args(args: &[String]) -> LaunchMode {
    let tail = args.get(1..).unwrap_or(&[]);
    if tail.iter().any(|arg| arg == "--mcp-stdio") {
        return LaunchMode::McpStdio;
    }

    let urls = tail
        .iter()
        .filter(|arg| !arg.is_empty() && !arg.starts_with('-'))
        .cloned()
        .collect();
    LaunchMode::Browser { urls }
}

#[cfg(test)]
mod tests {
    use super::{parse_args, LaunchMode};

    #[test]
    fn parse_args_empty_returns_browser_with_no_urls() {
        assert_eq!(parse_args(&[]), LaunchMode::Browser { urls: Vec::new() });
    }

    #[test]
    fn parse_args_exe_only_returns_browser_with_no_urls() {
        let args = vec!["nyari.exe".to_string()];
        assert_eq!(parse_args(&args), LaunchMode::Browser { urls: Vec::new() });
    }

    #[test]
    fn parse_args_mcp_stdio_flag_returns_mcp_mode() {
        let args = vec!["nyari.exe".to_string(), "--mcp-stdio".to_string()];
        assert_eq!(parse_args(&args), LaunchMode::McpStdio);

        let mixed = vec![
            "nyari.exe".to_string(),
            "https://example.com".to_string(),
            "--mcp-stdio".to_string(),
        ];
        assert_eq!(parse_args(&mixed), LaunchMode::McpStdio);
    }

    #[test]
    fn parse_args_mcp_stdio_in_arg0_only_does_not_trigger_mcp() {
        let args = vec!["--mcp-stdio".to_string()];
        assert_eq!(parse_args(&args), LaunchMode::Browser { urls: Vec::new() });
    }

    #[test]
    fn parse_args_collects_non_flag_urls_and_skips_flags() {
        let args = vec![
            "nyari.exe".to_string(),
            "--new-window".to_string(),
            "https://github.com".to_string(),
            "-flag".to_string(),
            "https://example.com".to_string(),
        ];
        assert_eq!(
            parse_args(&args),
            LaunchMode::Browser {
                urls: vec![
                    "https://github.com".to_string(),
                    "https://example.com".to_string(),
                ],
            }
        );
    }
}
