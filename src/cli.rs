use crate::NyariError;
use std::io::{BufRead, Write};

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

/// Runs the Phase-1 `--mcp-stdio` JSON-RPC stub over arbitrary `BufRead`/`Write` streams.
pub fn run_mcp_stdio_with_io<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
) -> Result<(), NyariError> {
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(val) => {
                let Some(id) = val.get("id").cloned() else {
                    continue;
                };
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32001,
                        "message": "nyari-mcp requires Phase 4 mcp_bridge or an active Nyari Browser pipe"
                    }
                })
            }
            Err(err) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": serde_json::Value::Null,
                "error": {
                    "code": -32700,
                    "message": format!("Parse error: {err}")
                }
            }),
        };

        let serialized = serde_json::to_string(&response)?;
        if let Err(err) = writeln!(writer, "{serialized}").and_then(|()| writer.flush()) {
            if err.kind() == std::io::ErrorKind::BrokenPipe {
                break;
            }
            return Err(err.into());
        }
    }

    Ok(())
}

/// Runs the Phase-1 `--mcp-stdio` JSON-RPC stub over process `stdin`/`stdout`.
pub fn run_mcp_stdio_stub() -> Result<(), NyariError> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    run_mcp_stdio_with_io(stdin.lock(), stdout.lock())
}

#[cfg(test)]
mod tests {
    use super::{parse_args, run_mcp_stdio_with_io, LaunchMode};
    use serde_json::Value;
    use std::io::Cursor;

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

    #[test]
    fn run_mcp_stdio_with_io_returns_json_rpc_error_line_for_request() {
        let input = Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n");
        let mut output = Vec::new();
        run_mcp_stdio_with_io(input, &mut output).expect("stdio stub should not fail");

        let text = String::from_utf8(output).expect("valid UTF-8 JSON-RPC output");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1);
        let parsed: Value = serde_json::from_str(lines[0]).expect("valid JSON response");
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 1);
        assert!(parsed["error"]["code"].is_i64());
        let msg = parsed["error"]["message"]
            .as_str()
            .expect("error message must be string");
        assert!(!msg.is_empty());
    }

    #[test]
    fn run_mcp_stdio_with_io_handles_invalid_json_and_blank_lines() {
        let input = Cursor::new(
            b"\n   \n{not-json\n{\"jsonrpc\":\"2.0\",\"id\":\"req-2\",\"method\":\"tools/list\"}\n",
        );
        let mut output = Vec::new();
        run_mcp_stdio_with_io(input, &mut output).expect("stdio stub should tolerate invalid JSON");

        let text = String::from_utf8(output).expect("valid UTF-8 output");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);

        let err1: Value = serde_json::from_str(lines[0]).expect("first line JSON");
        assert_eq!(err1["jsonrpc"], "2.0");
        assert!(err1["id"].is_null());
        assert_eq!(err1["error"]["code"], -32700);

        let err2: Value = serde_json::from_str(lines[1]).expect("second line JSON");
        assert_eq!(err2["jsonrpc"], "2.0");
        assert_eq!(err2["id"], "req-2");
        assert!(err2["error"]["code"].is_i64());
    }
}
