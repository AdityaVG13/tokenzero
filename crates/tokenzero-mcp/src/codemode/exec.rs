//! CodeMode plan executor and TokenZero operation dispatch.

use rquickjs::{Context, Runtime, function::Func};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokenzero_core::{Mode, ToolResponse, count_tokens, detect_content_type};
use tokenzero_filters::{discover, rewrite_command};

use crate::workspace::{
    allowed_roots_for_workspace, resolve_recovery_cache_path, tokenzero_work_root,
};
use crate::{EditHunk, EngineConfig, TokenZeroEngine, shell_timeout_from_secs};

use super::catalog::{describe_method, search_catalog};
use super::journal::{
    BeginOutcome, JournalOperation, JournalState, JournalTransaction, OperationClass,
    OperationSpec, atomic_write as journal_atomic_write, begin_plan, classify_method,
    current_digest, doctor_json as journal_doctor_json, inspect as inspect_journal,
    open_unresolved, sha256_bytes,
};
use super::parser::{Expr, MethodCall, Statement, parse_plan, resolve_expr, resolve_return};
use super::result::{CodeModeOptions, CodeModeResult, CodeModeStatus};
use super::sandbox::lower_code_plan;
use super::store::{
    CodeModeLimits, ExecutionStep, ExecutionStore, execution_id, finalize_result, now_ms,
};
use crate::expand_params::ExpandParams;

const EXACT_EXPAND_MARKER: &str = "__tz_exact_expand";

/// Per-session previous visible output keyed by resolved recovery cache path.
/// Used to estimate prefix-cache hits when the provider does not expose one.
static PREVIOUS_OUTPUT_BY_SESSION: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();

fn previous_output_by_session() -> &'static Mutex<HashMap<PathBuf, String>> {
    PREVIOUS_OUTPUT_BY_SESSION.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Approximate bytes per token for prevented-read counterfactuals. Used
/// only when the visible text gives no better per-token byte ratio.
const PREVENTED_READ_BYTES_PER_TOKEN: usize = 4;

thread_local! {
    /// Payload identity registry for the current execution: hashes of exact
    /// expand payloads (raw text and, when the payload parses as JSON, the
    /// canonical serialization). Values matching an entry are exempt from
    /// ref-first compaction so "explicit expand ALWAYS returns exact bytes"
    /// (dda8627) survives the JS-side envelope unwrap.
    static EXACT_EXPAND_REGISTRY: std::cell::RefCell<std::collections::HashSet<u64>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

fn exact_expand_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn record_exact_expand_payload(text: &str) {
    EXACT_EXPAND_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        registry.insert(exact_expand_hash(text));
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            if let Ok(canonical) = serde_json::to_string(&parsed) {
                registry.insert(exact_expand_hash(&canonical));
            }
        }
    });
}

pub(crate) fn is_exact_expand_value(value: &Value) -> bool {
    let key = match value {
        Value::String(text) => exact_expand_hash(text),
        Value::Object(_) | Value::Array(_) => match serde_json::to_string(value) {
            Ok(canonical) => exact_expand_hash(&canonical),
            Err(_) => return false,
        },
        _ => return false,
    };
    EXACT_EXPAND_REGISTRY.with(|registry| registry.borrow().contains(&key))
}

#[cfg(test)]
pub(crate) fn make_engine_for_root(root: PathBuf) -> TokenZeroEngine {
    make_engine_for_root_with_options(root, &CodeModeOptions::default())
}

fn make_engine_for_root_with_options(root: PathBuf, options: &CodeModeOptions) -> TokenZeroEngine {
    // Same default store as CLI expand / MCP (wqw.8): refs minted by codemode
    // must expand on the next call without re-running the producer. Explicit
    // --cache-path / CodeModeOptions.cache_path still override.
    let cache_path = options.cache_path.clone().map_or_else(
        || resolve_recovery_cache_path(&root, None),
        |path| resolve_recovery_cache_path(&root, Some(path)),
    );
    let config = EngineConfig {
        allowed_roots: allowed_roots_for_workspace(&root, &options.allowed_roots),
        cache_path,
        max_visible_tokens: options.max_visible_tokens,
        mode: Mode::Auto,
        shell_timeout: shell_timeout_from_secs(options.timeout_seconds),
        ..EngineConfig::for_root(&root)
    };
    // Share session crash-only health when the MCP call path provided one so
    // expand X0 inside a plan unlocks tz_expand on the same gate (wqw.9).
    match &options.surface_health {
        Some(health) => {
            TokenZeroEngine::with_shared_surface_health(config, std::sync::Arc::clone(health))
        }
        None => TokenZeroEngine::new(config),
    }
}

#[cfg(test)]
pub fn execute_codemode(plan: &str) -> CodeModeResult {
    let thread_id = format!("{:?}", std::thread::current().id())
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    execute_codemode_with_options(
        plan,
        CodeModeOptions {
            cache_path: Some(
                std::env::temp_dir().join(format!("tokenzero-codemode-test-{thread_id}.json")),
            ),
            ..CodeModeOptions::default()
        },
    )
}

pub fn execute_codemode_with_options(plan: &str, options: CodeModeOptions) -> CodeModeResult {
    let containment_options = options.clone();
    super::containment::execute(plan, &containment_options, move || {
        execute_codemode_uncontained(plan, options)
    })
}

fn execute_codemode_uncontained(plan: &str, options: CodeModeOptions) -> CodeModeResult {
    EXACT_EXPAND_REGISTRY.with(|registry| registry.borrow_mut().clear());
    let plan = plan.trim();
    let started_ms = now_ms();
    let limits = limits_from_options(&options);

    if plan.len() > limits.max_code_bytes {
        return finalize_codemode_result(
            CodeModeResult::error_with_kind(
                "validation",
                format!("plan exceeds max_code_bytes {}", limits.max_code_bytes),
                0,
                false,
            ),
            "code",
            plan,
            started_ms,
            &options,
            &limits,
            Vec::new(),
        );
    }

    if plan.is_empty() {
        return finalize_codemode_result(
            CodeModeResult::error("empty plan", 0),
            "code",
            plan,
            started_ms,
            &options,
            &limits,
            Vec::new(),
        );
    }
    if let Some(query) = plan.strip_prefix("search:") {
        let result = search_catalog(query.trim());
        let text = serde_json::to_string_pretty(&result).unwrap_or_default();
        let tokens = count_tokens(&text);
        return finalize_codemode_result(
            CodeModeResult::completed(result, Vec::new(), 1, tokens, tokens),
            "recipe",
            plan,
            started_ms,
            &options,
            &limits,
            vec![ExecutionStep {
                id: "search".to_string(),
                method: "codemode.search".to_string(),
                status: "completed".to_string(),
                refs: Vec::new(),
            }],
        );
    }
    if let Some(target) = plan.strip_prefix("describe:") {
        let result = describe_method(target.trim());
        let text = serde_json::to_string_pretty(&result).unwrap_or_default();
        let tokens = count_tokens(&text);
        return finalize_codemode_result(
            CodeModeResult::completed(result, Vec::new(), 1, tokens, tokens),
            "recipe",
            plan,
            started_ms,
            &options,
            &limits,
            vec![ExecutionStep {
                id: "describe".to_string(),
                method: "codemode.describe".to_string(),
                status: "completed".to_string(),
                refs: Vec::new(),
            }],
        );
    }

    if let Some(recipe) = lower_recipe_plan(plan) {
        return execute_lowered_plan(&recipe, options, &limits, "recipe", started_ms);
    }
    if plan.starts_with('{') || plan.starts_with('[') {
        return execute_json_plan(plan, options, &limits, started_ms);
    }

    let lowered = match lower_code_plan(plan, &limits) {
        Ok(lowered) => lowered,
        Err(message) => {
            return finalize_codemode_result(
                CodeModeResult::error(message, 0),
                "code",
                plan,
                started_ms,
                &options,
                &limits,
                Vec::new(),
            );
        }
    };
    // The lowered mini-interpreter only understands a small statement grammar;
    // anything it cannot FULLY parse (optional chaining, ??, computed calls)
    // must run in the real QuickJS sandbox instead of degrading — the old
    // lenient parser turned unknown expressions into source-text strings.
    let use_quickjs = should_run_quickjs(plan) || parse_plan(&lowered).is_err();
    if use_quickjs {
        if quickjs_plan_requests_mutation(plan) {
            // QuickJS sandbox still blocks free-form mutation (transaction
            // safety). Surface the write recovery ladder so agents are not
            // stuck on "use CodeMode" alone (wqw.12).
            let msg = crate::annotate_write_failure(
                "sandbox: mutating binding denied without transaction support \
                 (use the lowered zero.edit / tz_edit path, not free-form JS mutation)",
                false,
            );
            return finalize_codemode_result(
                CodeModeResult::error_with_kind("sandbox", msg, 0, false),
                "code",
                plan,
                started_ms,
                &options,
                &limits,
                Vec::new(),
            );
        }
        execute_quickjs_plan(plan, options, &limits, started_ms)
    } else {
        execute_lowered_plan(&lowered, options, &limits, "code", started_ms)
    }
}

fn should_run_quickjs(plan: &str) -> bool {
    let trimmed = plan.trim_start();
    trimmed.starts_with("export default")
        || trimmed.starts_with("async function")
        || trimmed.starts_with("function")
        || trimmed.starts_with("return zero.")
        || trimmed.starts_with("return ctx.")
        || plan.contains("=>")
        || plan.contains("Promise")
        || plan.contains('`')
}

fn quickjs_plan_requests_mutation(plan: &str) -> bool {
    let scanned = plan.replace(['\'', '"', '`'], " ");
    scanned.contains(".edit(") || scanned.contains(" edit(")
}

#[derive(Default)]
struct JsExecutionState {
    ops: usize,
    physical_ops: usize,
    visible_tokens: usize,
    raw_tokens: usize,
    prevented_read_bytes: usize,
    refs: Vec<String>,
    steps: Vec<ExecutionStep>,
    started_ms: u128,
    limits: CodeModeLimits,
}

