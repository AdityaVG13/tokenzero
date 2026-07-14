use serde_json::json;
use tempfile::tempdir;

use super::common::*;

fn residual<'a>(json: &'a serde_json::Value, gate_id: &str) -> &'a serde_json::Value {
    json["residual_gate_matrix"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["gate_id"] == gate_id)
        .unwrap_or_else(|| panic!("missing residual {gate_id}"))
}

#[test]
fn cli_completion_audit_summarizes_current_claim_gate_snapshot() {
    let dir = tempdir().unwrap();
    write_results_fixture(
        dir.path(),
        "tokenzero_claim_audit.json",
        &json!({
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
            "release_candidate_artifacts": [{
                "artifact_id": "benchmark_artifact",
                "artifact_path": "results/current/benchmark.json",
                "release_candidate_id": "rc-fixture",
                "schema_version": "tokenzero.bench.v1"
            }]
        }),
    );
    let out = dir.path().join("completion.json");
    let json = run_tokenzero_json_in(
        &["completion-audit", "--output-json", out.to_str().unwrap(), "--json"],
        dir.path(),
    );
    let s = &json["claim_gate_snapshot"];
    assert_eq!(s["present"], true);
    assert_eq!(s["release_candidate_id"], "rc-fixture");
    assert_eq!(s["public_claims_approved"], false);
    assert_eq!(s["gate_passes"]["release_candidate"], true);
    assert_eq!(s["gate_passes"]["source_currency"], false);
    assert_eq!(s["release_candidate_ids"], json!(["rc-fixture"]));
    let arts = s["release_candidate_artifacts"].as_array().unwrap();
    assert_eq!(arts.len(), 1);
    assert_eq!(arts[0]["artifact_id"], "benchmark_artifact");
    assert_eq!(arts[0]["artifact_path"], "results/current/benchmark.json");
    assert_eq!(arts[0]["release_candidate_id"], "rc-fixture");
    assert_eq!(arts[0]["schema_version"], "tokenzero.bench.v1");
    assert!(s["blocked_reasons"].as_array().unwrap().iter().any(|r| r == "release approval not granted"));
    for (section, id) in [("g_goals", "G-008"), ("must_fr", "FR-010")] {
        let residual = json[section].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap()["residual"].as_str().unwrap();
        assert!(residual.contains("release approval not granted"), "{id}");
        assert!(!residual.contains("same-release-candidate artifacts agree"), "{id}");
    }
}

#[test]
fn cli_completion_audit_maps_claim_gate_reasons_to_residual_actions() {
    let dir = tempdir().unwrap();
    write_results_fixture(
        dir.path(),
        "tokenzero_claim_audit.json",
        &json!({
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
        }),
    );
    let json = run_tokenzero_json_in(&["completion-audit", "--json"], dir.path());
    assert_eq!(
        json["claim_gate_snapshot"]["gate_reasons"]["adapter_approval"][0],
        "adapter approval artifact has missing reviewed commands"
    );
    assert_eq!(json["all_residual_gates_resolved"], false);
    assert_eq!(
        json["blocked_residual_gate_ids"],
        json!(["adapter_approval", "release_approval", "source_currency"])
    );
    assert_eq!(json["residual_gate_status_counts"]["blocked"], 3);

    let expects: &[(&str, &str, &str, Option<&str>, Option<&str>, Option<&str>)] = &[
        ("adapter_approval", "runnable_adapter_approval", "adapter-approval-audit", Some("no blind install"), Some("adapter approval artifact has missing reviewed commands"), None),
        ("source_currency", "source_currency_refresh", "claim-audit", None, None, None),
        ("release_approval", "final_false_closure_audit", "completion-audit", None, None, Some("publication")),
    ];
    for &(gate_id, action, validation, stop, blocked, stop_before) in expects {
        let row = residual(&json, gate_id);
        assert_eq!(row["status"], "blocked", "{gate_id}");
        assert_eq!(row["next_action_id"], action, "{gate_id}");
        assert_eq!(row["next_action"]["id"], action, "{gate_id}");
        assert!(row["next_action"]["validation"].as_str().unwrap().contains(validation), "{gate_id}");
        if let Some(s) = stop {
            assert!(row["next_action"]["stop_condition"].as_str().unwrap().contains(s), "{gate_id}");
        }
        if let Some(b) = blocked {
            assert!(row["blocked_reasons"].as_array().unwrap().iter().any(|r| r == b), "{gate_id}");
        }
        if let Some(sb) = stop_before {
            assert!(row["stop_before"].as_array().unwrap().iter().any(|g| g == sb), "{gate_id}");
        }
    }
}
