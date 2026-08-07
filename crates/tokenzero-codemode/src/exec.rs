//! CodeMode plan executor and TokenZero operation dispatch.

use rquickjs::{Context, Runtime, function::Func};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;
use tokenzero_core::{
    Mode, ToolResponse, count_tokens, detect_content_type, pack_to_token_boundary_with_char_limit,
};
use tokenzero_filters::discover;

use tokenzero_engine::wall::{WallDeadline, with_host_wall_deadline};
use tokenzero_engine::workspace::{
    allowed_roots_for_workspace, resolve_recovery_cache_path, tokenzero_work_root,
};
use tokenzero_engine::{EditHunk, EngineConfig, TokenZeroEngine, shell_timeout_from_secs};

use super::catalog::{describe_method, search_catalog};
use super::journal::{
    BeginOutcome, JournalOperation, JournalState, JournalTransaction, OperationClass,
    OperationSpec, atomic_write as journal_atomic_write, begin_plan, classify_method,
    current_digest, doctor_json as journal_doctor_json, inspect as inspect_journal,
    open_unresolved, sha256_bytes,
};
use super::parser::{Expr, MethodCall, Statement, parse_plan, resolve_expr, resolve_return};
use super::recipe_registry;
use super::result::{CodeModeOptions, CodeModeResult, CodeModeStatus};
use super::sandbox::lower_code_plan;
use super::store::{
    CodeModeLimits, ExecutionStep, ExecutionStore, execution_id, finalize_result, now_ms,
};
use tokenzero_engine::expand_params::ExpandParams;

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
    /// Re-expanding the same bytes is charged again, so this is a counter, not a set.
    static RECOVERED_TOKENS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