fn execute_quickjs_plan(
    plan: &str,
    options: CodeModeOptions,
    limits: &CodeModeLimits,
    started_ms: u128,
) -> CodeModeResult {
    let work_root = tokenzero_work_root(options.root.clone());
    let engine = Rc::new(make_engine_for_root_with_options(
        work_root.clone(),
        &options,
    ));
    let state = Rc::new(RefCell::new(JsExecutionState {
        started_ms,
        limits: limits.clone(),
        ..JsExecutionState::default()
    }));

    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            return finalize_codemode_result(
                CodeModeResult::error(format!("sandbox: QuickJS runtime init failed: {err}"), 0),
                "code",
                plan,
                started_ms,
                &options,
                limits,
                Vec::new(),
            );
        }
    };
    runtime.set_memory_limit(limits.max_memory_bytes);
    runtime.set_max_stack_size(512 * 1024);
    let context = match Context::full(&runtime) {
        Ok(context) => context,
        Err(err) => {
            return finalize_codemode_result(
                CodeModeResult::error(format!("sandbox: QuickJS context init failed: {err}"), 0),
                "code",
                plan,
                started_ms,
                &options,
                limits,
                Vec::new(),
            );
        }
    };

    let setup = context.with(|ctx| {
        let globals = ctx.globals();
        install_js_generic_binding(
            &globals,
            Rc::clone(&engine),
            work_root.clone(),
            Rc::clone(&state),
        )?;
        install_js_binding(
            &globals,
            "__tz_compact_json",
            "zero.token.compact",
            Rc::clone(&engine),
            work_root.clone(),
            Rc::clone(&state),
        )?;
        install_js_binding(
            &globals,
            "__tz_expand_json",
            "zero.token.expand",
            Rc::clone(&engine),
            work_root.clone(),
            Rc::clone(&state),
        )?;
        install_js_binding_json_arg(
            &globals,
            "__tz_compact_many_json",
            "zero.token.compactMany",
            Rc::clone(&engine),
            work_root.clone(),
            Rc::clone(&state),
        )?;
        install_js_binding_json_arg(
            &globals,
            "__tz_expand_many_json",
            "zero.token.expandMany",
            Rc::clone(&engine),
            work_root.clone(),
            Rc::clone(&state),
        )?;
        install_js_binding_json_arg(
            &globals,
            "__tz_dedupe_json",
            "zero.token.dedupe",
            Rc::clone(&engine),
            work_root.clone(),
            Rc::clone(&state),
        )?;
        ctx.eval::<(), _>(js_prelude())
    });
    if let Err(err) = setup {
        return finalize_codemode_result(
            CodeModeResult::error(format!("sandbox: QuickJS binding setup failed: {err}"), 0),
            "code",
            plan,
            started_ms,
            &options,
            limits,
            Vec::new(),
        );
    }

    let script = wrap_js_plan(plan);
    if let Err(err) = context.with(|ctx| ctx.eval::<(), _>(script.as_str())) {
        let ops = state.borrow().ops;
        return finalize_codemode_result(
            CodeModeResult::error(format!("sandbox: QuickJS eval failed: {err}"), ops),
            "code",
            plan,
            started_ms,
            &options,
            limits,
            state.borrow().steps.clone(),
        );
    }

    let mut drained = 0;
    while runtime.is_job_pending() {
        if now_ms().saturating_sub(started_ms) as u64 > limits.hard_max_wall_ms {
            let state = state.borrow();
            return finalize_codemode_result(
                CodeModeResult::error(
                    format!(
                        "runtime: hard_max_wall_ms exceeded {}",
                        limits.hard_max_wall_ms
                    ),
                    state.ops,
                ),
                "code",
                plan,
                started_ms,
                &options,
                limits,
                state.steps.clone(),
            );
        }
        if now_ms().saturating_sub(started_ms) as u64 > limits.max_wall_ms {
            let state = state.borrow();
            return finalize_codemode_result(
                CodeModeResult::error(
                    format!("runtime: max_wall_ms exceeded {}", limits.max_wall_ms),
                    state.ops,
                ),
                "code",
                plan,
                started_ms,
                &options,
                limits,
                state.steps.clone(),
            );
        }
        if drained >= limits.max_microtasks {
            let state = state.borrow();
            return finalize_codemode_result(
                CodeModeResult::error(
                    format!("sandbox: microtask cap exceeded {}", limits.max_microtasks),
                    state.ops,
                ),
                "code",
                plan,
                started_ms,
                &options,
                limits,
                state.steps.clone(),
            );
        }
        if let Err(err) = runtime.execute_pending_job() {
            let state = state.borrow();
            return finalize_codemode_result(
                CodeModeResult::error(format!("sandbox: QuickJS job failed: {err}"), state.ops),
                "code",
                plan,
                started_ms,
                &options,
                limits,
                state.steps.clone(),
            );
        }
        drained += 1;
    }

    let (result_json, error): (Option<String>, Option<String>) = context
        .with(|ctx| {
            let globals = ctx.globals();
            Ok::<_, rquickjs::Error>((globals.get("__tz_result")?, globals.get("__tz_error")?))
        })
        .unwrap_or((None, Some("sandbox: result extraction failed".to_string())));

    let state = state.borrow();
    if let Some(error) = error {
        return finalize_codemode_result(
            CodeModeResult::error(error, state.ops),
            "code",
            plan,
            started_ms,
            &options,
            limits,
            state.steps.clone(),
        );
    }
    let value = result_json
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(Value::Null);
    let visible =
        state.visible_tokens + count_tokens(&serde_json::to_string(&value).unwrap_or_default());
    let mut result = CodeModeResult::completed(
        value,
        state.refs.clone(),
        state.ops,
        visible,
        state.raw_tokens,
    );
    result.telemetry.physical_ops = state.physical_ops;
    result.telemetry.prevented_read_bytes = state.prevented_read_bytes;
    if let Some(extra) = result
        .telemetry
        .extra
        .as_mut()
        .and_then(Value::as_object_mut)
    {
        extra.insert(
            "prevented_read_bytes".to_string(),
            json!(state.prevented_read_bytes),
        );
    }
    finalize_codemode_result(
        result,
        "code",
        plan,
        started_ms,
        &options,
        limits,
        state.steps.clone(),
    )
}

fn install_js_generic_binding<'js>(
    globals: &rquickjs::Object<'js>,
    engine: Rc<TokenZeroEngine>,
    work_root: PathBuf,
    state: Rc<RefCell<JsExecutionState>>,
) -> rquickjs::Result<()> {
    globals.set(
        "__tz_call_json",
        Func::from(move |method: String, args_json: String| {
            let args = serde_json::from_str::<Vec<Value>>(&args_json).unwrap_or_default();
            invoke_js_binding(&engine, &work_root, &method, args, &state)
        }),
    )
}

fn install_js_binding<'js>(
    globals: &rquickjs::Object<'js>,
    name: &str,
    method: &'static str,
    engine: Rc<TokenZeroEngine>,
    work_root: PathBuf,
    state: Rc<RefCell<JsExecutionState>>,
) -> rquickjs::Result<()> {
    globals.set(
        name,
        Func::from(move |arg: String| {
            invoke_js_binding(
                &engine,
                &work_root,
                method,
                vec![Value::String(arg)],
                &state,
            )
        }),
    )
}

fn install_js_binding_json_arg<'js>(
    globals: &rquickjs::Object<'js>,
    name: &str,
    method: &'static str,
    engine: Rc<TokenZeroEngine>,
    work_root: PathBuf,
    state: Rc<RefCell<JsExecutionState>>,
) -> rquickjs::Result<()> {
    globals.set(
        name,
        Func::from(move |arg_json: String| {
            let arg = serde_json::from_str::<Value>(&arg_json).unwrap_or(Value::Null);
            invoke_js_binding(&engine, &work_root, method, vec![arg], &state)
        }),
    )
}

fn invoke_js_binding(
    engine: &TokenZeroEngine,
    work_root: &Path,
    method: &str,
    args: Vec<Value>,
    state: &Rc<RefCell<JsExecutionState>>,
) -> String {
    {
        let state_ref = state.borrow();
        if now_ms().saturating_sub(state_ref.started_ms) as u64 > state_ref.limits.hard_max_wall_ms
        {
            return serde_json::to_string(&json!({
                "__tz_error": format!("runtime: hard_max_wall_ms exceeded {}", state_ref.limits.hard_max_wall_ms)
            }))
            .unwrap_or_else(|_| "{\"__tz_error\":\"hard wall clock exceeded\"}".to_string());
        }
        if now_ms().saturating_sub(state_ref.started_ms) as u64 > state_ref.limits.max_wall_ms {
            return serde_json::to_string(&json!({
                "__tz_error": format!("runtime: max_wall_ms exceeded {}", state_ref.limits.max_wall_ms)
            }))
            .unwrap_or_else(|_| "{\"__tz_error\":\"wall clock exceeded\"}".to_string());
        }
        if state_ref.ops >= state_ref.limits.max_logical_ops {
            return serde_json::to_string(&json!({
                "__tz_error": format!("runtime: max_logical_ops exceeded {}", state_ref.limits.max_logical_ops)
            }))
            .unwrap_or_else(|_| "{\"__tz_error\":\"logical op cap exceeded\"}".to_string());
        }
        if state_ref.physical_ops >= state_ref.limits.max_physical_ops {
            return serde_json::to_string(&json!({
                "__tz_error": format!("runtime: max_physical_ops exceeded {}", state_ref.limits.max_physical_ops)
            }))
            .unwrap_or_else(|_| "{\"__tz_error\":\"physical op cap exceeded\"}".to_string());
        }
    }
    let outcome = match dispatch_values(engine, work_root, method, &args) {
        Ok(outcome) => outcome,
        Err(error) => {
            return serde_json::to_string(&json!({
                "__tz_error": error.error.as_ref().map(|error| error.message.as_str()).unwrap_or("unknown error")
            }))
            .unwrap_or_else(|_| "{\"__tz_error\":\"unknown error\"}".to_string());
        }
    };
    let prevented_read_bytes = outcome.prevented_read_bytes();
    let value = outcome.into_value();
    let refs = refs_from_value(&value);
    let mut state = state.borrow_mut();
    let logical_width = if matches!(
        method,
        "zero.token.compactMany" | "zero.token.expandMany" | "zero.token.dedupe"
    ) {
        args.first()
            .and_then(Value::as_array)
            .map(|items| items.len().max(1))
            .unwrap_or(1)
    } else {
        1
    };
    state.ops = state.ops.saturating_add(logical_width);
    state.physical_ops = state.physical_ops.saturating_add(1);
    state.visible_tokens = state
        .visible_tokens
        .saturating_add(result_visible_tokens(&value));
    state.raw_tokens = state.raw_tokens.saturating_add(result_raw_tokens(&value));
    state.prevented_read_bytes = state
        .prevented_read_bytes
        .saturating_add(prevented_read_bytes);
    state.refs.extend(refs.iter().cloned());
    let op = state.ops;
    state.steps.push(ExecutionStep {
        id: format!("js_step{op}"),
        method: method.to_string(),
        status: "completed".to_string(),
        refs,
    });
    serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string())
}

