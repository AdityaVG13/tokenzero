use anyhow::Result;
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::artifact_contracts::{json_artifact_path, load_json_artifact, release_candidate_id};
use crate::competitor_adapters::REQUIRED_COMPETITOR_ADAPTERS;
use crate::source_currency;
use crate::write_artifacts;

pub(crate) struct ClaimEvidenceInputs {
    pub(crate) source_artifact: Option<PathBuf>,
    pub(crate) benchmark_artifact: Option<PathBuf>,
    pub(crate) adapter_approval_artifact: Option<PathBuf>,
    pub(crate) recovery_artifact: Option<PathBuf>,
    pub(crate) task_success_artifact: Option<PathBuf>,
    pub(crate) os_artifact: Option<PathBuf>,
}

impl ClaimEvidenceInputs {
    fn with_current_defaults(mut self) -> Self {
        for (slot, name) in [
            (&mut self.source_artifact, "tokenzero_source_currency.json"),
            (&mut self.benchmark_artifact, "tokenzero_bench_competitors_shell_heavy.json"),
            (&mut self.adapter_approval_artifact, "tokenzero_adapter_approval_audit.json"),
            (&mut self.recovery_artifact, "tokenzero_exact_recovery_audit.json"),
            (&mut self.task_success_artifact, "tokenzero_one_shot_eval.json"),
            (&mut self.os_artifact, "tokenzero_os_reach_audit.json"),
        ] {
            if slot.is_none() {
                *slot = current_claim_artifact_path(name);
            }
        }
        self
    }
}

fn current_claim_artifact_path(filename: &str) -> Option<PathBuf> {
    let path = PathBuf::from("results").join("current").join(filename);
    path.is_file().then_some(path)
}

