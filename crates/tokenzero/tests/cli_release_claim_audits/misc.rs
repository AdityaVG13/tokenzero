use serde_json::Value;
use std::path::Path;
use tempfile::tempdir;

use super::common::*;

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
    write_json_fixture(&paths[2], &adapter_approval_audit_fixture("rc-beta", true));
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

fn fix_adapter_rc(dir: &Path, rc_id: &str) {
    write_json_fixture(
        &evidence_artifact_paths(dir)[2],
        &adapter_approval_audit_fixture(rc_id, true),
    );
}

fn assert_rc_gate_passes(json: &Value) {
    assert_eq!(json["public_claims_approved"], true);
    assert_eq!(json["release_publication_allowed"], true);
    assert_eq!(find_gate(json, "release_candidate")["pass"], true);
    assert!(json["claims"].as_array().unwrap().iter().all(|claim| {
        claim["approved"] == true && claim["public_safe_to_publish"] == true
    }));
}

fn assert_rc_gate_fails(json: &Value, expected_reason: &str) {
    assert_eq!(json["public_claims_approved"], false);
    let rc_gate = find_gate(json, "release_candidate");
    assert_eq!(rc_gate["pass"], false);
    assert_reason(rc_gate, expected_reason);
    assert_blocked_reason(json, expected_reason);
}

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

fn find_integrity_row<'a>(integrity: &'a [Value], req_id: &str, evidence: &str) -> &'a Value {
    integrity
        .iter()
        .find(|row| row["requirement_id"] == req_id && row["evidence"] == evidence)
        .unwrap_or_else(|| panic!("integrity row not found: {req_id} / {evidence}"))
}

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
    let command_specs: [(&str, &[&str], usize); 6] = [
        ("source", &["source-currency-audit"], 0),
        ("benchmark", &["bench", "competitors", "--suite", "shell-heavy"], 1),
        ("adapter", &["adapter-approval-audit"], 2),
        ("recovery", &["exact-recovery-audit"], 3),
        ("task", &["one-shot-eval"], 4),
        ("os", &["os-reach-audit"], 5),
    ];
    let mut commands = Vec::new();
    for (name, base, idx) in command_specs {
        let mut args: Vec<String> = base.iter().map(|s| (*s).to_string()).collect();
        args.extend([
            "--output-json".to_string(),
            paths[idx].display().to_string(),
            "--json".to_string(),
        ]);
        commands.push((name, args, paths[idx].clone()));
    }
    for (name, sub, file) in [
        ("os_release", "os-release-artifact", "os-release.json"),
        ("completion", "completion-audit", "completion.json"),
        ("handoff", "artifact-handoff", "handoff.json"),
    ] {
        let out = dir.join(file);
        commands.push((
            name,
            vec![
                sub.to_string(),
                "--output-json".to_string(),
                out.display().to_string(),
                "--json".to_string(),
            ],
            out,
        ));
    }
    (commands, paths)
}

#[test]
fn cli_claim_audit_blocks_public_claims_without_release_approval() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("claims.json");
    let json = parse_json_stdout(&assert_success(
        tokenzero_cmd()
            .current_dir(dir.path())
            .args([
                "claim-audit",
                "--output-json",
                output_json.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap(),
        "claim-audit blocked",
    ));
    assert_eq!(json["schema_version"], "tokenzero.claim_audit.v1");
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["transport_status"], "ok");
    assert_eq!(json["claim_status"], "blocked");
    assert_eq!(json["release_publication_allowed"], false);
    assert_eq!(json["gate_passes"]["release_approval"], false);
    assert_eq!(json["gate_passes"]["benchmark_artifact"], false);
    assert_reason_contains(
        json["gate_reasons"]["release_approval"].as_array().unwrap(),
        "release approval not granted",
    );
    assert_eq!(json["release_candidate_ids"], serde_json::json!([]));
    let release_candidate_artifacts = json["release_candidate_artifacts"].as_array().unwrap();
    assert_eq!(release_candidate_artifacts.len(), 6);
    assert!(release_candidate_artifacts.iter().all(|artifact| {
        artifact["release_candidate_id"] == serde_json::Value::Null
            && artifact["artifact_path"] == serde_json::Value::Null
    }));
    assert_blocked_reason(&json, "release approval not granted");
    assert!(json["claims"].as_array().unwrap().iter().all(|claim| {
        claim["approved"] == false && claim["public_safe_to_publish"] == false
    }));
    assert!(output_json.exists());
}

