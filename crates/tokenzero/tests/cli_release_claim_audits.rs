use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

mod common;
use common::*;

#[test]
fn cli_one_shot_eval_reports_zero_critical_misses_with_refs() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("one-shot.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "one-shot-eval",
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
    assert_eq!(json["schema_version"], "tokenzero.one_shot_eval.v1");
    assert_eq!(json["critical_miss_rate"], 0.0);
    assert_eq!(json["overall_miss_rate"], 0.0);
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    for trace_id in [
        "source_edit_anchor",
        "failure_diagnosis_anchor",
        "warning_changed_file_anchor",
        "diff_review_anchor",
        "recovery_degraded_anchor",
    ] {
        assert!(
            json["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["trace_id"] == trace_id),
            "{trace_id}"
        );
    }
    assert!(json["rows"].as_array().unwrap().iter().all(|row| {
        row["planned_expands"].as_array().unwrap().is_empty()
            && row["unplanned_second_call"] == false
            && row["required_anchors_present"] == true
            && row["task_success"] == true
            && (row["refs_available"] == true || row["degraded_explicit"] == true)
    }));
    assert!(output_json.exists());
}

#[test]
fn cli_claim_audit_blocks_public_claims_without_release_approval() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("claims.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .args([
            "claim-audit",
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
    assert_eq!(json["schema_version"], "tokenzero.claim_audit.v1");
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["transport_status"], "ok");
    assert_eq!(json["claim_status"], "blocked");
    assert_eq!(json["release_publication_allowed"], false);
    assert_eq!(json["gate_passes"]["release_approval"], false);
    assert_eq!(json["gate_passes"]["benchmark_artifact"], false);
    assert!(
        json["gate_reasons"]["release_approval"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "release approval not granted")
    );
    assert_eq!(json["release_candidate_ids"], serde_json::json!([]));
    let release_candidate_artifacts = json["release_candidate_artifacts"].as_array().unwrap();
    assert_eq!(release_candidate_artifacts.len(), 6);
    assert!(release_candidate_artifacts.iter().all(|artifact| {
        artifact["release_candidate_id"] == serde_json::Value::Null
            && artifact["artifact_path"] == serde_json::Value::Null
    }));
    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "release approval not granted")
    );
    assert!(
        json["claims"].as_array().unwrap().iter().all(|claim| {
            claim["approved"] == false && claim["public_safe_to_publish"] == false
        })
    );
    assert!(output_json.exists());
}

#[test]
fn cli_source_currency_audit_records_competitive_ledger_and_blocks_public_claims() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("source-ledger.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "source-currency-audit",
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
    assert_eq!(json["schema_version"], "tokenzero.source_currency.v1");
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    assert_eq!(json["fresh_for_private_planning"], true);
    assert_eq!(json["fresh_for_public_claim"], false);
    assert!(
        json["source_commit_pin_status"]["unpinned"]
            .as_u64()
            .unwrap()
            >= 11
    );
    assert!(
        json["unpinned_source_rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["source_commit"] == "snapshot-20260604")
    );

    let required_tools = [
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
    ];
    let rows = json["rows"].as_array().unwrap();
    for tool in required_tools {
        let row = rows
            .iter()
            .find(|row| row["tool"] == tool)
            .unwrap_or_else(|| panic!("missing source row for {tool}"));
        assert!(
            row["url"]
                .as_str()
                .unwrap()
                .starts_with("https://github.com/"),
            "{tool}"
        );
        assert_eq!(row["source_date"], "2026-06-04");
        assert!(row["source_commit"].as_str().unwrap().len() >= 7, "{tool}");
        assert!(!row["claimed_scope"].as_str().unwrap().is_empty(), "{tool}");
        assert!(
            !row["issue_pr_themes"].as_array().unwrap().is_empty(),
            "{tool}"
        );
        assert!(!row["strengths"].as_array().unwrap().is_empty(), "{tool}");
        assert!(!row["gaps"].as_array().unwrap().is_empty(), "{tool}");
        assert_eq!(row["fresh_for_private_planning"], true, "{tool}");
        assert_eq!(row["fresh_for_public_claim"], false, "{tool}");
    }

    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "source ledger requires same-release-candidate refresh")
    );
    assert!(output_json.exists());
}

