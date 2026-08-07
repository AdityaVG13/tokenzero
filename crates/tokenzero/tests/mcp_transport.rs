use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use tempfile::tempdir;
macro_rules! rpc {
    (initialize, $id:expr) => { json!({"jsonrpc":"2.0", "id":$id, "method":"initialize", "params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"tokenzero-mcp-smoke","version":"1.0.0"}}}).to_string() }; (tools, $id:expr) => { json!({"jsonrpc":"2.0", "id":$id, "method":"tools/list", "params":{}}).to_string() };
}
macro_rules! checks {
    ($($condition:expr => $message:literal $(, $argument:expr)*;)+) => {$(
        assert!($condition, $message $(, $argument)*);
    )+};
}
fn compatibility_mcp_bin() -> &'static PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("target"))
            .join("mcp-transport-compat");
        let status = Command::new("cargo")
            .args([
                "build",
                "-p",
                "tokenzero-cli",
                "--bin",
                "tokenzero",
                "--no-default-features",
                "--features",
                "surface-mcp",
            ])
            .env("CARGO_TARGET_DIR", &target)
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success(), "build explicit MCP compatibility CLI");
        target
            .join("debug")
            .join(format!("tokenzero{}", std::env::consts::EXE_SUFFIX))
    })
}

fn feed(lines: &[&str]) -> Output {
    let dir = tempdir().unwrap();
    let mut child = Command::new(compatibility_mcp_bin())
        .args([
            "mcp-server",
            "--allowed-root",
            dir.path().to_str().unwrap(),
            "--cache-path",
            dir.path().join("cache.json").to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    for line in lines {
        writeln!(child.stdin.as_mut().unwrap(), "{line}").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    checks! { output.status.success() => "{}", String::from_utf8_lossy(&output.stderr); }
    output
}
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
#[test]
fn mcp_server_survives_malformed_json() {
    let init = rpc!(initialize, Value::from(1));
    let tools = rpc!(tools, Value::from(2));
    let output = feed(&["{bad json", &init, INITIALIZED, &tools]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    checks! { stderr.contains("Parse error") || stderr.contains("JSON error") => "expected parse error in stderr, got: {stderr}"; }
    assert!(stdout.contains("tools"));
}
#[test]
fn mcp_server_handles_ndjson_transcript() {
    let init = rpc!(initialize, Value::from("first"));
    let tools = rpc!(tools, Value::from("second"));
    let output = feed(&[&init, INITIALIZED, &tools]);
    let lines: Vec<_> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .collect();
    checks! { lines.len() >= 2 => "expected at least 2 NDJSON lines, got {}", lines.len(); }
    let first: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["id"], "first");
    assert!(first["result"].is_object());
    assert_eq!(first["result"]["protocolVersion"], "2024-11-05");
    let second: Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(second["id"], "second");
    assert!(second["result"]["tools"].as_array().unwrap().len() > 5);
}
#[test]
fn mcp_smoke_verifies_initialize_success() {
    let output = Command::new(compatibility_mcp_bin())
        .args(["mcp-smoke", "--json"])
        .output()
        .unwrap();
    checks! { output.status.success() => "{}", String::from_utf8_lossy(&output.stderr); }
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true, "{json}");
    assert_eq!(json["initialize_successes_observed"], 1, "{json}");
    assert_eq!(json["initialize_failures"], 0, "{json}");
}

#[test]
fn canonical_default_cli_fails_loud_for_mcp_host_commands() {
    for command in ["mcp-server", "mcp-smoke"] {
        let output = Command::cargo_bin("tokenzero")
            .unwrap()
            .arg(command)
            .output()
            .unwrap();
        assert!(!output.status.success(), "default CLI hosted {command}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("surface-mcp"), "{command}: {stderr}");
    }
}
