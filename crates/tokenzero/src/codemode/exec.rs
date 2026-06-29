//! CodeMode plan executor and TokenZero operation dispatch.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokenzero_core::{Mode, ToolResponse, count_tokens, detect_content_type};
use tokenzero_mcp::{EditHunk, EngineConfig, TokenZeroEngine, shell_timeout_from_secs};

use crate::zerostack_store::{default_codemode_recovery_cache_path, allowed_roots_for_workspace, tokenzero_work_root};

use super::catalog::{describe_method, search_catalog};
use super::parser::{MethodCall, Statement, parse_plan, resolve_expr, resolve_return};
use super::result::{CodeModeOptions, CodeModeResult};

#[cfg(test)]
pub(crate) fn make_engine_for_root(root: PathBuf) -> TokenZeroEngine {
    make_engine_for_root_with_options(root, &CodeModeOptions::default())
}

fn make_engine_for_root_with_options(root: PathBuf, options: &CodeModeOptions) -> TokenZeroEngine {
    let cache_path = options
        .cache_path
        .clone()
        .unwrap_or_else(|| default_codemode_recovery_cache_path(&root));
    TokenZeroEngine::new(EngineConfig {
        allowed_roots: allowed_roots_for_workspace(&root, &options.allowed_roots),
        cache_path,
        max_visible_tokens: options.max_visible_tokens,
        mode: Mode::Auto,
        shell_timeout: shell_timeout_from_secs(options.timeout_seconds),
        ..EngineConfig::for_root(&root)
    })
}

#[cfg(test)]
pub fn execute_codemode(plan: &str) -> CodeModeResult {
    execute_codemode_with_options(plan, CodeModeOptions::default())
}

pub(crate) fn execute_codemode_with_options(
    plan: &str,
    options: CodeModeOptions,
) -> CodeModeResult {
    let plan = plan.trim();
    if plan.is_empty() {
        return CodeModeResult::error("empty plan", 0);
    }

    if let Some(query) = plan.strip_prefix("search:") {
        let result = search_catalog(query.trim());
        let text = serde_json::to_string_pretty(&result).unwrap_or_default();
        let tokens = count_tokens(&text);
        return CodeModeResult::completed(result, Vec::new(), 1, tokens, tokens);
    }
    if let Some(target) = plan.strip_prefix("describe:") {
        let result = describe_method(target.trim());
        let text = serde_json::to_string_pretty(&result).unwrap_or_default();
        let tokens = count_tokens(&text);
        return CodeModeResult::completed(result, Vec::new(), 1, tokens, tokens);
    }

    let statements = match parse_plan(plan) {
        Ok(s) => s,
        Err(e) => return CodeModeResult::error(e, 0),
    };

    let work_root = tokenzero_work_root(options.root.clone());
    let engine = make_engine_for_root_with_options(work_root.clone(), &options);
    let mut scope: HashMap<String, Value> = HashMap::new();
    let mut all_refs: Vec<String> = Vec::new();
    let mut ops: usize = 0;
    let mut total_visible: usize = 0;
    let mut total_raw: usize = 0;
    let mut last_value: Value = Value::Null;

    for stmt in &statements {
        match stmt {
            Statement::Binding { name, call } => {
                ops += 1;
                let result = match dispatch(&engine, &work_root, call, &scope) {
                    Ok(v) => v,
                    Err(mut e) => {
                        e.telemetry.operations = ops;
                        return *e;
                    }
                };
                collect_refs(&result, &mut all_refs);
                total_visible += result_visible_tokens(&result);
                total_raw += result_raw_tokens(&result);
                last_value = result.clone();
                scope.insert(name.clone(), result);
            }
            Statement::Call(call) => {
                ops += 1;
                let result = match dispatch(&engine, &work_root, call, &scope) {
                    Ok(v) => v,
                    Err(mut e) => {
                        e.telemetry.operations = ops;
                        return *e;
                    }
                };
                collect_refs(&result, &mut all_refs);
                total_visible += result_visible_tokens(&result);
                total_raw += result_raw_tokens(&result);
                last_value = result;
            }
            Statement::Return(expr) => {
                let value = resolve_return(expr, &scope);
                let vis = count_tokens(&serde_json::to_string(&value).unwrap_or_default());
                return CodeModeResult::completed(
                    value,
                    all_refs,
                    ops,
                    total_visible + vis,
                    total_raw,
                );
            }
        }
    }

    let vis = count_tokens(&serde_json::to_string(&last_value).unwrap_or_default());
    CodeModeResult::completed(last_value, all_refs, ops, total_visible + vis, total_raw)
}

