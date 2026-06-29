use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;



#[test]
fn cli_completion_audit_summarizes_current_claim_gate_snapshot() {
    let dir = tempdir().unwrap();
    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    std::fs::write(
        results_dir.join("tokenzero_claim_audit.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.claim_audit.v1",
            "release_candidate_id": "rc-fixture",
            "ok": true,
            "public_claims_approved": false,
            "blocked_reasons": ["release approval not granted"],
            "gate_passes": {
                "source_currency": false,
                "release_candidate": true,
                "release_approval": false
            },
            "gate_reasons": {
                "source_currency": ["source refresh not same-release-candidate"],
                "release_candidate": [],
                "release_approval": ["release approval not granted"]
            },
            "release_candidate_ids": ["rc-fixture"],
            "release_candidate_artifacts": [
                {
                    "artifact_id": "benchmark_artifact",
                    "artifact_path": "results/current/benchmark.json",
                    "release_candidate_id": "rc-fixture",
                    "schema_version": "tokenzero.bench.v1"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let output_json = dir.path().join("completion.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .args([
            "completion-audit",
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
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let snapshot = &json["claim_gate_snapshot"];
    assert_eq!(snapshot["present"], true);
    assert_eq!(snapshot["release_candidate_id"], "rc-fixture");
    assert_eq!(snapshot["public_claims_approved"], false);
    assert_eq!(snapshot["gate_passes"]["release_candidate"], true);
    assert_eq!(snapshot["gate_passes"]["source_currency"], false);
    assert_eq!(
        snapshot["release_candidate_ids"].as_array().unwrap(),
        &vec![serde_json::json!("rc-fixture")]
    );
    let release_artifacts = snapshot["release_candidate_artifacts"].as_array().unwrap();
    assert_eq!(release_artifacts.len(), 1);
    assert_eq!(release_artifacts[0]["artifact_id"], "benchmark_artifact");
    assert_eq!(
        release_artifacts[0]["artifact_path"],
        "results/current/benchmark.json"
    );
    assert_eq!(release_artifacts[0]["release_candidate_id"], "rc-fixture");
    assert_eq!(release_artifacts[0]["schema_version"], "tokenzero.bench.v1");
    assert!(
        snapshot["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "release approval not granted")
    );
    let g008 = json["g_goals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "G-008")
        .expect("G-008 row");
    let g008_residual = g008["residual"].as_str().unwrap();
    assert!(g008_residual.contains("release approval not granted"));
    assert!(
        !g008_residual.contains("same-release-candidate artifacts agree"),
        "G-008 residual should not cite stale release-candidate mismatch when the release-candidate gate passes"
    );
    let fr010 = json["must_fr"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "FR-010")
        .expect("FR-010 row");
    let fr010_residual = fr010["residual"].as_str().unwrap();
    assert!(fr010_residual.contains("release approval not granted"));
    assert!(
        !fr010_residual.contains("same-release-candidate artifacts agree"),
        "FR-010 residual should mirror the current claim gate blockers"
    );
}

#[test]
fn cli_completion_audit_maps_claim_gate_reasons_to_residual_actions() {
    let dir = tempdir().unwrap();
    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    std::fs::write(
        results_dir.join("tokenzero_claim_audit.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.claim_audit.v1",
            "ok": true,
            "public_claims_approved": false,
            "blocked_reasons": [
                "source refresh not same-release-candidate",
                "adapter approval artifact has missing reviewed commands",
                "release approval not granted"
            ],
            "evidence_gates": [
                {"id": "source_currency", "pass": false, "reasons": ["source refresh not same-release-candidate"]},
                {"id": "adapter_approval", "pass": false, "reasons": ["adapter approval artifact has missing reviewed commands"]},
                {"id": "release_candidate", "pass": true, "reasons": [], "details": {"release_candidate_ids": ["rc-fixture"]}},
                {"id": "release_approval", "pass": false, "reasons": ["release approval not granted"]}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .args(["completion-audit", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["claim_gate_snapshot"]["gate_reasons"]["adapter_approval"][0],
        "adapter approval artifact has missing reviewed commands"
    );
    assert_eq!(json["all_residual_gates_resolved"], false);
    assert_eq!(
        json["blocked_residual_gate_ids"],
        serde_json::json!(["adapter_approval", "release_approval", "source_currency"])
    );
    assert_eq!(json["residual_gate_status_counts"]["blocked"], 3);
    let residuals = json["residual_gate_matrix"].as_array().unwrap();
    let adapter = residuals
        .iter()
        .find(|row| row["gate_id"] == "adapter_approval")
        .expect("adapter residual");
    assert_eq!(adapter["status"], "blocked");
    assert_eq!(adapter["next_action_id"], "runnable_adapter_approval");
    assert_eq!(adapter["next_action"]["id"], "runnable_adapter_approval");
    assert!(
        adapter["next_action"]["validation"]
            .as_str()
            .unwrap()
            .contains("adapter-approval-audit")
    );
    assert!(
        adapter["next_action"]["stop_condition"]
            .as_str()
            .unwrap()
            .contains("no blind install")
    );
    assert!(
        adapter["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "adapter approval artifact has missing reviewed commands")
    );
    let source = residuals
        .iter()
        .find(|row| row["gate_id"] == "source_currency")
        .expect("source residual");
    assert_eq!(source["next_action_id"], "source_currency_refresh");
    assert_eq!(source["next_action"]["id"], "source_currency_refresh");
    assert!(
        source["next_action"]["validation"]
            .as_str()
            .unwrap()
            .contains("claim-audit")
    );
    let release = residuals
        .iter()
        .find(|row| row["gate_id"] == "release_approval")
        .expect("release residual");
    assert_eq!(release["next_action_id"], "final_false_closure_audit");
    assert_eq!(release["next_action"]["id"], "final_false_closure_audit");
    assert!(
        release["next_action"]["validation"]
            .as_str()
            .unwrap()
            .contains("completion-audit")
    );
    assert!(
        release["stop_before"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gate| gate == "publication")
    );
}
