use assert_cmd::prelude::*;
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

use super::common::*;

/// Generate the standard source_currency rows for the 11 required adapter tools.
fn source_currency_rows() -> Vec<Value> {
    required_adapter_tools()
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
        .collect()
}

/// Standard paths for the 6 evidence artifacts.
fn evidence_artifact_paths(dir: &Path) -> [std::path::PathBuf; 6] {
    [
        dir.join("source-currency.json"),
        dir.join("benchmark.json"),
        dir.join("adapter-approval.json"),
        dir.join("recovery.json"),
        dir.join("task-success.json"),
        dir.join("os.json"),
    ]
}

/// Write the 6 evidence artifact files with mismatched adapter RC ("rc-beta")
/// while the rest use the given `rc_id`.
fn write_mismatched_rc_fixtures(dir: &Path, rc_id: &str) {
    let source_rows: Vec<Value> = required_adapter_tools()
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
        .collect();
    let paths = evidence_artifact_paths(dir);
    write_json_fixture(
        &paths[0],
        &serde_json::json!({
            "schema_version": "tokenzero.source_currency.v1",
            "ok": true, "release_candidate_id": rc_id,
            "fresh_for_public_claim": true, "rows": source_rows
        }),
    );
    write_json_fixture(
        &paths[1],
        &serde_json::json!({
            "schema_version": "tokenzero.bench.v1",
            "ok": true, "release_candidate_id": rc_id,
            "public_claims_approved": true,
            "adapter_matrix": {"all_required_adapters_accounted": true, "blind_install_attempted": false},
            "rows": [{
                "tool": "tokenzero", "suite": "shell-heavy",
                "availability_status": "run", "raw_tokens": 100, "visible_tokens": 25,
                "recovery_tokens": 75, "safe_savings": 0.75, "harm_rate": 0.0,
                "task_success": true, "byte_perfect_recovery": true,
                "exact_expand_checks": [{"kind": "combined", "ref": "tz://blob/fixture", "byte_perfect": true}],
                "fairness_notes": "fixture row includes public-claim fields"
            }]
        }),
    );
    // Mismatched RC: adapter uses "rc-beta" while others use `rc_id`
    write_json_fixture(
        &paths[2],
        &serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_audit.v1",
            "ok": true, "release_candidate_id": "rc-beta",
            "execution_allowed": true, "public_claims_approved": true,
            "blind_install_attempted": false,
            "required_adapter_count": 11, "reviewed_command_count": 11,
            "missing_reviewed_command_count": 0, "duplicate_command_count": 0,
            "unsafe_command_count": 0, "adapters": reviewed_adapter_rows()
        }),
    );
    write_json_fixture(
        &paths[3],
        &serde_json::json!({
            "schema_version": "tokenzero.exact_recovery_audit.v1",
            "ok": true, "release_candidate_id": rc_id,
            "normal_rows": [{"all_refs_recover": true}]
        }),
    );
    write_json_fixture(
        &paths[4],
        &serde_json::json!({
            "schema_version": "tokenzero.one_shot_eval.v1",
            "ok": true, "release_candidate_id": rc_id,
            "critical_miss_rate": 0.0, "rows": [{"task_success": true}]
        }),
    );
    write_json_fixture(
        &paths[5],
        &serde_json::json!({
            "schema_version": "tokenzero.os_reach_audit.v1",
            "ok": true, "release_candidate_id": rc_id,
            "all_release_oses_run": true, "public_os_claim_approved": true
        }),
    );
}

/// Fix the adapter artifact to use a consistent RC ID.
fn fix_adapter_rc(dir: &Path, rc_id: &str) {
    let paths = evidence_artifact_paths(dir);
    write_json_fixture(
        &paths[2],
        &serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_audit.v1",
            "ok": true, "release_candidate_id": rc_id,
            "execution_allowed": true, "public_claims_approved": true,
            "blind_install_attempted": false,
            "required_adapter_count": 11, "reviewed_command_count": 11,
            "missing_reviewed_command_count": 0, "duplicate_command_count": 0,
            "unsafe_command_count": 0, "adapters": reviewed_adapter_rows()
        }),
    );
}