fn js_prelude() -> &'static str {
    r#"
        const __tz_parse = (text) => {
          const value = JSON.parse(text); if (value && value.__tz_exact_expand) { try { return JSON.parse(value.text); } catch (_) { return value.text; } }
          if (value && value.__tz_error) throw new Error(value.__tz_error);
          return value;
        };
        const __tz_call = (method, args) => __tz_parse(__tz_call_json(method, JSON.stringify(args)));
        const __tz_truthy = (value) => {
          if (typeof value === 'function') return Boolean(value());
          if (Array.isArray(value)) return value.length > 0;
          if (value && typeof value === 'object') return Object.keys(value).length > 0;
          return Boolean(value);
        };
        const __tz_lines = (value) => {
          const text = typeof value === 'string' ? value : (value && typeof value.text === 'string' ? value.text : '');
          return text.length === 0 ? [] : text.split(/\r?\n/);
        };
        const zero = Object.freeze({
          raw: (value) => ({ __tz_raw: true, value }),
          count: (value) => Array.isArray(value) ? value.length : __tz_lines(value).length,
          first: (value, n = 1) => {
            const take = Math.max(1, Number(n) || 1);
            if (Array.isArray(value)) return take === 1 ? value[0] : value.slice(0, take);
            const lines = __tz_lines(value).slice(0, take);
            return take === 1 ? (lines[0] || '') : lines.join('\n');
          },
          verdict: (ok, detail = '') => ({ ok: __tz_truthy(ok), detail: String(detail).split(/\r?\n/)[0] }),
          read: (...args) => __tz_call('zero.read', args),
          find: (...args) => __tz_call('zero.find', args),
          grep: (...args) => __tz_call('zero.grep', args),
          glob: (...args) => __tz_call('zero.glob', args),
          tree: (...args) => __tz_call('zero.tree', args),
          shell: (...args) => __tz_call('zero.shell', args),
          expand: (...args) => __tz_call('zero.token.expand', args),
          compact: (...args) => __tz_call('zero.token.compact', args.map(a => typeof a === 'string' ? a : JSON.stringify(a))),
          compact_max: (...args) => __tz_call('zero.compact_max', args.map(a => typeof a === 'string' ? a : JSON.stringify(a))),
          ingest: (...args) => __tz_call('zero.ingest', args),
          mem: (...args) => __tz_call('zero.mem', args),
          recall: (...args) => __tz_call('zero.recall', args),
          fetch: (...args) => __tz_call('zero.fetch', args),
          cache_pack: (...args) => __tz_call('zero.cache_pack', args),
          rewrite: (...args) => __tz_call('zero.rewrite', args),
          discover: (...args) => __tz_call('zero.discover', args),
          batch: (...args) => __tz_call('zero.batch', args),
          pipe: (...args) => __tz_call('zero.pipe', args),
          pick: (...args) => __tz_call('zero.pick', args),
          filter_lines: (...args) => __tz_call('zero.filter_lines', args),
          count_tokens: (...args) => __tz_call('zero.count_tokens', args),
          assert: (...args) => __tz_call('zero.assert', args),
          queryMany: (items) => __tz_parse(__tz_compact_many_json(JSON.stringify(items))),
          token: Object.freeze({
            compact: (text) => __tz_parse(__tz_compact_json(typeof text === 'string' ? text : JSON.stringify(text))),
            expand: (ref) => __tz_parse(__tz_expand_json(String(ref))),
            compactMany: (items) => __tz_parse(__tz_compact_many_json(JSON.stringify(items))),
            expandMany: (refs) => __tz_parse(__tz_expand_many_json(JSON.stringify(refs))),
            dedupe: (items) => __tz_parse(__tz_dedupe_json(JSON.stringify(items))),
            // Op parity for routed callers that address this substrate as
            // zero.token.* (the router's namespaced surface): same engine
            // ops as the top-level zero.* bindings, same policy machinery.
            shell: (...args) => __tz_call('zero.shell', args),
            read: (...args) => __tz_call('zero.read', args),
            find: (...args) => __tz_call('zero.find', args),
            grep: (...args) => __tz_call('zero.grep', args),
            glob: (...args) => __tz_call('zero.glob', args),
            tree: (...args) => __tz_call('zero.tree', args),
            rewrite: (...args) => __tz_call('zero.rewrite', args),
            mem: (...args) => __tz_call('zero.mem', args),
            recall: (...args) => __tz_call('zero.recall', args),
          }),
          ref: (value) => __tz_parse(__tz_compact_json(typeof value === 'string' ? value : JSON.stringify(value))),
        });
        const token = zero.token;
        const ctx = Object.freeze({ ref: zero.ref, step: async (_name, fn) => await fn() });
    "#
}

fn wrap_js_plan(plan: &str) -> String {
    let body = if plan.trim_start().starts_with("export default") {
        let fn_expr = plan.replacen("export default", "", 1);
        format!("return await ({fn_expr})({{ token, zero, ctx }});")
    } else if plan.trim_start().starts_with("async function")
        || plan.trim_start().starts_with("function")
    {
        format!("return await ({plan})({{ token, zero, ctx }});")
    } else {
        plan.to_string()
    };
    format!(
        r#"
        (async () => {{
          try {{
            const value = await (async () => {{ {body} }})();
            globalThis.__tz_result = JSON.stringify(value === undefined ? null : value);
          }} catch (err) {{
            globalThis.__tz_error = String((err && (err.message || err.stack)) || err);
          }}
        }})();
        "#
    )
}

pub(super) fn limits_from_options(options: &CodeModeOptions) -> CodeModeLimits {
    // `hard_max_wall_ms` is a real ceiling. A larger soft limit must never
    // raise it, otherwise caller-controlled limits can disable the sandbox
    // wall-clock guard. Internal callers that need a larger ceiling must set
    // both fields explicitly.
    let hard_max_wall_ms = options.hard_max_wall_ms;
    let max_wall_ms = options.max_wall_ms.min(hard_max_wall_ms);
    CodeModeLimits {
        max_output_bytes: options.max_output_bytes,
        max_refs_emitted: options.max_refs_emitted,
        max_logical_ops: options.max_logical_ops,
        max_physical_ops: options.max_physical_ops,
        max_microtasks: options.max_microtasks,
        max_memory_bytes: options.max_memory_bytes,
        max_code_bytes: options.max_code_bytes,
        max_wall_ms,
        hard_max_wall_ms,
        ..Default::default()
    }
}

fn is_journaled_edit(method: &str) -> bool {
    matches!(method, "zero.edit" | "edit" | "zero.token.edit" | "tz_edit")
}

fn prepare_lowered_transaction(
    engine: &TokenZeroEngine,
    work_root: &Path,
    statements: &[Statement],
    plan: &str,
    started_ms: u128,
) -> Result<(Option<JournalTransaction>, Option<String>, bool), String> {
    let empty_scope = HashMap::new();
    let mut steps = Vec::new();
    for (index, statement) in statements.iter().enumerate() {
        let (id, call) = match statement {
            Statement::Binding { name, call } => (name.clone(), call),
            Statement::Call(call) => (format!("step{index}"), call),
            Statement::Return(_) => continue,
        };
        let args = if is_journaled_edit(&call.method) {
            call.args
                .iter()
                .map(|arg| resolve_expr(arg, &empty_scope))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|message| {
                    format!(
                        "journaled edits require literal path and hunk inputs before apply: {message}"
                    )
                })?
        } else {
            Vec::new()
        };
        steps.push(json!({"id": id, "method": call.method, "args": args}));
    }
    let parsed = json!({
        "atomic": false,
        "plan_id": sha256_bytes(plan.as_bytes()),
        "execution_id": execution_id(plan, started_ms),
    });
    prepare_json_transaction(engine, work_root, &parsed, &steps, plan, started_ms)
}

fn dispatch_lowered_journaled(
    engine: &TokenZeroEngine,
    work_root: &Path,
    call: &MethodCall,
    scope: &HashMap<String, Value>,
    transaction: &mut Option<JournalTransaction>,
    journal_index: usize,
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let reversible = classify_method(&call.method) == OperationClass::ReversibleStoreMutation;
    let replayed_target_edit = reversible
        && transaction.as_ref().is_some_and(|tx| {
            tx.journal()
                .operations
                .get(journal_index)
                .is_some_and(|operation| {
                    operation.target.is_some() && !tx.step_needs_apply(journal_index)
                })
        });
    if reversible && !replayed_target_edit {
        if let Some(tx) = transaction.as_mut() {
            if let Err(original) = tx.mark_applying(journal_index) {
                let cache_path = engine.config.cache_path.clone();
                let combined = tx
                    .rollback(original.clone(), |operation| {
                        rollback_journal_operation(&cache_path, operation)
                    })
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or(original);
                return Err(Box::new(CodeModeResult::error_with_kind(
                    "transaction",
                    combined,
                    journal_index,
                    false,
                )));
            }
        }
    }
    let outcome = if replayed_target_edit {
        OpOutcome::from_catalog(json!({
            "idempotent_replay": true,
            "idempotency_key": transaction.as_ref()
                .and_then(|tx| tx.journal().operations.get(journal_index))
                .map(|operation| operation.idempotency_key.clone()),
        }))
    } else {
        match dispatch(engine, work_root, call, scope) {
            Ok(outcome) => outcome,
            Err(mut error) => {
                if let Some(tx) = transaction.as_mut() {
                    let original = error
                        .error
                        .as_ref()
                        .map(|detail| detail.message.clone())
                        .unwrap_or_else(|| "operation failed".to_string());
                    let cache_path = engine.config.cache_path.clone();
                    if let Err(combined) = tx.rollback(original, |operation| {
                        rollback_journal_operation(&cache_path, operation)
                    }) {
                        if let Some(detail) = error.error.as_mut() {
                            detail.message = combined.to_string();
                        }
                    }
                }
                return Err(error);
            }
        }
    };
    if reversible && !replayed_target_edit {
        if let Some(tx) = transaction.as_mut() {
            let postcondition = tx
                .journal()
                .operations
                .get(journal_index)
                .and_then(|operation| operation.target.as_deref())
                .map(Path::new)
                .map(current_digest)
                .transpose()
                .map_err(|error| {
                    Box::new(CodeModeResult::error_with_kind(
                        "transaction",
                        format!("read postcondition: {error}"),
                        journal_index + 1,
                        false,
                    ))
                })?
                .flatten();
            let compensation_refs = refs_from_value(outcome.as_value());
            tx.mark_applied(journal_index, postcondition, compensation_refs)
                .map_err(|message| {
                    Box::new(CodeModeResult::error_with_kind(
                        "transaction",
                        message,
                        journal_index + 1,
                        false,
                    ))
                })?;
        }
    }
    Ok(outcome)
}

fn finish_lowered_transaction(
    result: &mut CodeModeResult,
    transaction: Option<JournalTransaction>,
    downgrade: Option<&str>,
) -> Result<(), String> {
    if let Some(reason) = downgrade {
        if let Some(extra) = result
            .telemetry
            .extra
            .as_mut()
            .and_then(Value::as_object_mut)
        {
            extra.insert("transaction_atomic".to_string(), json!(false));
            extra.insert("transaction_downgrade".to_string(), json!(reason));
        }
    }
    if let Some(tx) = transaction {
        let journal = tx.commit()?;
        if let Some(extra) = result
            .telemetry
            .extra
            .as_mut()
            .and_then(Value::as_object_mut)
        {
            extra.insert("transaction_atomic".to_string(), json!(true));
            extra.insert("plan_journal_version".to_string(), json!(journal.version));
            extra.insert("journal_state".to_string(), json!(journal.state));
        }
    }
    Ok(())
}