pub(crate) fn run_claim_audit(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
    release_approval: bool,
    inputs: ClaimEvidenceInputs,
) -> Result<serde_json::Value> {
    let inputs = inputs.with_current_defaults();
    let source_currency = if let Some(path) = inputs.source_artifact.as_ref() {
        load_json_artifact(path)?
    } else {
        source_currency::source_currency_report(&release_candidate_id())
    };
    let source_gate = evaluate_source_claim_gate(&source_currency, inputs.source_artifact.as_ref());
    let benchmark_gate = evaluate_benchmark_claim_gate(inputs.benchmark_artifact.as_ref())?;
    let adapter_approval_gate =
        evaluate_adapter_approval_claim_gate(inputs.adapter_approval_artifact.as_ref())?;
    let recovery_gate = evaluate_recovery_claim_gate(inputs.recovery_artifact.as_ref())?;
    let task_success_gate =
        evaluate_task_success_claim_gate(inputs.task_success_artifact.as_ref())?;
    let os_gate = evaluate_os_claim_gate(inputs.os_artifact.as_ref())?;
    let release_candidate_gate = evaluate_release_candidate_claim_gate(&inputs)?;
    let release_gate = json!({
        "id": "release_approval",
        "pass": release_approval,
        "artifact_path": serde_json::Value::Null,
        "reasons": if release_approval { Vec::<String>::new() } else { vec!["release approval not granted".to_string()] },
        "details": {
            "release_approval": release_approval
        }
    });
    let evidence_gates = vec![
        source_gate.clone(),
        benchmark_gate.clone(),
        adapter_approval_gate.clone(),
        recovery_gate.clone(),
        task_success_gate.clone(),
        os_gate.clone(),
        release_candidate_gate.clone(),
        release_gate.clone(),
    ];
    let (
        gate_passes,
        gate_reasons,
        gate_artifact_paths,
        release_candidate_ids,
        release_candidate_artifacts,
    ) = claim_gate_summary(&evidence_gates);
    let public_claims_approved = evidence_gates.iter().all(|gate| gate["pass"] == true);
    let release_publication_allowed = public_claims_approved;
    let claims = vec![
        json!({
            "claim_id": "tokenzero_safe_savings",
            "claim": "TokenZero Safe Savings is release-ready",
            "source_current": source_gate["pass"],
            "benchmark_artifact_current": benchmark_gate["pass"],
            "adapter_execution_approved": adapter_approval_gate["pass"],
            "byte_perfect_recovery": recovery_gate["pass"],
            "task_success": task_success_gate["pass"],
            "release_approval": release_approval,
            "approved": public_claims_approved,
            "public_safe_to_publish": public_claims_approved,
            "reason": "release-facing savings claims remain gated until fresh sources, benchmark artifacts, recovery evidence, task success, and explicit approval all agree"
        }),
        json!({
            "claim_id": "os_agnostic",
            "claim": "TokenZero is proven across Windows, macOS, and Linux",
            "source_current": source_gate["pass"],
            "benchmark_artifact_current": benchmark_gate["pass"],
            "byte_perfect_recovery": recovery_gate["pass"],
            "task_success": os_gate["pass"],
            "release_approval": release_approval,
            "approved": public_claims_approved && os_gate["pass"] == true,
            "public_safe_to_publish": public_claims_approved && os_gate["pass"] == true,
            "reason": "all three OS artifact rows must be present before the public OS claim is approved"
        }),
    ];
    let mut blocked_reasons = Vec::<String>::new();
    for gate in &evidence_gates {
        if let Some(reasons) = gate["reasons"].as_array() {
            for reason in reasons {
                let reason = reason.as_str().unwrap_or_default().to_string();
                if !reason.is_empty() && !blocked_reasons.contains(&reason) {
                    blocked_reasons.push(reason);
                }
            }
        }
    }
    let claim_status = if public_claims_approved {
        "approved"
    } else {
        "blocked"
    };
    let report = json!({
        "schema_version": "tokenzero.claim_audit.v1",
        "release_candidate_id": release_candidate_id(),
        "status": "ok",
        "transport_status": "ok",
        "claim_status": claim_status,
        "ok": true,
        "public_claims_approved": public_claims_approved,
        "release_publication_allowed": release_publication_allowed,
        "blocked_reasons": blocked_reasons,
        "evidence_gates": evidence_gates,
        "gate_passes": gate_passes,
        "gate_reasons": gate_reasons,
        "gate_artifact_paths": gate_artifact_paths,
        "release_candidate_ids": release_candidate_ids,
        "release_candidate_artifacts": release_candidate_artifacts,
        "claims": claims,
        "source_currency": source_currency,
        "source_ledger": source_currency["rows"],
        "gated_actions": ["release", "publication", "remote mutation", "paid services", "global install apply"]
    });
    write_artifacts(&output_json, output_md.as_deref(), &report, "Claim audit")?;
    Ok(report)
}