/// Run `claim-audit` with all 6 evidence artifacts + `--release-approval --json`.
fn run_claim_audit_with_all_artifacts(dir: &Path) -> Value {
    let paths = evidence_artifact_paths(dir);
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--source-artifact",
            paths[0].to_str().unwrap(),
            "--benchmark-artifact",
            paths[1].to_str().unwrap(),
            "--adapter-approval-artifact",
            paths[2].to_str().unwrap(),
            "--recovery-artifact",
            paths[3].to_str().unwrap(),
            "--task-success-artifact",
            paths[4].to_str().unwrap(),
            "--os-artifact",
            paths[5].to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Assert the release_candidate gate passes and all claims are approved.
fn assert_rc_gate_passes(json: &Value) {
    assert_eq!(json["public_claims_approved"], true);
    assert_eq!(json["release_publication_allowed"], true);
    let rc_gate = find_gate(json, "release_candidate");
    assert_eq!(rc_gate["pass"], true);
    assert!(
        json["claims"]
            .as_array()
            .unwrap()
            .iter()
            .all(|claim| { claim["approved"] == true && claim["public_safe_to_publish"] == true })
    );
}

/// Assert the release_candidate gate fails with the given reason.
fn assert_rc_gate_fails(json: &Value, expected_reason: &str) {
    assert_eq!(json["public_claims_approved"], false);
    let rc_gate = find_gate(json, "release_candidate");
    assert_eq!(rc_gate["pass"], false);
    assert_reason(rc_gate, expected_reason);
    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r == expected_reason)
    );
}

/// Find an evidence gate by id.
fn find_gate<'a>(json: &'a Value, gate_id: &str) -> &'a Value {
    json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == gate_id)
        .unwrap_or_else(|| panic!("evidence gate not found: {gate_id}"))
}

/// Write the 6 `results/current/` fixture files.
fn write_all_results_current_fixtures(dir: &Path, release_candidate_id: &str) {
    let results_dir = dir.join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    let tools = required_adapter_tools();
    write_json_fixture(
        &results_dir.join("tokenzero_source_currency.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.source_currency.v1",
            "release_candidate_id": release_candidate_id,
            "ok": true, "fresh_for_public_claim": true,
            "public_claims_approved": false, "release_publication_allowed": false,
            "rows": source_currency_rows()
        }),
    );
    write_json_fixture(
        &results_dir.join("tokenzero_bench_competitors_shell_heavy.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.bench.v1",
            "release_candidate_id": release_candidate_id,
            "ok": true, "public_claims_approved": false, "release_publication_allowed": false,
            "adapter_matrix": {"all_required_adapters_accounted": true, "blind_install_attempted": false},
            "rows": [
                {
                    "tool": "tokenzero", "suite": "shell-heavy", "scenario_id": "read",
                    "availability_status": "run", "raw_tokens": 100, "visible_tokens": 20,
                    "recovery_tokens": 0, "safe_savings": 0.8, "harm_rate": 0.0,
                    "task_success": true, "fairness_notes": "fixture"
                },
                {
                    "tool": "rtk", "suite": "shell-heavy", "scenario_id": "read",
                    "availability_status": "unavailable", "raw_tokens": 0, "visible_tokens": 0,
                    "recovery_tokens": 0, "safe_savings": 0.0, "harm_rate": 0.0,
                    "task_success": false, "fairness_notes": "fixture unavailable"
                }
            ]
        }),
    );
    write_json_fixture(
        &results_dir.join("tokenzero_adapter_approval_audit.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_audit.v1",
            "release_candidate_id": release_candidate_id,
            "execution_allowed": false, "public_claims_approved": false,
            "blind_install_attempted": false,
            "required_adapter_count": tools.len(),
            "reviewed_command_count": tools.len(),
            "missing_reviewed_command_count": 0, "duplicate_command_count": 0,
            "unsafe_command_count": 0, "adapters": reviewed_adapter_rows()
        }),
    );
    write_json_fixture(
        &results_dir.join("tokenzero_exact_recovery_audit.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.exact_recovery_audit.v1",
            "release_candidate_id": release_candidate_id,
            "ok": true, "normal_rows": [{"id": "fixture", "all_refs_recover": true}]
        }),
    );
    write_json_fixture(
        &results_dir.join("tokenzero_one_shot_eval.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.one_shot_eval.v1",
            "release_candidate_id": release_candidate_id,
            "ok": true, "critical_miss_rate": 0.0, "public_claims_approved": false,
            "release_publication_allowed": false,
            "rows": [{"trace_id": "fixture", "task_success": true}]
        }),
    );
    write_json_fixture(
        &results_dir.join("tokenzero_os_reach_audit.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.os_reach_audit.v1",
            "release_candidate_id": release_candidate_id,
            "ok": true, "all_release_oses_run": false, "public_os_claim_approved": false
        }),
    );
}

