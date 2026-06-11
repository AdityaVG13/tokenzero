use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::process::Command;

pub(crate) fn load_json_artifact(path: &Path) -> Result<serde_json::Value> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("read {}", path.display()))?)
        .with_context(|| format!("parse {}", path.display()))
}

pub(crate) fn release_candidate_id() -> String {
    if let Ok(value) = std::env::var("TOKENZERO_RELEASE_CANDIDATE_ID") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
    {
        if output.status.success() {
            let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !sha.is_empty() {
                let dirty = Command::new("git")
                    .args(["diff", "--quiet", "--ignore-submodules"])
                    .status()
                    .is_ok_and(|status| !status.success());
                return if dirty {
                    format!("git-{sha}-dirty")
                } else {
                    format!("git-{sha}")
                };
            }
        }
    }

    "local-unpinned".to_string()
}

pub(crate) fn json_artifact_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn increment_status_count(counts: &mut serde_json::Map<String, serde_json::Value>, status: &str) {
    let count = counts
        .get(status)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        + 1;
    counts.insert(status.to_string(), json!(count));
}

pub(crate) fn completion_claim_public_residual(claim_gate_snapshot: &serde_json::Value) -> String {
    if claim_gate_snapshot["public_claims_approved"] == true {
        return "public claim approval is true; final publication still follows release handoff gates"
            .to_string();
    }
    let reasons = claim_gate_snapshot["blocked_reasons"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|reason| reason.as_str().map(str::to_string))
        .filter(|reason| !reason.is_empty())
        .collect::<Vec<_>>();
    if reasons.is_empty() {
        return "public claim approval intentionally false until claim audit evidence gates pass"
            .to_string();
    }
    format!(
        "public claim approval intentionally false: {}",
        reasons.join("; ")
    )
}

pub(crate) fn completion_source_public_residual(path: &Path) -> String {
    let expected_release_candidate_id = release_candidate_id();
    let Ok(artifact) = load_json_artifact(path) else {
        return "source refresh required for public claims; source currency artifact missing or unreadable"
            .to_string();
    };

    if artifact["schema_version"] != "tokenzero.source_currency.v1" {
        return "source refresh required for public claims; source currency artifact schema invalid"
            .to_string();
    }

    let artifact_release_candidate_id = artifact["release_candidate_id"].as_str();
    let fresh_for_public_claim = artifact["fresh_for_public_claim"] == true;
    if fresh_for_public_claim
        && artifact_release_candidate_id == Some(expected_release_candidate_id.as_str())
    {
        return "source evidence is current; public claims still require benchmark, recovery, task-success, OS, adapter, and release approval gates"
            .to_string();
    }
    if fresh_for_public_claim {
        return "source evidence is fresh but release_candidate_id does not match this release candidate"
            .to_string();
    }

    let reasons = artifact["blocked_reasons"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|reason| reason.as_str().map(str::to_string))
        .filter(|reason| !reason.is_empty())
        .collect::<Vec<_>>();
    if reasons.is_empty() {
        "source refresh required for public claims".to_string()
    } else {
        format!(
            "source refresh required for public claims: {}",
            reasons.join("; ")
        )
    }
}

pub(crate) fn completion_requirement_status_summary(
    sections: &[&Vec<serde_json::Value>],
) -> (serde_json::Value, serde_json::Value, bool) {
    let mut counts = serde_json::Map::new();
    let mut blocked_ids = Vec::new();
    let mut all_rows_passed = true;

    for section in sections {
        for row in *section {
            let status = row["status"].as_str().unwrap_or("unknown");
            increment_status_count(&mut counts, status);

            if !matches!(status, "passed" | "passed_private") {
                all_rows_passed = false;
                blocked_ids.push(row["id"].as_str().unwrap_or("unknown").to_string());
            }
        }
    }

    (json!(counts), json!(blocked_ids), all_rows_passed)
}

pub(crate) fn residual_gate_status_summary(
    residual_gate_matrix: &serde_json::Value,
) -> (serde_json::Value, serde_json::Value, bool) {
    let mut counts = serde_json::Map::new();
    let mut blocked_ids = Vec::new();

    let Some(rows) = residual_gate_matrix.as_array() else {
        return (json!(counts), json!(blocked_ids), true);
    };

    for row in rows {
        let status = row["status"].as_str().unwrap_or("unknown");
        increment_status_count(&mut counts, status);

        if status == "blocked" {
            let gate_id = row["gate_id"]
                .as_str()
                .or_else(|| row["id"].as_str())
                .unwrap_or("unknown");
            blocked_ids.push(gate_id.to_string());
        }
    }
    blocked_ids.sort();

    let all_resolved = blocked_ids.is_empty()
        && rows.iter().all(|row| {
            row["status"]
                .as_str()
                .is_some_and(|status| status == "resolved")
        });

    (json!(counts), json!(blocked_ids), all_resolved)
}

