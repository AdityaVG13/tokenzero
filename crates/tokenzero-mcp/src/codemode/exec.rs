//! CodeMode plan executor and TokenZero operation dispatch.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokenzero_core::{Mode, ToolResponse, count_tokens, detect_content_type};
use tokenzero_filters::{discover, rewrite_command};

use crate::workspace::{
    allowed_roots_for_workspace, default_codemode_recovery_cache_path, tokenzero_work_root,
};
use crate::{EditHunk, EngineConfig, TokenZeroEngine, shell_timeout_from_secs};

use super::catalog::{describe_method, search_catalog};
use super::parser::{Expr, MethodCall, Statement, parse_plan, resolve_expr, resolve_return};
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

pub fn execute_codemode_with_options(plan: &str, options: CodeModeOptions) -> CodeModeResult {
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
                let outcome = match dispatch(&engine, &work_root, call, &scope) {
                    Ok(outcome) => outcome,
                    Err(mut e) => {
                        e.telemetry.operations = ops;
                        return *e;
                    }
                };
                record_outcome(&outcome, &mut all_refs, &mut total_visible, &mut total_raw);
                last_value = outcome.as_value().clone();
                scope.insert(name.clone(), outcome.into_value());
            }
            Statement::Call(call) => {
                ops += 1;
                let outcome = match dispatch(&engine, &work_root, call, &scope) {
                    Ok(outcome) => outcome,
                    Err(mut e) => {
                        e.telemetry.operations = ops;
                        return *e;
                    }
                };
                record_outcome(&outcome, &mut all_refs, &mut total_visible, &mut total_raw);
                last_value = outcome.into_value();
            }
            Statement::Return(expr) => {
                let value = match resolve_return(expr, &scope) {
                    Ok(value) => value,
                    Err(message) => return CodeModeResult::error(message, ops),
                };
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
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let method = call.method.as_str();
    let args: Vec<Value> = call
        .args
        .iter()
        .map(|arg| {
            resolve_expr(arg, scope).map_err(|message| Box::new(CodeModeResult::error(message, 0)))
        })
        .collect::<Result<Vec<_>, _>>()?;

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
        "zero.compact_max" | "compact_max" => exec_compact_max(engine, &args),
        "zero.ingest" | "ingest" => exec_ingest(engine, &args),
        "zero.mem" | "mem" => exec_mem(engine),
        "zero.recall" | "recall" => exec_recall(engine, &args),
        "zero.fetch" | "fetch" => exec_fetch(engine, &args),
        "zero.cache_pack" | "cache_pack" | "cache-pack" => exec_cache_pack(engine, &args),
        "zero.rewrite" | "rewrite" => exec_rewrite(&args),
        "zero.discover" | "discover" => exec_discover(),
        "zero.batch" | "batch" => exec_batch(engine, &args),
        "zero.pipe" | "pipe" => exec_pipe(engine, work_root, &args),
        "zero.pick" | "pick" => exec_pick(&args),
        "zero.filter_lines" | "filter_lines" => exec_filter_lines(&args),
        "zero.count_tokens" | "count_tokens" => exec_count_tokens(&args),
        "zero.assert" | "assert" => exec_assert(&args),
        "codemode.search" | "search" => {
            let query = args.first().and_then(|v| v.as_str()).unwrap_or("");
            Ok(OpOutcome::from_catalog(search_catalog(query)))
        }
        "codemode.describe" | "describe" => {
            let path = args.first().and_then(|v| v.as_str()).unwrap_or("");
            Ok(OpOutcome::from_catalog(describe_method(path)))
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

#[derive(Debug)]
pub(crate) struct OpOutcome {
    value: Value,
}

impl OpOutcome {
    fn from_tool_response(resp: &ToolResponse) -> Self {
        Self {
            value: tool_response_to_value(resp),
        }
    }

    fn from_catalog(value: Value) -> Self {
        Self { value }
    }

    pub(crate) fn into_value(self) -> Value {
        self.value
    }

    pub(crate) fn as_value(&self) -> &Value {
        &self.value
    }

    fn with_value(mut self, update: impl FnOnce(&mut Value)) -> Self {
        update(&mut self.value);
        self
    }

    fn visible_tokens(&self) -> usize {
        result_visible_tokens(&self.value)
    }

    fn raw_tokens(&self) -> usize {
        result_raw_tokens(&self.value)
    }
}

fn record_outcome(
    outcome: &OpOutcome,
    all_refs: &mut Vec<String>,
    total_visible: &mut usize,
    total_raw: &mut usize,
) {
    collect_refs(&outcome.value, all_refs);
    *total_visible += outcome.visible_tokens();
    *total_raw += outcome.raw_tokens();
}

struct Opts<'a>(Option<&'a serde_json::Map<String, Value>>);

impl<'a> Opts<'a> {
    fn from_arg(args: &'a [Value], index: usize) -> Self {
        Self(args.get(index).and_then(|v| v.as_object()))
    }

    fn usize(&self, key: &str) -> Option<usize> {
        self.0?.get(key)?.as_u64().map(|n| n as usize)
    }

    fn bool(&self, key: &str) -> Option<bool> {
        self.0?.get(key)?.as_bool()
    }

    fn str(&self, key: &str) -> Option<&str> {
        self.0?.get(key)?.as_str()
    }

    fn mode_or(&self, key: &str, default: Mode) -> Mode {
        self.str(key)
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    }
}

fn require_str_arg<'a>(
    args: &'a [Value],
    index: usize,
    message: &str,
) -> Result<&'a str, Box<CodeModeResult>> {
    args.get(index)
        .and_then(|value| value.as_str())
        .ok_or_else(|| Box::new(CodeModeResult::error(message.to_string(), 0)))
}

