use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokenzero_core::{
    Accounting, ContentType, MCP_SCHEMA_VERSION, Mode, ToolResponse, count_tokens,
    shell_display_command_from_argv_for_platform,
};
use tokenzero_filters::{discover, rewrite_command};
use tokenzero_runtime::{ExecutionMode, plan_command_for_platform};

use crate::catalog::canonical_allowed_on_surface;
use crate::expand_params::ExpandParams;
use crate::jsonrpc::JsonRpcErrorData;
use crate::{EditHunk, ServeOptions, TokenZeroEngine, shell_timeout_from_secs};

pub(crate) fn call_tool(
    engine: &TokenZeroEngine,
    name: &str,
    args: &Value,
    call_id: Option<String>,
) -> Result<Value, JsonRpcErrorData> {
    let canonical = canonical_tool(name);
    let started = std::time::Instant::now();
    if !canonical_allowed_on_surface(engine.config.tool_surface, canonical) {
        return Err(JsonRpcErrorData::unknown_tool(name));
    }
    let result = dispatch_tool(engine, canonical, name, args);
    engine.record_tool_call(canonical, started.elapsed(), result.is_err());
    let response = result?;
    record_mcp_pulse(engine, canonical, args, &response, call_id);
    Ok(mcp_tool_response(response))
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
    let Some(root) = engine.config.allowed_roots.first() else {
        return;
    };
    let Some(accounting) = response.accounting.as_ref() else {
        return;
    };
    let mut ref_ids: Vec<String> = response
        .refs
        .iter()
        .map(|record| record.ref_id.clone())
        .collect();
    if canonical == "expand" {
        if let Some(ref_id) = args.get("ref").and_then(Value::as_str) {
            ref_ids.push(ref_id.to_string());
        }
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

fn json_tool_response(name: &str, value: Value) -> Result<ToolResponse, JsonRpcErrorData> {
    let text = serde_json::to_string(&value).map_err(|err| err.to_string())?;
    let tokens = count_tokens(&text);
    Ok(ToolResponse::ok(
        name,
        Mode::Structured,
        text,
        Vec::new(),
        Accounting {
            raw_tokens: tokens,
            visible_tokens: tokens,
            recovery_tokens: 0,
            exact_ref_tokens: None,
        },
    ))
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
        ..Default::default()
    };
    if let Some(root) = engine.config.allowed_roots.first() {
        options.root = Some(root.clone());
    }
    if let Some(limits) = args.get("limits").and_then(Value::as_object) {
        if let Some(value) = limits.get("max_output_bytes").and_then(Value::as_u64) {
            options.max_output_bytes = value as usize;
        }
        if let Some(value) = limits.get("max_refs_emitted").and_then(Value::as_u64) {
            options.max_refs_emitted = value as usize;
        }
        if let Some(value) = limits.get("max_logical_ops").and_then(Value::as_u64) {
            options.max_logical_ops = value as usize;
        }
        if let Some(value) = limits.get("max_physical_ops").and_then(Value::as_u64) {
            options.max_physical_ops = value as usize;
        }
        if let Some(value) = limits.get("max_microtasks").and_then(Value::as_u64) {
            options.max_microtasks = value as usize;
        }
        if let Some(value) = limits.get("max_memory_bytes").and_then(Value::as_u64) {
            options.max_memory_bytes = value as usize;
        }
        if let Some(value) = limits.get("max_code_bytes").and_then(Value::as_u64) {
            options.max_code_bytes = value as usize;
        }
    }
    let result = crate::execute_codemode_with_options(plan, options);
    json_tool_response(name, codemode_contract_payload(&result))
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
            "max_code_bytes": crate::CodeModeLimits::default().max_code_bytes
        }
    })
}

