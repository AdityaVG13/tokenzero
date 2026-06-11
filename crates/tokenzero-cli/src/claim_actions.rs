use crate::artifact_contracts::load_json_artifact;
use serde_json::json;
use std::path::Path;

pub(crate) fn completion_claim_gate_snapshot(path: &Path) -> serde_json::Value {
    let artifact = match load_json_artifact(path) {
        Ok(artifact) => artifact,
        Err(error) => {
            return json!({
                "present": false,
                "artifact_path": path.display().to_string(),
                "public_claims_approved": false,
                "gate_passes": {},
                "blocked_reasons": ["claim audit artifact missing or unreadable"],
                "release_candidate_ids": [],
                "release_candidate_artifacts": [],
                "error": error.to_string()
            });
        }
    };

    let mut gate_passes = artifact["gate_passes"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let mut gate_reasons = artifact["gate_reasons"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let release_candidate_ids_present = !artifact["release_candidate_ids"].is_null();
    let release_candidate_artifacts_present = !artifact["release_candidate_artifacts"].is_null();
    let mut release_candidate_ids = artifact["release_candidate_ids"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut release_candidate_artifacts = artifact["release_candidate_artifacts"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if let Some(gates) = artifact["evidence_gates"].as_array() {
        for gate in gates {
            if let Some(id) = gate["id"].as_str() {
                if !gate_passes.contains_key(id) {
                    gate_passes.insert(id.to_string(), gate["pass"].clone());
                }
                if !gate_reasons.contains_key(id) {
                    gate_reasons.insert(
                        id.to_string(),
                        gate["reasons"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .into(),
                    );
                }
                if id == "release_candidate" {
                    if !release_candidate_ids_present {
                        release_candidate_ids = gate["details"]["release_candidate_ids"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();
                    }
                    if !release_candidate_artifacts_present {
                        release_candidate_artifacts = gate["details"]["artifacts"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();
                    }
                }
            }
        }
    }

    json!({
        "present": true,
        "artifact_path": path.display().to_string(),
        "schema_version": artifact["schema_version"],
        "release_candidate_id": artifact["release_candidate_id"],
        "public_claims_approved": artifact["public_claims_approved"],
        "gate_passes": gate_passes,
        "gate_reasons": gate_reasons,
        "blocked_reasons": artifact["blocked_reasons"].as_array().cloned().unwrap_or_default(),
        "release_candidate_ids": release_candidate_ids,
        "release_candidate_artifacts": release_candidate_artifacts
    })
}

pub(crate) fn completion_residual_gate_matrix(
    claim_gate_snapshot: &serde_json::Value,
) -> serde_json::Value {
    let Some(gate_passes) = claim_gate_snapshot["gate_passes"].as_object() else {
        return json!([]);
    };
    let mut rows = Vec::new();
    for (gate_id, pass) in gate_passes {
        if pass == &serde_json::Value::Bool(true) {
            continue;
        }
        let reasons = claim_gate_snapshot["gate_reasons"][gate_id]
            .as_array()
            .cloned()
            .unwrap_or_else(|| {
                claim_gate_snapshot["blocked_reasons"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            });
        let (next_action_id, owner, stop_before) = completion_gate_next_action(gate_id, &reasons);
        rows.push(json!({
            "gate_id": gate_id,
            "status": "blocked",
            "blocked_reasons": reasons,
            "next_action_id": next_action_id,
            "next_action": artifact_loop_next_action(next_action_id),
            "owner": owner,
            "stop_before": stop_before
        }));
    }
    json!(rows)
}

fn completion_gate_next_action(
    gate_id: &str,
    reasons: &[serde_json::Value],
) -> (&'static str, &'static str, Vec<&'static str>) {
    match gate_id {
        "source_currency" => (
            "source_currency_refresh",
            "product/release",
            vec!["publication", "public benchmark claim"],
        ),
        "benchmark_artifact"
            if reason_values_contain(
                reasons,
                "benchmark competitor rows must be runnable for public claims",
            ) =>
        {
            (
                "runnable_adapter_approval",
                "bench/release",
                vec!["competitor execution", "public benchmark claim"],
            )
        }
        "benchmark_artifact" => (
            "benchmark_publication_approval",
            "product/release",
            vec!["publication", "public benchmark claim"],
        ),
        "adapter_approval" => (
            "runnable_adapter_approval",
            "bench/release",
            vec!["competitor execution", "public benchmark claim"],
        ),
        "os_artifact" => (
            "os_matrix_expansion",
            "release/verification",
            vec!["OS-agnostic public claim", "publication"],
        ),
        "release_approval" => (
            "final_false_closure_audit",
            "implementer",
            vec!["release", "publication", "global install apply"],
        ),
        _ => (
            "final_false_closure_audit",
            "implementer",
            vec!["release", "publication"],
        ),
    }
}

fn reason_values_contain(reasons: &[serde_json::Value], needle: &str) -> bool {
    reasons
        .iter()
        .any(|reason| reason.as_str().is_some_and(|reason| reason == needle))
}

pub(crate) fn artifact_loop_next_actions(
    residual_gate_matrix: &serde_json::Value,
) -> Vec<serde_json::Value> {
    let mut action_ids = Vec::<String>::new();
    if let Some(rows) = residual_gate_matrix.as_array() {
        for row in rows {
            if let Some(action_id) = row["next_action_id"].as_str() {
                if !action_ids.iter().any(|existing| existing == action_id) {
                    action_ids.push(action_id.to_string());
                }
            }
        }
    }
    if !action_ids
        .iter()
        .any(|existing| existing == "final_false_closure_audit")
    {
        action_ids.push("final_false_closure_audit".to_string());
    }
    action_ids
        .iter()
        .map(|action_id| artifact_loop_next_action(action_id))
        .filter(|action| !action.is_null())
        .collect()
}

fn artifact_loop_next_action(action_id: &str) -> serde_json::Value {
    match action_id {
        "os_matrix_expansion" => os_matrix_expansion_next_action(),
        "source_currency_refresh" => json!({
            "id": "source_currency_refresh",
            "owner": "product/release",
            "action": "refresh primary source pages and pin release-candidate IDs across source, benchmark, recovery, task-success, OS, and adapter approval artifacts before public claims",
            "validation": "tokenzero source-currency-audit --json and tokenzero claim-audit --source-artifact <source.json> --benchmark-artifact <bench.json> --adapter-approval-artifact <adapter.json> --recovery-artifact <recovery.json> --task-success-artifact <task.json> --os-artifact <os.json> --json",
            "stop_condition": "do not publish savings/superiority claims while fresh_for_public_claim is false or release_candidate gate fails"
        }),
        "runnable_adapter_approval" => json!({
            "id": "runnable_adapter_approval",
            "owner": "bench/release",
            "action": "approve reviewed competitor commands, link them into the benchmark as approved_not_executed evidence, and only then decide whether an explicitly approved execution phase is warranted",
            "validation": "tokenzero adapter-approval-audit --approval-file <reviewed.json> --execution-approval --json, then tokenzero bench competitors --adapter-approval-artifact <adapter-approval.json> --json and inspect approved_not_executed rows before any runnable execution",
            "stop_condition": "no blind install, no unreviewed competitor binary execution, and no public benchmark claim from approved_not_executed rows"
        }),
        "benchmark_publication_approval" => json!({
            "id": "benchmark_publication_approval",
            "owner": "product/release",
            "action": "approve benchmark publication only after source, adapter, recovery, task-success, and OS evidence gates agree",
            "validation": "tokenzero claim-audit --benchmark-artifact <bench.json> --adapter-approval-artifact <adapter.json> --source-artifact <source.json> --recovery-artifact <recovery.json> --task-success-artifact <task.json> --os-artifact <os.json> --json",
            "stop_condition": "do not publish benchmark superiority claims until claim-audit reports public_claims_approved=true and release approval is explicit"
        }),
        "final_false_closure_audit" => json!({
            "id": "final_false_closure_audit",
            "owner": "implementer",
            "action": "rerun completion audit and reconcile every residual gate before claiming completion",
            "validation": "tokenzero completion-audit --json",
            "stop_condition": "completion_achieved must remain false until every required evidence row is direct and current"
        }),
        _ => serde_json::Value::Null,
    }
}

fn os_matrix_expansion_next_action() -> serde_json::Value {
    let missing =
        missing_release_os_rows(Path::new("results/current/tokenzero_os_reach_audit.json"));
    let missing_display = if missing.is_empty() {
        "Windows, Linux, and macOS".to_string()
    } else {
        join_human_list(
            &missing
                .iter()
                .map(|os| release_os_display_name(os).to_string())
                .collect::<Vec<_>>(),
        )
    };
    let artifact_args = if missing.is_empty() {
        "--os-artifact <windows.json> --os-artifact <linux.json> --os-artifact <macos.json>"
            .to_string()
    } else {
        missing
            .iter()
            .map(|os| format!("--os-artifact <{os}.json>"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    json!({
        "id": "os_matrix_expansion",
        "owner": "release/verification",
        "action": format!("run os-release-artifact on {missing_display}, then rerun OS reach audit with those artifacts"),
        "validation": format!("tokenzero os-release-artifact --json on {missing_display}, then tokenzero os-reach-audit {artifact_args} --json with Windows/Linux/macOS release-candidate rows"),
        "missing_release_oses": missing,
        "stop_condition": "do not claim OS-agnostic until all release OS rows pass"
    })
}

pub(crate) fn missing_release_os_rows(path: &Path) -> Vec<String> {
    let Ok(artifact) = load_json_artifact(path) else {
        return ["windows", "linux", "macos"]
            .iter()
            .map(|os| os.to_string())
            .collect();
    };
    let rows = artifact["os_rows"].as_array().cloned().unwrap_or_default();
    ["windows", "linux", "macos"]
        .iter()
        .filter(|os| {
            !rows
                .iter()
                .any(|row| row["os"].as_str() == Some(**os) && row["claim_ready"] == true)
        })
        .map(|os| os.to_string())
        .collect()
}

fn release_os_display_name(os: &str) -> &str {
    match os {
        "windows" => "Windows",
        "linux" => "Linux",
        "macos" => "macOS",
        _ => os,
    }
}

pub(crate) fn os_matrix_residual_message(missing: &[String]) -> String {
    if missing.is_empty() {
        return "all release OS artifacts are present; public OS claim still requires release approval"
            .to_string();
    }
    let missing_display = join_human_list(
        &missing
            .iter()
            .map(|os| release_os_display_name(os).to_string())
            .collect::<Vec<_>>(),
    );
    format!("{missing_display} shell and install artifacts missing")
}

pub(crate) fn os_reach_artifact_purpose(missing: &[String]) -> String {
    if missing.is_empty() {
        return "Windows, Linux, and macOS OS reach proof with no missing release OS rows"
            .to_string();
    }
    format!(
        "OS reach evidence with {} release claim still blocked",
        release_os_list_display(missing)
    )
}

pub(crate) fn os_release_artifact_purpose(missing: &[String]) -> String {
    if missing.is_empty() {
        return "Release artifact schema for completed Windows, Linux, and macOS OS matrix runs"
            .to_string();
    }
    format!(
        "Current release artifact schema; next OS release artifact needed for {}",
        release_os_list_display(missing)
    )
}

pub(crate) fn release_os_list_display(oses: &[String]) -> String {
    join_human_list(
        &oses
            .iter()
            .map(|os| release_os_display_name(os).to_string())
            .collect::<Vec<_>>(),
    )
}

fn join_human_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        _ => {
            let mut joined = items[..items.len() - 1].join(", ");
            joined.push_str(", and ");
            joined.push_str(&items[items.len() - 1]);
            joined
        }
    }
}

pub(crate) fn handoff_resolve_residual_next_actions(
    residual_gate_matrix: &serde_json::Value,
    next_actions: &[serde_json::Value],
) -> serde_json::Value {
    let Some(rows) = residual_gate_matrix.as_array() else {
        return json!([]);
    };

    let enriched_rows = rows
        .iter()
        .map(|row| {
            let mut object = row.as_object().cloned().unwrap_or_default();
            let next_action = object
                .get("next_action_id")
                .and_then(serde_json::Value::as_str)
                .and_then(|id| {
                    next_actions
                        .iter()
                        .find(|action| action["id"].as_str() == Some(id))
                })
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            object.insert("next_action".to_string(), next_action);
            serde_json::Value::Object(object)
        })
        .collect::<Vec<_>>();

    json!(enriched_rows)
}

#[cfg(test)]
mod tests {
    #[test]
    fn claim_actions_do_not_import_cli_monolith() {
        let source = include_str!("claim_actions.rs");
        let forbidden_imports = [
            format!("use {}::", "super"),
            format!("{}::", "super"),
            format!("use crate::{}", "main"),
            format!("crate::{}::", "main"),
        ];
        for forbidden in forbidden_imports {
            assert!(
                !source.contains(&forbidden),
                "claim_actions.rs must not back-import the CLI monolith: {forbidden}"
            );
        }
    }
}
