use crate::artifact_contracts::{
    completion_claim_public_residual, completion_evidence_integrity_matrix, completion_req_row,
    completion_requirement_status_summary, completion_source_public_residual, handoff_artifact,
    handoff_artifact_integrity_matrix, handoff_completion_audit_snapshot,
    handoff_verification_evidence_integrity_matrix, handoff_verification_plan_status_summary,
    release_candidate_id, residual_gate_status_summary,
};
use crate::claim_actions::{
    artifact_loop_next_actions, completion_claim_gate_snapshot, completion_residual_gate_matrix,
    handoff_resolve_residual_next_actions, missing_release_os_rows, os_matrix_residual_message,
    os_reach_artifact_purpose, os_release_artifact_purpose, release_os_list_display,
};
use serde_json::json;
use std::path::Path;

pub(crate) fn completion_audit_report() -> serde_json::Value {
    let claim_gate_snapshot =
        completion_claim_gate_snapshot(Path::new("results/current/tokenzero_claim_audit.json"));
    let residual_gate_matrix = completion_residual_gate_matrix(&claim_gate_snapshot);
    let (residual_gate_status_counts, blocked_residual_gate_ids, all_residual_gates_resolved) =
        residual_gate_status_summary(&residual_gate_matrix);
    let missing_release_oses =
        missing_release_os_rows(Path::new("results/current/tokenzero_os_reach_audit.json"));
    let os_matrix_residual = os_matrix_residual_message(&missing_release_oses);
    let claim_public_residual = completion_claim_public_residual(&claim_gate_snapshot);
    let source_public_residual = completion_source_public_residual(Path::new(
        "results/current/tokenzero_source_currency.json",
    ));
    let g_goals = vec![
        json!({
            "id": "G-001",
            "status": "passed_private",
            "claim": "Competitive evidence ledger covers named and adjacent repositories",
            "direct_evidence": ["results/current/tokenzero_source_currency.json"],
            "residual": &source_public_residual
        }),
        json!({
            "id": "G-002",
            "status": "passed_private",
            "claim": "Benchmark harness measures TokenZero and accounts for competitor adapters",
            "direct_evidence": ["results/current/tokenzero_bench_competitors_shell_heavy.json", "results/current/tokenzero_adapter_approval_audit.json"],
            "residual": "runnable competitor execution remains approval-gated"
        }),
        json!({
            "id": "G-003",
            "status": "passed",
            "claim": "Exact Recovery Always has refs or degraded diagnostics",
            "direct_evidence": ["results/current/tokenzero_exact_recovery_audit.json", "results/current/tokenzero_exact_recovery_shell.json"],
            "residual": serde_json::Value::Null
        }),
        json!({
            "id": "G-004",
            "status": "passed_private",
            "claim": "Adaptive One-Shot Planner avoids hidden second-call dependence on golden critical traces",
            "direct_evidence": ["results/current/tokenzero_one_shot_eval.json"],
            "residual": "public one-shot claim remains gated"
        }),
        json!({
            "id": "G-005",
            "status": "blocked_public",
            "claim": "No-Daemon OS Runtime preserves Windows, macOS, and Linux behavior",
            "direct_evidence": ["results/current/tokenzero_os_reach_audit.json", "results/current/tokenzero_os_release_artifact.json"],
            "residual": os_matrix_residual
        }),
        json!({
            "id": "G-006",
            "status": "passed",
            "claim": "Stable CLI/MCP diagnostics separate transport from child command success",
            "direct_evidence": ["cargo test --workspace", "results/current/tokenzero_false_success_shell.json", "results/current/tokenzero_bench_competitors_shell_heavy.json"],
            "residual": serde_json::Value::Null
        }),
        json!({
            "id": "G-007",
            "status": "passed_private",
            "claim": "Reach and install coverage identifies intercepted and bypassed host surfaces",
            "direct_evidence": ["results/current/tokenzero_reach.json", "results/current/tokenzero_os_reach_audit.json", "results/current/tokenzero_os_release_artifact.json"],
            "residual": "non-current OS release artifacts remain gated"
        }),
        json!({
            "id": "G-008",
            "status": "passed_blocked",
            "claim": "Public Claim Gate blocks release-facing savings claims",
            "direct_evidence": ["results/current/tokenzero_claim_audit.json"],
            "residual": &claim_public_residual
        }),
        json!({
            "id": "G-009",
            "status": "passed",
            "claim": "Security and privacy keep raw payloads local and avoid unapproved external writes",
            "direct_evidence": ["results/current/tokenzero_security_privacy_audit.json"],
            "residual": serde_json::Value::Null
        }),
        json!({
            "id": "G-010",
            "status": "passed_private",
            "claim": "Agent Execution Pack supports future implementation without this chat",
            "direct_evidence": ["results/current/tokenzero_artifact_handoff.json", "docs/advanced-adr-execution-record.md", "results/current/tokenzero_competitive_superiority_reconciliation.md", "validate_prd_goal.py --min-score 930"],
            "residual": "completion remains blocked by explicit residual gates"
        }),
    ];
    let must_fr = vec![
        completion_req_row(
            "FR-001",
            "passed_private",
            &["results/current/tokenzero_source_currency.json"],
            &source_public_residual,
        ),
        completion_req_row(
            "FR-002",
            "passed_private",
            &[
                "results/current/tokenzero_bench_competitors_shell_heavy.json",
                "results/current/tokenzero_adapter_approval_audit.json",
            ],
            "runnable competitor adapters require approval",
        ),
        completion_req_row(
            "FR-003",
            "passed",
            &["results/current/tokenzero_exact_recovery_audit.json"],
            "",
        ),
        completion_req_row(
            "FR-004",
            "passed",
            &["results/current/tokenzero_protected_anchor_audit.json"],
            "",
        ),
        completion_req_row(
            "FR-005",
            "passed_private",
            &["results/current/tokenzero_one_shot_eval.json"],
            "public one-shot claim still gated",
        ),
        completion_req_row(
            "FR-006",
            "passed",
            &[
                "cargo test --workspace",
                "results/current/tokenzero_false_success_shell.json",
            ],
            "",
        ),
        completion_req_row(
            "FR-007",
            "blocked_public",
            &[
                "results/current/tokenzero_os_reach_audit.json",
                "results/current/tokenzero_os_release_artifact.json",
            ],
            &os_matrix_residual,
        ),
        completion_req_row(
            "FR-010",
            "passed_blocked",
            &["results/current/tokenzero_claim_audit.json"],
            &claim_public_residual,
        ),
    ];
    let critical_nfr = vec![
        completion_req_row(
            "NFR-001",
            "passed",
            &["results/current/tokenzero_exact_recovery_audit.json"],
            "",
        ),
        completion_req_row(
            "NFR-002",
            "blocked_public",
            &[
                "results/current/tokenzero_os_reach_audit.json",
                "results/current/tokenzero_os_release_artifact.json",
            ],
            &os_matrix_residual,
        ),
        completion_req_row(
            "NFR-003",
            "passed",
            &["results/current/tokenzero_security_privacy_audit.json"],
            "",
        ),
        completion_req_row(
            "NFR-004",
            "passed",
            &["results/current/tokenzero_security_privacy_audit.json"],
            "",
        ),
    ];
    let (requirement_status_counts, blocked_requirement_ids, all_requirement_rows_passed) =
        completion_requirement_status_summary(&[&g_goals, &must_fr, &critical_nfr]);
    let evidence_integrity_matrix =
        completion_evidence_integrity_matrix(&g_goals, &must_fr, &critical_nfr);
    let all_direct_file_evidence_present = evidence_integrity_matrix
        .iter()
        .all(|row| row["status"] != "missing");
    let all_direct_artifact_evidence_valid = evidence_integrity_matrix.iter().all(|row| {
        row["evidence_kind"] != "artifact" || row["artifact_valid"].as_bool().unwrap_or(false)
    });
    let os_matrix_residual_gap =
        format!("{os_matrix_residual}; do not claim OS-agnostic release readiness");
    let claim_public_residual_gap =
        format!("{claim_public_residual}; do not publish release-facing savings claims");
    let residual_gaps = vec![
        os_matrix_residual_gap.as_str(),
        claim_public_residual_gap.as_str(),
        "runnable competitor adapter execution requires reviewed commands and explicit approval",
        "release/publication/global install apply remain gated actions",
    ];
    json!({
        "schema_version": "tokenzero.completion_audit.v1",
        "release_candidate_id": release_candidate_id(),
        "status": "ok",
        "completion_status": "blocked",
        "ok": true,
        "completion_achieved": false,
        "final_summary_is_evidence": false,
        "public_claims_approved": false,
        "release_publication_allowed": false,
        "g_goals": g_goals,
        "must_fr": must_fr,
        "critical_nfr": critical_nfr,
        "requirement_status_counts": requirement_status_counts,
        "blocked_requirement_ids": blocked_requirement_ids,
        "all_requirement_rows_passed": all_requirement_rows_passed,
        "residual_gate_status_counts": residual_gate_status_counts,
        "blocked_residual_gate_ids": blocked_residual_gate_ids,
        "all_residual_gates_resolved": all_residual_gates_resolved,
        "evidence_integrity_matrix": evidence_integrity_matrix,
        "all_direct_file_evidence_present": all_direct_file_evidence_present,
        "all_direct_artifact_evidence_valid": all_direct_artifact_evidence_valid,
        "residual_gaps": residual_gaps,
        "claim_gate_snapshot": claim_gate_snapshot,
        "residual_gate_matrix": residual_gate_matrix,
        "artifact_loop_handoff": {
            "next": "OS matrix expansion, runnable adapter approval if desired, and final release-gate review",
            "stop_before": ["release", "publication", "remote mutation", "paid services", "global install apply"]
        }
    })
}