fn codemode_contract_payload(result: &crate::CodeModeResult) -> Value {
    let ack = result.visible_ack.clone();
    let mut refs = serde_json::Map::new();
    if let Some(execution_refs) = result.execution_refs.as_ref().and_then(Value::as_object) {
        for key in ["code", "steps", "telemetry"] {
            if let Some(value) = execution_refs.get(key) {
                refs.insert(key.to_string(), value.clone());
            }
        }
        match result.status {
            crate::CodeModeStatus::Completed => {
                if let Some(value) = execution_refs.get("result") {
                    refs.insert("result".to_string(), value.clone());
                }
            }
            crate::CodeModeStatus::Error => {
                if let Some(value) = execution_refs.get("error") {
                    refs.insert("error".to_string(), value.clone());
                }
            }
        }
    }
    let mut payload = json!({
        "ack": ack,
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

/// Tool dispatch shared by direct calls and `tz_batch` sub-ops.
fn dispatch_tool(
    engine: &TokenZeroEngine,
    canonical: &str,
    name: &str,
    args: &Value,
) -> Result<ToolResponse, JsonRpcErrorData> {
    let response = match canonical {
        "execute_code" => exec_codemode_tool(engine, name, args)?,
        "codemode_search" => exec_codemode_search_tool(name, args)?,
        "codemode_describe" => exec_codemode_describe_tool(name, args)?,
        "read" => {
            let path = arg_path_list(args, "path")?;
            engine.read_with_options(
                &path,
                arg_mode(args),
                arg_u64(args, "start_line"),
                arg_u64(args, "end_line"),
                arg_bool(args, "raw"),
                arg_u64(args, "max_files").unwrap_or(20),
                arg_u64(args, "max_visible_tokens").unwrap_or(4000),
                arg_serve_options(args),
            )
        }
        "find" => {
            let query = arg_string_any(args, &["query", "pattern"])?;
            let path = arg_path_list(args, "path").unwrap_or_else(|_| vec![PathBuf::from(".")]);
            engine.find_with_options(
                query,
                &path,
                arg_mode(args),
                arg_u64(args, "max_files").unwrap_or(20),
                arg_u64(args, "max_visible_tokens").unwrap_or(4000),
                arg_serve_options(args),
            )
        }
        "grep" => {
            let query = arg_string_any(args, &["query", "pattern"])?;
            let path = arg_path_list(args, "path").unwrap_or_else(|_| vec![PathBuf::from(".")]);
            engine.grep_with_options(
                query,
                &path,
                arg_mode(args),
                arg_u64(args, "max_files").unwrap_or(20),
                arg_u64(args, "max_visible_tokens").unwrap_or(4000),
                arg_serve_options(args),
            )
        }
        "recall" => {
            let query = arg_string_any(args, &["query", "pattern"])?;
            engine.recall(
                query,
                arg_u64(args, "max_hits").unwrap_or(50),
                arg_mode(args),
                arg_u64(args, "max_visible_tokens").unwrap_or(4000),
            )
        }
        "glob" => {
            let pattern = arg_string_any(args, &["pattern", "glob", "query"])?;
            let path = arg_path_list(args, "path").unwrap_or_else(|_| vec![PathBuf::from(".")]);
            engine.glob(
                pattern,
                &path,
                arg_bool(args, "include_hidden"),
                arg_mode(args),
                arg_u64(args, "max_files").unwrap_or(200),
                arg_u64(args, "max_visible_tokens").unwrap_or(4000),
            )
        }
        "tree" => {
            let path = arg_path_list(args, "path").unwrap_or_else(|_| vec![PathBuf::from(".")]);
            engine.tree(
                &path,
                arg_u64(args, "depth").unwrap_or(2),
                arg_bool(args, "include_hidden"),
                arg_mode(args),
                arg_u64(args, "max_files").unwrap_or(200),
                arg_u64(args, "max_visible_tokens").unwrap_or(4000),
            )
        }
        "edit" => {
            let path = args
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| "missing path".to_string())?;
            let edits = arg_edit_hunks(args)?;
            engine.edit(
                Path::new(path),
                &edits,
                arg_bool(args, "create"),
                arg_bool(args, "dry_run"),
                arg_mode(args),
                arg_u64(args, "max_visible_tokens").unwrap_or(4000),
            )
        }
        "shell" => {
            let (command, argv) = arg_command(args)?;
            engine.shell(
                &command,
                argv,
                args.get("cwd").and_then(Value::as_str).map(Path::new),
                arg_mode(args),
                args.get("rewrite").and_then(Value::as_str),
                arg_bool(args, "no_rewrite"),
                None,
                args.get("stdin").and_then(Value::as_str),
                arg_timeout_any(
                    args,
                    &[
                        "timeout_seconds",
                        "timeout_secs",
                        "timeout",
                        "shell_timeout_seconds",
                    ],
                ),
            )
        }
        "ingest" => {
            let text = args
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| args.get("input").and_then(Value::as_str))
                .ok_or_else(|| "missing text".to_string())?;
            engine.ingest(text, ContentType::Unknown, arg_mode(args), "mcp-ingest")
        }
        "expand" => {
            let params = ExpandParams::from_tool_args(args)?;
            engine.expand_with_params(params)
        }
        "mem" => engine.mem(),
        "cache_pack" => {
            engine.cache_pack(args.get("scope").and_then(Value::as_str).unwrap_or("agent"))
        }
        "rewrite" => {
            let (command, _) = arg_command(args)?;
            let value = serde_json::to_string_pretty(&rewrite_command(
                &command,
                args.get("mode").and_then(Value::as_str).unwrap_or("safe"),
                true,
            ))
            .unwrap_or_default();
            ToolResponse::ok(
                "rewrite",
                Mode::Hybrid,
                value.clone(),
                Vec::new(),
                Accounting {
                    raw_tokens: count_tokens(&command),
                    visible_tokens: count_tokens(&value),
                    recovery_tokens: 0,
                    exact_ref_tokens: Some(0),
                },
            )
        }
        "discover" => {
            let value = serde_json::to_string_pretty(&discover()).unwrap_or_default();
            ToolResponse::ok(
                "discover",
                Mode::Hybrid,
                value.clone(),
                Vec::new(),
                Accounting {
                    raw_tokens: count_tokens(&value),
                    visible_tokens: count_tokens(&value),
                    recovery_tokens: 0,
                    exact_ref_tokens: Some(0),
                },
            )
        }
        "batch" => batch_response(engine, args)?,
        "fetch" => {
            let url = arg_string_any(args, &["url", "uri"])?;
            engine.fetch(
                url,
                arg_u64(args, "ttl_seconds"),
                arg_bool(args, "fresh"),
                arg_mode(args),
                arg_u64(args, "max_visible_tokens").unwrap_or(4000),
            )
        }
        _ => return Err(JsonRpcErrorData::unknown_tool(name)),
    };
    Ok(response)
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

fn mcp_tool_response(response: ToolResponse) -> Value {
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
    let mut result = json!({
        "content": [{"type": "text", "text": text}],
        "resultType": "complete"
    });
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
    if response.refs.is_empty() {
        return None;
    }
    if response.refs.iter().any(|r| text.contains(&r.ref_id)) {
        return None;
    }
    let mut listed: Vec<String> = Vec::new();
    let mut extra: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for record in &response.refs {
        // blob/file carry whole payloads; combined covers full shell output.
        if record.kind == "blob" || record.kind == "file" || record.kind == "combined" {
            listed.push(record.ref_id.clone());
        } else if record.kind == "undo" {
            listed.push(format!("undo:{}", record.ref_id));
        } else {
            *extra.entry(record.kind.as_str()).or_default() += 1;
        }
    }
    if listed.is_empty() {
        listed = response
            .refs
            .iter()
            .map(|record| record.ref_id.clone())
            .collect();
        extra.clear();
    }
    let mut line = format!("refs: {}", listed.join(" "));
    for (kind, count) in extra {
        line.push_str(&format!(" +{count}:{kind}"));
    }
    Some(line)
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
        other => other,
    }
}

fn arg_mode(args: &Value) -> Mode {
    args.get("mode")
        .and_then(Value::as_str)
        .and_then(|v| v.parse().ok())
        .unwrap_or(Mode::Auto)
}

/// Per-call session-redundancy options: `fresh: true` bypasses the seen-set
/// dedup/diff layer for this call (the serve is still recorded).
fn arg_serve_options(args: &Value) -> ServeOptions {
    ServeOptions {
        fresh: arg_bool(args, "fresh"),
    }
}

// Alias tools advertise a permissive `{"type": "object"}` stub, so clients
// without the canonical schema may serialize booleans and integers as
// strings. Coerce those instead of silently dropping the argument.
fn arg_bool(args: &Value, key: &str) -> bool {
    match args.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "true" | "1" | "yes"
            )
        }
        _ => false,
    }
}

