use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

#[path = "../common/mod.rs"]
mod common;
use common::*;


#[test]
fn cli_claim_audit_requires_adapter_approval_artifact_for_public_claims() {
    let dir = tempdir().unwrap();
    let adapter_approval = dir.path().join("adapter-approval.json");
    std::fs::write(
        &adapter_approval,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_audit.v1",
            "ok": true,
            "execution_allowed": false,
            "public_claims_approved": false,
            "blind_install_attempted": false,
            "required_adapter_count": 11,
            "reviewed_command_count": 0,
            "missing_reviewed_command_count": 11,
            "unsafe_command_count": 0,
            "adapters": []
        }))
        .unwrap(),
    )
    .unwrap();
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--adapter-approval-artifact",
            adapter_approval.to_str().unwrap(),
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
    assert_eq!(json["public_claims_approved"], false);
    let adapter_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "adapter_approval")
        .expect("adapter approval gate");
    assert_eq!(adapter_gate["pass"], false);
    assert!(
        adapter_gate["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| {
                reason
                    .as_str()
                    .unwrap()
                    .contains("adapter approval artifact does not allow execution")
            })
    );
    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| {
                reason
                    .as_str()
                    .unwrap()
                    .contains("adapter approval artifact does not allow execution")
            })
    );
}

#[test]
fn cli_claim_audit_rejects_adapter_approval_missing_adapter_rows() {
    let dir = tempdir().unwrap();
    let adapter_approval = dir.path().join("adapter-approval.json");
    std::fs::write(
        &adapter_approval,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_audit.v1",
            "ok": true,
            "execution_allowed": true,
            "public_claims_approved": true,
            "blind_install_attempted": false,
            "required_adapter_count": 11,
            "reviewed_command_count": 11,
            "missing_reviewed_command_count": 0,
            "duplicate_command_count": 0,
            "unsafe_command_count": 0,
            "adapters": []
        }))
        .unwrap(),
    )
    .unwrap();
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--adapter-approval-artifact",
            adapter_approval.to_str().unwrap(),
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
    let adapter_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "adapter_approval")
        .expect("adapter approval gate");
    assert_eq!(adapter_gate["pass"], false);
    assert!(
        adapter_gate["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "adapter approval artifact rows do not cover required adapters")
    );
    assert_eq!(json["public_claims_approved"], false);
}

#[test]
fn cli_claim_audit_separates_reviewed_adapter_coverage_from_execution_approval() {
    let dir = tempdir().unwrap();
    let template_json = dir.path().join("adapter-approval-template.json");
    let audit_json = dir.path().join("adapter-approval.json");
    let claim_json = dir.path().join("claim.json");

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

    let claim_output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--adapter-approval-artifact",
            audit_json.to_str().unwrap(),
            "--output-json",
            claim_json.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        claim_output.status.success(),
        "{}",
        String::from_utf8_lossy(&claim_output.stderr)
    );
    let claim: Value = serde_json::from_slice(&claim_output.stdout).unwrap();
    let adapter_gate = claim["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "adapter_approval")
        .unwrap();
    assert_eq!(adapter_gate["pass"], false);
    assert!(
        adapter_gate["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "adapter approval artifact does not allow execution")
    );
    assert!(!adapter_gate["reasons"].as_array().unwrap().iter().any(
        |reason| reason == "adapter approval artifact rows do not cover required adapters"
    ));
    assert_eq!(adapter_gate["details"]["reviewed_command_count"], 11);
    assert_eq!(adapter_gate["details"]["missing_reviewed_command_count"], 0);
}
