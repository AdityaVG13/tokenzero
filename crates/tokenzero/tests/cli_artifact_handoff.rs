mod common;
use common::*;

use serde_json::Value;
use tempfile::tempdir;

const HANDOFF_ARTIFACT_IDS: &[&str] = &[
    "completion_audit",
    "security_privacy_audit",
    "bench_competitors",
    "adapter_approval_audit",
    "adapter_approval_file",
    "source_currency",
    "os_reach",
    "os_release_artifact",
    "one_shot",
    "task_success",
    "exact_recovery",
    "exact_recovery_shell",
    "reach",
    "mcp_smoke",
    "shell_matrix",
    "competitive_reconciliation",
];

const STANDARD_HANDOFF_FILES: &[(&str, &str)] = &[
    ("tokenzero_completion_audit.json", "tokenzero.completion_audit.v1"),
    ("tokenzero_security_privacy_audit.json", "tokenzero.security_privacy_audit.v1"),
    ("tokenzero_bench_competitors_shell_heavy.json", "tokenzero.bench.v1"),
    ("tokenzero_adapter_approval_audit.json", "tokenzero.adapter_approval_audit.v1"),
    ("tokenzero_adapter_approval_file.json", "tokenzero.adapter_approval_file.v1"),
    ("tokenzero_source_currency.json", "tokenzero.source_currency.v1"),
    ("tokenzero_claim_audit.json", "tokenzero.claim_audit.v1"),
    ("tokenzero_os_reach_audit.json", "tokenzero.os_reach_audit.v1"),
    ("tokenzero_os_release_artifact.json", "tokenzero.os_release_artifact.v1"),
    ("tokenzero_one_shot_eval.json", "tokenzero.one_shot_eval.v1"),
    ("tokenzero_exact_recovery_audit.json", "tokenzero.exact_recovery_audit.v1"),
    ("tokenzero_exact_recovery_shell.json", "tokenzero.exact_recovery_shell.v1"),
    ("tokenzero_false_success_shell.json", "tokenzero.false_success_shell.v1"),
    ("tokenzero_reach.json", "tokenzero.reach.v1"),
    ("rust_mcp_smoke.json", "tokenzero.rust_mcp_churn.v1"),
    ("tokenzero_shell_matrix.json", "tokenzero.shell_matrix.v1"),
];

fn run_artifact_handoff_json(root: &std::path::Path, release_candidate_id: &str) -> Value {
    run_tokenzero_json_in_with_env(
        &["artifact-handoff", "--json"],
        root,
        &[("TOKENZERO_RELEASE_CANDIDATE_ID", release_candidate_id)],
    )
}

fn handoff_artifact_row<'a>(json: &'a Value, artifact_id: &str) -> &'a Value {
    find_row_by(
        json["artifact_integrity_matrix"].as_array().unwrap(),
        "id",
        artifact_id,
    )
}

fn handoff_verification_evidence_row<'a>(
    json: &'a Value,
    verification_id: &str,
    artifact_id: &str,
) -> &'a Value {
    json["verification_evidence_integrity_matrix"]
        .as_array()
        .expect("verification evidence integrity matrix")
        .iter()
        .find(|row| row["verification_id"] == verification_id && row["artifact_id"] == artifact_id)
        .unwrap_or_else(|| panic!("{verification_id} {artifact_id} evidence link"))
}

fn write_standard_handoff_fixtures(results_dir: &std::path::Path, release_candidate_id: &str) {
    for (file_name, schema_version) in STANDARD_HANDOFF_FILES {
        write_json_fixture(
            &results_dir.join(file_name),
            &serde_json::json!({
                "schema_version": schema_version,
                "release_candidate_id": release_candidate_id,
                "completion_achieved": false,
                "public_claims_approved": false,
                "release_publication_allowed": false,
                "all_requirement_rows_passed": false,
                "blocked_requirement_ids": ["G-005", "FR-010"],
                "requirement_status_counts": {"passed": 2, "blocked_public": 2},
                "residual_gate_matrix": []
            }),
        );
    }
}