fn arg_u64(args: &Value, key: &str) -> Option<usize> {
    coerce_u64(args.get(key)?).and_then(|value| usize::try_from(value).ok())
}

fn coerce_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn arg_timeout_any(args: &Value, keys: &[&str]) -> Option<Duration> {
    keys.iter().find_map(|key| {
        args.get(*key)
            .and_then(coerce_u64)
            .map(|seconds| shell_timeout_from_secs(Some(seconds)))
    })
}

fn arg_string_any<'a>(args: &'a Value, keys: &[&str]) -> Result<&'a str, String> {
    for key in keys {
        if let Some(value) = args.get(*key).and_then(Value::as_str) {
            return Ok(value);
        }
    }
    Err(format!("missing {}", keys.join("|")))
}

fn arg_command(args: &Value) -> Result<(String, Option<Vec<String>>), String> {
    if let Some(value) = args.as_str() {
        return Ok((value.to_string(), None));
    }
    if let Some(items) = args.as_array() {
        let argv = string_array_arg(items, "argv")?;
        return Ok((display_command_for_argv(&argv), Some(argv)));
    }
    for key in ["command", "cmd", "input", "script"] {
        if let Some(value) = args.get(key).and_then(Value::as_str) {
            return Ok((value.to_string(), None));
        }
    }
    for key in ["argv", "args"] {
        if let Some(items) = args.get(key).and_then(Value::as_array) {
            let argv = string_array_arg(items, key)?;
            return Ok((display_command_for_argv(&argv), Some(argv)));
        }
    }
    Err("missing command; expected command/cmd/input/script string or argv/args array".to_string())
}

