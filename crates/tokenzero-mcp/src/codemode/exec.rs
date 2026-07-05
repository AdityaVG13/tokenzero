//! CodeMode plan executor and TokenZero operation dispatch.

use rquickjs::{Context, Runtime, function::Func};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
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
use super::sandbox::lower_code_plan;
use super::store::{CodeModeLimits, ExecutionStep, ExecutionStore, finalize_result, now_ms};
use crate::expand_params::ExpandParams;

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
    if should_run_quickjs(plan) {
        if quickjs_plan_requests_mutation(plan) {
            return finalize_codemode_result(
                CodeModeResult::error(
                    "sandbox: mutating binding denied without transaction support".to_string(),
                    0,
                ),
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
          const value = JSON.parse(text);
          if (value && value.__tz_error) throw new Error(value.__tz_error);
          return value;
        };
        const __tz_call = (method, args) => __tz_parse(__tz_call_json(method, JSON.stringify(args)));
        const zero = Object.freeze({
          read: (...args) => __tz_call('zero.read', args),
          find: (...args) => __tz_call('zero.find', args),
          grep: (...args) => __tz_call('zero.grep', args),
          glob: (...args) => __tz_call('zero.glob', args),
          tree: (...args) => __tz_call('zero.tree', args),
          shell: (...args) => __tz_call('zero.shell', args),
          expand: (...args) => __tz_call('zero.token.expand', args),
          compact: (...args) => __tz_call('zero.token.compact', args),
          compact_max: (...args) => __tz_call('zero.compact_max', args),
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
            compact: (text) => __tz_parse(__tz_compact_json(String(text))),
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

fn limits_from_options(options: &CodeModeOptions) -> CodeModeLimits {
    CodeModeLimits {
        max_output_bytes: options.max_output_bytes,
        max_refs_emitted: options.max_refs_emitted,
        max_logical_ops: options.max_logical_ops,
        max_physical_ops: options.max_physical_ops,
        max_microtasks: options.max_microtasks,
        max_memory_bytes: options.max_memory_bytes,
        max_code_bytes: options.max_code_bytes,
        ..Default::default()
    }
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
    let mut scope: HashMap<String, Value> = HashMap::new();
    let mut all_refs: Vec<String> = Vec::new();
    let mut ops: usize = 0;
    let mut total_visible: usize = 0;
    let mut total_raw: usize = 0;
    let mut last_value: Value = Value::Null;
    let mut steps: Vec<ExecutionStep> = Vec::new();

    for stmt in &statements {
        match stmt {
            Statement::Binding { name, call } => {
                ops += 1;
                let outcome = match dispatch(&engine, &work_root, call, &scope) {
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
                steps.push(ExecutionStep {
                    id: name.clone(),
                    method: call.method.clone(),
                    status: "completed".to_string(),
                    refs: refs_from_value(outcome.as_value()),
                });
                record_outcome(&outcome, &mut all_refs, &mut total_visible, &mut total_raw);
                last_value = outcome.as_value().clone();
                scope.insert(name.clone(), outcome.into_value());
            }
            Statement::Call(call) => {
                ops += 1;
                let outcome = match dispatch(&engine, &work_root, call, &scope) {
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
                steps.push(ExecutionStep {
                    id: format!("step{ops}"),
                    method: call.method.clone(),
                    status: "completed".to_string(),
                    refs: refs_from_value(outcome.as_value()),
                });
                record_outcome(&outcome, &mut all_refs, &mut total_visible, &mut total_raw);
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
                return finalize_codemode_result(
                    CodeModeResult::completed(value, all_refs, ops, total_visible + vis, total_raw),
                    kind,
                    plan,
                    started_ms,
                    &options,
                    limits,
                    steps,
                );
            }
        }
    }

    let vis = count_tokens(&serde_json::to_string(&last_value).unwrap_or_default());
    finalize_codemode_result(
        CodeModeResult::completed(last_value, all_refs, ops, total_visible + vis, total_raw),
        kind,
        plan,
        started_ms,
        &options,
        limits,
        steps,
    )
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
    let operations = result.telemetry.operations();
    let physical_ops = physical_ops_for(&result);
    result.telemetry.operations = operations;
    result.telemetry.logical_ops = operations;
    result.telemetry.physical_ops = physical_ops;
    result.telemetry.batched_ops = if operations > physical_ops { 1 } else { 0 };
    result.telemetry.internal_actions = operations.saturating_add(result.refs.len());
    if let Some(extra) = result
        .telemetry
        .extra
        .as_mut()
        .and_then(Value::as_object_mut)
    {
        extra.insert("refs_count".to_string(), json!(result.refs.len()));
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
    let mut scope: HashMap<String, Value> = HashMap::new();
    let mut all_refs = Vec::new();
    let mut total_visible = 0usize;
    let mut total_raw = 0usize;
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
        let outcome = match dispatch(&engine, &work_root, &call, &scope) {
            Ok(outcome) => outcome,
            Err(mut err) => {
                if let Some(extra) = err.telemetry.extra.as_mut().and_then(Value::as_object_mut) {
                    extra.insert("operations".to_string(), json!(idx + 1));
                }
                err.telemetry.operations = idx + 1;
                err.telemetry.logical_ops = idx + 1;
                return finalize_codemode_result(
                    *err, "json", plan, started_ms, &options, limits, executed,
                );
            }
        };
        let refs = refs_from_value(outcome.as_value());
        executed.push(ExecutionStep {
            id: id.clone(),
            method,
            status: "completed".to_string(),
            refs,
        });
        record_outcome(&outcome, &mut all_refs, &mut total_visible, &mut total_raw);
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
    }
    result.telemetry.parallel_groups = Some(count_parallel_groups(steps_arr));
    result.telemetry.physical_ops = estimate_physical_ops(steps_arr);
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
        "zero.read" | "read" | "zero.token.read" => exec_read(engine, args),
        "zero.find" | "find" | "zero.token.find" => exec_find(engine, work_root, args, false),
        "zero.grep" | "grep" | "zero.token.grep" => exec_find(engine, work_root, args, true),
        "zero.glob" | "glob" | "zero.token.glob" => exec_glob(engine, work_root, args),
        "zero.tree" | "tree" | "zero.token.tree" => exec_tree(engine, work_root, args),
        "zero.shell" | "shell" | "zero.token.shell" => exec_shell(engine, args),
        "zero.edit" | "edit" => Err(Box::new(CodeModeResult::error_with_kind(
            "policy",
            "mutating binding denied without transaction support",
            0,
            false,
        ))),
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
    if resp.status == "error" {
        let message = resp
            .error
            .as_ref()
            .map(|error| error.message.clone())
            .unwrap_or_else(|| "zero.read failed".to_string());
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

#[allow(dead_code)]
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
    let params = ExpandParams::from_codemode_args(args)
        .map_err(|message| Box::new(CodeModeResult::error(message, 0)))?;
    if !params.ref_id.starts_with("tz://") {
        return Err(Box::new(CodeModeResult::error(
            format!(
                "zero.token.expand/zero.expand: ref must start with tz://, got: {}",
                params.ref_id
            ),
            0,
        )));
    }
    let resp = engine.expand_with_params(params);
    Ok(OpOutcome::from_tool_response(&resp))
}

fn exec_expand_many(
    engine: &TokenZeroEngine,
    args: &[Value],
) -> Result<OpOutcome, Box<CodeModeResult>> {
    let items = match args.first() {
        Some(Value::Array(items)) => items,
        _ => {
            return Err(Box::new(CodeModeResult::error(
                "zero.token.expandMany requires an array of tz:// refs or item objects",
                0,
            )));
        }
    };
    let mut results = Vec::with_capacity(items.len());
    for item in items {
        let params = ExpandParams::from_expand_many_item(item)
            .map_err(|message| Box::new(CodeModeResult::error(message, 0)))?;
        if !params.ref_id.starts_with("tz://") {
            return Err(Box::new(CodeModeResult::error(
                format!(
                    "zero.token.expandMany: ref must start with tz://, got: {}",
                    params.ref_id
                ),
                0,
            )));
        }
        let resp = engine.expand_with_params(params);
        results.push(OpOutcome::from_tool_response(&resp).into_value());
    }
    Ok(OpOutcome::from_catalog(json!({
        "items": results,
        "count": results.len(),
    })))
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