fn write_handoff_docs_fixtures(root: &std::path::Path) {
    let docs_dir = root.join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();
    std::fs::write(
        docs_dir.join("advanced-adr-execution-record.md"),
        "## ADR-000 Fixture\nFailure-first evidence:\nResidual gates:\nvalidate_prd_goal.py\ncargo test --workspace\n",
    )
    .unwrap();
    std::fs::write(
        root.join("results")
            .join("current")
            .join("tokenzero_competitive_superiority_reconciliation.md"),
        "## Snapshot\nno gated action was performed\n",
    )
    .unwrap();
}

fn run_handoff_json(root: &std::path::Path, release_candidate_id: &str) -> Value {
    let output_json = root.join("handoff.json");
    let json = run_tokenzero_json_in_with_env(
        &[
            "artifact-handoff",
            "--output-json",
            output_json.to_str().unwrap(),
            "--json",
        ],
        root,
        &[("TOKENZERO_RELEASE_CANDIDATE_ID", release_candidate_id)],
    );
    assert!(output_json.exists());
    json
}

fn assert_artifact_present_with_path_prefix(json: &Value, artifact_id: &str, path_prefix: &str) {
    let path = find_artifact(json, artifact_id)["path"].as_str().unwrap();
    assert!(
        path.starts_with(path_prefix),
        "{artifact_id} path {path:?} should start with {path_prefix:?}"
    );
}

fn assert_artifact_purpose_contains(json: &Value, artifact_id: &str, substring: &str) {
    let purpose = find_artifact(json, artifact_id)["purpose"].as_str().unwrap();
    assert!(
        purpose.contains(substring),
        "{artifact_id} purpose should contain {substring:?}, got {purpose:?}"
    );
}

fn assert_purpose_excludes(json: &Value, artifact_id: &str, excluded: &str, msg: &str) {
    let purpose = find_artifact(json, artifact_id)["purpose"].as_str().unwrap();
    assert!(!purpose.contains(excluded), "{msg}");
}

fn find_residual<'a>(residuals: &'a [Value], gate_id: &str) -> &'a Value {
    find_row_by(residuals, "gate_id", gate_id)
}

fn find_vp_row<'a>(vp_rows: &'a [Value], vp_id: &str) -> &'a Value {
    find_row_by(vp_rows, "id", vp_id)
}

fn write_completion_audit_with_residual_gates(
    results_dir: &std::path::Path,
    release_candidate_id: &str,
) {
    write_json_fixture(
        &results_dir.join("tokenzero_completion_audit.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.completion_audit.v1",
            "release_candidate_id": release_candidate_id,
            "completion_achieved": false,
            "public_claims_approved": false,
            "release_publication_allowed": false,
            "all_direct_file_evidence_present": true,
            "all_direct_artifact_evidence_valid": false,
            "all_requirement_rows_passed": false,
            "blocked_requirement_ids": ["G-008", "FR-010"],
            "requirement_status_counts": {
                "passed": 2,
                "passed_blocked": 1,
                "blocked_public": 1
            },
            "all_residual_gates_resolved": false,
            "blocked_residual_gate_ids": ["adapter_approval", "release_approval"],
            "residual_gate_status_counts": {"blocked": 2},
            "claim_gate_snapshot": {
                "present": true,
                "release_candidate_id": release_candidate_id,
                "public_claims_approved": false,
                "gate_passes": {
                    "adapter_approval": false,
                    "release_candidate": true
                },
                "gate_reasons": {
                    "adapter_approval": ["adapter approval artifact has missing reviewed commands"],
                    "release_candidate": []
                },
                "release_candidate_ids": [release_candidate_id],
                "release_candidate_artifacts": [{
                    "artifact_id": "adapter_approval_artifact",
                    "artifact_path": "results/current/tokenzero_adapter_approval_audit.json",
                    "release_candidate_id": release_candidate_id,
                    "schema_version": "tokenzero.adapter_approval_audit.v1"
                }]
            },
            "evidence_integrity_matrix": [
                {
                    "section": "must_fr",
                    "requirement_id": "FR-006",
                    "evidence": "cargo test --workspace",
                    "evidence_kind": "command",
                    "present": null,
                    "status": "command_evidence"
                },
                {
                    "section": "g_goals",
                    "requirement_id": "G-008",
                    "evidence": "results/current/tokenzero_claim_audit.json",
                    "evidence_kind": "artifact",
                    "present": true,
                    "status": "invalid",
                    "schema_version": "tokenzero.source_currency.v1",
                    "expected_schema_version": "tokenzero.claim_audit.v1",
                    "schema_matches": false,
                    "artifact_valid": false,
                    "reasons": ["schema_version mismatch"]
                }
            ],
            "residual_gate_matrix": [
                {
                    "gate_id": "adapter_approval",
                    "status": "blocked",
                    "blocked_reasons": ["adapter approval artifact has missing reviewed commands"],
                    "next_action_id": "runnable_adapter_approval",
                    "owner": "bench/release",
                    "stop_before": ["competitor execution", "public benchmark claim"]
                },
                {
                    "gate_id": "release_approval",
                    "status": "blocked",
                    "blocked_reasons": ["release approval not granted"],
                    "next_action_id": "final_false_closure_audit",
                    "owner": "implementer",
                    "stop_before": ["release", "publication", "global install apply"]
                }
            ]
        }),
    );
}

