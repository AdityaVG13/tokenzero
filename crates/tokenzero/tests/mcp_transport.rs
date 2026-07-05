use assert_cmd::prelude::*;
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::tempdir;

#[test]
fn mcp_server_survives_malformed_json() {
    let dir = tempdir().unwrap();
    let mut child = Command::cargo_bin("tokenzero")
        .unwrap()
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
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, "{{bad json").unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"tokenzero-mcp-smoke","version":"1.0.0"}}})
        )
        .unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})
        )
        .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // FastMCP emits parse/codec errors to stderr, not stdout.
    assert!(
        stderr.contains("Parse error") || stderr.contains("JSON error"),
        "expected parse error in stderr, got: {stderr}"
    );
    assert!(stdout.contains("tools"));
}

#[test]
fn mcp_server_handles_ndjson_transcript() {
    let dir = tempdir().unwrap();
    let mut child = Command::cargo_bin("tokenzero")
        .unwrap()
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
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":"first","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"tokenzero-mcp-smoke","version":"1.0.0"}}})
        )
        .unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":"second","method":"tools/list","params":{}})
        )
        .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 2,
        "expected at least 2 NDJSON lines, got {}",
        lines.len()
    );

    // First line: initialize response
    let first: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["id"], "first");
    assert!(first["result"].is_object());
    assert_eq!(first["result"]["protocolVersion"], "2024-11-05");

    // Second line: tools/list response
    let second: Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(second["id"], "second");
    assert!(second["result"]["tools"].as_array().unwrap().len() > 5);
}

#[test]
fn mcp_smoke_verifies_initialize_success() {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["mcp-smoke", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true, "{json}");
    assert_eq!(json["initialize_successes_observed"], 1, "{json}");
    assert_eq!(json["initialize_failures"], 0, "{json}");
}