pub(crate) fn completion_req_row(
    id: &str,
    status: &str,
    direct_evidence: &[&str],
    residual: &str,
) -> serde_json::Value {
    json!({
        "id": id,
        "status": status,
        "direct_evidence": direct_evidence,
        "residual": if residual.is_empty() { serde_json::Value::Null } else { json!(residual) }
    })
}

pub(crate) fn completion_evidence_integrity_matrix(
    g_goals: &[serde_json::Value],
    must_fr: &[serde_json::Value],
    critical_nfr: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for (section, section_rows) in [
        ("g_goals", g_goals),
        ("must_fr", must_fr),
        ("critical_nfr", critical_nfr),
    ] {
        for row in section_rows {
            let requirement_id = row["id"].as_str().unwrap_or("unknown");
            let requirement_status = row["status"].clone();
            let requirement_residual = row["residual"].clone();
            if let Some(evidence_items) = row["direct_evidence"].as_array() {
                for evidence in evidence_items.iter().filter_map(|item| item.as_str()) {
                    let evidence_kind = completion_evidence_kind(evidence);
                    let integrity = completion_evidence_artifact_integrity(evidence, evidence_kind);
                    rows.push(json!({
                        "section": section,
                        "requirement_id": requirement_id,
                        "evidence": evidence,
                        "evidence_kind": evidence_kind,
                        "present": integrity.present,
                        "status": integrity.status,
                        "schema_version": integrity.schema_version,
                        "expected_schema_version": integrity.expected_schema_version,
                        "schema_matches": integrity.schema_matches,
                        "release_candidate_id": integrity.release_candidate_id,
                        "expected_release_candidate_id": integrity.expected_release_candidate_id,
                        "release_candidate_matches": integrity.release_candidate_matches,
                        "expected_content_markers": integrity.expected_content_markers,
                        "content_markers_present": integrity.content_markers_present,
                        "artifact_valid": integrity.artifact_valid,
                        "reasons": integrity.reasons,
                        "requirement_status": requirement_status,
                        "requirement_residual": requirement_residual
                    }));
                }
            }
        }
    }
    rows
}

struct CompletionEvidenceIntegrity {
    present: serde_json::Value,
    status: &'static str,
    schema_version: serde_json::Value,
    expected_schema_version: Option<&'static str>,
    schema_matches: serde_json::Value,
    release_candidate_id: serde_json::Value,
    expected_release_candidate_id: Option<String>,
    release_candidate_matches: serde_json::Value,
    expected_content_markers: serde_json::Value,
    content_markers_present: serde_json::Value,
    artifact_valid: serde_json::Value,
    reasons: Vec<String>,
}