fn paths_from_arg(args: &[Value], index: usize, default: PathBuf) -> Vec<PathBuf> {
    match args.get(index) {
        Some(Value::String(path)) => vec![PathBuf::from(path)],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|value| value.as_str().map(PathBuf::from))
            .collect(),
        _ => vec![default],
    }
}

fn require_paths_from_arg(
    args: &[Value],
    index: usize,
    message: &str,
) -> Result<Vec<PathBuf>, Box<CodeModeResult>> {
    match args.get(index) {
        Some(Value::String(path)) => Ok(vec![PathBuf::from(path)]),
        Some(Value::Array(items)) => {
            let paths: Vec<PathBuf> = items
                .iter()
                .filter_map(|value| value.as_str().map(PathBuf::from))
                .collect();
            if paths.is_empty() {
                Err(Box::new(CodeModeResult::error(message.to_string(), 0)))
            } else {
                Ok(paths)
            }
        }
        _ => Err(Box::new(CodeModeResult::error(message.to_string(), 0))),
    }
}

fn exec_read(engine: &TokenZeroEngine, args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let paths = require_paths_from_arg(
        args,
        0,
        "zero.read requires a path string or array as first argument",
    )?;
    let opts = Opts::from_arg(args, 1);
    let mode = opts.mode_or("mode", Mode::Auto);
    let start_line = opts.usize("start_line");
    let end_line = opts.usize("end_line");
    let max_visible = opts
        .usize("max_visible_tokens")
        .unwrap_or(engine.config.max_visible_tokens);

    let resp = engine.read(&paths, mode, start_line, end_line, false, 20, max_visible);
    Ok(OpOutcome::from_tool_response(&resp))
}

fn exec_find(
    engine: &TokenZeroEngine,
    work_root: &Path,
    args: &[Value],
    exact: bool,
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let pattern = require_str_arg(
        args,
        0,
        "zero.find/grep requires a pattern string as first argument",
    )?;
    let paths = paths_from_arg(args, 1, work_root.to_path_buf());
    let opts = Opts::from_arg(args, 2);
    let mode = opts.mode_or("mode", Mode::Auto);
    let max_files = opts.usize("max_files").unwrap_or(20);
    let max_visible = opts
        .usize("max_visible_tokens")
        .unwrap_or(engine.config.max_visible_tokens);

    let resp = if exact {
        engine.grep(pattern, &paths, mode, max_files, max_visible)
    } else {
        engine.find(pattern, &paths, mode, max_files, max_visible)
    };
    Ok(OpOutcome::from_tool_response(&resp))
}

