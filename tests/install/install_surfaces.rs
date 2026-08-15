//! Coverage for the install-side inspection surfaces (CC1-R6-006 / F-029):
//! doctor, inspect, and content detection. windows_path predicates are
//! pub(crate); they are covered inline in src/windows_path.rs tests.

use std::fs;

use tokenzero_install::{
    detect_present_agents, doctor, doctor_exit_codes, doctor_fix, doctor_ls, doctor_undo,
    inspect_client_surface, InstallWrite,
};

// ---------- doctor ----------

#[test]
fn doctor_reports_missing_root_as_finding() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("does-not-exist");
    let report = doctor(&missing, None);
    let findings = report["findings"].as_array().expect("findings array");
    assert!(
        findings.iter().any(|f| f["id"] == "tz-root-missing"),
        "missing root must produce tz-root-missing: {report}"
    );
    assert_eq!(
        report["mcp"]["ready"], false,
        "missing root must not claim MCP ready: {report}"
    );
}

#[test]
fn doctor_on_healthy_root_has_no_missing_root_finding() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tz");
    fs::create_dir_all(&root).unwrap();
    let report = doctor(&root, None);
    let findings = report["findings"].as_array().expect("findings array");
    assert!(
        !findings.iter().any(|f| f["id"] == "tz-root-missing"),
        "healthy root must not report tz-root-missing: {report}"
    );
    assert_eq!(
        report["mcp"]["ready"], true,
        "healthy root with default launch config must report MCP ready: {report}"
    );
}

#[test]
fn doctor_pins_minimum_agent_envelope_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let report = doctor(&tmp.path().join("does-not-exist"), None);
    assert_eq!(
        serde_json::json!({
            "schema_version": report["schema_version"],
            "status": report["status"],
            "tool": report["tool"],
            "ack": report["ack"],
        }),
        serde_json::json!({
            "schema_version": "tokenzero.doctor.v1",
            "status": "blocked",
            "tool": "doctor",
            "ack": "blocked",
        })
    );
}

#[test]
fn doctor_ls_is_stable_envelope() {
    let tmp = tempfile::tempdir().unwrap();
    let report = doctor_ls(tmp.path());
    assert!(
        report.is_object(),
        "doctor ls must return a JSON object envelope"
    );
}

#[test]
fn doctor_exit_codes_envelope_is_nonempty() {
    let report = doctor_exit_codes();
    let text = report.to_string();
    // The envelope must document the blocking and usage-error codes at minimum.
    assert!(text.contains("\"code\":0") || text.contains("0"), "{text}");
    assert!(text.contains("usage_error"), "{text}");
    assert!(text.contains("blocked"), "{text}");
}

#[test]
fn doctor_undo_latest_missing_is_failed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tz");
    fs::create_dir_all(&root).unwrap();
    let report = doctor_undo(&root, "latest");
    assert_eq!(report["status"], "failed");
    assert_eq!(report["exit_code"], 3);
    assert!(
        report["error"]
            .as_str()
            .unwrap_or("")
            .contains("could not resolve latest run"),
        "{report}"
    );
}

#[test]
fn doctor_undo_missing_actions_is_failed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tz");
    fs::create_dir_all(&root).unwrap();
    let report = doctor_undo(&root, "no-such-run");
    assert_eq!(report["status"], "failed");
    assert_eq!(report["exit_code"], 3);
    assert!(
        report["error"]
            .as_str()
            .unwrap_or("")
            .contains("could not read actions.jsonl"),
        "{report}"
    );
}

#[test]
fn doctor_undo_restores_empty_cache_parent_and_refuses_nonempty() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tz");
    fs::create_dir_all(&root).unwrap();

    let fix = doctor_fix(&root, None, false);
    assert_eq!(fix["status"], "ok", "{fix}");
    let run_id = fix["run_id"].as_str().expect("run_id").to_string();
    assert!(root.join(".tokenzero").is_dir());

    let undo = doctor_undo(&root, &run_id);
    assert_eq!(undo["status"], "ok", "{undo}");
    assert!(!root.join(".tokenzero").exists());
    let undo_path = root.join(".doctor/runs").join(&run_id).join("undo.json");
    assert!(
        undo_path.is_file(),
        "successful undo must persist undo.json"
    );
    let listed = doctor_ls(&root);
    let runs = listed["runs"].as_array().expect("runs");
    assert!(
        runs.iter()
            .any(|run| run["run_id"] == run_id && run["has_undo"] == true),
        "doctor ls must observe the persisted undo artifact: {listed}"
    );

    let fix2 = doctor_fix(&root, None, false);
    assert_eq!(fix2["status"], "ok", "{fix2}");
    let run_id2 = fix2["run_id"].as_str().expect("run_id").to_string();
    fs::write(root.join(".tokenzero").join("keep.txt"), b"x").unwrap();
    let undo2 = doctor_undo(&root, &run_id2);
    assert_eq!(undo2["status"], "failed", "{undo2}");
    assert_eq!(undo2["exit_code"], 3);
    assert_eq!(
        undo2["reason"],
        "created directory is no longer empty; refusing to move later user data"
    );
    assert!(root.join(".tokenzero").join("keep.txt").exists());
}

#[test]
fn doctor_undo_reports_partial_when_undo_artifact_cannot_be_written() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tz");
    fs::create_dir_all(&root).unwrap();

    let fix = doctor_fix(&root, None, false);
    assert_eq!(fix["status"], "ok", "{fix}");
    let run_id = fix["run_id"].as_str().expect("run_id").to_string();
    let run_dir = root.join(".doctor/runs").join(&run_id);
    fs::create_dir(run_dir.join("undo.json")).unwrap();

    let undo = doctor_undo(&root, &run_id);
    assert_eq!(undo["status"], "partial", "{undo}");
    assert_eq!(undo["ok"], false, "{undo}");
    assert_eq!(undo["exit_code"], 2, "{undo}");
    let errors = undo["artifact_errors"].as_array().expect("artifact_errors");
    assert!(
        errors.iter().any(|error| error["artifact"] == "undo"),
        "must name the undo artifact that failed to persist: {undo}"
    );
    assert!(
        !root.join(".tokenzero").exists(),
        "directory restore still happens; the report must not call that full success"
    );
}

// ---------- inspect ----------

fn sample_write(path: &std::path::Path) -> InstallWrite {
    InstallWrite {
        path: path.display().to_string(),
        action: "write".to_string(),
        backup_id: String::new(),
        capability: "mcp-config".to_string(),
        global: false,
    }
}

#[test]
fn inspect_missing_path_reports_missing_state() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join(".tokenzero/mcp-server.json");
    let status = inspect_client_surface(&sample_write(&target), tmp.path());
    assert!(!status.exists);
    assert!(!status.installed);
    assert_eq!(status.state, "missing");
}

// ---------- content ----------

#[test]
fn detect_present_agents_finds_config_dir_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    // Claude Code is detected via ~/.claude config presence.
    fs::create_dir_all(home.join(".claude")).unwrap();
    let agents = detect_present_agents(home, Some(""));
    assert!(
        agents.iter().any(|a| a.agent.contains("claude")),
        "expected claude detection from ~/.claude dir: {agents:?}"
    );
}

#[test]
fn detect_present_agents_empty_home_detects_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let agents = detect_present_agents(tmp.path(), Some(""));
    assert!(
        agents.is_empty(),
        "empty home and empty PATH must detect no agents: {agents:?}"
    );
}
