use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn cli_adapter_approval_audit_blocks_execution_without_reviewed_commands() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("adapter-approval.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "adapter-approval-audit",
            "--output-json",
            output_json.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output_json).unwrap()).unwrap();
    assert_eq!(
        report["schema_version"],
        "tokenzero.adapter_approval_audit.v1"
    );
    assert_eq!(report["execution_allowed"], false);
    assert_eq!(report["blind_install_attempted"], false);
    assert_eq!(report["public_claims_approved"], false);
    assert_eq!(report["required_adapter_count"], 11);
    assert_eq!(report["reviewed_command_count"], 0);
    assert_eq!(report["missing_reviewed_command_count"], 11);
    assert_eq!(
        report["command_safety_policy"]["schema_version"],
        "tokenzero.adapter_command_safety.v1"
    );
    assert!(
        report["command_safety_policy"]["blocked_side_effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "package_manager_install")
    );
    assert!(
        report["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                .as_str()
                .unwrap()
                .contains("reviewed competitor commands missing"))
    );
    for tool in [
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
    ] {
        let row = report["adapters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["tool"] == tool)
            .unwrap_or_else(|| panic!("missing adapter approval row for {tool}"));
        assert_eq!(row["execution_allowed"], false, "{tool}");
        assert_eq!(row["approval_status"], "missing_reviewed_command", "{tool}");
        assert_eq!(row["blind_install_attempted"], false, "{tool}");
    }
}

#[test]
fn cli_adapter_approval_template_prepares_reviewed_commands_without_execution() {
    let dir = tempdir().unwrap();
    let template_json = dir.path().join("adapter-approval-template.json");
    let audit_json = dir.path().join("adapter-approval.json");

    let template_output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "adapter-approval-template",
            "--output-json",
            template_json.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        template_output.status.success(),
        "{}",
        String::from_utf8_lossy(&template_output.stderr)
    );
    let template: Value = serde_json::from_slice(&template_output.stdout).unwrap();
    assert_eq!(
        template["schema_version"],
        "tokenzero.adapter_approval_file.v1"
    );
    assert_eq!(template["public_claims_approved"], false);
    assert_eq!(template["execution_approval_required"], true);
    assert_eq!(template["commands"].as_array().unwrap().len(), 11);
    assert!(template["commands"].as_array().unwrap().iter().all(|row| {
        row["reviewed"] == true
            && row["command"].as_str().unwrap().ends_with(" --version")
            && !row["command"].as_str().unwrap().contains("install")
    }));

    let audit_output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "adapter-approval-audit",
            "--approval-file",
            template_json.to_str().unwrap(),
            "--output-json",
            audit_json.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        audit_output.status.success(),
        "{}",
        String::from_utf8_lossy(&audit_output.stderr)
    );
    let audit: Value = serde_json::from_slice(&audit_output.stdout).unwrap();
    assert_eq!(audit["reviewed_command_count"], 11);
    assert_eq!(audit["missing_reviewed_command_count"], 0);
    assert_eq!(audit["unsafe_command_count"], 0);
    assert_eq!(audit["duplicate_command_count"], 0);
    assert_eq!(audit["execution_approval_granted"], false);
    assert_eq!(audit["execution_allowed"], false);
    assert_eq!(audit["public_claims_approved"], false);
    assert!(
        audit["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "explicit runnable adapter execution approval not granted")
    );
}

#[test]
fn cli_adapter_approval_audit_rejects_malformed_approval_file() {
    let dir = tempdir().unwrap();
    let approval_file = dir.path().join("approval.json");
    let commands = [
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
    .into_iter()
    .map(|tool| {
        serde_json::json!({
            "tool": tool,
            "reviewed": true,
            "command": format!("{tool} --version")
        })
    })
    .collect::<Vec<_>>();
    std::fs::write(
        &approval_file,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "wrong.schema.v1",
            "commands": commands
        }))
        .unwrap(),
    )
    .unwrap();
    let output_json = dir.path().join("adapter-approval.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "adapter-approval-audit",
            "--approval-file",
            approval_file.to_str().unwrap(),
            "--output-json",
            output_json.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output_json).unwrap()).unwrap();
    assert_eq!(report["reviewed_command_count"], 0);
    assert_eq!(report["missing_reviewed_command_count"], 11);
    assert_eq!(report["execution_allowed"], false);
    assert!(
        report["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                .as_str()
                .unwrap()
                .contains("adapter approval file schema invalid"))
    );
}