fn exec_glob(
    engine: &TokenZeroEngine,
    work_root: &Path,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let pattern = require_str_arg(
        args,
        0,
        "zero.glob requires a pattern string as first argument",
    )?;
    let paths = paths_from_arg(args, 1, work_root.to_path_buf());
    let max_files = Opts::from_arg(args, 2).usize("max_files").unwrap_or(200);

    let resp = engine.glob(
        pattern,
        &paths,
        false,
        Mode::Auto,
        max_files,
        engine.config.max_visible_tokens,
    );
    Ok(OpOutcome::from_tool_response(&resp))
}

fn exec_tree(
    engine: &TokenZeroEngine,
    work_root: &Path,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let roots = vec![
        args.first()
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| work_root.to_path_buf()),
    ];
    let opts = Opts::from_arg(args, 1);
    let depth = opts.usize("depth").unwrap_or(3);
    let include_hidden = opts.bool("include_hidden").unwrap_or(false);
    let max_files = opts.usize("max_files").unwrap_or(200);

    let resp = engine.tree(
        &roots,
        depth,
        include_hidden,
        Mode::Auto,
        max_files,
        engine.config.max_visible_tokens,
    );
    Ok(OpOutcome::from_tool_response(&resp))
}

fn exec_shell(engine: &TokenZeroEngine, args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let command = require_str_arg(
        args,
        0,
        "zero.shell requires a command string as first argument",
    )?;
    let opts = Opts::from_arg(args, 1);
    let cwd = opts.str("cwd").map(PathBuf::from);
    let mode = opts.mode_or("mode", Mode::Auto);
    let timeout = opts
        .usize("timeout_seconds")
        .map(|secs| Duration::from_secs(secs as u64));

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

    Ok(OpOutcome::from_tool_response(&resp).with_value(|value| {
        if let Some(telem) = &resp.telemetry {
            if let Some(exit) = telem.get("exit_code") {
                value["exit_code"] = exit.clone();
            }
            if let Some(success) = telem.get("command_success") {
                value["success"] = success.clone();
            }
        }
    }))
}

pub(crate) fn exec_edit(
    engine: &TokenZeroEngine,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let path = PathBuf::from(require_str_arg(
        args,
        0,
        "zero.edit requires a path string as first argument",
    )?);
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
    let opts = Opts::from_arg(args, 2);
    let dry_run = opts.bool("dry_run").unwrap_or(false);
    let create = opts.bool("create").unwrap_or(false);

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
    Ok(OpOutcome::from_tool_response(&resp).with_value(|value| {
        value["hunks_applied"] = json!(hunks_applied);
    }))
}

fn exec_expand(engine: &TokenZeroEngine, args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let ref_id = require_str_arg(
        args,
        0,
        "zero.token.expand/zero.expand requires a tz:// ref string as first argument",
    )?;
    if !ref_id.starts_with("tz://") {
        return Err(Box::new(CodeModeResult::error(
            format!("zero.token.expand/zero.expand: ref must start with tz://, got: {ref_id}"),
            0,
        )));
    }
    let opts = Opts::from_arg(args, 1);
    let start_line = opts.usize("start_line");
    let end_line = opts.usize("end_line");
    let selector = opts.str("selector").map(str::to_string);

    let resp = engine.expand(
        ref_id,
        selector.as_deref(),
        start_line,
        end_line,
        None,
        None,
    );
    Ok(OpOutcome::from_tool_response(&resp))
}

fn exec_compact(
    engine: &TokenZeroEngine,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    exec_compact_inner(engine, args, false)
}