fn claim_gate_summary(
    evidence_gates: &[serde_json::Value],
) -> (
    serde_json::Value,
    serde_json::Value,
    serde_json::Value,
    Vec<serde_json::Value>,
    Vec<serde_json::Value>,
) {
    let mut gate_passes = serde_json::Map::new();
    let mut gate_reasons = serde_json::Map::new();
    let mut gate_artifact_paths = serde_json::Map::new();
    let mut release_candidate_ids = Vec::<serde_json::Value>::new();
    let mut release_candidate_artifacts = Vec::<serde_json::Value>::new();

    for gate in evidence_gates {
        if let Some(id) = gate["id"].as_str() {
            gate_passes.insert(id.to_string(), gate["pass"].clone());
            gate_reasons.insert(
                id.to_string(),
                json!(gate["reasons"].as_array().cloned().unwrap_or_default()),
            );
            gate_artifact_paths.insert(id.to_string(), gate["artifact_path"].clone());
            if id == "release_candidate" {
                release_candidate_ids = gate["details"]["release_candidate_ids"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                release_candidate_artifacts = gate["details"]["artifacts"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
            }
        }
    }

    (
        json!(gate_passes),
        json!(gate_reasons),
        json!(gate_artifact_paths),
        release_candidate_ids,
        release_candidate_artifacts,
    )
}

fn claim_gate(
    id: &str,
    artifact_path: Option<&Path>,
    reasons: Vec<String>,
    details: serde_json::Value,
) -> serde_json::Value {
    json!({
        "id": id,
        "pass": reasons.is_empty(),
        "artifact_path": artifact_path.map(json_artifact_path),
        "reasons": reasons,
        "details": details
    })
}

fn evaluate_release_candidate_claim_gate(
    inputs: &ClaimEvidenceInputs,
) -> Result<serde_json::Value> {
    let artifact_paths = [
        ("source_artifact", inputs.source_artifact.as_ref()),
        ("benchmark_artifact", inputs.benchmark_artifact.as_ref()),
        (
            "adapter_approval_artifact",
            inputs.adapter_approval_artifact.as_ref(),
        ),
        ("recovery_artifact", inputs.recovery_artifact.as_ref()),
        (
            "task_success_artifact",
            inputs.task_success_artifact.as_ref(),
        ),
        ("os_artifact", inputs.os_artifact.as_ref()),
    ];
    let mut reasons = Vec::new();
    let mut release_candidate_ids = Vec::<String>::new();
    let mut rows = Vec::new();
    let mut attached_artifact_count = 0usize;

    for (artifact_id, path) in artifact_paths {
        match path {
            Some(path) => {
                attached_artifact_count += 1;
                let artifact = load_json_artifact(path)?;
                let release_candidate_id = artifact["release_candidate_id"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if release_candidate_id.is_empty() {
                    push_unique_reason(
                        &mut reasons,
                        "evidence artifact missing release_candidate_id",
                    );
                } else if !release_candidate_ids
                    .iter()
                    .any(|existing| existing == &release_candidate_id)
                {
                    release_candidate_ids.push(release_candidate_id.clone());
                }
                rows.push(json!({
                    "artifact_id": artifact_id,
                    "artifact_path": json_artifact_path(path),
                    "schema_version": artifact["schema_version"],
                    "release_candidate_id": if release_candidate_id.is_empty() {
                        serde_json::Value::Null
                    } else {
                        json!(release_candidate_id)
                    }
                }));
            }
            None => {
                push_unique_reason(&mut reasons, "same-release-candidate evidence incomplete");
                rows.push(json!({
                    "artifact_id": artifact_id,
                    "artifact_path": serde_json::Value::Null,
                    "schema_version": serde_json::Value::Null,
                    "release_candidate_id": serde_json::Value::Null
                }));
            }
        }
    }

    if release_candidate_ids.len() > 1 {
        push_unique_reason(
            &mut reasons,
            "evidence artifacts are not from the same release candidate",
        );
    }

    Ok(claim_gate(
        "release_candidate",
        None,
        reasons,
        json!({
            "artifact_count": rows.len(),
            "attached_artifact_count": attached_artifact_count,
            "release_candidate_ids": release_candidate_ids,
            "artifacts": rows
        }),
    ))
}

fn evaluate_source_claim_gate(
    source_currency: &serde_json::Value,
    artifact_path: Option<&PathBuf>,
) -> serde_json::Value {
    let mut reasons = Vec::new();
    let mut pinned_commit_count = 0usize;
    let mut missing_commit_count = 0usize;
    let mut unpinned_source_rows = Vec::<serde_json::Value>::new();
    if source_currency["schema_version"] != "tokenzero.source_currency.v1" {
        reasons.push("source artifact schema mismatch".to_string());
    }
    if source_currency["fresh_for_public_claim"] != true {
        reasons.push("source ledger requires same-release-candidate refresh".to_string());
        reasons.push("source refresh not same-release-candidate".to_string());
    }
    if source_currency["rows"]
        .as_array()
        .is_none_or(|rows| rows.len() < REQUIRED_COMPETITOR_ADAPTERS.len())
    {
        reasons.push("source ledger missing required competitor rows".to_string());
    }
    if let Some(rows) = source_currency["rows"].as_array() {
        for tool in REQUIRED_COMPETITOR_ADAPTERS {
            if !rows.iter().any(|row| row["tool"] == *tool) {
                push_unique_reason(
                    &mut reasons,
                    "source ledger missing required competitor rows",
                );
            }
        }
        for row in rows {
            if row["source_date"].as_str().is_none_or(str::is_empty) {
                push_unique_reason(&mut reasons, "source ledger row missing source date");
            }
            let source_commit = row["source_commit"].as_str().unwrap_or_default().trim();
            if source_commit.is_empty() {
                missing_commit_count += 1;
                push_unique_reason(&mut reasons, "source ledger row missing source commit");
            } else if source_currency::source_commit_is_release_candidate_pin(source_commit) {
                pinned_commit_count += 1;
            } else {
                push_unique_reason(
                    &mut reasons,
                    "source ledger row source commit is not a release-candidate pin",
                );
                unpinned_source_rows.push(json!({
                    "tool": row["tool"],
                    "url": row["url"],
                    "source_commit": source_commit
                }));
            }
            if !row["url"]
                .as_str()
                .is_some_and(|url| url.starts_with("https://github.com/"))
            {
                push_unique_reason(&mut reasons, "source ledger row missing primary URL");
            }
            if row["claimed_scope"].as_str().is_none_or(str::is_empty) {
                push_unique_reason(&mut reasons, "source ledger row missing claimed scope");
            }
            for (field, reason) in [
                (
                    "issue_pr_themes",
                    "source ledger row missing issue/PR themes",
                ),
                ("strengths", "source ledger row missing strengths"),
                ("gaps", "source ledger row missing gaps"),
            ] {
                if row[field].as_array().is_none_or(Vec::is_empty) {
                    push_unique_reason(&mut reasons, reason);
                }
            }
        }
    }
    claim_gate(
        "source_currency",
        artifact_path.map(PathBuf::as_path),
        reasons,
        json!({
            "schema_version": source_currency["schema_version"],
            "release_candidate_id": source_currency["release_candidate_id"],
            "fresh_for_public_claim": source_currency["fresh_for_public_claim"],
            "row_count": source_currency["rows"].as_array().map_or(0, Vec::len),
            "source_commit_pin_status": {
                "pinned": pinned_commit_count,
                "missing": missing_commit_count,
                "unpinned": unpinned_source_rows.len()
            },
            "unpinned_source_rows": unpinned_source_rows
        }),
    )
}

fn push_unique_reason(reasons: &mut Vec<String>, reason: &str) {
    if !reasons.iter().any(|existing| existing == reason) {
        reasons.push(reason.to_string());
    }
}

fn evaluate_benchmark_claim_gate(artifact_path: Option<&PathBuf>) -> Result<serde_json::Value> {
    let Some(path) = artifact_path else {
        return Ok(missing_claim_gate(
            "benchmark_artifact",
            "benchmark artifact not approved for publication",
        ));
    };
    let artifact = load_json_artifact(path)?;
    let mut reasons = Vec::new();
    if artifact["schema_version"] != "tokenzero.bench.v1" {
        reasons.push("benchmark artifact schema mismatch".to_string());
    }
    if artifact["ok"] != true {
        reasons.push("benchmark artifact did not pass".to_string());
    }
    if artifact["public_claims_approved"] != true {
        reasons.push("benchmark artifact not approved for publication".to_string());
    }
    let adapter_matrix = &artifact["adapter_matrix"];
    if adapter_matrix["all_required_adapters_accounted"] != true {
        reasons.push(
            "benchmark adapter matrix does not account for all required competitors".to_string(),
        );
    }
    if adapter_matrix["blind_install_attempted"] == true {
        reasons.push("benchmark attempted blind install".to_string());
    }
    if artifact["rows"].as_array().is_none_or(Vec::is_empty) {
        reasons.push("benchmark rows missing".to_string());
    }
    let mut public_claim_status = benchmark_public_claim_status(artifact["rows"].as_array());
    let competitor_unavailable_rows = public_claim_status["competitor_unavailable_rows"]
        .as_u64()
        .unwrap_or(0);
    let competitor_non_runnable_rows = public_claim_status["competitor_non_runnable_rows"]
        .as_u64()
        .unwrap_or(0);
    if competitor_unavailable_rows > 0 {
        push_unique_reason(
            &mut reasons,
            "benchmark competitor rows must be runnable for public claims",
        );
    }
    if competitor_non_runnable_rows > 0 {
        push_unique_reason(
            &mut reasons,
            "benchmark competitor rows include non-runnable public claim evidence",
        );
    }
    if let Some(rows) = artifact["rows"].as_array() {
        for row in rows {
            validate_benchmark_public_claim_row(row, &mut reasons);
        }
    }
    public_claim_status["gate_reasons"] = json!(reasons.clone());
    Ok(claim_gate(
        "benchmark_artifact",
        Some(path.as_path()),
        reasons,
        json!({
            "schema_version": artifact["schema_version"],
            "public_claims_approved": artifact["public_claims_approved"],
            "adapter_matrix": adapter_matrix,
            "public_claim_status": public_claim_status
        }),
    ))
}

fn benchmark_public_claim_status(rows: Option<&Vec<serde_json::Value>>) -> serde_json::Value {
    let mut tokenzero_run_rows = 0usize;
    let mut competitor_run_rows = 0usize;
    let mut competitor_unavailable_rows = 0usize;
    let mut competitor_non_runnable_rows = 0usize;
    let mut unavailable_competitors = Vec::<serde_json::Value>::new();
    let mut non_runnable_competitors = Vec::<serde_json::Value>::new();
    if let Some(rows) = rows {
        for row in rows {
            let tool = row["tool"].as_str().unwrap_or_default();
            let status = row["availability_status"].as_str().unwrap_or_default();
            if tool == "tokenzero" && status == "run" {
                tokenzero_run_rows += 1;
            } else if tool != "tokenzero" {
                if status == "run" {
                    competitor_run_rows += 1;
                } else if status == "unavailable" {
                    competitor_unavailable_rows += 1;
                    unavailable_competitors.push(json!({
                        "tool": row["tool"],
                        "scenario_id": row["scenario_id"],
                        "availability_status": row["availability_status"],
                        "availability_reason": row["availability_reason"]
                    }));
                } else {
                    competitor_non_runnable_rows += 1;
                    non_runnable_competitors.push(json!({
                        "tool": row["tool"],
                        "scenario_id": row["scenario_id"],
                        "availability_status": row["availability_status"],
                        "availability_reason": row["availability_reason"]
                    }));
                }
            }
        }
    }
    json!({
        "tokenzero_run_rows": tokenzero_run_rows,
        "competitor_run_rows": competitor_run_rows,
        "competitor_unavailable_rows": competitor_unavailable_rows,
        "competitor_non_runnable_rows": competitor_non_runnable_rows,
        "unavailable_competitors": unavailable_competitors,
        "non_runnable_competitors": non_runnable_competitors
    })
}

fn validate_benchmark_public_claim_row(row: &serde_json::Value, reasons: &mut Vec<String>) {
    for field in ["tool", "suite", "availability_status", "fairness_notes"] {
        if row[field].is_null() {
            push_unique_reason(
                reasons,
                &format!("benchmark row missing public-claim field: {field}"),
            );
        }
    }
    for field in [
        "raw_tokens",
        "visible_tokens",
        "recovery_tokens",
        "safe_savings",
        "harm_rate",
    ] {
        if !row[field].is_number() {
            push_unique_reason(
                reasons,
                &format!("benchmark row missing public-claim field: {field}"),
            );
        }
    }
    if !row["task_success"].is_boolean() {
        push_unique_reason(
            reasons,
            "benchmark row missing public-claim field: task_success",
        );
    }
    let availability_status = row["availability_status"].as_str().unwrap_or_default();
    if availability_status != "run" {
        if row["availability_reason"]
            .as_str()
            .is_none_or(|value| value.trim().is_empty())
        {
            push_unique_reason(
                reasons,
                "benchmark unavailable row missing availability_reason",
            );
        }
        return;
    }
    if !row["byte_perfect_recovery"].is_boolean() {
        push_unique_reason(
            reasons,
            "benchmark row missing public-claim field: byte_perfect_recovery",
        );
    } else if row["byte_perfect_recovery"] != true {
        push_unique_reason(reasons, "benchmark row failed byte-perfect recovery");
    }
    match row["exact_expand_checks"].as_array() {
        Some(checks) if !checks.is_empty() => {
            if !checks.iter().all(|check| check["byte_perfect"] == true) {
                push_unique_reason(reasons, "benchmark row has non-byte-perfect expand checks");
            }
            if checks.iter().any(|check| {
                check["ref"]
                    .as_str()
                    .is_none_or(|value| !value.starts_with("tz://"))
            }) {
                push_unique_reason(reasons, "benchmark row exact expand check missing ref");
            }
        }
        Some(_) => push_unique_reason(reasons, "benchmark row has non-byte-perfect expand checks"),
        None => push_unique_reason(
            reasons,
            "benchmark row missing public-claim field: exact_expand_checks",
        ),
    }
}

fn evaluate_adapter_approval_claim_gate(
    artifact_path: Option<&PathBuf>,
) -> Result<serde_json::Value> {
    let Some(path) = artifact_path else {
        return Ok(missing_claim_gate(
            "adapter_approval",
            "adapter approval artifact not attached to public claim",
        ));
    };
    let artifact = load_json_artifact(path)?;
    let mut reasons = Vec::new();
    if artifact["schema_version"] != "tokenzero.adapter_approval_audit.v1" {
        reasons.push("adapter approval artifact schema invalid".to_string());
    }
    if artifact["blind_install_attempted"] == true {
        reasons.push("adapter approval artifact attempted blind install".to_string());
    }
    if artifact["execution_allowed"] != true {
        reasons.push("adapter approval artifact does not allow execution".to_string());
    }
    if artifact["public_claims_approved"] != true {
        reasons.push("adapter approval artifact not approved for public claims".to_string());
    }
    if artifact["missing_reviewed_command_count"]
        .as_u64()
        .unwrap_or(1)
        > 0
    {
        reasons.push("adapter approval artifact has missing reviewed commands".to_string());
    }
    if artifact["unsafe_command_count"].as_u64().unwrap_or(1) > 0 {
        reasons.push("adapter approval artifact has unsafe reviewed commands".to_string());
    }
    if artifact["duplicate_command_count"].as_u64().unwrap_or(0) > 0 {
        reasons.push("adapter approval artifact has duplicate reviewed commands".to_string());
    }
    if artifact["required_adapter_count"].as_u64().unwrap_or(0)
        < REQUIRED_COMPETITOR_ADAPTERS.len() as u64
    {
        reasons.push("adapter approval artifact does not cover required adapters".to_string());
    }
    validate_adapter_approval_rows(&artifact, &mut reasons);
    Ok(claim_gate(
        "adapter_approval",
        Some(path.as_path()),
        reasons,
        json!({
            "schema_version": artifact["schema_version"],
            "execution_allowed": artifact["execution_allowed"],
            "public_claims_approved": artifact["public_claims_approved"],
            "blind_install_attempted": artifact["blind_install_attempted"],
            "required_adapter_count": artifact["required_adapter_count"],
            "reviewed_command_count": artifact["reviewed_command_count"],
            "missing_reviewed_command_count": artifact["missing_reviewed_command_count"],
            "duplicate_command_count": artifact["duplicate_command_count"],
            "unsafe_command_count": artifact["unsafe_command_count"]
        }),
    ))
}

fn validate_adapter_approval_rows(artifact: &serde_json::Value, reasons: &mut Vec<String>) {
    let Some(rows) = artifact["adapters"].as_array() else {
        push_unique_reason(
            reasons,
            "adapter approval artifact rows do not cover required adapters",
        );
        return;
    };
    for required in REQUIRED_COMPETITOR_ADAPTERS {
        let covered = rows.iter().any(|row| {
            row["tool"].as_str() == Some(*required)
                && row["approval_status"] == "reviewed"
                && row["reviewed_command"]
                    .as_str()
                    .is_some_and(|command| !command.trim().is_empty() && command != "null")
        });
        if !covered {
            push_unique_reason(
                reasons,
                "adapter approval artifact rows do not cover required adapters",
            );
            break;
        }
    }
}

fn missing_claim_gate(id: &str, reason: &str) -> serde_json::Value {
    claim_gate(id, None, vec![reason.to_string()], json!({"supplied": false}))
}

fn evaluate_recovery_claim_gate(artifact_path: Option<&PathBuf>) -> Result<serde_json::Value> {
    let Some(path) = artifact_path else {
        return Ok(missing_claim_gate(
            "recovery_artifact",
            "byte-perfect recovery proof not attached to public claim",
        ));
    };
    let artifact = load_json_artifact(path)?;
    let mut reasons = Vec::new();
    if artifact["ok"] != true {
        reasons.push("recovery artifact did not pass".to_string());
    }
    let normal_rows_recover = artifact["normal_rows"].as_array().is_some_and(|rows| {
        !rows.is_empty() && rows.iter().all(|row| row["all_refs_recover"] == true)
    });
    if !normal_rows_recover {
        reasons.push("byte-perfect recovery proof not attached to public claim".to_string());
    }
    Ok(claim_gate(
        "recovery_artifact",
        Some(path.as_path()),
        reasons,
        json!({
            "schema_version": artifact["schema_version"],
            "normal_row_count": artifact["normal_rows"].as_array().map_or(0, Vec::len)
        }),
    ))
}

fn evaluate_task_success_claim_gate(artifact_path: Option<&PathBuf>) -> Result<serde_json::Value> {
    let Some(path) = artifact_path else {
        return Ok(missing_claim_gate(
            "task_success_artifact",
            "task-success proof not attached to public claim",
        ));
    };
    let artifact = load_json_artifact(path)?;
    let mut reasons = Vec::new();
    if artifact["ok"] != true
        || artifact["critical_miss_rate"] != 0.0
        || artifact["rows"].as_array().is_none_or(|rows| {
            rows.is_empty() || !rows.iter().all(|row| row["task_success"] == true)
        })
    {
        reasons.push("task-success proof not attached to public claim".to_string());
    }
    Ok(claim_gate(
        "task_success_artifact",
        Some(path.as_path()),
        reasons,
        json!({
            "schema_version": artifact["schema_version"],
            "critical_miss_rate": artifact["critical_miss_rate"],
            "row_count": artifact["rows"].as_array().map_or(0, Vec::len)
        }),
    ))
}

fn evaluate_os_claim_gate(artifact_path: Option<&PathBuf>) -> Result<serde_json::Value> {
    let Some(path) = artifact_path else {
        return Ok(missing_claim_gate(
            "os_artifact",
            "OS artifact set not attached to public claim",
        ));
    };
    let artifact = load_json_artifact(path)?;
    let mut reasons = Vec::new();
    if artifact["public_os_claim_approved"] != true {
        reasons.push("OS artifact set not approved for public claim".to_string());
    }
    Ok(claim_gate(
        "os_artifact",
        Some(path.as_path()),
        reasons,
        json!({
            "schema_version": artifact["schema_version"],
            "all_release_oses_run": artifact["all_release_oses_run"],
            "public_os_claim_approved": artifact["public_os_claim_approved"]
        }),
    ))
}

#[cfg(test)]
mod tests;