#[test]
fn cli_source_currency_audit_refreshes_release_candidate_pins_without_public_approval() {
    let dir = tempdir().unwrap();
    let refresh_ledger = dir.path().join("source-refresh.json");
    let output_json = dir.path().join("source-ledger.json");
    let claim_json = dir.path().join("claims.json");
    let tools = [
        "rtk",
        "ztk",
        "lean-ctx",
        "tokenpak",
        "tokenjuice",
        "context-mode",
        "caveman",
        "cavekit",
        "cavemem",
        "caveman-code",
        "headroom",
        "engram",
        "claw",
        "contextpilot",
        "wilpel-caveman-compression",
        "compresh",
        "compresh-mcp",
        "context-gateway",
    ];
    let rows: Vec<Value> = tools
        .iter()
        .enumerate()
        .map(|(idx, tool)| {
            serde_json::json!({
                "tool": tool,
                "source_commit": format!("{:040x}", idx + 1),
                "source_date": "2026-06-04"
            })
        })
        .collect();
    std::fs::write(
        &refresh_ledger,
        serde_json::to_vec_pretty(&serde_json::json!({ "rows": rows })).unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-source-refresh")
        .args([
            "source-currency-audit",
            "--refresh-ledger",
            refresh_ledger.to_str().unwrap(),
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
    assert_eq!(json["schema_version"], "tokenzero.source_currency.v1");
    assert_eq!(json["release_candidate_id"], "rc-source-refresh");
    assert_eq!(json["fresh_for_public_claim"], true);
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    assert_eq!(json["source_commit_pin_status"]["pinned"], tools.len());
    assert_eq!(json["source_commit_pin_status"]["missing"], 0);
    assert_eq!(json["source_commit_pin_status"]["unpinned"], 0);
    assert!(json["unpinned_source_rows"].as_array().unwrap().is_empty());
    assert!(json["rows"].as_array().unwrap().iter().all(|row| {
        row["fresh_for_public_claim"] == true
            && row["source_commit"]
                .as_str()
                .unwrap()
                .chars()
                .all(|ch| ch.is_ascii_hexdigit())
    }));
    assert!(
        !json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "source ledger requires same-release-candidate refresh")
    );

    let claim_output = Command::cargo_bin("tokenzero")
        .unwrap()
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-source-refresh")
        .args([
            "claim-audit",
            "--source-artifact",
            output_json.to_str().unwrap(),
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
    let source_gate = claim["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "source_currency")
        .unwrap();
    assert_eq!(source_gate["pass"], true);
    assert_eq!(claim["public_claims_approved"], false);
    assert!(
        claim["claims"]
            .as_array()
            .unwrap()
            .iter()
            .all(|claim| { claim["public_safe_to_publish"] == false })
    );

    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    std::fs::copy(
        &output_json,
        results_dir.join("tokenzero_source_currency.json"),
    )
    .unwrap();
    let completion_output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-source-refresh")
        .args(["completion-audit", "--json"])
        .output()
        .unwrap();
    assert!(
        completion_output.status.success(),
        "{}",
        String::from_utf8_lossy(&completion_output.stderr)
    );
    let completion: Value = serde_json::from_slice(&completion_output.stdout).unwrap();
    let g001 = completion["g_goals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "G-001")
        .expect("G-001 row");
    let g001_residual = g001["residual"].as_str().unwrap();
    assert!(g001_residual.contains("source evidence is current"));
    assert!(
        !g001_residual.contains("refresh required"),
        "fresh source evidence should not be reported as a remaining source refresh"
    );
    let fr001 = completion["must_fr"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "FR-001")
        .expect("FR-001 row");
    let fr001_residual = fr001["residual"].as_str().unwrap();
    assert!(fr001_residual.contains("source evidence is current"));
    assert!(
        !fr001_residual.contains("refresh still required"),
        "FR-001 should not ask for a source refresh once the source artifact is fresh"
    );
}

#[test]
fn cli_claim_audit_uses_results_current_artifacts_without_explicit_paths() {
    let dir = tempdir().unwrap();
    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    let release_candidate_id = "rc-current-defaults";
    let tools = [
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
    ];
    let source_rows: Vec<Value> = tools
        .iter()
        .enumerate()
        .map(|(idx, tool)| {
            serde_json::json!({
                "tool": tool,
                "url": format!("https://github.com/example/{tool}"),
                "source_date": "2026-06-04",
                "source_commit": format!("{:040x}", idx + 1),
                "claimed_scope": "claim gate fixture",
                "issue_pr_themes": ["fixture issue"],
                "strengths": ["fixture strength"],
                "gaps": ["fixture gap"],
                "fresh_for_private_planning": true,
                "fresh_for_public_claim": true
            })
        })
        .collect();
    std::fs::write(
        results_dir.join("tokenzero_source_currency.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": "tokenzero.source_currency.v1",
            "release_candidate_id": release_candidate_id,
            "ok": true,
            "fresh_for_public_claim": true,
            "public_claims_approved": false,
            "release_publication_allowed": false,
            "rows": source_rows
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_bench_competitors_shell_heavy.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": "tokenzero.bench.v1",
            "release_candidate_id": release_candidate_id,
            "ok": true,
            "public_claims_approved": false,
            "release_publication_allowed": false,
            "adapter_matrix": {
                "all_required_adapters_accounted": true,
                "blind_install_attempted": false
            },
            "rows": [
                {
                    "tool": "tokenzero",
                    "suite": "shell-heavy",
                    "scenario_id": "read",
                    "availability_status": "run",
                    "raw_tokens": 100,
                    "visible_tokens": 20,
                    "recovery_tokens": 0,
                    "safe_savings": 0.8,
                    "harm_rate": 0.0,
                    "task_success": true,
                    "fairness_notes": "fixture"
                },
                {
                    "tool": "rtk",
                    "suite": "shell-heavy",
                    "scenario_id": "read",
                    "availability_status": "unavailable",
                    "raw_tokens": 0,
                    "visible_tokens": 0,
                    "recovery_tokens": 0,
                    "safe_savings": 0.0,
                    "harm_rate": 0.0,
                    "task_success": false,
                    "fairness_notes": "fixture unavailable"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_adapter_approval_audit.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_audit.v1",
            "release_candidate_id": release_candidate_id,
            "execution_allowed": false,
            "public_claims_approved": false,
            "blind_install_attempted": false,
            "required_adapter_count": tools.len(),
            "reviewed_command_count": tools.len(),
            "missing_reviewed_command_count": 0,
            "duplicate_command_count": 0,
            "unsafe_command_count": 0,
            "adapters": reviewed_adapter_rows()
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_exact_recovery_audit.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": "tokenzero.exact_recovery_audit.v1",
            "release_candidate_id": release_candidate_id,
            "ok": true,
            "normal_rows": [
                {"id": "fixture", "all_refs_recover": true}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_one_shot_eval.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": "tokenzero.one_shot_eval.v1",
            "release_candidate_id": release_candidate_id,
            "ok": true,
            "critical_miss_rate": 0.0,
            "public_claims_approved": false,
            "release_publication_allowed": false,
            "rows": [
                {"trace_id": "fixture", "task_success": true}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_os_reach_audit.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": "tokenzero.os_reach_audit.v1",
            "release_candidate_id": release_candidate_id,
            "ok": true,
            "all_release_oses_run": false,
            "public_os_claim_approved": false
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", release_candidate_id)
        .args(["claim-audit", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let source_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "source_currency")
        .expect("source gate");
    let release_candidate_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "release_candidate")
        .expect("release candidate gate");

    assert_eq!(source_gate["pass"], true);
    let source_artifact_path = json["gate_artifact_paths"]["source_currency"]
        .as_str()
        .unwrap();
    assert!(!source_artifact_path.contains('\\'));
    assert_eq!(
        source_artifact_path,
        "results/current/tokenzero_source_currency.json"
    );
    assert_eq!(
        release_candidate_gate["details"]["attached_artifact_count"],
        6
    );
    assert_eq!(
        release_candidate_gate["details"]["release_candidate_ids"],
        serde_json::json!([release_candidate_id])
    );
    assert!(
        release_candidate_gate["details"]["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|artifact| artifact["artifact_path"].as_str())
            .all(|path| !path.contains('\\'))
    );
    let blocked_reasons = json["blocked_reasons"].as_array().unwrap();
    for stale_reason in [
        "source ledger requires same-release-candidate refresh",
        "same-release-candidate evidence incomplete",
        "byte-perfect recovery proof not attached to public claim",
        "task-success proof not attached to public claim",
    ] {
        assert!(
            !blocked_reasons.iter().any(|reason| reason == stale_reason),
            "plain claim-audit should consume current artifacts instead of emitting stale reason: {stale_reason}"
        );
    }
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
}

#[test]
fn cli_claim_audit_includes_source_currency_gate_even_with_release_approval() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("claims.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
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
    assert_eq!(json["schema_version"], "tokenzero.claim_audit.v1");
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(
        json["source_currency"]["schema_version"],
        "tokenzero.source_currency.v1"
    );
    assert_eq!(json["source_currency"]["fresh_for_public_claim"], false);
    assert!(json["source_currency"]["rows"].as_array().unwrap().len() >= 11);
    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "source ledger requires same-release-candidate refresh")
    );
    assert!(json["claims"].as_array().unwrap().iter().all(|claim| {
        claim["source_current"] == false && claim["public_safe_to_publish"] == false
    }));
    assert!(output_json.exists());
}

#[test]
fn cli_claim_audit_evaluates_supplied_evidence_artifacts_fail_closed() {
    let dir = tempdir().unwrap();
    let bad_benchmark = dir.path().join("bad-benchmark.json");
    std::fs::write(
        &bad_benchmark,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.bench.v1",
            "ok": true,
            "public_claims_approved": true,
            "adapter_matrix": {
                "all_required_adapters_accounted": false,
                "blind_install_attempted": true,
                "runnable_adapter_count": 0
            },
            "rows": []
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
            bad_benchmark.to_str().unwrap(),
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
                == "benchmark adapter matrix does not account for all required competitors")
    );
    assert!(
        benchmark_gate["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "benchmark attempted blind install")
    );
    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                == "benchmark adapter matrix does not account for all required competitors")
    );
}

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
}

#[test]
fn cli_claim_audit_rejects_source_artifact_missing_currency_fields() {
    let dir = tempdir().unwrap();
    let source_artifact = dir.path().join("source-currency.json");
    let required_tools = [
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
    ];
    let rows = required_tools
        .iter()
        .enumerate()
        .map(|(idx, tool)| {
            let mut row = serde_json::json!({
                "tool": tool,
                "url": format!("https://github.com/example/{tool}"),
                "claimed_scope": "fixture",
                "issue_pr_themes": ["fixture"],
                "strengths": ["fixture"],
                "gaps": ["fixture"],
                "source_date": "2026-06-04",
                "source_commit": "release-candidate"
            });
            if idx == 0 {
                row.as_object_mut().unwrap().remove("source_commit");
            }
            row
        })
        .collect::<Vec<_>>();
    std::fs::write(
        &source_artifact,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.source_currency.v1",
            "release_candidate_id": "rc-source",
            "ok": true,
            "fresh_for_public_claim": true,
            "rows": rows
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--source-artifact",
            source_artifact.to_str().unwrap(),
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
    let source_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "source_currency")
        .expect("source gate");
    assert_eq!(source_gate["pass"], false);
    assert_eq!(
        source_gate["artifact_path"],
        source_artifact.to_str().unwrap().replace('\\', "/")
    );
    assert_eq!(source_gate["details"]["release_candidate_id"], "rc-source");
    assert_eq!(
        json["gate_artifact_paths"]["source_currency"],
        source_artifact.to_str().unwrap().replace('\\', "/")
    );
    assert!(
        source_gate["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "source ledger row missing source commit")
    );
    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "source ledger row missing source commit")
    );
    assert_eq!(json["public_claims_approved"], false);
}

#[test]
fn cli_claim_audit_rejects_source_artifact_with_snapshot_source_commits() {
    let dir = tempdir().unwrap();
    let source_artifact = dir.path().join("source-currency.json");
    let required_tools = [
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
    ];
    let rows = required_tools
        .iter()
        .enumerate()
        .map(|(idx, tool)| {
            serde_json::json!({
                "tool": tool,
                "url": format!("https://github.com/example/{tool}"),
                "claimed_scope": "fixture",
                "issue_pr_themes": ["fixture"],
                "strengths": ["fixture"],
                "gaps": ["fixture"],
                "source_date": "2026-06-04",
                "source_commit": if idx == 0 { "snapshot-20260604" } else { "abcdef1" }
            })
        })
        .collect::<Vec<_>>();
    std::fs::write(
        &source_artifact,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.source_currency.v1",
            "release_candidate_id": "rc-source",
            "ok": true,
            "fresh_for_public_claim": true,
            "public_claims_approved": true,
            "rows": rows
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--source-artifact",
            source_artifact.to_str().unwrap(),
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
    let source_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "source_currency")
        .expect("source gate");

    assert_eq!(source_gate["pass"], false);
    assert!(
        source_gate["reasons"].as_array().unwrap().iter().any(
            |reason| reason == "source ledger row source commit is not a release-candidate pin"
        )
    );
    assert!(
        source_gate["details"]["unpinned_source_rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["tool"] == "rtk" && row["source_commit"] == "snapshot-20260604")
    );
    assert_eq!(
        source_gate["details"]["source_commit_pin_status"]["unpinned"],
        1
    );
    assert_eq!(json["public_claims_approved"], false);
}

#[test]
fn cli_claim_audit_requires_same_release_candidate_across_supplied_artifacts() {
    let dir = tempdir().unwrap();
    let source_artifact = dir.path().join("source-currency.json");
    let benchmark_artifact = dir.path().join("benchmark.json");
    let adapter_artifact = dir.path().join("adapter-approval.json");
    let recovery_artifact = dir.path().join("recovery.json");
    let task_artifact = dir.path().join("task-success.json");
    let os_artifact = dir.path().join("os.json");

    let required_tools = [
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
    ];
    let source_rows = required_tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "tool": tool,
                "url": format!("https://github.com/example/{tool}"),
                "claimed_scope": "fixture",
                "issue_pr_themes": ["fixture"],
                "strengths": ["fixture"],
                "gaps": ["fixture"],
                "source_date": "2026-06-04",
                "source_commit": "abcdef1"
            })
        })
        .collect::<Vec<_>>();
    std::fs::write(
        &source_artifact,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.source_currency.v1",
            "ok": true,
            "release_candidate_id": "rc-alpha",
            "fresh_for_public_claim": true,
            "rows": source_rows
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        &benchmark_artifact,
        serde_json::to_vec(&serde_json::json!({
                    "schema_version": "tokenzero.bench.v1",
                    "ok": true,
                    "release_candidate_id": "rc-alpha",
                    "public_claims_approved": true,
                    "adapter_matrix": {
                        "all_required_adapters_accounted": true,
                        "blind_install_attempted": false
                    },
                    "rows": [{
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
            {"kind": "combined", "ref": "tz://blob/fixture", "byte_perfect": true}
        ],
        "fairness_notes": "fixture row includes public-claim fields"
        }]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        &adapter_artifact,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_audit.v1",
            "ok": true,
            "release_candidate_id": "rc-beta",
            "execution_allowed": true,
            "public_claims_approved": true,
            "blind_install_attempted": false,
            "required_adapter_count": 11,
            "reviewed_command_count": 11,
            "missing_reviewed_command_count": 0,
            "duplicate_command_count": 0,
            "unsafe_command_count": 0,
            "adapters": reviewed_adapter_rows()
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        &recovery_artifact,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.exact_recovery_audit.v1",
            "ok": true,
            "release_candidate_id": "rc-alpha",
            "normal_rows": [{"all_refs_recover": true}]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        &task_artifact,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.one_shot_eval.v1",
            "ok": true,
            "release_candidate_id": "rc-alpha",
            "critical_miss_rate": 0.0,
            "rows": [{"task_success": true}]
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        &os_artifact,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.os_reach_audit.v1",
            "ok": true,
            "release_candidate_id": "rc-alpha",
            "all_release_oses_run": true,
            "public_os_claim_approved": true
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--source-artifact",
            source_artifact.to_str().unwrap(),
            "--benchmark-artifact",
            benchmark_artifact.to_str().unwrap(),
            "--adapter-approval-artifact",
            adapter_artifact.to_str().unwrap(),
            "--recovery-artifact",
            recovery_artifact.to_str().unwrap(),
            "--task-success-artifact",
            task_artifact.to_str().unwrap(),
            "--os-artifact",
            os_artifact.to_str().unwrap(),
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
    let release_candidate_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "release_candidate")
        .expect("release candidate gate");
    assert_eq!(release_candidate_gate["pass"], false);
    assert!(
        release_candidate_gate["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "evidence artifacts are not from the same release candidate")
    );
    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "evidence artifacts are not from the same release candidate")
    );

    std::fs::write(
        &adapter_artifact,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_audit.v1",
            "ok": true,
            "release_candidate_id": "rc-alpha",
            "execution_allowed": true,
            "public_claims_approved": true,
            "blind_install_attempted": false,
            "required_adapter_count": 11,
            "reviewed_command_count": 11,
            "missing_reviewed_command_count": 0,
            "duplicate_command_count": 0,
            "unsafe_command_count": 0,
            "adapters": reviewed_adapter_rows()
        }))
        .unwrap(),
    )
    .unwrap();
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--source-artifact",
            source_artifact.to_str().unwrap(),
            "--benchmark-artifact",
            benchmark_artifact.to_str().unwrap(),
            "--adapter-approval-artifact",
            adapter_artifact.to_str().unwrap(),
            "--recovery-artifact",
            recovery_artifact.to_str().unwrap(),
            "--task-success-artifact",
            task_artifact.to_str().unwrap(),
            "--os-artifact",
            os_artifact.to_str().unwrap(),
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
    assert_eq!(json["public_claims_approved"], true);
    assert_eq!(json["release_publication_allowed"], true);
    let release_candidate_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "release_candidate")
        .expect("release candidate gate");
    assert_eq!(release_candidate_gate["pass"], true);
    assert!(
        json["claims"]
            .as_array()
            .unwrap()
            .iter()
            .all(|claim| { claim["approved"] == true && claim["public_safe_to_publish"] == true })
    );
}

#[test]
fn cli_claim_evidence_artifacts_emit_release_candidate_id() {
    let dir = tempdir().unwrap();
    let source_json = dir.path().join("source.json");
    let bench_json = dir.path().join("bench.json");
    let adapter_json = dir.path().join("adapter.json");
    let recovery_json = dir.path().join("recovery.json");
    let task_json = dir.path().join("task.json");
    let os_json = dir.path().join("os.json");
    let os_release_json = dir.path().join("os-release.json");
    let claim_json = dir.path().join("claim.json");
    let completion_json = dir.path().join("completion.json");
    let handoff_json = dir.path().join("handoff.json");
    let commands = [
        (
            "source",
            vec![
                "source-currency-audit".to_string(),
                "--output-json".to_string(),
                source_json.display().to_string(),
                "--json".to_string(),
            ],
            source_json.clone(),
        ),
        (
            "benchmark",
            vec![
                "bench".to_string(),
                "competitors".to_string(),
                "--suite".to_string(),
                "shell-heavy".to_string(),
                "--output-json".to_string(),
                bench_json.display().to_string(),
                "--json".to_string(),
            ],
            bench_json.clone(),
        ),
        (
            "adapter",
            vec![
                "adapter-approval-audit".to_string(),
                "--output-json".to_string(),
                adapter_json.display().to_string(),
                "--json".to_string(),
            ],
            adapter_json.clone(),
        ),
        (
            "recovery",
            vec![
                "exact-recovery-audit".to_string(),
                "--output-json".to_string(),
                recovery_json.display().to_string(),
                "--json".to_string(),
            ],
            recovery_json.clone(),
        ),
        (
            "task",
            vec![
                "one-shot-eval".to_string(),
                "--output-json".to_string(),
                task_json.display().to_string(),
                "--json".to_string(),
            ],
            task_json.clone(),
        ),
        (
            "os",
            vec![
                "os-reach-audit".to_string(),
                "--output-json".to_string(),
                os_json.display().to_string(),
                "--json".to_string(),
            ],
            os_json.clone(),
        ),
        (
            "os_release",
            vec![
                "os-release-artifact".to_string(),
                "--output-json".to_string(),
                os_release_json.display().to_string(),
                "--json".to_string(),
            ],
            os_release_json.clone(),
        ),
        (
            "completion",
            vec![
                "completion-audit".to_string(),
                "--output-json".to_string(),
                completion_json.display().to_string(),
                "--json".to_string(),
            ],
            completion_json.clone(),
        ),
        (
            "handoff",
            vec![
                "artifact-handoff".to_string(),
                "--output-json".to_string(),
                handoff_json.display().to_string(),
                "--json".to_string(),
            ],
            handoff_json.clone(),
        ),
    ];

    for (name, args, output_json) in commands {
        let output = Command::cargo_bin("tokenzero")
            .unwrap()
            .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-fixture")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let artifact: Value = serde_json::from_slice(&std::fs::read(&output_json).unwrap())
            .unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(artifact["release_candidate_id"], "rc-fixture", "{name}");
    }

    let claim_output = Command::cargo_bin("tokenzero")
        .unwrap()
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-fixture")
        .args([
            "claim-audit",
            "--source-artifact",
            source_json.to_str().unwrap(),
            "--benchmark-artifact",
            bench_json.to_str().unwrap(),
            "--adapter-approval-artifact",
            adapter_json.to_str().unwrap(),
            "--recovery-artifact",
            recovery_json.to_str().unwrap(),
            "--task-success-artifact",
            task_json.to_str().unwrap(),
            "--os-artifact",
            os_json.to_str().unwrap(),
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
    let claim_artifact: Value = serde_json::from_slice(&std::fs::read(&claim_json).unwrap())
        .unwrap_or_else(|err| panic!("claim: {err}"));
    assert_eq!(
        claim_artifact["release_candidate_id"], "rc-fixture",
        "claim"
    );
}

#[test]
fn cli_completion_audit_maps_requirements_and_blocks_false_closure() {
    let dir = tempdir().unwrap();
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
    assert_eq!(json["schema_version"], "tokenzero.completion_audit.v1");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["completion_status"], "blocked");
    assert_eq!(json["ok"], true);
    assert_eq!(json["completion_achieved"], false);
    assert_eq!(json["final_summary_is_evidence"], false);
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    assert_eq!(json["all_requirement_rows_passed"], false);
    assert_eq!(
        json["blocked_requirement_ids"],
        serde_json::json!(["G-005", "G-008", "FR-007", "FR-010", "NFR-002"])
    );
    assert_eq!(json["requirement_status_counts"]["passed"], 9);
    assert_eq!(json["requirement_status_counts"]["passed_private"], 8);
    assert_eq!(json["requirement_status_counts"]["passed_blocked"], 2);
    assert_eq!(json["requirement_status_counts"]["blocked_public"], 3);

    let goal_rows = json["g_goals"].as_array().unwrap();
    for goal_id in [
        "G-001", "G-002", "G-003", "G-004", "G-005", "G-006", "G-007", "G-008", "G-009", "G-010",
    ] {
        assert!(
            goal_rows.iter().any(|row| row["id"] == goal_id),
            "{goal_id}"
        );
    }

    let must_fr_rows = json["must_fr"].as_array().unwrap();
    for fr_id in [
        "FR-001", "FR-002", "FR-003", "FR-004", "FR-005", "FR-006", "FR-007", "FR-010",
    ] {
        assert!(must_fr_rows.iter().any(|row| row["id"] == fr_id), "{fr_id}");
    }

    let critical_nfr_rows = json["critical_nfr"].as_array().unwrap();
    for nfr_id in ["NFR-001", "NFR-002", "NFR-003", "NFR-004"] {
        assert!(
            critical_nfr_rows.iter().any(|row| row["id"] == nfr_id),
            "{nfr_id}"
        );
    }

    assert!(json["g_goals"].as_array().unwrap().iter().all(|row| {
        row["direct_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .all(|evidence| {
                evidence.as_str().unwrap().starts_with("results/current/")
                    || evidence.as_str().unwrap().starts_with("cargo ")
                    || evidence.as_str().unwrap().contains("validate_prd_goal.py")
                    || evidence.as_str().unwrap().starts_with("docs/")
            })
    }));
    assert!(json["residual_gaps"].as_array().unwrap().iter().any(|gap| {
        gap.as_str()
            .unwrap()
            .contains("shell and install artifacts missing")
    }));
    assert!(
        json["residual_gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap.as_str().unwrap().contains("public claim"))
    );
    assert!(output_json.exists());
}

#[test]
fn cli_completion_audit_reports_direct_evidence_integrity() {
    let dir = tempdir().unwrap();
    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    std::fs::write(
        results_dir.join("tokenzero_claim_audit.json"),
        r#"{"schema_version":"tokenzero.source_currency.v1"}"#,
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_artifact_handoff.json"),
        r#"{"schema_version":"tokenzero.claim_audit.v1"}"#,
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_source_currency.json"),
        r#"{"schema_version":"tokenzero.source_currency.v1","release_candidate_id":"rc-other"}"#,
    )
    .unwrap();
    let docs_dir = dir.path().join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();
    std::fs::write(
        docs_dir.join("advanced-adr-execution-record.md"),
        "## ADR-000 Evidence Fixture\nFailure-first evidence:\nResidual gates:\n",
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_competitive_superiority_reconciliation.md"),
        "reconciliation fixture without gate evidence\n",
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-current")
        .args(["completion-audit", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let integrity = json["evidence_integrity_matrix"]
        .as_array()
        .expect("completion audit exposes evidence integrity matrix");

    let claim_artifact = integrity
        .iter()
        .find(|row| {
            row["requirement_id"] == "G-008"
                && row["evidence"] == "results/current/tokenzero_claim_audit.json"
        })
        .expect("claim audit evidence row");
    assert_eq!(claim_artifact["evidence_kind"], "artifact");
    assert_eq!(claim_artifact["present"], true);
    assert_eq!(claim_artifact["status"], "invalid");
    assert_eq!(
        claim_artifact["schema_version"],
        "tokenzero.source_currency.v1"
    );
    assert_eq!(
        claim_artifact["expected_schema_version"],
        "tokenzero.claim_audit.v1"
    );
    assert_eq!(claim_artifact["schema_matches"], false);
    assert_eq!(claim_artifact["artifact_valid"], false);
    assert!(
        claim_artifact["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "schema_version mismatch")
    );
    assert_eq!(claim_artifact["requirement_status"], "passed_blocked");
    assert_eq!(
        claim_artifact["requirement_residual"],
        "public claim approval intentionally false until claim audit evidence gates pass"
    );

    let source_artifact = integrity
        .iter()
        .find(|row| {
            row["requirement_id"] == "G-001"
                && row["evidence"] == "results/current/tokenzero_source_currency.json"
        })
        .expect("source currency evidence row");
    assert_eq!(source_artifact["evidence_kind"], "artifact");
    assert_eq!(source_artifact["present"], true);
    assert_eq!(source_artifact["status"], "invalid");
    assert_eq!(
        source_artifact["schema_version"],
        "tokenzero.source_currency.v1"
    );
    assert_eq!(
        source_artifact["expected_schema_version"],
        "tokenzero.source_currency.v1"
    );
    assert_eq!(source_artifact["schema_matches"], true);
    assert_eq!(source_artifact["release_candidate_id"], "rc-other");
    assert_eq!(
        source_artifact["expected_release_candidate_id"],
        "rc-current"
    );
    assert_eq!(source_artifact["release_candidate_matches"], false);
    assert_eq!(source_artifact["artifact_valid"], false);
    assert!(
        source_artifact["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "release_candidate_id mismatch")
    );

    let handoff_artifact = integrity
        .iter()
        .find(|row| {
            row["requirement_id"] == "G-010"
                && row["evidence"] == "results/current/tokenzero_artifact_handoff.json"
        })
        .expect("artifact handoff evidence row");
    assert_eq!(handoff_artifact["evidence_kind"], "artifact");
    assert_eq!(handoff_artifact["present"], true);
    assert_eq!(handoff_artifact["status"], "invalid");
    assert_eq!(
        handoff_artifact["schema_version"],
        "tokenzero.claim_audit.v1"
    );
    assert_eq!(
        handoff_artifact["expected_schema_version"],
        "tokenzero.artifact_handoff.v1"
    );
    assert_eq!(handoff_artifact["schema_matches"], false);

    let adr_artifact = integrity
        .iter()
        .find(|row| {
            row["requirement_id"] == "G-010"
                && row["evidence"] == "docs/advanced-adr-execution-record.md"
        })
        .expect("ADR evidence row");
    assert_eq!(adr_artifact["evidence_kind"], "artifact");
    assert_eq!(adr_artifact["present"], true);
    assert_eq!(adr_artifact["status"], "invalid");
    assert_eq!(
        adr_artifact["expected_content_markers"],
        serde_json::json!([
            "## ADR-",
            "Failure-first evidence:",
            "Residual gates:",
            "validate_prd_goal.py",
            "cargo test --workspace"
        ])
    );
    assert_eq!(adr_artifact["content_markers_present"], false);
    assert_eq!(adr_artifact["artifact_valid"], false);
    assert!(
        adr_artifact["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "content marker missing: validate_prd_goal.py")
    );

    let reconciliation_artifact = integrity
        .iter()
        .find(|row| {
            row["requirement_id"] == "G-010"
                && row["evidence"]
                    == "results/current/tokenzero_competitive_superiority_reconciliation.md"
        })
        .expect("reconciliation evidence row");
    assert_eq!(reconciliation_artifact["evidence_kind"], "artifact");
    assert_eq!(reconciliation_artifact["present"], true);
    assert_eq!(reconciliation_artifact["status"], "invalid");
    assert_eq!(
        reconciliation_artifact["expected_content_markers"],
        serde_json::json!(["Snapshot", "no gated action was performed"])
    );
    assert_eq!(reconciliation_artifact["content_markers_present"], false);
    assert_eq!(reconciliation_artifact["artifact_valid"], false);
    assert!(
        reconciliation_artifact["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "content marker missing: Snapshot")
    );
    assert_eq!(handoff_artifact["artifact_valid"], false);
    assert!(
        handoff_artifact["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "schema_version mismatch")
    );

    let command_evidence = integrity
        .iter()
        .find(|row| row["requirement_id"] == "G-006" && row["evidence"] == "cargo test --workspace")
        .expect("workspace test command evidence row");
    assert_eq!(command_evidence["evidence_kind"], "command");
    assert_eq!(command_evidence["present"], serde_json::Value::Null);
    assert_eq!(command_evidence["status"], "command_evidence");
    assert_eq!(command_evidence["schema_version"], serde_json::Value::Null);
    assert_eq!(
        command_evidence["expected_schema_version"],
        serde_json::Value::Null
    );
    assert_eq!(command_evidence["schema_matches"], serde_json::Value::Null);
    assert_eq!(command_evidence["artifact_valid"], serde_json::Value::Null);
    assert_eq!(command_evidence["requirement_status"], "passed");
    assert_eq!(
        command_evidence["requirement_residual"],
        serde_json::Value::Null
    );

    assert!(
        integrity
            .iter()
            .any(|row| row["status"] == "missing" && row["evidence_kind"] == "artifact"),
        "missing fixture artifacts should be visible instead of silently treated as proof"
    );
    assert_eq!(json["all_direct_file_evidence_present"], false);
    assert_eq!(json["all_direct_artifact_evidence_valid"], false);
}

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

#[test]
fn cli_security_privacy_audit_proves_local_raw_payload_and_root_guards() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("security.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "security-privacy-audit",
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
    assert_eq!(
        json["schema_version"],
        "tokenzero.security_privacy_audit.v1"
    );
    assert_eq!(json["ok"], true);
    assert_eq!(json["raw_payloads_local_by_default"], true);
    assert_eq!(json["pulse_records_raw_payload"], false);
    assert_eq!(json["secret_masking_active"], true);
    assert_eq!(json["allowed_root_controls_active"], true);
    assert_eq!(json["unapproved_external_writes"], false);
    assert_eq!(json["release_publication_allowed"], false);

    for row_id in [
        "cli_visible_secret_masking",
        "exact_ref_local_recovery",
        "pulse_no_raw_payload",
        "mcp_allowed_root_enforced",
        "no_unapproved_external_writes",
    ] {
        assert!(
            json["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == row_id && row["pass"] == true),
            "{row_id}"
        );
    }
    assert!(output_json.exists());
}

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