fn completion_evidence_artifact_integrity(
    evidence: &str,
    evidence_kind: &str,
) -> CompletionEvidenceIntegrity {
    if evidence_kind != "artifact" {
        return CompletionEvidenceIntegrity {
            present: serde_json::Value::Null,
            status: "command_evidence",
            schema_version: serde_json::Value::Null,
            expected_schema_version: None,
            schema_matches: serde_json::Value::Null,
            release_candidate_id: serde_json::Value::Null,
            expected_release_candidate_id: None,
            release_candidate_matches: serde_json::Value::Null,
            expected_content_markers: serde_json::Value::Null,
            content_markers_present: serde_json::Value::Null,
            artifact_valid: serde_json::Value::Null,
            reasons: Vec::new(),
        };
    }

    let path = Path::new(evidence);
    let expected_schema_version = completion_expected_evidence_schema_version(evidence);
    let expected_release_candidate_id = completion_expected_release_candidate_id(evidence);
    let expected_content_markers = completion_expected_evidence_content_markers(evidence);
    if !path.exists() {
        return CompletionEvidenceIntegrity {
            present: json!(false),
            status: "missing",
            schema_version: serde_json::Value::Null,
            expected_schema_version,
            schema_matches: if expected_schema_version.is_some() {
                json!(false)
            } else {
                serde_json::Value::Null
            },
            release_candidate_id: serde_json::Value::Null,
            expected_release_candidate_id,
            release_candidate_matches: serde_json::Value::Null,
            expected_content_markers: completion_content_marker_value(&expected_content_markers),
            content_markers_present: if expected_content_markers.is_empty() {
                serde_json::Value::Null
            } else {
                json!(false)
            },
            artifact_valid: json!(false),
            reasons: vec!["artifact missing".to_string()],
        };
    }

    let Some(expected) = expected_schema_version else {
        if !expected_content_markers.is_empty() {
            return completion_markdown_evidence_integrity(
                path,
                expected_schema_version,
                &expected_content_markers,
            );
        }
        return CompletionEvidenceIntegrity {
            present: json!(true),
            status: "present",
            schema_version: serde_json::Value::Null,
            expected_schema_version,
            schema_matches: serde_json::Value::Null,
            release_candidate_id: serde_json::Value::Null,
            expected_release_candidate_id,
            release_candidate_matches: serde_json::Value::Null,
            expected_content_markers: serde_json::Value::Null,
            content_markers_present: serde_json::Value::Null,
            artifact_valid: json!(true),
            reasons: Vec::new(),
        };
    };

    match load_json_artifact(path) {
        Ok(artifact) => {
            let schema_version = artifact["schema_version"].clone();
            let release_candidate_id = artifact["release_candidate_id"].clone();
            let mut reasons = Vec::new();
            let schema_matches = if schema_version.is_null() {
                reasons.push("schema_version missing".to_string());
                false
            } else {
                let matches = schema_version.as_str() == Some(expected);
                if !matches {
                    reasons.push("schema_version mismatch".to_string());
                }
                matches
            };
            let release_candidate_matches =
                if let Some(expected_rc) = expected_release_candidate_id.as_deref() {
                    if release_candidate_id.is_null() {
                        reasons.push("release_candidate_id missing".to_string());
                        Some(false)
                    } else {
                        let matches = release_candidate_id.as_str() == Some(expected_rc);
                        if !matches {
                            reasons.push("release_candidate_id mismatch".to_string());
                        }
                        Some(matches)
                    }
                } else {
                    None
                };
            let artifact_valid = schema_matches && release_candidate_matches.unwrap_or(true);
            CompletionEvidenceIntegrity {
                present: json!(true),
                status: if artifact_valid { "present" } else { "invalid" },
                schema_version,
                expected_schema_version,
                schema_matches: json!(schema_matches),
                release_candidate_id,
                expected_release_candidate_id,
                release_candidate_matches: release_candidate_matches
                    .map_or(serde_json::Value::Null, |matches| json!(matches)),
                expected_content_markers: serde_json::Value::Null,
                content_markers_present: serde_json::Value::Null,
                artifact_valid: json!(artifact_valid),
                reasons,
            }
        }
        Err(error) => CompletionEvidenceIntegrity {
            present: json!(true),
            status: "invalid",
            schema_version: serde_json::Value::Null,
            expected_schema_version,
            schema_matches: json!(false),
            release_candidate_id: serde_json::Value::Null,
            expected_release_candidate_id,
            release_candidate_matches: serde_json::Value::Null,
            expected_content_markers: serde_json::Value::Null,
            content_markers_present: serde_json::Value::Null,
            artifact_valid: json!(false),
            reasons: vec![error.to_string()],
        },
    }
}