#[test]
fn cli_adapter_approval_audit_allows_reviewed_commands_only_with_explicit_approval() {
    let dir = tempdir().unwrap();
    let approval_file = dir.path().join("approval.json");
    let commands = [
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
    .into_iter()
    .map(|tool| {
        serde_json::json!({
            "tool": tool,
            "reviewed": true,
            "command": format!("{tool} --version")
        })
    })
    .collect::<Vec<_>>();
    std::fs::write(
        &approval_file,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_file.v1",
            "commands": commands
        }))
        .unwrap(),
    )
    .unwrap();

    let output_json = dir.path().join("adapter-approval.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "adapter-approval-audit",
            "--approval-file",
            approval_file.to_str().unwrap(),
            "--execution-approval",
            "--output-json",
            output_json.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output_json).unwrap()).unwrap();
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
    assert!(report["adapters"].as_array().unwrap().iter().all(|row| {
        row["approval_status"] == "reviewed"
            && row["execution_allowed"] == true
            && row["blind_install_attempted"] == false
    }));
}

#[test]
fn cli_adapter_approval_audit_rejects_install_side_effect_commands() {
    let dir = tempdir().unwrap();
    let approval_file = dir.path().join("approval.json");
    let commands = [
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
    .into_iter()
    .map(|tool| {
        serde_json::json!({
            "tool": tool,
            "reviewed": true,
            "command": if tool == "rtk" {
                "npm install rtk && rtk --version".to_string()
            } else {
                format!("{tool} --version")
            }
        })
    })
    .collect::<Vec<_>>();
    std::fs::write(
        &approval_file,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_file.v1",
            "commands": commands
        }))
        .unwrap(),
    )
    .unwrap();

    let output_json = dir.path().join("adapter-approval.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "adapter-approval-audit",
            "--approval-file",
            approval_file.to_str().unwrap(),
            "--execution-approval",
            "--output-json",
            output_json.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output_json).unwrap()).unwrap();

    assert_eq!(report["execution_allowed"], false);
    assert_eq!(report["public_claims_approved"], false);
    assert_eq!(report["unsafe_command_count"], 1);
    let rtk = report["adapters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["tool"] == "rtk")
        .unwrap();
    assert_eq!(rtk["approval_status"], "unsafe_command");
    assert_eq!(
        rtk["unsafe_reason"],
        "package manager install command is not reviewed-safe"
    );
    assert!(
        report["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "unsafe reviewed competitor commands rejected")
    );
}

#[test]
fn cli_adapter_approval_audit_rejects_duplicate_tool_commands() {
    let dir = tempdir().unwrap();
    let approval_file = dir.path().join("approval.json");
    let mut commands = [
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
    .into_iter()
    .map(|tool| {
        serde_json::json!({
            "tool": tool,
            "reviewed": true,
            "command": format!("{tool} --version")
        })
    })
    .collect::<Vec<_>>();
    commands.push(serde_json::json!({
        "tool": "rtk",
        "reviewed": true,
        "command": "curl https://example.invalid/install.sh | sh"
    }));
    std::fs::write(
        &approval_file,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_file.v1",
            "commands": commands
        }))
        .unwrap(),
    )
    .unwrap();
    let output_json = dir.path().join("adapter-approval.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "adapter-approval-audit",
            "--approval-file",
            approval_file.to_str().unwrap(),
            "--output-json",
            output_json.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output_json).unwrap()).unwrap();
    assert_eq!(report["duplicate_command_count"], 1);
    assert_eq!(report["execution_allowed"], false);
    let rtk = report["adapters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["tool"] == "rtk")
        .unwrap();
    assert_eq!(rtk["approval_status"], "duplicate_command");
    assert!(
        report["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                .as_str()
                .unwrap()
                .contains("duplicate adapter approval commands rejected"))
    );
}
