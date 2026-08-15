use serde_json::{Value, json};
use tokenzero_core::{Accounting, MCP_SCHEMA_VERSION, Mode, ToolResponse, count_tokens};

use crate::TokenZeroEngine;
use crate::catalog::ToolKind;
use crate::jsonrpc::JsonRpcErrorData;

macro_rules! gated_tool_entry {
    ($name:ident, $gate:ident) => {
        pub(crate) fn $name(
            engine: &TokenZeroEngine,
            name: &str,
            args: &Value,
            call_id: Option<String>,
        ) -> Result<Value, JsonRpcErrorData> {
            dispatch_gated_tool(
                engine,
                name,
                args,
                call_id,
                crate::surface_health::GateMode::$gate,
            )
        }
    };
}

gated_tool_entry!(call_tool, Strict);
// FastMCP registration already filters by surface, so call-time gating only checks health.
gated_tool_entry!(call_tool_fastmcp, HealthOnly);

fn dispatch_gated_tool(
    engine: &TokenZeroEngine,
    name: &str,
    args: &Value,
    call_id: Option<String>,
    mode: crate::surface_health::GateMode,
) -> Result<Value, JsonRpcErrorData> {
    let canonical = canonical_tool(name);
    let started = std::time::Instant::now();
    engine
        .surface_health()
        .gate_tools_call(engine.config.tool_surface, name, mode)
        .map_err(|refusal| match refusal {
            crate::surface_health::GateRefusal::UnknownTool => JsonRpcErrorData::unknown_tool(name),
            crate::surface_health::GateRefusal::Policy(message) => {
                JsonRpcErrorData::policy_refusal(name, message)
            }
        })?;
    let result = dispatch_tool(engine, canonical, name, args);
    let engine_elapsed = started.elapsed();
    let persist_started = std::time::Instant::now();
    engine.record_tool_call(canonical, engine_elapsed, result.is_err());
    let response = match result {
        Ok(response) => response,
        Err(error) => {
            engine.record_tool_attribution(canonical, engine_elapsed, persist_started.elapsed());
            return Err(error);
        }
    };
    // Expand health is recorded inside expand_with_params.
    record_mcp_pulse(engine, canonical, args, &response, call_id);
    record_opt_in_mcp_usage(engine, &response);
    engine.record_ledger_response(canonical, &response);
    engine.record_tool_attribution(canonical, engine_elapsed, persist_started.elapsed());
    let mut response = response;
    engine.apply_session_visible_ref_aliases(&mut response);
    Ok(mcp_tool_response(response))
}