fn completion_markdown_evidence_integrity(
    path: &Path,
    expected_schema_version: Option<&'static str>,
    expected_content_markers: &[&'static str],
) -> CompletionEvidenceIntegrity {
    match fs::read_to_string(path) {
        Ok(text) => {
            let reasons: Vec<String> = expected_content_markers
                .iter()
                .filter(|marker| !text.contains(**marker))
                .map(|marker| format!("content marker missing: {marker}"))
                .collect();
            let markers_present = reasons.is_empty();
            CompletionEvidenceIntegrity {
                present: json!(true),
                status: if markers_present {
                    "present"
                } else {
                    "invalid"
                },
                schema_version: serde_json::Value::Null,
                expected_schema_version,
                schema_matches: serde_json::Value::Null,
                release_candidate_id: serde_json::Value::Null,
                expected_release_candidate_id: None,
                release_candidate_matches: serde_json::Value::Null,
                expected_content_markers: completion_content_marker_value(expected_content_markers),
                content_markers_present: json!(markers_present),
                artifact_valid: json!(markers_present),
                reasons,
            }
        }
        Err(error) => CompletionEvidenceIntegrity {
            present: json!(true),
            status: "invalid",
            schema_version: serde_json::Value::Null,
            expected_schema_version,
            schema_matches: serde_json::Value::Null,
            release_candidate_id: serde_json::Value::Null,
            expected_release_candidate_id: None,
            release_candidate_matches: serde_json::Value::Null,
            expected_content_markers: completion_content_marker_value(expected_content_markers),
            content_markers_present: json!(false),
            artifact_valid: json!(false),
            reasons: vec![error.to_string()],
        },
    }
}

fn completion_content_marker_value(markers: &[&'static str]) -> serde_json::Value {
    if markers.is_empty() {
        serde_json::Value::Null
    } else {
        json!(markers)
    }
}

fn completion_expected_evidence_schema_version(evidence: &str) -> Option<&'static str> {
    match evidence {
        "results/current/rust_mcp_smoke.json" => Some("tokenzero.rust_mcp_churn.v1"),
        "results/current/tokenzero_adapter_approval_audit.json" => {
            Some("tokenzero.adapter_approval_audit.v1")
        }
        "results/current/tokenzero_artifact_handoff.json" => Some("tokenzero.artifact_handoff.v1"),
        "results/current/tokenzero_bench_competitors_shell_heavy.json" => {
            Some("tokenzero.bench.v1")
        }
        "results/current/tokenzero_claim_audit.json" => Some("tokenzero.claim_audit.v1"),
        "results/current/tokenzero_exact_recovery_audit.json" => {
            Some("tokenzero.exact_recovery_audit.v1")
        }
        "results/current/tokenzero_exact_recovery_shell.json" => {
            Some("tokenzero.exact_recovery_shell.v1")
        }
        "results/current/tokenzero_false_success_shell.json" => {
            Some("tokenzero.false_success_shell.v1")
        }
        "results/current/tokenzero_one_shot_eval.json" => Some("tokenzero.one_shot_eval.v1"),
        "results/current/tokenzero_os_reach_audit.json" => Some("tokenzero.os_reach_audit.v1"),
        "results/current/tokenzero_os_release_artifact.json" => {
            Some("tokenzero.os_release_artifact.v1")
        }
        "results/current/tokenzero_protected_anchor_audit.json" => {
            Some("tokenzero.protected_anchor_audit.v1")
        }
        "results/current/tokenzero_reach.json" => Some("tokenzero.reach.v1"),
        "results/current/tokenzero_security_privacy_audit.json" => {
            Some("tokenzero.security_privacy_audit.v1")
        }
        "results/current/tokenzero_shell_matrix.json" => Some("tokenzero.shell_matrix.v1"),
        "results/current/tokenzero_source_currency.json" => Some("tokenzero.source_currency.v1"),
        _ => None,
    }
}

fn completion_expected_release_candidate_id(evidence: &str) -> Option<String> {
    match evidence {
        "results/current/tokenzero_adapter_approval_audit.json"
        | "results/current/tokenzero_artifact_handoff.json"
        | "results/current/tokenzero_bench_competitors_shell_heavy.json"
        | "results/current/tokenzero_claim_audit.json"
        | "results/current/tokenzero_exact_recovery_audit.json"
        | "results/current/tokenzero_one_shot_eval.json"
        | "results/current/tokenzero_os_reach_audit.json"
        | "results/current/tokenzero_os_release_artifact.json"
        | "results/current/tokenzero_source_currency.json" => Some(release_candidate_id()),
        _ => None,
    }
}

fn completion_expected_evidence_content_markers(evidence: &str) -> Vec<&'static str> {
    match evidence {
        "docs/advanced-adr-execution-record.md" => {
            vec![
                "## ADR-",
                "Failure-first evidence:",
                "Residual gates:",
                "validate_prd_goal.py",
                "cargo test --workspace",
            ]
        }
        "results/current/tokenzero_competitive_superiority_reconciliation.md" => {
            vec!["Snapshot", "no gated action was performed"]
        }
        _ => Vec::new(),
    }
}

fn completion_evidence_kind(evidence: &str) -> &'static str {
    if evidence.starts_with("results/")
        || evidence.starts_with("docs/")
        || evidence.starts_with("crates/")
    {
        "artifact"
    } else {
        "command"
    }
}