fn dispatch(
    engine: &TokenZeroEngine,
    work_root: &Path,
    call: &MethodCall,
    scope: &HashMap<String, Value>,
) -> Result<Value, Box<CodeModeResult>> {
    let method = call.method.as_str();
    let args: Vec<Value> = call.args.iter().map(|a| resolve_expr(a, scope)).collect();

    match method {
        "zero.read" | "read" => exec_read(engine, &args),
        "zero.find" | "find" => exec_find(engine, work_root, &args, false),
        "zero.grep" | "grep" => exec_find(engine, work_root, &args, true),
        "zero.glob" | "glob" => exec_glob(engine, work_root, &args),
        "zero.tree" | "tree" => exec_tree(engine, work_root, &args),
        "zero.shell" | "shell" => exec_shell(engine, &args),
        "zero.edit" | "edit" => exec_edit(engine, &args),
        "zero.token.expand" | "zero.expand" | "expand" => exec_expand(engine, &args),
        "zero.token.compact" | "zero.compact" | "compact" => exec_compact(engine, &args),
        "zero.ingest" | "ingest" => exec_ingest(engine, &args),
        "zero.mem" | "mem" => exec_mem(engine),
        "codemode.search" | "search" => {
            let query = args.first().and_then(|v| v.as_str()).unwrap_or("");
            Ok(search_catalog(query))
        }
        "codemode.describe" | "describe" => {
            let path = args.first().and_then(|v| v.as_str()).unwrap_or("");
            Ok(describe_method(path))
        }
        _ => Err(Box::new(CodeModeResult::error(
            format!(
                "unknown method: {method}. Use codemode.search() to discover available methods"
            ),
            0,
        ))),
    }
}

// ─── Operation implementations ──────────────────────────────────────────────

fn tool_response_to_value(resp: &ToolResponse) -> Value {
    let text = resp
        .visible
        .as_ref()
        .map(|v| v.text.clone())
        .unwrap_or_default();
    let refs: Vec<String> = resp.refs.iter().map(|r| r.ref_id.clone()).collect();
    let accounting = resp.accounting.as_ref();
    let mut obj = json!({
        "text": text,
        "status": resp.status,
    });
    if !refs.is_empty() {
        obj["ref"] = json!(refs[0]);
        if refs.len() > 1 {
            obj["refs"] = json!(refs);
        }
    }
    if let Some(acc) = accounting {
        obj["visible_tokens"] = json!(acc.visible_tokens);
        obj["raw_tokens"] = json!(acc.raw_tokens);
    }
    if let Some(err) = &resp.error {
        obj["error"] = json!(err.message);
    }
    obj
}