fn exec_compact_max(
    engine: &TokenZeroEngine,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    exec_compact_inner(engine, args, true)
}

fn exec_compact_inner(
    engine: &TokenZeroEngine,
    args: &[Value],
    aggressive: bool,
) -> Result<OpOutcome, Box<CodeModeResult>> {
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
    let raw_tokens = count_tokens(&data);
    // Store for recovery first
    let mut store = tokenzero_recovery::RecoveryStore::new(Some(engine.config.cache_path.clone()));
    let stored = store
        .store_payload(&data, content_type, None, None, None)
        .ok();
    let recovery_ref = stored.as_ref().map(|s| s.blob_ref.as_str());
    // Use content-aware compression
    let capsule = tokenzero_core::make_capsule_content_aware(
        &data,
        raw_tokens,
        content_type,
        engine.config.max_visible_tokens,
        Some("compact"),
        recovery_ref,
        aggressive,
    );
    let mut refs_out = Vec::new();
    if let Some(s) = &stored {
        refs_out.push(tokenzero_core::ref_record(
            "blob",
            s.blob_ref.clone(),
            data.len(),
        ));
        refs_out.push(tokenzero_core::ref_record(
            "file",
            s.file_ref.clone(),
            data.len(),
        ));
    }
    let strategy = if aggressive {
        "content_aware_max"
    } else {
        "content_aware"
    };
    let ref_id = stored.as_ref().map(|s| s.blob_ref.as_str()).unwrap_or("");
    let mut value = json!({
        "text": capsule.text,
        "status": "ok",
        "raw_tokens": raw_tokens,
        "visible_tokens": capsule.visible_tokens,
        "compression_strategy": strategy,
        "savings_pct": format!("{:.0}%", tokenzero_core::savings_ratio(raw_tokens, capsule.visible_tokens) * 100.0),
    });
    if !ref_id.is_empty() {
        value["ref"] = json!(ref_id);
    }
    if refs_out.len() > 1 {
        value["refs"] = json!(refs_out.iter().map(|r| &r.ref_id).collect::<Vec<_>>());
    }
    Ok(OpOutcome { value })
}

fn exec_ingest(engine: &TokenZeroEngine, args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let text = require_str_arg(args, 0, "zero.ingest requires text as first argument")?;
    let opts = Opts::from_arg(args, 1);
    let mode = opts.mode_or("mode", Mode::Auto);
    let source = opts.str("source").unwrap_or("codemode-ingest");
    let content_type = detect_content_type(text, None);

    let resp = engine.ingest(text, content_type, mode, source);
    Ok(OpOutcome::from_tool_response(&resp))
}

fn exec_mem(engine: &TokenZeroEngine) -> Result<OpOutcome, Box<CodeModeResult>> {
    let resp = engine.mem();
    Ok(OpOutcome::from_tool_response(&resp))
}

fn exec_recall(engine: &TokenZeroEngine, args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let query = require_str_arg(
        args,
        0,
        "zero.recall requires a query string as first argument",
    )?;
    let opts = Opts::from_arg(args, 1);
    let mode = opts.mode_or("mode", Mode::Auto);
    let max_hits = opts.usize("max_hits").unwrap_or(50);
    let max_visible = opts
        .usize("max_visible_tokens")
        .unwrap_or(engine.config.max_visible_tokens);
    let resp = engine.recall(query, max_hits, mode, max_visible);
    Ok(OpOutcome::from_tool_response(&resp))
}

fn exec_fetch(engine: &TokenZeroEngine, args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let url = require_str_arg(
        args,
        0,
        "zero.fetch requires an http(s) URL as first argument",
    )?;
    let opts = Opts::from_arg(args, 1);
    let mode = opts.mode_or("mode", Mode::Auto);
    let ttl_seconds = opts.usize("ttl_seconds");
    let fresh = opts.bool("fresh").unwrap_or(false);
    let max_visible = opts
        .usize("max_visible_tokens")
        .unwrap_or(engine.config.max_visible_tokens);
    let resp = engine.fetch(url, ttl_seconds, fresh, mode, max_visible);
    Ok(OpOutcome::from_tool_response(&resp))
}