pub(crate) fn handoff_verification_plan_status_summary(
    verification_plan_matrix: &serde_json::Value,
) -> (serde_json::Value, serde_json::Value, bool) {
    let mut counts = serde_json::Map::new();
    let mut blocked_ids = Vec::new();
    let mut all_rows_passed = true;
    let Some(rows) = verification_plan_matrix.as_array() else {
        return (json!({}), json!([]), false);
    };

    for row in rows {
        let status = row["status"].as_str().unwrap_or("unknown");
        increment_status_count(&mut counts, status);
        if !status.starts_with("passed") {
            all_rows_passed = false;
            blocked_ids.push(row["id"].as_str().unwrap_or("unknown").to_string());
        }
    }

    (json!(counts), json!(blocked_ids), all_rows_passed)
}

pub(crate) fn handoff_verification_evidence_integrity_matrix(
    verification_plan_matrix: &serde_json::Value,
    artifact_integrity_matrix: &serde_json::Value,
) -> (serde_json::Value, bool, bool) {
    let mut rows = Vec::new();
    let mut all_present = true;
    let mut all_valid = true;
    let artifact_rows = artifact_integrity_matrix
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    let Some(vp_rows) = verification_plan_matrix.as_array() else {
        return (json!([]), false, false);
    };

    for vp_row in vp_rows {
        let verification_id = vp_row["id"].as_str().unwrap_or("unknown");
        let Some(evidence_ids) = vp_row["evidence_artifact_ids"].as_array() else {
            continue;
        };
        for artifact_id in evidence_ids.iter().filter_map(|item| item.as_str()) {
            let artifact_row = artifact_rows
                .iter()
                .find(|row| row["id"].as_str() == Some(artifact_id));
            let (artifact_path, present, valid, status, reasons) =
                if let Some(artifact_row) = artifact_row {
                    let present = artifact_row["present"].as_bool().unwrap_or(false);
                    let valid = artifact_row["valid"].as_bool().unwrap_or(false);
                    let status = if present && valid {
                        "linked_valid"
                    } else if !present {
                        "missing"
                    } else {
                        "invalid"
                    };
                    (
                        artifact_row["path"].clone(),
                        present,
                        valid,
                        status,
                        artifact_row["reasons"].clone(),
                    )
                } else {
                    (
                        serde_json::Value::Null,
                        false,
                        false,
                        "unlinked",
                        json!(["artifact id not listed in handoff artifacts"]),
                    )
                };

            if !present {
                all_present = false;
            }
            if !valid {
                all_valid = false;
            }

            rows.push(json!({
                "verification_id": verification_id,
                "artifact_id": artifact_id,
                "artifact_path": artifact_path,
                "present": present,
                "valid": valid,
                "status": status,
                "reasons": reasons
            }));
        }
    }

    (json!(rows), all_present, all_valid)
}

