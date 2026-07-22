//! Durable CodeMode execution records, limits, refs, and response guards.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use tokenzero_core::{AckClass, ContentType, count_tokens, render_ack};
use tokenzero_recovery::RecoveryStore;

use super::journal::{OperationClass, classify_method};
use super::result::{CodeModeResult, CodeModeStatus, CodeModeTelemetry};

pub const CODEMODE_LIMITS_SCHEMA: &str = "tokenzero.codemode.limits.v1";
pub const DEFAULT_MAX_LOGICAL_OPS: usize = 1000;
pub const DEFAULT_MAX_PHYSICAL_OPS: usize = 256;
pub const HARD_MAX_WALL_MS: u64 = 5000;

/// Deployment override for the server-level hard wall ceiling, clamped to
/// [1s, 300s]. Five seconds serializes real work behind machine-permit waits
/// on busy multi-session machines (2026-07-16 incident); hubs set
/// `TOKENZERO_CODEMODE_HARD_MAX_WALL_MS` to trade latency for headroom while
/// per-call limits still clamp to this ceiling.
pub fn hard_max_wall_ms() -> u64 {
    static VALUE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("TOKENZERO_CODEMODE_HARD_MAX_WALL_MS")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .map(|ms| ms.clamp(1_000, 300_000))
            .unwrap_or(HARD_MAX_WALL_MS)
    })
}
pub const DEFAULT_MAX_MICROTASKS: usize = 4096;
pub const DEFAULT_MAX_MEMORY_BYTES: usize = 32 * 1024 * 1024;
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;
pub const DEFAULT_MAX_RESULT_REF_BYTES: usize = 10 * 1024 * 1024;
pub const DEFAULT_MAX_REFS_EMITTED: usize = 256;
pub const DEFAULT_MAX_PARALLEL_WIDTH: usize = 2;
pub const DEFAULT_MAX_CODE_BYTES: usize = 64 * 1024;
pub const DEFAULT_MAX_VISIBLE_TOKENS: usize = 4000;

/// Deployment default for the recipe and response token envelope. Per-call
/// limits.max_visible_tokens remains authoritative when supplied.
pub fn default_max_visible_tokens() -> usize {
    std::env::var("TOKENZERO_CODEMODE_MAX_VISIBLE_TOKENS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .map(|tokens| tokens.clamp(1, 1_000_000))
        .unwrap_or(DEFAULT_MAX_VISIBLE_TOKENS)
}

// serde(default): tool callers send PARTIAL limits objects (the documented
// contract — e.g. {"max_output_bytes": 1024}); without per-field defaults a
// partial object fails deserialization and tools.rs's `if let Ok` silently
// DROPS the caller's limits (observed in PR 16 review — the exact
// silent-failure class this codebase hunts).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CodeModeLimits {
    pub max_logical_ops: usize,
    pub max_physical_ops: usize,
    pub max_wall_ms: u64,
    pub hard_max_wall_ms: u64,
    pub max_microtasks: usize,
    pub max_memory_bytes: usize,
    pub max_output_bytes: usize,
    pub max_result_ref_bytes: usize,
    pub max_refs_emitted: usize,
    pub max_parallel_width: usize,
    pub max_code_bytes: usize,
    pub max_visible_tokens: usize,
}

impl Default for CodeModeLimits {
    fn default() -> Self {
        Self {
            max_logical_ops: DEFAULT_MAX_LOGICAL_OPS,
            max_physical_ops: DEFAULT_MAX_PHYSICAL_OPS,
            max_wall_ms: hard_max_wall_ms(),
            hard_max_wall_ms: hard_max_wall_ms(),
            max_microtasks: DEFAULT_MAX_MICROTASKS,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            max_result_ref_bytes: DEFAULT_MAX_RESULT_REF_BYTES,
            max_refs_emitted: DEFAULT_MAX_REFS_EMITTED,
            max_parallel_width: DEFAULT_MAX_PARALLEL_WIDTH,
            max_code_bytes: DEFAULT_MAX_CODE_BYTES,
            max_visible_tokens: default_max_visible_tokens(),
        }
    }
}

