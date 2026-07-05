#![allow(dead_code)]

use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::{Command, Output};
use tempfile::{TempDir, tempdir};

/// The 11 required competitor adapter tool names.
pub fn required_adapter_tools() -> &'static [&'static str] {
    &[
        "rtk",
        "ztk",
        "lean-ctx",
        "tokenpak",
        "tokenjuice",
        "context-mode",
        "caveman",
        "headroom",
        "claw",
        "compresh",
        "context-gateway",
    ]
}

/// Build the 11 reviewed adapter rows used across adapter-approval tests.
pub fn reviewed_adapter_rows() -> Vec<Value> {
    required_adapter_tools()
        .iter()
        .map(|tool| {
            serde_json::json!({
                "tool": tool,
                "approval_status": "reviewed",
                "execution_allowed": true,
                "reviewed_command": format!("{tool} --version"),
                "blind_install_attempted": false
            })
        })
        .collect()
}

/// Run `tokenzero <args>` with deterministic agent env vars.
pub fn tokenzero_with_agent_env(args: &[&str]) -> Output {
    Command::cargo_bin("tokenzero")
        .unwrap()
        .args(args)
        .env("NO_COLOR", "1")
        .env("CI", "true")
        .env("TERM", "dumb")
        .env("SOURCE_DATE_EPOCH", "1234567890")
        .output()
        .unwrap()
}

/// Assert output contains no ANSI escape sequences.
pub fn assert_no_ansi(bytes: &[u8]) {
    assert!(
        !bytes.contains(&0x1b),
        "unexpected ANSI escape in output:\n{}",
        String::from_utf8_lossy(bytes)
    );
}

/// Set up a temp dir with cache path.
pub fn setup_temp_with_cache() -> (TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    (dir, cache)
}

/// Run `tokenzero <args> --json`, assert success, parse and return JSON value.
pub fn run_tokenzero_json(args: &[&str]) -> Value {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "tokenzero {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Run `tokenzero <args> --json` from a given directory, assert success, parse JSON.
pub fn run_tokenzero_json_in(args: &[&str], cwd: &std::path::Path) -> Value {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "tokenzero {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Run `tokenzero <args> --json` with env vars, assert success, parse JSON.
pub fn run_tokenzero_json_with_env(args: &[&str], envs: &[(&str, &str)]) -> Value {
    let mut cmd = Command::cargo_bin("tokenzero").unwrap();
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "tokenzero {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Write a JSON fixture to a specific path.
pub fn write_json_fixture(path: &std::path::Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

/// Create `results/current/` under root and write a JSON fixture file.
pub fn write_results_fixture(root: &std::path::Path, file_name: &str, value: &Value) {
    let results_dir = root.join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    write_json_fixture(&results_dir.join(file_name), value);
}

/// Set up a minimal handoff completion audit fixture on disk.
pub fn write_minimal_handoff_completion_audit(
    results_dir: &std::path::Path,
    release_candidate_id: &str,
) {
    write_json_fixture(
        &results_dir.join("tokenzero_completion_audit.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.completion_audit.v1",
            "release_candidate_id": release_candidate_id,
            "completion_achieved": false,
            "public_claims_approved": false,
            "release_publication_allowed": false,
            "residual_gate_matrix": []
        }),
    );
}

/// Assert that a JSON object's "reasons" array contains a specific reason string.
pub fn assert_reason(row: &Value, expected_reason: &str) {
    assert!(
        row["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == expected_reason),
        "missing reason {expected_reason:?} in {}",
        row["reasons"]
    );
}
