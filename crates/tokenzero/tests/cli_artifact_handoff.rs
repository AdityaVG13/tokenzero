mod common;
use common::*;

use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn cli_artifact_handoff_packet_lists_next_actions_and_stop_gates() {
    let dir = tempdir().unwrap();
    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    for (file_name, schema_version) in [
        (
            "tokenzero_completion_audit.json",
            "tokenzero.completion_audit.v1",
        ),
        (
            "tokenzero_security_privacy_audit.json",
            "tokenzero.security_privacy_audit.v1",
        ),
        (
            "tokenzero_bench_competitors_shell_heavy.json",
            "tokenzero.bench.v1",
        ),
        (
            "tokenzero_adapter_approval_audit.json",
            "tokenzero.adapter_approval_audit.v1",
        ),
        (
            "tokenzero_adapter_approval_file.json",
            "tokenzero.adapter_approval_file.v1",
        ),
        (
            "tokenzero_source_currency.json",
            "tokenzero.source_currency.v1",
        ),
        ("tokenzero_claim_audit.json", "tokenzero.claim_audit.v1"),
        (
            "tokenzero_os_reach_audit.json",
            "tokenzero.os_reach_audit.v1",
        ),
        (
            "tokenzero_os_release_artifact.json",
            "tokenzero.os_release_artifact.v1",
        ),
        ("tokenzero_one_shot_eval.json", "tokenzero.one_shot_eval.v1"),
        (
            "tokenzero_exact_recovery_audit.json",
            "tokenzero.exact_recovery_audit.v1",
        ),
        (
            "tokenzero_exact_recovery_shell.json",
            "tokenzero.exact_recovery_shell.v1",
        ),
        (
            "tokenzero_false_success_shell.json",
            "tokenzero.false_success_shell.v1",
        ),
        ("tokenzero_reach.json", "tokenzero.reach.v1"),
        ("rust_mcp_smoke.json", "tokenzero.rust_mcp_churn.v1"),
        ("tokenzero_shell_matrix.json", "tokenzero.shell_matrix.v1"),
    ] {
        std::fs::write(
            results_dir.join(file_name),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": schema_version,
                "release_candidate_id": "rc-fixture",
                "completion_achieved": false,
                "public_claims_approved": false,
                "release_publication_allowed": false,
                "all_requirement_rows_passed": false,
                "blocked_requirement_ids": ["G-005", "FR-010"],
                "requirement_status_counts": {
                    "passed": 2,
                    "blocked_public": 2
                },
                "residual_gate_matrix": []
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let docs_dir = dir.path().join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();
    std::fs::write(
        docs_dir.join("advanced-adr-execution-record.md"),
        "## ADR-000 Fixture\nFailure-first evidence:\nResidual gates:\nvalidate_prd_goal.py\ncargo test --workspace\n",
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_competitive_superiority_reconciliation.md"),
        "## Snapshot\nno gated action was performed\n",
    )
    .unwrap();
    let output_json = dir.path().join("handoff.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-fixture")
        .args([
            "artifact-handoff",
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
    assert!(
        json["release_verification_binary"]
            .as_str()
            .unwrap()
            .ends_with("tokenzero.exe")
            || json["release_verification_binary"]
                .as_str()
                .unwrap()
                .ends_with("tokenzero")
    );

    for artifact_id in [
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
    ] {
        assert!(
            json["artifacts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == artifact_id
                    && row["path"]
                        .as_str()
                        .unwrap()
                        .starts_with("results/current/")),
            "{artifact_id}"
        );
    }
    let mcp_smoke = json["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "mcp_smoke")
        .expect("MCP smoke artifact");
    assert_eq!(mcp_smoke["path"], "results/current/rust_mcp_smoke.json");
    assert!(mcp_smoke["purpose"].as_str().unwrap().contains("VP-003"));
    let shell_matrix = json["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "shell_matrix")
        .expect("shell matrix artifact");
    assert_eq!(
        shell_matrix["path"],
        "results/current/tokenzero_shell_matrix.json"
    );
    assert!(shell_matrix["purpose"].as_str().unwrap().contains("VP-004"));
    let adapter_approval_file = json["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "adapter_approval_file")
        .expect("adapter approval file artifact");
    assert_eq!(
        adapter_approval_file["path"],
        "results/current/tokenzero_adapter_approval_file.json"
    );
    assert!(
        adapter_approval_file["purpose"]
            .as_str()
            .unwrap()
            .contains("reviewed command-shape")
    );
    let reach = json["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "reach")
        .expect("reach artifact");
    assert_eq!(reach["path"], "results/current/tokenzero_reach.json");
    assert!(reach["purpose"].as_str().unwrap().contains("host reach"));
    let reconciliation = json["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "competitive_reconciliation")
        .expect("competitive reconciliation artifact");
    assert_eq!(
        reconciliation["path"],
        "results/current/tokenzero_competitive_superiority_reconciliation.md"
    );
    assert!(
        reconciliation["purpose"]
            .as_str()
            .unwrap()
            .contains("residual gate reconciliation")
    );
    let task_success = json["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "task_success")
        .expect("task-success artifact");
    assert_eq!(
        task_success["path"],
        "results/current/tokenzero_one_shot_eval.json"
    );
    assert!(
        task_success["purpose"]
            .as_str()
            .unwrap()
            .contains("task-success proof")
    );

    let next_actions = json["next_actions"].as_array().unwrap();
    assert!(next_actions.iter().any(|row| {
        row["id"] == "final_false_closure_audit"
            && row["validation"]
                .as_str()
                .unwrap()
                .contains("completion-audit")
    }));
    assert!(
        !next_actions
            .iter()
            .any(|row| row["id"] == "source_currency_refresh")
    );
    let stop_before = json["stop_before"].as_array().unwrap();
    assert!(
        !stop_before.is_empty(),
        "stop_before should contain at least one gate"
    );
    let vp_rows = json["verification_plan_matrix"].as_array().unwrap();
    let vp3 = vp_rows
        .iter()
        .find(|row| row["id"] == "VP-003")
        .expect("VP-003 row");
    assert_eq!(
        vp3["evidence_artifact_ids"],
        serde_json::json!(["mcp_smoke"])
    );
    assert_eq!(vp3["status"], "passed_local");
    let vp4 = vp_rows
        .iter()
        .find(|row| row["id"] == "VP-004")
        .expect("VP-004 row");
    assert_eq!(
        vp4["evidence_artifact_ids"],
        serde_json::json!(["shell_matrix", "os_release_artifact", "os_reach"])
    );
    assert_eq!(vp4["status"], "blocked_public");
    assert!(
        vp4["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                == "Windows, Linux, and macOS shell matrix artifacts not run on this host")
    );
    let vp5 = vp_rows
        .iter()
        .find(|row| row["id"] == "VP-005")
        .expect("VP-005 row");
    assert_eq!(
        vp5["evidence_artifact_ids"],
        serde_json::json!(["bench_competitors", "adapter_approval_audit"])
    );
    let vp_integrity = json["verification_evidence_integrity_matrix"]
        .as_array()
        .expect("verification evidence integrity matrix");
    let vp5_bench = vp_integrity
        .iter()
        .find(|row| row["verification_id"] == "VP-005" && row["artifact_id"] == "bench_competitors")
        .expect("VP-005 benchmark evidence link");
    assert_eq!(
        vp5_bench["artifact_path"],
        "results/current/tokenzero_bench_competitors_shell_heavy.json"
    );
    assert_eq!(vp5_bench["present"], true);
    assert_eq!(vp5_bench["valid"], true);
    assert_eq!(vp5_bench["status"], "linked_valid");
    assert!(
        vp_integrity.iter().any(|row| {
            row["verification_id"] == "VP-005"
                && row["artifact_id"] == "adapter_approval_audit"
                && row["status"] == "linked_valid"
        }),
        "VP-005 adapter approval link"
    );
    assert_eq!(json["all_verification_evidence_artifacts_present"], true);
    assert_eq!(json["all_verification_evidence_artifacts_valid"], true);
    assert_eq!(json["all_verification_plan_rows_passed"], false);
    assert_eq!(
        json["blocked_verification_plan_ids"],
        serde_json::json!(["VP-004", "VP-008"])
    );
    assert_eq!(json["verification_plan_status_counts"]["passed_local"], 4);
    assert_eq!(json["verification_plan_status_counts"]["passed_private"], 2);
    assert_eq!(json["verification_plan_status_counts"]["blocked_public"], 2);
    assert_eq!(json["all_requirement_rows_passed"], false);
    assert_eq!(
        json["blocked_requirement_ids"],
        serde_json::json!(["G-005", "FR-010"])
    );
    assert_eq!(json["requirement_status_counts"]["passed"], 2);
    assert_eq!(json["requirement_status_counts"]["blocked_public"], 2);
    let vp6 = vp_rows
        .iter()
        .find(|row| row["id"] == "VP-006")
        .expect("VP-006 row");
    assert_eq!(
        vp6["evidence_artifact_ids"],
        serde_json::json!(["exact_recovery"])
    );
    let vp8 = vp_rows
        .iter()
        .find(|row| row["id"] == "VP-008")
        .expect("VP-008 row");
    assert_eq!(
        vp8["evidence_artifact_ids"],
        serde_json::json!(["claim_audit"])
    );
    assert_eq!(vp8["status"], "blocked_public");
    assert!(
        vp8["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "public claims intentionally blocked until release gates pass")
    );
    assert!(
        json["stop_before"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gate| gate == "publication")
    );
    assert!(
        json["anti_drift_reminders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["risk"] == "Repeated agent drift"
                && row["validation"]
                    .as_str()
                    .unwrap()
                    .contains("completion-audit"))
    );
    assert!(output_json.exists());
}

#[test]
fn cli_artifact_handoff_carries_completion_residual_gate_matrix() {
    let dir = tempdir().unwrap();
    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    std::fs::write(
        results_dir.join("tokenzero_completion_audit.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.completion_audit.v1",
            "release_candidate_id": "rc-fixture",
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
            "residual_gate_status_counts": {
                "blocked": 2
            },
            "claim_gate_snapshot": {
                "present": true,
                "release_candidate_id": "rc-fixture",
                "public_claims_approved": false,
                "gate_passes": {
                    "adapter_approval": false,
                    "release_candidate": true
                },
                "gate_reasons": {
                    "adapter_approval": ["adapter approval artifact has missing reviewed commands"],
                    "release_candidate": []
                },
                "release_candidate_ids": ["rc-fixture"],
                "release_candidate_artifacts": [
                    {
                        "artifact_id": "adapter_approval_artifact",
                        "artifact_path": "results/current/tokenzero_adapter_approval_audit.json",
                        "release_candidate_id": "rc-fixture",
                        "schema_version": "tokenzero.adapter_approval_audit.v1"
                    }
                ]
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
        }))
        .unwrap(),
    )
    .unwrap();
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .args(["artifact-handoff", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["completion_audit_snapshot"]["artifact_path"],
        "results/current/tokenzero_completion_audit.json"
    );
    assert_eq!(json["completion_audit_snapshot"]["present"], true);
    assert_eq!(
        json["completion_audit_snapshot"]["release_candidate_id"],
        "rc-fixture"
    );
    assert_eq!(
        json["completion_audit_snapshot"]["all_direct_file_evidence_present"],
        true
    );
    assert_eq!(
        json["completion_audit_snapshot"]["all_direct_artifact_evidence_valid"],
        false
    );
    assert_eq!(
        json["completion_audit_snapshot"]["all_requirement_rows_passed"],
        false
    );
    assert_eq!(
        json["completion_audit_snapshot"]["blocked_requirement_ids"],
        serde_json::json!(["G-008", "FR-010"])
    );
    assert_eq!(
        json["completion_audit_snapshot"]["requirement_status_counts"]["passed"],
        2
    );
    assert_eq!(
        json["completion_audit_snapshot"]["requirement_status_counts"]["passed_blocked"],
        1
    );
    assert_eq!(
        json["completion_audit_snapshot"]["requirement_status_counts"]["blocked_public"],
        1
    );
    assert_eq!(
        json["completion_audit_snapshot"]["all_residual_gates_resolved"],
        false
    );
    assert_eq!(
        json["completion_audit_snapshot"]["blocked_residual_gate_ids"],
        serde_json::json!(["adapter_approval", "release_approval"])
    );
    assert_eq!(
        json["completion_audit_snapshot"]["residual_gate_status_counts"]["blocked"],
        2
    );
    let evidence_integrity = json["completion_audit_snapshot"]["evidence_integrity_matrix"]
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
    let claim_snapshot = &json["completion_audit_snapshot"]["claim_gate_snapshot"];
    assert_eq!(claim_snapshot["present"], true);
    assert_eq!(claim_snapshot["gate_passes"]["adapter_approval"], false);
    assert_eq!(
        claim_snapshot["gate_reasons"]["adapter_approval"][0],
        "adapter approval artifact has missing reviewed commands"
    );
    let release_candidate_artifacts = claim_snapshot["release_candidate_artifacts"]
        .as_array()
        .expect("handoff carries claim release-candidate artifacts");
    assert_eq!(release_candidate_artifacts.len(), 1);
    assert_eq!(
        release_candidate_artifacts[0]["artifact_path"],
        "results/current/tokenzero_adapter_approval_audit.json"
    );
    let residuals = json["residual_gate_matrix"].as_array().unwrap();
    assert_eq!(json["all_residual_gates_resolved"], false);
    assert_eq!(
        json["blocked_residual_gate_ids"],
        serde_json::json!(["adapter_approval", "release_approval"])
    );
    assert_eq!(json["residual_gate_status_counts"]["blocked"], 2);
    let adapter = residuals
        .iter()
        .find(|row| row["gate_id"] == "adapter_approval")
        .expect("adapter residual handoff row");
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
    let release = residuals
        .iter()
        .find(|row| row["gate_id"] == "release_approval")
        .expect("release residual handoff row");
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
fn cli_artifact_handoff_uses_current_residual_actions_after_source_and_linux_evidence() {
    let dir = tempdir().unwrap();
    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    std::fs::write(
        results_dir.join("tokenzero_claim_audit.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.claim_audit.v1",
            "release_candidate_id": "rc-current",
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
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_os_reach_audit.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.os_reach_audit.v1",
            "release_candidate_id": "rc-current",
            "all_release_oses_run": false,
            "public_os_claim_approved": false,
            "blocked_reasons": ["macos not run with shell and install artifacts"],
            "os_rows": [
                {"os": "windows", "claim_ready": true},
                {"os": "linux", "claim_ready": true},
                {"os": "macos", "claim_ready": false}
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let completion_output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-current")
        .args(["completion-audit", "--json"])
        .output()
        .unwrap();
    assert!(
        completion_output.status.success(),
        "{}",
        String::from_utf8_lossy(&completion_output.stderr)
    );
    let completion: Value = serde_json::from_slice(&completion_output.stdout).unwrap();
    let handoff_output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-current")
        .args(["artifact-handoff", "--json"])
        .output()
        .unwrap();
    assert!(
        handoff_output.status.success(),
        "{}",
        String::from_utf8_lossy(&handoff_output.stderr)
    );
    let handoff: Value = serde_json::from_slice(&handoff_output.stdout).unwrap();
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
    assert!(os_action["action"].as_str().unwrap().contains("macOS"));
    assert!(
        !os_action["action"]
            .as_str()
            .unwrap()
            .contains("Linux and macOS")
    );
    assert!(
        os_action["validation"]
            .as_str()
            .unwrap()
            .contains("<macos.json>")
    );
    assert!(
        !os_action["validation"]
            .as_str()
            .unwrap()
            .contains("<linux.json> <macos.json>")
    );
    let artifacts = handoff["artifacts"].as_array().unwrap();
    let os_reach_purpose = artifacts
        .iter()
        .find(|row| row["id"] == "os_reach")
        .expect("OS reach artifact")["purpose"]
        .as_str()
        .unwrap();
    assert!(os_reach_purpose.contains("macOS"));
    assert!(
        !os_reach_purpose.contains("Linux/macOS"),
        "OS reach artifact purpose should not reintroduce missing Linux after Linux evidence exists"
    );
    let os_release_purpose = artifacts
        .iter()
        .find(|row| row["id"] == "os_release_artifact")
        .expect("OS release artifact")["purpose"]
        .as_str()
        .unwrap();
    assert!(os_release_purpose.contains("macOS"));
    assert!(
        !os_release_purpose.contains("Linux/macOS"),
        "OS release artifact purpose should not reintroduce missing Linux after Linux evidence exists"
    );
    let vp4 = handoff["verification_plan_matrix"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "VP-004")
        .expect("VP-004 handoff row");
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
    let benchmark = residuals
        .iter()
        .find(|row| row["gate_id"] == "benchmark_artifact")
        .expect("benchmark residual");
    assert_eq!(
        benchmark["next_action_id"],
        serde_json::json!("runnable_adapter_approval")
    );
    assert!(
        benchmark["next_action"]["validation"]
            .as_str()
            .unwrap()
            .contains("adapter-approval-audit")
    );
    let fr007 = completion["must_fr"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "FR-007")
        .expect("FR-007 completion row");
    let fr007_residual = fr007["residual"].as_str().unwrap();
    assert!(fr007_residual.contains("macOS"));
    assert!(!fr007_residual.contains("Linux/macOS"));
}

fn run_artifact_handoff_json(root: &std::path::Path, release_candidate_id: &str) -> Value {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(root)
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", release_candidate_id)
        .args(["artifact-handoff", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn handoff_artifact_row<'a>(json: &'a Value, artifact_id: &str) -> &'a Value {
    json["artifact_integrity_matrix"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == artifact_id)
        .unwrap_or_else(|| panic!("{artifact_id} integrity row"))
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

#[test]
fn cli_artifact_handoff_rejects_mismatched_release_candidate_artifacts() {
    let dir = tempdir().unwrap();
    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    write_minimal_handoff_completion_audit(&results_dir, "rc-current");
    write_json_fixture(
        &results_dir.join("tokenzero_source_currency.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.source_currency.v1",
            "release_candidate_id": "rc-other"
        }),
    );
    let json = run_artifact_handoff_json(dir.path(), "rc-current");
    let source = handoff_artifact_row(&json, "source_currency");
    assert_eq!(source["present"], true);
    assert_eq!(source["readable"], true);
    assert_eq!(source["schema_version"], "tokenzero.source_currency.v1");
    assert_eq!(source["expected_release_candidate_id"], "rc-current");
    assert_eq!(source["release_candidate_id"], "rc-other");
    assert_eq!(source["release_candidate_matches"], false);
    assert_eq!(source["valid"], false);
    assert_reason(source, "release_candidate_id mismatch");
}

#[test]
fn cli_artifact_handoff_rejects_swapped_and_malformed_schema_bound_artifacts() {
    let dir = tempdir().unwrap();
    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
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

    let claim = handoff_artifact_row(&json, "claim_audit");
    assert_eq!(claim["present"], true);
    assert_eq!(claim["readable"], true);
    assert_eq!(claim["schema_version"], "tokenzero.source_currency.v1");
    assert_eq!(claim["expected_schema_version"], "tokenzero.claim_audit.v1");
    assert_eq!(claim["schema_matches"], false);
    assert_eq!(claim["expected_release_candidate_id"], "rc-current");
    assert_eq!(claim["release_candidate_id"], "rc-current");
    assert_eq!(claim["release_candidate_matches"], true);
    assert_eq!(claim["valid"], false);
    assert_reason(claim, "schema_version mismatch");

    let bench = handoff_artifact_row(&json, "bench_competitors");
    assert_eq!(bench["present"], true);
    assert_eq!(bench["readable"], true);
    assert_eq!(bench["schema_version"], serde_json::Value::Null);
    assert_eq!(bench["expected_schema_version"], "tokenzero.bench.v1");
    assert_eq!(bench["schema_matches"], false);
    assert_eq!(bench["expected_release_candidate_id"], "rc-current");
    assert_eq!(bench["release_candidate_matches"], false);
    assert_eq!(bench["valid"], false);
    assert_reason(bench, "artifact JSON unreadable");

    let vp8_claim = handoff_verification_evidence_row(&json, "VP-008", "claim_audit");
    assert_eq!(vp8_claim["status"], "invalid");
    assert_eq!(vp8_claim["valid"], false);
    assert_reason(vp8_claim, "schema_version mismatch");

    let vp5_bench = handoff_verification_evidence_row(&json, "VP-005", "bench_competitors");
    assert_eq!(vp5_bench["status"], "invalid");
    assert_eq!(vp5_bench["valid"], false);
    assert_reason(vp5_bench, "artifact JSON unreadable");
    // Malformed/unreadable artifacts are treated as not present
    assert_eq!(json["all_verification_evidence_artifacts_present"], false);
    assert_eq!(json["all_verification_evidence_artifacts_valid"], false);
}

#[test]
fn cli_artifact_handoff_rejects_missing_schema_and_release_candidate_fields() {
    let dir = tempdir().unwrap();
    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    write_minimal_handoff_completion_audit(&results_dir, "rc-current");
    write_json_fixture(
        &results_dir.join("tokenzero_claim_audit.json"),
        &serde_json::json!({
            "release_candidate_id": "rc-current"
        }),
    );
    write_json_fixture(
        &results_dir.join("tokenzero_source_currency.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.source_currency.v1"
        }),
    );

    let json = run_artifact_handoff_json(dir.path(), "rc-current");

    let claim = handoff_artifact_row(&json, "claim_audit");
    assert_eq!(claim["present"], true);
    assert_eq!(claim["readable"], true);
    assert_eq!(claim["schema_version"], serde_json::Value::Null);
    assert_eq!(claim["expected_schema_version"], "tokenzero.claim_audit.v1");
    assert_eq!(claim["schema_matches"], false);
    assert_eq!(claim["expected_release_candidate_id"], "rc-current");
    assert_eq!(claim["release_candidate_id"], "rc-current");
    assert_eq!(claim["release_candidate_matches"], true);
    assert_eq!(claim["valid"], false);
    assert_reason(claim, "schema_version missing");

    let source = handoff_artifact_row(&json, "source_currency");
    assert_eq!(source["present"], true);
    assert_eq!(source["readable"], true);
    assert_eq!(source["schema_version"], "tokenzero.source_currency.v1");
    assert_eq!(
        source["expected_schema_version"],
        "tokenzero.source_currency.v1"
    );
    assert_eq!(source["schema_matches"], true);
    assert_eq!(source["expected_release_candidate_id"], "rc-current");
    assert_eq!(source["release_candidate_id"], serde_json::Value::Null);
    assert_eq!(source["release_candidate_matches"], false);
    assert_eq!(source["valid"], false);
    assert_reason(source, "release_candidate_id missing");

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
    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    std::fs::write(
        results_dir.join("tokenzero_completion_audit.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.completion_audit.v1",
            "completion_achieved": false,
            "public_claims_approved": false,
            "release_publication_allowed": false,
            "residual_gate_matrix": []
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_bench_competitors_shell_heavy.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.claim_audit.v1"
        }))
        .unwrap(),
    )
    .unwrap();
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
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .args(["artifact-handoff", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["all_required_artifacts_present"], false);
    let integrity = json["artifact_integrity_matrix"].as_array().unwrap();
    let completion = integrity
        .iter()
        .find(|row| row["id"] == "completion_audit")
        .expect("completion audit integrity row");
    assert_eq!(completion["present"], true);
    assert_eq!(completion["readable"], true);
    assert_eq!(
        completion["schema_version"],
        "tokenzero.completion_audit.v1"
    );
    let bench = integrity
        .iter()
        .find(|row| row["id"] == "bench_competitors")
        .expect("benchmark artifact integrity row");
    assert_eq!(bench["present"], true);
    assert_eq!(bench["readable"], true);
    assert_eq!(bench["schema_version"], "tokenzero.claim_audit.v1");
    assert_eq!(bench["expected_schema_version"], "tokenzero.bench.v1");
    assert_eq!(bench["schema_matches"], false);
    assert_eq!(bench["valid"], false);
    assert!(
        bench["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "schema_version mismatch")
    );
    let task_success = integrity
        .iter()
        .find(|row| row["id"] == "task_success")
        .expect("task success integrity row");
    assert_eq!(task_success["present"], false);
    assert_eq!(task_success["readable"], false);
    assert!(
        task_success["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "artifact missing")
    );
    let advanced_adr = integrity
        .iter()
        .find(|row| row["id"] == "advanced_adr")
        .expect("advanced ADR integrity row");
    assert_eq!(advanced_adr["present"], true);
    assert_eq!(advanced_adr["readable"], true);
    assert_eq!(
        advanced_adr["expected_content_markers"],
        serde_json::json!([
            "## ADR-",
            "Failure-first evidence:",
            "Residual gates:",
            "validate_prd_goal.py",
            "cargo test --workspace"
        ])
    );
    assert_eq!(advanced_adr["content_markers_present"], false);
    assert_eq!(advanced_adr["valid"], false);
    assert!(
        advanced_adr["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "content marker missing: validate_prd_goal.py")
    );
    let reconciliation = integrity
        .iter()
        .find(|row| row["id"] == "competitive_reconciliation")
        .expect("competitive reconciliation integrity row");
    assert_eq!(reconciliation["present"], true);
    assert_eq!(reconciliation["readable"], true);
    assert_eq!(
        reconciliation["expected_content_markers"],
        serde_json::json!(["Snapshot", "no gated action was performed"])
    );
    assert_eq!(reconciliation["content_markers_present"], false);
    assert_eq!(reconciliation["valid"], false);
    assert!(
        reconciliation["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "content marker missing: no gated action was performed")
    );
}