pub(crate) fn handoff_artifact_integrity_matrix(
    artifacts: &[serde_json::Value],
) -> (serde_json::Value, bool, bool) {
    let mut rows = Vec::new();
    let mut all_required_present = true;
    let mut all_required_valid = true;

    for artifact in artifacts {
        let id = artifact["id"].as_str().unwrap_or_default();
        let path_text = artifact["path"].as_str().unwrap_or_default();
        let required = artifact["required_for_final_reconciliation"]
            .as_bool()
            .unwrap_or(false);
        let expected_schema_version = handoff_expected_schema_version(id);
        let expected_release_candidate_id = handoff_expected_release_candidate_id(id);
        let expected_content_markers = completion_expected_evidence_content_markers(path_text);
        let path = Path::new(path_text);
        let present = path.exists();
        let mut readable = false;
        let mut schema_version = serde_json::Value::Null;
        let mut schema_matches = serde_json::Value::Null;
        let mut release_candidate_id = serde_json::Value::Null;
        let mut release_candidate_matches = serde_json::Value::Null;
        let mut content_markers_present = if expected_content_markers.is_empty() {
            serde_json::Value::Null
        } else {
            json!(false)
        };
        let mut reasons = Vec::<String>::new();

        if !present {
            reasons.push("artifact missing".to_string());
        } else {
            match fs::read(path) {
                Ok(bytes) => {
                    readable = true;
                    if !expected_content_markers.is_empty() {
                        let text = String::from_utf8_lossy(&bytes);
                        let missing_markers: Vec<&str> = expected_content_markers
                            .iter()
                            .copied()
                            .filter(|marker| !text.contains(marker))
                            .collect();
                        content_markers_present = json!(missing_markers.is_empty());
                        for marker in missing_markers {
                            reasons.push(format!("content marker missing: {marker}"));
                        }
                    }
                    let should_parse_json = expected_schema_version.is_some()
                        || expected_release_candidate_id.is_some()
                        || path
                            .extension()
                            .and_then(|extension| extension.to_str())
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
                    if should_parse_json {
                        match serde_json::from_slice::<serde_json::Value>(&bytes) {
                            Ok(json_artifact) => {
                                schema_version = json_artifact["schema_version"].clone();
                                if schema_version.is_null() {
                                    reasons.push("schema_version missing".to_string());
                                    schema_matches = json!(false);
                                } else if let Some(expected) = expected_schema_version {
                                    let matches = schema_version.as_str() == Some(expected);
                                    schema_matches = json!(matches);
                                    if !matches {
                                        reasons.push("schema_version mismatch".to_string());
                                    }
                                } else {
                                    schema_matches = serde_json::Value::Null;
                                }
                                if let Some(expected) = expected_release_candidate_id.as_deref() {
                                    release_candidate_id =
                                        json_artifact["release_candidate_id"].clone();
                                    if release_candidate_id.is_null() {
                                        release_candidate_matches = json!(false);
                                        reasons.push("release_candidate_id missing".to_string());
                                    } else {
                                        let matches =
                                            release_candidate_id.as_str() == Some(expected);
                                        release_candidate_matches = json!(matches);
                                        if !matches {
                                            reasons
                                                .push("release_candidate_id mismatch".to_string());
                                        }
                                    }
                                }
                            }
                            Err(_) => {
                                reasons.push("artifact JSON unreadable".to_string());
                                if expected_schema_version.is_some() {
                                    schema_matches = json!(false);
                                }
                                if expected_release_candidate_id.is_some() {
                                    release_candidate_matches = json!(false);
                                }
                            }
                        }
                    }
                }
                Err(_) => reasons.push("artifact unreadable".to_string()),
            }
        }

        let valid = reasons.is_empty();
        if required && !present {
            all_required_present = false;
        }
        if required && !valid {
            all_required_valid = false;
        }

        rows.push(json!({
            "id": id,
            "path": path_text,
            "required_for_final_reconciliation": required,
            "present": present,
            "readable": readable,
            "schema_version": schema_version,
            "expected_schema_version": expected_schema_version,
            "schema_matches": schema_matches,
            "release_candidate_id": release_candidate_id,
            "expected_release_candidate_id": expected_release_candidate_id,
            "release_candidate_matches": release_candidate_matches,
            "expected_content_markers": completion_content_marker_value(&expected_content_markers),
            "content_markers_present": content_markers_present,
            "valid": valid,
            "reasons": reasons
        }));
    }

    (json!(rows), all_required_present, all_required_valid)
}

fn handoff_expected_schema_version(id: &str) -> Option<&'static str> {
    match id {
        "adapter_approval_audit" => Some("tokenzero.adapter_approval_audit.v1"),
        "adapter_approval_file" => Some("tokenzero.adapter_approval_file.v1"),
        "bench_competitors" => Some("tokenzero.bench.v1"),
        "claim_audit" => Some("tokenzero.claim_audit.v1"),
        "completion_audit" => Some("tokenzero.completion_audit.v1"),
        "exact_recovery" => Some("tokenzero.exact_recovery_audit.v1"),
        "exact_recovery_shell" => Some("tokenzero.exact_recovery_shell.v1"),
        "false_success_shell" => Some("tokenzero.false_success_shell.v1"),
        "mcp_smoke" => Some("tokenzero.rust_mcp_churn.v1"),
        "one_shot" | "task_success" => Some("tokenzero.one_shot_eval.v1"),
        "os_reach" => Some("tokenzero.os_reach_audit.v1"),
        "os_release_artifact" => Some("tokenzero.os_release_artifact.v1"),
        "reach" => Some("tokenzero.reach.v1"),
        "security_privacy_audit" => Some("tokenzero.security_privacy_audit.v1"),
        "shell_matrix" => Some("tokenzero.shell_matrix.v1"),
        "source_currency" => Some("tokenzero.source_currency.v1"),
        _ => None,
    }
}