fn exec_cache_pack(
    engine: &TokenZeroEngine,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let opts = Opts::from_arg(args, 0);
    let scope = opts.str("scope").unwrap_or("agent");
    let resp = engine.cache_pack(scope);
    Ok(OpOutcome::from_tool_response(&resp))
}

fn exec_rewrite(args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let command = require_str_arg(
        args,
        0,
        "zero.rewrite requires a command string as first argument",
    )?;
    let opts = Opts::from_arg(args, 1);
    let mode = opts.str("mode").unwrap_or("safe");
    let value = serde_json::to_value(rewrite_command(command, mode, true)).map_err(|err| {
        Box::new(CodeModeResult::error(
            format!("zero.rewrite failed: {err}"),
            0,
        ))
    })?;
    Ok(OpOutcome::from_catalog(value))
}

fn exec_discover() -> Result<OpOutcome, Box<CodeModeResult>> {
    Ok(OpOutcome::from_catalog(
        serde_json::to_value(discover()).unwrap_or(Value::Null),
    ))
}

fn exec_batch(engine: &TokenZeroEngine, args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let ops = match args.first() {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::String(text)) => serde_json::from_str(text).map_err(|err| {
            Box::new(CodeModeResult::error(
                format!("zero.batch ops is not valid JSON: {err}"),
                0,
            ))
        })?,
        _ => {
            return Err(Box::new(CodeModeResult::error(
                "zero.batch requires an array of {tool, args} objects as first argument",
                0,
            )));
        }
    };
    if ops.is_empty() {
        return Err(Box::new(CodeModeResult::error(
            "zero.batch requires at least one op",
            0,
        )));
    }
    let wrapped = json!({"ops": ops, "mode": "auto"});
    match crate::tools::batch_response(engine, &wrapped) {
        Ok(resp) => Ok(OpOutcome::from_tool_response(&resp)),
        Err(error) => Err(Box::new(CodeModeResult::error(error.message_text(), 0))),
    }
}

// ─── Composition built-ins ──────────────────────────────────────────────────

/// Execute a sequence of operations, threading each result into `_prev`.
/// Input: array of {method: string, args?: array} objects.
/// Returns: array of operation results in order.
fn exec_pipe(
    engine: &TokenZeroEngine,
    work_root: &Path,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let steps = match args.first() {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::String(text)) => serde_json::from_str(text).map_err(|err| {
            Box::new(CodeModeResult::error(
                format!("zero.pipe: steps is not valid JSON array: {err}"),
                0,
            ))
        })?,
        _ => {
            return Err(Box::new(CodeModeResult::error(
                "zero.pipe requires an array of {method, args} steps",
                0,
            )));
        }
    };
    if steps.is_empty() {
        return Err(Box::new(CodeModeResult::error(
            "zero.pipe requires at least one step",
            0,
        )));
    }
    let mut results: Vec<Value> = Vec::with_capacity(steps.len());
    let mut pipe_scope: HashMap<String, Value> = HashMap::new();
    for (idx, step) in steps.iter().enumerate() {
        let method = step.get("method").and_then(|v| v.as_str()).ok_or_else(|| {
            Box::new(CodeModeResult::error(
                format!("zero.pipe: step {idx} missing 'method' string"),
                0,
            ))
        })?;
        let step_args: Vec<Expr> = match step.get("args") {
            Some(Value::Array(arr)) => arr
                .iter()
                .map(|v| match v {
                    Value::String(s) if s == "_prev" => Expr::VarRef("_prev".to_string()),
                    _ => Expr::StringLit(serde_json::to_string(v).unwrap_or_default()),
                })
                .collect(),
            _ => Vec::new(),
        };
        let call = MethodCall {
            method: method.to_string(),
            args: step_args,
        };
        let outcome = dispatch(engine, work_root, &call, &pipe_scope)?;
        let val = outcome.into_value();
        pipe_scope.insert("_prev".to_string(), val.clone());
        pipe_scope.insert(format!("_step{idx}"), val.clone());
        results.push(val);
    }
    Ok(OpOutcome::from_catalog(json!({
        "steps": results.len(),
        "results": results,
        "last": results.last().cloned().unwrap_or(Value::Null),
    })))
}