fn write_claim_audit_with_evidence_gates(
    results_dir: &std::path::Path,
    release_candidate_id: &str,
) {
    write_json_fixture(
        &results_dir.join("tokenzero_claim_audit.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.claim_audit.v1",
            "release_candidate_id": release_candidate_id,
            "public_claims_approved": false,
            "blocked_reasons": [],
            "evidence_gates": [
                {"id": "source_currency", "pass": true, "reasons": []},
                {
                    "id": "benchmark_artifact",
                    "pass": false,
                    "reasons": [
                        "benchmark artifact not approved for publication",
                        "benchmark competitor rows must be runnable for public claims"
                    ]
                },
                {
                    "id": "adapter_approval",
                    "pass": false,
                    "reasons": [
                        "adapter approval artifact does not allow execution",
                        "adapter approval artifact not approved for public claims"
                    ]
                },
                {
                    "id": "os_artifact",
                    "pass": false,
                    "reasons": ["OS artifact set not approved for public claim"]
                },
                {"id": "release_candidate", "pass": true, "reasons": []},
                {
                    "id": "release_approval",
                    "pass": false,
                    "reasons": ["release approval not granted"]
                }
            ]
        }),
    );
}

#[test]
fn cli_artifact_handoff_packet_lists_next_actions_and_stop_gates() {
    let dir = tempdir().unwrap();
    let results_dir = results_current_dir(dir.path());
    write_standard_handoff_fixtures(&results_dir, "rc-fixture");
    write_handoff_docs_fixtures(dir.path());
    let json = run_handoff_json(dir.path(), "rc-fixture");

    assert_eq!(json["schema_version"], "tokenzero.artifact_handoff.v1");
    assert_eq!(json["ok"], true);
    assert_eq!(json["completion_achieved"], false);
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    assert_eq!(
        json["global_tokenzero_release_verification_trusted"],
        json["installed_wrapper_audit"]["resolved_is_current_exe"]
    );
    assert_eq!(
        json["approved_install_required_for_global_update"],
        !json["global_tokenzero_release_verification_trusted"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(json["installed_wrapper_audit"]["global_writes"], false);
    assert_eq!(json["installed_wrapper_audit"]["daemon_required"], false);
    let binary = json["release_verification_binary"].as_str().unwrap();
    assert!(
        binary.ends_with("tokenzero.exe") || binary.ends_with("tokenzero"),
        "unexpected binary path: {binary}"
    );

    for artifact_id in HANDOFF_ARTIFACT_IDS {
        assert_artifact_present_with_path_prefix(&json, artifact_id, "results/current/");
    }

    for (id, path, purpose_sub) in [
        ("mcp_smoke", "results/current/rust_mcp_smoke.json", "VP-003"),
        (
            "shell_matrix",
            "results/current/tokenzero_shell_matrix.json",
            "VP-004",
        ),
        (
            "adapter_approval_file",
            "results/current/tokenzero_adapter_approval_file.json",
            "reviewed command-shape",
        ),
        ("reach", "results/current/tokenzero_reach.json", "host reach"),
        (
            "competitive_reconciliation",
            "results/current/tokenzero_competitive_superiority_reconciliation.md",
            "residual gate reconciliation",
        ),
        (
            "task_success",
            "results/current/tokenzero_one_shot_eval.json",
            "task-success proof",
        ),
    ] {
        assert_eq!(find_artifact(&json, id)["path"], path);
        assert_artifact_purpose_contains(&json, id, purpose_sub);
    }

    let next_actions = json["next_actions"].as_array().unwrap();
    assert!(next_actions.iter().any(|row| {
        row["id"] == "final_false_closure_audit"
            && row["validation"]
                .as_str()
                .unwrap()
                .contains("completion-audit")
    }));
    assert!(!next_actions.iter().any(|row| row["id"] == "source_currency_refresh"));

    let stop_before = json["stop_before"].as_array().unwrap();
    assert!(!stop_before.is_empty());
    assert!(stop_before.iter().any(|gate| gate == "publication"));

    let vp_rows = json["verification_plan_matrix"].as_array().unwrap();
    let vp3 = find_vp_row(vp_rows, "VP-003");
    assert_eq!(vp3["evidence_artifact_ids"], serde_json::json!(["mcp_smoke"]));
    assert_eq!(vp3["status"], "passed_local");
    let vp4 = find_vp_row(vp_rows, "VP-004");
    assert_eq!(
        vp4["evidence_artifact_ids"],
        serde_json::json!(["shell_matrix", "os_release_artifact", "os_reach"])
    );
    assert_eq!(vp4["status"], "blocked_public");
    assert_reason_contains(
        vp4["blocked_reasons"].as_array().unwrap(),
        "Windows, Linux, and macOS shell matrix artifacts not run on this host",
    );
    let vp5 = find_vp_row(vp_rows, "VP-005");
    assert_eq!(
        vp5["evidence_artifact_ids"],
        serde_json::json!(["bench_competitors", "adapter_approval_audit"])
    );
    let vp5_bench = handoff_verification_evidence_row(&json, "VP-005", "bench_competitors");
    assert_eq!(
        vp5_bench["artifact_path"],
        "results/current/tokenzero_bench_competitors_shell_heavy.json"
    );
    assert_eq!(vp5_bench["present"], true);
    assert_eq!(vp5_bench["valid"], true);
    assert_eq!(vp5_bench["status"], "linked_valid");
    let vp_integrity = json["verification_evidence_integrity_matrix"]
        .as_array()
        .expect("verification evidence integrity matrix");
    assert!(vp_integrity.iter().any(|row| {
        row["verification_id"] == "VP-005"
            && row["artifact_id"] == "adapter_approval_audit"
            && row["status"] == "linked_valid"
    }));
    for (k,v) in [
        ("all_verification_evidence_artifacts_present", serde_json::json!(true)),
        ("all_verification_evidence_artifacts_valid", serde_json::json!(true)),
        ("all_verification_plan_rows_passed", serde_json::json!(false)),
        ("all_requirement_rows_passed", serde_json::json!(false)),
    ] { assert_eq!(json[k], v, "{k}"); }
    assert_eq!(json["blocked_verification_plan_ids"], serde_json::json!(["VP-004", "VP-008"]));
    assert_eq!(json["blocked_requirement_ids"], serde_json::json!(["G-005", "FR-010"]));
    for (map,k,n) in [
        ("verification_plan_status_counts","passed_local",4),
        ("verification_plan_status_counts","passed_private",2),
        ("verification_plan_status_counts","blocked_public",2),
        ("requirement_status_counts","passed",2),
        ("requirement_status_counts","blocked_public",2),
    ] { assert_eq!(json[map][k], n, "{map}.{k}"); }
    let vp6 = find_vp_row(vp_rows, "VP-006");
    assert_eq!(
        vp6["evidence_artifact_ids"],
        serde_json::json!(["exact_recovery"])
    );
    let vp8 = find_vp_row(vp_rows, "VP-008");
    assert_eq!(vp8["evidence_artifact_ids"], serde_json::json!(["claim_audit"]));
    assert_eq!(vp8["status"], "blocked_public");
    assert_reason_contains(
        vp8["blocked_reasons"].as_array().unwrap(),
        "public claims intentionally blocked until release gates pass",
    );
    assert!(json["anti_drift_reminders"].as_array().unwrap().iter().any(|row| {
        row["risk"] == "Repeated agent drift"
            && row["validation"]
                .as_str()
                .unwrap()
                .contains("completion-audit")
    }));
}

#[test]
fn cli_artifact_handoff_carries_completion_residual_gate_matrix() {
    let dir = tempdir().unwrap();
    let results_dir = results_current_dir(dir.path());
    write_completion_audit_with_residual_gates(&results_dir, "rc-fixture");
    let json = run_artifact_handoff_json(dir.path(), "rc-fixture");
    let snapshot = &json["completion_audit_snapshot"];
    assert_eq!(
        snapshot["artifact_path"],
        "results/current/tokenzero_completion_audit.json"
    );
    for (k, v) in [
        ("present", serde_json::json!(true)),
        ("release_candidate_id", serde_json::json!("rc-fixture")),
        ("all_direct_file_evidence_present", serde_json::json!(true)),
        ("all_direct_artifact_evidence_valid", serde_json::json!(false)),
        ("all_requirement_rows_passed", serde_json::json!(false)),
        ("all_residual_gates_resolved", serde_json::json!(false)),
    ] {
        assert_eq!(snapshot[k], v, "{k}");
    }
    assert_eq!(snapshot["blocked_requirement_ids"], serde_json::json!(["G-008", "FR-010"]));
    assert_eq!(snapshot["blocked_residual_gate_ids"], serde_json::json!(["adapter_approval", "release_approval"]));
    for (k, n) in [("passed", 2), ("passed_blocked", 1), ("blocked_public", 1)] {
        assert_eq!(snapshot["requirement_status_counts"][k], n, "{k}");
    }
    assert_eq!(snapshot["residual_gate_status_counts"]["blocked"], 2);
    let evidence_integrity = snapshot["evidence_integrity_matrix"]
        .as_array()
        .expect("handoff carries completion evidence integrity matrix");
    assert!(evidence_integrity.iter().any(|row| {
        row["requirement_id"] == "FR-006"
            && row["evidence"] == "cargo test --workspace"
            && row["status"] == "command_evidence"
    }));
    assert!(evidence_integrity.iter().any(|row| {
        row["requirement_id"] == "G-008"
            && row["evidence"] == "results/current/tokenzero_claim_audit.json"
            && row["status"] == "invalid"
            && row["expected_schema_version"] == "tokenzero.claim_audit.v1"
            && row["artifact_valid"] == false
    }));
    let claim_snapshot = &snapshot["claim_gate_snapshot"];
    assert_eq!(claim_snapshot["present"], true);
    assert_eq!(claim_snapshot["gate_passes"]["adapter_approval"], false);
    assert_eq!(
        claim_snapshot["gate_reasons"]["adapter_approval"][0],
        "adapter approval artifact has missing reviewed commands"
    );
    let rc_artifacts = claim_snapshot["release_candidate_artifacts"]
        .as_array()
        .expect("handoff carries claim release-candidate artifacts");
    assert_eq!(rc_artifacts.len(), 1);
    assert_eq!(
        rc_artifacts[0]["artifact_path"],
        "results/current/tokenzero_adapter_approval_audit.json"
    );
    let residuals = json["residual_gate_matrix"].as_array().unwrap();
    assert_eq!(json["all_residual_gates_resolved"], false);
    assert_eq!(
        json["blocked_residual_gate_ids"],
        serde_json::json!(["adapter_approval", "release_approval"])
    );
    assert_eq!(json["residual_gate_status_counts"]["blocked"], 2);
    let adapter = find_residual(residuals, "adapter_approval");
    assert_eq!(adapter["next_action_id"], "runnable_adapter_approval");
    assert_eq!(adapter["next_action"]["id"], "runnable_adapter_approval");
    assert!(adapter["next_action"]["validation"]
        .as_str()
        .unwrap()
        .contains("adapter-approval-audit"));
    assert!(adapter["next_action"]["stop_condition"]
        .as_str()
        .unwrap()
        .contains("no blind install"));
    assert_reason_contains(
        adapter["blocked_reasons"].as_array().unwrap(),
        "adapter approval artifact has missing reviewed commands",
    );
    let release = find_residual(residuals, "release_approval");
    assert_eq!(release["next_action"]["id"], "final_false_closure_audit");
    assert!(release["next_action"]["validation"]
        .as_str()
        .unwrap()
        .contains("completion-audit"));
    assert!(release["stop_before"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| gate == "publication"));
}

fn write_os_reach_partial_fixture(results_dir: &std::path::Path, release_candidate_id: &str) {
    write_json_fixture(
        &results_dir.join("tokenzero_os_reach_audit.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.os_reach_audit.v1",
            "release_candidate_id": release_candidate_id,
            "all_release_oses_run": false,
            "public_os_claim_approved": false,
            "blocked_reasons": ["macos not run with shell and install artifacts"],
            "os_rows": [
                {"os": "windows", "claim_ready": true},
                {"os": "linux", "claim_ready": true},
                {"os": "macos", "claim_ready": false}
            ]
        }),
    );
}

#[test]
fn cli_artifact_handoff_uses_current_residual_actions_after_source_and_linux_evidence() {
    let dir = tempdir().unwrap();
    let results_dir = results_current_dir(dir.path());
    write_claim_audit_with_evidence_gates(&results_dir, "rc-current");
    write_os_reach_partial_fixture(&results_dir, "rc-current");
    let completion = run_tokenzero_json_in(&["completion-audit", "--json"], dir.path());
    let handoff = run_tokenzero_json_in(&["artifact-handoff", "--json"], dir.path());
    let actions = handoff["next_actions"].as_array().unwrap();
    assert!(
        !actions
            .iter()
            .any(|action| action["id"] == "source_currency_refresh"),
        "source refresh should not remain queued after source gate passes"
    );
    let os_action = actions
        .iter()
        .find(|action| action["id"] == "os_matrix_expansion")
        .expect("macOS OS action");
    let action_text = os_action["action"].as_str().unwrap();
    assert!(action_text.contains("macOS"));
    assert!(!action_text.contains("Linux and macOS"));
    let validation_text = os_action["validation"].as_str().unwrap();
    assert!(validation_text.contains("<macos.json>"));
    assert!(!validation_text.contains("<linux.json> <macos.json>"));
    assert_artifact_purpose_contains(&handoff, "os_reach", "macOS");
    assert_purpose_excludes(
        &handoff,
        "os_reach",
        "Linux/macOS",
        "OS reach artifact purpose should not reintroduce missing Linux after Linux evidence exists",
    );
    assert_artifact_purpose_contains(&handoff, "os_release_artifact", "macOS");
    assert_purpose_excludes(
        &handoff,
        "os_release_artifact",
        "Linux/macOS",
        "OS release artifact purpose should not reintroduce missing Linux after Linux evidence exists",
    );
    let vp4 = find_vp_row(
        handoff["verification_plan_matrix"].as_array().unwrap(),
        "VP-004",
    );
    let vp4_reasons = vp4["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reason| reason.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(vp4_reasons.contains("macOS"));
    assert!(
        !vp4_reasons.contains("Linux/macOS"),
        "VP-004 should report only the OS rows still missing from current evidence"
    );
    let residuals = handoff["residual_gate_matrix"].as_array().unwrap();
    let benchmark = find_residual(residuals, "benchmark_artifact");
    assert_eq!(
        benchmark["next_action_id"],
        serde_json::json!("runnable_adapter_approval")
    );
    assert!(benchmark["next_action"]["validation"]
        .as_str()
        .unwrap()
        .contains("adapter-approval-audit"));
    let fr007 = find_row_by(completion["must_fr"].as_array().unwrap(), "id", "FR-007");
    let fr007_residual = fr007["residual"].as_str().unwrap();
    assert!(fr007_residual.contains("macOS"));
    assert!(!fr007_residual.contains("Linux/macOS"));
}

#[test]
fn cli_artifact_handoff_rejects_mismatched_release_candidate_artifacts() {
    let dir = tempdir().unwrap();
    let results_dir = results_current_dir(dir.path());
    write_minimal_handoff_completion_audit(&results_dir, "rc-current");
    write_json_fixture(
        &results_dir.join("tokenzero_source_currency.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.source_currency.v1",
            "release_candidate_id": "rc-other"
        }),
    );
    let json = run_artifact_handoff_json(dir.path(), "rc-current");
    assert_integrity_row(
        handoff_artifact_row(&json, "source_currency"),
        true, true,
        serde_json::json!("tokenzero.source_currency.v1"),
        serde_json::Value::Null,
        None,
        serde_json::json!("rc-current"),
        serde_json::json!("rc-other"),
        false, false,
        "release_candidate_id mismatch",
    );
}

#[test]
fn cli_artifact_handoff_rejects_swapped_and_malformed_schema_bound_artifacts() {
    let dir = tempdir().unwrap();
    let results_dir = results_current_dir(dir.path());
    write_minimal_handoff_completion_audit(&results_dir, "rc-current");
    write_json_fixture(
        &results_dir.join("tokenzero_claim_audit.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.source_currency.v1",
            "release_candidate_id": "rc-current"
        }),
    );
    std::fs::write(
        results_dir.join("tokenzero_bench_competitors_shell_heavy.json"),
        "not json",
    )
    .unwrap();
    let json = run_artifact_handoff_json(dir.path(), "rc-current");
    assert_integrity_row(
        handoff_artifact_row(&json, "claim_audit"),
        true, true,
        serde_json::json!("tokenzero.source_currency.v1"),
        serde_json::json!("tokenzero.claim_audit.v1"),
        Some(false),
        serde_json::json!("rc-current"),
        serde_json::json!("rc-current"),
        true, false,
        "schema_version mismatch",
    );
    assert_integrity_row(
        handoff_artifact_row(&json, "bench_competitors"),
        true, true,
        serde_json::Value::Null,
        serde_json::json!("tokenzero.bench.v1"),
        Some(false),
        serde_json::json!("rc-current"),
        serde_json::Value::Null,
        false, false,
        "artifact JSON unreadable",
    );
    for (vid, aid, reason) in [
        ("VP-008", "claim_audit", "schema_version mismatch"),
        ("VP-005", "bench_competitors", "artifact JSON unreadable"),
    ] {
        let row = handoff_verification_evidence_row(&json, vid, aid);
        assert_eq!(row["status"], "invalid");
        assert_eq!(row["valid"], false);
        assert_reason(row, reason);
    }
    assert_eq!(json["all_verification_evidence_artifacts_present"], false);
    assert_eq!(json["all_verification_evidence_artifacts_valid"], false);
}