impl CodeModeLimits {
    pub fn as_json(&self) -> Value {
        json!({
            "schema": CODEMODE_LIMITS_SCHEMA,
            "max_logical_ops": self.max_logical_ops,
            "max_physical_ops": self.max_physical_ops,
            "max_wall_ms": self.max_wall_ms,
            "hard_max_wall_ms": self.hard_max_wall_ms,
            "max_microtasks": self.max_microtasks,
            "max_memory_bytes": self.max_memory_bytes,
            "max_output_bytes": self.max_output_bytes,
            "max_result_ref_bytes": self.max_result_ref_bytes,
            "max_refs_emitted": self.max_refs_emitted,
            "max_parallel_width": self.max_parallel_width,
            "max_code_bytes": self.max_code_bytes,
            "max_visible_tokens": self.max_visible_tokens,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStep {
    pub id: String,
    pub method: String,
    pub status: String,
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub execution_id: String,
    pub kind: String,
    pub status: String,
    pub visible_ack: String,
    pub code_ref: String,
    pub steps_ref: String,
    pub telemetry_ref: String,
    pub result_ref: Option<String>,
    pub error_ref: Option<String>,
    pub refs: Vec<String>,
}

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

pub fn execution_id(code: &str, started_ms: u128) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(code.as_bytes());
    let digest = hasher.finalize();
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("cm://exec/{started_ms}-{}", &hex[..12])
}

pub fn execution_ref(id: &str, suffix: &str) -> String {
    let normalized = id.strip_prefix("cm://exec/").unwrap_or(id);
    if suffix.is_empty() {
        format!("tz://codemode/execution/{normalized}")
    } else {
        format!("tz://codemode/execution/{normalized}/{suffix}")
    }
}

#[cfg(test)]
thread_local! {
    static COMMIT_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub struct ExecutionStore {
    store: RecoveryStore,
}

impl ExecutionStore {
    pub fn new(cache_path: std::path::PathBuf) -> Self {
        Self {
            store: RecoveryStore::new(Some(cache_path)),
        }
    }
    pub fn store_json_deferred(&mut self, value: &Value) -> Result<String, String> {
        self.store_text_deferred(
            &serde_json::to_string_pretty(value).map_err(|error| error.to_string())?,
            ContentType::JsonConfig,
        )
    }

    pub fn store_text_deferred(
        &mut self,
        text: &str,
        content_type: ContentType,
    ) -> Result<String, String> {
        Ok(self
            .store
            .store_payload_deferred_batch(text, content_type, None, None, None)
            .blob_ref)
    }

    pub fn alias_deferred(&mut self, logical_ref: &str, target_ref: &str) {
        self.store.store_alias_deferred(logical_ref, target_ref);
    }

    pub fn commit(&mut self) -> Result<(), String> {
        #[cfg(test)]
        COMMIT_CALLS.with(|calls| calls.set(calls.get() + 1));
        self.store
            .persist_pending_durable()
            .map_err(|error| error.to_string())
    }
}

fn storage_failure(message: String, operations: usize) -> CodeModeResult {
    CodeModeResult::error_with_kind(
        "store",
        format!("execution record commit failed: {message}"),
        operations,
        false,
    )
}

fn deferred_json(store: &mut ExecutionStore, value: &Value) -> String {
    store
        .store_json_deferred(value)
        .unwrap_or_else(|error| format!("store-error:{error}"))
}

fn deferred_text(store: &mut ExecutionStore, text: &str, content_type: ContentType) -> String {
    store
        .store_text_deferred(text, content_type)
        .unwrap_or_else(|error| format!("store-error:{error}"))
}


#[allow(clippy::too_many_arguments)]
pub fn finalize_result(
    mut result: CodeModeResult,
    kind: &str,
    plan: &str,
    started_ms: u128,
    finished_ms: u128,
    mut store: ExecutionStore,
    limits: &CodeModeLimits,
    steps: Vec<ExecutionStep>,
) -> CodeModeResult {
    let id = execution_id(plan, started_ms);
    let completed = matches!(result.status, CodeModeStatus::Completed);
    let silent_success = completed
        && !steps.is_empty()
        && steps.iter().all(|step| {
            classify_method(&step.method) == OperationClass::ReversibleStoreMutation
        });
    let (status_str, telemetry_status) = if completed {
        ("completed", "ok")
    } else {
        ("error", "error")
    };
    result.execution_id = Some(id.clone());
    if completed {
        result.visible_ack = render_ack(AckClass::Success, silent_success).into();
    }
    result.telemetry.kind = "codemode.execute".into();
    result.telemetry.status = telemetry_status.into();
    result.telemetry.wall_ms = finished_ms.saturating_sub(started_ms) as u64;
    result.telemetry.steps_run = Some(steps.len());
    let mut extra = result
        .telemetry
        .extra
        .take()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    extra.insert("execution_id".to_string(), json!(id.clone()));
    extra.insert("visible_ack".to_string(), json!(result.visible_ack.clone()));
    extra.insert("steps_run".to_string(), json!(steps.len()));
    extra.insert("started_at_ms".to_string(), json!(started_ms));
    extra.insert("finished_at_ms".to_string(), json!(finished_ms));
    result.telemetry.extra = Some(Value::Object(extra));

    let code_ref = deferred_text(&mut store, plan, ContentType::Code);
    let steps_ref = deferred_json(&mut store, &json!(steps));
    let telemetry_ref = deferred_json(&mut store, &json!(result.telemetry));

    let result_ref = result.value.as_ref().and_then(|value| {
        serde_json::to_vec(value)
            .ok()
            .filter(|bytes| bytes.len() <= limits.max_result_ref_bytes)
            .map(|_| deferred_json(&mut store, value))
    });
    let error_ref = result
        .error
        .as_ref()
        .map(|error| deferred_json(&mut store, &json!(error)));

    let logical_ref = |suffix| execution_ref(&id, suffix);
    let execution_logical_ref = logical_ref("");
    let code_logical_ref = logical_ref("code");
    let steps_logical_ref = logical_ref("steps");
    let telemetry_logical_ref = logical_ref("telemetry");
    let result_logical_ref = logical_ref("result");
    let error_logical_ref = logical_ref("error");
    let envelope_logical_ref = logical_ref("envelope");
    result.detail_ref = Some(if completed && result_ref.is_some() {
        result_logical_ref.clone()
    } else if completed {
        execution_logical_ref.clone()
    } else {
        error_logical_ref.clone()
    });

    // Persist full record + stream blobs for expand, but do not spell them in
    // the visible envelope: every suffix is derivable from execution_id.
    let record = ExecutionRecord {
        execution_id: id.clone(),
        kind: kind.to_string(),
        status: status_str.to_string(),
        visible_ack: result.visible_ack.clone(),
        code_ref: code_ref.clone(),
        steps_ref: steps_ref.clone(),
        telemetry_ref: telemetry_ref.clone(),
        result_ref: result_ref.clone(),
        error_ref: error_ref.clone(),
        refs: result.refs.clone(),
    };
    let record_value = serde_json::to_value(&record).unwrap_or(Value::Null);
    let execution_record_ref = deferred_json(&mut store, &record_value);

    // envelope.v3: one execution_id replaces execution_refs + store blocks.
    let envelope_bundle = json!({
        "schema": "tokenzero.codemode.envelope.v3",
        "execution_id": id.clone(),
        "status": status_str,
        "ack": result.visible_ack.clone(),
        "detail_ref": result.detail_ref.clone(),
        "telemetry": result.telemetry.clone(),
        "refs": result.refs.clone(),
    });
    let envelope_ref = deferred_json(&mut store, &envelope_bundle);

    for (logical, stored) in [
        (&execution_logical_ref, Some(execution_record_ref.as_str())),
        (&code_logical_ref, Some(code_ref.as_str())),
        (&steps_logical_ref, Some(steps_ref.as_str())),
        (&telemetry_logical_ref, Some(telemetry_ref.as_str())),
        (&result_logical_ref, result_ref.as_deref()),
        (&error_logical_ref, error_ref.as_deref()),
        (&envelope_logical_ref, Some(envelope_ref.as_str())),
    ] {
        if let Some(stored) = stored {
            store.alias_deferred(logical, stored);
        }
    }

    if let Err(error) = store.commit() {
        return storage_failure(error, result.telemetry.operations());
    }

    if result.refs.len() < limits.max_refs_emitted {
        result.refs.push(execution_record_ref);
    }
    // Visible execution_refs: execution + envelope only (suffixes are derivable).
    result.execution_refs = Some(json!({
        "execution": execution_logical_ref,
        "envelope": envelope_logical_ref,
        "stored": {
            "envelope": envelope_ref,
        }
    }));
    guard_visible_output(&mut result, limits, result_ref.as_deref());
    // Autopage: expose the terminal result blob (never envelope) for one-hop expand.
    if let Some(continuation) = result
        .value
        .as_ref()
        .and_then(|value| value.get("continuation_ref"))
        .and_then(Value::as_str)
    {
        if let Some(stored) = result
            .execution_refs
            .as_mut()
            .and_then(|refs| refs.get_mut("stored"))
            .and_then(Value::as_object_mut)
        {
            stored.insert("result".to_string(), json!(continuation));
        }
    }
    result
}

fn record_visible_tokens(telemetry: &mut CodeModeTelemetry, visible: usize) {
    telemetry.visible_tokens = visible;
    if let Some(extra) = telemetry.extra.as_mut().and_then(Value::as_object_mut) {
        extra.insert("visible_tokens".to_string(), json!(visible));
    }
}

fn guard_visible_output(
    result: &mut CodeModeResult,
    limits: &CodeModeLimits,
    continuation_ref: Option<&str>,
) {
    if result.refs.len() > limits.max_refs_emitted {
        result.refs.truncate(limits.max_refs_emitted);
    }
    if let Some(value) = &mut result.value {
        if cap_exact_expand_value(value, limits.max_output_bytes) {
            let visible = count_tokens(&serde_json::to_string(value).unwrap_or_default());
            record_visible_tokens(&mut result.telemetry, visible);
        }
        strip_exact_expand_markers(value);
    }
    if let Some(value) = &mut result.value {
        if let Value::String(text) = value.clone() {
            if text.len() > limits.max_output_bytes && super::exec::is_exact_expand_value(value) {
                let fitted = truncate_string_to_fit(text, |candidate| {
                    serde_json::to_string(candidate)
                        .is_ok_and(|serialized| serialized.len() <= limits.max_output_bytes)
                });
                super::exec::record_exact_expand_payload(&fitted);
                *value = Value::String(fitted);
                let visible = count_tokens(&serde_json::to_string(value).unwrap_or_default());
                result.telemetry.visible_tokens = visible;
            }
        }
    }
    if let Some(value) = &result.value {
        let bytes = serde_json::to_vec(value)
            .map(|bytes| bytes.len())
            .unwrap_or(0);
        if bytes > limits.max_output_bytes {
            let paged = autopage_over_cap(value, bytes, limits.max_output_bytes, continuation_ref);
            if let Some(cref) = paged
                .get("continuation_ref")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                if !result.refs.contains(&cref) && result.refs.len() < limits.max_refs_emitted {
                    result.refs.push(cref);
                }
            }
            let visible = count_tokens(&serde_json::to_string(&paged).unwrap_or_default());
            result.value = Some(paged);
            record_visible_tokens(&mut result.telemetry, visible);
        }
    }
}

/// Head slice within budget + one continuation ref to the terminal payload blob.
/// Never points at an intermediate envelope (tokenzero-result-cap-autopage-be8).
fn autopage_over_cap(
    value: &Value,
    bytes: usize,
    max_output_bytes: usize,
    continuation_ref: Option<&str>,
) -> Value {
    let Some(continuation_ref) = continuation_ref.filter(|r| {
        r.starts_with("tz://") && !r.contains("/envelope") && !r.contains("envelope.v")
    }) else {
        return json!({
            "truncated": true,
            "message": "visible result exceeded CodeMode max_output_bytes; expand result ref from execution_refs.stored.result",
            "bytes": bytes,
        });
    };

    let head_seed = match value {
        Value::String(text) => Value::String(text.clone()),
        other => Value::String(serde_json::to_string(other).unwrap_or_default()),
    };
    let mut head = head_seed;
    loop {
        let candidate = json!({
            "head": head,
            "continuation_ref": continuation_ref,
            "truncated": true,
            "bytes": bytes,
        });
        let fits = serde_json::to_vec(&candidate)
            .map(|serialized| serialized.len() <= max_output_bytes)
            .unwrap_or(false);
        if fits {
            return candidate;
        }
        match &head {
            Value::String(text) if !text.is_empty() => {
                let keep = text.chars().count().saturating_mul(3) / 4;
                if keep == 0 {
                    head = Value::String(String::new());
                } else {
                    head = Value::String(text.chars().take(keep).collect());
                }
            }
            _ => {
                // Minimal page: continuation only (tiny budgets like test max=8).
                return json!({
                    "continuation_ref": continuation_ref,
                    "truncated": true,
                    "bytes": bytes,
                });
            }
        }
    }
}

const TRUNCATION_NOTE: &str = "\n[tokenzero expand truncated: output exceeded CodeMode max_output_bytes; rerun expand with start_line/end_line windowing opts]\n";

fn truncate_string_to_fit(mut text: String, mut fits: impl FnMut(&str) -> bool) -> String {
    loop {
        let candidate = format!("{text}{TRUNCATION_NOTE}");
        if text.is_empty() || fits(&candidate) {
            return candidate;
        }
        let keep_chars = text.chars().count().saturating_mul(3) / 4;
        text = text.chars().take(keep_chars).collect();
    }
}

fn record_exact_text(value: &Value) {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        super::exec::record_exact_expand_payload(text);
    }
}

fn cap_exact_expand_value(value: &mut Value, max_output_bytes: usize) -> bool {
    match value {
        Value::Object(map)
            if map
                .get("__tz_exact_expand")
                .and_then(Value::as_bool)
                .unwrap_or(false) =>
        {
            let Some(kept) = map.get("text").and_then(Value::as_str).map(str::to_string) else {
                return false;
            };
            if serde_json::to_vec(&*map)
                .map(|bytes| bytes.len() <= max_output_bytes)
                .unwrap_or(true)
            {
                return false;
            }
            let fitted = truncate_string_to_fit(kept, |candidate| {
                map.insert("text".into(), Value::String(candidate.to_string()));
                serde_json::to_vec(&*map).is_ok_and(|bytes| bytes.len() <= max_output_bytes)
            });
            map.insert("text".into(), Value::String(fitted));
            record_exact_text(value);
            true
        }
        Value::Object(map) => map.values_mut().fold(false, |changed, item| {
            cap_exact_expand_value(item, max_output_bytes) | changed
        }),
        Value::Array(items) => items.iter_mut().fold(false, |changed, item| {
            cap_exact_expand_value(item, max_output_bytes) | changed
        }),
        _ => false,
    }
}

fn strip_exact_expand_markers(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("__tz_exact_expand");
            map.values_mut().for_each(strip_exact_expand_markers);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_exact_expand_markers),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completed(value: Value) -> CodeModeResult {
        CodeModeResult::completed(value, Vec::new(), 0, 1, 1)
    }

    #[test]
    fn visible_token_limit_is_backward_compatible_and_serialized() {
        let legacy: CodeModeLimits = serde_json::from_value(json!({
            "max_output_bytes": 1024
        }))
        .unwrap();
        assert_eq!(legacy.max_visible_tokens, default_max_visible_tokens());

        let explicit: CodeModeLimits = serde_json::from_value(json!({
            "max_visible_tokens": 511
        }))
        .unwrap();
        assert_eq!(explicit.max_visible_tokens, 511);
        assert_eq!(explicit.as_json()["max_visible_tokens"], 511);
    }

    #[test]
    fn finalization_commits_one_batch_and_recovers_every_logical_ref() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache.json");
        COMMIT_CALLS.with(|calls| calls.set(0));
        let finalized = finalize_result(
            completed(json!({"ok": true})),
            "code",
            "return { ok: true }",
            100,
            101,
            ExecutionStore::new(cache.clone()),
            &CodeModeLimits::default(),
            Vec::new(),
        );

        assert_eq!(finalized.status, CodeModeStatus::Completed);
        assert_eq!(finalized.visible_ack, "0");
        COMMIT_CALLS.with(|calls| assert_eq!(calls.get(), 1));

        let id = finalized.execution_id.as_deref().unwrap();
        let mut restarted = RecoveryStore::new(Some(cache));
        for suffix in ["", "code", "steps", "telemetry", "result", "envelope"] {
            let logical = execution_ref(id, suffix);
            let expanded = restarted.expand(&logical, Some("raw"), None, None, None, None);
            assert!(expanded.found, "{logical}: {}", expanded.reason);
        }
    }