/// Extract specific keys from an object value.
/// Args: (value_or_var, keys_array) or (value_or_var, "key1", "key2", ...)
fn exec_pick(args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let source = args.first().ok_or_else(|| {
        Box::new(CodeModeResult::error(
            "zero.pick requires a source object as first argument",
            0,
        ))
    })?;
    let obj = source.as_object().ok_or_else(|| {
        Box::new(CodeModeResult::error(
            "zero.pick: first argument must be an object",
            0,
        ))
    })?;
    let keys: Vec<&str> = match args.get(1) {
        Some(Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str()).collect(),
        Some(Value::String(k)) => {
            let mut ks = vec![k.as_str()];
            for arg in args.iter().skip(2) {
                if let Some(s) = arg.as_str() {
                    ks.push(s);
                }
            }
            ks
        }
        _ => {
            return Err(Box::new(CodeModeResult::error(
                "zero.pick: second argument must be an array of keys or key strings",
                0,
            )));
        }
    };
    let picked: serde_json::Map<String, Value> = keys
        .into_iter()
        .filter_map(|key| obj.get(key).map(|v| (key.to_string(), v.clone())))
        .collect();
    Ok(OpOutcome::from_catalog(Value::Object(picked)))
}

/// Filter lines in a text value by a substring pattern.
/// Args: (value_with_text_field, pattern) or (string_value, pattern)
fn exec_filter_lines(args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let source = args.first().ok_or_else(|| {
        Box::new(CodeModeResult::error(
            "zero.filter_lines requires a source as first argument",
            0,
        ))
    })?;
    let text = source
        .as_str()
        .or_else(|| source.get("text").and_then(|v| v.as_str()))
        .unwrap_or("");
    let pattern = args.get(1).and_then(|v| v.as_str()).ok_or_else(|| {
        Box::new(CodeModeResult::error(
            "zero.filter_lines requires a pattern string as second argument",
            0,
        ))
    })?;
    let filtered: Vec<&str> = text.lines().filter(|line| line.contains(pattern)).collect();
    Ok(OpOutcome::from_catalog(json!({
        "text": filtered.join("\n"),
        "lines": filtered.len(),
        "pattern": pattern,
    })))
}

fn exec_count_tokens(args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let text = match args.first() {
        Some(Value::String(s)) => s.clone(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
        None => {
            return Err(Box::new(CodeModeResult::error(
                "zero.count_tokens requires data as first argument",
                0,
            )));
        }
    };
    let tokens = count_tokens(&text);
    Ok(OpOutcome::from_catalog(json!({
        "tokens": tokens,
        "bytes": text.len(),
        "lines": text.lines().count(),
    })))
}

fn exec_assert(args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let condition = args.first().and_then(|v| v.as_bool()).unwrap_or_else(|| {
        // Truthy: non-null, non-false, non-empty-string, non-zero
        match args.first() {
            Some(Value::Null) | None => false,
            Some(Value::Bool(b)) => *b,
            Some(Value::String(s)) => !s.is_empty(),
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
            Some(Value::Array(a)) => !a.is_empty(),
            Some(Value::Object(o)) => !o.is_empty(),
        }
    });
    let message = args
        .get(1)
        .and_then(|v| v.as_str())
        .unwrap_or("assertion failed");
    if condition {
        Ok(OpOutcome::from_catalog(json!({ "ok": true })))
    } else {
        Err(Box::new(CodeModeResult::error(message.to_string(), 0)))
    }
}

// ─── Utilities ──────────────────────────────────────────────────────────────

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