fn handoff_expected_release_candidate_id(id: &str) -> Option<String> {
    match id {
        "adapter_approval_audit"
        | "bench_competitors"
        | "claim_audit"
        | "completion_audit"
        | "exact_recovery"
        | "one_shot"
        | "os_reach"
        | "os_release_artifact"
        | "source_currency"
        | "task_success" => Some(release_candidate_id()),
        _ => None,
    }
}

pub(crate) fn handoff_completion_audit_snapshot(path: &Path) -> serde_json::Value {
    let artifact = match load_json_artifact(path) {
        Ok(artifact) => artifact,
        Err(error) => {
            return json!({
                "present": false,
                "artifact_path": path.display().to_string(),
                "completion_achieved": false,
                "public_claims_approved": false,
                "release_publication_allowed": false,
                "all_direct_artifact_evidence_valid": false,
                "residual_gate_status_counts": {},
                "blocked_residual_gate_ids": [],
                "all_residual_gates_resolved": false,
                "residual_gate_matrix": [],
                "error": error.to_string()
            });
        }
    };

    json!({
        "present": true,
        "artifact_path": path.display().to_string(),
        "schema_version": artifact["schema_version"],
        "release_candidate_id": artifact["release_candidate_id"],
        "completion_achieved": artifact["completion_achieved"],
        "completion_status": artifact["completion_status"],
        "public_claims_approved": artifact["public_claims_approved"],
        "release_publication_allowed": artifact["release_publication_allowed"],
        "all_direct_file_evidence_present": artifact["all_direct_file_evidence_present"],
        "all_direct_artifact_evidence_valid": artifact["all_direct_artifact_evidence_valid"],
        "all_requirement_rows_passed": artifact["all_requirement_rows_passed"],
        "blocked_requirement_ids": artifact["blocked_requirement_ids"].as_array().cloned().unwrap_or_default(),
        "requirement_status_counts": artifact["requirement_status_counts"].as_object().cloned().unwrap_or_default(),
        "residual_gate_status_counts": artifact["residual_gate_status_counts"].as_object().cloned().unwrap_or_default(),
        "blocked_residual_gate_ids": artifact["blocked_residual_gate_ids"].as_array().cloned().unwrap_or_default(),
        "all_residual_gates_resolved": artifact["all_residual_gates_resolved"],
        "evidence_integrity_matrix": artifact["evidence_integrity_matrix"].as_array().cloned().unwrap_or_default(),
        "claim_gate_snapshot": artifact["claim_gate_snapshot"].as_object().cloned().unwrap_or_default(),
        "residual_gate_matrix": artifact["residual_gate_matrix"].as_array().cloned().unwrap_or_default()
    })
}

pub(crate) fn handoff_artifact(id: &str, path: &str, purpose: &str) -> serde_json::Value {
    json!({
        "id": id,
        "path": path,
        "purpose": purpose,
        "required_for_final_reconciliation": true
    })
}

#[cfg(test)]
#[test]
fn handoff_integrity_rejects_schema_bound_non_json_payload_without_json_extension() {
    let dir = tempfile::tempdir().unwrap();
    let artifact_path = dir.path().join("claim-audit.txt");
    fs::write(&artifact_path, "not json").unwrap();
    let artifact_path = artifact_path.to_string_lossy().into_owned();
    let artifacts = vec![handoff_artifact(
        "claim_audit",
        &artifact_path,
        "claim audit fixture",
    )];

    let (matrix, all_required_present, all_required_valid) =
        handoff_artifact_integrity_matrix(&artifacts);

    assert!(all_required_present);
    assert!(!all_required_valid);
    let rows = matrix.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["id"], "claim_audit");
    assert_eq!(row["present"], true);
    assert_eq!(row["readable"], true);
    assert_eq!(row["expected_schema_version"], "tokenzero.claim_audit.v1");
    assert_eq!(row["schema_matches"], false);
    assert_eq!(row["valid"], false);
    assert!(
        row["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "artifact JSON unreadable")
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn artifact_contracts_do_not_import_cli_monolith() {
        let source = include_str!("artifact_contracts.rs");
        let forbidden_imports = [
            format!("use {}::", "crate"),
            format!("{}::", "crate"),
            format!("use {}::", "super"),
            format!("{}::", "super"),
        ];
        for forbidden in forbidden_imports {
            assert!(
                !source.contains(&forbidden),
                "artifact_contracts.rs must stay independent of main.rs facade imports: {forbidden}"
            );
        }
    }
}