fn execute_lowered_plan(
    plan: &str,
    options: CodeModeOptions,
    limits: &CodeModeLimits,
    kind: &str,
    started_ms: u128,
) -> CodeModeResult {
    let statements = match parse_plan(plan) {
        Ok(s) => s,
        Err(e) => {
            return finalize_codemode_result(
                CodeModeResult::error(e, 0),
                kind,
                plan,
                started_ms,
                &options,
                limits,
                Vec::new(),
            );
        }
    };

    if statements.len() > limits.max_logical_ops {
        return finalize_codemode_result(
            CodeModeResult::error(
                format!("plan exceeds max_logical_ops {}", limits.max_logical_ops),
                0,
            ),
            kind,
            plan,
            started_ms,
            &options,
            limits,
            Vec::new(),
        );
    }

    let work_root = tokenzero_work_root(options.root.clone());
    let engine = make_engine_for_root_with_options(work_root.clone(), &options);
    let (mut transaction, transaction_downgrade, already_committed) =
        match prepare_lowered_transaction(&engine, &work_root, &statements, plan, started_ms) {
            Ok(value) => value,
            Err(message) => {
                return finalize_codemode_result(
                    CodeModeResult::error_with_kind("transaction", message, 0, false),
                    kind,
                    plan,
                    started_ms,
                    &options,
                    limits,
                    Vec::new(),
                );
            }
        };
    if already_committed {
        return finalize_codemode_result(
            CodeModeResult::completed(
                json!({"transaction": "already_committed", "idempotent_replay": true}),
                Vec::new(),
                0,
                0,
                0,
            ),
            kind,
            plan,
            started_ms,
            &options,
            limits,
            Vec::new(),
        );
    }
    let mut scope: HashMap<String, Value> = HashMap::new();
    let mut all_refs: Vec<String> = Vec::new();
    let mut ops: usize = 0;
    let mut total_visible: usize = 0;
    let mut total_raw: usize = 0;
    let mut total_prevented: usize = 0;
    let mut last_value: Value = Value::Null;
    let mut steps: Vec<ExecutionStep> = Vec::new();
    let mut journal_index = 0usize;

    for stmt in &statements {
        match stmt {
            Statement::Binding { name, call } => {
                ops += 1;
                let outcome = match dispatch_lowered_journaled(
                    &engine,
                    &work_root,
                    call,
                    &scope,
                    &mut transaction,
                    journal_index,
                ) {
                    Ok(outcome) => outcome,
                    Err(mut e) => {
                        if let Some(extra) =
                            e.telemetry.extra.as_mut().and_then(Value::as_object_mut)
                        {
                            extra.insert("operations".to_string(), json!(ops));
                        }
                        e.telemetry.operations = ops;
                        e.telemetry.logical_ops = ops;
                        return finalize_codemode_result(
                            *e, kind, plan, started_ms, &options, limits, steps,
                        );
                    }
                };
                journal_index += 1;
                steps.push(ExecutionStep {
                    id: name.clone(),
                    method: call.method.clone(),
                    status: "completed".to_string(),
                    refs: refs_from_value(outcome.as_value()),
                });
                record_outcome(
                    &outcome,
                    &mut all_refs,
                    &mut total_visible,
                    &mut total_raw,
                    &mut total_prevented,
                );
                last_value = outcome.as_value().clone();
                scope.insert(name.clone(), outcome.into_value());
            }
            Statement::Call(call) => {
                ops += 1;
                let outcome = match dispatch_lowered_journaled(
                    &engine,
                    &work_root,
                    call,
                    &scope,
                    &mut transaction,
                    journal_index,
                ) {
                    Ok(outcome) => outcome,
                    Err(mut e) => {
                        if let Some(extra) =
                            e.telemetry.extra.as_mut().and_then(Value::as_object_mut)
                        {
                            extra.insert("operations".to_string(), json!(ops));
                        }
                        e.telemetry.operations = ops;
                        e.telemetry.logical_ops = ops;
                        return finalize_codemode_result(
                            *e, kind, plan, started_ms, &options, limits, steps,
                        );
                    }
                };
                journal_index += 1;
                steps.push(ExecutionStep {
                    id: format!("step{ops}"),
                    method: call.method.clone(),
                    status: "completed".to_string(),
                    refs: refs_from_value(outcome.as_value()),
                });
                record_outcome(
                    &outcome,
                    &mut all_refs,
                    &mut total_visible,
                    &mut total_raw,
                    &mut total_prevented,
                );
                last_value = outcome.into_value();
            }
            Statement::Return(expr) => {
                let value = match resolve_return(expr, &scope) {
                    Ok(value) => value,
                    Err(message) => {
                        return finalize_codemode_result(
                            CodeModeResult::error(message, ops),
                            kind,
                            plan,
                            started_ms,
                            &options,
                            limits,
                            steps,
                        );
                    }
                };
                let vis = count_tokens(&serde_json::to_string(&value).unwrap_or_default());
                let mut result =
                    CodeModeResult::completed(value, all_refs, ops, total_visible + vis, total_raw);
                result.telemetry.prevented_read_bytes = total_prevented;
                if let Some(extra) = result
                    .telemetry
                    .extra
                    .as_mut()
                    .and_then(Value::as_object_mut)
                {
                    extra.insert("prevented_read_bytes".to_string(), json!(total_prevented));
                }
                if let Err(message) = finish_lowered_transaction(
                    &mut result,
                    transaction,
                    transaction_downgrade.as_deref(),
                ) {
                    return finalize_codemode_result(
                        CodeModeResult::error_with_kind("transaction", message, ops, false),
                        kind,
                        plan,
                        started_ms,
                        &options,
                        limits,
                        steps,
                    );
                }
                return finalize_codemode_result(
                    result, kind, plan, started_ms, &options, limits, steps,
                );
            }
        }
    }

    let vis = count_tokens(&serde_json::to_string(&last_value).unwrap_or_default());
    let mut result =
        CodeModeResult::completed(last_value, all_refs, ops, total_visible + vis, total_raw);
    result.telemetry.prevented_read_bytes = total_prevented;
    if let Some(extra) = result
        .telemetry
        .extra
        .as_mut()
        .and_then(Value::as_object_mut)
    {
        extra.insert("prevented_read_bytes".to_string(), json!(total_prevented));
    }
    if let Err(message) =
        finish_lowered_transaction(&mut result, transaction, transaction_downgrade.as_deref())
    {
        return finalize_codemode_result(
            CodeModeResult::error_with_kind("transaction", message, ops, false),
            kind,
            plan,
            started_ms,
            &options,
            limits,
            steps,
        );
    }
    finalize_codemode_result(result, kind, plan, started_ms, &options, limits, steps)
}

fn ref_first_final_value(
    engine: &TokenZeroEngine,
    value: Value,
    options: &CodeModeOptions,
) -> (Value, Vec<String>) {
    if !options.ref_first {
        return (unwrap_raw_value(value), Vec::new());
    }
    let mut store = tokenzero_recovery::RecoveryStore::new(Some(engine.config.cache_path.clone()));
    let mut refs = Vec::new();
    let budget_tokens = options
        .ref_first_budget
        .max(crate::shell_inline_budget_from_env());
    let value = ref_first_value(value, budget_tokens, &mut store, &mut refs);
    (value, refs)
}

fn ref_first_value(
    value: Value,
    budget_tokens: usize,
    store: &mut tokenzero_recovery::RecoveryStore,
    refs: &mut Vec<String>,
) -> Value {
    if is_exact_expand_value(&value) {
        return value;
    }
    match value {
        Value::String(text) => {
            if count_tokens(&text) > budget_tokens {
                let content_type = detect_content_type(&text, None);
                if let Ok(stored) = store.store_payload(&text, content_type, None, None, None) {
                    let ref_id = stored.blob_ref.as_str().to_string();
                    if !refs.contains(&ref_id) {
                        refs.push(ref_id.clone());
                    }
                    return json!({
                        "ref": ref_id,
                        "preview": first_line_preview(&text),
                    });
                }
            }
            Value::String(text)
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| ref_first_value(item, budget_tokens, store, refs))
                .collect(),
        ),
        Value::Object(mut map) => {
            if map
                .get("__tz_raw")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return map.remove("value").unwrap_or(Value::Null);
            }
            if map
                .get(EXACT_EXPAND_MARKER)
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                map.remove(EXACT_EXPAND_MARKER);
                return Value::Object(map);
            }
            Value::Object(
                map.into_iter()
                    .map(|(key, value)| (key, ref_first_value(value, budget_tokens, store, refs)))
                    .collect(),
            )
        }
        other => other,
    }
}

fn unwrap_raw_value(value: Value) -> Value {
    match value {
        Value::Object(mut map)
            if map
                .get("__tz_raw")
                .and_then(Value::as_bool)
                .unwrap_or(false) =>
        {
            map.remove("value").unwrap_or(Value::Null)
        }
        Value::Object(mut map) => {
            map.remove(EXACT_EXPAND_MARKER);
            Value::Object(map)
        }
        other => other,
    }
}

fn first_line_preview(text: &str) -> String {
    let mut preview = text.lines().next().unwrap_or("").trim().to_string();
    if preview.chars().count() > 32 {
        preview = preview.chars().take(32).collect();
    }
    preview
}

fn finalize_codemode_result(
    mut result: CodeModeResult,
    kind: &str,
    plan: &str,
    started_ms: u128,
    options: &CodeModeOptions,
    limits: &CodeModeLimits,
    steps: Vec<ExecutionStep>,
) -> CodeModeResult {
    let work_root = tokenzero_work_root(options.root.clone());
    let engine = make_engine_for_root_with_options(work_root, options);
    let journal_health = journal_doctor_json(&engine.config.cache_path);
    if let Some(extra) = result
        .telemetry
        .extra
        .as_mut()
        .and_then(Value::as_object_mut)
    {
        extra.insert("plan_journals".to_string(), journal_health);
    }
    if matches!(result.status, CodeModeStatus::Completed) {
        if let Some(value) = result.value.take() {
            let (value, refs) = ref_first_final_value(&engine, value, options);
            for ref_id in refs {
                if !result.refs.contains(&ref_id) {
                    result.refs.push(ref_id);
                }
            }
            result.value = Some(value);
        }
    }
    let payload_tokens = result
        .value
        .as_ref()
        .map(|value| count_tokens(&serde_json::to_string(value).unwrap_or_default()))
        .unwrap_or_else(|| {
            result
                .error
                .as_ref()
                .map(|error| count_tokens(&error.message))
                .unwrap_or(0)
        });
    let operations = result.telemetry.operations();
    let physical_ops = physical_ops_for(&result);
    result.telemetry.operations = operations;
    result.telemetry.logical_ops = operations;
    result.telemetry.physical_ops = physical_ops;
    result.telemetry.batched_ops = if operations > physical_ops { 1 } else { 0 };
    result.telemetry.internal_actions = operations.saturating_add(result.refs.len());
    result.telemetry.payload_tokens = payload_tokens;
    result.telemetry.envelope_tokens = count_tokens(&result.visible_ack);
    // Granular token attribution buckets (6ot).
    result.telemetry.ack_tokens = count_tokens(&result.visible_ack);
    let ref_strings = result.refs.join(" ");
    result.telemetry.ref_string_tokens = count_tokens(&ref_strings);
    // Framing = envelope minus ack and ref strings (JSON keys, punctuation, structure).
    result.telemetry.framing_tokens = result
        .telemetry
        .envelope_tokens
        .saturating_sub(result.telemetry.ack_tokens)
        .saturating_sub(result.telemetry.ref_string_tokens);
    // Preview = payload minus ref strings (inline text the model sees directly).
    result.telemetry.preview_tokens =
        payload_tokens.saturating_sub(result.telemetry.ref_string_tokens);

    // Provider-exposed cached_tokens take priority when available; byte-prefix
    // comparison is the fallback estimate.
    let current_output =
        serde_json::to_string(result.value.as_ref().unwrap_or(&Value::Null)).unwrap_or_default();
    let (cached_tokens, total_output_tokens) = {
        let cache_path = engine.config.cache_path.clone();
        let mut map = previous_output_by_session().lock().unwrap();
        let n = map
            .get(&cache_path)
            .map(|prev| common_prefix_len(&current_output, prev))
            .unwrap_or(0);
        let matched = &current_output[..n];
        let cached = count_tokens(matched);
        let total = count_tokens(&current_output);
        map.insert(cache_path, current_output);
        (cached, total)
    };
    result.telemetry.prefix_cache_hits = result
        .telemetry
        .prefix_cache_hits
        .saturating_add(cached_tokens);
    result.telemetry.prefix_cache_total = result
        .telemetry
        .prefix_cache_total
        .saturating_add(total_output_tokens);

    if let Some(extra) = result
        .telemetry
        .extra
        .as_mut()
        .and_then(Value::as_object_mut)
    {
        extra.insert("refs_count".to_string(), json!(result.refs.len()));
        extra.insert(
            "payload_tokens".to_string(),
            json!(result.telemetry.payload_tokens),
        );
        extra.insert(
            "envelope_tokens".to_string(),
            json!(result.telemetry.envelope_tokens),
        );
        extra.insert("ack_tokens".to_string(), json!(result.telemetry.ack_tokens));
        extra.insert(
            "ref_string_tokens".to_string(),
            json!(result.telemetry.ref_string_tokens),
        );
        extra.insert(
            "framing_tokens".to_string(),
            json!(result.telemetry.framing_tokens),
        );
        extra.insert(
            "preview_tokens".to_string(),
            json!(result.telemetry.preview_tokens),
        );
        extra.insert(
            "prevented_read_bytes".to_string(),
            json!(result.telemetry.prevented_read_bytes),
        );
        extra.insert(
            "prefix_cache_hits".to_string(),
            json!(result.telemetry.prefix_cache_hits),
        );
        extra.insert(
            "prefix_cache_total".to_string(),
            json!(result.telemetry.prefix_cache_total),
        );
        extra.insert(
            "prefix_cache_hit_rate".to_string(),
            json!(if result.telemetry.prefix_cache_total == 0 {
                0.0
            } else {
                result.telemetry.prefix_cache_hits as f64
                    / result.telemetry.prefix_cache_total as f64
            }),
        );
    }
    result.telemetry.refs_count = Some(result.refs.len());
    finalize_result(
        result,
        kind,
        plan,
        started_ms,
        now_ms(),
        ExecutionStore::new(engine.config.cache_path.clone()),
        limits,
        steps,
    )
}