fn exec_read(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let paths = match args.first() {
        Some(Value::String(s)) => vec![PathBuf::from(s)],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(PathBuf::from))
            .collect(),
        _ => {
            return Err(Box::new(CodeModeResult::error(
                "zero.read requires a path string or array as first argument",
                0,
            )));
        }
    };
    let opts = args.get(1).and_then(|v| v.as_object());
    let mode = opts
        .and_then(|o| o.get("mode"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Mode::Auto);
    let start_line = opts
        .and_then(|o| o.get("start_line"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let end_line = opts
        .and_then(|o| o.get("end_line"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let max_visible = opts
        .and_then(|o| o.get("max_visible_tokens"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(engine.config.max_visible_tokens);

    let resp = engine.read(&paths, mode, start_line, end_line, false, 20, max_visible);
    Ok(tool_response_to_value(&resp))
}

fn exec_find(
    engine: &TokenZeroEngine,
    work_root: &Path,
    args: &[Value],
    exact: bool,
) -> Result<Value, Box<CodeModeResult>> {
    let pattern = match args.first().and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.find/grep requires a pattern string as first argument",
                0,
            )));
        }
    };
    let paths: Vec<PathBuf> = match args.get(1) {
        Some(Value::String(s)) => vec![PathBuf::from(s)],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(PathBuf::from))
            .collect(),
        _ => vec![work_root.to_path_buf()],
    };
    let opts = args.get(2).and_then(|v| v.as_object());
    let mode = opts
        .and_then(|o| o.get("mode"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Mode::Auto);
    let max_files = opts
        .and_then(|o| o.get("max_files"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(20);
    let max_visible = opts
        .and_then(|o| o.get("max_visible_tokens"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(engine.config.max_visible_tokens);

    let resp = if exact {
        engine.grep(pattern, &paths, mode, max_files, max_visible)
    } else {
        engine.find(pattern, &paths, mode, max_files, max_visible)
    };
    Ok(tool_response_to_value(&resp))
}

fn exec_glob(engine: &TokenZeroEngine, work_root: &Path, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let pattern = match args.first().and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.glob requires a pattern string as first argument",
                0,
            )));
        }
    };
    let paths: Vec<PathBuf> = match args.get(1) {
        Some(Value::String(s)) => vec![PathBuf::from(s)],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(PathBuf::from))
            .collect(),
        _ => vec![work_root.to_path_buf()],
    };
    let max_files = args
        .get(2)
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("max_files"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(200);

    let resp = engine.glob(
        pattern,
        &paths,
        false,
        Mode::Auto,
        max_files,
        engine.config.max_visible_tokens,
    );
    Ok(tool_response_to_value(&resp))
}