/// Pulse-account every MCP `tools/call`, including `tz_expand`. Engine owns
/// the Pulse write (session_id attribution). Expand still forwards the
/// request ref so recovery can join the original serve. Best-effort.
fn record_mcp_pulse(
    engine: &TokenZeroEngine,
    canonical: &str,
    args: &Value,
    response: &ToolResponse,
    call_id: Option<String>,
) {
    let extra_ref_ids = if canonical == "expand" {
        args.get("ref")
            .and_then(Value::as_str)
            .map(|id| vec![id.to_string()])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    engine.record_tool_pulse(canonical, response, call_id, extra_ref_ids);
}

fn record_opt_in_mcp_usage(engine: &TokenZeroEngine, response: &ToolResponse) {
    let enabled = crate::usage_telemetry_enabled(engine.config.telemetry_enabled);
    if !enabled {
        return;
    }
    let Some(accounting) = response.accounting.as_ref() else {
        return;
    };
    crate::record_mcp_accounting(
        &engine.config.cache_path,
        enabled,
        accounting.raw_tokens,
        accounting.visible_tokens,
    );
}

fn json_tool_response(name: &str, value: Value) -> Result<ToolResponse, JsonRpcErrorData> {
    let text = serde_json::to_string(&value).map_err(|err| err.to_string())?;
    let tokens = count_tokens(&text);
    let mut response = inline_response(name, Mode::Structured, text, tokens);
    response
        .accounting
        .as_mut()
        .expect("inline accounting")
        .exact_ref_tokens = None;
    Ok(response)
}

fn exec_codemode_tool(
    _engine: &TokenZeroEngine,
    name: &str,
    _args: &Value,
) -> Result<ToolResponse, JsonRpcErrorData> {
    Err(JsonRpcErrorData::policy_refusal(
        name,
        "engine-local CodeMode execution was retired; submit plans to the ZeroStack aggregate host, which dispatches TokenZero bindings through raw-worker v2".to_string(),
    ))
}

fn exec_codemode_search_tool(name: &str, args: &Value) -> Result<ToolResponse, JsonRpcErrorData> {
    let query = arg_string_any(args, &["query"])?;
    let mut value = crate::search_codemode_catalog(query);
    if let Some(limit) = arg_u64(args, "limit") {
        if let Some(items) = value.as_array_mut() {
            items.truncate(limit.min(50));
        }
    }
    json_tool_response(name, value)
}

fn exec_codemode_describe_tool(name: &str, args: &Value) -> Result<ToolResponse, JsonRpcErrorData> {
    let target = arg_string_any(args, &["name"])?;
    let value = if target == "capabilities" {
        codemode_capabilities_manifest()
    } else {
        crate::describe_codemode_method(target)
    };
    json_tool_response(name, value)
}

fn codemode_capabilities_manifest() -> Value {
    json!({
        "contract_version": "1.0",
        "ns": "tz",
        "mutation": "denied",
        "plan_forms": ["recipe", "json", "js"],
        "limits": {
            "max_logical_ops": crate::CodeModeLimits::default().max_logical_ops,
            "max_microtasks": crate::CodeModeLimits::default().max_microtasks,
            "max_output_bytes": crate::CodeModeLimits::default().max_output_bytes,
            "max_code_bytes": crate::CodeModeLimits::default().max_code_bytes,
            "max_visible_tokens": crate::CodeModeLimits::default().max_visible_tokens
        }
    })
}

fn inline_response(tool: &str, mode: Mode, text: String, raw_tokens: usize) -> ToolResponse {
    let visible_tokens = count_tokens(&text);
    ToolResponse::ok(
        tool,
        mode,
        text,
        Vec::new(),
        Accounting {
            raw_tokens,
            visible_tokens,
            recovery_tokens: 0,
            billed_tokens: visible_tokens,
            cached_tokens: 0,
            exact_ref_tokens: Some(0),
        },
    )
}

/// Tool dispatch for classic compatibility calls.
///
/// Domain operations route through [`tokenzero_engine::dispatch_operation`].
/// Retired execute requests fail closed here; catalog search and describe stay
/// available as aggregate-binding metadata.
pub(crate) fn dispatch_tool(
    engine: &TokenZeroEngine,
    canonical: &str,
    name: &str,
    args: &Value,
) -> Result<ToolResponse, JsonRpcErrorData> {
    let kind = if canonical == "compact" {
        Some(ToolKind::Ingest)
    } else {
        ToolKind::from_canonical(canonical)
    }
    .ok_or_else(|| JsonRpcErrorData::unknown_tool(name))?;
    match kind {
        ToolKind::ExecuteCode => exec_codemode_tool(engine, name, args),
        ToolKind::CodemodeSearch => exec_codemode_search_tool(name, args),
        ToolKind::CodemodeDescribe => exec_codemode_describe_tool(name, args),
        _ => {
            let outcome = tokenzero_engine::dispatch_operation(
                engine,
                tokenzero_engine::DispatchSurface::Mcp,
                canonical,
                args,
            );
            if let Some(err) = outcome.domain_error {
                return Err(match err.kind {
                    tokenzero_core::operation_abi::DomainErrorKind::Validation
                        if err.message.starts_with("unknown tool:") =>
                    {
                        JsonRpcErrorData::unknown_tool(err.op.as_deref().unwrap_or("unknown"))
                    }
                    _ => JsonRpcErrorData::from(err.message),
                });
            }
            outcome.tool_response.ok_or_else(|| {
                JsonRpcErrorData::from("domain dispatch returned no tool response".to_string())
            })
        }
    }
}

pub(crate) fn mcp_tool_response(response: ToolResponse) -> Value {
    // F-MCP-006 (nt0i/1cwf): a shell child that ran but failed is a tool
    // error for MCP clients — isError mirrors telemetry.command_success so
    // agents do not treat failed commands as successful results.
    let child_failed = response.tool == "shell"
        && response
            .telemetry
            .as_ref()
            .and_then(|t| t.get("command_success"))
            .and_then(Value::as_bool)
            == Some(false);
    let is_error = response.status == "error" || child_failed;
    let mut text = response
        .visible
        .as_ref()
        .map(|v| v.text.clone())
        .or_else(|| response.error.as_ref().map(|e| e.message.clone()))
        .unwrap_or_default();
    if let Some(diagnostic) = &response.diagnostic {
        if !text.contains(diagnostic.code.as_str()) {
            text.push_str(&format!(
                "\ndiagnostic: {}: {}",
                diagnostic.code, diagnostic.message
            ));
        }
    }
    if let Some(footer) = refs_footer(&response, &text) {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&footer);
        // Steer edits at the moment of reading: a tz_read does not satisfy
        // harness read-before-Edit checks, and agents that learn that from
        // a failed Edit tend to retry it blindly. Tied to the refs footer so
        // tiny passthrough renders stay exactly as cheap as raw.
        if response.status != "error" && response.tool == "read" {
            text.push_str(
                "\nedit: tz_edit applies find/replace hunks directly; native Edit tools require a prior native bounded read",
            );
        }
    }
    if let Some(ack) = response.ack.as_deref() {
        if !ack.is_empty() && text.trim() != ack {
            if !text.is_empty() {
                text.push(char::from(10));
            }
            text.push_str(ack);
        }
    }
    let mut result = json!({
        "content": [{"type": "text", "text": text}],
        "resultType": "complete"
    });
    // yevj: the recovery receipt rides the wire unconditionally (three bools,
    // no envelope-mode gate) so adapters can honor do-not-recompact without
    // parsing text. The text block stays byte-exact.
    if let Some(recovery) = &response.recovery {
        result["recovery"] = json!({
            "terminal": recovery.terminal,
            "do_not_recompact": recovery.do_not_recompact,
            "exact_bytes": recovery.exact_bytes,
        });
    }
    if let Some(structured) = response
        .telemetry
        .as_ref()
        .and_then(|telemetry| telemetry.get("structuredContent"))
        .cloned()
    {
        result["structuredContent"] = structured;
    } else {
        // structuredContent diverging from the text block makes several MCP
        // clients render the JSON envelope instead of the tool text, and it
        // roughly doubles the per-call context cost. Default is text-only; the
        // envelope stays available for machine consumers via
        // TOKENZERO_MCP_ENVELOPE=compact|full.
        match envelope_mode() {
            EnvelopeMode::None => {}
            EnvelopeMode::Compact | EnvelopeMode::Full => {
                let cli = if matches!(envelope_mode(), EnvelopeMode::Full) {
                    serde_json::to_value(&response).expect("ToolResponse is serializable")
                } else {
                    compact_cli_envelope(&response)
                };
                result["structuredContent"] = json!({
                    "schema_version": MCP_SCHEMA_VERSION,
                    "status": response.status,
                    "tool": response.tool,
                    "cli": cli
                });
            }
        }
    }
    // Always stamp isError after structuredContent attachment so FastMCP never
    // treats a typed compatibility-tool failure as a successful result.
    if is_error {
        result["isError"] = Value::Bool(true);
    }
    result
}