#[test]
fn cli_artifact_handoff_rejects_missing_schema_and_release_candidate_fields() {
    let dir = tempdir().unwrap();
    let results_dir = results_current_dir(dir.path());
    write_minimal_handoff_completion_audit(&results_dir, "rc-current");
    write_json_fixture(
        &results_dir.join("tokenzero_claim_audit.json"),
        &serde_json::json!({ "release_candidate_id": "rc-current" }),
    );
    write_json_fixture(
        &results_dir.join("tokenzero_source_currency.json"),
        &serde_json::json!({ "schema_version": "tokenzero.source_currency.v1" }),
    );
    let json = run_artifact_handoff_json(dir.path(), "rc-current");
    assert_integrity_row(
        handoff_artifact_row(&json, "claim_audit"),
        true, true,
        serde_json::Value::Null,
        serde_json::json!("tokenzero.claim_audit.v1"),
        Some(false),
        serde_json::json!("rc-current"),
        serde_json::json!("rc-current"),
        true, false,
        "schema_version missing",
    );
    assert_integrity_row(
        handoff_artifact_row(&json, "source_currency"),
        true, true,
        serde_json::json!("tokenzero.source_currency.v1"),
        serde_json::json!("tokenzero.source_currency.v1"),
        Some(true),
        serde_json::json!("rc-current"),
        serde_json::Value::Null,
        false, false,
        "release_candidate_id missing",
    );
    let vp8_claim = handoff_verification_evidence_row(&json, "VP-008", "claim_audit");
    assert_eq!(vp8_claim["status"], "invalid");
    assert_eq!(vp8_claim["valid"], false);
    assert_reason(vp8_claim, "schema_version missing");
    assert_eq!(json["all_required_artifacts_valid"], false);
    assert_eq!(json["all_verification_evidence_artifacts_valid"], false);
}