    #[test]
    fn ack2_pure_mutation_success_is_silent_and_has_detail_ref() {
        let dir = tempfile::tempdir().unwrap();
        let finalized = finalize_result(
            completed(json!({"hunks_applied": 1})),
            "code",
            "return zero.edit('x', [])",
            150,
            151,
            ExecutionStore::new(dir.path().join("cache.json")),
            &CodeModeLimits::default(),
            vec![ExecutionStep {
                id: "step-1".into(),
                method: "zero.edit".into(),
                status: "ok".into(),
                refs: Vec::new(),
            }],
        );
        assert_eq!(finalized.visible_ack, "");
        assert!(finalized.detail_ref.as_deref().is_some_and(|value| value.ends_with("/result")));
    }

    #[test]
    fn failed_batch_commit_never_returns_a_completed_ack() {
        let dir = tempfile::tempdir().unwrap();
        let blocked_parent = dir.path().join("not-a-directory");
        std::fs::write(&blocked_parent, "block").unwrap();
        let finalized = finalize_result(
            completed(json!({"ok": true})),
            "code",
            "return true",
            200,
            201,
            ExecutionStore::new(blocked_parent.join("cache.json")),
            &CodeModeLimits::default(),
            Vec::new(),
        );

        assert_eq!(finalized.status, CodeModeStatus::Error);
        assert_ne!(finalized.visible_ack, "C");
        assert!(finalized.execution_refs.is_none());
        let error = finalized.error.as_ref().unwrap();
        assert_eq!(error.kind, "store");
        assert!(error.message.contains("execution record commit failed"));
    }