fn exec_tree(engine: &TokenZeroEngine, work_root: &Path, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let roots = vec![
        args.first()
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| work_root.to_path_buf()),
    ];
    let opts = args.get(1).and_then(|v| v.as_object());
    let depth = opts
        .and_then(|o| o.get("depth"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(3);
    let include_hidden = opts
        .and_then(|o| o.get("include_hidden"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let max_files = opts
        .and_then(|o| o.get("max_files"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(200);

    let resp = engine.tree(
        &roots,
        depth,
        include_hidden,
        Mode::Auto,
        max_files,
        engine.config.max_visible_tokens,
    );
    Ok(tool_response_to_value(&resp))
}

fn exec_shell(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let command = match args.first().and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.shell requires a command string as first argument",
                0,
            )));
        }
    };
    let opts = args.get(1).and_then(|v| v.as_object());
    let cwd = opts
        .and_then(|o| o.get("cwd"))
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
    let mode = opts
        .and_then(|o| o.get("mode"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Mode::Auto);
    let timeout = opts
        .and_then(|o| o.get("timeout_seconds"))
        .and_then(|v| v.as_u64())
        .map(Duration::from_secs);

    let resp = engine.shell(
        command,
        None,
        cwd.as_deref(),
        mode,
        Some("safe"),
        false,
        None,
        None,
        timeout,
    );

    let mut val = tool_response_to_value(&resp);
    if let Some(telem) = &resp.telemetry {
        if let Some(exit) = telem.get("exit_code") {
            val["exit_code"] = exit.clone();
        }
        if let Some(success) = telem.get("command_success") {
            val["success"] = success.clone();
        }
    }
    Ok(val)
}

pub(crate) fn exec_edit(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let path = match args.first().and_then(|v| v.as_str()) {
        Some(p) => PathBuf::from(p),
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.edit requires a path string as first argument",
                0,
            )));
        }
    };
    let edits_val = match args.get(1) {
        Some(Value::Array(arr)) => arr,
        _ => {
            return Err(Box::new(CodeModeResult::error(
                "zero.edit requires an array of {find, replace} hunks as second argument",
                0,
            )));
        }
    };
    let mut edits = Vec::with_capacity(edits_val.len());
    for (idx, value) in edits_val.iter().enumerate() {
        let hunk: EditHunk = serde_json::from_value(value.clone()).map_err(|err| {
            Box::new(CodeModeResult::error(
                format!("zero.edit: invalid hunk at index {idx}: {err}"),
                0,
            ))
        })?;
        edits.push(hunk);
    }
    if edits.is_empty() {
        return Err(Box::new(CodeModeResult::error(
            "zero.edit: no edit hunks provided",
            0,
        )));
    }
    let opts = args.get(2).and_then(|v| v.as_object());
    let dry_run = opts
        .and_then(|o| o.get("dry_run"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let create = opts
        .and_then(|o| o.get("create"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let resp = engine.edit(
        &path,
        &edits,
        create,
        dry_run,
        Mode::Auto,
        engine.config.max_visible_tokens,
    );
    let hunks_applied = if resp.status == "ok" {
        resp.telemetry
            .as_ref()
            .and_then(|t| t.get("hunks"))
            .and_then(|v| v.as_u64())
            .unwrap_or(edits.len() as u64)
    } else {
        0
    };
    let mut val = tool_response_to_value(&resp);
    val["hunks_applied"] = json!(hunks_applied);
    Ok(val)
}

fn exec_expand(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let ref_id = match args.first().and_then(|v| v.as_str()) {
        Some(r) => r,
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.token.expand/zero.expand requires a tz:// ref string as first argument",
                0,
            )));
        }
    };
    if !ref_id.starts_with("tz://") {
        return Err(Box::new(CodeModeResult::error(
            format!("zero.token.expand/zero.expand: ref must start with tz://, got: {ref_id}"),
            0,
        )));
    }
    let opts = args.get(1).and_then(|v| v.as_object());
    let start_line = opts
        .and_then(|o| o.get("start_line"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let end_line = opts
        .and_then(|o| o.get("end_line"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let selector = opts
        .and_then(|o| o.get("selector"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let resp = engine.expand(
        ref_id,
        selector.as_deref(),
        start_line,
        end_line,
        None,
        None,
    );
    Ok(tool_response_to_value(&resp))
}

fn exec_compact(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let data = match args.first() {
        Some(Value::String(s)) => s.clone(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.token.compact/zero.compact requires data as first argument",
                0,
            )));
        }
    };
    let content_type = detect_content_type(&data, None);
    let resp = engine.ingest(&data, content_type, Mode::Auto, "codemode-compact");
    let mut val = tool_response_to_value(&resp);
    val["raw_tokens"] = json!(count_tokens(&data));
    Ok(val)
}

fn exec_ingest(engine: &TokenZeroEngine, args: &[Value]) -> Result<Value, Box<CodeModeResult>> {
    let text = match args.first().and_then(|v| v.as_str()) {
        Some(t) => t,
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.ingest requires text as first argument",
                0,
            )));
        }
    };
    let opts = args.get(1).and_then(|v| v.as_object());
    let mode = opts
        .and_then(|o| o.get("mode"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Mode::Auto);
    let source = opts
        .and_then(|o| o.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("codemode-ingest");
    let content_type = detect_content_type(text, None);

    let resp = engine.ingest(text, content_type, mode, source);
    Ok(tool_response_to_value(&resp))
}

fn exec_mem(engine: &TokenZeroEngine) -> Result<Value, Box<CodeModeResult>> {
    let resp = engine.mem();
    Ok(tool_response_to_value(&resp))
}

fn collect_refs(value: &Value, refs: &mut Vec<String>) {
    if let Some(r) = value.get("ref").and_then(|v| v.as_str()) {
        if r.starts_with("tz://") && !refs.contains(&r.to_string()) {
            refs.push(r.to_string());
        }
    }
    if let Some(arr) = value.get("refs").and_then(|v| v.as_array()) {
        for r in arr.iter().filter_map(|v| v.as_str()) {
            if r.starts_with("tz://") && !refs.contains(&r.to_string()) {
                refs.push(r.to_string());
            }
        }
    }
}

fn result_visible_tokens(value: &Value) -> usize {
    value
        .get("visible_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize
}

fn result_raw_tokens(value: &Value) -> usize {
    value
        .get("raw_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize
}