#[test]
fn cli_claim_audit_uses_results_current_artifacts_without_explicit_paths() {
    let dir = tempdir().unwrap();
    let release_candidate_id = "rc-current-defaults";
    write_all_results_current_fixtures(dir.path(), release_candidate_id);
    let json = run_tokenzero_json_in_with_env(
        &["claim-audit", "--json"],
        dir.path(),
        &[("TOKENZERO_RELEASE_CANDIDATE_ID", release_candidate_id)],
    );
    let source_gate = find_gate(&json, "source_currency");
    let rc_gate = find_gate(&json, "release_candidate");
    assert_eq!(source_gate["pass"], true);
    let source_artifact_path = json["gate_artifact_paths"]["source_currency"]
        .as_str()
        .unwrap();
    assert!(!source_artifact_path.contains('\\'));
    assert_eq!(
        source_artifact_path,
        "results/current/tokenzero_source_currency.json"
    );
    assert_eq!(rc_gate["details"]["attached_artifact_count"], 6);
    assert_eq!(
        rc_gate["details"]["release_candidate_ids"],
        serde_json::json!([release_candidate_id])
    );
    assert!(rc_gate["details"]["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|artifact| artifact["artifact_path"].as_str())
        .all(|path| !path.contains('\\')));
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
    write_json_fixture(
        &bad_benchmark,
        &serde_json::json!({
            "schema_version": "tokenzero.bench.v1",
            "ok": true,
            "public_claims_approved": true,
            "adapter_matrix": {
                "all_required_adapters_accounted": false,
                "blind_install_attempted": true,
                "runnable_adapter_count": 0
            },
            "rows": []
        }),
    );
    let json = parse_json_stdout(&assert_success(
        tokenzero_cmd()
            .args([
                "claim-audit",
                "--release-approval",
                "--benchmark-artifact",
                bad_benchmark.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap(),
        "claim fail-closed",
    ));
    assert_eq!(json["public_claims_approved"], false);
    let benchmark_gate = find_gate(&json, "benchmark_artifact");
    assert_eq!(benchmark_gate["pass"], false);
    for reason in [
        "benchmark adapter matrix does not account for all required competitors",
        "benchmark attempted blind install",
    ] {
        assert_reason(benchmark_gate, reason);
    }
    assert_blocked_reason(
        &json,
        "benchmark adapter matrix does not account for all required competitors",
    );
}

#[test]
fn cli_claim_audit_requires_same_release_candidate_across_supplied_artifacts() {
    let dir = tempdir().unwrap();
    write_mismatched_rc_fixtures(dir.path(), "rc-alpha");
    let json = run_claim_audit_with_all_artifacts(dir.path());
    assert_rc_gate_fails(
        &json,
        "evidence artifacts are not from the same release candidate",
    );
    fix_adapter_rc(dir.path(), "rc-alpha");
    let json = run_claim_audit_with_all_artifacts(dir.path());
    assert_rc_gate_passes(&json);
}

#[test]
fn cli_claim_evidence_artifacts_emit_release_candidate_id() {
    let dir = tempdir().unwrap();
    let (commands, paths) = rc_emission_test_commands(dir.path());
    for (name, args, output_json) in commands {
        let output = assert_success(
            tokenzero_cmd()
                .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-fixture")
                .args(&args)
                .output()
                .unwrap(),
            name,
        );
        let _ = output;
        let artifact: Value = serde_json::from_slice(&std::fs::read(&output_json).unwrap())
            .unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(artifact["release_candidate_id"], "rc-fixture", "{name}");
    }
    let claim_output = assert_success(
        tokenzero_cmd()
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
            .unwrap(),
        "claim emit rc",
    );
    let _ = claim_output;
    let claim_artifact: Value = serde_json::from_slice(&std::fs::read(&paths[6]).unwrap())
        .unwrap_or_else(|err| panic!("claim: {err}"));
    assert_eq!(claim_artifact["release_candidate_id"], "rc-fixture", "claim");
}

#[test]
fn cli_completion_audit_maps_requirements_and_blocks_false_closure() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("completion.json");
    let json = parse_json_stdout(&assert_success(
        tokenzero_cmd()
            .current_dir(dir.path())
            .args([
                "completion-audit",
                "--output-json",
                output_json.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap(),
        "completion audit",
    ));
    assert_eq!(json["schema_version"], "tokenzero.completion_audit.v1");
    for (k, v) in [
        ("status", serde_json::json!("ok")),
        ("completion_status", serde_json::json!("blocked")),
        ("ok", serde_json::json!(true)),
        ("completion_achieved", serde_json::json!(false)),
        ("final_summary_is_evidence", serde_json::json!(false)),
        ("public_claims_approved", serde_json::json!(false)),
        ("release_publication_allowed", serde_json::json!(false)),
        ("all_requirement_rows_passed", serde_json::json!(false)),
    ] {
        assert_eq!(json[k], v, "{k}");
    }
    assert_eq!(
        json["blocked_requirement_ids"],
        serde_json::json!(["G-005", "G-008", "FR-007", "FR-010", "NFR-002"])
    );
    for (k, n) in [
        ("passed", 9),
        ("passed_private", 8),
        ("passed_blocked", 2),
        ("blocked_public", 3),
    ] {
        assert_eq!(json["requirement_status_counts"][k], n, "{k}");
    }
    let goal_rows = json["g_goals"].as_array().unwrap();
    for goal_id in [
        "G-001", "G-002", "G-003", "G-004", "G-005", "G-006", "G-007", "G-008", "G-009", "G-010",
    ] {
        assert!(goal_rows.iter().any(|row| row["id"] == goal_id), "{goal_id}");
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
        row["direct_evidence"].as_array().unwrap().iter().all(|evidence| {
            let s = evidence.as_str().unwrap();
            s.starts_with("results/current/")
                || s.starts_with("cargo ")
                || s.contains("validate_prd_goal.py")
                || s.starts_with("docs/")
        })
    }));
    assert!(json["residual_gaps"].as_array().unwrap().iter().any(|gap| {
        gap.as_str()
            .unwrap()
            .contains("shell and install artifacts missing")
    }));
    assert!(json["residual_gaps"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gap| gap.as_str().unwrap().contains("public claim")));
    assert!(output_json.exists());
}

#[test]
fn cli_completion_audit_reports_direct_evidence_integrity() {
    let dir = tempdir().unwrap();
    let results_dir = results_current_dir(dir.path());
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
    let json = run_tokenzero_json_in_with_env(
        &["completion-audit", "--json"],
        dir.path(),
        &[("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-current")],
    );
    let integrity = json["evidence_integrity_matrix"]
        .as_array()
        .expect("completion audit exposes evidence integrity matrix");
    let claim_row = find_integrity_row(
        integrity,
        "G-008",
        "results/current/tokenzero_claim_audit.json",
    );
    {
        assert_integrity_row_fields(claim_row, "artifact", serde_json::json!(true), "invalid", serde_json::json!(false));
        assert_eq!(claim_row["schema_version"], "tokenzero.source_currency.v1");
        assert_eq!(claim_row["expected_schema_version"], "tokenzero.claim_audit.v1");
        assert_eq!(claim_row["schema_matches"], false);
        assert_reason(claim_row, "schema_version mismatch");
    };
    assert_eq!(claim_row["requirement_status"], "passed_blocked");
    assert_eq!(
        claim_row["requirement_residual"],
        "public claim approval intentionally false until claim audit evidence gates pass"
    );
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
    let handoff_row = find_integrity_row(
        integrity,
        "G-010",
        "results/current/tokenzero_artifact_handoff.json",
    );
    {
        assert_integrity_row_fields(handoff_row, "artifact", serde_json::json!(true), "invalid", serde_json::json!(false));
        assert_eq!(handoff_row["schema_version"], "tokenzero.claim_audit.v1");
        assert_eq!(handoff_row["expected_schema_version"], "tokenzero.artifact_handoff.v1");
        assert_eq!(handoff_row["schema_matches"], false);
        assert_reason(handoff_row, "schema_version mismatch");
    };
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
    let json = parse_json_stdout(&assert_success(
        tokenzero_cmd()
            .args([
                "security-privacy-audit",
                "--output-json",
                output_json.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap(),
        "security privacy",
    ));
    assert_eq!(json["schema_version"], "tokenzero.security_privacy_audit.v1");
    for (k, v) in [
        ("ok", serde_json::json!(true)),
        ("raw_payloads_local_by_default", serde_json::json!(true)),
        ("pulse_records_raw_payload", serde_json::json!(false)),
        ("secret_masking_active", serde_json::json!(true)),
        ("allowed_root_controls_active", serde_json::json!(true)),
        ("unapproved_external_writes", serde_json::json!(false)),
        ("release_publication_allowed", serde_json::json!(false)),
    ] {
        assert_eq!(json[k], v, "{k}");
    }
    let rows = json["rows"].as_array().unwrap();
    for row_id in [
        "cli_visible_secret_masking",
        "exact_ref_local_recovery",
        "pulse_no_raw_payload",
        "mcp_allowed_root_enforced",
        "no_unapproved_external_writes",
    ] {
        assert!(
            rows.iter()
                .any(|row| row["id"] == row_id && row["pass"] == true),
            "{row_id}"
        );
    }
    assert!(output_json.exists());
}