/// One-line recovery footer: keeps every payload recoverable from the text
/// content alone. Primary blob/file refs are listed verbatim; the edit
/// pre-image is listed verbatim with an `undo:` label (it is a distinct
/// payload — without its id on the wire, text-only clients could not honor
/// the documented undo contract); other secondary per-match refs are
/// summarized by kind (their content stays recoverable through the listed
/// blob/file refs plus visible line numbers).
fn refs_footer(response: &ToolResponse, text: &str) -> Option<String> {
    if response.refs.is_empty()
        || tokenzero_engine::is_already_resident_text(text)
        || response
            .refs
            .iter()
            .any(|record| text.contains(&record.ref_id))
    {
        return None;
    }
    let primary = ["combined", "blob", "file", "undo"]
        .into_iter()
        .find_map(|kind| response.refs.iter().find(|record| record.kind == kind))
        .or_else(|| response.refs.first())?;
    let undo = response
        .refs
        .iter()
        .find(|record| record.kind == "undo" && record.ref_id != primary.ref_id);
    Some(match undo {
        Some(undo) => format!("refs: {} undo:{}", primary.ref_id, undo.ref_id),
        None => format!("refs: {}", primary.ref_id),
    })
}

enum EnvelopeMode {
    None,
    Compact,
    Full,
}