#[test]
fn cli_artifact_handoff_reports_required_artifact_integrity() {
    let dir = tempdir().unwrap();
    let results_dir = results_current_dir(dir.path());
    write_json_fixture(
        &results_dir.join("tokenzero_completion_audit.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.completion_audit.v1",
            "completion_achieved": false,
            "public_claims_approved": false,
            "release_publication_allowed": false,
            "residual_gate_matrix": []
        }),
    );
    write_json_fixture(
        &results_dir.join("tokenzero_bench_competitors_shell_heavy.json"),
        &serde_json::json!({ "schema_version": "tokenzero.claim_audit.v1" }),
    );
    let docs_dir = dir.path().join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();
    std::fs::write(
        docs_dir.join("advanced-adr-execution-record.md"),
        "## ADR-053 Placeholder\nFailure-first evidence:\nResidual gates:\n",
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_competitive_superiority_reconciliation.md"),
        "## Snapshot\n",
    )
    .unwrap();
    let json = run_tokenzero_json_in(&["artifact-handoff", "--json"], dir.path());
    assert_eq!(json["all_required_artifacts_present"], false);
    let integrity = json["artifact_integrity_matrix"].as_array().unwrap();
    let completion = find_row_by(integrity, "id", "completion_audit");
    assert_eq!(completion["present"], true);
    assert_eq!(completion["readable"], true);
    assert_eq!(completion["schema_version"], "tokenzero.completion_audit.v1");
    let bench = find_row_by(integrity, "id", "bench_competitors");
    assert_eq!(bench["present"], true);
    assert_eq!(bench["readable"], true);
    assert_eq!(bench["schema_version"], "tokenzero.claim_audit.v1");
    assert_eq!(bench["expected_schema_version"], "tokenzero.bench.v1");
    assert_eq!(bench["schema_matches"], false);
    assert_eq!(bench["valid"], false);
    assert_reason(bench, "schema_version mismatch");
    let task_success = find_row_by(integrity, "id", "task_success");
    assert_eq!(task_success["present"], false);
    assert_eq!(task_success["readable"], false);
    assert_reason(task_success, "artifact missing");
    for (id, markers, reason) in [
        (
            "advanced_adr",
            serde_json::json!([
                "## ADR-",
                "Failure-first evidence:",
                "Residual gates:",
                "validate_prd_goal.py",
                "cargo test --workspace"
            ]),
            "content marker missing: validate_prd_goal.py",
        ),
        (
            "competitive_reconciliation",
            serde_json::json!(["Snapshot", "no gated action was performed"]),
            "content marker missing: no gated action was performed",
        ),
    ] {
        let row = find_row_by(integrity, "id", id);
        assert_eq!(row["present"], true);
        assert_eq!(row["readable"], true);
        assert_eq!(row["expected_content_markers"], markers);
        assert_eq!(row["content_markers_present"], false);
        assert_eq!(row["valid"], false);
        assert_reason(row, reason);
    }
}
