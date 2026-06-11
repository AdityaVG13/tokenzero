use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

use crate::source_currency;

pub(crate) const REQUIRED_COMPETITOR_ADAPTERS: &[&str] = &[
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

pub(crate) fn load_benchmark_adapter_approval(
    path: Option<&PathBuf>,
) -> Result<Option<serde_json::Value>> {
    let Some(path) = path else {
        return Ok(None);
    };
    serde_json::from_slice::<serde_json::Value>(
        &fs::read(path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))
    .map(Some)
}

pub(crate) fn competitor_adapter_rows(
    suite: &str,
    adapter_approval: Option<&serde_json::Value>,
) -> Vec<serde_json::Value> {
    source_currency::competitor_adapter_sources()
        .map(|source| {
            let approval_row = adapter_approval
                .and_then(|artifact| artifact["adapters"].as_array())
                .and_then(|rows| rows.iter().find(|row| row["tool"] == source.tool));
            let approved_not_executed = adapter_approval.is_some_and(|artifact| {
                artifact["schema_version"] == "tokenzero.adapter_approval_audit.v1"
                    && artifact["execution_allowed"] == true
                    && artifact["public_claims_approved"] == true
                    && artifact["blind_install_attempted"] == false
                    && approval_row.is_some_and(|row| {
                        row["approval_status"] == "reviewed"
                            && row["execution_allowed"] == true
                            && row["blind_install_attempted"] == false
                            && row["reviewed_command"]
                                .as_str()
                                .is_some_and(|cmd| !cmd.is_empty())
                    })
            });
            json!({
                "schema_version": "tokenzero.bench.v1",
                "suite": suite,
                "scenario_id": format!("adapter_{}", source.tool),
                "tool": source.tool,
                "adapter_kind": "competitor",
                "adapter_allowlisted": true,
                "blind_install_attempted": false,
                "availability_status": if approved_not_executed { "approved_not_executed" } else { "unavailable" },
                "availability_reason": if approved_not_executed {
                    "adapter command reviewed and explicitly approved, but competitor execution is still not performed by this local proof command"
                } else {
                    "adapter command is approval-gated; no blind install or unreviewed competitor binary execution in this local proof"
                },
                "adapter_command": approval_row
                    .and_then(|row| row["reviewed_command"].as_str())
                    .map(|command| json!(command))
                    .unwrap_or(serde_json::Value::Null),
                "adapter_command_reviewed": approved_not_executed,
                "adapter_source_url": source.url,
                "adapter_source_commit": source.source_commit,
                "raw_tokens": 0,
                "visible_tokens": 0,
                "recovery_tokens": 0,
                "recovery_adjusted_savings": 0.0,
                "byte_perfect_recovery": false,
                "task_success": false,
                "harm_gate": "not_evaluated_unavailable",
                "harm_rate": 0.0,
                "latency_overhead_ms": 0,
                "host_coverage": ["cli"],
                "interception_depth": if approved_not_executed { "approved_adapter_not_executed" } else { "adapter_not_run" },
                "safe_savings": 0.0,
                "fairness_notes": if approved_not_executed {
                    format!("adapter row linked to reviewed approval artifact; {} command is not executed or blindly installed", source.tool)
                } else {
                    format!("adapter row accounted from source ledger; {} is unavailable rather than fabricated or blindly installed", source.tool)
                }
            })
        })
        .collect()
}

pub(crate) fn competitor_adapter_matrix(adapter_rows: &[serde_json::Value]) -> serde_json::Value {
    let accounted = adapter_rows
        .iter()
        .filter_map(|row| row["tool"].as_str())
        .collect::<Vec<_>>();
    let all_required_adapters_accounted = REQUIRED_COMPETITOR_ADAPTERS
        .iter()
        .all(|tool| accounted.iter().any(|row_tool| row_tool == tool));
    let runnable = adapter_rows
        .iter()
        .filter(|row| row["availability_status"] == "run")
        .count();
    let approved = adapter_rows
        .iter()
        .filter(|row| row["availability_status"] == "approved_not_executed")
        .count();
    let unavailable = adapter_rows
        .iter()
        .filter(|row| row["availability_status"] == "unavailable")
        .count();
    let blind_install_attempted = adapter_rows
        .iter()
        .any(|row| row["blind_install_attempted"] == true);
    json!({
        "schema_version": "tokenzero.adapter_matrix.v1",
        "status": if all_required_adapters_accounted && !blind_install_attempted { "ok" } else { "blocked" },
        "ok": all_required_adapters_accounted && !blind_install_attempted,
        "required_adapter_count": REQUIRED_COMPETITOR_ADAPTERS.len(),
        "adapter_total_rows": adapter_rows.len(),
        "runnable_adapter_count": runnable,
        "approved_adapter_count": approved,
        "unavailable_adapter_count": unavailable,
        "all_required_adapters_accounted": all_required_adapters_accounted,
        "blind_install_attempted": blind_install_attempted,
        "public_claims_approved": false,
        "release_publication_allowed": false,
        "blocked_reasons": [
            "competitor adapter execution requires reviewed commands and approval",
            "unavailable rows are evidence-accounting rows, not public superiority measurements"
        ]
    })
}

pub(crate) fn adapter_approval_audit_report(
    approval_file: Option<&Path>,
    execution_approval: bool,
    release_candidate_id: &str,
) -> Result<serde_json::Value> {
    let approval = match approval_file {
        Some(path) => Some(serde_json::from_slice::<serde_json::Value>(&fs::read(
            path,
        )?)?),
        None => None,
    };
    let approval_schema_valid = approval
        .as_ref()
        .is_some_and(|value| value["schema_version"] == "tokenzero.adapter_approval_file.v1");
    let approved_commands = approval
        .as_ref()
        .filter(|_| approval_schema_valid)
        .and_then(|value| value["commands"].as_array())
        .cloned()
        .unwrap_or_default();
    let mut reviewed_command_count = 0usize;
    let mut unsafe_command_count = 0usize;
    let adapters = REQUIRED_COMPETITOR_ADAPTERS
        .iter()
        .map(|tool| {
            let command = approved_commands
                .iter()
                .find(|row| row["tool"].as_str() == Some(*tool));
            let duplicate_command = approved_commands
                .iter()
                .filter(|row| row["tool"].as_str() == Some(*tool))
                .count()
                > 1;
            let (approval_status, reviewed_command, unsafe_reason) = match command {
                _ if duplicate_command => ("duplicate_command", None, None),
                Some(row) => {
                    let reviewed = row["reviewed"].as_bool().unwrap_or(false);
                    let command_text = row["command"].as_str().unwrap_or("").trim();
                    let unsafe_reason = adapter_command_unsafe_reason(command_text);
                    if reviewed && !command_text.is_empty() && unsafe_reason.is_none() {
                        reviewed_command_count += 1;
                        ("reviewed", Some(command_text.to_string()), None)
                    } else if unsafe_reason.is_some() {
                        unsafe_command_count += 1;
                        (
                            "unsafe_command",
                            Some(command_text.to_string()),
                            unsafe_reason,
                        )
                    } else {
                        ("missing_reviewed_command", None, None)
                    }
                }
                None => ("missing_reviewed_command", None, None),
            };
            let row_execution_allowed = execution_approval && approval_status == "reviewed";
            json!({
                "tool": tool,
                "url": source_currency::competitor_source_url(tool).unwrap_or(""),
                "approval_status": approval_status,
                "execution_allowed": row_execution_allowed,
                "reviewed_command": reviewed_command,
                "unsafe_reason": unsafe_reason,
                "blind_install_attempted": false,
                "sandbox_required": true,
                "approval_required": true
            })
        })
        .collect::<Vec<_>>();
    let missing_reviewed_command_count = adapters
        .iter()
        .filter(|row| row["approval_status"] == "missing_reviewed_command")
        .count();
    let duplicate_command_count = adapters
        .iter()
        .filter(|row| row["approval_status"] == "duplicate_command")
        .count();
    let mut blocked_reasons = Vec::new();
    if approval.is_some() && !approval_schema_valid {
        blocked_reasons.push("adapter approval file schema invalid".to_string());
    }
    if duplicate_command_count > 0 {
        blocked_reasons.push("duplicate adapter approval commands rejected".to_string());
    }
    if missing_reviewed_command_count > 0 {
        blocked_reasons.push("reviewed competitor commands missing".to_string());
    }
    if unsafe_command_count > 0 {
        blocked_reasons.push("unsafe reviewed competitor commands rejected".to_string());
    }
    if !execution_approval {
        blocked_reasons
            .push("explicit runnable adapter execution approval not granted".to_string());
    }
    let all_required_reviewed = approval_schema_valid
        && reviewed_command_count == REQUIRED_COMPETITOR_ADAPTERS.len()
        && missing_reviewed_command_count == 0
        && unsafe_command_count == 0
        && duplicate_command_count == 0;
    let execution_allowed = execution_approval && all_required_reviewed;
    let report = json!({
        "schema_version": "tokenzero.adapter_approval_audit.v1",
        "status": "ok",
        "ok": true,
        "release_candidate_id": release_candidate_id,
        "execution_approval_granted": execution_approval,
        "execution_allowed": execution_allowed,
        "public_claims_approved": execution_allowed,
        "release_publication_allowed": false,
        "blind_install_attempted": false,
        "approval_file_schema_valid": approval_schema_valid,
        "approval_file": approval_file.map(|path| path.display().to_string()),
        "required_adapter_count": REQUIRED_COMPETITOR_ADAPTERS.len(),
        "reviewed_command_count": reviewed_command_count,
        "unsafe_command_count": unsafe_command_count,
        "duplicate_command_count": duplicate_command_count,
        "missing_reviewed_command_count": missing_reviewed_command_count,
        "command_safety_policy": adapter_command_safety_policy(),
        "adapters": adapters,
        "blocked_reasons": blocked_reasons
    });
    Ok(report)
}

pub(crate) fn adapter_approval_template_report(release_candidate_id: &str) -> serde_json::Value {
    let commands = REQUIRED_COMPETITOR_ADAPTERS
        .iter()
        .map(|tool| {
            json!({
                "tool": tool,
                "url": source_currency::competitor_source_url(tool).unwrap_or(""),
                "reviewed": true,
                "command": format!("{tool} --version"),
                "review_scope": "command-shape safety review only; does not approve execution",
                "execution_approval_required": true
            })
        })
        .collect::<Vec<_>>();
    json!({
        "schema_version": "tokenzero.adapter_approval_file.v1",
        "status": "ok",
        "ok": true,
        "release_candidate_id": release_candidate_id,
        "generated_by": "tokenzero adapter-approval-template",
        "review_scope": "command-shape safety review only; explicit execution approval remains required",
        "execution_approval_required": true,
        "public_claims_approved": false,
        "required_adapter_count": REQUIRED_COMPETITOR_ADAPTERS.len(),
        "command_count": commands.len(),
        "command_safety_policy": adapter_command_safety_policy(),
        "commands": commands
    })
}

fn adapter_command_safety_policy() -> serde_json::Value {
    json!({
        "schema_version": "tokenzero.adapter_command_safety.v1",
        "blocked_side_effects": [
            "package_manager_install",
            "external_fetch_or_container_execution",
            "curl_pipe_execution",
            "powershell_web_pipe_execution",
            "destructive_filesystem_command"
        ],
        "execution_policy": "reviewed commands may be linked as approved_not_executed evidence; commands with install/fetch/container/destructive side effects remain blocked before any runnable execution phase"
    })
}

fn adapter_command_unsafe_reason(command: &str) -> Option<&'static str> {
    let lower = command.to_ascii_lowercase();
    if contains_package_install_side_effect(&lower) {
        return Some("package manager install command is not reviewed-safe");
    }
    if lower.contains("git clone") || lower.contains("docker run") || lower.contains("docker pull")
    {
        return Some("external fetch or container execution command is not reviewed-safe");
    }
    if lower.contains("curl") && lower.contains('|') {
        return Some("curl pipe execution is not reviewed-safe");
    }
    if lower.contains("irm ") && lower.contains('|') {
        return Some("PowerShell web pipe execution is not reviewed-safe");
    }
    if lower.contains("iwr ") && lower.contains('|') {
        return Some("PowerShell web pipe execution is not reviewed-safe");
    }
    if lower.contains("rm -rf") || lower.contains("remove-item") {
        return Some("destructive filesystem command is not reviewed-safe");
    }
    None
}

fn contains_package_install_side_effect(command: &str) -> bool {
    [
        "npm install",
        "npm i ",
        "pnpm add",
        "pnpm install",
        "pnpm dlx",
        "yarn add",
        "yarn install",
        "cargo install",
        "go install",
        "gem install",
        "pip install",
        "pip3 install",
        "pipx install",
        "uv tool install",
        "uv pip install",
        "brew install",
        "choco install",
        "winget install",
        "scoop install",
        "apt install",
        "apt-get install",
    ]
    .iter()
    .any(|needle| command.contains(needle))
}
