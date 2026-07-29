//! Transport-neutral domain kernel (tokenzero-irx9.2).
//!
//! One in-process implementation of every registry domain operation.
//! Adapters must call [`crate::dispatcher::dispatch_operation`]; they must not
//! re-implement auth/root/mutation/ref/telemetry here or below.

use crate::expand_params::ExpandParams;
use crate::{
    EditHunk, ServeOptions, TokenZeroEngine, annotate_write_failure, shell_timeout_from_millis,
    shell_timeout_from_secs,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokenzero_core::operation_abi::resolve_operation;
use tokenzero_core::{
    Accounting, ChannelSeparation, ContentType, Mode, ToolResponse, count_tokens,
    detect_content_type, shell_display_command_from_argv_for_platform,
};
use tokenzero_filters::{discover, rewrite_command};
use tokenzero_runtime::{ExecutionMode, plan_command_for_platform};

/// Domain-kernel dispatch errors (no JSON-RPC / MCP framing).
#[derive(Debug, Clone)]
pub enum DomainDispatchError {
    UnknownTool(String),
    InvalidArgs {
        op: String,
        message: String,
    },
    /// Adapter-owned control/composition/resource ops must not enter the kernel.
    TransportOnly(String),
}

impl DomainDispatchError {
    pub fn message_text(&self) -> String {
        match self {
            Self::UnknownTool(name) => format!("unknown tool: {name}"),
            Self::InvalidArgs { message, .. } => message.clone(),
            Self::TransportOnly(name) => {
                format!("{name} is transport-control only; not a domain engine op")
            }
        }
    }
}

/// Execute one canonical domain operation without transport framing.
pub fn execute_domain_op(
    engine: &TokenZeroEngine,
    op_name: &str,
    args: &Value,
) -> Result<ToolResponse, DomainDispatchError> {
    let op = resolve_operation(op_name)
        .ok_or_else(|| DomainDispatchError::UnknownTool(op_name.to_string()))?;
    if !crate::dispatcher::operation_is_domain(op) {
        return Err(DomainDispatchError::TransportOnly(op.name.to_string()));
    }
    let canonical = op.name;
    let bare = canonical.strip_prefix("tz_").unwrap_or(canonical);
    // Legacy compact alias maps to ingest kernel path.
    let bare = if op_name == "compact" { "ingest" } else { bare };

    let map_args = |message: String| DomainDispatchError::InvalidArgs {
        op: canonical.to_string(),
        message,
    };

    let response = match bare {
        "read" => engine.read_with_options(
            &arg_path_list(args, "path").map_err(map_args)?,
            arg_mode(args),
            arg_u64(args, "start_line"),
            arg_u64(args, "end_line"),
            arg_bool(args, "raw"),
            arg_u64_or(args, "max_files", 20),
            arg_u64_or(args, "max_visible_tokens", 4000),
            arg_serve_options(args),
        ),
        "find" => {
            let query = arg_string_any(args, &["query", "pattern"]).map_err(map_args)?;
            let path = arg_path_list(args, "path").unwrap_or_else(|_| vec![PathBuf::from(".")]);
            engine.find_with_options(
                query,
                &path,
                arg_mode(args),
                arg_u64_or(args, "max_files", 20),
                arg_u64_or(args, "max_visible_tokens", 4000),
                arg_serve_options(args),
            )
        }
        "grep" => {
            let query = arg_string_any(args, &["query", "pattern"]).map_err(map_args)?;
            let path = arg_path_list(args, "path").unwrap_or_else(|_| vec![PathBuf::from(".")]);
            engine.grep_with_options(
                query,
                &path,
                arg_mode(args),
                arg_u64_or(args, "max_files", 20),
                arg_u64_or(args, "max_visible_tokens", 4000),
                arg_serve_options(args),
            )
        }
        "recall" => engine.recall(
            arg_string_any(args, &["query", "pattern"]).map_err(map_args)?,
            arg_u64_or(args, "max_hits", 50),
            arg_mode(args),
            arg_u64_or(args, "max_visible_tokens", 4000),
        ),
        "glob" => engine.glob(
            arg_string_any(args, &["pattern", "glob", "query"]).map_err(map_args)?,
            &arg_paths_or_dot(args),
            arg_bool(args, "include_hidden"),
            arg_mode(args),
            arg_u64_or(args, "max_files", 200),
            arg_u64_or(args, "max_visible_tokens", 4000),
        ),
        "tree" => engine.tree(
            &arg_paths_or_dot(args),
            arg_u64_or(args, "depth", 2),
            arg_bool(args, "include_hidden"),
            arg_mode(args),
            arg_u64_or(args, "max_files", 200),
            arg_u64_or(args, "max_visible_tokens", 4000),
        ),
        "edit" => {
            let path = arg_string_any(args, &["path"]).map_err(map_args)?;
            let mut response = engine.edit(
                Path::new(path),
                &arg_edit_hunks(args).map_err(map_args)?,
                arg_bool(args, "create"),
                arg_bool(args, "dry_run"),
                arg_mode(args),
                arg_u64_or(args, "max_visible_tokens", 4000),
            );
            if response.status == "error" {
                if let Some(error) = response.error.as_mut() {
                    error.message = annotate_write_failure(&error.message, false);
                }
            }
            response
        }
        "shell" => {
            let (command, argv) = arg_command(args).map_err(map_args)?;
            let env = arg_env_map(args);
            engine.shell(
                &command,
                argv,
                arg_str(args, "cwd").map(Path::new),
                arg_mode(args),
                arg_str(args, "rewrite"),
                arg_bool(args, "no_rewrite"),
                env,
                arg_str(args, "stdin"),
                arg_shell_timeout(args),
            )
        }
        "ingest" => {
            let text = arg_string_any(args, &["text", "input"]).map_err(map_args)?;
            let tool = if op_name == "compact" {
                "compact"
            } else {
                arg_str(args, "source").unwrap_or("mcp-ingest")
            };
            let kind = content_type_from_arg(args, text);
            engine.ingest(text, kind, arg_mode(args), tool)
        }
        "expand" => {
            engine.expand_with_params(ExpandParams::from_tool_args(args).map_err(map_args)?)
        }
        "mem" => engine.mem(),
        "cache_pack" => engine.cache_pack(arg_str(args, "scope").unwrap_or("agent")),
        "rewrite" => {
            let (command, _) = arg_command(args).map_err(map_args)?;
            pretty_json_response(
                "rewrite",
                Mode::Hybrid,
                &rewrite_command(&command, arg_str(args, "mode").unwrap_or("safe"), true),
                Some(count_tokens(&command)),
            )
        }
        "discover" => pretty_json_response("discover", Mode::Hybrid, &discover(), None),
        "report_tool_issue" => {
            let tool = arg_string_any(args, &["tool", "name", "tool_name", "surface"])
                .map_err(map_args)?;
            let summary =
                arg_string_any(args, &["summary", "message", "title"]).map_err(map_args)?;
            let detail = arg_string_any(args, &["detail", "body", "repro", "context"])
                .ok()
                .or(Some(summary));
            match crate::record_tool_issue(
                &engine.config.cache_path,
                tool,
                summary,
                detail,
                Some(engine.session_id()),
            ) {
                Ok(report) => {
                    pretty_json_response("report_tool_issue", Mode::Structured, &report, None)
                }
                Err(message) => ToolResponse::error(
                    "report_tool_issue",
                    "not_reportable",
                    message,
                    Some("use tool=zero_execute (or tz_execute_code / zero.token.*) for CodeMode failures".into()),
                ),
            }
        }
        "batch" => {
            batch_response(engine, args).map_err(|message| DomainDispatchError::InvalidArgs {
                op: "tz_batch".into(),
                message,
            })?
        }
        "fetch" => engine.fetch(
            arg_string_any(args, &["url", "uri"]).map_err(map_args)?,
            arg_u64(args, "ttl_seconds"),
            arg_bool(args, "fresh"),
            arg_mode(args),
            arg_u64_or(args, "max_visible_tokens", 4000),
        ),
        other => {
            return Err(DomainDispatchError::UnknownTool(format!(
                "{canonical} (bare={other})"
            )));
        }
    };
    Ok(attach_channels(response, bare, args))
}

/// vz89.11: attach the opt-in machine-action channel. Gate off leaves the
/// response untouched, so default serialization stays byte-identical.
fn attach_channels(response: ToolResponse, bare: &str, args: &Value) -> ToolResponse {
    attach_channels_gated(
        response,
        bare,
        args,
        tokenzero_core::channel_separation_enabled(),
    )
}

/// Pure core of the gate so tests can drive both directions without touching
/// process env (the engine crate forbids unsafe env mutation).
fn attach_channels_gated(
    mut response: ToolResponse,
    bare: &str,
    args: &Value,
    enabled: bool,
) -> ToolResponse {
    if !enabled {
        return response;
    }
    response.channels = Some(ChannelSeparation {
        action: bare.to_string(),
        status_line: channel_status_line(bare, args),
        user_message: None,
    });
    response
}

#[cfg(test)]
mod channel_tests {
    use super::*;

    #[test]
    fn channels_gate_off_leaves_response_byte_identical() {
        let response = ToolResponse::default();
        let before = serde_json::to_string(&response).unwrap();
        let after =
            attach_channels_gated(response, "read", &json!({"path": ["src/main.rs"]}), false);
        assert!(after.channels.is_none());
        assert_eq!(serde_json::to_string(&after).unwrap(), before);
    }

    #[test]
    fn channels_gate_on_attaches_action_status_and_null_user_message() {
        let response = attach_channels_gated(
            ToolResponse::default(),
            "read",
            &json!({"path": ["src/main.rs"]}),
            true,
        );
        let channels = response.channels.as_ref().expect("channels attached");
        assert_eq!(channels.action, "read");
        assert_eq!(channels.status_line, "Reading src/main.rs");
        assert_eq!(channels.user_message, None);
        let serialized = serde_json::to_value(&response).unwrap();
        let user_message = serialized
            .get("channels")
            .and_then(|c| c.get("user_message"));
        assert!(
            user_message.is_some(),
            "nullable user_message key must serialize, not be skipped"
        );
        assert_eq!(user_message, Some(&Value::Null));
    }

    #[test]
    fn status_lines_are_deterministic_per_op() {
        let shell = attach_channels_gated(
            ToolResponse::default(),
            "shell",
            &json!({"command": "cargo test -p foo"}),
            true,
        );
        assert_eq!(
            shell.channels.unwrap().status_line,
            "Running cargo test -p foo"
        );
        let expand = attach_channels_gated(
            ToolResponse::default(),
            "expand",
            &json!({"ref": "tz://blob/ab12"}),
            true,
        );
        assert_eq!(
            expand.channels.unwrap().status_line,
            "Expanding tz://blob/ab12"
        );
        let glob = attach_channels_gated(
            ToolResponse::default(),
            "glob",
            &json!({"pattern": "**/*.rs"}),
            true,
        );
        assert_eq!(glob.channels.unwrap().status_line, "Globbing **/*.rs");
    }
}

/// Deterministic harness-renderable status line derived from the operation
/// and its arguments; no model prose involved (vz89.11).
fn channel_status_line(bare: &str, args: &Value) -> String {
    fn clip(text: &str, max: usize) -> String {
        let mut out: String = text.chars().take(max).collect();
        if text.chars().count() > max {
            out.push('…');
        }
        out
    }
    fn str_arg<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a str> {
        keys.iter()
            .find_map(|key| args.get(*key).and_then(Value::as_str))
    }
    let paths = args.get("path").and_then(|value| match value {
        Value::String(single) => Some(clip(single, 80)),
        Value::Array(items) => {
            let joined = items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            (!joined.is_empty()).then(|| clip(&joined, 80))
        }
        _ => None,
    });
    let query = str_arg(args, &["query", "pattern"]).map(|text| clip(text, 60));
    match bare {
        "read" => format!("Reading {}", paths.unwrap_or_else(|| "file".into())),
        "find" => format!("Finding {}", query.unwrap_or_default()),
        "grep" => format!("Searching for {}", query.unwrap_or_default()),
        "glob" => format!(
            "Globbing {}",
            str_arg(args, &["pattern", "glob", "query"])
                .map(|text| clip(text, 60))
                .unwrap_or_default()
        ),
        "tree" => format!("Listing {}", paths.unwrap_or_else(|| ".".into())),
        "edit" => format!("Editing {}", paths.unwrap_or_default()),
        "shell" => {
            let command = str_arg(args, &["command"]).map(str::to_string).or_else(|| {
                args.get("argv").and_then(Value::as_array).map(|argv| {
                    argv.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
            });
            format!(
                "Running {}",
                command.map(|c| clip(&c, 80)).unwrap_or_default()
            )
        }
        "ingest" => "Storing payload".to_string(),
        "expand" => format!("Expanding {}", str_arg(args, &["ref"]).unwrap_or("ref")),
        "mem" => "Inspecting recovery cache".to_string(),
        "cache_pack" => "Building cache pack".to_string(),
        "rewrite" => "Planning rewrite".to_string(),
        "discover" => "Discovering capabilities".to_string(),
        "fetch" => format!(
            "Fetching {}",
            str_arg(args, &["url", "uri"])
                .map(|text| clip(text, 80))
                .unwrap_or_default()
        ),
        "report_tool_issue" => "Reporting tool issue".to_string(),
        "batch" => "Running batch ops".to_string(),
        other => format!("Running {other}"),
    }
}

pub fn batch_response(engine: &TokenZeroEngine, args: &Value) -> Result<ToolResponse, String> {
    let ops = batch_ops(args)?;
    let mut sections = Vec::with_capacity(ops.len());
    let mut refs: Vec<tokenzero_core::RefRecord> = Vec::new();
    let mut listed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut raw_tokens = 0usize;
    let mut recovery_tokens = 0usize;
    let mut per_op = Vec::with_capacity(ops.len());
    for (index, (tool, op_args)) in ops.iter().enumerate() {
        let canonical = tool
            .strip_prefix("tz_")
            .map(|_| tool.as_str())
            .unwrap_or(tool.as_str());
        let position = index + 1;
        if canonical == "batch" || canonical == "tz_batch" {
            sections.push(format!(
                "## {position} {tool} — error: nested batch is not allowed"
            ));
            per_op.push(json!({"tool": tool, "status": "error", "code": "nested_batch"}));
            continue;
        }
        // Sub-ops go through the shared domain kernel (not MCP framing).
        match execute_domain_op(engine, tool, op_args) {
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

fn pretty_json_response(
    tool: &str,
    mode: Mode,
    value: &impl serde::Serialize,
    raw_tokens: Option<usize>,
) -> ToolResponse {
    let text = serde_json::to_string_pretty(value).unwrap_or_default();
    let tokens = raw_tokens.unwrap_or_else(|| count_tokens(&text));
    inline_response(tool, mode, text, tokens)
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

fn arg_bool(args: &Value, key: &str) -> bool {
    args.get(key).is_some_and(|value| match value {
        Value::Bool(value) => *value,
        Value::String(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes"
        ),
        _ => false,
    })
}

fn arg_u64(args: &Value, key: &str) -> Option<usize> {
    coerce_u64(args.get(key)?).and_then(|value| usize::try_from(value).ok())
}

fn arg_u64_or(args: &Value, key: &str, default: usize) -> usize {
    arg_u64(args, key).unwrap_or(default)
}

fn arg_paths_or_dot(args: &Value) -> Vec<PathBuf> {
    arg_path_list(args, "path").unwrap_or_else(|_| vec![PathBuf::from(".")])
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

/// Shell deadline spellings accepted in milliseconds.
const SHELL_TIMEOUT_MS_KEYS: &[&str] = &["timeout_ms", "timeoutMs", "shell_timeout_ms"];

/// Shell deadline spellings accepted in seconds.
const SHELL_TIMEOUT_SECS_KEYS: &[&str] = &[
    "timeout_seconds",
    "timeout_secs",
    "timeout",
    "shell_timeout_seconds",
];

/// Resolves a shell deadline from any accepted spelling, in either unit.
///
/// `timeout_ms` was previously not among the keys consulted here, so callers
/// that spelled the deadline in milliseconds had it silently discarded: the
/// command ran to completion under the default 60s timeout and was reported as
/// a success. Measured before this fix, a `{ timeout_ms: 1000 }` request ran
/// 8048ms and returned status `ok`. Milliseconds are checked first because they
/// are the more precise unit, so a caller passing both gets the tighter bound
/// rather than a unit-dependent coin flip.
fn arg_shell_timeout(args: &Value) -> Option<Duration> {
    SHELL_TIMEOUT_MS_KEYS
        .iter()
        .find_map(|key| {
            args.get(*key)
                .and_then(coerce_u64)
                .map(|millis| shell_timeout_from_millis(Some(millis)))
        })
        .or_else(|| arg_timeout_any(args, SHELL_TIMEOUT_SECS_KEYS))
}

fn arg_command(args: &Value) -> Result<(String, Option<Vec<String>>), String> {
    if let Some(value) = args.as_str() {
        return Ok((value.to_string(), None));
    }
    if let Some(items) = args.as_array() {
        let argv = string_array_arg(items, "argv")?;
        return Ok((display_command_for_argv(&argv), Some(argv)));
    }
    // Prefer structured argv when present so CLI/runtime plan fidelity is preserved.
    if let Some((key, items)) = ["argv", "args"].into_iter().find_map(|key| {
        args.get(key)
            .and_then(Value::as_array)
            .map(|items| (key, items))
    }) {
        let argv = string_array_arg(items, key)?;
        let display = arg_string_any(args, &["command", "cmd", "input", "script"])
            .map(|s| s.to_string())
            .unwrap_or_else(|_| display_command_for_argv(&argv));
        return Ok((display, Some(argv)));
    }
    if let Ok(command) = arg_string_any(args, &["command", "cmd", "input", "script"]) {
        return Ok((command.to_string(), None));
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
    let value = args.get(key).ok_or_else(|| format!("missing {key}"))?;
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
        return Ok(string_array_arg(items, key)?
            .into_iter()
            .map(PathBuf::from)
            .collect());
    }
    Err(format!("invalid {key}"))
}

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

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn arg_string_any<'a>(args: &'a Value, keys: &[&str]) -> Result<&'a str, String> {
    keys.iter()
        .find_map(|key| args.get(*key).and_then(Value::as_str))
        .ok_or_else(|| format!("missing {}", keys.join("|")))
}

fn arg_env_map(args: &Value) -> Option<std::collections::BTreeMap<String, String>> {
    let obj = args.get("env")?.as_object()?;
    let mut out = std::collections::BTreeMap::new();
    for (k, v) in obj {
        if let Some(s) = v.as_str() {
            out.insert(k.clone(), s.to_string());
        }
    }
    Some(out)
}

fn content_type_from_arg(args: &Value, text: &str) -> ContentType {
    match arg_str(args, "content_type").unwrap_or("unknown") {
        "code" => ContentType::Code,
        "shell" | "tool-output" | "shell_output" => ContentType::ShellOutput,
        "diff" => ContentType::Diff,
        "json" | "json_config" => ContentType::JsonConfig,
        "markdown" | "pack" => ContentType::Markdown,
        "log" | "logs" => ContentType::Logs,
        "search_result" => ContentType::SearchResult,
        "tree" => ContentType::Tree,
        "unknown" => ContentType::Unknown,
        _ => detect_content_type(text, None),
    }
}