/// Find a row in the evidence_integrity_matrix by requirement_id and evidence path.
fn find_integrity_row<'a>(integrity: &'a [Value], req_id: &str, evidence: &str) -> &'a Value {
    integrity
        .iter()
        .find(|row| row["requirement_id"] == req_id && row["evidence"] == evidence)
        .unwrap_or_else(|| panic!("integrity row not found: {req_id} / {evidence}"))
}

/// Assert standard fields on an integrity matrix row.
fn assert_integrity_row_fields(
    row: &Value,
    expected_kind: &str,
    expected_present: Value,
    expected_status: &str,
    expected_valid: Value,
) {
    assert_eq!(row["evidence_kind"], expected_kind);
    assert_eq!(row["present"], expected_present);
    assert_eq!(row["status"], expected_status);
    assert_eq!(row["artifact_valid"], expected_valid);
}

type RcEmissionCommands = (
    Vec<(&'static str, Vec<String>, std::path::PathBuf)>,
    [std::path::PathBuf; 7],
);

/// Build the 9 (name, args, output_json) command definitions for RC emission tests.
/// Returns (commands, [source, bench, adapter, recovery, task, os, claim] artifact paths).
fn rc_emission_test_commands(dir: &Path) -> RcEmissionCommands {
    let paths = [
        dir.join("source.json"),
        dir.join("bench.json"),
        dir.join("adapter.json"),
        dir.join("recovery.json"),
        dir.join("task.json"),
        dir.join("os.json"),
        dir.join("claim.json"),
    ];
    let commands = vec![
        (
            "source",
            vec![
                "source-currency-audit".into(),
                "--output-json".into(),
                paths[0].display().to_string(),
                "--json".into(),
            ],
            paths[0].clone(),
        ),
        (
            "benchmark",
            vec![
                "bench".into(),
                "competitors".into(),
                "--suite".into(),
                "shell-heavy".into(),
                "--output-json".into(),
                paths[1].display().to_string(),
                "--json".into(),
            ],
            paths[1].clone(),
        ),
        (
            "adapter",
            vec![
                "adapter-approval-audit".into(),
                "--output-json".into(),
                paths[2].display().to_string(),
                "--json".into(),
            ],
            paths[2].clone(),
        ),
        (
            "recovery",
            vec![
                "exact-recovery-audit".into(),
                "--output-json".into(),
                paths[3].display().to_string(),
                "--json".into(),
            ],
            paths[3].clone(),
        ),
        (
            "task",
            vec![
                "one-shot-eval".into(),
                "--output-json".into(),
                paths[4].display().to_string(),
                "--json".into(),
            ],
            paths[4].clone(),
        ),
        (
            "os",
            vec![
                "os-reach-audit".into(),
                "--output-json".into(),
                paths[5].display().to_string(),
                "--json".into(),
            ],
            paths[5].clone(),
        ),
        (
            "os_release",
            vec![
                "os-release-artifact".into(),
                "--output-json".into(),
                dir.join("os-release.json").display().to_string(),
                "--json".into(),
            ],
            dir.join("os-release.json"),
        ),
        (
            "completion",
            vec![
                "completion-audit".into(),
                "--output-json".into(),
                dir.join("completion.json").display().to_string(),
                "--json".into(),
            ],
            dir.join("completion.json"),
        ),
        (
            "handoff",
            vec![
                "artifact-handoff".into(),
                "--output-json".into(),
                dir.join("handoff.json").display().to_string(),
                "--json".into(),
            ],
            dir.join("handoff.json"),
        ),
    ];
    (commands, paths)
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
fn cli_claim_audit_uses_results_current_artifacts_without_explicit_paths() {
    let dir = tempdir().unwrap();
    let release_candidate_id = "rc-current-defaults";
    write_all_results_current_fixtures(dir.path(), release_candidate_id);

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
    let source_gate = find_gate(&json, "source_currency");
    let rc_gate = find_gate(&json, "release_candidate");

    // Oracle: source gate passes and artifact path is forward-slash relative
    assert_eq!(source_gate["pass"], true);
    let source_artifact_path = json["gate_artifact_paths"]["source_currency"]
        .as_str()
        .unwrap();
    assert!(!source_artifact_path.contains('\\'));
    assert_eq!(
        source_artifact_path,
        "results/current/tokenzero_source_currency.json"
    );

    // Oracle: all 6 artifacts attached under the current RC
    assert_eq!(rc_gate["details"]["attached_artifact_count"], 6);
    assert_eq!(
        rc_gate["details"]["release_candidate_ids"],
        serde_json::json!([release_candidate_id])
    );
    assert!(
        rc_gate["details"]["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|artifact| artifact["artifact_path"].as_str())
            .all(|path| !path.contains('\\'))
    );

    // Oracle: no stale reasons emitted — current artifacts are consumed directly
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
fn cli_claim_audit_requires_same_release_candidate_across_supplied_artifacts() {
    let dir = tempdir().unwrap();
    // Phase 1: mismatched adapter RC ("rc-beta" vs "rc-alpha") → gate fails
    write_mismatched_rc_fixtures(dir.path(), "rc-alpha");
    let json = run_claim_audit_with_all_artifacts(dir.path());
    assert_rc_gate_fails(
        &json,
        "evidence artifacts are not from the same release candidate",
    );

    // Phase 2: fix adapter to "rc-alpha" → gate passes
    fix_adapter_rc(dir.path(), "rc-alpha");
    let json = run_claim_audit_with_all_artifacts(dir.path());
    assert_rc_gate_passes(&json);
}

#[test]
fn cli_claim_evidence_artifacts_emit_release_candidate_id() {
    let dir = tempdir().unwrap();
    let (commands, paths) = rc_emission_test_commands(dir.path());

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

    // Oracle: claim-audit itself emits the RC ID
    let claim_output = Command::cargo_bin("tokenzero")
        .unwrap()
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-fixture")
        .args([
            "claim-audit",
            "--source-artifact",
            paths[0].to_str().unwrap(),
            "--benchmark-artifact",
            paths[1].to_str().unwrap(),
            "--adapter-approval-artifact",
            paths[2].to_str().unwrap(),
            "--recovery-artifact",
            paths[3].to_str().unwrap(),
            "--task-success-artifact",
            paths[4].to_str().unwrap(),
            "--os-artifact",
            paths[5].to_str().unwrap(),
            "--output-json",
            paths[6].to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        claim_output.status.success(),
        "{}",
        String::from_utf8_lossy(&claim_output.stderr)
    );
    let claim_artifact: Value = serde_json::from_slice(&std::fs::read(&paths[6]).unwrap())
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

    // Oracle: claim audit artifact has wrong schema_version
    let claim_row = find_integrity_row(
        integrity,
        "G-008",
        "results/current/tokenzero_claim_audit.json",
    );
    assert_integrity_row_fields(
        claim_row,
        "artifact",
        serde_json::json!(true),
        "invalid",
        serde_json::json!(false),
    );
    assert_eq!(claim_row["schema_version"], "tokenzero.source_currency.v1");
    assert_eq!(
        claim_row["expected_schema_version"],
        "tokenzero.claim_audit.v1"
    );
    assert_eq!(claim_row["schema_matches"], false);
    assert_reason(claim_row, "schema_version mismatch");
    assert_eq!(claim_row["requirement_status"], "passed_blocked");
    assert_eq!(
        claim_row["requirement_residual"],
        "public claim approval intentionally false until claim audit evidence gates pass"
    );

    // Oracle: source currency has wrong release_candidate_id
    let source_row = find_integrity_row(
        integrity,
        "G-001",
        "results/current/tokenzero_source_currency.json",
    );
    assert_integrity_row_fields(
        source_row,
        "artifact",
        serde_json::json!(true),
        "invalid",
        serde_json::json!(false),
    );
    assert_eq!(source_row["schema_version"], "tokenzero.source_currency.v1");
    assert_eq!(
        source_row["expected_schema_version"],
        "tokenzero.source_currency.v1"
    );
    assert_eq!(source_row["schema_matches"], true);
    assert_eq!(source_row["release_candidate_id"], "rc-other");
    assert_eq!(source_row["expected_release_candidate_id"], "rc-current");
    assert_eq!(source_row["release_candidate_matches"], false);
    assert_reason(source_row, "release_candidate_id mismatch");

    // Oracle: artifact handoff has wrong schema_version
    let handoff_row = find_integrity_row(
        integrity,
        "G-010",
        "results/current/tokenzero_artifact_handoff.json",
    );
    assert_integrity_row_fields(
        handoff_row,
        "artifact",
        serde_json::json!(true),
        "invalid",
        serde_json::json!(false),
    );
    assert_eq!(handoff_row["schema_version"], "tokenzero.claim_audit.v1");
    assert_eq!(
        handoff_row["expected_schema_version"],
        "tokenzero.artifact_handoff.v1"
    );
    assert_eq!(handoff_row["schema_matches"], false);
    assert_reason(handoff_row, "schema_version mismatch");

    // Oracle: ADR doc is missing expected content markers
    let adr_row = find_integrity_row(integrity, "G-010", "docs/advanced-adr-execution-record.md");
    assert_integrity_row_fields(
        adr_row,
        "artifact",
        serde_json::json!(true),
        "invalid",
        serde_json::json!(false),
    );
    assert_eq!(
        adr_row["expected_content_markers"],
        serde_json::json!([
            "## ADR-",
            "Failure-first evidence:",
            "Residual gates:",
            "validate_prd_goal.py",
            "cargo test --workspace"
        ])
    );
    assert_eq!(adr_row["content_markers_present"], false);
    assert_reason(adr_row, "content marker missing: validate_prd_goal.py");

    // Oracle: reconciliation doc is missing Snapshot marker
    let recon_row = find_integrity_row(
        integrity,
        "G-010",
        "results/current/tokenzero_competitive_superiority_reconciliation.md",
    );
    assert_integrity_row_fields(
        recon_row,
        "artifact",
        serde_json::json!(true),
        "invalid",
        serde_json::json!(false),
    );
    assert_eq!(
        recon_row["expected_content_markers"],
        serde_json::json!(["Snapshot", "no gated action was performed"])
    );
    assert_eq!(recon_row["content_markers_present"], false);
    assert_reason(recon_row, "content marker missing: Snapshot");

    // Oracle: command evidence is unverifiable (null fields) but requirement passes
    let cmd_row = find_integrity_row(integrity, "G-006", "cargo test --workspace");
    assert_integrity_row_fields(
        cmd_row,
        "command",
        serde_json::Value::Null,
        "command_evidence",
        serde_json::Value::Null,
    );
    assert_eq!(cmd_row["schema_version"], serde_json::Value::Null);
    assert_eq!(cmd_row["expected_schema_version"], serde_json::Value::Null);
    assert_eq!(cmd_row["schema_matches"], serde_json::Value::Null);
    assert_eq!(cmd_row["requirement_status"], "passed");
    assert_eq!(cmd_row["requirement_residual"], serde_json::Value::Null);

    // Oracle: missing artifacts are visible, not silently treated as proof
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