fn physical_ops_for(result: &CodeModeResult) -> usize {
    result.telemetry.physical_ops
}

fn lower_recipe_plan(plan: &str) -> Option<String> {
    let plan = plan.trim();
    if let Some(data) = plan.strip_prefix("compact:") {
        let quoted = serde_json::to_string(data.trim()).ok()?;
        return Some(format!("await zero.token.compact({quoted})"));
    }
    if let Some(ref_id) = plan
        .strip_prefix("expand:")
        .or_else(|| plan.strip_prefix("expand-pack:"))
    {
        let quoted = serde_json::to_string(ref_id.trim()).ok()?;
        return Some(format!("await zero.token.expand({quoted})"));
    }
    if let Some(refs) = plan.strip_prefix("dedupe:") {
        let arr = refs
            .split(',')
            .map(|s| Value::String(s.trim().to_string()))
            .collect::<Vec<_>>();
        let quoted = serde_json::to_string(&arr).ok()?;
        return Some(format!("await zero.token.dedupe({quoted})"));
    }
    if plan == "pack" || plan.starts_with("pack:") {
        return Some("await zero.cache_pack()".to_string());
    }
    None
}

fn predict_edit_postimage(text: &str, edits: &[Value], create: bool) -> Result<String, String> {
    if create {
        if edits.len() != 1 {
            return Err("create=true requires exactly one edit hunk".to_string());
        }
        let hunk = edits[0]
            .as_object()
            .ok_or_else(|| "edit hunk must be an object".to_string())?;
        if !hunk
            .get("find")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
        {
            return Err("create=true requires an empty find".to_string());
        }
        return hunk
            .get("replace")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "edit hunk requires replace text".to_string());
    }
    let mut next = text.to_string();
    for (index, value) in edits.iter().enumerate() {
        let hunk = value
            .as_object()
            .ok_or_else(|| format!("edit hunk {index} must be an object"))?;
        let find = hunk
            .get("find")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("edit hunk {index} requires find text"))?;
        let replace = hunk
            .get("replace")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("edit hunk {index} requires replace text"))?;
        let replace_all = hunk
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let matches = next.match_indices(find).count();
        if matches == 0 {
            return Err(format!("edit hunk {index} find text not found"));
        }
        if !replace_all && matches != 1 {
            return Err(format!(
                "edit hunk {index} is ambiguous ({matches} matches)"
            ));
        }
        next = if replace_all {
            next.replace(find, replace)
        } else {
            next.replacen(find, replace, 1)
        };
    }
    Ok(next)
}

fn prepare_json_transaction(
    engine: &TokenZeroEngine,
    work_root: &Path,
    parsed: &Value,
    steps: &[Value],
    plan: &str,
    started_ms: u128,
) -> Result<(Option<JournalTransaction>, Option<String>, bool), String> {
    let execution_id = parsed
        .get("execution_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| execution_id(plan, started_ms));
    let plan_id = parsed
        .get("plan_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| sha256_bytes(plan.as_bytes()));
    let atomic_requested = parsed
        .get("atomic")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match inspect_journal(&engine.config.cache_path, &execution_id) {
        Ok(existing) if existing.atomic => {
            if existing.plan_id != plan_id {
                return Err("journal identity collision".to_string());
            }
            if !atomic_requested {
                return Err("journal replay changed atomic policy".to_string());
            }
            return match existing.state {
                JournalState::Committed => Ok((None, None, true)),
                JournalState::RolledBack => {
                    Err("execution was already rolled back; use a new execution_id".to_string())
                }
                _ => open_unresolved(&engine.config.cache_path, &execution_id)
                    .map(|transaction| (Some(transaction), None, false)),
            };
        }
        Ok(_) => {}
        Err(message) if message.contains("No such file or directory") => {}
        Err(message) => return Err(message),
    }
    let mut seen_targets = std::collections::HashSet::new();
    let mut specs = Vec::with_capacity(steps.len());
    for (index, step) in steps.iter().enumerate() {
        let id = step
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("step{index}"));
        let method = step
            .get("method")
            .or_else(|| step.get("tool"))
            .and_then(Value::as_str)
            .ok_or_else(|| format!("json plan step {index} missing method"))?
            .to_string();
        let mut spec = OperationSpec {
            id,
            method: method.clone(),
            target: None,
            precondition_digest: None,
            precondition_exists: None,
            postcondition_digest: None,
            undo_refs: Vec::new(),
            size: None,
        };
        if classify_method(&method) == OperationClass::ReversibleStoreMutation
            && is_journaled_edit(&method)
        {
            let path = step
                .get("args")
                .and_then(Value::as_array)
                .and_then(|args| args.first())
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    format!(
                        "journaled edit step {index} requires a literal path; dynamic targets are not CAS-safe"
                    )
                })?;
            let path = resolve_paths_against_work_root(vec![PathBuf::from(path)], work_root)
                .into_iter()
                .next()
                .ok_or_else(|| format!("journaled edit step {index} has no target"))?;
            if !seen_targets.insert(path.clone()) {
                return Err(format!(
                    "atomic plan has repeated edit target {}; combine its hunks into one step",
                    path.display()
                ));
            }
            let (exists, bytes) = match std::fs::read(&path) {
                Ok(bytes) => (true, bytes),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => (false, Vec::new()),
                Err(err) => {
                    return Err(format!("read edit precondition {}: {err}", path.display()));
                }
            };
            let text = String::from_utf8(bytes.clone())
                .map_err(|_| format!("journaled edit target {} is not UTF-8", path.display()))?;
            let mut store =
                tokenzero_recovery::RecoveryStore::new(Some(engine.config.cache_path.clone()));
            let stored = store
                .store_payload(
                    &text,
                    detect_content_type(&text, Some(&path)),
                    Some(&path),
                    None,
                    None,
                )
                .map_err(|err| format!("persist edit undo pre-image: {err}"))?;
            let args = step
                .get("args")
                .and_then(Value::as_array)
                .expect("path came from args");
            let edits = args
                .get(1)
                .and_then(Value::as_array)
                .ok_or_else(|| format!("journaled edit step {index} requires an edit array"))?;
            let create = args
                .get(2)
                .and_then(Value::as_object)
                .and_then(|opts| opts.get("create"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let postimage = predict_edit_postimage(&text, edits, create)?;
            spec.target = Some(path);
            spec.precondition_digest = exists.then(|| sha256_bytes(&bytes));
            spec.precondition_exists = Some(exists);
            spec.postcondition_digest = Some(sha256_bytes(postimage.as_bytes()));
            spec.undo_refs.push(stored.blob_ref);
            spec.size = Some(bytes.len() as u64);
        }
        specs.push(spec);
    }
    match begin_plan(
        &engine.config.cache_path,
        work_root,
        &plan_id,
        &execution_id,
        specs,
        atomic_requested,
    )? {
        BeginOutcome::Disabled => Ok((None, None, false)),
        BeginOutcome::Downgraded { reason } => Ok((None, Some(reason), false)),
        BeginOutcome::AlreadyCommitted => Ok((None, None, true)),
        BeginOutcome::Transaction(tx) => Ok((Some(*tx), None, false)),
    }
}

fn rollback_journal_operation(
    cache_path: &Path,
    operation: &JournalOperation,
) -> Result<(), String> {
    if operation.classification != OperationClass::ReversibleStoreMutation {
        return Ok(());
    }
    let Some(target) = operation.target.as_deref() else {
        // CAS puts are immutable and duplicate-safe; no visible state needs restoring.
        return Ok(());
    };
    let path = Path::new(target);
    let actual = current_digest(path).map_err(|err| format!("read rollback target: {err}"))?;
    if actual == operation.precondition_digest {
        return Ok(());
    }
    if actual != operation.postcondition_digest {
        return Err(format!(
            "rollback CAS refused for {target}: expected post-image {:?}, actual {:?}",
            operation.postcondition_digest, actual
        ));
    }
    if operation.precondition_exists == Some(false) {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(format!("remove created file {target}: {err}")),
        }
    }
    let undo_ref = operation
        .undo_refs
        .first()
        .ok_or_else(|| format!("rollback step {} is missing an undo ref", operation.index))?;
    let mut store = tokenzero_recovery::RecoveryStore::new(Some(cache_path.to_path_buf()));
    let expanded = store.expand(undo_ref, None, None, None, None, None);
    if !expanded.found {
        return Err(format!(
            "undo ref {undo_ref} unavailable: {}",
            expanded.reason
        ));
    }
    journal_atomic_write(path, expanded.content.as_bytes())
        .map_err(|err| format!("restore {target}: {err}"))
}

