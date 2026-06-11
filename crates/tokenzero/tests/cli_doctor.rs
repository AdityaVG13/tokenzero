use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn cli_doctor_json_exposes_agent_contract() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "doctor",
            "--root",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.doctor.v1");
    assert_eq!(json["ok"], true);
    assert_eq!(json["mutates"], false);
    assert!(json["findings"].as_array().unwrap().is_empty());
    assert_eq!(json["capabilities"]["supports_fix"], true);
    assert_eq!(
        json["capabilities"]["commands"].as_array().unwrap()[0]["mutates"],
        false
    );
    assert!(
        json["exit_codes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["code"] == 1 && row["label"] == "blocked")
    );
    assert_eq!(json["doctor_contract"]["default_read_only"], true);
    assert!(
        json["robot_docs"]["recommended_invocation"]
            .as_str()
            .unwrap()
            .contains("doctor --json")
    );
}

#[test]
fn cli_doctor_missing_root_exits_nonzero_with_finding() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("missing");

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["doctor", "--root", missing.to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "blocked");
    assert!(
        json["findings"].as_array().unwrap().iter().any(|finding| {
            finding["id"] == "tz-root-missing" && finding["severity"] == "error"
        })
    );
    assert!(
        json["next_steps"].as_array().unwrap()[0]["command"]
            .as_str()
            .unwrap()
            .contains("--root <existing-directory>")
    );
}

#[test]
fn cli_doctor_health_prints_one_line_summary() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "doctor",
            "health",
            "--root",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.starts_with("ok tokenzero="), "{stdout}");
    assert!(stdout.contains("findings=0"), "{stdout}");
}

#[test]
fn cli_doctor_capabilities_subcommand_exposes_contract() {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["doctor", "capabilities", "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.doctor.capabilities.v1");
    assert_eq!(json["supports_fix"], true);
    assert_eq!(json["supports_undo"], true);
    assert!(json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == "doctor health" && row["mutates"] == false && row["json"] == true
    }));
    assert!(
        json["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| { row["name"] == "doctor robot-docs" && row["mutates"] == false })
    );
    assert!(
        json["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| { row["name"] == "doctor ls --json" && row["mutates"] == false })
    );
    assert!(
        json["fixers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "tz-cache-parent-missing" && row["undo"] == true)
    );
}

#[test]
fn cli_doctor_robot_docs_subcommand_is_paste_ready() {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["doctor", "robot-docs"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("# TokenZero Doctor Robot Guide"));
    assert!(stdout.contains("EXIT CODES"));
    assert!(stdout.contains("This doctor will NEVER do"));
    assert!(stdout.contains("tokenzero doctor capabilities --json"));
}

#[test]
fn cli_doctor_explain_known_finding_without_current_failure() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "doctor",
            "explain",
            "tz-root-missing",
            "--root",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.doctor.explain.v1");
    assert_eq!(json["current"], false);
    assert_eq!(json["finding"]["id"], "tz-root-missing");
    assert!(
        json["finding"]["remediation"]["command"]
            .as_str()
            .unwrap()
            .contains("--root <existing-directory>")
    );
}

#[test]
fn cli_doctor_robot_triage_returns_next_command() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("missing");

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "doctor",
            "--robot-triage",
            "--root",
            missing.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.doctor.robot_triage.v1");
    assert_eq!(json["ok"], false);
    assert_eq!(
        json["recommended_command"],
        "tokenzero doctor --json --root <existing-directory>"
    );
    assert!(json["actions_planned"].as_array().unwrap().is_empty());
}

#[test]
fn cli_doctor_robot_triage_plans_fixable_cache_parent() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join(".tokenzero/recovery-cache.json");

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "doctor",
            "--robot-triage",
            "--root",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.doctor.robot_triage.v1");
    assert_eq!(
        json["recommended_command"],
        "tokenzero doctor --dry-run --fix --json"
    );
    let planned = json["actions_planned"].as_array().unwrap();
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0]["finding_id"], "tz-cache-parent-missing");
}

#[test]
fn cli_doctor_dry_run_fix_plans_cache_parent_without_writing() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join(".tokenzero/recovery-cache.json");
    let parent = cache.parent().unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "doctor",
            "--dry-run",
            "--fix",
            "--root",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.doctor.fix.v1");
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["mutates"], false);
    assert_eq!(json["actions_taken"], 0);
    assert!(!parent.exists());
}

#[test]
fn cli_doctor_fix_is_idempotent_and_undo_restores_cache_parent_absence() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join(".tokenzero/recovery-cache.json");
    let parent = cache.parent().unwrap().to_path_buf();

    let fixed = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "doctor",
            "fix",
            "--root",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        fixed.status.success(),
        "{}",
        String::from_utf8_lossy(&fixed.stderr)
    );
    let fixed_json: Value = serde_json::from_slice(&fixed.stdout).unwrap();
    assert_eq!(fixed_json["actions_taken"], 1);
    assert!(parent.is_dir());
    let run_id = fixed_json["run_id"].as_str().unwrap().to_string();

    let second = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "doctor",
            "--fix",
            "--root",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_json: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second_json["actions_taken"], 0);

    let undone = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "doctor",
            "undo",
            &run_id,
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        undone.status.success(),
        "{}",
        String::from_utf8_lossy(&undone.stderr)
    );
    let undo_json: Value = serde_json::from_slice(&undone.stdout).unwrap();
    assert_eq!(undo_json["schema_version"], "tokenzero.doctor.undo.v1");
    assert_eq!(undo_json["ok"], true);
    assert!(!parent.exists());
}

#[test]
fn cli_doctor_ls_lists_run_artifacts() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join(".tokenzero/recovery-cache.json");

    let fixed = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "doctor",
            "fix",
            "--root",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        fixed.status.success(),
        "{}",
        String::from_utf8_lossy(&fixed.stderr)
    );
    let fixed_json: Value = serde_json::from_slice(&fixed.stdout).unwrap();
    let run_id = fixed_json["run_id"].as_str().unwrap();

    let listing = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "doctor",
            "ls",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        listing.status.success(),
        "{}",
        String::from_utf8_lossy(&listing.stderr)
    );
    let json: Value = serde_json::from_slice(&listing.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.doctor.ls.v1");
    assert_eq!(json["run_count"], 1);
    assert_eq!(json["runs"][0]["run_id"], run_id);
    assert_eq!(json["runs"][0]["latest"], true);
    assert_eq!(json["runs"][0]["action_count"], 1);
    assert!(
        json["runs"][0]["undo_command"]
            .as_str()
            .unwrap()
            .contains("tokenzero doctor undo")
    );
}
