//! CodeMode plan executor and TokenZero operation dispatch.

use rquickjs::{Context, Runtime, function::Func};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokenzero_core::{
    Mode, ToolResponse, count_tokens, detect_content_type, pack_to_token_boundary_with_char_limit,
};
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

/// Maximum number of recovery-cache sessions retained for fallback prefix telemetry.
const PREVIOUS_OUTPUT_MAX_SESSIONS: usize = 32;
/// Only the leading bytes can contribute to a future common-prefix hit.
const PREVIOUS_OUTPUT_MAX_PREFIX_BYTES: usize = 64 * 1024;

#[derive(Debug, Default)]
struct PreviousOutputLru {
    entries: HashMap<PathBuf, String>,
    oldest_first: std::collections::VecDeque<PathBuf>,
    stored_bytes: usize,
    recipes: HashMap<PathBuf, HashMap<String, String>>,
    recipe_oldest_first: VecDeque<PathBuf>,
}

impl PreviousOutputLru {
    fn observe(&mut self, cache_path: PathBuf, current: &str) -> usize {
        {
            let matched = self
                .entries
                .get(&cache_path)
                .map(|p| common_prefix_len(current, p))
                .unwrap_or(0);
            if let Some(pos) = self.oldest_first.iter().position(|k| k == &cache_path) {
                self.oldest_first.remove(pos);
            }
            if let Some(prev) = self.entries.remove(&cache_path) {
                self.stored_bytes = self.stored_bytes.saturating_sub(prev.len());
            }
            let mut end = current.len().min(PREVIOUS_OUTPUT_MAX_PREFIX_BYTES);
            while !current.is_char_boundary(end) {
                end -= 1;
            }
            let prefix = current[..end].to_owned();
            self.stored_bytes = self.stored_bytes.saturating_add(prefix.len());
            self.entries.insert(cache_path.clone(), prefix);
            self.oldest_first.push_back(cache_path);
            while self.entries.len() > PREVIOUS_OUTPUT_MAX_SESSIONS {
                let Some(oldest) = self.oldest_first.pop_front() else {
                    break;
                };
                if let Some(prev) = self.entries.remove(&oldest) {
                    self.stored_bytes = self.stored_bytes.saturating_sub(prev.len());
                }
            }
            matched
        }
    }
    const RECIPE_MAX_SESSIONS: usize = 32;
    const RECIPE_MAX_PER_SESSION: usize = 64;
    const RECIPE_MAX_SOURCE_BYTES: usize = 64 * 1024;

    fn touch_recipe_session(&mut self, cache_path: &Path) {
        if let Some(pos) = self
            .recipe_oldest_first
            .iter()
            .position(|k| k == cache_path)
        {
            self.recipe_oldest_first.remove(pos);
        }
        self.recipe_oldest_first.push_back(cache_path.to_path_buf());
    }

    fn recipe_session_mut(&mut self, cache_path: &Path) -> &mut HashMap<String, String> {
        if !self.recipes.contains_key(cache_path) {
            while self.recipes.len() >= Self::RECIPE_MAX_SESSIONS {
                let Some(oldest) = self.recipe_oldest_first.pop_front() else {
                    break;
                };
                self.recipes.remove(&oldest);
            }
            self.recipes
                .insert(cache_path.to_path_buf(), HashMap::new());
        }
        self.touch_recipe_session(cache_path);
        self.recipes
            .get_mut(cache_path)
            .expect("recipe session inserted above")
    }

    fn recipe_source(&mut self, cache_path: &Path, name: &str) -> Option<String> {
        let source = self
            .recipes
            .get(cache_path)
            .and_then(|r| r.get(name))
            .cloned();
        if source.is_some() {
            self.touch_recipe_session(cache_path);
        }
        source
    }

    fn recipe_names(&mut self, cache_path: &Path) -> Vec<String> {
        let mut names: Vec<String> = self
            .recipes
            .get(cache_path)
            .map(|r| r.keys().cloned().collect())
            .unwrap_or_default();
        if self.recipes.contains_key(cache_path) {
            self.touch_recipe_session(cache_path);
        }
        names.sort();
        names
    }
}

static PREVIOUS_OUTPUT_BY_SESSION: OnceLock<Mutex<PreviousOutputLru>> = OnceLock::new();

fn previous_output_by_session() -> &'static Mutex<PreviousOutputLru> {
    PREVIOUS_OUTPUT_BY_SESSION.get_or_init(|| Mutex::new(PreviousOutputLru::default()))
}