fn execute_json_plan(
    plan: &str,
    options: CodeModeOptions,
    limits: &CodeModeLimits,
    started_ms: u128,
) -> CodeModeResult {
    let parsed: Value = match serde_json::from_str(plan) {
        Ok(value) => value,
        Err(err) => {
            return finalize_codemode_result(
                CodeModeResult::error(format!("json plan parse error: {err}"), 0),
                "json",
                plan,
                started_ms,
                &options,
                limits,
                Vec::new(),
            );
        }
    };
    let steps_value = if let Some(steps) = parsed.get("steps") {
        steps.clone()
    } else {
        parsed.clone()
    };
    let steps_arr = match steps_value.as_array() {
        Some(steps) => steps,
        None => {
            return finalize_codemode_result(
                CodeModeResult::error("json plan requires a steps array".to_string(), 0),
                "json",
                plan,
                started_ms,
                &options,
                limits,
                Vec::new(),
            );
        }
    };
    if steps_arr.len() > limits.max_logical_ops {
        return finalize_codemode_result(
            CodeModeResult::error(
                format!(
                    "json plan exceeds max_logical_ops {}",
                    limits.max_logical_ops
                ),
                0,
            ),
            "json",
            plan,
            started_ms,
            &options,
            limits,
            Vec::new(),
        );
    }

    let work_root = tokenzero_work_root(options.root.clone());
    let engine = make_engine_for_root_with_options(work_root.clone(), &options);
    let (mut transaction, transaction_downgrade, already_committed) =
        match prepare_json_transaction(&engine, &work_root, &parsed, steps_arr, plan, started_ms) {
            Ok(value) => value,
            Err(message) => {
                return finalize_codemode_result(
                    CodeModeResult::error_with_kind("transaction", message, 0, false),
                    "json",
                    plan,
                    started_ms,
                    &options,
                    limits,
                    Vec::new(),
                );
            }
        };
    if already_committed {
        return finalize_codemode_result(
            CodeModeResult::completed(
                json!({"transaction": "already_committed", "idempotent_replay": true}),
                Vec::new(),
                0,
                0,
                0,
            ),
            "json",
            plan,
            started_ms,
            &options,
            limits,
            Vec::new(),
        );
    }
    let mut scope: HashMap<String, Value> = HashMap::new();
    let mut all_refs = Vec::new();
    let mut total_visible = 0usize;
    let mut total_raw = 0usize;
    let mut total_prevented = 0usize;
    let mut executed = Vec::new();
    let mut last = Value::Null;

    for (idx, step) in steps_arr.iter().enumerate() {
        let id = step
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| format!("step{idx}"));
        let method = match step
            .get("method")
            .or_else(|| step.get("tool"))
            .and_then(|v| v.as_str())
        {
            Some(method) => method.to_string(),
            None => {
                return finalize_codemode_result(
                    CodeModeResult::error(format!("json plan step {idx} missing method"), idx),
                    "json",
                    plan,
                    started_ms,
                    &options,
                    limits,
                    executed,
                );
            }
        };
        let args = match json_args_to_exprs(step.get("args"), &scope) {
            Ok(args) => args,
            Err(message) => {
                return finalize_codemode_result(
                    CodeModeResult::error(message, idx),
                    "json",
                    plan,
                    started_ms,
                    &options,
                    limits,
                    executed,
                );
            }
        };
        let call = MethodCall {
            method: method.clone(),
            args,
        };
        let reversible = classify_method(&method) == OperationClass::ReversibleStoreMutation;
        let replayed = reversible
            && transaction
                .as_ref()
                .is_some_and(|tx| !tx.step_needs_apply(idx));
        if reversible && !replayed {
            if let Some(tx) = transaction.as_mut() {
                if let Err(message) = tx.mark_applying(idx) {
                    let cache_path = engine.config.cache_path.clone();
                    let combined = tx
                        .rollback(message.clone(), |op| {
                            rollback_journal_operation(&cache_path, op)
                        })
                        .err()
                        .map(|err| err.to_string())
                        .unwrap_or(message);
                    return finalize_codemode_result(
                        CodeModeResult::error_with_kind("transaction", combined, idx, false),
                        "json",
                        plan,
                        started_ms,
                        &options,
                        limits,
                        executed,
                    );
                }
            }
        }
        let outcome = if replayed {
            OpOutcome::from_catalog(json!({
                "idempotent_replay": true,
                "idempotency_key": transaction
                    .as_ref()
                    .and_then(|tx| tx.journal().operations.get(idx))
                    .map(|op| op.idempotency_key.clone()),
            }))
        } else {
            match dispatch(&engine, &work_root, &call, &scope) {
                Ok(outcome) => outcome,
                Err(mut err) => {
                    if let Some(extra) = err.telemetry.extra.as_mut().and_then(Value::as_object_mut)
                    {
                        extra.insert("operations".to_string(), json!(idx + 1));
                    }
                    err.telemetry.operations = idx + 1;
                    err.telemetry.logical_ops = idx + 1;
                    if let Some(tx) = transaction.as_mut() {
                        let original = err
                            .error
                            .as_ref()
                            .map(|detail| detail.message.clone())
                            .unwrap_or_else(|| "operation failed".to_string());
                        let cache_path = engine.config.cache_path.clone();
                        if let Err(combined) =
                            tx.rollback(original, |op| rollback_journal_operation(&cache_path, op))
                        {
                            if let Some(detail) = err.error.as_mut() {
                                detail.message = combined.to_string();
                            }
                        }
                    }
                    return finalize_codemode_result(
                        *err, "json", plan, started_ms, &options, limits, executed,
                    );
                }
            }
        };
        if reversible && !replayed {
            if let Some(tx) = transaction.as_mut() {
                let postcondition = tx
                    .journal()
                    .operations
                    .get(idx)
                    .and_then(|op| op.target.as_deref())
                    .map(Path::new)
                    .map(current_digest)
                    .transpose()
                    .map_err(|err| format!("read postcondition: {err}"));
                let postcondition = match postcondition {
                    Ok(value) => value.flatten(),
                    Err(message) => {
                        let cache_path = engine.config.cache_path.clone();
                        let combined = tx
                            .rollback(message.clone(), |op| {
                                rollback_journal_operation(&cache_path, op)
                            })
                            .err()
                            .map(|err| err.to_string())
                            .unwrap_or(message);
                        return finalize_codemode_result(
                            CodeModeResult::error_with_kind(
                                "transaction",
                                combined,
                                idx + 1,
                                false,
                            ),
                            "json",
                            plan,
                            started_ms,
                            &options,
                            limits,
                            executed,
                        );
                    }
                };
                let compensation_refs = refs_from_value(outcome.as_value());
                if let Err(message) = tx.mark_applied(idx, postcondition, compensation_refs) {
                    let cache_path = engine.config.cache_path.clone();
                    let combined = tx
                        .rollback(message.clone(), |op| {
                            rollback_journal_operation(&cache_path, op)
                        })
                        .err()
                        .map(|err| err.to_string())
                        .unwrap_or(message);
                    return finalize_codemode_result(
                        CodeModeResult::error_with_kind("transaction", combined, idx + 1, false),
                        "json",
                        plan,
                        started_ms,
                        &options,
                        limits,
                        executed,
                    );
                }
            }
        }
        let refs = refs_from_value(outcome.as_value());
        executed.push(ExecutionStep {
            id: id.clone(),
            method,
            status: "completed".to_string(),
            refs,
        });
        record_outcome(
            &outcome,
            &mut all_refs,
            &mut total_visible,
            &mut total_raw,
            &mut total_prevented,
        );
        last = outcome.into_value();
        scope.insert(id, last.clone());
    }

    let value = parsed
        .get("return")
        .and_then(|return_value| resolve_json_return(return_value, &scope).ok())
        .unwrap_or(last);
    let vis = count_tokens(&serde_json::to_string(&value).unwrap_or_default());
    let mut result = CodeModeResult::completed(
        value,
        all_refs,
        steps_arr.len(),
        total_visible + vis,
        total_raw,
    );
    result.telemetry.prevented_read_bytes = total_prevented;
    if let Some(extra) = result
        .telemetry
        .extra
        .as_mut()
        .and_then(Value::as_object_mut)
    {
        extra.insert(
            "parallel_groups".to_string(),
            json!(count_parallel_groups(steps_arr)),
        );
        extra.insert("prevented_read_bytes".to_string(), json!(total_prevented));
    }
    result.telemetry.parallel_groups = Some(count_parallel_groups(steps_arr));
    result.telemetry.physical_ops = estimate_physical_ops(steps_arr);
    if let Some(reason) = transaction_downgrade {
        if let Some(extra) = result
            .telemetry
            .extra
            .as_mut()
            .and_then(Value::as_object_mut)
        {
            extra.insert("transaction_atomic".to_string(), json!(false));
            extra.insert("transaction_downgrade".to_string(), json!(reason));
        }
    }
    if let Some(tx) = transaction {
        match tx.commit() {
            Ok(journal) => {
                if let Some(extra) = result
                    .telemetry
                    .extra
                    .as_mut()
                    .and_then(Value::as_object_mut)
                {
                    extra.insert("transaction_atomic".to_string(), json!(true));
                    extra.insert("plan_journal_version".to_string(), json!(journal.version));
                    extra.insert("journal_state".to_string(), json!(journal.state));
                }
            }
            Err(message) => {
                return finalize_codemode_result(
                    CodeModeResult::error_with_kind("transaction", message, steps_arr.len(), false),
                    "json",
                    plan,
                    started_ms,
                    &options,
                    limits,
                    executed,
                );
            }
        }
    }
    finalize_codemode_result(result, "json", plan, started_ms, &options, limits, executed)
}

fn json_args_to_exprs(
    value: Option<&Value>,
    scope: &HashMap<String, Value>,
) -> Result<Vec<Expr>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let arr = value
        .as_array()
        .ok_or_else(|| "json plan step args must be an array".to_string())?;
    arr.iter()
        .map(|value| json_value_to_expr(resolve_json_binding(value, scope)?))
        .collect()
}

fn resolve_json_return(value: &Value, scope: &HashMap<String, Value>) -> Result<Value, String> {
    resolve_json_binding(value, scope)
}

fn resolve_json_binding(value: &Value, scope: &HashMap<String, Value>) -> Result<Value, String> {
    match value {
        Value::String(s) if s.starts_with('$') => resolve_binding_string(s, scope),
        Value::Array(items) => items
            .iter()
            .map(|item| resolve_json_binding(item, scope))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| resolve_json_binding(value, scope).map(|v| (key.clone(), v)))
            .collect::<Result<serde_json::Map<String, Value>, _>>()
            .map(Value::Object),
        other => Ok(other.clone()),
    }
}

fn resolve_binding_string(s: &str, scope: &HashMap<String, Value>) -> Result<Value, String> {
    let path = s.trim_start_matches('$');
    if path.is_empty() {
        return Err("empty json binding".to_string());
    }
    let mut parts = path.split('.');
    let first = parts.next().unwrap();
    let mut value = scope
        .get(first)
        .cloned()
        .ok_or_else(|| format!("undefined json binding: ${first}"))?;
    for part in parts {
        value = value
            .get(part)
            .cloned()
            .ok_or_else(|| format!("undefined json binding property: ${path}"))?;
    }
    Ok(value)
}

fn json_value_to_expr(value: Value) -> Result<Expr, String> {
    match value {
        Value::String(s) => Ok(Expr::StringLit(s)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Expr::IntLit(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Expr::FloatLit(f))
            } else {
                Ok(Expr::Null)
            }
        }
        Value::Bool(b) => Ok(Expr::BoolLit(b)),
        Value::Null => Ok(Expr::Null),
        Value::Array(items) => items
            .into_iter()
            .map(json_value_to_expr)
            .collect::<Result<Vec<_>, _>>()
            .map(Expr::Array),
        Value::Object(map) => map
            .into_iter()
            .map(|(key, value)| json_value_to_expr(value).map(|expr| (key, expr)))
            .collect::<Result<Vec<_>, _>>()
            .map(Expr::Object),
    }
}

fn count_parallel_groups(steps: &[Value]) -> usize {
    steps
        .iter()
        .filter(|step| {
            step.get("needs")
                .is_some_and(|needs| needs.as_array().is_some_and(|arr| arr.len() > 1))
        })
        .count()
}

fn estimate_physical_ops(steps: &[Value]) -> usize {
    // V1 exposes explicit batch methods, so every JSON step is one native dispatch.
    steps.len()
}

fn refs_from_value(value: &Value) -> Vec<String> {
    let mut refs = Vec::new();
    collect_refs(value, &mut refs);
    refs
}

