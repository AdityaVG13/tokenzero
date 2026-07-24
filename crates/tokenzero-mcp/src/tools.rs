use serde_json::{Value, json};
use std::path::Path;
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
    // Expand health is recorded inside expand_with_params (CLI + CodeMode + MCP).
    record_mcp_pulse(engine, canonical, args, &response, call_id);
    // Opt-in usage telemetry: MCP tools only. CodeMode execute records separately.
    if canonical != "execute_code" {
        record_opt_in_mcp_usage(engine, canonical, args, &response);
    }
    engine.record_ledger_response(canonical, &response);
    engine.record_tool_attribution(canonical, engine_elapsed, persist_started.elapsed());
    let mut response = response;
    // CodeMode results may be replayed by upstream execution caches after
    // session aliases are pruned. Keep their content-addressed refs canonical;
    // classic one-shot MCP tools may still use compact session aliases.
    if canonical != "execute_code" {
        engine.apply_session_visible_ref_aliases(&mut response);
    }
    Ok(mcp_tool_response(response))
}

/// A routed execution root may rebase onto an independently recognizable workspace (wqw.5).
fn has_workspace_evidence(path: &Path) -> bool {
    const MARKERS: &[&str] = &[
        ".git",
        ".zerostack",
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "go.mod",
        "CHANGELOG.md",
    ];
    path.is_dir() && MARKERS.iter().any(|marker| path.join(marker).exists())
}

/// Reject MCP-supplied roots that escape the server's configured allowlist.
fn ensure_path_under_server_allowlist(
    engine: &TokenZeroEngine,
    path: &Path,
) -> Result<(), JsonRpcErrorData> {
    if engine.path_allowed(path) {
        return Ok(());
    }
    Err(JsonRpcErrorData::policy_refusal(
        "execute_code",
        format!(
            "path_not_allowed: {} is outside the MCP server allowed roots",
            path.display()
        ),
    ))
}

/// Pulse-account every MCP `tools/call`, including `tz_expand`. Without this
/// the MCP surface — the main integration surface — wrote no Pulse events,
/// so expand-time recovery was never charged back to the original serve and
/// "savings after recovery" did not hold for agent-routed usage. Session,
/// call, and ref ids make that attribution joinable. Best-effort: accounting
/// must never fail the call.
fn record_mcp_pulse(
    engine: &TokenZeroEngine,
    canonical: &str,
    args: &Value,
    response: &ToolResponse,
    call_id: Option<String>,
) {
    let (Some(root), Some(accounting)) = (
        engine.config.allowed_roots.first(),
        response.accounting.as_ref(),
    ) else {
        return;
    };
    let mut ref_ids: Vec<String> = response
        .refs
        .iter()
        .map(|record| record.ref_id.clone())
        .collect();
    if canonical == "expand"
        && let Some(ref_id) = args.get("ref").and_then(Value::as_str)
    {
        ref_ids.push(ref_id.to_string());
    }
    let mut event = tokenzero_pulse::PulseEvent::tool_call(
        canonical,
        response.mode.as_deref().unwrap_or("hybrid"),
        accounting.raw_tokens,
        accounting.visible_tokens,
        accounting.recovery_tokens,
        response.refs.len(),
        0,
        None,
    )
    .with_attribution(Some(engine.session_id().to_string()), call_id, ref_ids);
    event.failure = response.error.is_some();
    let _ = tokenzero_pulse::record_event(&tokenzero_pulse::default_ledger_path(root), &event);
}