    #[test]
    fn concurrent_finalizers_publish_isolated_replayable_batches() {
        let dir = tempfile::tempdir().unwrap();
        let cache = std::sync::Arc::new(dir.path().join("cache.json"));
        let workers = [
            ("return 'alpha'", 300_u128, "alpha"),
            ("return 'beta'", 301_u128, "beta"),
        ]
        .into_iter()
        .map(|(plan, started, value)| {
            let cache = std::sync::Arc::clone(&cache);
            std::thread::spawn(move || {
                finalize_result(
                    completed(json!(value)),
                    "code",
                    plan,
                    started,
                    started + 1,
                    ExecutionStore::new((*cache).clone()),
                    &CodeModeLimits::default(),
                    Vec::new(),
                )
            })
        })
        .collect::<Vec<_>>();
        let finalized = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();

        let mut restarted = RecoveryStore::new(Some((*cache).clone()));
        for result in finalized {
            assert_eq!(result.status, CodeModeStatus::Completed);
            let id = result.execution_id.as_deref().unwrap();
            for suffix in ["code", "result", "envelope"] {
                let logical = execution_ref(id, suffix);
                let expanded = restarted.expand(&logical, Some("raw"), None, None, None, None);
                assert!(expanded.found, "{logical}: {}", expanded.reason);
            }
        }
    }
}