fn dispatch(
    engine: &TokenZeroEngine,
    work_root: &Path,
    call: &MethodCall,
    scope: &HashMap<String, Value>,
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let args: Vec<Value> = call
        .args
        .iter()
        .map(|arg| {
            resolve_expr(arg, scope).map_err(|message| Box::new(CodeModeResult::error(message, 0)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    dispatch_values(engine, work_root, &call.method, &args)
}

fn dispatch_values(
    engine: &TokenZeroEngine,
    work_root: &Path,
    method: &str,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    // zero.token.* aliases: the router's namespaced surface addresses this
    // substrate as zero.token.<op>; both spellings hit the same engine ops.
    match method {
        "zero.read" | "read" | "zero.token.read" => exec_read(engine, work_root, args),
        "zero.find" | "find" | "zero.token.find" => exec_find(engine, work_root, args, false),
        "zero.grep" | "grep" | "zero.token.grep" => exec_find(engine, work_root, args, true),
        "zero.glob" | "glob" | "zero.token.glob" => exec_glob(engine, work_root, args),
        "zero.tree" | "tree" | "zero.token.tree" => exec_tree(engine, work_root, args),
        "zero.shell" | "shell" | "zero.token.shell" => exec_shell(engine, work_root, args),
        "zero.edit" | "edit" | "zero.token.edit" => exec_edit(engine, work_root, args),
        "zero.token.expand" | "zero.expand" | "expand" => exec_expand(engine, args),
        "zero.token.expandMany" | "zero.expandMany" | "expandMany" | "expand_many" => {
            exec_expand_many(engine, args)
        }
        "zero.token.compact" | "zero.compact" | "compact" | "zero.ref" | "ref" => {
            exec_compact(engine, args)
        }
        "zero.token.compactMany" | "zero.compactMany" | "compactMany" | "compact_many" => {
            exec_compact_many(engine, args)
        }
        "zero.token.dedupe" | "zero.dedupe" | "dedupe" => exec_dedupe(args),
        "zero.compact_max" | "compact_max" => exec_compact_max(engine, args),
        "zero.ingest" | "ingest" => exec_ingest(engine, args),
        "zero.mem" | "mem" | "zero.token.mem" => exec_mem(engine),
        "zero.recall" | "recall" | "zero.token.recall" => exec_recall(engine, args),
        "zero.fetch" | "fetch" => exec_fetch(engine, args),
        "zero.cache_pack" | "cache_pack" | "cache-pack" => exec_cache_pack(engine, args),
        "zero.rewrite" | "rewrite" | "zero.token.rewrite" => exec_rewrite(args),
        "zero.discover" | "discover" => exec_discover(),
        "zero.batch" | "batch" => exec_batch(engine, args),
        "zero.pipe" | "pipe" => exec_pipe(engine, work_root, args),
        "zero.pick" | "pick" => exec_pick(args),
        "zero.filter_lines" | "filter_lines" => exec_filter_lines(args),
        "zero.count" | "count" => exec_count(args),
        "zero.first" | "first" => exec_first(args),
        "zero.verdict" | "verdict" => exec_verdict(args),
        "zero.raw" | "raw" => exec_raw(args),
        "zero.count_tokens" | "count_tokens" => exec_count_tokens(args),
        "zero.assert" | "assert" => exec_assert(args),
        "codemode.search" | "search" => {
            let query = args.first().and_then(|v| v.as_str()).unwrap_or("");
            Ok(OpOutcome::from_catalog(search_catalog(query)))
        }
        "codemode.describe" | "describe" => {
            let path = args.first().and_then(|v| v.as_str()).unwrap_or("");
            Ok(OpOutcome::from_catalog(describe_method(path)))
        }
        "codemode.limits" | "limits" => {
            Ok(OpOutcome::from_catalog(CodeModeLimits::default().as_json()))
        }
        "codemode.journalDoctor" | "journalDoctor" | "journal_doctor" => Ok(
            OpOutcome::from_catalog(journal_doctor_json(&engine.config.cache_path)),
        ),
        "codemode.journalInspect" | "journalInspect" | "journal_inspect" => {
            exec_journal_inspect(engine, args)
        }
        "codemode.journalResume" | "journalResume" | "journal_resume" => {
            exec_journal_resume(engine, args)
        }
        "codemode.journalRollback" | "journalRollback" | "journal_rollback" => {
            exec_journal_rollback(engine, args)
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

fn journal_execution_arg(args: &[Value]) -> Result<&str, Box<CodeModeResult>> {
    require_str_arg(args, 0, "journal command requires an execution_id string")
}

fn exec_journal_inspect(
    engine: &TokenZeroEngine,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let execution_id = journal_execution_arg(args)?;
    inspect_journal(&engine.config.cache_path, execution_id)
        .and_then(|journal| serde_json::to_value(journal).map_err(|err| err.to_string()))
        .map(OpOutcome::from_catalog)
        .map_err(|message| {
            Box::new(CodeModeResult::error_with_kind(
                "journal_inspect",
                message,
                0,
                false,
            ))
        })
}

fn exec_journal_resume(
    engine: &TokenZeroEngine,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let execution_id = journal_execution_arg(args)?;
    let journal = inspect_journal(&engine.config.cache_path, execution_id).map_err(|message| {
        Box::new(CodeModeResult::error_with_kind(
            "journal_resume",
            message,
            0,
            false,
        ))
    })?;
    if journal.state.is_resolved() {
        return Err(Box::new(CodeModeResult::error_with_kind(
            "journal_resume",
            format!("journal is already resolved as {:?}", journal.state),
            0,
            false,
        )));
    }
    Ok(OpOutcome::from_catalog(json!({
        "execution_id": execution_id,
        "state": journal.state,
        "resume": "rerun the original redacted plan with the same execution_id; idempotency keys and CAS checks skip completed mutations",
    })))
}

fn exec_journal_rollback(
    engine: &TokenZeroEngine,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let execution_id = journal_execution_arg(args)?;
    let mut transaction =
        open_unresolved(&engine.config.cache_path, execution_id).map_err(|message| {
            Box::new(CodeModeResult::error_with_kind(
                "journal_rollback",
                message,
                0,
                false,
            ))
        })?;
    let cache_path = engine.config.cache_path.clone();
    transaction
        .rollback("manual rollback requested", |operation| {
            rollback_journal_operation(&cache_path, operation)
        })
        .map_err(|error| {
            Box::new(CodeModeResult::error_with_kind(
                "journal_rollback",
                error.to_string(),
                0,
                false,
            ))
        })?;
    let journal = inspect_journal(&engine.config.cache_path, execution_id).map_err(|message| {
        Box::new(CodeModeResult::error_with_kind(
            "journal_rollback",
            message,
            0,
            false,
        ))
    })?;
    Ok(OpOutcome::from_catalog(json!({
        "execution_id": execution_id,
        "state": journal.state,
        "rolled_back": true,
    })))
}

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
    if resp.tool == "shell" {
        for record in &resp.refs {
            obj[format!("{}_ref", record.kind)] = json!(record.ref_id);
        }
    } else if !refs.is_empty() {
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

/// Counterfactual estimate of bytes that a full read would have cost but
/// that were avoided because graph queries, search hits, or ref expansion
/// satisfied the request without reading the whole source.
///
/// Methodology (honest, lower-bound):
/// - For search/glob/tree (graph-guided sufficiency and search precision),
///   the engine exposes `raw_tokens` (the full underlying output) and
///   `visible_tokens` (the compact summary actually shown). The difference is
///   the tokens we did not have to materialize in the response. We convert
///   those tokens to bytes using the visible text's actual bytes-per-token
///   ratio when available, otherwise a conservative 4 bytes/token fallback.
/// - For expand (demand paging), the exact payload bytes are counted as
///   prevented because the content was recovered from the cache/ref store
///   instead of re-reading the source file.
///
/// This is a counterfactual estimate: it assumes the agent would otherwise
/// have requested the full underlying content, and it does not include
/// unread files that produced no matches.
fn estimate_prevented_read_bytes(resp: &ToolResponse) -> usize {
    let accounting = match &resp.accounting {
        Some(acc) => acc,
        None => return 0,
    };
    let visible_text = resp.visible.as_ref().map(|v| v.text.as_str()).unwrap_or("");
    let mut prevented = 0usize;
    match resp.tool.as_str() {
        "find" | "grep" | "glob" | "tree" => {
            let bytes_per_token = visible_text
                .len()
                .checked_div(accounting.visible_tokens)
                .unwrap_or(PREVENTED_READ_BYTES_PER_TOKEN)
                .max(1);
            prevented = accounting
                .raw_tokens
                .saturating_sub(accounting.visible_tokens)
                .saturating_mul(bytes_per_token);
        }
        "expand" | "expand_many" => {
            prevented = visible_text.len();
        }
        _ => {}
    }
    prevented
}

#[derive(Debug)]
pub(crate) struct OpOutcome {
    value: Value,
    prevented_read_bytes: usize,
}

impl OpOutcome {
    fn from_tool_response(resp: &ToolResponse) -> Self {
        Self {
            value: tool_response_to_value(resp),
            prevented_read_bytes: estimate_prevented_read_bytes(resp),
        }
    }

    fn from_catalog(value: Value) -> Self {
        Self {
            value,
            prevented_read_bytes: 0,
        }
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

    fn mark_exact_expand(mut self) -> Self {
        if let Value::Object(map) = &mut self.value {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                record_exact_expand_payload(text);
            }
            map.insert(EXACT_EXPAND_MARKER.to_string(), Value::Bool(true));
        }
        self
    }

    fn visible_tokens(&self) -> usize {
        result_visible_tokens(&self.value)
    }

    fn raw_tokens(&self) -> usize {
        result_raw_tokens(&self.value)
    }

    fn prevented_read_bytes(&self) -> usize {
        self.prevented_read_bytes
    }

    fn with_prevented_read_bytes(mut self, bytes: usize) -> Self {
        self.prevented_read_bytes = bytes;
        self
    }
}

fn record_outcome(
    outcome: &OpOutcome,
    all_refs: &mut Vec<String>,
    total_visible: &mut usize,
    total_raw: &mut usize,
    total_prevented_read_bytes: &mut usize,
) {
    collect_refs(&outcome.value, all_refs);
    *total_visible += outcome.visible_tokens();
    *total_raw += outcome.raw_tokens();
    *total_prevented_read_bytes += outcome.prevented_read_bytes();
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

/// Collect path args, joining relative entries to `work_root` (wqw.5).
/// When the path arg is omitted, returns `[work_root]`.
fn paths_from_arg(args: &[Value], index: usize, work_root: PathBuf) -> Vec<PathBuf> {
    let raw = match args.get(index) {
        Some(Value::String(path)) => vec![PathBuf::from(path)],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|value| value.as_str().map(PathBuf::from))
            .collect(),
        _ => return vec![work_root],
    };
    resolve_paths_against_work_root(raw, &work_root)
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

/// Resolve plan paths against the CodeMode execute root (wqw.5).
///
/// Allowlist algorithm: effective roots = `allowed_roots_for_workspace(execute_root,
/// explicit_allowlist)` — always includes the call `root`, plus any configured
/// `--allowed-root` entries, deduped by canonical path. Relative paths are joined
/// to `work_root` (the execute root); absolute paths are kept as-is and still must
/// fall under an effective root. Paths outside every effective root are denied.
pub(crate) fn resolve_paths_against_work_root(
    paths: Vec<PathBuf>,
    work_root: &Path,
) -> Vec<PathBuf> {
    paths
        .into_iter()
        .map(|path| {
            if path.as_os_str().is_empty() {
                work_root.to_path_buf()
            } else if path.is_absolute() {
                path
            } else {
                work_root.join(path)
            }
        })
        .collect()
}

fn exec_read(
    engine: &TokenZeroEngine,
    work_root: &Path,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let paths = require_paths_from_arg(
        args,
        0,
        "zero.read requires a path string or array as first argument",
    )?;
    let paths = resolve_paths_against_work_root(paths, work_root);
    let opts = Opts::from_arg(args, 1);
    let mode = opts.mode_or("mode", Mode::Auto);
    let start_line = opts.usize("start_line");
    let end_line = opts.usize("end_line");
    let max_visible = opts
        .usize("max_visible_tokens")
        .unwrap_or(engine.config.max_visible_tokens);

    let resp = engine.read(&paths, mode, start_line, end_line, false, 20, max_visible);
    if resp.status == "error" {
        let message = resp
            .error
            .as_ref()
            .map(|error| error.message.clone())
            .unwrap_or_else(|| "zero.read failed".to_string());
        let code = resp
            .error
            .as_ref()
            .map(|error| error.code.as_str())
            .unwrap_or("");
        // wqw.5: outside allowlist is a hard plan error (clear deny), not a soft capsule.
        if code == "path_not_allowed" || code == "path_outside_allowed_roots" {
            return Err(Box::new(CodeModeResult::error_with_kind(
                "path_not_allowed",
                message,
                0,
                false,
            )));
        }
        let lower = message.to_ascii_lowercase();
        if lower.contains("__zerostack_missing_target__")
            || lower.contains("not found")
            || lower.contains("no such")
        {
            return Err(Box::new(CodeModeResult::error_with_kind(
                "substrate",
                message,
                0,
                false,
            )));
        }
    }
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
    let roots = resolve_paths_against_work_root(
        vec![
            args.first()
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .unwrap_or_else(|| work_root.to_path_buf()),
        ],
        work_root,
    );
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

fn exec_shell(
    engine: &TokenZeroEngine,
    work_root: &Path,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let command = require_str_arg(
        args,
        0,
        "zero.shell requires a command string as first argument",
    )?;
    let opts = Opts::from_arg(args, 1);
    // Relative cwd is always anchored to this plan's execute root. Explicit
    // allowed roots may precede the execute root in engine configuration, so
    // using allowed_roots.first() can silently run in the wrong project.
    let cwd = opts.str("cwd").map(|raw| {
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            path
        } else {
            work_root.join(path)
        }
    });
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
    work_root: &Path,
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

    // Resolve relative edit paths against the execute root (same as read).
    let path = resolve_paths_against_work_root(vec![path], work_root)
        .into_iter()
        .next()
        .unwrap_or_else(|| work_root.to_path_buf());

    let resp = engine.edit(
        &path,
        &edits,
        create,
        dry_run,
        Mode::Auto,
        engine.config.max_visible_tokens,
    );
    if resp.status == "error" {
        let message = resp
            .error
            .as_ref()
            .map(|e| e.message.clone())
            .unwrap_or_else(|| "zero.edit failed".to_string());
        // Expand/read health is intentionally independent of write health.
        // A missing ref must never authorize a native-write escape for an
        // unrelated edit failure.
        let annotated = crate::annotate_write_failure(&message, false);
        return Err(Box::new(CodeModeResult::error_with_kind(
            resp.error
                .as_ref()
                .map(|e| e.code.as_str())
                .unwrap_or("edit_failed"),
            annotated,
            0,
            false,
        )));
    }
    let hunks_applied = resp
        .telemetry
        .as_ref()
        .and_then(|t| t.get("hunks"))
        .and_then(|v| v.as_u64())
        .unwrap_or(edits.len() as u64);
    Ok(OpOutcome::from_tool_response(&resp).with_value(|value| {
        value["hunks_applied"] = json!(hunks_applied);
    }))
}

fn exec_expand(engine: &TokenZeroEngine, args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let params = ExpandParams::from_codemode_args(args)
        .map_err(|message| Box::new(CodeModeResult::error(message, 0)))?;
    if !tokenzero_recovery::is_expandable_ref(&params.ref_id) {
        return Err(Box::new(CodeModeResult::error(
            format!(
                "expand takes a tz:// fz:// gz:// ref; to read a file use zero.fs.compound('read',{{path}}) -- got: {}",
                params.ref_id
            ),
            0,
        )));
    }
    // Soft capsule on expand miss/error: plan continues with status in value.
    // Shared SurfaceHealth is updated inside expand_with_params (wqw.9).
    let resp = engine.expand_with_params(params);
    // Strict ZeroRef parsing can classify a missing legacy-shaped blob as malformed.
    // In CodeMode that is still an expand-surface miss for crash-only recovery (wqw.9).
    if resp
        .error
        .as_ref()
        .is_some_and(|error| error.code == "zeroref_malformed")
    {
        engine.surface_health().record_codemode_expand_x0();
    }
    let outcome = OpOutcome::from_tool_response(&resp);
    if resp.error.is_none() {
        Ok(outcome.mark_exact_expand())
    } else {
        Ok(outcome)
    }
}

fn exec_expand_many(
    engine: &TokenZeroEngine,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let items = match args.first() {
        Some(Value::Array(items)) => items,
        _ => {
            return Err(Box::new(CodeModeResult::error(
                "zero.token.expandMany requires an array of tz://, fz://, or gz:// refs or item objects",
                0,
            )));
        }
    };
    let mut results = Vec::with_capacity(items.len());
    let mut prevented = 0usize;
    for item in items {
        let params = ExpandParams::from_expand_many_item(item)
            .map_err(|message| Box::new(CodeModeResult::error(message, 0)))?;
        if !tokenzero_recovery::is_expandable_ref(&params.ref_id) {
            return Err(Box::new(CodeModeResult::error(
                format!(
                    "expandMany takes a tz:// fz:// gz:// ref; to read a file use zero.fs.compound('read',{{path}}) -- got: {}",
                    params.ref_id
                ),
                0,
            )));
        }
        let resp = engine.expand_with_params(params);
        prevented = prevented.saturating_add(estimate_prevented_read_bytes(&resp));
        results.push(
            OpOutcome::from_tool_response(&resp)
                .mark_exact_expand()
                .into_value(),
        );
    }
    Ok(OpOutcome::from_catalog(json!({
        "items": results,
        "count": results.len(),
    }))
    .with_prevented_read_bytes(prevented))
}

fn exec_compact(
    engine: &TokenZeroEngine,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    exec_compact_inner(engine, args, false)
}

fn exec_compact_many(
    engine: &TokenZeroEngine,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let items = match args.first() {
        Some(Value::Array(items)) => items.clone(),
        _ => {
            return Err(Box::new(CodeModeResult::error(
                "zero.token.compactMany requires an array of payloads",
                0,
            )));
        }
    };
    let mut results = Vec::with_capacity(items.len());
    let mut refs = Vec::new();
    for item in items {
        let outcome = exec_compact_inner(engine, &[item], false)?;
        collect_refs(outcome.as_value(), &mut refs);
        results.push(outcome.into_value());
    }
    Ok(OpOutcome::from_catalog(json!({
        "items": results,
        "count": results.len(),
        "refs": refs,
    })))
}

fn exec_dedupe(args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let items = match args.first() {
        Some(Value::Array(items)) => items,
        _ => {
            return Err(Box::new(CodeModeResult::error(
                "zero.token.dedupe requires an array",
                0,
            )));
        }
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut unique = Vec::new();
    for item in items {
        let key = serde_json::to_string(item).unwrap_or_default();
        if seen.insert(key) {
            unique.push(item.clone());
        }
    }
    Ok(OpOutcome::from_catalog(json!({
        "items": unique,
        "count": unique.len(),
    })))
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
    Ok(OpOutcome {
        value,
        prevented_read_bytes: 0,
    })
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
    let mut prevented = 0usize;
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
                    Value::String(s) => Expr::StringLit(s.clone()),
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
        prevented = prevented.saturating_add(outcome.prevented_read_bytes());
        let val = outcome.into_value();
        pipe_scope.insert("_prev".to_string(), val.clone());
        pipe_scope.insert(format!("_step{idx}"), val.clone());
        results.push(val);
    }
    let full = json!({
        "steps": results.len(),
        "results": results,
        "last": results.last().cloned().unwrap_or(Value::Null),
    });
    if args
        .get(1)
        .and_then(Value::as_object)
        .and_then(|opts| opts.get("raw"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(OpOutcome::from_catalog(full).with_prevented_read_bytes(prevented));
    }
    let text = serde_json::to_string(&full).unwrap_or_default();
    let content_type = detect_content_type(&text, None);
    let mut store = tokenzero_recovery::RecoveryStore::new(Some(engine.config.cache_path.clone()));
    let stored = store
        .store_payload(&text, content_type, None, None, None)
        .ok();
    let ref_id = stored
        .as_ref()
        .map(|s| s.blob_ref.as_str().to_string())
        .unwrap_or_default();
    if ref_id.is_empty() {
        return Ok(OpOutcome::from_catalog(full).with_prevented_read_bytes(prevented));
    }
    Ok(OpOutcome::from_catalog(json!({
        "ref": ref_id,
        "preview": first_line_preview(&text),
    }))
    .with_prevented_read_bytes(prevented))
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

fn exec_count(args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let value = args.first().ok_or_else(|| {
        Box::new(CodeModeResult::error(
            "zero.count requires a value as first argument",
            0,
        ))
    })?;
    let count = if let Some(items) = value.as_array() {
        items.len()
    } else {
        text_from_value(value)
            .map(|text| text.lines().count())
            .unwrap_or(0)
    };
    Ok(OpOutcome::from_catalog(json!(count)))
}

fn exec_first(args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let value = args.first().ok_or_else(|| {
        Box::new(CodeModeResult::error(
            "zero.first requires a value as first argument",
            0,
        ))
    })?;
    let n = args.get(1).and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
    if let Some(items) = value.as_array() {
        if n == 1 {
            return Ok(OpOutcome::from_catalog(
                items.first().cloned().unwrap_or(Value::Null),
            ));
        }
        return Ok(OpOutcome::from_catalog(Value::Array(
            items.iter().take(n).cloned().collect(),
        )));
    }
    let text = text_from_value(value).unwrap_or("");
    let lines = text.lines().take(n).collect::<Vec<_>>();
    let out = if n == 1 {
        lines.first().copied().unwrap_or("").to_string()
    } else {
        lines.join("\n")
    };
    Ok(OpOutcome::from_catalog(json!(out)))
}

fn exec_verdict(args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let ok = args.first().map(value_truthy).unwrap_or(false);
    let detail = args
        .get(1)
        .and_then(Value::as_str)
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    Ok(OpOutcome::from_catalog(
        json!({ "ok": ok, "detail": detail }),
    ))
}

fn exec_raw(args: &[Value]) -> Result<OpOutcome, Box<CodeModeResult>> {
    let value = args.first().cloned().unwrap_or(Value::Null);
    Ok(OpOutcome::from_catalog(
        json!({ "__tz_raw": true, "value": value }),
    ))
}

fn text_from_value(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("text").and_then(Value::as_str))
}

fn value_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
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

/// Byte-stable prefix length aligned to char boundaries, used for the
/// per-session prefix-cache hit-rate fallback estimate.
fn common_prefix_len(a: &str, b: &str) -> usize {
    let mut len = 0usize;
    for ((idx, ca), (_, cb)) in a.char_indices().zip(b.char_indices()) {
        if ca != cb {
            break;
        }
        len = idx + ca.len_utf8();
    }
    len
}
