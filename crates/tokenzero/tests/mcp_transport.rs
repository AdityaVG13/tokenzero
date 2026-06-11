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
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, "{{bad json").unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}})
        )
        .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Parse error"));
    assert!(stdout.contains("tools"));
}

#[test]
fn mcp_server_handles_mixed_framed_and_unframed_transcript() {
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
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        let framed = serde_json::json!({"jsonrpc":"2.0","id":"framed","method":"ping"});
        let framed = framed.to_string();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", framed.len(), framed).unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":"line","method":"tools/list","params":{}})
        )
        .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let marker = b"\r\n\r\n";
    let header_end = output
        .stdout
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("missing framed response header terminator");
    let header = std::str::from_utf8(&output.stdout[..header_end]).unwrap();
    let length = header
        .strip_prefix("Content-Length: ")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_else(|| panic!("invalid framed response header: {header}"));
    let body_start = header_end + marker.len();
    let body_end = body_start + length;
    let framed: Value = serde_json::from_slice(&output.stdout[body_start..body_end]).unwrap();
    assert_eq!(framed["id"], "framed");
    assert!(framed["result"].is_object());

    let line: Value = serde_json::from_slice(&output.stdout[body_end..]).unwrap();
    assert_eq!(line["id"], "line");
    assert!(line["result"]["tools"].as_array().unwrap().len() > 5);
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