fn exact_expand_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn register_exact_expand_payload(text: &str) {
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

pub(crate) fn record_exact_expand_payload(text: &str) {
    RECOVERED_TOKENS.with(|tokens| {
        tokens.set(tokens.get().saturating_add(count_tokens(text)));
    });
    register_exact_expand_payload(text);
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
        telemetry_enabled: options.telemetry_enabled,
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
    crate::install_shell_hooks();

    let containment_options = options.clone();
    super::containment::execute(plan, &containment_options, move || {
        execute_codemode_uncontained(plan, options)
    })
}

fn execute_codemode_uncontained(plan: &str, options: CodeModeOptions) -> CodeModeResult {
    EXACT_EXPAND_REGISTRY.with(|registry| registry.borrow_mut().clear());
    RECOVERED_TOKENS.with(|tokens| tokens.set(0));
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
    // Mutation denial is enforced at the canonical dispatch boundary
    // (begin_js_host_op) from resolved effect metadata, not by scanning plan
    // source (tokenzero-b452).
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

#[derive(Default)]
struct JsExecutionState {
    ops: usize,
    physical_ops: usize,
    parallel_groups: usize,
    in_flight: usize,
    wave_peak: usize,
    visible_tokens: usize,
    raw_tokens: usize,
    prevented_read_bytes: usize,
    refs: Vec<String>,
    steps: Vec<ExecutionStep>,
    started_ms: u128,
    limits: CodeModeLimits,
}

/// Bounded concurrent host-op gate for QuickJS Promise.all fan-out.
struct ParallelWidthGate {
    active: Mutex<usize>,
    cv: Condvar,
    max: usize,
}

impl ParallelWidthGate {
    fn new(max: usize) -> Self {
        Self {
            active: Mutex::new(0),
            cv: Condvar::new(),
            max: max.max(1),
        }
    }

    fn acquire(&self) {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        while *active >= self.max {
            active = self.cv.wait(active).unwrap_or_else(|e| e.into_inner());
        }
        *active += 1;
    }

    fn release(&self) {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        *active = active.saturating_sub(1);
        self.cv.notify_one();
    }
}

/// Completion signal shared by host-job workers and pollers: workers bump the
/// epoch and notify on every job resolution so pollers block on the condvar
/// instead of spinning the JS microtask loop.
struct CompletionGate {
    epoch: Mutex<u64>,
    cv: Condvar,
}

impl CompletionGate {
    fn new() -> Self {
        Self {
            epoch: Mutex::new(0),
            cv: Condvar::new(),
        }
    }

    fn epoch(&self) -> u64 {
        *self.epoch.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn bump_and_notify(&self) {
        let mut epoch = self.epoch.lock().unwrap_or_else(|e| e.into_inner());
        *epoch = epoch.saturating_add(1);
        self.cv.notify_all();
    }

    /// Block until the epoch moves past `since` or `max` elapses. Returns
    /// immediately when the epoch already moved.
    fn wait_for_change(&self, since: u64, max: Duration) {
        let epoch = self.epoch.lock().unwrap_or_else(|e| e.into_inner());
        if *epoch != since {
            return;
        }
        let _ = self
            .cv
            .wait_timeout(epoch, max)
            .unwrap_or_else(|e| e.into_inner());
    }
}

struct AsyncHostJob {
    /// `None` while running; `Some(json)` when the worker finishes.
    result: Mutex<Option<String>>,
    method: String,
    /// True when begin reserved ops / in_flight (real scheduled work).
    tracks_wave: bool,
    applied: Mutex<bool>,
}

struct AsyncHostRuntime {
    next_id: AtomicU64,
    jobs: Mutex<HashMap<u64, Arc<AsyncHostJob>>>,
    gate: Arc<ParallelWidthGate>,
    completion: Arc<CompletionGate>,
    /// Join handles for detached host workers (RA-13808): workers resolve via
    /// the jobs map, but the runtime must not drop while threads are live.
    workers: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

impl Drop for AsyncHostRuntime {
    fn drop(&mut self) {
        // Join every detached worker. Workers are wall-deadline bounded and
        // the finisher guarantees gate release, so these joins terminate.
        let handles = std::mem::take(&mut *self.workers.lock().unwrap_or_else(|e| e.into_inner()));
        for handle in handles {
            let _ = handle.join();
        }
    }
}

/// Guarantees every begun host job resolves: when the worker thread exits for
/// any reason (panic included), the job result is populated (error payload
/// when the worker died before producing one), pollers are woken, and the
/// width-gate slot is released so later fan-out cannot stall on a leaked slot.
struct HostJobFinisher {
    job: Arc<AsyncHostJob>,
    completion: Arc<CompletionGate>,
    width_gate: Arc<ParallelWidthGate>,
}

impl Drop for HostJobFinisher {
    fn drop(&mut self) {
        {
            let mut slot = self.job.result.lock().unwrap_or_else(|e| e.into_inner());
            if slot.is_none() {
                *slot = Some(tz_error_json(
                    "runtime: host op worker exited before producing a result",
                    "host op worker lost",
                ));
            }
        }
        self.completion.bump_and_notify();
        self.width_gate.release();
    }
}

fn wall_clock_limit_error(elapsed: u64, limits: &CodeModeLimits) -> Option<(String, &'static str)> {
    if elapsed > limits.hard_max_wall_ms {
        // Same message shape as `check_wall_deadline` (host-op checkpoints).
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

fn host_wall_deadline(started_ms: u128, hard_max_wall_ms: u64) -> WallDeadline {
    let elapsed = now_ms().saturating_sub(started_ms) as u64;
    WallDeadline::from_elapsed_ms(elapsed, hard_max_wall_ms)
}

fn tool_aborting_wall(resp: ToolResponse) -> OpResult {
    if resp
        .error
        .as_ref()
        .is_some_and(|error| error.code == "hard_max_wall_ms")
    {
        let message = resp
            .error
            .map(|error| error.message)
            .unwrap_or_else(|| "runtime: hard_max_wall_ms exceeded".to_string());
        return Err(operation_error(message));
    }
    tool(resp)
}
fn tz_error_json(message: &str, fallback: &str) -> String {
    serde_json::to_string(&json!({ "__tz_error": message }))
        .unwrap_or_else(|_| format!(r#"{{"__tz_error":"{fallback}"}}"#))
}

fn logical_width_for_method(method: &str, args: &[Value]) -> usize {
    if matches!(
        method,
        "zero.token.compactMany" | "zero.token.expandMany" | "zero.token.dedupe"
    ) {
        args.first()
            .and_then(Value::as_array)
            .map(|items| items.len().max(1))
            .unwrap_or(1)
    } else {
        1
    }
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
        let work_root_arc = Arc::new(work_root.clone());
        let engine = Arc::new(make_engine_for_root_with_options(
            work_root.clone(),
            &options,
        ));
        let state = Rc::new(RefCell::new(JsExecutionState {
            started_ms,
            limits: limits.clone(),
            ..Default::default()
        }));
        let async_rt = Rc::new(AsyncHostRuntime {
            next_id: AtomicU64::new(1),
            jobs: Mutex::new(HashMap::new()),
            gate: Arc::new(ParallelWidthGate::new(limits.max_parallel_width)),
            completion: Arc::new(CompletionGate::new()),
            workers: Mutex::new(Vec::new()),
        });
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
            install_js_async_binding(
                &ctx.globals(),
                Arc::clone(&engine),
                Arc::clone(&work_root_arc),
                Rc::clone(&state),
                Rc::clone(&async_rt),
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
            let host_op_pending = {
                let jobs = async_rt
                    .jobs
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                jobs.values().any(|job| {
                    job.result
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .is_none()
                })
            };
            if host_op_pending {
                // Polling a host-bound operation schedules one Promise continuation per poll.
                // Those jobs are executor bookkeeping, not plan-authored microtasks.
                drained = 0;
            } else if drained >= limits.max_microtasks {
                let snapshot = state.borrow();
                return fail_state(
                    &snapshot,
                    format!(
                        "sandbox: microtask cap exceeded: drained {drained} promise continuations against limit {} with no host op pending ({} host ops completed, {} parallel groups so far). Plan-authored promise churn drives this count — batch independent host calls with Promise.all (host width is capped at {}), split the work into smaller zero_execute plans, or pass limits.max_microtasks to raise the cap for this call.",
                        limits.max_microtasks,
                        snapshot.ops,
                        snapshot.parallel_groups,
                        limits.max_parallel_width,
                    ),
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
        result.telemetry.parallel_groups = Some(state.parallel_groups);
        telemetry_insert(&mut result, "parallel_groups", json!(state.parallel_groups));
        set_prevented_read_bytes(&mut result, state.prevented_read_bytes);
        finish(result, state.steps.clone())
    }
}

fn install_js_async_binding<'js>(
    globals: &rquickjs::Object<'js>,
    engine: Arc<TokenZeroEngine>,
    work_root: Arc<PathBuf>,
    state: Rc<RefCell<JsExecutionState>>,
    async_rt: Rc<AsyncHostRuntime>,
) -> rquickjs::Result<()> {
    let begin_state = Rc::clone(&state);
    let begin_rt = Rc::clone(&async_rt);
    let begin_engine = Arc::clone(&engine);
    let begin_root = Arc::clone(&work_root);
    globals.set(
        "__tz_begin",
        Func::from(move |method: String, args_json: String| {
            begin_js_host_op(
                &method,
                &args_json,
                &begin_engine,
                &begin_root,
                &begin_state,
                &begin_rt,
            )
        }),
    )?;
    let poll_state = Rc::clone(&state);
    let poll_rt = Rc::clone(&async_rt);
    globals.set(
        "__tz_poll",
        Func::from(move |id_text: String| poll_js_host_op(&id_text, &poll_state, &poll_rt)),
    )?;
    Ok(())
}

fn begin_js_host_op(
    method: &str,
    args_json: &str,
    engine: &Arc<TokenZeroEngine>,
    work_root: &Arc<PathBuf>,
    state: &Rc<RefCell<JsExecutionState>>,
    async_rt: &Rc<AsyncHostRuntime>,
) -> String {
    let mut args = serde_json::from_str::<Vec<Value>>(args_json).unwrap_or_default();
    if matches!(method, "codemode.recipeRun" | "recipeRun" | "recipe_run") {
        let (max_code_bytes, max_visible_tokens) = {
            let state = state.borrow();
            (state.limits.max_code_bytes, state.limits.max_visible_tokens)
        };
        args.push(json!(max_code_bytes));
        args.push(json!(max_visible_tokens));
    }
    // Canonical dispatch authorization (tokenzero-b452): mutation denial is
    // decided from the resolved operation's effect metadata at this dispatch
    // boundary, not from scanning plan source. The quickjs bridge has no
    // journal/transaction support, so the workspace-mutating edit family is
    // refused regardless of alias, computed, or obfuscated spellings.
    if quickjs_edit_dispatch_denied(method) {
        let message = tokenzero_engine::annotate_write_failure(
            concat!(
                "sandbox: mutating binding denied without transaction support ",
                "(use the lowered zero.edit / tz_edit path, not free-form JS mutation)",
            ),
            false,
        );
        let id = async_rt.next_id.fetch_add(1, Ordering::Relaxed);
        let job = Arc::new(AsyncHostJob {
            result: Mutex::new(Some(
                serde_json::to_string(&json!({
                    "__tz_error": message,
                    "__tz_error_kind": "sandbox",
                }))
                .unwrap_or_else(|_| {
                    r#"{"__tz_error":"mutating binding denied","__tz_error_kind":"sandbox"}"#
                        .to_string()
                }),
            )),
            method: method.to_string(),
            tracks_wave: false,
            applied: Mutex::new(false),
        });
        async_rt
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, job);
        return id.to_string();
    }
    let logical_width = logical_width_for_method(method, &args);
    let limit_error = {
        let state_ref = state.borrow();
        let elapsed = now_ms().saturating_sub(state_ref.started_ms) as u64;
        wall_clock_limit_error(elapsed, &state_ref.limits).or_else(|| {
            if state_ref.ops.saturating_add(logical_width) > state_ref.limits.max_logical_ops {
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
        })
    };
    let id = async_rt.next_id.fetch_add(1, Ordering::Relaxed);
    if let Some((message, fallback)) = limit_error {
        let job = Arc::new(AsyncHostJob {
            result: Mutex::new(Some(tz_error_json(&message, fallback))),
            method: method.to_string(),
            tracks_wave: false,
            applied: Mutex::new(false),
        });
        async_rt
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, job);
        return id.to_string();
    }
    {
        let mut state = state.borrow_mut();
        state.ops = state.ops.saturating_add(logical_width);
        state.physical_ops = state.physical_ops.saturating_add(1);
        state.in_flight = state.in_flight.saturating_add(1);
        state.wave_peak = state.wave_peak.max(state.in_flight);
    }
    let job = Arc::new(AsyncHostJob {
        result: Mutex::new(None),
        method: method.to_string(),
        tracks_wave: true,
        applied: Mutex::new(false),
    });
    async_rt
        .jobs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, Arc::clone(&job));
    let gate = Arc::clone(&async_rt.gate);
    let completion = Arc::clone(&async_rt.completion);
    let engine = Arc::clone(engine);
    let work_root = Arc::clone(work_root);
    let method_owned = method.to_string();
    let wall = {
        let state_ref = state.borrow();
        host_wall_deadline(state_ref.started_ms, state_ref.limits.hard_max_wall_ms)
    };
    let worker_handle = std::thread::spawn(move || {
        gate.acquire();
        let finisher = HostJobFinisher {
            job,
            completion,
            width_gate: gate,
        };
        let result_json = with_host_wall_deadline(wall, || {
            match dispatch_values(&engine, work_root.as_path(), &method_owned, &args) {
                Ok(outcome) => {
                    let prevented_read_bytes = outcome.prevented_read_bytes;
                    let value = outcome.into_value();
                    serde_json::to_string(&json!({
                        "__tz_ok": true,
                        "value": value,
                        "prevented_read_bytes": prevented_read_bytes,
                    }))
                    .unwrap_or_else(|_| {
                        "{\"__tz_ok\":true,\"value\":null,\"prevented_read_bytes\":0}".to_string()
                    })
                }
                Err(error) => serde_json::to_string(&json!({
                    "__tz_error": error.error.as_ref().map(|error| error.message.as_str()).unwrap_or("unknown error"),
                    "__tz_error_kind": error.error.as_ref().map(|error| error.kind.as_str()).unwrap_or("runtime"),
                }))
                .unwrap_or_else(|_| {
                    "{\"__tz_error\":\"unknown error\",\"__tz_error_kind\":\"runtime\"}".to_string()
                }),
            }
        });
        *finisher
            .job
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(result_json);
        // Finisher drop wakes pollers and releases the width-gate slot; on a
        // dispatch panic it also fills the result with an error payload.
        drop(finisher);
    });
    async_rt
        .workers
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(worker_handle);
    id.to_string()
}

fn poll_js_host_op(
    id_text: &str,
    state: &Rc<RefCell<JsExecutionState>>,
    async_rt: &Rc<AsyncHostRuntime>,
) -> String {
    let Ok(id) = id_text.parse::<u64>() else {
        return json!({"done": true, "result": tz_error_json("invalid async job id", "bad job id")})
            .to_string();
    };
    let job = {
        let jobs = async_rt.jobs.lock().unwrap_or_else(|e| e.into_inner());
        jobs.get(&id).cloned()
    };
    let Some(job) = job else {
        return json!({"done": true, "result": tz_error_json("unknown async job", "unknown job")})
            .to_string();
    };
    let finished = job.result.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(raw) = finished else {
        // Block on the completion gate until any worker resolves or the hard
        // wall budget expires; never busy-spin the JS microtask loop.
        let (wait_ms, hard_exceeded, hard_limit) = {
            let state_ref = state.borrow();
            let elapsed = now_ms().saturating_sub(state_ref.started_ms) as u64;
            let hard = state_ref.limits.hard_max_wall_ms;
            (
                hard.saturating_sub(elapsed).clamp(1, 5_000),
                elapsed > hard,
                hard,
            )
        };
        if hard_exceeded {
            // Resolve the promise with the same hard-wall error the host-op
            // checkpoints produce instead of polling forever.
            return json!({
                "done": true,
                "result": tz_error_json(
                    &format!("runtime: hard_max_wall_ms exceeded {hard_limit}"),
                    "hard wall clock exceeded",
                ),
            })
            .to_string();
        }
        let epoch = async_rt.completion.epoch();
        async_rt
            .completion
            .wait_for_change(epoch, Duration::from_millis(wait_ms));
        return json!({"done": false}).to_string();
    };
    {
        let mut applied = job.applied.lock().unwrap_or_else(|e| e.into_inner());
        if !*applied {
            *applied = true;
            apply_js_host_result(state, &job.method, &raw);
            if job.tracks_wave {
                let mut state = state.borrow_mut();
                if state.in_flight > 0 {
                    state.in_flight -= 1;
                }
                if state.in_flight == 0 {
                    if state.wave_peak > 1 {
                        state.parallel_groups = state.parallel_groups.saturating_add(1);
                    }
                    state.wave_peak = 0;
                }
            }
        }
    }
    let result_for_js = match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(mut map)) if map.get("__tz_ok").and_then(Value::as_bool) == Some(true) => {
            map.remove("value")
                .map(|v| serde_json::to_string(&v).unwrap_or_else(|_| "null".to_string()))
                .unwrap_or_else(|| "null".to_string())
        }
        _ => raw,
    };
    json!({"done": true, "result": result_for_js}).to_string()
}

fn apply_js_host_result(state: &Rc<RefCell<JsExecutionState>>, method: &str, raw: &str) {
    let parsed = serde_json::from_str::<Value>(raw).unwrap_or(Value::Null);
    if parsed.get("__tz_error").is_some() {
        let mut state = state.borrow_mut();
        let op = state.ops;
        state.steps.push(ExecutionStep {
            id: format!("js_step{op}"),
            method: method.to_string(),
            status: "error".to_string(),
            refs: Vec::new(),
        });
        return;
    }
    let (value, prevented_read_bytes) =
        if parsed.get("__tz_ok").and_then(Value::as_bool) == Some(true) {
            (
                parsed.get("value").cloned().unwrap_or(Value::Null),
                parsed
                    .get("prevented_read_bytes")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize,
            )
        } else {
            (parsed, 0)
        };
    let refs = refs_from_value(&value);
    let mut state = state.borrow_mut();
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
}

fn js_prelude() -> &'static str {
    r#"
        const __tz_parse = (text) => {
          const value = JSON.parse(text); if (value && value.__tz_exact_expand) { try { return JSON.parse(value.text); } catch (_) { return value.text; } }
          if (value && value.__tz_error) { const error = new Error(value.__tz_error); error.__tz_error_kind = value.__tz_error_kind || 'runtime'; throw error; }
          return value;
        };
        const __tz_call = (method, args) => {
          const id = __tz_begin(method, JSON.stringify(args));
          return (async () => {
            for (;;) {
              const status = JSON.parse(__tz_poll(String(id)));
              if (status.done) return __tz_parse(status.result);
              await Promise.resolve();
            }
          })();
        };
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
          const recipe = await __tz_call('codemode.recipeRun', [String(name)]);
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
          describeRecipe: (name) => __tz_call('codemode.recipeDescribe', [String(name)]),
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
        max_visible_tokens: options.max_visible_tokens,
        max_wall_ms,
        hard_max_wall_ms,
        max_parallel_width: options.max_parallel_width.max(1),
        ..Default::default()
    }
}

fn is_journaled_edit(method: &str) -> bool {
    matches!(method, "zero.edit" | "edit" | "zero.token.edit" | "tz_edit")
}

/// Canonical mutation authorization for the quickjs dispatch boundary
/// (tokenzero-b452): deny the workspace-mutating edit family by resolved
/// effect metadata (cluster == "edit"), covering canonical, alias, and
/// legacy spellings alike. Unknown names are not denied here; the dispatcher
/// fails closed on them with an unknown-method error.
fn quickjs_edit_dispatch_denied(method: &str) -> bool {
    if is_journaled_edit(method) {
        return true;
    }
    matches!(
        tokenzero_core::operation_abi::resolve_operation(method),
        Some(op)
            if op.cluster == "edit"
                && op.mutability == tokenzero_core::operation_abi::Mutability::WorkspaceMutating
    )
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
        let elapsed = now_ms().saturating_sub(started_ms) as u64;
        if let Some((message, _)) = wall_clock_limit_error(elapsed, limits) {
            return finish(CodeModeResult::error(message, progress.ops), progress.steps);
        }
        let wall = host_wall_deadline(started_ms, limits.hard_max_wall_ms);
        let outcome = match with_host_wall_deadline(wall, || {
            dispatch_journaled(
                &boot.engine,
                &boot.work_root,
                call,
                &progress.scope,
                &mut boot.transaction,
                journal_index,
                JournalDispatchMode::Lowered,
            )
        }) {
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
    let budget_tokens = options.ref_first_budget;
    // vz89.10: one session turn per codemode execution; the ledger is shared
    // per session scope across the per-call engines CodeMode builds.
    let exposure_ledger = engine.session_exposure();
    let mut exposure = exposure_ledger.lock().unwrap_or_else(|e| e.into_inner());
    exposure.next_turn();
    let value = ref_first_value(value, budget_tokens, &mut store, &mut refs, &mut exposure);
    (value, refs)
}

fn ref_first_value(
    value: Value,
    budget_tokens: usize,
    store: &mut tokenzero_recovery::RecoveryStore,
    refs: &mut Vec<String>,
    exposure: &mut tokenzero_engine::exposure::SessionExposureLedger,
) -> Value {
    {
        if is_exact_expand_value(&value) {
            return value;
        }
        match value {
            Value::String(text) => {
                let text_tokens = count_tokens(&text);
                if text_tokens > budget_tokens {
                    let content_type = detect_content_type(&text, None);
                    if let Ok(stored) = store.store_payload(&text, content_type, None, None, None) {
                        let ref_id = stored.blob_ref.as_str().to_string();
                        if !refs.contains(&ref_id) {
                            refs.push(ref_id.clone());
                        }
                        let preview_budget = budget_tokens.saturating_sub(count_tokens(&ref_id));
                        // The preview alone is a dead end: it is capped at 32
                        // chars, so an agent that only sees {ref, preview} has
                        // no way to know the full text is one expand away and
                        // re-runs the command with a narrower filter instead
                        // (tokenzero-codemode-shell-output-unreachable-ref-qr4o).
                        // State the recovery inline, next to the ref it applies
                        // to, rather than relying on out-of-band documentation.
                        return json!({
                            "ref": ref_id,
                            "preview": compact_value_preview(&text, preview_budget),
                            "chars": text.chars().count(),
                            "expand": format!("await zero.token.expand('{ref_id}')"),
                        });
                    }
                } else if text_tokens > REF_FIRST_INLINE_REF_FLOOR_TOKENS {
                    // pn93: the value inlines fully below, but a nontrivial
                    // payload still mints its exact-recovery ref so the agent
                    // can re-expand later without re-running the plan.
                    let content_type = detect_content_type(&text, None);
                    if let Ok(stored) = store.store_payload(&text, content_type, None, None, None) {
                        let ref_id = stored.blob_ref.as_str().to_string();
                        // vz89.10: the session already holds these bytes, so a
                        // second reference sends the short ref instead of
                        // re-inlining. Expand always recovers the full bytes.
                        if exposure.exposure(&ref_id, None).is_some() {
                            if !refs.contains(&ref_id) {
                                refs.push(ref_id.clone());
                            }
                            return json!({
                                "ref": ref_id,
                                "chars": text.chars().count(),
                                "expand": format!("await zero.token.expand('{ref_id}')"),
                                "session_exposure": "held",
                            });
                        }
                        exposure.record(&ref_id, None, text.len() as u64);
                        if !refs.contains(&ref_id) {
                            refs.push(ref_id);
                        }
                    }
                }
                Value::String(text)
            }
            Value::Array(items) => Value::Array(
                items
                    .into_iter()
                    .map(|item| ref_first_value(item, budget_tokens, store, refs, exposure))
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
                            (
                                key,
                                ref_first_value(value, budget_tokens, store, refs, exposure),
                            )
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

/// Strings at or below this size inline without minting a recovery ref: the
/// store write would cost more than the recovery option is worth.
const REF_FIRST_INLINE_REF_FLOOR_TOKENS: usize = 64;

fn compact_value_preview(text: &str, max_tokens: usize) -> String {
    let value = tokenzero_engine::render::preview(text);
    pack_to_token_boundary_with_char_limit(&value, max_tokens, 32).to_string()
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
    Box::new(CodeModeResult::error(message, 0))
}
fn capsule_operation_error(error: String) -> Box<CodeModeResult> {
    boxed_error(
        "capsule_omission_invalid",
        format!("capsule omission validation failed: {error}"),
    )
}
fn map_journal_err(kind: &'static str) -> impl FnOnce(String) -> Box<CodeModeResult> {
    move |message| boxed_error(kind, message)
}
fn tx_error(message: impl Into<String>, ops: usize) -> Box<CodeModeResult> {
    Box::new(CodeModeResult::error_with_kind(
        "transaction",
        message,
        ops,
        false,
    ))
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
    let recovered_tokens = RECOVERED_TOKENS.with(std::cell::Cell::get);
    result.telemetry.recovery_tokens = recovered_tokens;
    let raw_tokens = result.telemetry.raw_tokens();
    let charged_tokens = result
        .telemetry
        .visible_tokens()
        .saturating_add(recovered_tokens);
    result.telemetry.recovery_adjusted_savings_pct = if raw_tokens == 0 {
        0.0
    } else {
        (raw_tokens as f64 - charged_tokens as f64) * 100.0 / raw_tokens as f64
    };
    telemetry_insert(&mut result, "recovery_tokens", json!(recovered_tokens));
    let adjusted_pct = result.telemetry.recovery_adjusted_savings_pct;
    telemetry_insert(
        &mut result,
        "recovery_adjusted_savings_pct",
        json!(adjusted_pct),
    );
    {
        let work_root = tokenzero_work_root(options.root.clone());
        let root_was_explicit = options.root.is_some();
        let engine = make_engine_for_root_with_options(work_root.clone(), options);
        let journal_health = journal_doctor_json(&engine.config.cache_path);
        let journals_have_signal = journal_health
            .get("unresolved")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
            || journal_health
                .get("corrupt")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty());
        if journals_have_signal {
            telemetry_insert(&mut result, "plan_journals", journal_health);
        }
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
            .map(str::to_owned)
        {
            let ack = format!(
                "{}\n# warning: root_fallback: {warning}",
                finalized.visible_ack.trim_end()
            );
            finalized.set_visible_ack(ack);
        }
        let enabled = tokenzero_engine::usage_telemetry_enabled(options.telemetry_enabled);
        finalized.telemetry.billed_input_tokens = count_tokens(plan);
        finalized.telemetry.cached_input_tokens = 0;
        finalized.telemetry.billed_output_tokens = finalized.telemetry.visible_tokens();
        finalized.telemetry.cached_output_tokens = finalized
            .telemetry
            .prefix_cache_hits
            .min(finalized.telemetry.billed_output_tokens);
        tokenzero_engine::record_codemode_accounting(
            &engine.config.cache_path,
            enabled,
            finalized.telemetry.raw_tokens(),
            finalized.telemetry.visible_tokens(),
        );
        if let Some(root) = engine.config.allowed_roots.first() {
            let mut event = tokenzero_pulse::PulseEvent::tool_call(
                "codemode",
                "codemode",
                finalized.telemetry.raw_tokens(),
                finalized.telemetry.visible_tokens(),
                finalized.telemetry.recovery_tokens(),
                finalized.refs.len(),
                u128::from(finalized.telemetry.wall_ms),
                None,
            )
            .with_attribution(
                Some(engine.session_id().to_string()),
                finalized.execution_id.clone(),
                finalized.refs.clone(),
            );
            event.failure = !matches!(finalized.status, CodeModeStatus::Completed);
            let _ =
                tokenzero_pulse::record_event(&tokenzero_pulse::default_ledger_path(root), &event);
        }
        // vz89.11: opt-in machine-action channel on the codemode envelope.
        // Gate off leaves the envelope byte-identical.
        let channel_mode = tokenzero_core::channel_mode();
        if channel_mode.enabled() {
            let status_line = match finalized.status {
                CodeModeStatus::Completed => format!(
                    "Executed {kind} plan ({} physical op(s))",
                    finalized.telemetry.physical_ops
                ),
                CodeModeStatus::Error => {
                    let error_kind = finalized
                        .error
                        .as_ref()
                        .map(|error| error.kind.as_str())
                        .unwrap_or("error");
                    format!("Plan failed ({error_kind})")
                }
            };
            let user_message = channel_mode
                .emits_user_message()
                .then(|| terminal_user_message(&finalized));
            finalized.channels = Some(tokenzero_core::ChannelSeparation {
                action: format!("codemode.{kind}"),
                status_line,
                user_message,
            });
        }
        finalized
    }
}

/// vz89.11: the one brief final explanation, generated from the plan receipt
/// rather than from model prose. Failures name the error; successes report the
/// work done and the refs left behind for follow-up.
fn terminal_user_message(result: &CodeModeResult) -> String {
    if let Some(error) = &result.error {
        return format!("Plan failed ({}): {}", error.kind, error.message);
    }
    let ops = result.telemetry.physical_ops;
    let mut message = format!(
        "Done: {ops} operation{} in {}ms",
        if ops == 1 { "" } else { "s" },
        result.telemetry.wall_ms
    );
    let refs = result.refs.len();
    if refs > 0 {
        message.push_str(&format!(
            ", {refs} ref{} available to expand",
            if refs == 1 { "" } else { "s" }
        ));
    }
    message.push('.');
    message
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
        let elapsed = now_ms().saturating_sub(started_ms) as u64;
        if let Some((message, _)) = wall_clock_limit_error(elapsed, limits) {
            return finish(CodeModeResult::error(message, progress.ops), progress.steps);
        }
        let wall = host_wall_deadline(started_ms, limits.hard_max_wall_ms);
        let outcome = match with_host_wall_deadline(wall, || {
            dispatch_journaled(
                &boot.engine,
                &boot.work_root,
                &call,
                &progress.scope,
                &mut boot.transaction,
                index,
                JournalDispatchMode::Json,
            )
        }) {
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
        let mut names = recipe_registry::list()
            .into_iter()
            .map(|recipe| recipe.name)
            .collect::<Vec<_>>();
        names.extend(with_previous_outputs(|registry| {
            registry.recipe_names(&engine.config.cache_path)
        }));
        names.sort();
        names.dedup();
        return catalog(json!(names));
    }
    if matches!(
        method,
        "codemode.recipeDescribe" | "recipeDescribe" | "recipe_describe"
    ) {
        let name = require_str_arg(args, 0, "recipe describe requires a name string")?;
        if let Some(recipe) = recipe_registry::get(name) {
            let envelope_tokens = recipe.envelope_tokens();
            return catalog(json!({
                "registry_version": recipe_registry::RECIPE_REGISTRY_VERSION,
                "recipe": recipe,
                "envelope_tokens": envelope_tokens,
            }));
        }
        let custom = with_previous_outputs(|registry| {
            registry.recipe_source(&engine.config.cache_path, name)
        });
        return custom
            .map(|source| {
                catalog(json!({"name": name, "version": "session", "source_bytes": source.len()}))
            })
            .unwrap_or_else(|| {
                Err(boxed_error(
                    "recipe_not_found",
                    format!("recipe not found: {name}"),
                ))
            });
    }
    if matches!(method, "codemode.recipeRun" | "recipeRun" | "recipe_run") {
        let name = require_str_arg(args, 0, "recipe run requires a name string")?;
        let max_code_bytes = args
            .get(1)
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or_else(|| CodeModeLimits::default().max_code_bytes);
        let builtin = recipe_registry::get(name);
        let source = with_previous_outputs(|registry| {
            registry.recipe_source(&engine.config.cache_path, name)
        })
        .or_else(|| builtin.as_ref().map(|recipe| recipe.source.clone()))
        .ok_or_else(|| boxed_error("recipe_not_found", format!("recipe not found: {name}")))?;
        if let Some(recipe) = builtin.as_ref() {
            let budget = args
                .get(2)
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_else(|| CodeModeLimits::default().max_visible_tokens);
            let envelope = recipe.envelope_tokens();
            if envelope > budget {
                return Err(boxed_error(
                    "recipe_budget_exceeded",
                    format!(
                        "recipe {name}@{} envelope {envelope} exceeds declared budget {budget}",
                        recipe.version
                    ),
                ));
            }
        }
        if source.trim().is_empty() {
            return Err(boxed_error("validation", "recipe source must not be empty"));
        }
        if source.len() > max_code_bytes {
            return Err(boxed_error(
                "validation",
                format!("recipe exceeds max_code_bytes {max_code_bytes}"),
            ));
        }
        return catalog(match builtin {
            Some(recipe) => {
                let envelope_tokens = recipe.envelope_tokens();
                json!({"source": source, "version": recipe.version, "envelope_tokens": envelope_tokens})
            }
            None => json!({"source": source, "version": "session"}),
        });
    }
    match method {
        "zero.read" | "read" | "zero.token.read" => exec_read(engine, work_root, args),
        "zero.find" | "find" | "zero.token.find" => exec_find(engine, work_root, args, false),
        "zero.grep" | "grep" | "zero.token.grep" => exec_find(engine, work_root, args, true),
        "zero.glob" | "glob" | "zero.token.glob" => exec_glob(engine, work_root, args),
        "zero.tree" | "tree" | "zero.token.tree" => exec_tree(engine, work_root, args),
        "zero.shell" | "shell" | "zero.token.shell" => exec_shell(engine, work_root, args),
        "zero.token.job" | "zero.job" | "job" => exec_job(engine, args),
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
        "zero.rewrite" | "rewrite" | "zero.token.rewrite" => exec_rewrite(engine, args),
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

/// Route a domain op through the shared typed dispatcher (tokenzero-irx9.2).
fn domain_via_dispatcher(engine: &TokenZeroEngine, method: &str, args: &Value) -> OpResult {
    let outcome = tokenzero_engine::dispatch_codemode_method(engine, method, args)
        .map_err(|err| operation_error(format!("{}: {}", err.kind.as_str(), err.message)))?;
    if let Some(resp) = outcome.tool_response {
        return tool(resp);
    }
    if let Some(err) = outcome.domain_error {
        return Err(operation_error(format!(
            "{}: {}",
            err.kind.as_str(),
            err.message
        )));
    }
    Err(operation_error(format!(
        "domain dispatch for {method} returned empty outcome"
    )))
}

macro_rules! op_catalog {
    ($name:ident ($($arg:ident : $ty:ty),*) => $body:expr) => {
        fn $name($($arg : $ty),*) -> OpResult { catalog($body) }
    };
}

fn exec_mem(engine: &TokenZeroEngine) -> OpResult {
    domain_via_dispatcher(engine, "zero.mem", &json!({}))
}
op_catalog!(exec_discover() => serde_json::to_value(discover()).unwrap_or(Value::Null));
op_catalog!(exec_raw(args: &[Value]) => json!({ "__tz_raw": true, "value": args.first().cloned().unwrap_or(Value::Null) }));
op_catalog!(exec_count_tokens(args: &[Value]) => json!(count_tokens(text_from_value(args.first().unwrap_or(&Value::Null)).unwrap_or(""))));

fn exec_ingest(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let text = require_str_arg(args, 0, "zero.ingest requires text as first argument")?;
    let mode = Opts::from_arg(args, 1).mode_or("mode", Mode::Auto);
    // Domain dispatcher uses tz_ingest schema (text/input); source is kernel-side label for MCP.
    domain_via_dispatcher(
        engine,
        "zero.ingest",
        &json!({"text": text, "mode": mode.to_string(), "input": text}),
    )
}

fn exec_rewrite(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let command = require_str_arg(
        args,
        0,
        "zero.rewrite requires a command string as first argument",
    )?;
    let opts = Opts::from_arg(args, 1);
    let mode = opts.str("mode").unwrap_or("safe");
    domain_via_dispatcher(
        engine,
        "zero.rewrite",
        &json!({"command": command, "mode": mode}),
    )
}

fn exec_cache_pack(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let opts = Opts::from_arg(args, 0);
    let scope = opts.str("scope").unwrap_or("agent");
    domain_via_dispatcher(engine, "zero.cache_pack", &json!({"scope": scope}))
}

fn exec_recall(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let query = require_str_arg(
        args,
        0,
        "zero.recall requires a query string as first argument",
    )?;
    let opts = Opts::from_arg(args, 1);
    domain_via_dispatcher(
        engine,
        "zero.recall",
        &json!({
            "query": query,
            "max_hits": opts.usize("max_hits").unwrap_or(50),
            "mode": opts.mode_or("mode", Mode::Auto).to_string(),
            "max_visible_tokens": opts.usize("max_visible_tokens").unwrap_or(engine.config.max_visible_tokens),
        }),
    )
}

fn exec_fetch(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let url = require_str_arg(
        args,
        0,
        "zero.fetch requires an http(s) URL as first argument",
    )?;
    let opts = Opts::from_arg(args, 1);
    let mut payload = json!({
        "url": url,
        "fresh": opts.bool("fresh").unwrap_or(false),
        "mode": opts.mode_or("mode", Mode::Auto).to_string(),
        "max_visible_tokens": opts.usize("max_visible_tokens").unwrap_or(engine.config.max_visible_tokens),
    });
    if let Some(ttl) = opts.usize("ttl_seconds") {
        payload["ttl_seconds"] = json!(ttl);
    }
    domain_via_dispatcher(engine, "zero.fetch", &payload)
}

fn exec_batch(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let ops = array_or_json_arg(
        args,
        "zero.batch requires an array of {tool, args} objects as first argument",
        |err| format!("zero.batch ops is not valid JSON: {err}"),
    )?;
    if ops.is_empty() {
        return Err(operation_error("zero.batch requires at least one op"));
    }
    match tokenzero_engine::batch_response(engine, &json!({"ops": ops, "mode": "auto"})) {
        Ok(resp) => {
            let operations = resp
                .telemetry
                .as_ref()
                .and_then(|telemetry| telemetry.get("ops"))
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize;
            let failed_operations = resp
                .telemetry
                .as_ref()
                .and_then(|telemetry| telemetry.get("failed_ops"))
                .and_then(Value::as_u64)
                .unwrap_or_else(|| {
                    resp.telemetry
                        .as_ref()
                        .and_then(|telemetry| telemetry.get("per_op"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter(|row| row.get("status") == Some(&json!("error")))
                                .count() as u64
                        })
                        .unwrap_or(0)
                }) as usize;
            if resp.status == "ok" && failed_operations == 0 {
                return tool(resp);
            }
            let message = resp
                .error
                .as_ref()
                .map(|error| error.message.clone())
                .unwrap_or_else(|| {
                    format!("{failed_operations} of {operations} batch operations failed")
                });
            let response_value = json!(&resp);
            let response_refs = resp
                .refs
                .iter()
                .map(|record| record.ref_id.clone())
                .collect::<Vec<_>>();
            let accounting = resp.accounting.clone();
            let mut error = CodeModeResult::error_with_kind("batch", message, operations, false);
            error.value = Some(response_value);
            error.refs = response_refs;
            error.telemetry.refs_count = Some(error.refs.len());
            error.telemetry.internal_actions = operations.saturating_add(error.refs.len());
            error.telemetry.store_writes = error.refs.len();
            if let Some(accounting) = accounting {
                error.telemetry.visible_tokens = accounting.visible_tokens;
                error.telemetry.raw_tokens = accounting.raw_tokens;
                error.telemetry.recovery_tokens = accounting.recovery_tokens;
                error.telemetry.billed_output_tokens = accounting.billed_tokens;
                error.telemetry.cached_output_tokens = accounting.cached_tokens;
                error.telemetry.bytes_materialized = accounting.raw_tokens;
                error.telemetry.payload_tokens = accounting.visible_tokens;
            }
            if let Some(telemetry) = resp.telemetry {
                telemetry_insert(&mut error, "batch", telemetry);
            }
            Err(Box::new(error))
        }
        Err(error) => Err(operation_error(&error)),
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
    catalog(json!({"items": results, "count": results.len(), "refs": refs}))
}

const DEFAULT_JOB_WAIT_MS: u64 = 30_000;
const MAX_JOB_WAIT_MS: u64 = 30_000;
const DEFAULT_JOB_TAIL_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JobPollOptions {
    wait_ms: u64,
    since: usize,
    tail_bytes: usize,
}

fn job_poll_options(opts: &Opts<'_>) -> JobPollOptions {
    let wait_ms = ["waitMs", "wait_ms"]
        .iter()
        .find_map(|key| opts.u64(key))
        .unwrap_or(DEFAULT_JOB_WAIT_MS)
        .min(MAX_JOB_WAIT_MS);
    let since = ["since", "cursor"]
        .iter()
        .find_map(|key| opts.usize(key))
        .unwrap_or(0);
    let tail_bytes = ["tailBytes", "tail_bytes"]
        .iter()
        .find_map(|key| opts.usize(key))
        .unwrap_or(DEFAULT_JOB_TAIL_BYTES);
    JobPollOptions {
        wait_ms,
        since,
        tail_bytes,
    }
}

fn exec_job(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    let id = require_str_arg(args, 0, "zero.token.job requires a job id string")?;
    let opts = job_poll_options(&Opts::from_arg(args, 1));
    engine
        .shell_job_wait(
            id,
            Duration::from_millis(opts.wait_ms),
            opts.since,
            opts.tail_bytes,
        )
        .map(OpOutcome::from_catalog)
        .map_err(operation_error)
}

#[cfg(test)]
mod method_inventory_tests {
    use super::*;

    fn error_message(result: &CodeModeResult) -> &str {
        result
            .error
            .as_ref()
            .map(|error| error.message.as_str())
            .unwrap_or(result.visible_ack.as_str())
    }

    #[test]
    fn every_catalog_path_reaches_a_real_dispatch_arm() {
        let root = tempfile::tempdir().expect("temp root");
        let engine = TokenZeroEngine::new(EngineConfig::for_root(root.path()));

        for path in tokenzero_engine::codemode_catalog::method_paths() {
            if let Err(result) = dispatch_values(&engine, root.path(), path, &[]) {
                let message = error_message(&result);
                assert!(
                    !message.starts_with("unknown method:"),
                    "catalog path {path} has no executor arm: {message}"
                );
            }
        }

        let unknown = dispatch_values(&engine, root.path(), "zero.not_registered", &[])
            .expect_err("kill control must reach the unknown-method arm");
        assert!(error_message(&unknown).starts_with("unknown method: zero.not_registered"));
    }
}

#[cfg(test)]
mod store_and_batch_truth_tests {
    use super::*;

    fn engine_with_blocked_cache() -> (tempfile::TempDir, TokenZeroEngine) {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-directory");
        std::fs::write(&blocker, "block").unwrap();
        let mut config = EngineConfig::for_root(dir.path());
        config.cache_path = blocker.join("cache.json");
        let engine = TokenZeroEngine::new(config);
        (dir, engine)
    }

    #[test]
    fn compact_and_pipe_fail_typed_when_recovery_storage_is_unavailable() {
        let (dir, engine) = engine_with_blocked_cache();
        let compact = exec_compact_inner(&engine, &[json!("payload")], false).unwrap_err();
        assert_eq!(compact.error.as_ref().unwrap().kind, "store");
        assert!(
            compact
                .error
                .as_ref()
                .unwrap()
                .message
                .contains("zero.token.compact")
        );

        let pipe = exec_pipe(
            &engine,
            dir.path(),
            &[json!([{"method": "zero.count", "args": [[1, 2, 3]]}])],
        )
        .unwrap_err();
        assert_eq!(pipe.error.as_ref().unwrap().kind, "store");
        assert!(pipe.error.as_ref().unwrap().message.contains("zero.pipe"));
    }

    #[test]
    fn codemode_batch_failure_is_an_error_with_structured_per_op_truth() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
        let error = exec_batch(
            &engine,
            &[json!([
                {"tool": "ingest", "args": {"text": "batch-retained"}},
                {"tool": "batch", "args": {"ops": []}}
            ])],
        )
        .unwrap_err();

        assert_eq!(error.error.as_ref().unwrap().kind, "batch");
        let batch = error
            .telemetry
            .extra
            .as_ref()
            .and_then(|extra| extra.get("batch"))
            .expect("batch telemetry must survive CodeMode error conversion");
        assert_eq!(batch["ops"], 2);
        assert_eq!(batch["succeeded_ops"], 1);
        assert_eq!(batch["failed_ops"], 1);
        assert_eq!(batch["per_op"][1]["code"], "nested_batch");

        let response = error
            .value
            .as_ref()
            .expect("complete failing ToolResponse must survive as the error value");
        assert_eq!(response["status"], "error");
        assert_eq!(response["error"]["code"], "batch_operation_failed");
        assert!(
            response["visible"]["text"]
                .as_str()
                .unwrap()
                .contains("## 1 ingest")
        );
        assert!(
            response["visible"]["text"]
                .as_str()
                .unwrap()
                .contains("nested batch is not allowed")
        );
        assert_eq!(&response["telemetry"], batch);
        let response_refs = response["refs"].as_array().unwrap();
        assert!(!response_refs.is_empty());
        let retained_ref = response_refs[0]["ref"].as_str().unwrap();
        assert!(error.refs.iter().any(|ref_id| ref_id == retained_ref));
        assert_eq!(
            error.telemetry.visible_tokens,
            response["accounting"]["visible_tokens"].as_u64().unwrap() as usize
        );
        assert_eq!(
            error.telemetry.raw_tokens,
            response["accounting"]["raw_tokens"].as_u64().unwrap() as usize
        );
        assert_eq!(error.telemetry.refs_count, Some(response_refs.len()));

        let success = exec_batch(
            &engine,
            &[json!([{"tool": "ingest", "args": {"text": "success"}}])],
        )
        .unwrap();
        assert_eq!(success.as_value()["status"], "ok");
        assert!(success.as_value().get("ref").is_some());
    }
}

#[cfg(test)]
mod job_poll_option_tests {
    use super::*;

    fn parse(value: Value) -> JobPollOptions {
        let args = vec![Value::String("job".to_string()), value];
        job_poll_options(&Opts::from_arg(&args, 1))
    }

    #[test]
    fn bare_job_defaults_to_server_side_thirty_second_long_poll() {
        assert_eq!(
            parse(Value::Null),
            JobPollOptions {
                wait_ms: 30_000,
                since: 0,
                tail_bytes: 8 * 1024,
            }
        );
    }

    #[test]
    fn camel_case_contract_and_legacy_aliases_are_supported() {
        assert_eq!(
            parse(json!({"waitMs": 0, "since": 17, "tailBytes": 99})),
            JobPollOptions {
                wait_ms: 0,
                since: 17,
                tail_bytes: 99,
            }
        );
        assert_eq!(
            parse(json!({"wait_ms": 1_000, "cursor": 7, "tail_bytes": 42})),
            JobPollOptions {
                wait_ms: 1_000,
                since: 7,
                tail_bytes: 42,
            }
        );
    }

    #[test]
    fn excessive_wait_is_clamped_to_server_bound() {
        assert_eq!(parse(json!({"waitMs": 90_000})).wait_ms, 30_000);
    }
}

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
        .map_err(map_journal_err("journal_inspect"))
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
        // yevj: the catalog documents `ref` as the stable top-level owner ref
        // for every op. Shell previously exposed only <kind>_ref keys, so
        // `result.ref` was undefined (Grok session 019fa59e evidence). The
        // owner is the combined-stream blob (always minted, full fidelity).
        if !refs.is_empty() {
            let owner = resp
                .refs
                .iter()
                .find(|record| record.kind == "combined")
                .unwrap_or(&resp.refs[0]);
            obj["ref"] = json!(owner.ref_id);
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
    // yevj: surface the ToolResponse recovery receipt so plans can branch on
    // the terminal/do-not-recompact contract instead of guessing from size.
    if let Some(recovery) = &resp.recovery {
        obj["recovery"] = json!({
            "terminal": recovery.terminal,
            "do_not_recompact": recovery.do_not_recompact,
            "exact_bytes": recovery.exact_bytes,
        });
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

/// Millisecond shell-deadline spellings, mirroring the engine dispatcher.
const SHELL_TIMEOUT_MS_KEYS: &[&str] = &["timeout_ms", "timeoutMs", "shell_timeout_ms"];

/// Reads a shell deadline in milliseconds from any accepted spelling.
fn shell_timeout_ms_opt(opts: &Opts<'_>) -> Option<u64> {
    SHELL_TIMEOUT_MS_KEYS
        .iter()
        .find_map(|key| opts.u64(key))
        .filter(|millis| *millis > 0)
}

struct Opts<'a>(Option<&'a serde_json::Map<String, Value>>);

#[derive(Clone, Copy)]
enum OptionType {
    Bool,
    PositiveInteger,
    String,
    Mode,
}

const READ_OPTION_CONTRACT: &[(&str, OptionType)] = &[
    ("mode", OptionType::Mode),
    ("start_line", OptionType::PositiveInteger),
    ("end_line", OptionType::PositiveInteger),
    ("raw", OptionType::Bool),
    ("fresh", OptionType::Bool),
    ("max_files", OptionType::PositiveInteger),
    ("max_visible_tokens", OptionType::PositiveInteger),
];

const SHELL_OPTION_CONTRACT: &[(&str, OptionType)] = &[
    ("cwd", OptionType::String),
    ("mode", OptionType::Mode),
    ("rewrite", OptionType::String),
    ("no_rewrite", OptionType::Bool),
    ("stdin", OptionType::String),
    ("timeout_ms", OptionType::PositiveInteger),
    ("timeout_seconds", OptionType::PositiveInteger),
    ("background", OptionType::Bool),
];

impl<'a> Opts<'a> {
    fn from_arg(args: &'a [Value], index: usize) -> Self {
        Self(args.get(index).and_then(|v| v.as_object()))
    }

    fn checked(
        args: &'a [Value],
        index: usize,
        method: &str,
        contract: &[(&str, OptionType)],
    ) -> Result<Self, Box<CodeModeResult>> {
        let Some(value) = args.get(index) else {
            return Ok(Self(None));
        };
        let object = value.as_object().ok_or_else(|| {
            boxed_error(
                "validation",
                format!("{method} options must be an object, got {value}"),
            )
        })?;
        for (key, value) in object {
            let Some((_, expected)) = contract.iter().find(|(name, _)| *name == key) else {
                let supported = contract
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", ");
                let advice = if method == "zero.shell" && key == "raw" {
                    r#"; use { mode: "exact" } for exact shell output"#
                } else {
                    ""
                };
                return Err(boxed_error(
                    "validation",
                    format!(
                        "{method} unknown option '{key}'; supported options: {supported}{advice}"
                    ),
                ));
            };
            let valid = match expected {
                OptionType::Bool => value.is_boolean(),
                OptionType::PositiveInteger => value
                    .as_u64()
                    .and_then(|number| usize::try_from(number).ok())
                    .is_some_and(|number| number > 0),
                OptionType::String => value.is_string(),
                OptionType::Mode => value
                    .as_str()
                    .is_some_and(|mode| mode.parse::<Mode>().is_ok()),
            };
            if !valid {
                return Err(boxed_error(
                    "validation",
                    format!("{method} option '{key}' has an invalid value: {value}"),
                ));
            }
        }
        Ok(Self(Some(object)))
    }

    fn usize(&self, key: &str) -> Option<usize> {
        self.0?.get(key)?.as_u64().map(|n| n as usize)
    }

    fn u64(&self, key: &str) -> Option<u64> {
        self.0?.get(key)?.as_u64()
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

fn path_vals(paths: &[PathBuf]) -> Vec<Value> {
    paths
        .iter()
        .map(|p| Value::String(p.display().to_string()))
        .collect()
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
    let paths = resolve_paths_against_work_root(
        require_paths_from_arg(
            args,
            0,
            "zero.read requires a path string or array as first argument",
        )?,
        work_root,
    );
    let opts = Opts::checked(args, 1, "zero.read", READ_OPTION_CONTRACT)?;
    let mut payload = json!({
        "path": path_vals(&paths),
        "mode": opts.mode_or("mode", Mode::Auto).to_string(),
        "raw": opts.bool("raw").unwrap_or(false),
        "fresh": opts.bool("fresh").unwrap_or(false),
        "max_files": opts.usize_or("max_files", 20),
        "max_visible_tokens": opts.max_visible(engine),
    });
    if let Some(s) = opts.usize("start_line") {
        payload["start_line"] = json!(s);
    }
    if let Some(e) = opts.usize("end_line") {
        payload["end_line"] = json!(e);
    }
    let outcome = tokenzero_engine::dispatch_codemode_method(engine, "zero.read", &payload)
        .map_err(|err| operation_error(format!("{}: {}", err.kind.as_str(), err.message)))?;
    if let Some(resp) = outcome.tool_response {
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
        return tool(resp);
    }
    Err(operation_error("zero.read: empty domain outcome"))
}

fn exec_find(engine: &TokenZeroEngine, work_root: &Path, args: &[Value], exact: bool) -> OpResult {
    let pattern = require_str_arg(
        args,
        0,
        "zero.find/grep requires a pattern string as first argument",
    )?;
    let paths = paths_from_arg(args, 1, work_root.to_path_buf());
    let opts = Opts::from_arg(args, 2);
    let method = if exact { "zero.grep" } else { "zero.find" };
    let payload = json!({
        "query": pattern,
        "pattern": pattern,
        "path": path_vals(&paths),
        "mode": opts.mode_or("mode", Mode::Auto).to_string(),
        "max_files": opts.usize_or("max_files", 20),
        "max_visible_tokens": opts.max_visible(engine),
    });
    let outcome = tokenzero_engine::dispatch_codemode_method(engine, method, &payload)
        .map_err(|err| operation_error(format!("{}: {}", err.kind.as_str(), err.message)))?;
    if let Some(resp) = outcome.tool_response {
        return tool_aborting_wall(resp);
    }
    Err(operation_error(format!("{method}: empty domain outcome")))
}

fn expand_params_to_tool_args(params: &ExpandParams) -> Value {
    let mut payload = json!({
        "ref": params.ref_id,
        "fresh": params.fresh,
        "raw": params.raw,
    });
    if let Some(v) = &params.selector {
        payload["selector"] = json!(v);
    }
    if let Some(v) = params.start_line {
        payload["start_line"] = json!(v);
    }
    if let Some(v) = params.end_line {
        payload["end_line"] = json!(v);
    }
    if let Some(v) = &params.anchor_kind {
        payload["anchor_kind"] = json!(v);
    }
    if let Some(v) = &params.symbol {
        payload["symbol"] = json!(v);
    }
    if let Some(v) = &params.since {
        payload["since"] = json!(v);
    }
    payload
}

fn exec_glob(engine: &TokenZeroEngine, work_root: &Path, args: &[Value]) -> OpResult {
    let pattern = require_str_arg(
        args,
        0,
        "zero.glob requires a pattern string as first argument",
    )?;
    let paths = paths_from_arg(args, 1, work_root.to_path_buf());
    domain_via_dispatcher(
        engine,
        "zero.glob",
        &json!({
            "pattern": pattern,
            "path": path_vals(&paths),
            "include_hidden": false,
            "mode": Mode::Auto.to_string(),
            "max_files": Opts::from_arg(args, 2).usize_or("max_files", 200),
            "max_visible_tokens": engine.config.max_visible_tokens,
        }),
    )
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
    domain_via_dispatcher(
        engine,
        "zero.tree",
        &json!({
            "path": path_vals(&roots),
            "depth": opts.usize_or("depth", 3),
            "include_hidden": opts.bool("include_hidden").unwrap_or(false),
            "mode": Mode::Auto.to_string(),
            "max_files": opts.usize_or("max_files", 200),
            "max_visible_tokens": engine.config.max_visible_tokens,
        }),
    )
}

fn exec_shell(engine: &TokenZeroEngine, work_root: &Path, args: &[Value]) -> OpResult {
    let (command, argv) = match args.first() {
        Some(Value::String(command)) => (Some(command.as_str()), None),
        Some(Value::Array(items)) => {
            let argv = items
                .iter()
                .map(|item| item.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
                .filter(|argv| !argv.is_empty())
                .ok_or_else(|| {
                    boxed_error(
                        "validation",
                        "zero.shell argv must be a non-empty array of strings",
                    )
                })?;
            (None, Some(argv))
        }
        _ => {
            return Err(boxed_error(
                "validation",
                "zero.shell requires a command string or argv string array as first argument",
            ));
        }
    };
    let opts = Opts::checked(args, 1, "zero.shell", SHELL_OPTION_CONTRACT)?;
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
    // Both units are accepted. Reading only `timeout_seconds` here is what made
    // `{ timeout_ms: 1000 }` a no-op: an unrecognized key, dropped in silence,
    // so the command ran under the 60s default and reported success.
    let timeout_ms = shell_timeout_ms_opt(&opts);
    let timeout = timeout_ms.map(Duration::from_millis).or_else(|| {
        opts.usize("timeout_seconds")
            .map(|secs| Duration::from_secs(secs as u64))
    });
    // Background jobs are transport-side composition (not a registry domain op).
    if opts.bool("background").unwrap_or(false) {
        let command = command.ok_or_else(|| {
            boxed_error(
                "validation",
                "zero.shell background mode requires a command string, not argv",
            )
        })?;
        return engine
            .shell_background(command, Some(cwd.as_path()), timeout)
            .map(OpOutcome::from_catalog)
            .map_err(operation_error);
    }
    let mut payload = json!({
        "cwd": cwd.display().to_string(),
        "mode": mode.to_string(),
        "rewrite": opts.str("rewrite").unwrap_or("safe"),
        "no_rewrite": opts.bool("no_rewrite").unwrap_or(false),
    });
    if let Some(command) = command {
        payload["command"] = json!(command);
    }
    if let Some(argv) = argv {
        payload["argv"] = json!(argv);
    }
    if let Some(stdin) = opts.str("stdin") {
        payload["stdin"] = json!(stdin);
    }
    // Forward milliseconds verbatim so sub-second deadlines survive; the
    // dispatcher owns the clamping for both units.
    if let Some(millis) = timeout_ms {
        payload["timeout_ms"] = json!(millis);
    } else if let Some(secs) = opts.usize("timeout_seconds") {
        payload["timeout_seconds"] = json!(secs);
    }
    let outcome = tokenzero_engine::dispatch_codemode_method(engine, "zero.shell", &payload)
        .map_err(|err| operation_error(format!("{}: {}", err.kind.as_str(), err.message)))?;
    let Some(resp) = outcome.tool_response else {
        return Err(operation_error("zero.shell: empty domain outcome"));
    };
    Ok(OpOutcome::from_tool_response(&resp).with_value(|value| {
        if let Some(telem) = &resp.telemetry {
            if let Some(exit) = telem.get("exit_code") {
                value["exit_code"] = exit.clone();
            }
            if let Some(success) = telem.get("command_success") {
                value["success"] = success.clone();
            }
            if cwd != work_root {
                if let Some(cwd_val) = telem.get("cwd") {
                    value["cwd"] = cwd_val.clone();
                }
                if let Some(src) = telem.get("cwd_source") {
                    value["cwd_source"] = src.clone();
                }
            }
            if telem.get("stdout_ref").is_some() && value.get("stdout_ref").is_none() {
                if let Some(combined) = value.get("combined_ref").cloned() {
                    value["stdout_ref"] = combined;
                }
            }
        }
    }))
}

pub(crate) fn exec_edit(engine: &TokenZeroEngine, work_root: &Path, args: &[Value]) -> OpResult {
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
    let payload = json!({
        "path": path.display().to_string(),
        "edits": edits_val,
        "create": create,
        "dry_run": dry_run,
        "mode": Mode::Auto.to_string(),
        "max_visible_tokens": engine.config.max_visible_tokens,
    });
    let outcome = tokenzero_engine::dispatch_codemode_method(engine, "zero.edit", &payload)
        .map_err(|err| operation_error(format!("{}: {}", err.kind.as_str(), err.message)))?;
    let Some(resp) = outcome.tool_response else {
        return Err(operation_error("zero.edit: empty domain outcome"));
    };
    if resp.status == "error" {
        let message = resp
            .error
            .as_ref()
            .map(|e| e.message.clone())
            .unwrap_or_else(|| "zero.edit failed".to_string());
        let annotated = tokenzero_engine::annotate_write_failure(&message, false);
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

const DEFAULT_EXPAND_VISIBLE_TOKENS: usize = 1200;

fn bound_default_expand_response(
    params: &ExpandParams,
    response: &mut ToolResponse,
    configured_limit: usize,
) -> Result<bool, Box<CodeModeResult>> {
    if params.raw
        || params.selector.is_some()
        || params.start_line.is_some()
        || params.end_line.is_some()
        || params.symbol.is_some()
        || params.anchor_kind.is_some()
        || params.since.is_some()
    {
        return Ok(false);
    }
    let Some(text) = response
        .visible
        .as_ref()
        .map(|visible| visible.text.clone())
    else {
        return Ok(false);
    };
    let raw_tokens = count_tokens(&text);
    let limit = configured_limit.clamp(128, DEFAULT_EXPAND_VISIBLE_TOKENS);
    if raw_tokens <= limit {
        return Ok(false);
    }
    let content_type = detect_content_type(&text, None);
    let capsule = tokenzero_core::make_capsule_content_aware(
        &text,
        raw_tokens,
        content_type,
        limit,
        Some("expand"),
        Some(&params.ref_id),
        false,
    )
    .map_err(capsule_operation_error)?;
    if let Some(visible) = response.visible.as_mut() {
        visible.text = capsule.text;
    }
    response.mode = Some(Mode::Auto.to_string());
    response.detail_ref = Some(params.ref_id.clone());
    if !response
        .refs
        .iter()
        .any(|record| record.ref_id == params.ref_id)
    {
        response.refs.push(tokenzero_core::ref_record(
            "blob",
            params.ref_id.clone(),
            text.len(),
        ));
    }
    if let Some(accounting) = response.accounting.as_mut() {
        accounting.raw_tokens = raw_tokens;
        accounting.visible_tokens = capsule.visible_tokens;
        accounting.exact_ref_tokens = Some(count_tokens(&params.ref_id));
    }
    let telemetry = response.telemetry.get_or_insert_with(|| json!({}));
    telemetry["expand_bounded"] = json!(true);
    telemetry["raw_tokens"] = json!(raw_tokens);
    telemetry["visible_tokens"] = json!(capsule.visible_tokens);
    telemetry["exact_recovery"] = json!("use {raw:true} or an explicit line/symbol window");
    Ok(true)
}

fn exec_expand(engine: &TokenZeroEngine, args: &[Value]) -> OpResult {
    // Array first arg → expandMany (agents often pass expand([ref, ...])).
    if args.first().is_some_and(Value::is_array) {
        return exec_expand_many(engine, args);
    }
    let params = ExpandParams::from_codemode_args(args).map_err(operation_error)?;
    if !tokenzero_recovery::is_expandable_ref(&params.ref_id) {
        return Err(operation_error(format!(
            "expand takes a tz:// fz:// gz:// ref; to read a file use zero.fs.compound('read',{{path}}) -- got: {}",
            params.ref_id
        )));
    }
    // Soft capsule on expand miss/error: plan continues with status in value.
    // Shared SurfaceHealth is updated inside expand_with_params (wqw.9).
    let payload = expand_params_to_tool_args(&params);
    let outcome = tokenzero_engine::dispatch_codemode_method(engine, "zero.expand", &payload)
        .map_err(|err| operation_error(format!("{}: {}", err.kind.as_str(), err.message)))?;
    let Some(mut resp) = outcome.tool_response else {
        return Err(operation_error("zero.expand: empty domain outcome"));
    };
    // Strict ZeroRef parsing can classify a missing legacy-shaped blob as malformed.
    // In CodeMode that is still an expand-surface miss for crash-only recovery (wqw.9).
    if resp
        .error
        .as_ref()
        .is_some_and(|error| error.code == "zeroref_malformed")
    {
        engine.surface_health().record_codemode_expand_x0();
    }
    if resp
        .error
        .as_ref()
        .is_some_and(|error| error.code == "hard_max_wall_ms")
    {
        return tool_aborting_wall(resp);
    }
    let bounded = resp.error.is_none()
        && bound_default_expand_response(&params, &mut resp, engine.config.max_visible_tokens)?;
    // yevj: the typed receipt is the do-not-recompact source of truth;
    // legacy responses without one keep the prior terminal default.
    let terminal = resp
        .recovery
        .as_ref()
        .map(|receipt| receipt.do_not_recompact)
        .unwrap_or(true);
    let outcome = OpOutcome::from_tool_response(&resp);
    if resp.error.is_none() && !bounded && terminal {
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
    for (idx, item) in items.iter().enumerate() {
        if let Some((message, _)) = tokenzero_engine::wall::check_active_wall_deadline_every(idx, 1)
        {
            return Err(operation_error(message));
        }
        let params = ExpandParams::from_expand_many_item(item).map_err(operation_error)?;
        if !tokenzero_recovery::is_expandable_ref(&params.ref_id) {
            return Err(operation_error(format!(
                "expandMany takes a tz:// fz:// gz:// ref; to read a file use zero.fs.compound('read',{{path}}) -- got: {}",
                params.ref_id
            )));
        }
        let payload = expand_params_to_tool_args(&params);
        let outcome = tokenzero_engine::dispatch_codemode_method(engine, "zero.expand", &payload)
            .map_err(|err| {
            operation_error(format!("{}: {}", err.kind.as_str(), err.message))
        })?;
        let Some(mut resp) = outcome.tool_response else {
            return Err(operation_error("zero.expand: empty domain outcome"));
        };
        if resp
            .error
            .as_ref()
            .is_some_and(|error| error.code == "hard_max_wall_ms")
        {
            return tool_aborting_wall(resp);
        }
        prevented = prevented.saturating_add(estimate_prevented_read_bytes(&resp));
        let bounded = resp.error.is_none()
            && bound_default_expand_response(&params, &mut resp, engine.config.max_visible_tokens)?;
        let terminal = resp
            .recovery
            .as_ref()
            .map(|receipt| receipt.do_not_recompact)
            .unwrap_or(true);
        let outcome = OpOutcome::from_tool_response(&resp);
        results.push(if bounded || !terminal {
            outcome.into_value()
        } else {
            outcome.mark_exact_expand().into_value()
        });
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
        let stored = Some(
            store
                .store_payload(&data, content_type, None, None, None)
                .map_err(|error| {
                    boxed_error(
                        "store",
                        format!(
                            "zero.token.compact could not persist its recovery payload: {error}"
                        ),
                    )
                })?,
        );
        let recovery_ref = stored.as_ref().map(|s| s.blob_ref.as_str());
        let capsule = tokenzero_core::make_capsule_content_aware(
            &data,
            raw_tokens,
            content_type,
            engine.config.max_visible_tokens,
            Some("compact"),
            recovery_ref,
            aggressive,
        )
        .map_err(capsule_operation_error)?;
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
        // A compact call cannot know which bytes a later operation or turn will
        // recover. Label this one-shot figure gross rather than presenting it
        // as recovery-adjusted session savings.
        let mut value = json!({ "text": capsule.text, "status": "ok", "raw_tokens": raw_tokens, "visible_tokens": capsule.visible_tokens, "compression_strategy": strategy, "gross_savings_pct": format!("{:.0}%", tokenzero_core::savings_ratio(raw_tokens, capsule.visible_tokens) * 100.0), "savings_scope": "initial_compaction_before_future_recovery" });
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
        let stored = Some(
            store
                .store_payload(&text, content_type, None, None, None)
                .map_err(|error| {
                    boxed_error(
                        "store",
                        format!(
                            "zero.pipe could not persist its aggregate recovery payload: {error}"
                        ),
                    )
                })?,
        );
        let ref_id = stored
            .as_ref()
            .map(|s| s.blob_ref.as_str().to_string())
            .unwrap_or_default();
        if ref_id.is_empty() {
            return Ok(OpOutcome::from_catalog(full).with_prevented_read_bytes(prevented));
        }
        Ok(OpOutcome::from_catalog(json!({ "ref": ref_id, "preview": compact_value_preview(&text, 256usize.saturating_sub(count_tokens(&ref_id))) })).with_prevented_read_bytes(prevented))
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
    fn capsule_operation_error_has_a_stable_kind() {
        let result = capsule_operation_error("synthetic invariant failure".to_string());
        let error = result.error.as_ref().expect("typed CodeMode error");
        assert_eq!(error.kind, "capsule_omission_invalid");
        assert!(error.message.contains("synthetic invariant failure"));
    }

    #[test]
    fn compact_max_recovery_unaware_savings_are_labeled_gross() {
        let payload = "decisive evidence ".repeat(2_000);
        let plan = format!(
            "return zero.compact_max({})",
            serde_json::to_string(&payload).unwrap()
        );
        let result = execute_codemode(&plan);
        assert_eq!(result.status, CodeModeStatus::Completed);
        let value = result.value.expect("compact_max value");
        assert!(value.get("gross_savings_pct").is_some(), "{value}");
        assert!(value.get("savings_pct").is_none(), "{value}");
        assert_eq!(
            value.get("savings_scope").and_then(Value::as_str),
            Some("initial_compaction_before_future_recovery")
        );
    }

    #[test]
    fn exact_expand_registry_is_execution_scoped() {
        let stale = Value::String("stale exact payload".to_string());
        RECOVERED_TOKENS.with(|tokens| tokens.set(0));
        record_exact_expand_payload(stale.as_str().unwrap());
        record_exact_expand_payload(stale.as_str().unwrap());
        assert!(is_exact_expand_value(&stale));
        assert_eq!(
            RECOVERED_TOKENS.with(std::cell::Cell::get),
            count_tokens(stale.as_str().unwrap()) * 2,
            "re-presenting identical recovered bytes must debit M_rec again",
        );
        let fresh = execute_codemode("return 'fresh'");
        assert_eq!(fresh.status, CodeModeStatus::Completed);
        assert_eq!(fresh.telemetry.recovery_tokens(), 0);
        assert!(!is_exact_expand_value(&stale));
    }
}

#[cfg(test)]
mod async_host_runtime_tests {
    use super::*;

    fn test_runtime() -> Rc<AsyncHostRuntime> {
        Rc::new(AsyncHostRuntime {
            next_id: AtomicU64::new(1),
            jobs: Mutex::new(HashMap::new()),
            gate: Arc::new(ParallelWidthGate::new(4)),
            completion: Arc::new(CompletionGate::new()),
            workers: Mutex::new(Vec::new()),
        })
    }

    #[test]
    fn runtime_drop_joins_detached_workers() {
        let rt = test_runtime();
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_flag = Arc::clone(&flag);
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            worker_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        rt.workers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(handle);
        drop(rt);
        assert!(
            flag.load(std::sync::atomic::Ordering::SeqCst),
            "runtime drop must join detached workers"
        );
    }

    fn test_state(started_ms: u128) -> Rc<RefCell<JsExecutionState>> {
        Rc::new(RefCell::new(JsExecutionState {
            started_ms,
            limits: limits_from_options(&CodeModeOptions::default()),
            ..JsExecutionState::default()
        }))
    }

    fn pending_job(rt: &AsyncHostRuntime, id: u64) -> Arc<AsyncHostJob> {
        let job = Arc::new(AsyncHostJob {
            result: Mutex::new(None),
            method: "zero.test".to_string(),
            tracks_wave: true,
            applied: Mutex::new(false),
        });
        rt.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Arc::clone(&job));
        job
    }

    #[test]
    fn finisher_resolves_job_and_releases_width_gate_on_panic() {
        let rt = test_runtime();
        let job = pending_job(&rt, 1);
        let finisher = HostJobFinisher {
            job: Arc::clone(&job),
            completion: Arc::clone(&rt.completion),
            width_gate: Arc::clone(&rt.gate),
        };
        rt.gate.acquire();
        let worker = std::thread::spawn(move || {
            let _finisher = finisher;
            panic!("simulated dispatch panic");
        });
        assert!(worker.join().is_err(), "worker must have panicked");
        let resolved = job
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .expect("finisher must resolve the job on panic");
        assert!(
            resolved.contains("__tz_error"),
            "expected error payload: {resolved}"
        );
        let active = *rt.gate.active.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(active, 0, "width-gate slot must be released on panic");
        let state = test_state(now_ms());
        let status = poll_js_host_op("1", &state, &rt);
        let parsed: Value = serde_json::from_str(&status).expect("poll json");
        assert_eq!(parsed.get("done").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn poll_blocks_on_completion_gate_instead_of_spinning() {
        let rt = test_runtime();
        let job = pending_job(&rt, 1);
        let completer = {
            let job = Arc::clone(&job);
            let completion = Arc::clone(&rt.completion);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(80));
                *job.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(
                    "{\"__tz_ok\":true,\"value\":null,\"prevented_read_bytes\":0}".to_string(),
                );
                completion.bump_and_notify();
            })
        };
        let state = test_state(now_ms());
        let started = std::time::Instant::now();
        let status = poll_js_host_op("1", &state, &rt);
        let blocked_ms = started.elapsed().as_millis();
        assert!(
            blocked_ms >= 50,
            "poll must block on the completion gate, not return after 1ms: {blocked_ms}ms"
        );
        let parsed: Value = serde_json::from_str(&status).expect("poll json");
        assert_eq!(parsed.get("done").and_then(Value::as_bool), Some(false));
        completer.join().expect("completer thread");
        let status = poll_js_host_op("1", &state, &rt);
        let parsed: Value = serde_json::from_str(&status).expect("poll json");
        assert_eq!(parsed.get("done").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn poll_resolves_hard_wall_error_when_budget_exceeded() {
        let rt = test_runtime();
        let _job = pending_job(&rt, 1);
        let hard = limits_from_options(&CodeModeOptions::default()).hard_max_wall_ms;
        let state = test_state(now_ms().saturating_sub(u128::from(hard) + 5_000));
        let status = poll_js_host_op("1", &state, &rt);
        let parsed: Value = serde_json::from_str(&status).expect("poll json");
        assert_eq!(parsed.get("done").and_then(Value::as_bool), Some(true));
        let result = parsed
            .get("result")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            result.contains("hard_max_wall_ms exceeded"),
            "expected hard-wall error payload: {result}"
        );
    }
}