fn with_previous_outputs<R>(f: impl FnOnce(&mut PreviousOutputLru) -> R) -> R {
    let mut guard = previous_output_by_session()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
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
    let finish = |result, kind, steps| {
        finalize_codemode_result(result, kind, plan, started_ms, &options, &limits, steps)
    };
    if plan.len() > limits.max_code_bytes {
        return finish(
            CodeModeResult::error_with_kind(
                "validation",
                format!("plan exceeds max_code_bytes {}", limits.max_code_bytes),
                0,
                false,
            ),
            "code",
            Vec::new(),
        );
    }
    if plan.is_empty() {
        return finish(CodeModeResult::error("empty plan", 0), "code", Vec::new());
    }
    let catalog = plan
        .strip_prefix("search:")
        .map(|query| (search_catalog(query.trim()), "search", "codemode.search"))
        .or_else(|| {
            plan.strip_prefix("describe:").map(|target| {
                (
                    describe_method(target.trim()),
                    "describe",
                    "codemode.describe",
                )
            })
        });
    if let Some((result, id, method)) = catalog {
        let tokens = count_tokens(&serde_json::to_string_pretty(&result).unwrap_or_default());
        return finish(
            CodeModeResult::completed(result, Vec::new(), 1, tokens, tokens),
            "recipe",
            vec![ExecutionStep {
                id: id.to_string(),
                method: method.to_string(),
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
        Err(message) => return finish(CodeModeResult::error(message, 0), "code", Vec::new()),
    };
    let use_quickjs = should_run_quickjs(plan) || parse_plan(&lowered).is_err();
    if use_quickjs && quickjs_plan_requests_mutation(plan) {
        let message = crate::annotate_write_failure(
            concat!(
                "sandbox: mutating binding denied without transaction support ",
                "(use the lowered zero.edit / tz_edit path, not free-form JS mutation)",
            ),
            false,
        );
        return finish(
            CodeModeResult::error_with_kind("sandbox", message, 0, false),
            "code",
            Vec::new(),
        );
    }
    if use_quickjs {
        execute_quickjs_plan(plan, options, &limits, started_ms)
    } else {
        execute_lowered_plan(&lowered, options, &limits, "code", started_ms)
    }
}
fn should_run_quickjs(plan: &str) -> bool {
    if plan.contains("zero.register(") || plan.contains("zero.run(") || plan.contains("zero.list(")
    {
        return true;
    }
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

fn wall_clock_limit_error(elapsed: u64, limits: &CodeModeLimits) -> Option<(String, &'static str)> {
    if elapsed > limits.hard_max_wall_ms {
        Some((
            format!(
                "runtime: hard_max_wall_ms exceeded {}",
                limits.hard_max_wall_ms
            ),
            "hard wall clock exceeded",
        ))
    } else if elapsed > limits.max_wall_ms {
        Some((
            format!("runtime: max_wall_ms exceeded {}", limits.max_wall_ms),
            "wall clock exceeded",
        ))
    } else {
        None
    }
}
fn tz_error_json(message: &str, fallback: &str) -> String {
    serde_json::to_string(&json!({ "__tz_error": message }))
        .unwrap_or_else(|_| format!(r#"{{"__tz_error":"{fallback}"}}"#))
}

fn execute_quickjs_plan(
    plan: &str,
    options: CodeModeOptions,
    limits: &CodeModeLimits,
    started_ms: u128,
) -> CodeModeResult {
    let finish = |result, steps| {
        finalize_codemode_result(result, "code", plan, started_ms, &options, limits, steps)
    };
    let fail0 = |message: String| finish(CodeModeResult::error(message, 0), Vec::new());
    let fail_state = |state: &JsExecutionState, message: String| {
        finish(
            CodeModeResult::error(message, state.ops),
            state.steps.clone(),
        )
    };
    {
        let work_root = tokenzero_work_root(options.root.clone());
        let engine = Rc::new(make_engine_for_root_with_options(
            work_root.clone(),
            &options,
        ));
        let state = Rc::new(RefCell::new(JsExecutionState {
            started_ms,
            limits: limits.clone(),
            ..Default::default()
        }));
        let runtime = match Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => return fail0(format!("sandbox: QuickJS runtime init failed: {error}")),
        };
        runtime.set_memory_limit(limits.max_memory_bytes);
        runtime.set_max_stack_size(512 * 1024);
        let context = match Context::full(&runtime) {
            Ok(context) => context,
            Err(error) => return fail0(format!("sandbox: QuickJS context init failed: {error}")),
        };
        if let Err(error) = context.with(|ctx| {
            install_js_generic_binding(
                &ctx.globals(),
                Rc::clone(&engine),
                work_root.clone(),
                Rc::clone(&state),
            )?;
            ctx.eval::<(), _>(js_prelude())
        }) {
            return fail0(format!("sandbox: QuickJS binding setup failed: {error}"));
        }
        if let Err(error) = context.with(|ctx| ctx.eval::<(), _>(wrap_js_plan(plan).as_str())) {
            return fail_state(
                &state.borrow(),
                format!("sandbox: QuickJS eval failed: {error}"),
            );
        }
        let mut drained = 0;
        while runtime.is_job_pending() {
            let elapsed = now_ms().saturating_sub(started_ms) as u64;
            if let Some((message, _)) = wall_clock_limit_error(elapsed, limits) {
                return fail_state(&state.borrow(), message);
            }
            if drained >= limits.max_microtasks {
                return fail_state(
                    &state.borrow(),
                    format!("sandbox: microtask cap exceeded {}", limits.max_microtasks),
                );
            }
            if let Err(error) = runtime.execute_pending_job() {
                return fail_state(
                    &state.borrow(),
                    format!("sandbox: QuickJS job failed: {error}"),
                );
            }
            drained += 1;
        }
        let (result_json, error, error_kind): (Option<String>, Option<String>, Option<String>) =
            context
                .with(|ctx| {
                    let globals = ctx.globals();
                    Ok::<_, rquickjs::Error>((
                        globals.get("__tz_result")?,
                        globals.get("__tz_error")?,
                        globals.get("__tz_error_kind")?,
                    ))
                })
                .unwrap_or((
                    None,
                    Some("sandbox: result extraction failed".to_string()),
                    Some("sandbox".to_string()),
                ));
        let state = state.borrow();
        if let Some(error) = error {
            return finish(
                CodeModeResult::error_with_kind(
                    error_kind.as_deref().unwrap_or("sandbox"),
                    error,
                    state.ops,
                    false,
                ),
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
        set_prevented_read_bytes(&mut result, state.prevented_read_bytes);
        finish(result, state.steps.clone())
    }
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

fn invoke_js_binding(
    engine: &TokenZeroEngine,
    work_root: &Path,
    method: &str,
    mut args: Vec<Value>,
    state: &Rc<RefCell<JsExecutionState>>,
) -> String {
    {
        if matches!(method, "codemode.recipeRun" | "recipeRun" | "recipe_run") {
            args.push(json!(state.borrow().limits.max_code_bytes));
        }
        {
            let state_ref = state.borrow();
            let elapsed = now_ms().saturating_sub(state_ref.started_ms) as u64;
            let limit_err = wall_clock_limit_error(elapsed, &state_ref.limits).or_else(|| {
                if state_ref.ops >= state_ref.limits.max_logical_ops {
                    Some((
                        format!(
                            "runtime: max_logical_ops exceeded {}",
                            state_ref.limits.max_logical_ops
                        ),
                        "logical op cap exceeded",
                    ))
                } else if state_ref.physical_ops >= state_ref.limits.max_physical_ops {
                    Some((
                        format!(
                            "runtime: max_physical_ops exceeded {}",
                            state_ref.limits.max_physical_ops
                        ),
                        "physical op cap exceeded",
                    ))
                } else {
                    None
                }
            });
            if let Some((message, fallback)) = limit_err {
                return tz_error_json(&message, fallback);
            }
        }
        let outcome = match dispatch_values(engine, work_root, method, &args) {
            Ok(outcome) => outcome,
            Err(error) => {
                return serde_json::to_string(&json!({
                    "__tz_error": error.error.as_ref().map(|error| error.message.as_str()).unwrap_or("unknown error"),
                    "__tz_error_kind": error.error.as_ref().map(|error| error.kind.as_str()).unwrap_or("runtime"),
                })).unwrap_or_else(|_| "{\"__tz_error\":\"unknown error\",\"__tz_error_kind\":\"runtime\"}".to_string());
            }
        };
        let prevented_read_bytes = outcome.prevented_read_bytes;
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
            .saturating_add(result_token_field(&value, "visible_tokens"));
        state.raw_tokens = state
            .raw_tokens
            .saturating_add(result_token_field(&value, "raw_tokens"));
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
}

fn js_prelude() -> &'static str {
    r#"
        const __tz_parse = (text) => {
          const value = JSON.parse(text); if (value && value.__tz_exact_expand) { try { return JSON.parse(value.text); } catch (_) { return value.text; } }
          if (value && value.__tz_error) { const error = new Error(value.__tz_error); error.__tz_error_kind = value.__tz_error_kind || 'runtime'; throw error; }
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
        const __tz_deep_freeze = (value) => {
          if (value && typeof value === 'object' && !Object.isFrozen(value)) {
            for (const child of Object.values(value)) __tz_deep_freeze(child);
            Object.freeze(value);
          }
          return value;
        };
        const __tz_run_recipe = async (name, args = {}) => {
          const recipe = __tz_call('codemode.recipeRun', [String(name)]);
          const frozenArgs = __tz_deep_freeze(args === undefined ? {} : args);
          const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
          const invoke = new AsyncFunction('args', 'zero', 'ctx', 'token', recipe.source);
          return await invoke(frozenArgs, zero, ctx, token);
        };
        const __tz_bind = (method) => (...args) => __tz_call(method, args);
        const __tz_json_bind = (method) => (...args) => __tz_call(method, args.map(a => typeof a === 'string' ? a : JSON.stringify(a)));
        const __tz_methods = (prefix, names) => Object.fromEntries(names.map((name) => [name, __tz_bind(prefix + name)]));
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
          register: (name, source) => __tz_call('codemode.recipeRegister', [name, source]),
          run: (name, args = {}) => __tz_run_recipe(name, args),
          list: () => __tz_call('codemode.recipeList', []),
          ...__tz_methods('zero.', ['read','find','grep','glob','tree','shell','expand','ingest','mem','recall','fetch','cache_pack','rewrite','discover','batch','pipe','pick','filter_lines','count_tokens','assert']),
          compact: __tz_json_bind('zero.token.compact'),
          compact_max: __tz_json_bind('zero.compact_max'),
          queryMany: __tz_bind('zero.token.compactMany'),
          token: Object.freeze({
            compact: __tz_json_bind('zero.token.compact'),
            expand: __tz_bind('zero.token.expand'),
            compactMany: __tz_bind('zero.token.compactMany'),
            expandMany: __tz_bind('zero.token.expandMany'),
            dedupe: __tz_bind('zero.token.dedupe'),
            shell: __tz_bind('zero.shell'),
            job: __tz_bind('zero.token.job'),
            ...__tz_methods('zero.', ['read','find','grep','glob','tree','rewrite','mem','recall']),
          }),
          ref: __tz_json_bind('zero.token.compact'),
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
            globalThis.__tz_error_kind = String((err && err.__tz_error_kind) || 'sandbox');
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

struct PlanBootstrap {
    work_root: PathBuf,
    engine: TokenZeroEngine,
    transaction: Option<JournalTransaction>,
    downgrade: Option<String>,
}

#[allow(clippy::result_large_err)]
fn bootstrap_plan_executor(
    options: &CodeModeOptions,
    step_count: usize,
    limits: &CodeModeLimits,
    kind_label: &str,
    prepare: impl FnOnce(
        &TokenZeroEngine,
        &Path,
    ) -> Result<(Option<JournalTransaction>, Option<String>, bool), String>,
) -> Result<PlanBootstrap, CodeModeResult> {
    {
        if step_count > limits.max_logical_ops {
            return Err(CodeModeResult::error(
                format!(
                    "{kind_label}exceeds max_logical_ops {}",
                    limits.max_logical_ops
                ),
                0,
            ));
        }
        let work_root = tokenzero_work_root(options.root.clone());
        let engine = make_engine_for_root_with_options(work_root.clone(), options);
        let (transaction, downgrade, already_committed) = prepare(&engine, &work_root)
            .map_err(|message| CodeModeResult::error_with_kind("transaction", message, 0, false))?;
        if already_committed {
            return Err(CodeModeResult::completed(
                json!({"transaction": "already_committed", "idempotent_replay": true}),
                Vec::new(),
                0,
                0,
                0,
            ));
        }
        Ok(PlanBootstrap {
            work_root,
            engine,
            transaction,
            downgrade,
        })
    }
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

#[derive(Clone, Copy)]
enum JournalDispatchMode {
    /// Lowered plans: replay only when the op has a target; do not rollback on
    /// postcondition / mark_applied failures.
    Lowered,
    /// JSON plans: replay whenever the step does not need apply; rollback on
    /// postcondition / mark_applied failures.
    Json,
}

fn journal_op_replayed(
    transaction: &Option<JournalTransaction>,
    journal_index: usize,
    mode: JournalDispatchMode,
) -> bool {
    transaction.as_ref().is_some_and(|tx| {
        let needs = tx.step_needs_apply(journal_index);
        match mode {
            JournalDispatchMode::Lowered => tx
                .journal()
                .operations
                .get(journal_index)
                .is_some_and(|operation| operation.target.is_some() && !needs),
            JournalDispatchMode::Json => !needs,
        }
    })
}

fn tx_error(message: impl Into<String>, ops: usize) -> Box<CodeModeResult> {
    Box::new(CodeModeResult::error_with_kind(
        "transaction",
        message,
        ops,
        false,
    ))
}

fn dispatch_journaled(
    engine: &TokenZeroEngine,
    work_root: &Path,
    call: &MethodCall,
    scope: &Scope,
    transaction: &mut Option<JournalTransaction>,
    journal_index: usize,
    mode: JournalDispatchMode,
) -> OpResult {
    {
        let reversible = classify_method(&call.method) == OperationClass::ReversibleStoreMutation;
        let replayed = reversible && journal_op_replayed(transaction, journal_index, mode);
        if reversible && !replayed {
            if let Some(tx) = transaction.as_mut() {
                if let Err(original) = tx.mark_applying(journal_index) {
                    let combined = rollback_tx_on_error(tx, original, &engine.config.cache_path);
                    return Err(tx_error(combined, journal_index));
                }
            }
        }
        let outcome = if replayed {
            OpOutcome::from_catalog(json!({
                "idempotent_replay": true,
                "idempotency_key": transaction.as_ref().and_then(|tx| tx.journal().operations.get(journal_index)).map(|operation| operation.idempotency_key.clone()),
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
                        if let Err(combined) = tx.rollback(original, |operation| {
                            rollback_journal_operation(&engine.config.cache_path, operation)
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
        if reversible && !replayed {
            if let Some(tx) = transaction.as_mut() {
                let postcondition = match tx
                    .journal()
                    .operations
                    .get(journal_index)
                    .and_then(|operation| operation.target.as_deref())
                    .map(Path::new)
                    .map(current_digest)
                    .transpose()
                {
                    Ok(value) => value.flatten(),
                    Err(error) => {
                        let message = format!("read postcondition: {error}");
                        let message = if matches!(mode, JournalDispatchMode::Json) {
                            rollback_tx_on_error(tx, message, &engine.config.cache_path)
                        } else {
                            message
                        };
                        return Err(tx_error(message, journal_index + 1));
                    }
                };
                let compensation_refs = refs_from_value(outcome.as_value());
                if let Err(message) =
                    tx.mark_applied(journal_index, postcondition, compensation_refs)
                {
                    let message = if matches!(mode, JournalDispatchMode::Json) {
                        rollback_tx_on_error(tx, message, &engine.config.cache_path)
                    } else {
                        message
                    };
                    return Err(tx_error(message, journal_index + 1));
                }
            }
        }
        Ok(outcome)
    }
}

fn finish_lowered_transaction(
    result: &mut CodeModeResult,
    transaction: Option<JournalTransaction>,
    downgrade: Option<&str>,
) -> Result<(), String> {
    if let Some(reason) = downgrade {
        telemetry_insert(result, "transaction_atomic", json!(false));
        telemetry_insert(result, "transaction_downgrade", json!(reason));
    }
    if let Some(tx) = transaction {
        let journal = tx.commit()?;
        telemetry_insert(result, "transaction_atomic", json!(true));
        telemetry_insert(result, "plan_journal_version", json!(journal.version));
        telemetry_insert(result, "journal_state", json!(journal.state));
    }
    Ok(())
}

#[derive(Default)]
struct PlanProgress {
    scope: Scope,
    refs: Vec<String>,
    ops: usize,
    visible: usize,
    raw: usize,
    prevented: usize,
    last: Value,
    steps: Vec<ExecutionStep>,
}

impl PlanProgress {
    fn record(&mut self, id: String, call: &MethodCall, outcome: &OpOutcome) {
        self.steps.push(ExecutionStep {
            id,
            method: call.method.clone(),
            status: "completed".to_string(),
            refs: refs_from_value(outcome.as_value()),
        });
        record_outcome(
            outcome,
            &mut self.refs,
            &mut self.visible,
            &mut self.raw,
            &mut self.prevented,
        );
        self.last = outcome.as_value().clone();
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_lowered_result(
    value: Value,
    mut progress: PlanProgress,
    transaction: Option<JournalTransaction>,
    downgrade: Option<&str>,
    kind: &str,
    plan: &str,
    started_ms: u128,
    options: &CodeModeOptions,
    limits: &CodeModeLimits,
    decorate: impl FnOnce(&mut CodeModeResult),
) -> CodeModeResult {
    let ops = progress.ops;
    let mut result = completed_with_progress(value, &progress, ops);
    decorate(&mut result);
    if let Err(message) = finish_lowered_transaction(&mut result, transaction, downgrade) {
        result = CodeModeResult::error_with_kind("transaction", message, ops, false);
    }
    finalize_codemode_result(
        result,
        kind,
        plan,
        started_ms,
        options,
        limits,
        std::mem::take(&mut progress.steps),
    )
}

fn execute_lowered_plan(
    plan: &str,
    options: CodeModeOptions,
    limits: &CodeModeLimits,
    kind: &str,
    started_ms: u128,
) -> CodeModeResult {
    let finish = |result: CodeModeResult, steps: Vec<ExecutionStep>| {
        finalize_codemode_result(result, kind, plan, started_ms, &options, limits, steps)
    };
    let statements = match parse_plan(plan) {
        Ok(statements) => statements,
        Err(message) => return finish(CodeModeResult::error(message, 0), Vec::new()),
    };
    let mut boot = match bootstrap_plan_executor(
        &options,
        statements.len(),
        limits,
        "plan ",
        |engine, work_root| {
            prepare_lowered_transaction(engine, work_root, &statements, plan, started_ms)
        },
    ) {
        Ok(boot) => boot,
        Err(result) => return finish(result, Vec::new()),
    };

    let mut progress = PlanProgress::default();
    for (journal_index, statement) in statements.iter().enumerate() {
        let (id, call, binding) = match statement {
            Statement::Binding { name, call } => (name.clone(), call, Some(name)),
            Statement::Call(call) => (format!("step{}", progress.ops + 1), call, None),
            Statement::Return(expr) => {
                let value = match resolve_return(expr, &progress.scope) {
                    Ok(value) => value,
                    Err(message) => {
                        return finish(
                            CodeModeResult::error(message, progress.ops),
                            progress.steps,
                        );
                    }
                };
                return finish_lowered_result(
                    value,
                    progress,
                    boot.transaction,
                    boot.downgrade.as_deref(),
                    kind,
                    plan,
                    started_ms,
                    &options,
                    limits,
                    |_| {},
                );
            }
        };
        progress.ops += 1;
        let outcome = match dispatch_journaled(
            &boot.engine,
            &boot.work_root,
            call,
            &progress.scope,
            &mut boot.transaction,
            journal_index,
            JournalDispatchMode::Lowered,
        ) {
            Ok(outcome) => outcome,
            Err(mut error) => {
                stamp_ops_on_error(&mut error, progress.ops);
                return finish(*error, progress.steps);
            }
        };
        progress.record(id, call, &outcome);
        if let Some(name) = binding {
            progress.scope.insert(name.clone(), outcome.into_value());
        }
    }
    let value = std::mem::take(&mut progress.last);
    finish_lowered_result(
        value,
        progress,
        boot.transaction,
        boot.downgrade.as_deref(),
        kind,
        plan,
        started_ms,
        &options,
        limits,
        |_| {},
    )
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
    {
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
                        let preview_budget = budget_tokens.saturating_sub(count_tokens(&ref_id));
                        return json!({ "ref": ref_id, "preview": first_line_preview(&text, preview_budget) });
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
                        .map(|(key, value)| {
                            (key, ref_first_value(value, budget_tokens, store, refs))
                        })
                        .collect(),
                )
            }
            other => other,
        }
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

fn first_line_preview(text: &str, max_tokens: usize) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    pack_to_token_boundary_with_char_limit(line, max_tokens, 32).to_string()
}

fn telemetry_insert(result: &mut CodeModeResult, key: &str, value: Value) {
    let extra = result.telemetry.extra.get_or_insert_with(|| json!({}));
    if let Some(obj) = extra.as_object_mut() {
        obj.insert(key.to_string(), value);
    }
}

fn rollback_tx_on_error(
    tx: &mut JournalTransaction,
    original: String,
    cache_path: &Path,
) -> String {
    tx.rollback(original.clone(), |operation| {
        rollback_journal_operation(cache_path, operation)
    })
    .err()
    .map(|error| error.to_string())
    .unwrap_or(original)
}

type OpResult = Result<OpOutcome, Box<CodeModeResult>>;
type Scope = HashMap<String, Value>;

fn catalog(value: Value) -> OpResult {
    Ok(OpOutcome::from_catalog(value))
}
fn tool(response: ToolResponse) -> OpResult {
    Ok(OpOutcome::from_tool_response(&response))
}

fn boxed_error(kind: &str, message: impl Into<String>) -> Box<CodeModeResult> {
    Box::new(CodeModeResult::error_with_kind(kind, message, 0, false))
}

fn operation_error(message: impl Into<String>) -> Box<CodeModeResult> {
    Box::new(CodeModeResult::error(message.into(), 0))
}

fn map_journal_err(kind: &'static str) -> impl FnOnce(String) -> Box<CodeModeResult> {
    move |message| boxed_error(kind, message)
}

fn stamp_ops_on_error(error: &mut CodeModeResult, ops: usize) {
    telemetry_insert(error, "operations", json!(ops));
    error.telemetry.operations = ops;
    error.telemetry.logical_ops = ops;
}

fn set_prevented_read_bytes(result: &mut CodeModeResult, prevented: usize) {
    result.telemetry.prevented_read_bytes = prevented;
    telemetry_insert(result, "prevented_read_bytes", json!(prevented));
}

fn completed_with_progress(value: Value, progress: &PlanProgress, ops: usize) -> CodeModeResult {
    let visible =
        progress.visible + count_tokens(&serde_json::to_string(&value).unwrap_or_default());
    let mut result =
        CodeModeResult::completed(value, progress.refs.clone(), ops, visible, progress.raw);
    set_prevented_read_bytes(&mut result, progress.prevented);
    result
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
    {
        let work_root = tokenzero_work_root(options.root.clone());
        let root_was_explicit = options.root.is_some();
        let engine = make_engine_for_root_with_options(work_root.clone(), options);
        let journal_health = journal_doctor_json(&engine.config.cache_path);
        telemetry_insert(&mut result, "plan_journals", journal_health);
        telemetry_insert(
            &mut result,
            "work_root",
            json!(work_root.display().to_string()),
        );
        telemetry_insert(&mut result, "root_explicit", json!(root_was_explicit));
        if !root_was_explicit {
            let warning = format!(
                "no root set; falling back to server/process cwd {}",
                work_root.display()
            );
            telemetry_insert(&mut result, "root_fallback_warning", json!(warning));
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
        let physical_ops = result.telemetry.physical_ops;
        result.telemetry.operations = operations;
        result.telemetry.logical_ops = operations;
        result.telemetry.physical_ops = physical_ops;
        result.telemetry.batched_ops = if operations > physical_ops { 1 } else { 0 };
        result.telemetry.internal_actions = operations.saturating_add(result.refs.len());
        result.telemetry.payload_tokens = payload_tokens;
        result.telemetry.envelope_tokens = count_tokens(&result.visible_ack);
        result.telemetry.ack_tokens = count_tokens(&result.visible_ack);
        let ref_strings = result.refs.join(" ");
        result.telemetry.ref_string_tokens = count_tokens(&ref_strings);
        result.telemetry.framing_tokens = result
            .telemetry
            .envelope_tokens
            .saturating_sub(result.telemetry.ack_tokens)
            .saturating_sub(result.telemetry.ref_string_tokens);
        result.telemetry.preview_tokens =
            payload_tokens.saturating_sub(result.telemetry.ref_string_tokens);
        let current_output = serde_json::to_string(result.value.as_ref().unwrap_or(&Value::Null))
            .unwrap_or_default();
        let (cached_tokens, total_output_tokens) = {
            let cache_path = engine.config.cache_path.clone();
            let matched_bytes =
                with_previous_outputs(|outputs| outputs.observe(cache_path, &current_output));
            (
                count_tokens(&current_output[..matched_bytes]),
                count_tokens(&current_output),
            )
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
            let cache_rate = if result.telemetry.prefix_cache_total == 0 {
                0.0
            } else {
                result.telemetry.prefix_cache_hits as f64
                    / result.telemetry.prefix_cache_total as f64
            };
            for (key, value) in [
                ("refs_count", json!(result.refs.len())),
                ("payload_tokens", json!(result.telemetry.payload_tokens)),
                ("envelope_tokens", json!(result.telemetry.envelope_tokens)),
                ("ack_tokens", json!(result.telemetry.ack_tokens)),
                (
                    "ref_string_tokens",
                    json!(result.telemetry.ref_string_tokens),
                ),
                ("framing_tokens", json!(result.telemetry.framing_tokens)),
                ("preview_tokens", json!(result.telemetry.preview_tokens)),
                (
                    "prevented_read_bytes",
                    json!(result.telemetry.prevented_read_bytes),
                ),
                (
                    "prefix_cache_hits",
                    json!(result.telemetry.prefix_cache_hits),
                ),
                (
                    "prefix_cache_total",
                    json!(result.telemetry.prefix_cache_total),
                ),
                ("prefix_cache_hit_rate", json!(cache_rate)),
            ] {
                extra.insert(key.to_string(), value);
            }
        }
        result.telemetry.refs_count = Some(result.refs.len());
        let mut finalized = finalize_result(
            result,
            kind,
            plan,
            started_ms,
            now_ms(),
            ExecutionStore::new(engine.config.cache_path.clone()),
            limits,
            steps,
        );
        // One-line warning after store finalization (which resets visible_ack to C/X0).
        if let Some(warning) = finalized
            .telemetry
            .extra
            .as_ref()
            .and_then(|extra| extra.get("root_fallback_warning"))
            .and_then(Value::as_str)
        {
            finalized.visible_ack = format!(
                "{}\n# warning: root_fallback: {warning}",
                finalized.visible_ack.trim_end()
            );
        }
        finalized
    }
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
    {
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
}

fn jstr<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
fn jstring(value: &Value, key: &str) -> Option<String> {
    jstr(value, key).map(str::to_string)
}
fn jbool(value: &Value, key: &str, default: bool) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(default)
}
fn prepare_json_transaction(
    engine: &TokenZeroEngine,
    work_root: &Path,
    parsed: &Value,
    steps: &[Value],
    plan: &str,
    started_ms: u128,
) -> Result<(Option<JournalTransaction>, Option<String>, bool), String> {
    {
        let execution_id =
            jstring(parsed, "execution_id").unwrap_or_else(|| execution_id(plan, started_ms));
        let plan_id = jstring(parsed, "plan_id").unwrap_or_else(|| sha256_bytes(plan.as_bytes()));
        let atomic_requested = jbool(parsed, "atomic", false);
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
            Err(message) if message.starts_with("journal not found:") => {}
            Err(message) => return Err(message),
        }
        let mut seen_targets = std::collections::HashSet::new();
        let mut specs = Vec::with_capacity(steps.len());
        for (index, step) in steps.iter().enumerate() {
            let id = jstring(step, "id").unwrap_or_else(|| format!("step{index}"));
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
                let path = step.get("args").and_then(Value::as_array).and_then(|args| args.first()).and_then(Value::as_str).ok_or_else(|| format!("journaled edit step {index} requires a literal path; dynamic targets are not CAS-safe"))?;
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
                let text = String::from_utf8(bytes.clone()).map_err(|_| {
                    format!("journaled edit target {} is not UTF-8", path.display())
                })?;
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
    let finish = |result, steps| {
        finalize_codemode_result(result, "json", plan, started_ms, &options, limits, steps)
    };
    let parsed: Value = match serde_json::from_str(plan) {
        Ok(value) => value,
        Err(error) => {
            return finish(
                CodeModeResult::error(format!("json plan parse error: {error}"), 0),
                Vec::new(),
            );
        }
    };
    let steps = match parsed.get("steps").unwrap_or(&parsed).as_array() {
        Some(steps) => steps,
        None => {
            return finish(
                CodeModeResult::error("json plan requires a steps array".to_string(), 0),
                Vec::new(),
            );
        }
    };
    let mut boot = match bootstrap_plan_executor(
        &options,
        steps.len(),
        limits,
        "json plan ",
        |engine, work_root| {
            prepare_json_transaction(engine, work_root, &parsed, steps, plan, started_ms)
        },
    ) {
        Ok(boot) => boot,
        Err(result) => return finish(result, Vec::new()),
    };

    let mut progress = PlanProgress::default();
    for (index, step) in steps.iter().enumerate() {
        let id = step
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("step{index}"));
        let method = match step
            .get("method")
            .or_else(|| step.get("tool"))
            .and_then(Value::as_str)
        {
            Some(method) => method.to_string(),
            None => {
                return finish(
                    CodeModeResult::error(format!("json plan step {index} missing method"), index),
                    progress.steps,
                );
            }
        };
        let args = match json_args_to_exprs(step.get("args"), &progress.scope) {
            Ok(args) => args,
            Err(message) => return finish(CodeModeResult::error(message, index), progress.steps),
        };
        let call = MethodCall {
            method: method.clone(),
            args,
        };
        progress.ops = index + 1;
        let outcome = match dispatch_journaled(
            &boot.engine,
            &boot.work_root,
            &call,
            &progress.scope,
            &mut boot.transaction,
            index,
            JournalDispatchMode::Json,
        ) {
            Ok(outcome) => outcome,
            Err(mut error) => {
                stamp_ops_on_error(&mut error, progress.ops);
                return finish(*error, progress.steps);
            }
        };
        progress.record(id.clone(), &call, &outcome);
        progress.scope.insert(id, outcome.into_value());
    }

    let last = std::mem::take(&mut progress.last);
    let value = parsed
        .get("return")
        .and_then(|value| resolve_json_binding(value, &progress.scope).ok())
        .unwrap_or(last);
    progress.ops = steps.len();
    finish_lowered_result(
        value,
        progress,
        boot.transaction,
        boot.downgrade.as_deref(),
        "json",
        plan,
        started_ms,
        &options,
        limits,
        |result| {
            let parallel_groups = count_parallel_groups(steps);
            telemetry_insert(result, "parallel_groups", json!(parallel_groups));
            result.telemetry.parallel_groups = Some(parallel_groups);
            result.telemetry.physical_ops = steps.len();
        },
    )
}

fn json_args_to_exprs(value: Option<&Value>, scope: &Scope) -> Result<Vec<Expr>, String> {
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

fn resolve_json_binding(value: &Value, scope: &Scope) -> Result<Value, String> {
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

fn resolve_binding_string(s: &str, scope: &Scope) -> Result<Value, String> {
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

fn refs_from_value(value: &Value) -> Vec<String> {
    let mut refs = Vec::new();
    collect_refs(value, &mut refs);
    refs
}

fn dispatch(
    engine: &TokenZeroEngine,
    work_root: &Path,
    call: &MethodCall,
    scope: &Scope,
) -> OpResult {
    let args: Vec<Value> = call
        .args
        .iter()
        .map(|arg| resolve_expr(arg, scope).map_err(operation_error))
        .collect::<Result<Vec<_>, _>>()?;
    dispatch_values(engine, work_root, &call.method, &args)
}

fn dispatch_values(
    engine: &TokenZeroEngine,
    work_root: &Path,
    method: &str,
    args: &[Value],
) -> OpResult {
    if matches!(
        method,
        "codemode.recipeRegister" | "recipeRegister" | "recipe_register"
    ) {
        let name = require_str_arg(args, 0, "recipe register requires a name string")?;
        let source = require_str_arg(args, 1, "recipe register requires a source string")?;
        if source.trim().is_empty() {
            return Err(boxed_error("validation", "recipe source must not be empty"));
        }
        if source.len() > PreviousOutputLru::RECIPE_MAX_SOURCE_BYTES {
            return Err(boxed_error(
                "recipe_source_too_large",
                format!(
                    "recipe source exceeds {} bytes",
                    PreviousOutputLru::RECIPE_MAX_SOURCE_BYTES
                ),
            ));
        }
        return with_previous_outputs(|registry| {
            let recipes = registry.recipe_session_mut(&engine.config.cache_path);
            if !recipes.contains_key(name)
                && recipes.len() >= PreviousOutputLru::RECIPE_MAX_PER_SESSION
            {
                return Err(boxed_error(
                    "recipe_registry_full",
                    format!(
                        "recipe registry is limited to {} recipes per session",
                        PreviousOutputLru::RECIPE_MAX_PER_SESSION
                    ),
                ));
            }
            recipes.insert(name.to_string(), source.to_string());
            catalog(json!({"name": name, "registered": true}))
        });
    }
    if matches!(method, "codemode.recipeList" | "recipeList" | "recipe_list") {
        return catalog(json!(with_previous_outputs(|registry| {
            registry.recipe_names(&engine.config.cache_path)
        })));
    }
    if matches!(method, "codemode.recipeRun" | "recipeRun" | "recipe_run") {
        let name = require_str_arg(args, 0, "recipe run requires a name string")?;
        let max_code_bytes = args
            .get(1)
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or_else(|| CodeModeLimits::default().max_code_bytes);
        let source = with_previous_outputs(|registry| {
            registry.recipe_source(&engine.config.cache_path, name)
        })
        .ok_or_else(|| boxed_error("recipe_not_found", format!("recipe not found: {name}")))?;
        if source.trim().is_empty() {
            return Err(boxed_error("validation", "recipe source must not be empty"));
        }
        if source.len() > max_code_bytes {
            return Err(boxed_error(
                "validation",
                format!("recipe exceeds max_code_bytes {max_code_bytes}"),
            ));
        }
        if quickjs_plan_requests_mutation(&source) {
            return Err(boxed_error(
                "sandbox",
                "sandbox: mutating binding denied without transaction support",
            ));
        }
        return catalog(json!({"source": source}));
    }
    match method {
        "zero.read" | "read" | "zero.token.read" => exec_read(engine, work_root, args),
        "zero.find" | "find" | "zero.token.find" => exec_find(engine, work_root, args, false),
        "zero.grep" | "grep" | "zero.token.grep" => exec_find(engine, work_root, args, true),
        "zero.glob" | "glob" | "zero.token.glob" => exec_glob(engine, work_root, args),
        "zero.tree" | "tree" | "zero.token.tree" => exec_tree(engine, work_root, args),
        "zero.shell" | "shell" | "zero.token.shell" => exec_shell(engine, work_root, args),
        "zero.job" | "job" | "zero.token.job" => exec_job(engine, args),
        "zero.edit" | "edit" | "zero.token.edit" => exec_edit(engine, work_root, args),
        "zero.token.expand" | "zero.expand" | "expand" => exec_expand(engine, args),
        "zero.token.expandMany" | "zero.expandMany" | "expandMany" | "expand_many" => {
            exec_expand_many(engine, args)
        }
        "zero.token.compact" | "zero.compact" | "compact" | "zero.ref" | "ref" => {
            exec_compact_inner(engine, args, false)
        }
        "zero.token.compactMany" | "zero.compactMany" | "compactMany" | "compact_many" => {
            exec_compact_many(engine, args)
        }
        "zero.token.dedupe" | "zero.dedupe" | "dedupe" => exec_dedupe(args),
        "zero.compact_max" | "compact_max" => exec_compact_inner(engine, args, true),
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
            catalog(search_catalog(query))
        }
        "codemode.describe" | "describe" => {
            let path = args.first().and_then(|v| v.as_str()).unwrap_or("");
            catalog(describe_method(path))
        }
        "codemode.limits" | "limits" => catalog(CodeModeLimits::default().as_json()),
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
        _ => Err(operation_error(format!(
            "unknown method: {method}. Use codemode.search() to discover available methods"
        ))),
    }
}

fn journal_execution_arg(args: &[Value]) -> Result<&str, Box<CodeModeResult>> {
    require_str_arg(args, 0, "journal command requires an execution_id string")
}

macro_rules! op_tool {
    ($name:ident ($($arg:ident : $ty:ty),*) => $body:expr) => {
        fn $name($($arg : $ty),*) -> OpResult { tool($body) }
    };
}
macro_rules! op_catalog {
    ($name:ident ($($arg:ident : $ty:ty),*) => $body:expr) => {
        fn $name($($arg : $ty),*) -> OpResult { catalog($body) }
    };
}
op_tool!(exec_mem(engine: &TokenZeroEngine) => engine.mem());
op_catalog!(exec_discover() => serde_json::to_value(discover()).unwrap_or(Value::Null));
op_catalog!(exec_raw(args: &[Value]) => json!({ "__tz_raw": true, "value": args.first().cloned().unwrap_or(Value::Null) }));

fn exec_ingest(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let text = require_str_arg(args, 0, "zero.ingest requires text as first argument")?;
    let opts = Opts::from_arg(args, 1);
    let mode = opts.mode_or("mode", Mode::Auto);
    let source = opts.str("source").unwrap_or("codemode-ingest");
    let content_type = detect_content_type(text, None);

    let resp = engine.ingest(text, content_type, mode, source);
    tool(resp)
}

fn exec_rewrite(args: &[Value]) -> OpResult {
    let command = require_str_arg(
        args,
        0,
        "zero.rewrite requires a command string as first argument",
    )?;
    let opts = Opts::from_arg(args, 1);
    let mode = opts.str("mode").unwrap_or("safe");
    let value = serde_json::to_value(rewrite_command(command, mode, true))
        .map_err(|err| operation_error(format!("zero.rewrite failed: {err}")))?;
    catalog(value)
}

fn exec_cache_pack(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let opts = Opts::from_arg(args, 0);
    let scope = opts.str("scope").unwrap_or("agent");
    let resp = engine.cache_pack(scope);
    tool(resp)
}

fn exec_recall(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
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
    tool(resp)
}

fn exec_fetch(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
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
    tool(resp)
}

fn exec_batch(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let ops = match args.first() {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::String(text)) => serde_json::from_str(text)
            .map_err(|err| operation_error(format!("zero.batch ops is not valid JSON: {err}")))?,
        _ => {
            return Err(operation_error(
                "zero.batch requires an array of {tool, args} objects as first argument",
            ));
        }
    };
    if ops.is_empty() {
        return Err(operation_error("zero.batch requires at least one op"));
    }
    let wrapped = json!({"ops": ops, "mode": "auto"});
    match crate::tools::batch_response(engine, &wrapped) {
        Ok(resp) => tool(resp),
        Err(error) => Err(operation_error(error.message_text())),
    }
}

fn exec_compact_many(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let items = require_array_arg(
        args,
        0,
        "zero.token.compactMany requires an array of payloads",
    )?;
    let mut results = Vec::with_capacity(items.len());
    let mut refs = Vec::new();
    for item in items.iter().cloned() {
        let outcome = exec_compact_inner(engine, &[item], false)?;
        collect_refs(outcome.as_value(), &mut refs);
        results.push(outcome.into_value());
    }
    catalog(json!({
        "items": results,
        "count": results.len(),
        "refs": refs,
    }))
}

fn exec_job(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    engine
        .shell_job(require_str_arg(
            args,
            0,
            "zero.token.job requires a job id string",
        )?)
        .map(OpOutcome::from_catalog)
        .map_err(operation_error)
}
op_catalog!(exec_count_tokens(args: &[Value]) => json!(count_tokens(text_from_value(args.first().unwrap_or(&Value::Null)).unwrap_or(""))));
fn exec_verdict(args: &[Value]) -> OpResult {
    let ok = args.first().map(value_truthy).unwrap_or(false);
    let detail = args
        .get(1)
        .and_then(Value::as_str)
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    catalog(json!({ "ok": ok, "detail": detail }))
}
fn exec_assert(args: &[Value]) -> OpResult {
    if !value_truthy(args.first().unwrap_or(&Value::Null)) {
        return Err(operation_error(
            args.get(1)
                .and_then(Value::as_str)
                .unwrap_or("assertion failed"),
        ));
    }
    catalog(json!(true))
}
fn exec_count(args: &[Value]) -> OpResult {
    let value = args
        .first()
        .ok_or_else(|| operation_error("zero.count requires a value as first argument"))?;
    let count = value.as_array().map(|i| i.len()).unwrap_or_else(|| {
        text_from_value(value)
            .map(|t| t.lines().count())
            .unwrap_or(0)
    });
    catalog(json!(count))
}
fn exec_dedupe(args: &[Value]) -> OpResult {
    let items = require_array_arg(args, 0, "zero.dedupe requires an array as first argument")?;
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in items {
        if seen.insert(serde_json::to_string(item).unwrap_or_default()) {
            out.push(item.clone());
        }
    }
    catalog(Value::Array(out))
}
fn exec_filter_lines(args: &[Value]) -> OpResult {
    let text = text_from_value(
        args.first()
            .ok_or_else(|| operation_error("zero.filter_lines requires text"))?,
    )
    .unwrap_or("");
    let pattern = require_str_arg(
        args,
        1,
        "zero.filter_lines requires a pattern string as second argument",
    )?;
    catalog(json!(
        text.lines()
            .filter(|l| l.contains(pattern))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}
fn exec_journal_inspect(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
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

fn exec_journal_resume(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let execution_id = journal_execution_arg(args)?;
    let journal = inspect_journal(&engine.config.cache_path, execution_id)
        .map_err(map_journal_err("journal_resume"))?;
    if journal.state.is_resolved() {
        return Err(boxed_error(
            "journal_resume",
            format!("journal is already resolved as {:?}", journal.state),
        ));
    }
    catalog(json!({
        "execution_id": execution_id,
        "state": journal.state,
        "resume": "rerun the original redacted plan with the same execution_id; idempotency keys and CAS checks skip completed mutations",
    }))
}

fn exec_journal_rollback(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let execution_id = journal_execution_arg(args)?;
    let mut transaction = open_unresolved(&engine.config.cache_path, execution_id)
        .map_err(map_journal_err("journal_rollback"))?;
    let cache_path = engine.config.cache_path.clone();
    transaction
        .rollback("manual rollback requested", |operation| {
            rollback_journal_operation(&cache_path, operation)
        })
        .map_err(|error| boxed_error("journal_rollback", error.to_string()))?;
    let journal = inspect_journal(&engine.config.cache_path, execution_id)
        .map_err(map_journal_err("journal_rollback"))?;
    catalog(json!({
        "execution_id": execution_id,
        "state": journal.state,
        "rolled_back": true,
    }))
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
    *total_visible += result_token_field(&outcome.value, "visible_tokens");
    *total_raw += result_token_field(&outcome.value, "raw_tokens");
    *total_prevented_read_bytes += outcome.prevented_read_bytes;
}

struct Opts<'a>(Option<&'a serde_json::Map<String, Value>>);

impl<'a> Opts<'a> {
    fn from_arg(args: &'a [Value], index: usize) -> Self {
        Self(args.get(index).and_then(|v| v.as_object()))
    }

    fn usize(&self, key: &str) -> Option<usize> {
        self.0?.get(key)?.as_u64().map(|n| n as usize)
    }

    fn usize_or(&self, key: &str, default: usize) -> usize {
        self.usize(key).unwrap_or(default)
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

    fn max_visible(&self, engine: &TokenZeroEngine) -> usize {
        self.usize_or("max_visible_tokens", engine.config.max_visible_tokens)
    }
}

fn array_or_json_arg(
    args: &[Value],
    required: &str,
    bad_json: impl FnOnce(&serde_json::Error) -> String,
) -> Result<Vec<Value>, Box<CodeModeResult>> {
    match args.first() {
        Some(Value::Array(items)) => Ok(items.clone()),
        Some(Value::String(text)) => {
            serde_json::from_str(text).map_err(|err| operation_error(bad_json(&err)))
        }
        _ => Err(operation_error(required.to_string())),
    }
}

fn data_arg(args: &[Value], message: &str) -> Result<String, Box<CodeModeResult>> {
    match args.first() {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(other) => Ok(serde_json::to_string(other).unwrap_or_default()),
        None => Err(operation_error(message.to_string())),
    }
}

fn require_str_arg<'a>(
    args: &'a [Value],
    index: usize,
    message: &str,
) -> Result<&'a str, Box<CodeModeResult>> {
    args.get(index)
        .and_then(|value| value.as_str())
        .ok_or_else(|| operation_error(message.to_string()))
}

fn require_array_arg<'a>(
    args: &'a [Value],
    index: usize,
    message: &str,
) -> Result<&'a [Value], Box<CodeModeResult>> {
    args.get(index)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| operation_error(message.to_string()))
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
                Err(operation_error(message.to_string()))
            } else {
                Ok(paths)
            }
        }
        _ => Err(operation_error(message.to_string())),
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

fn exec_read(engine: &TokenZeroEngine, work_root: &Path, args: &[Value]) -> OpResult {
    {
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
        let max_visible = opts.max_visible(engine);
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
            if code == "path_not_allowed" || code == "path_outside_allowed_roots" {
                return Err(boxed_error("path_not_allowed", message));
            }
            let lower = message.to_ascii_lowercase();
            if lower.contains("__zerostack_missing_target__")
                || lower.contains("not found")
                || lower.contains("no such")
            {
                return Err(boxed_error("substrate", message));
            }
        }
        tool(resp)
    }
}

fn exec_find(engine: &TokenZeroEngine, work_root: &Path, args: &[Value], exact: bool) -> OpResult {
    let pattern = require_str_arg(
        args,
        0,
        "zero.find/grep requires a pattern string as first argument",
    )?;
    let paths = paths_from_arg(args, 1, work_root.to_path_buf());
    let opts = Opts::from_arg(args, 2);
    let mode = opts.mode_or("mode", Mode::Auto);
    let max_files = opts.usize_or("max_files", 20);
    let max_visible = opts.max_visible(engine);

    let resp = if exact {
        engine.grep(pattern, &paths, mode, max_files, max_visible)
    } else {
        engine.find(pattern, &paths, mode, max_files, max_visible)
    };
    tool(resp)
}

fn exec_glob(engine: &TokenZeroEngine, work_root: &Path, args: &[Value]) -> OpResult {
    let pattern = require_str_arg(
        args,
        0,
        "zero.glob requires a pattern string as first argument",
    )?;
    let paths = paths_from_arg(args, 1, work_root.to_path_buf());
    let max_files = Opts::from_arg(args, 2).usize_or("max_files", 200);

    let resp = engine.glob(
        pattern,
        &paths,
        false,
        Mode::Auto,
        max_files,
        engine.config.max_visible_tokens,
    );
    tool(resp)
}

fn exec_tree(engine: &TokenZeroEngine, work_root: &Path, args: &[Value]) -> OpResult {
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
    let depth = opts.usize_or("depth", 3);
    let include_hidden = opts.bool("include_hidden").unwrap_or(false);
    let max_files = opts.usize_or("max_files", 200);

    let resp = engine.tree(
        &roots,
        depth,
        include_hidden,
        Mode::Auto,
        max_files,
        engine.config.max_visible_tokens,
    );
    tool(resp)
}

fn exec_shell(engine: &TokenZeroEngine, work_root: &Path, args: &[Value]) -> OpResult {
    {
        let command = require_str_arg(
            args,
            0,
            "zero.shell requires a command string as first argument",
        )?;
        let opts = Opts::from_arg(args, 1);
        // Default cwd to the plan/work root (not silent process cwd).
        let cwd = opts
            .str("cwd")
            .map(|raw| {
                let path = PathBuf::from(raw);
                if path.is_absolute() {
                    path
                } else {
                    work_root.join(path)
                }
            })
            .unwrap_or_else(|| work_root.to_path_buf());
        let mode = opts.mode_or("mode", Mode::Auto);
        let timeout = opts
            .usize("timeout_seconds")
            .map(|secs| Duration::from_secs(secs as u64));
        if opts.bool("background").unwrap_or(false) {
            return engine
                .shell_background(command, Some(cwd.as_path()), timeout)
                .map(OpOutcome::from_catalog)
                .map_err(operation_error);
        }
        let resp = engine.shell(
            command,
            None,
            Some(cwd.as_path()),
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
                if let Some(cwd_val) = telem.get("cwd") {
                    value["cwd"] = cwd_val.clone();
                }
                if let Some(src) = telem.get("cwd_source") {
                    value["cwd_source"] = src.clone();
                }
            }
        }))
    }
}

pub(crate) fn exec_edit(engine: &TokenZeroEngine, work_root: &Path, args: &[Value]) -> OpResult {
    {
        let path = PathBuf::from(require_str_arg(
            args,
            0,
            "zero.edit requires a path string as first argument",
        )?);
        let edits_val = require_array_arg(
            args,
            1,
            "zero.edit requires an array of {find, replace} hunks as second argument",
        )?;
        let mut edits = Vec::with_capacity(edits_val.len());
        for (idx, value) in edits_val.iter().enumerate() {
            let hunk: EditHunk = serde_json::from_value(value.clone()).map_err(|err| {
                operation_error(format!("zero.edit: invalid hunk at index {idx}: {err}"))
            })?;
            edits.push(hunk);
        }
        if edits.is_empty() {
            return Err(operation_error("zero.edit: no edit hunks provided"));
        }
        let opts = Opts::from_arg(args, 2);
        let dry_run = opts.bool("dry_run").unwrap_or(false);
        let create = opts.bool("create").unwrap_or(false);
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
}

fn exec_expand(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let params = ExpandParams::from_codemode_args(args).map_err(operation_error)?;
    if !tokenzero_recovery::is_expandable_ref(&params.ref_id) {
        return Err(operation_error(format!(
            "expand takes a tz:// fz:// gz:// ref; to read a file use zero.fs.compound('read',{{path}}) -- got: {}",
            params.ref_id
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

fn exec_expand_many(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let items = require_array_arg(
        args,
        0,
        "zero.token.expandMany requires an array of tz://, fz://, or gz:// refs or item objects",
    )?;
    let mut results = Vec::with_capacity(items.len());
    let mut prevented = 0usize;
    for item in items {
        let params = ExpandParams::from_expand_many_item(item).map_err(operation_error)?;
        if !tokenzero_recovery::is_expandable_ref(&params.ref_id) {
            return Err(operation_error(format!(
                "expandMany takes a tz:// fz:// gz:// ref; to read a file use zero.fs.compound('read',{{path}}) -- got: {}",
                params.ref_id
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

fn exec_compact_inner(engine: &TokenZeroEngine, args: &[Value], aggressive: bool) -> OpResult {
    {
        let data = data_arg(
            args,
            "zero.token.compact/zero.compact requires data as first argument",
        )?;
        let content_type = detect_content_type(&data, None);
        let raw_tokens = count_tokens(&data);
        let mut store =
            tokenzero_recovery::RecoveryStore::new(Some(engine.config.cache_path.clone()));
        let stored = store
            .store_payload(&data, content_type, None, None, None)
            .ok();
        let recovery_ref = stored.as_ref().map(|s| s.blob_ref.as_str());
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
        let mut value = json!({ "text": capsule.text, "status": "ok", "raw_tokens": raw_tokens, "visible_tokens": capsule.visible_tokens, "compression_strategy": strategy, "savings_pct": format!("{:.0}%", tokenzero_core::savings_ratio(raw_tokens, capsule.visible_tokens) * 100.0) });
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
}

fn exec_pipe(engine: &TokenZeroEngine, work_root: &Path, args: &[Value]) -> OpResult {
    {
        let steps = array_or_json_arg(
            args,
            "zero.pipe requires an array of {method, args} steps",
            |err| format!("zero.pipe: steps is not valid JSON array: {err}"),
        )?;
        if steps.is_empty() {
            return Err(operation_error("zero.pipe requires at least one step"));
        }
        let mut results: Vec<Value> = Vec::with_capacity(steps.len());
        let mut pipe_scope: Scope = HashMap::new();
        let mut prevented = 0usize;
        for (idx, step) in steps.iter().enumerate() {
            let method = step.get("method").and_then(|v| v.as_str()).ok_or_else(|| {
                operation_error(format!("zero.pipe: step {idx} missing 'method' string"))
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
            prevented = prevented.saturating_add(outcome.prevented_read_bytes);
            let val = outcome.into_value();
            pipe_scope.insert("_prev".to_string(), val.clone());
            pipe_scope.insert(format!("_step{idx}"), val.clone());
            results.push(val);
        }
        let full = json!({ "steps": results.len(), "results": results, "last": results.last().cloned().unwrap_or(Value::Null) });
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
        let mut store =
            tokenzero_recovery::RecoveryStore::new(Some(engine.config.cache_path.clone()));
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
        Ok(OpOutcome::from_catalog(json!({ "ref": ref_id, "preview": first_line_preview(&text, 256usize.saturating_sub(count_tokens(&ref_id))) })).with_prevented_read_bytes(prevented))
    }
}

fn exec_pick(args: &[Value]) -> OpResult {
    let source = args
        .first()
        .ok_or_else(|| operation_error("zero.pick requires a source object as first argument"))?;
    let obj = source
        .as_object()
        .ok_or_else(|| operation_error("zero.pick: first argument must be an object"))?;
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
            return Err(operation_error(
                "zero.pick: second argument must be an array of keys or key strings",
            ));
        }
    };
    let picked: serde_json::Map<String, Value> = keys
        .into_iter()
        .filter_map(|key| obj.get(key).map(|v| (key.to_string(), v.clone())))
        .collect();
    catalog(Value::Object(picked))
}

fn exec_first(args: &[Value]) -> OpResult {
    {
        let value = args
            .first()
            .ok_or_else(|| operation_error("zero.first requires a value as first argument"))?;
        let n = args.get(1).and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
        if let Some(items) = value.as_array() {
            if n == 1 {
                return catalog(items.first().cloned().unwrap_or(Value::Null));
            }
            return catalog(Value::Array(items.iter().take(n).cloned().collect()));
        }
        let text = text_from_value(value).unwrap_or("");
        let lines = text.lines().take(n).collect::<Vec<_>>();
        let out = if n == 1 {
            lines.first().copied().unwrap_or("").to_string()
        } else {
            lines.join("\n")
        };
        catalog(json!(out))
    }
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

fn result_token_field(value: &Value, field: &str) -> usize {
    value.get(field).and_then(|v| v.as_u64()).unwrap_or(0) as usize
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

#[cfg(test)]
mod accumulator_bounds {
    use super::*;

    #[test]
    fn previous_output_lru_bounds_entries_and_bytes() {
        let mut outputs = PreviousOutputLru::default();
        let payload = "x".repeat(PREVIOUS_OUTPUT_MAX_PREFIX_BYTES + 1024);
        let first = PathBuf::from("session-0");
        assert_eq!(outputs.observe(first.clone(), &payload), 0);
        for index in 1..PREVIOUS_OUTPUT_MAX_SESSIONS {
            assert_eq!(
                outputs.observe(PathBuf::from(format!("session-{index}")), &payload),
                0
            );
        }
        assert_eq!(
            outputs.observe(first.clone(), &payload),
            PREVIOUS_OUTPUT_MAX_PREFIX_BYTES
        );
        outputs.observe(
            PathBuf::from(format!("session-{}", PREVIOUS_OUTPUT_MAX_SESSIONS)),
            &payload,
        );
        assert_eq!(outputs.entries.len(), PREVIOUS_OUTPUT_MAX_SESSIONS);
        assert!(
            outputs.stored_bytes <= PREVIOUS_OUTPUT_MAX_SESSIONS * PREVIOUS_OUTPUT_MAX_PREFIX_BYTES
        );
        assert!(outputs.entries.contains_key(&first));
        assert!(!outputs.entries.contains_key(&PathBuf::from("session-1")));
    }

    #[test]
    fn exact_expand_registry_is_execution_scoped() {
        let stale = Value::String("stale exact payload".to_string());
        record_exact_expand_payload(stale.as_str().unwrap());
        assert!(is_exact_expand_value(&stale));
        assert_eq!(
            execute_codemode("return 'fresh'").status,
            CodeModeStatus::Completed
        );
        assert!(!is_exact_expand_value(&stale));
    }
}