pub(crate) fn artifact_handoff_report(
    installed_wrapper_audit: serde_json::Value,
) -> serde_json::Value {
    let completion_audit_snapshot = handoff_completion_audit_snapshot(Path::new(
        "results/current/tokenzero_completion_audit.json",
    ));
    let completion_residual_gate_matrix = completion_audit_snapshot["residual_gate_matrix"].clone();
    let missing_release_oses =
        missing_release_os_rows(Path::new("results/current/tokenzero_os_reach_audit.json"));
    let os_reach_purpose = os_reach_artifact_purpose(&missing_release_oses);
    let os_release_artifact_purpose = os_release_artifact_purpose(&missing_release_oses);
    let artifacts = vec![
        handoff_artifact(
            "completion_audit",
            "results/current/tokenzero_completion_audit.json",
            "false-closure audit and requirement map",
        ),
        handoff_artifact(
            "security_privacy_audit",
            "results/current/tokenzero_security_privacy_audit.json",
            "G-009/NFR-003/NFR-004 local security and privacy proof",
        ),
        handoff_artifact(
            "bench_competitors",
            "results/current/tokenzero_bench_competitors_shell_heavy.json",
            "Safe Savings benchmark and unavailable-row adapter matrix",
        ),
        handoff_artifact(
            "adapter_approval_audit",
            "results/current/tokenzero_adapter_approval_audit.json",
            "Non-executing reviewed-command gate for runnable competitor adapters",
        ),
        handoff_artifact(
            "adapter_approval_file",
            "results/current/tokenzero_adapter_approval_file.json",
            "reviewed command-shape approval file; execution and public claims remain gated",
        ),
        handoff_artifact(
            "source_currency",
            "results/current/tokenzero_source_currency.json",
            "private source ledger and public freshness gate",
        ),
        handoff_artifact(
            "claim_audit",
            "results/current/tokenzero_claim_audit.json",
            "public claim gate, same-release-candidate check, and gated action list",
        ),
        handoff_artifact(
            "os_reach",
            "results/current/tokenzero_os_reach_audit.json",
            &os_reach_purpose,
        ),
        handoff_artifact(
            "os_release_artifact",
            "results/current/tokenzero_os_release_artifact.json",
            &os_release_artifact_purpose,
        ),
        handoff_artifact(
            "one_shot",
            "results/current/tokenzero_one_shot_eval.json",
            "golden critical trace one-shot adequacy evidence",
        ),
        handoff_artifact(
            "task_success",
            "results/current/tokenzero_one_shot_eval.json",
            "claim-gate task-success proof from one-shot adequacy rows",
        ),
        handoff_artifact(
            "exact_recovery",
            "results/current/tokenzero_exact_recovery_audit.json",
            "normal and degraded exact recovery audit",
        ),
        handoff_artifact(
            "exact_recovery_shell",
            "results/current/tokenzero_exact_recovery_shell.json",
            "VP-006 byte-perfect shell expand checks for emitted local refs",
        ),
        handoff_artifact(
            "false_success_shell",
            "results/current/tokenzero_false_success_shell.json",
            "FR-006 shell status truth audit for nonzero, failed cd, masked pipeline, timeout, and success",
        ),
        handoff_artifact(
            "reach",
            "results/current/tokenzero_reach.json",
            "FR-008/G-007 host reach and installed wrapper trust evidence",
        ),
        handoff_artifact(
            "mcp_smoke",
            "results/current/rust_mcp_smoke.json",
            "VP-003 MCP smoke proof with ok true and no unexpected exits",
        ),
        handoff_artifact(
            "shell_matrix",
            "results/current/tokenzero_shell_matrix.json",
            "VP-004 shell matrix proof for current-host runtime behavior",
        ),
        handoff_artifact(
            "advanced_adr",
            "docs/advanced-adr-execution-record.md",
            "phase decisions and evidence record",
        ),
        handoff_artifact(
            "competitive_reconciliation",
            "results/current/tokenzero_competitive_superiority_reconciliation.md",
            "residual gate reconciliation snapshot and no-gated-action proof",
        ),
    ];
    let (artifact_integrity_matrix, all_required_artifacts_present, all_required_artifacts_valid) =
        handoff_artifact_integrity_matrix(&artifacts);
    let verification_plan_matrix = handoff_verification_plan_matrix(&missing_release_oses);
    let (
        verification_plan_status_counts,
        blocked_verification_plan_ids,
        all_verification_plan_rows_passed,
    ) = handoff_verification_plan_status_summary(&verification_plan_matrix);
    let (
        verification_evidence_integrity_matrix,
        all_verification_evidence_artifacts_present,
        all_verification_evidence_artifacts_valid,
    ) = handoff_verification_evidence_integrity_matrix(
        &verification_plan_matrix,
        &artifact_integrity_matrix,
    );
    let next_actions = artifact_loop_next_actions(&completion_residual_gate_matrix);
    let residual_gate_matrix =
        handoff_resolve_residual_next_actions(&completion_residual_gate_matrix, &next_actions);
    let (residual_gate_status_counts, blocked_residual_gate_ids, mut all_residual_gates_resolved) =
        residual_gate_status_summary(&residual_gate_matrix);
    if completion_audit_snapshot["present"] != true {
        all_residual_gates_resolved = false;
    }
    let global_tokenzero_release_verification_trusted =
        installed_wrapper_audit["resolved_is_current_exe"]
            .as_bool()
            .unwrap_or(false);
    let release_verification_binary = installed_wrapper_audit["current_exe"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let anti_drift_reminders = vec![
        json!({
            "risk": "Repeated source staleness",
            "action": "Add or rerun source currency command before public claims",
            "surface": "repo CLI or docs",
            "validation": "claim audit"
        }),
        json!({
            "risk": "Repeated agent drift",
            "action": "Use completion-audit and this handoff packet before final response",
            "surface": "PRD template or skill",
            "validation": "completion-audit"
        }),
        json!({
            "risk": "Global wrapper drift",
            "action": if global_tokenzero_release_verification_trusted {
                "global tokenzero resolves to the current release-verification executable"
            } else {
                "use release_verification_binary or explicitly approved install apply before relying on global tokenzero"
            },
            "surface": "local shell",
            "validation": "installed_wrapper_audit"
        }),
    ];
    let stop_before = vec![
        "release",
        "publication",
        "remote mutation",
        "paid services",
        "global install apply",
        "public benchmark claim",
    ];
    json!({
        "schema_version": "tokenzero.artifact_handoff.v1",
        "release_candidate_id": release_candidate_id(),
        "status": "ok",
        "ok": true,
        "completion_achieved": false,
        "public_claims_approved": false,
        "release_publication_allowed": false,
        "installed_wrapper_audit": installed_wrapper_audit,
        "global_tokenzero_release_verification_trusted": global_tokenzero_release_verification_trusted,
        "approved_install_required_for_global_update": !global_tokenzero_release_verification_trusted,
        "release_verification_binary": release_verification_binary,
        "artifacts": artifacts,
        "artifact_integrity_matrix": artifact_integrity_matrix,
        "all_required_artifacts_present": all_required_artifacts_present,
        "all_required_artifacts_valid": all_required_artifacts_valid,
        "verification_plan_matrix": verification_plan_matrix,
        "verification_evidence_integrity_matrix": verification_evidence_integrity_matrix,
        "all_verification_evidence_artifacts_present": all_verification_evidence_artifacts_present,
        "all_verification_evidence_artifacts_valid": all_verification_evidence_artifacts_valid,
        "verification_plan_status_counts": verification_plan_status_counts,
        "blocked_verification_plan_ids": blocked_verification_plan_ids,
        "all_verification_plan_rows_passed": all_verification_plan_rows_passed,
        "requirement_status_counts": completion_audit_snapshot["requirement_status_counts"].clone(),
        "blocked_requirement_ids": completion_audit_snapshot["blocked_requirement_ids"].clone(),
        "all_requirement_rows_passed": completion_audit_snapshot["all_requirement_rows_passed"].clone(),
        "residual_gate_status_counts": residual_gate_status_counts,
        "blocked_residual_gate_ids": blocked_residual_gate_ids,
        "all_residual_gates_resolved": all_residual_gates_resolved,
        "completion_audit_snapshot": completion_audit_snapshot,
        "residual_gate_matrix": residual_gate_matrix,
        "next_actions": next_actions,
        "anti_drift_reminders": anti_drift_reminders,
        "stop_before": stop_before,
        "thread_goal": "Implement tokenzero_competitive_superiority_goal.md phase by phase with verification evidence",
        "handoff_note": "Use current worktree and artifacts as authoritative; do not infer completion from summary prose."
    })
}

fn handoff_verification_plan_matrix(missing_release_oses: &[String]) -> serde_json::Value {
    let vp4_status = if missing_release_oses.is_empty() {
        "passed_local"
    } else {
        "blocked_public"
    };
    let vp4_blocked_reasons = os_matrix_verification_blocked_reasons(missing_release_oses);
    let vp4_stop_before = if missing_release_oses.is_empty() {
        Vec::<&str>::new()
    } else {
        vec!["OS-agnostic public claim", "publication"]
    };
    json!([
        {
            "id": "VP-001",
            "command": "python scripts/validate_prd_goal.py PRD_GOAL.md --min-score 930",
            "status": "passed_local",
            "evidence_artifact_ids": ["advanced_adr"],
            "passing_condition": "PASS and no check failures",
            "blocked_reasons": [],
            "stop_before": []
        },
        {
            "id": "VP-002",
            "command": "cargo test --workspace",
            "status": "passed_local",
            "evidence_artifact_ids": ["completion_audit"],
            "passing_condition": "exit code 0",
            "blocked_reasons": [],
            "stop_before": []
        },
        {
            "id": "VP-003",
            "command": "target\\windows-verify\\release\\tokenzero.exe mcp-smoke --json",
            "status": "passed_local",
            "evidence_artifact_ids": ["mcp_smoke"],
            "passing_condition": "ok true and no unexpected exits",
            "blocked_reasons": [],
            "stop_before": []
        },
        {
            "id": "VP-004",
            "command": "target\\windows-verify\\release\\tokenzero.exe shell-matrix --json",
            "status": vp4_status,
            "evidence_artifact_ids": ["shell_matrix", "os_release_artifact", "os_reach"],
            "passing_condition": "each release OS passes before OS claim",
            "blocked_reasons": vp4_blocked_reasons,
            "stop_before": vp4_stop_before
        },
        {
            "id": "VP-005",
            "command": "target\\windows-verify\\release\\tokenzero.exe bench competitors --suite shell-heavy --json",
            "status": "passed_private",
            "evidence_artifact_ids": ["bench_competitors", "adapter_approval_audit"],
            "passing_condition": "Safe Savings artifact, honest unavailable rows, public_claims_approved false until evidence",
            "blocked_reasons": ["public benchmark claim remains gated"],
            "stop_before": ["public benchmark claim", "publication"]
        },
        {
            "id": "VP-006",
            "command": "exact expand checks for every emitted local ref in benchmark suite",
            "status": "passed_local",
            "evidence_artifact_ids": ["exact_recovery"],
            "passing_condition": "byte-perfect recovery true",
            "blocked_reasons": [],
            "stop_before": []
        },
        {
            "id": "VP-007",
            "command": "one-shot golden trace evaluator",
            "status": "passed_private",
            "evidence_artifact_ids": ["one_shot", "task_success"],
            "passing_condition": "0% critical miss and less than 2% overall miss",
            "blocked_reasons": ["public one-shot claim remains gated"],
            "stop_before": ["publication"]
        },
        {
            "id": "VP-008",
            "command": "claim audit with source refresh",
            "status": "blocked_public",
            "evidence_artifact_ids": ["claim_audit"],
            "passing_condition": "approved only when source and benchmark evidence agree",
            "blocked_reasons": ["public claims intentionally blocked until release gates pass"],
            "stop_before": ["release", "publication", "global install apply"]
        }
    ])
}

fn os_matrix_verification_blocked_reasons(missing: &[String]) -> Vec<String> {
    if missing.is_empty() {
        return Vec::new();
    }
    vec![format!(
        "{} shell matrix artifacts not run on this host",
        release_os_list_display(missing)
    )]
}

#[cfg(test)]
mod tests {
    #[test]
    fn completion_handoff_does_not_import_cli_monolith_or_artifact_writer() {
        let source = include_str!("completion_handoff.rs");
        let forbidden = [
            concat!("write_", "artifacts("),
            concat!("installed_", "tokenzero_command_audit("),
            concat!("use ", "super::"),
            concat!("crate::", "main"),
        ];
        for forbidden in forbidden {
            assert!(
                !source.contains(forbidden),
                "completion_handoff.rs must stay report-only and independent of CLI facade: {forbidden}"
            );
        }
    }
}
