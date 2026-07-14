mod common;
use common::*;

use serde_json::Value;
use std::path::Path;
use tempfile::tempdir;

fn version_commands() -> Vec<Value> {
    required_adapter_tools().iter().map(|tool| {
        serde_json::json!({"tool": tool, "reviewed": true, "command": format!("{tool} --version")})
    }).collect()
}

fn write_approval(path: &Path, schema: &str, commands: Vec<Value>) {
    write_json_fixture(path, &serde_json::json!({"schema_version": schema, "commands": commands}));
}

fn audit(dir: &Path, extra: &[&str]) -> (Value, Value) {
    let out = dir.join("adapter-approval.json");
    let mut args = vec!["adapter-approval-audit", "--output-json", out.to_str().unwrap(), "--json"];
    args.extend_from_slice(extra);
    let stdout = run_tokenzero_json(&args);
    let disk = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    (disk, stdout)
}

fn audit_commands(schema: &str, commands: Vec<Value>, approve: bool) -> Value {
    let dir = tempdir().unwrap();
    let approval = dir.path().join("approval.json");
    write_approval(&approval, schema, commands);
    let mut extra = vec!["--approval-file", approval.to_str().unwrap()];
    if approve { extra.push("--execution-approval"); }
    audit(dir.path(), &extra).0
}

fn reason_has(report: &Value, needle: &str) -> bool {
    report["blocked_reasons"].as_array().unwrap().iter()
        .any(|r| r.as_str().unwrap().contains(needle))
}

fn row<'a>(report: &'a Value, tool: &str) -> &'a Value {
    find_row_by(report["adapters"].as_array().unwrap(), "tool", tool)
}

#[test]
fn cli_adapter_approval_audit_blocks_execution_without_reviewed_commands() {
    let dir = tempdir().unwrap();
    let (report, stdout) = audit(dir.path(), &[]);
    assert_eq!(report["schema_version"], "tokenzero.adapter_approval_audit.v1");
    assert_eq!(stdout["schema_version"], report["schema_version"]);
    assert_eq!(report["execution_allowed"], false);
    assert_eq!(report["blind_install_attempted"], false);
    assert_eq!(report["public_claims_approved"], false);
    assert_eq!(report["required_adapter_count"], 11);
    assert_eq!(report["reviewed_command_count"], 0);
    assert_eq!(report["missing_reviewed_command_count"], 11);
    assert_eq!(report["command_safety_policy"]["schema_version"], "tokenzero.adapter_command_safety.v1");
    assert!(report["command_safety_policy"]["blocked_side_effects"].as_array().unwrap()
        .iter().any(|item| item == "package_manager_install"));
    assert!(reason_has(&report, "reviewed competitor commands missing"));
    for tool in required_adapter_tools() {
        let adapter = row(&report, tool);
        assert_eq!(adapter["execution_allowed"], false, "{tool}");
        assert_eq!(adapter["approval_status"], "missing_reviewed_command", "{tool}");
        assert_eq!(adapter["blind_install_attempted"], false, "{tool}");
    }
}

#[test]
fn cli_adapter_approval_template_prepares_reviewed_commands_without_execution() {
    let dir = tempdir().unwrap();
    let template_path = dir.path().join("adapter-approval-template.json");
    let template = run_tokenzero_json(&[
        "adapter-approval-template", "--output-json", template_path.to_str().unwrap(), "--json",
    ]);
    assert_eq!(template["schema_version"], "tokenzero.adapter_approval_file.v1");
    assert_eq!(template["public_claims_approved"], false);
    assert_eq!(template["execution_approval_required"], true);
    assert_eq!(template["commands"].as_array().unwrap().len(), 11);
    assert!(template["commands"].as_array().unwrap().iter().all(|r| {
        r["reviewed"] == true && r["command"].as_str().unwrap().ends_with(" --version")
            && !r["command"].as_str().unwrap().contains("install")
    }));
    let report = audit(dir.path(), &["--approval-file", template_path.to_str().unwrap()]).0;
    assert_eq!(report["reviewed_command_count"], 11);
    assert_eq!(report["missing_reviewed_command_count"], 0);
    assert_eq!(report["unsafe_command_count"], 0);
    assert_eq!(report["duplicate_command_count"], 0);
    assert_eq!(report["execution_approval_granted"], false);
    assert_eq!(report["execution_allowed"], false);
    assert_eq!(report["public_claims_approved"], false);
    assert!(report["blocked_reasons"].as_array().unwrap().iter()
        .any(|reason| reason == "explicit runnable adapter execution approval not granted"));
}