fn envelope_mode() -> EnvelopeMode {
    match std::env::var("TOKENZERO_MCP_ENVELOPE") {
        Ok(value) if value.eq_ignore_ascii_case("full") => EnvelopeMode::Full,
        Ok(value) if value.eq_ignore_ascii_case("compact") => EnvelopeMode::Compact,
        _ => EnvelopeMode::None,
    }
}

/// Heavy telemetry fields that duplicate the visible capsule, repeat refs
/// already carried elsewhere in the result, or only matter for offline
/// forensics (recoverable via `expand` and CLI `--json`). Pruned from the MCP
/// envelope so every tool call ships the minimum context cost by default.
const PRUNED_TELEMETRY_FIELDS: &[&str] = &[
    "alias_dependency",
    "allocator_pressure_relief",
    "argv",
    "capture_ref",
    "command",
    "cwd",
    "execution_mode",
    "family",
    "policy_reason",
    "raw_tokens",
    "recovery_tokens",
    "rewrite_applied",
    "rewrite_skip_reason",
    "shell_syntax_summary",
    "stderr_capture",
    "stderr_preview",
    "stdout_capture",
    "stdout_preview",
    "transport_status",
    "visible_tokens",
];

/// Compact `structuredContent.cli`: keeps status truth, accounting, refs, and
/// actionable telemetry; drops payload duplicates and forensic detail. The
/// full envelope remains available via `TOKENZERO_MCP_ENVELOPE=full` and the
/// CLI `--json` surface.
pub(crate) fn compact_cli_envelope(response: &ToolResponse) -> Value {
    let mut value = serde_json::to_value(response).expect("ToolResponse is serializable");
    if let Some(object) = value.as_object_mut() {
        // The capsule text already ships as content[0].text; repeating it
        // here doubles the cost of every call.
        object.remove("visible");
        let detail_is_in_refs = object
            .get("detail_ref")
            .and_then(Value::as_str)
            .is_some_and(|detail| {
                object
                    .get("refs")
                    .and_then(Value::as_array)
                    .is_some_and(|refs| {
                        refs.iter()
                            .any(|record| record.get("ref").and_then(Value::as_str) == Some(detail))
                    })
            });
        if detail_is_in_refs {
            object.remove("detail_ref");
        }
        if let Some(telemetry) = object.get_mut("telemetry").and_then(Value::as_object_mut) {
            for field in PRUNED_TELEMETRY_FIELDS {
                telemetry.remove(*field);
            }
            telemetry.retain(|_, field_value| !field_value.is_null());
        }
        if let Some(safety) = object.get_mut("safety").and_then(Value::as_object_mut) {
            safety.retain(|key, _| key == "secret_masking" || key == "refs_cover_full_output");
        }
    }
    value
}

fn canonical_tool(name: &str) -> &str {
    match name.strip_prefix("tz_").unwrap_or(name) {
        "cache-pack" => "cache_pack",
        "compact" => "compact",
        "report-tool-issue" => "report_tool_issue",
        other => other,
    }
}

fn arg_string_any<'a>(args: &'a Value, keys: &[&str]) -> Result<&'a str, String> {
    keys.iter()
        .find_map(|key| args.get(*key).and_then(Value::as_str))
        .ok_or_else(|| format!("missing {}", keys.join("|")))
}

fn arg_u64(args: &Value, key: &str) -> Option<usize> {
    let value = args.get(key)?;
    let n = match value {
        Value::Number(number) => number.as_u64()?,
        Value::String(text) => text.trim().parse::<u64>().ok()?,
        _ => return None,
    };
    usize::try_from(n).ok()
}

#[cfg(test)]
#[path = "../../../tests/mcp-compat/inline/tools__classic_mcp_response_tests.rs"]
mod classic_mcp_response_tests;