fn display_command_for_argv(argv: &[String]) -> String {
    display_command_for_argv_on_platform(argv, tokenzero_runtime::current_platform())
}

fn display_command_for_argv_on_platform(argv: &[String], platform: &str) -> String {
    match plan_command_for_platform(argv, None, false, platform) {
        Ok(plan) if plan.execution_mode == ExecutionMode::Shell => argv.join(" "),
        _ => shell_display_command_from_argv_for_platform(argv, platform),
    }
}

fn arg_path_list(args: &Value, key: &str) -> Result<Vec<PathBuf>, String> {
    let Some(value) = args.get(key) else {
        return Err(format!("missing {key}"));
    };
    if let Some(path) = value.as_str() {
        // Stub-schema clients may send a list as its JSON-encoded string.
        if path.trim_start().starts_with('[') {
            if let Ok(paths) = serde_json::from_str::<Vec<String>>(path) {
                if paths.is_empty() {
                    return Err(format!("invalid {key}; expected non-empty array"));
                }
                return Ok(paths.into_iter().map(PathBuf::from).collect());
            }
        }
        return Ok(vec![PathBuf::from(path)]);
    }
    if let Some(items) = value.as_array() {
        let paths = string_array_arg(items, key)?
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        return Ok(paths);
    }
    Err(format!("invalid {key}"))
}

// Stub-schema clients may serialize the edits array as its JSON-encoded
// string; accept both shapes and coerce stringly-typed replace_all booleans.
fn arg_edit_hunks(args: &Value) -> Result<Vec<EditHunk>, String> {
    let value = args
        .get("edits")
        .ok_or_else(|| "missing edits".to_string())?;
    let items: Vec<Value> = match value {
        Value::Array(items) => items.clone(),
        Value::String(text) => serde_json::from_str(text).map_err(|_| {
            "invalid edits; expected a JSON array of {find, replace} objects".to_string()
        })?,
        _ => return Err("invalid edits; expected array of {find, replace} objects".to_string()),
    };
    if items.is_empty() {
        return Err("invalid edits; expected non-empty array".to_string());
    }
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let find = item
                .get("find")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("invalid edits[{index}].find; expected string"))?;
            let replace = item
                .get("replace")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("invalid edits[{index}].replace; expected string"))?;
            Ok(EditHunk {
                find: find.to_string(),
                replace: replace.to_string(),
                replace_all: arg_bool(item, "replace_all"),
            })
        })
        .collect()
}

fn string_array_arg(items: &[Value], label: &str) -> Result<Vec<String>, String> {
    if items.is_empty() {
        return Err(format!(
            "invalid {label}; expected non-empty array of strings"
        ));
    }
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("invalid {label}[{index}]; expected array of strings"))
        })
        .collect()
}

#[cfg(test)]
mod tests;
