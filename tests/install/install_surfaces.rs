//! Coverage for the install-side inspection surfaces (CC1-R6-006 / F-029):
//! doctor, inspect, and content detection. windows_path predicates are
//! pub(crate); they are covered inline in src/windows_path.rs tests.

use std::fs;

use tokenzero_install::{
    InstallWrite, detect_present_agents, doctor, doctor_exit_codes, doctor_ls,
    inspect_client_surface,
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
