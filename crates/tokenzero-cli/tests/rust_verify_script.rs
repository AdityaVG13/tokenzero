#![cfg(unix)]

use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn rust_verify_script() -> PathBuf {
    repo_root().join("scripts/rust_verify.sh")
}

#[test]
fn rust_verify_dry_run_robot_json_can_be_written_to_report() {
    let temp = tempdir().unwrap();
    let report = temp.path().join("nested/rust_verify.json");

    let output = Command::new(rust_verify_script())
        .current_dir(repo_root())
        .args(["--dry-run", "--robot", "--output-json"])
        .arg(&report)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout_json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let report_json: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();

    assert_eq!(stdout_json, report_json);
    assert_eq!(stdout_json["schema_version"], "tokenzero.rust_verify.v1");
    assert_eq!(stdout_json["dry_run"], true);
    assert_eq!(stdout_json["success"], true);

    let commands = stdout_json["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 4);
    assert!(
        commands
            .iter()
            .any(|row| row["command"] == "cargo test --workspace")
    );
    assert!(
        commands
            .iter()
            .any(|row| row["command"] == "cargo clippy --workspace --all-targets -- -D warnings")
    );
}

#[test]
fn rust_verify_robot_json_reports_first_failing_step() {
    let temp = tempdir().unwrap();
    let fake_bin = temp.path().join("bin");
    fs::create_dir(&fake_bin).unwrap();

    let fake_cargo = fake_bin.join("cargo");
    fs::write(
        &fake_cargo,
        "#!/bin/sh\nprintf 'fake cargo failed\\n' >&2\nexit 17\n",
    )
    .unwrap();
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755)).unwrap();

    let report = temp.path().join("rust_verify_failure.json");
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = Command::new(rust_verify_script())
        .current_dir(repo_root())
        .env("PATH", path)
        .args(["--robot", "--output-json"])
        .arg(&report)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));

    let stdout_json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let report_json: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(stdout_json, report_json);
    assert_eq!(stdout_json["schema_version"], "tokenzero.rust_verify.v1");
    assert_eq!(stdout_json["dry_run"], false);
    assert_eq!(stdout_json["success"], false);

    let steps = stdout_json["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["name"], "fmt");
    assert_eq!(steps[0]["command"], "cargo fmt --all -- --check");
    assert_eq!(steps[0]["exit_code"], 17);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("running fmt: cargo fmt --all -- --check"));
    assert!(stderr.contains("fake cargo failed"));
}
