use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn cli_claim_audit_rejects_benchmark_rows_missing_public_claim_fields() {
    let dir = tempdir().unwrap();
    let thin_benchmark = dir.path().join("thin-benchmark.json");
    std::fs::write(
        &thin_benchmark,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.bench.v1",
            "ok": true,
            "public_claims_approved": true,
            "adapter_matrix": {
                "all_required_adapters_accounted": true,
                "blind_install_attempted": false
            },
            "rows": [{"tool": "tokenzero", "safe_savings": 0.75}]
        }))
        .unwrap(),
    )
    .unwrap();
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--benchmark-artifact",
            thin_benchmark.to_str().unwrap(),
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
    let benchmark_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "benchmark_artifact")
        .expect("benchmark gate");
    assert_eq!(benchmark_gate["pass"], false);
    assert!(
        benchmark_gate["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "benchmark row missing public-claim field: raw_tokens")
    );
    assert_eq!(json["public_claims_approved"], false);
    assert!(benchmark_gate["details"].is_object());
}

#[test]
fn cli_claim_audit_rejects_public_benchmark_rows_missing_byte_perfect_recovery() {
    let dir = tempdir().unwrap();
    let benchmark = dir.path().join("benchmark-missing-recovery.json");
    std::fs::write(
        &benchmark,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.bench.v1",
            "release_candidate_id": "rc-benchmark",
            "ok": true,
            "public_claims_approved": true,
            "adapter_matrix": {
                "all_required_adapters_accounted": true,
                "blind_install_attempted": false
            },
            "rows": [
                {
                    "tool": "tokenzero",
                    "suite": "shell-heavy",
                    "availability_status": "run",
                    "raw_tokens": 10,
                    "visible_tokens": 4,
                    "recovery_tokens": 0,
                    "safe_savings": 0.6,
                    "harm_rate": 0.0,
                    "task_success": true,
                    "fairness_notes": "fixture tokenzero row"
                },
                {
                    "tool": "rtk",
                    "suite": "shell-heavy",
                    "availability_status": "run",
                    "raw_tokens": 10,
                    "visible_tokens": 7,
                    "recovery_tokens": 0,
                    "safe_savings": 0.3,
                    "harm_rate": 0.0,
                    "task_success": true,
                    "fairness_notes": "fixture runnable competitor row"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--benchmark-artifact",
            benchmark.to_str().unwrap(),
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
    let benchmark_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "benchmark_artifact")
        .expect("benchmark gate");

    assert_eq!(benchmark_gate["pass"], false);
    assert!(
        benchmark_gate["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                == "benchmark row missing public-claim field: byte_perfect_recovery")
    );
    assert_eq!(json["public_claims_approved"], false);
    assert!(benchmark_gate["details"].is_object());
}

#[test]
fn cli_claim_audit_rejects_public_benchmark_rows_with_ref_less_expand_checks() {
    let dir = tempdir().unwrap();
    let benchmark = dir.path().join("benchmark-missing-expand-ref.json");
    std::fs::write(
        &benchmark,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.bench.v1",
            "release_candidate_id": "rc-benchmark",
            "ok": true,
            "public_claims_approved": true,
            "adapter_matrix": {
                "all_required_adapters_accounted": true,
                "blind_install_attempted": false
            },
            "rows": [
                {
                    "tool": "tokenzero",
                    "suite": "shell-heavy",
                    "availability_status": "run",
                    "raw_tokens": 10,
                    "visible_tokens": 4,
                    "recovery_tokens": 0,
                    "safe_savings": 0.6,
                    "harm_rate": 0.0,
                    "task_success": true,
                    "byte_perfect_recovery": true,
                    "exact_expand_checks": [
                        {"byte_perfect": true}
                    ],
                    "fairness_notes": "fixture tokenzero row"
                },
                {
                    "tool": "rtk",
                    "suite": "shell-heavy",
                    "availability_status": "run",
                    "raw_tokens": 10,
                    "visible_tokens": 7,
                    "recovery_tokens": 0,
                    "safe_savings": 0.3,
                    "harm_rate": 0.0,
                    "task_success": true,
                    "byte_perfect_recovery": true,
                    "exact_expand_checks": [
                        {"byte_perfect": true, "ref": "tz://blob/fixture"}
                    ],
                    "fairness_notes": "fixture runnable competitor row"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--benchmark-artifact",
            benchmark.to_str().unwrap(),
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
    let benchmark_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "benchmark_artifact")
        .expect("benchmark gate");

    assert_eq!(benchmark_gate["pass"], false);
    assert!(
        benchmark_gate["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "benchmark row exact expand check missing ref")
    );
    assert_eq!(json["public_claims_approved"], false);
    assert!(benchmark_gate["details"].is_object());
    assert!(!json["blocked_reasons"].as_array().unwrap().is_empty());
}

#[test]
fn cli_claim_audit_rejects_public_benchmark_with_unavailable_competitor_rows() {
    let dir = tempdir().unwrap();
    let benchmark = dir.path().join("benchmark.json");
    std::fs::write(
        &benchmark,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.bench.v1",
            "ok": true,
            "public_claims_approved": true,
            "adapter_matrix": {
                "all_required_adapters_accounted": true,
                "blind_install_attempted": false
            },
            "rows": [
                {
                    "tool": "tokenzero",
                    "suite": "shell-heavy",
                    "availability_status": "run",
                    "raw_tokens": 100,
                    "visible_tokens": 25,
                    "recovery_tokens": 75,
                    "safe_savings": 0.75,
                    "harm_rate": 0.0,
                    "task_success": true,
                    "fairness_notes": "fixture tokenzero row"
                },
                {
                    "tool": "rtk",
                    "suite": "shell-heavy",
                    "availability_status": "unavailable",
                    "availability_reason": "fixture adapter is not executed without review",
                    "raw_tokens": 0,
                    "visible_tokens": 0,
                    "recovery_tokens": 0,
                    "safe_savings": 0.0,
                    "harm_rate": 0.0,
                    "task_success": false,
                    "fairness_notes": "fixture unavailable competitor row"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--benchmark-artifact",
            benchmark.to_str().unwrap(),
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
    let benchmark_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "benchmark_artifact")
        .expect("benchmark gate");

    assert_eq!(benchmark_gate["pass"], false);
    assert!(
        benchmark_gate["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "benchmark competitor rows must be runnable for public claims")
    );
    assert_eq!(
        benchmark_gate["details"]["public_claim_status"]["competitor_unavailable_rows"],
        1
    );
    assert_eq!(
        benchmark_gate["details"]["public_claim_status"]["unavailable_competitors"][0]["availability_reason"],
        "fixture adapter is not executed without review"
    );
    assert_eq!(json["public_claims_approved"], false);
    assert!(benchmark_gate["details"].is_object());
}

#[test]
fn cli_claim_audit_does_not_treat_unavailable_benchmark_rows_as_recovery_failures() {
    let dir = tempdir().unwrap();
    let benchmark = dir.path().join("benchmark-unavailable-row.json");
    std::fs::write(
        &benchmark,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.bench.v1",
            "ok": true,
            "public_claims_approved": true,
            "adapter_matrix": {
                "all_required_adapters_accounted": true,
                "blind_install_attempted": false
            },
            "rows": [
                {
                    "tool": "tokenzero",
                    "suite": "shell-heavy",
                    "availability_status": "run",
                    "raw_tokens": 100,
                    "visible_tokens": 25,
                    "recovery_tokens": 75,
                    "safe_savings": 0.75,
                    "harm_rate": 0.0,
                    "task_success": true,
                    "byte_perfect_recovery": true,
                    "exact_expand_checks": [
                        {"byte_perfect": true, "ref": "tz://blob/tokenzero"}
                    ],
                    "fairness_notes": "fixture tokenzero row"
                },
                {
                    "tool": "rtk",
                    "suite": "shell-heavy",
                    "availability_status": "unavailable",
                    "availability_reason": "fixture adapter is not executed without review",
                    "raw_tokens": 0,
                    "visible_tokens": 0,
                    "recovery_tokens": 0,
                    "safe_savings": 0.0,
                    "harm_rate": 0.0,
                    "task_success": false,
                    "fairness_notes": "fixture unavailable competitor row"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--benchmark-artifact",
            benchmark.to_str().unwrap(),
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
    let benchmark_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "benchmark_artifact")
        .expect("benchmark gate");
    let reasons = benchmark_gate["reasons"].as_array().unwrap();

    assert_eq!(benchmark_gate["pass"], false);
    assert!(
        reasons
            .iter()
            .any(|reason| reason == "benchmark competitor rows must be runnable for public claims")
    );
    assert!(
        !reasons
            .iter()
            .any(|reason| reason == "benchmark row failed byte-perfect recovery")
    );
    assert!(!reasons
        .iter()
        .any(|reason| reason == "benchmark row missing public-claim field: exact_expand_checks"));
    assert_eq!(
        benchmark_gate["details"]["public_claim_status"]["competitor_unavailable_rows"],
        1
    );
    assert_eq!(json["public_claims_approved"], false);
}

#[test]
fn cli_claim_audit_rejects_unavailable_benchmark_rows_missing_availability_reason() {
    let dir = tempdir().unwrap();
    let benchmark = dir.path().join("benchmark-unavailable-without-reason.json");
    std::fs::write(
        &benchmark,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.bench.v1",
            "ok": true,
            "public_claims_approved": true,
            "adapter_matrix": {
                "all_required_adapters_accounted": true,
                "blind_install_attempted": false
            },
            "rows": [
                {
                    "tool": "tokenzero",
                    "suite": "shell-heavy",
                    "availability_status": "run",
                    "raw_tokens": 100,
                    "visible_tokens": 25,
                    "recovery_tokens": 75,
                    "safe_savings": 0.75,
                    "harm_rate": 0.0,
                    "task_success": true,
                    "byte_perfect_recovery": true,
                    "exact_expand_checks": [
                        {"byte_perfect": true, "ref": "tz://blob/tokenzero"}
                    ],
                    "fairness_notes": "fixture tokenzero row"
                },
                {
                    "tool": "rtk",
                    "suite": "shell-heavy",
                    "availability_status": "unavailable",
                    "raw_tokens": 0,
                    "visible_tokens": 0,
                    "recovery_tokens": 0,
                    "safe_savings": 0.0,
                    "harm_rate": 0.0,
                    "task_success": false,
                    "fairness_notes": "fixture unavailable competitor row"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--benchmark-artifact",
            benchmark.to_str().unwrap(),
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
    let benchmark_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "benchmark_artifact")
        .expect("benchmark gate");

    assert_eq!(benchmark_gate["pass"], false);
    assert!(
        benchmark_gate["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "benchmark unavailable row missing availability_reason")
    );
    assert_eq!(json["public_claims_approved"], false);
    assert!(!json["blocked_reasons"].as_array().unwrap().is_empty());
}