#[test]
fn cli_adapter_approval_audit_rejects_malformed_approval_file() {
    let report = audit_commands("wrong.schema.v1", version_commands(), false);
    assert_eq!(report["reviewed_command_count"], 0);
    assert_eq!(report["missing_reviewed_command_count"], 11);
    assert_eq!(report["execution_allowed"], false);
    assert!(reason_has(&report, "adapter approval file schema invalid"));
}

#[test]
fn cli_adapter_approval_audit_allows_reviewed_commands_only_with_explicit_approval() {
    let report = audit_commands("tokenzero.adapter_approval_file.v1", version_commands(), true);
    assert_eq!(report["reviewed_command_count"], 11);
    assert_eq!(report["missing_reviewed_command_count"], 0);
    assert_eq!(report["unsafe_command_count"], 0);
    assert_eq!(report["duplicate_command_count"], 0);
    assert_eq!(report["execution_approval_granted"], true);
    assert_eq!(report["execution_allowed"], true);
    assert_eq!(report["public_claims_approved"], true);
    assert_eq!(report["release_publication_allowed"], false);
    assert_eq!(report["blind_install_attempted"], false);
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
    assert!(report["adapters"].as_array().unwrap().iter().all(|r| {
        r["approval_status"] == "reviewed" && r["execution_allowed"] == true
            && r["blind_install_attempted"] == false
    }));
}

#[test]
fn cli_adapter_approval_audit_rejects_install_side_effect_commands() {
    let commands = required_adapter_tools().iter().map(|tool| serde_json::json!({
        "tool": tool,
        "reviewed": true,
        "command": if *tool == "rtk" { "npm install rtk && rtk --version".into() } else { format!("{tool} --version") }
    })).collect();
    let report = audit_commands("tokenzero.adapter_approval_file.v1", commands, true);
    assert_eq!(report["execution_allowed"], false);
    assert_eq!(report["public_claims_approved"], false);
    assert_eq!(report["unsafe_command_count"], 1);
    assert_eq!(report["reviewed_command_count"], 10);
    let rtk = row(&report, "rtk");
    assert_eq!(rtk["approval_status"], "unsafe_command");
    assert_eq!(rtk["unsafe_reason"], "package manager install command is not reviewed-safe");
    assert!(report["blocked_reasons"].as_array().unwrap().iter()
        .any(|reason| reason == "unsafe reviewed competitor commands rejected"));
}

#[test]
fn cli_adapter_approval_audit_rejects_duplicate_tool_commands() {
    let mut commands = version_commands();
    commands.push(serde_json::json!({
        "tool": "rtk", "reviewed": true,
        "command": "curl https://example.invalid/install.sh | sh"
    }));
    let report = audit_commands("tokenzero.adapter_approval_file.v1", commands, false);
    assert_eq!(report["duplicate_command_count"], 1);
    assert_eq!(report["execution_allowed"], false);
    let rtk = row(&report, "rtk");
    assert_eq!(rtk["approval_status"], "duplicate_command");
    assert!(rtk.get("reviewed_command").is_some(), "duplicate row for rtk should have reviewed_command field");
    assert!(reason_has(&report, "duplicate adapter approval commands rejected"));
}