fn record_opt_in_mcp_usage(
    engine: &TokenZeroEngine,
    operation: &str,
    args: &Value,
    response: &ToolResponse,
) {
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
    let input_tokens = count_tokens(&serde_json::to_string(args).unwrap_or_default());
    crate::record_operation_amplification(
        &engine.config.cache_path,
        enabled,
        crate::ExecutionPath::Mcp,
        operation,
        crate::DirectionTokens::measured(input_tokens, input_tokens, input_tokens, 0),
        crate::DirectionTokens::measured(
            accounting.raw_tokens,
            accounting.visible_tokens,
            accounting.billed_tokens,
            accounting.cached_tokens.min(accounting.billed_tokens),
        ),
        response.refs.len(),
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
    engine: &TokenZeroEngine,
    name: &str,
    args: &Value,
) -> Result<ToolResponse, JsonRpcErrorData> {
    let plan = arg_string_any(args, &["plan"])?;
    let mut options = crate::CodeModeOptions {
        allowed_roots: engine.config.allowed_roots.clone(),
        cache_path: Some(engine.config.cache_path.clone()),
        max_visible_tokens: engine.config.max_visible_tokens,
        // Share session health so plan expand outcomes unlock recovery here.
        surface_health: Some(engine.surface_health_handle()),
        telemetry_enabled: engine.config.telemetry_enabled,
        ..Default::default()
    };
    // wqw.5: the per-call root defines the execution workspace boundary, which
    // CodeMode unions with the configured roots. Per-operation policy still
    // denies paths outside every effective root.
    if let Ok(root) = arg_string_any(args, &["root", "cwd", "workspace"]) {
        let root_path = std::path::PathBuf::from(root);
        if !engine.path_allowed(&root_path) && !has_workspace_evidence(&root_path) {
            ensure_path_under_server_allowlist(engine, &root_path)?;
        }
        options.root = Some(root_path);
    } else if let Some(root) = engine.config.allowed_roots.first() {
        options.root = Some(root.clone());
    }
    if let Some(extra) = args
        .get("allowed_root")
        .or_else(|| args.get("allowed_roots"))
    {
        match extra {
            Value::String(path) => {
                let path = std::path::PathBuf::from(path);
                ensure_path_under_server_allowlist(engine, &path)?;
                if !options.allowed_roots.iter().any(|r| r == &path) {
                    options.allowed_roots.push(path);
                }
            }
            Value::Array(items) => {
                for item in items {
                    if let Some(path) = item.as_str() {
                        let path = std::path::PathBuf::from(path);
                        ensure_path_under_server_allowlist(engine, &path)?;
                        if !options.allowed_roots.iter().any(|r| r == &path) {
                            options.allowed_roots.push(path);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(limits) = args.get("limits") {
        if let Ok(limits) = serde_json::from_value::<crate::CodeModeLimits>(limits.clone()) {
            // MCP callers may shorten the wall clock, but the server-owned
            // hard ceiling is not caller-expandable.
            let server_hard_max_wall_ms = options.hard_max_wall_ms;
            options.max_output_bytes = limits.max_output_bytes;
            options.max_refs_emitted = limits.max_refs_emitted;
            options.max_logical_ops = limits.max_logical_ops;
            options.max_physical_ops = limits.max_physical_ops;
            options.max_microtasks = limits.max_microtasks;
            options.max_memory_bytes = limits.max_memory_bytes;
            options.max_code_bytes = limits.max_code_bytes;
            options.max_visible_tokens = limits.max_visible_tokens;
            options.hard_max_wall_ms = limits.hard_max_wall_ms.min(server_hard_max_wall_ms);
            options.max_wall_ms = limits.max_wall_ms.min(options.hard_max_wall_ms);
        }
    }
    if let Some(envelope) = args.get("envelope").and_then(Value::as_str) {
        options.envelope = Some(envelope.to_string());
    }
    if let Some(ref_first) = args.get("ref_first").and_then(Value::as_bool) {
        options.ref_first = ref_first;
    }
    if let Some(budget) = args.get("ref_first_budget").and_then(Value::as_u64) {
        options.ref_first_budget = budget as usize;
    }
    let result = crate::execute_codemode_with_options(plan, options.clone());
    // wqw.9: expand outcomes are recorded on the shared SurfaceHealth inside
    // expand_with_params. Only substrate_down (no expand call) needs a bridge.
    if matches!(result.status, crate::CodeModeStatus::Error) {
        let kind = result.error.as_ref().map(|e| e.kind.as_str()).unwrap_or("");
        if kind == "substrate_down" {
            engine.surface_health().record_substrate_down();
        }
    }
    match codemode_envelope_version(args, &options).as_str() {
        "v1" => json_tool_response(name, codemode_contract_payload_v1(&result)),
        "v2" => codemode_v2_tool_response(name, &result),
        _ => codemode_v3_tool_response(name, &result),
    }
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

fn codemode_envelope_version(args: &Value, options: &crate::CodeModeOptions) -> String {
    if let Some(value) = options
        .envelope
        .as_deref()
        .or_else(|| args.get("envelope").and_then(Value::as_str))
    {
        return value.to_ascii_lowercase();
    }
    std::env::var("ZERO_ENVELOPE")
        .unwrap_or_else(|_| "v3".to_string())
        .to_ascii_lowercase()
}

fn codemode_envelope_ref(result: &crate::CodeModeResult) -> Option<String> {
    result
        .execution_refs
        .as_ref()
        .and_then(|refs| {
            refs.pointer("/stored/envelope")
                .or_else(|| refs.get("envelope"))
        })
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[derive(Clone, Copy)]
enum CodemodeEnvelope {
    /// v2: optional `t:{telemetry_ref}` trailer; no execution_id in ack/structured.
    V2,
    /// v3: mechanical ` {execution_id}` trailer; structuredContent always carries value.
    V3,
}

fn codemode_ack_pct(result: &crate::CodeModeResult) -> String {
    if result.telemetry.raw_tokens > 0 {
        format!(
            "{:.0}%",
            tokenzero_core::savings_ratio(
                result.telemetry.raw_tokens,
                result.telemetry.envelope_tokens + result.telemetry.payload_tokens,
            ) * 100.0
        )
    } else {
        "-".to_string()
    }
}

fn codemode_error_parts(result: &crate::CodeModeResult) -> (&str, &str, String) {
    result
        .error
        .as_ref()
        .map_or(("runtime", "final", "unknown error".to_string()), |error| {
            (
                error.kind.as_str(),
                if error.retryable {
                    "retryable"
                } else {
                    "final"
                },
                error.message.chars().take(120).collect(),
            )
        })
}

fn codemode_ack_trailer(
    result: &crate::CodeModeResult,
    envelope: CodemodeEnvelope,
    store_ref: Option<&str>,
) -> String {
    match envelope {
        CodemodeEnvelope::V2 => store_ref.map(|r| format!(" t:{r}")).unwrap_or_default(),
        CodemodeEnvelope::V3 => format!(
            " {}",
            result
                .execution_id
                .as_deref()
                .unwrap_or("cm://exec/unknown")
        ),
    }
}

fn codemode_ack(
    result: &crate::CodeModeResult,
    envelope: CodemodeEnvelope,
    store_ref: Option<&str>,
) -> String {
    let trailer = codemode_ack_trailer(result, envelope, store_ref);
    match result.status {
        crate::CodeModeStatus::Completed => {
            format!(
                "ok tz{} {}{trailer}",
                result.telemetry.logical_ops,
                codemode_ack_pct(result)
            )
        }
        crate::CodeModeStatus::Error => {
            let (kind, retryable, first) = codemode_error_parts(result);
            format!("err {kind} {retryable} {first}{trailer}")
        }
    }
}

/// Fold a short scalar into ack text. v2 inserts before ` t:`; v3 appends.
/// Idempotent so double-fold cannot render `=true =true`.
fn scalar_folded_codemode_ack(
    ack: &str,
    value: &Value,
    envelope: CodemodeEnvelope,
) -> Option<String> {
    if !(value.is_string() || value.is_number() || value.is_boolean()) {
        return None;
    }
    let value_text = serde_json::to_string(value).ok()?;
    if count_tokens(&value_text) > 16 {
        return None;
    }
    if ack.contains(&format!(" ={value_text}")) {
        return None;
    }
    match envelope {
        CodemodeEnvelope::V2 => {
            let (prefix, suffix) = ack.rsplit_once(" t:")?;
            Some(format!("{prefix} ={value_text} t:{suffix}"))
        }
        CodemodeEnvelope::V3 => Some(format!("{ack} ={value_text}")),
    }
}

fn refs_referenced_by_value(value: Option<&Value>, ordered_refs: &[String]) -> Vec<String> {
    fn collect(value: &Value, refs: &mut std::collections::HashSet<String>) {
        match value {
            Value::String(text) => {
                if text.starts_with("tz://") {
                    refs.insert(text.clone());
                }
            }
            Value::Array(items) => items.iter().for_each(|item| collect(item, refs)),
            Value::Object(map) => map.values().for_each(|value| collect(value, refs)),
            _ => {}
        }
    }

    let mut referenced = std::collections::HashSet::new();
    if let Some(value) = value {
        collect(value, &mut referenced);
    }
    ordered_refs
        .iter()
        .filter(|ref_id| referenced.contains(*ref_id))
        .cloned()
        .collect()
}

fn value_has_role_labeled_shell_refs(value: &Value) -> bool {
    value.as_object().is_some_and(|map| {
        ["stdout_ref", "stderr_ref", "combined_ref", "capture_ref"]
            .iter()
            .any(|key| {
                map.get(*key)
                    .and_then(Value::as_str)
                    .is_some_and(|text| text.starts_with("tz://"))
            })
    })
}

fn codemode_structured(
    result: &crate::CodeModeResult,
    ack: &str,
    store_ref: Option<&str>,
    envelope: CodemodeEnvelope,
) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("ack".to_string(), json!(ack));
    object.insert(
        "telemetry".to_string(),
        json!({
            "kind": result.telemetry.kind,
            "status": result.telemetry.status,
            "logical_ops": result.telemetry.logical_ops,
            "physical_ops": result.telemetry.physical_ops,
            "batched_ops": result.telemetry.batched_ops,
            "internal_actions": result.telemetry.internal_actions,
            "cache_hits": result.telemetry.cache_hits,
            "cache_misses": result.telemetry.cache_misses,
            "store_writes": result.telemetry.store_writes,
            "wall_ms": result.telemetry.wall_ms,
            "bytes_materialized": result.telemetry.bytes_materialized,
            "raw_tokens": result.telemetry.raw_tokens,
            "visible_tokens": result.telemetry.visible_tokens,
            "recovery_tokens": result.telemetry.recovery_tokens,
            "recovery_adjusted_savings_pct": result.telemetry.recovery_adjusted_savings_pct,
            "measurement_coverage_pct": result.telemetry.measurement_coverage_pct,
        }),
    );
    if matches!(result.status, crate::CodeModeStatus::Completed) {
        if let Some(value) = &result.value {
            object.insert("value".to_string(), value.clone());
        }
    } else if let Some(error) = &result.error {
        object.insert(
            "error".to_string(),
            json!({
                "kind": error.kind,
                "message": error.message,
                "retryable": error.retryable,
            }),
        );
    }
    if matches!(envelope, CodemodeEnvelope::V3) {
        if let Some(execution_id) = result.execution_id.as_deref() {
            object.insert("execution_id".to_string(), json!(execution_id));
        }
    }
    if let Some(store_ref) = store_ref {
        object.insert("ref".to_string(), json!(store_ref));
    }
    let value_refs = refs_referenced_by_value(result.value.as_ref(), &result.refs);
    if !value_refs.is_empty()
        && !result
            .value
            .as_ref()
            .is_some_and(value_has_role_labeled_shell_refs)
    {
        object.insert("refs".to_string(), json!(value_refs));
    }
    Value::Object(object)
}

/// True when a completed plan's return value is present on structuredContent.
fn codemode_result_surfaced(result: &crate::CodeModeResult, structured: &Value) -> bool {
    matches!(result.status, crate::CodeModeStatus::Completed)
        && result.value.is_some()
        && structured.get("value").is_some()
}

/// Completed + in-memory value must ship structuredContent.value. Else downgrade
/// so hubs never claim ok with result_surfaced=false (jhh).
fn enforce_completed_result_surfaced(
    result: &crate::CodeModeResult,
    ack: String,
    structured: Value,
    result_surfaced: bool,
) -> (String, Value, bool) {
    let must_surface =
        matches!(result.status, crate::CodeModeStatus::Completed) && result.value.is_some();
    if !must_surface || result_surfaced {
        return (ack, structured, false);
    }
    let exec = result
        .execution_id
        .as_deref()
        .unwrap_or("cm://exec/unknown");
    let message =
        "completed plan result was stored but not attached to structuredContent; refuse ok ack";
    let err_ack = format!("err result_not_surfaced final {message} {exec}");
    let err_structured = json!({
        "ack": err_ack,
        "error": {
            "kind": "result_not_surfaced",
            "message": message,
            "retryable": false,
        },
        "execution_id": result.execution_id,
    });
    (err_ack, err_structured, true)
}

fn codemode_envelope_tool_response(
    name: &str,
    result: &crate::CodeModeResult,
    envelope: CodemodeEnvelope,
) -> Result<ToolResponse, JsonRpcErrorData> {
    // Never invent tz://missing-envelope — that sentinel was treated as a live
    // success ref by hubs and savings ledgers when busy aborted before store.
    let store_ref = codemode_envelope_ref(result);
    let mut ack = codemode_ack(result, envelope, store_ref.as_deref());
    // Fold scalar into ack for short text, but never drop structuredContent.value
    // (tokenzero-codemode-result-not-surfaced-jhh). v3 acks lack `t:tz://`, so
    // hubs recover folded scalars only via structuredContent.
    let folded_scalar = matches!(result.status, crate::CodeModeStatus::Completed)
        && result
            .value
            .as_ref()
            .and_then(|value| scalar_folded_codemode_ack(&ack, value, envelope))
            .map(|folded| {
                ack = folded;
            })
            .is_some();
    let structured = codemode_structured(result, &ack, store_ref.as_deref(), envelope);
    let result_surfaced = codemode_result_surfaced(result, &structured);
    let (ack, structured, force_error) =
        enforce_completed_result_surfaced(result, ack, structured, result_surfaced);
    let structured_tokens = if folded_scalar && !force_error {
        0
    } else {
        count_tokens(&serde_json::to_string(&structured).unwrap_or_default())
    };
    let envelope_tokens = count_tokens(&ack) + structured_tokens;
    // Unexecuted / errored plans must not credit raw_tokens savings.
    let credited_raw = if matches!(result.status, crate::CodeModeStatus::Error) || force_error {
        0
    } else {
        result.telemetry.raw_tokens
    };
    let mut response = inline_response(name, Mode::Structured, ack, credited_raw);
    let accounting = response.accounting.as_mut().expect("inline accounting");
    accounting.visible_tokens = envelope_tokens;
    accounting.exact_ref_tokens = None;
    let mut telemetry = json!({
        "envelope_tokens": envelope_tokens,
        "payload_tokens": result.telemetry.payload_tokens,
        "result_surfaced": result_surfaced && !force_error,
        "structuredContent": structured,
    });
    if matches!(envelope, CodemodeEnvelope::V3) {
        telemetry["envelope"] = json!("v3");
        if let Some(execution_id) = &result.execution_id {
            telemetry["execution_id"] = json!(execution_id);
        }
    }
    if let Some(store_ref) = &store_ref {
        telemetry["telemetry_ref"] = json!(store_ref);
    }
    response.telemetry = Some(telemetry);
    if matches!(result.status, crate::CodeModeStatus::Error) || force_error {
        response.status = "error".to_string();
        response.error = if force_error {
            Some(tokenzero_core::CliError {
                code: "result_not_surfaced".into(),
                message: "completed plan had a result that was not attached to structuredContent"
                    .into(),
                repair: None,
            })
        } else {
            result.error.as_ref().map(|error| tokenzero_core::CliError {
                code: error.kind.clone(),
                message: error.message.clone(),
                repair: None,
            })
        };
    }
    Ok(response)
}

fn codemode_v2_tool_response(
    name: &str,
    result: &crate::CodeModeResult,
) -> Result<ToolResponse, JsonRpcErrorData> {
    codemode_envelope_tool_response(name, result, CodemodeEnvelope::V2)
}

fn codemode_v3_tool_response(
    name: &str,
    result: &crate::CodeModeResult,
) -> Result<ToolResponse, JsonRpcErrorData> {
    codemode_envelope_tool_response(name, result, CodemodeEnvelope::V3)
}

fn logical_execution_suffix(execution_id: &str, suffix: &str) -> String {
    let normalized = execution_id
        .strip_prefix("cm://exec/")
        .unwrap_or(execution_id);
    if suffix.is_empty() {
        format!("tz://codemode/execution/{normalized}")
    } else {
        format!("tz://codemode/execution/{normalized}/{suffix}")
    }
}

fn codemode_contract_payload_v1(result: &crate::CodeModeResult) -> Value {
    let ack = result.visible_ack.clone();
    let mut refs = serde_json::Map::new();
    if let Some(execution_id) = result.execution_id.as_deref() {
        let status_ref = match result.status {
            crate::CodeModeStatus::Completed => "result",
            crate::CodeModeStatus::Error => "error",
        };
        for key in ["code", "steps", "telemetry", status_ref] {
            refs.insert(
                key.to_string(),
                json!(logical_execution_suffix(execution_id, key)),
            );
        }
    }
    let mut payload = json!({
        "ack": ack,
        "detail_ref": result.detail_ref,
        "execution_id": result.execution_id,
        "refs": refs,
        "telemetry": result.telemetry,
    });
    if matches!(result.status, crate::CodeModeStatus::Completed) {
        if let Some(value) = &result.value {
            payload["value"] = value.clone();
        }
    } else {
        if let Some(error) = &result.error {
            payload["error"] = json!(error);
        }
        if let Some(error_ref) = payload.pointer("/refs/error").cloned() {
            payload["error_ref"] = error_ref;
        }
    }
    payload
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

/// Tool dispatch shared by direct calls and `tz_batch` sub-ops.
///
/// Domain operations route through [`tokenzero_engine::dispatch_operation`].
/// Transport-control tools (execute_code / codemode search+describe) stay here.
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

pub(crate) fn batch_response(
    engine: &TokenZeroEngine,
    args: &Value,
) -> Result<ToolResponse, JsonRpcErrorData> {
    let ops = batch_ops(args).map_err(JsonRpcErrorData::from)?;
    let mut sections = Vec::with_capacity(ops.len());
    let mut refs: Vec<tokenzero_core::RefRecord> = Vec::new();
    let mut listed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut raw_tokens = 0usize;
    let mut recovery_tokens = 0usize;
    let mut per_op = Vec::with_capacity(ops.len());
    for (index, (tool, op_args)) in ops.iter().enumerate() {
        let canonical = canonical_tool(tool);
        let position = index + 1;
        if canonical == "batch" {
            sections.push(format!(
                "## {position} {tool} — error: nested batch is not allowed"
            ));
            per_op.push(json!({"tool": tool, "status": "error", "code": "nested_batch"}));
            continue;
        }
        match dispatch_tool(engine, canonical, tool, op_args) {
            Ok(response) => {
                let text = response
                    .visible
                    .as_ref()
                    .map(|visible| visible.text.clone())
                    .or_else(|| {
                        response
                            .error
                            .as_ref()
                            .map(|error| format!("error: {} ({})", error.message, error.code))
                    })
                    .unwrap_or_default();
                sections.push(format!("## {position} {canonical}\n{text}"));
                per_op.push(json!({"tool": tool, "status": response.status}));
                if let Some(accounting) = &response.accounting {
                    raw_tokens += accounting.raw_tokens;
                    recovery_tokens += accounting.recovery_tokens;
                }
                for record in response.refs {
                    if listed.insert(record.ref_id.clone()) {
                        refs.push(record);
                    }
                }
            }
            Err(error) => {
                sections.push(format!(
                    "## {position} {canonical} — error: {}",
                    error.message_text()
                ));
                per_op.push(json!({"tool": tool, "status": "error"}));
            }
        }
    }
    let text = sections.join("\n\n");
    let visible_tokens = count_tokens(&text);
    let exact_ref_tokens = refs.iter().map(|record| count_tokens(&record.ref_id)).sum();
    let mut response = ToolResponse::ok(
        "batch",
        arg_mode(args),
        text,
        refs,
        Accounting {
            raw_tokens,
            visible_tokens,
            recovery_tokens,
            billed_tokens: visible_tokens,
            cached_tokens: 0,
            exact_ref_tokens: Some(exact_ref_tokens),
        },
    );
    response.telemetry = Some(json!({
        "ops": per_op.len(),
        "per_op": per_op,
    }));
    Ok(response)
}

fn batch_ops(args: &Value) -> Result<Vec<(String, Value)>, String> {
    const MAX_BATCH_OPS: usize = 16;
    let raw = args
        .get("ops")
        .ok_or_else(|| "missing ops: an array of {tool, args} objects".to_string())?;
    // Stub-schema clients may send the array JSON-encoded as a string.
    let parsed;
    let items = match raw {
        Value::Array(items) => items,
        Value::String(text) => {
            parsed = serde_json::from_str::<Value>(text)
                .map_err(|err| format!("ops is not valid JSON: {err}"))?;
            parsed
                .as_array()
                .ok_or_else(|| "ops must be an array".to_string())?
        }
        _ => return Err("ops must be an array of {tool, args} objects".to_string()),
    };
    if items.is_empty() {
        return Err("ops must contain at least one op".to_string());
    }
    if items.len() > MAX_BATCH_OPS {
        return Err(format!("ops is capped at {MAX_BATCH_OPS} per batch"));
    }
    items
        .iter()
        .map(|item| {
            let tool = item
                .get("tool")
                .and_then(Value::as_str)
                .ok_or_else(|| "each op needs a tool name".to_string())?;
            let op_args = item.get("args").cloned().unwrap_or_else(|| json!({}));
            Ok((tool.to_string(), op_args))
        })
        .collect()
}

pub(crate) fn mcp_tool_response(response: ToolResponse) -> Value {
    let is_error = response.status == "error";
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
                    serde_json::to_value(&response).unwrap_or(Value::Null)
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
    // Always stamp isError after structuredContent attachment. An early return
    // previously dropped isError for CodeMode v2, so FastMCP treated retryable
    // busy (machine_permit_busy) as a successful tool result.
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
        || text.trim() == tokenzero_recovery::working_set::ALREADY_RESIDENT_ATOM
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
    let mut value = serde_json::to_value(response).unwrap_or(Value::Null);
    if let Some(object) = value.as_object_mut() {
        // The capsule text already ships as content[0].text; repeating it
        // here doubles the cost of every call.
        object.remove("visible");
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

fn arg_mode(args: &Value) -> Mode {
    args.get("mode")
        .and_then(Value::as_str)
        .and_then(|v| v.parse().ok())
        .unwrap_or(Mode::Auto)
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
mod result_surfaced_envelope_tests {
    use super::*;
    use crate::CodeModeResult;

    #[test]
    fn v3_completed_scalar_keeps_structured_value() {
        let mut result = CodeModeResult::completed(json!(42), Vec::new(), 1, 1, 10);
        result.execution_id = Some("cm://exec/jhh-test".into());
        let response = codemode_v3_tool_response("tz_execute_code", &result).unwrap();
        assert_eq!(response.status, "ok");
        let telemetry = response.telemetry.as_ref().unwrap();
        assert_eq!(telemetry.get("result_surfaced"), Some(&json!(true)));
        assert_eq!(
            telemetry.pointer("/structuredContent/value"),
            Some(&json!(42))
        );
        let text = response.visible.as_ref().unwrap().text.as_str();
        assert!(text.starts_with("ok "), "{text}");
        assert!(text.contains("=42"), "{text}");
        let mcp = mcp_tool_response(response);
        assert_eq!(mcp.pointer("/structuredContent/value"), Some(&json!(42)));
    }

    #[test]
    fn v3_object_result_always_surfaces_value() {
        let mut result = CodeModeResult::completed(json!({"answer": 42}), Vec::new(), 1, 1, 10);
        result.execution_id = Some("cm://exec/jhh-object".into());
        let response = codemode_v3_tool_response("tz_execute_code", &result).unwrap();
        assert_eq!(response.status, "ok");
        let telemetry = response.telemetry.as_ref().unwrap();
        assert_eq!(telemetry.get("result_surfaced"), Some(&json!(true)));
        assert_eq!(
            telemetry.pointer("/structuredContent/value/answer"),
            Some(&json!(42))
        );
    }

    #[test]
    fn codemode_structured_surfaces_recovery_adjusted_telemetry() {
        let mut result = CodeModeResult::completed(json!({"answer": 42}), Vec::new(), 100, 20, 10);
        result.telemetry.recovery_tokens = 40;
        result.telemetry.recovery_adjusted_savings_pct = 40.0;
        let structured = codemode_structured(&result, "ok", None, CodemodeEnvelope::V2);
        assert_eq!(
            structured.pointer("/telemetry/recovery_tokens"),
            Some(&json!(40))
        );
        assert_eq!(
            structured.pointer("/telemetry/recovery_adjusted_savings_pct"),
            Some(&json!(40.0))
        );
    }
}

#[cfg(test)]
mod permit_busy_envelope_tests {
    use super::*;
    use crate::CodeModeResult;
    use crate::fastmcp_mode::fastmcp_content_texts_from_tool_result;

    #[test]
    fn busy_without_envelope_sets_is_error_and_skips_sentinel() {
        let result =
            CodeModeResult::error_with_kind("busy", "machine_permit_busy: held by pid 1", 99, true);
        let response = codemode_v2_tool_response("tz_execute_code", &result).unwrap();
        assert_eq!(response.status, "error");
        let accounting = response.accounting.as_ref().expect("accounting");
        assert_eq!(
            accounting.raw_tokens, 0,
            "unexecuted busy must not credit raw_tokens for savings"
        );

        let mcp = mcp_tool_response(response);
        assert_eq!(mcp.get("isError"), Some(&Value::Bool(true)));
        let text = mcp["content"][0]["text"].as_str().unwrap_or("");
        assert!(
            text.starts_with("err busy retryable"),
            "expected typed busy ack, got {text:?}"
        );
        assert!(
            !text.contains("tz://missing-envelope"),
            "sentinel must not appear as success ref: {text}"
        );
        let structured = mcp
            .get("structuredContent")
            .expect("structuredContent for v2 errors");
        assert_eq!(structured["error"]["kind"], "busy");
        assert_eq!(structured["error"]["retryable"], true);
        assert!(
            structured.get("ref").is_none(),
            "no invented envelope ref: {structured}"
        );
        assert!(
            fastmcp_content_texts_from_tool_result(&mcp).is_err(),
            "FastMCP must treat busy as tool error, not dual-content success"
        );
    }
}
