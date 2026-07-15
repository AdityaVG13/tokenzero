//! Durable CodeMode execution records, limits, refs, and response guards.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use tokenzero_core::{ContentType, count_tokens};
use tokenzero_recovery::RecoveryStore;

use super::result::{CodeModeResult, CodeModeStatus, CodeModeTelemetry};

pub const CODEMODE_LIMITS_SCHEMA: &str = "tokenzero.codemode.limits.v1";
pub const DEFAULT_MAX_LOGICAL_OPS: usize = 1000;
pub const DEFAULT_MAX_PHYSICAL_OPS: usize = 256;
pub const DEFAULT_MAX_WALL_MS: u64 = 5000;
pub const HARD_MAX_WALL_MS: u64 = 5000;
pub const DEFAULT_MAX_MICROTASKS: usize = 4096;
pub const DEFAULT_MAX_MEMORY_BYTES: usize = 32 * 1024 * 1024;
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;
pub const DEFAULT_MAX_RESULT_REF_BYTES: usize = 10 * 1024 * 1024;
pub const DEFAULT_MAX_REFS_EMITTED: usize = 256;
pub const DEFAULT_MAX_PARALLEL_WIDTH: usize = 16;
pub const DEFAULT_MAX_CODE_BYTES: usize = 64 * 1024;

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
}

impl Default for CodeModeLimits {
    fn default() -> Self {
        Self {
            max_logical_ops: DEFAULT_MAX_LOGICAL_OPS,
            max_physical_ops: DEFAULT_MAX_PHYSICAL_OPS,
            max_wall_ms: DEFAULT_MAX_WALL_MS,
            hard_max_wall_ms: HARD_MAX_WALL_MS,
            max_microtasks: DEFAULT_MAX_MICROTASKS,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            max_result_ref_bytes: DEFAULT_MAX_RESULT_REF_BYTES,
            max_refs_emitted: DEFAULT_MAX_REFS_EMITTED,
            max_parallel_width: DEFAULT_MAX_PARALLEL_WIDTH,
            max_code_bytes: DEFAULT_MAX_CODE_BYTES,
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

pub struct ExecutionStore {
    store: RecoveryStore,
}

impl ExecutionStore {
    pub fn new(cache_path: std::path::PathBuf) -> Self {
        Self {
            store: RecoveryStore::new(Some(cache_path)),
        }
    }
    pub fn store_json(&mut self, value: &Value) -> Result<String, String> {
        self.store_text(
            &serde_json::to_string_pretty(value).map_err(|error| error.to_string())?,
            ContentType::JsonConfig,
        )
    }
    pub fn store_text(&mut self, text: &str, content_type: ContentType) -> Result<String, String> {
        self.store
            .store_payload(text, content_type, None, None, None)
            .map(|stored| stored.blob_ref.as_str().to_string())
            .map_err(|error| error.to_string())
    }
    pub fn alias(&mut self, logical_ref: &str, target_ref: &str) -> Result<(), String> {
        self.store
            .store_alias(logical_ref, target_ref)
            .map_err(|error| error.to_string())
    }
}

fn stored(result: Result<String, String>) -> String {
    result.unwrap_or_else(|error| format!("store-error:{error}"))
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
    let (status_str, ack, telemetry_status) = if completed {
        ("completed", "C", "ok")
    } else {
        ("error", "X0", "error")
    };
    result.execution_id = Some(id.clone());
    result.visible_ack = ack.into();
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

    let code_ref = stored(store.store_text(plan, ContentType::Code));
    let steps_ref = stored(store.store_json(&json!(steps)));
    let telemetry_ref = stored(store.store_json(&json!(result.telemetry)));

    let result_ref = result.value.as_ref().and_then(|value| {
        serde_json::to_vec(value)
            .ok()
            .filter(|bytes| bytes.len() <= limits.max_result_ref_bytes)
            .and_then(|_| store.store_json(value).ok())
    });
    let error_ref = result
        .error
        .as_ref()
        .and_then(|error| store.store_json(&json!(error)).ok());

    let logical_ref = |suffix| execution_ref(&id, suffix);
    let execution_logical_ref = logical_ref("");
    let code_logical_ref = logical_ref("code");
    let steps_logical_ref = logical_ref("steps");
    let telemetry_logical_ref = logical_ref("telemetry");
    let result_logical_ref = logical_ref("result");
    let error_logical_ref = logical_ref("error");

    let mut logical_refs = json!({
        "execution": execution_logical_ref,
        "code": code_logical_ref,
        "steps": steps_logical_ref,
        "telemetry": telemetry_logical_ref,
        "result": result_logical_ref,
        "error": error_logical_ref,
        "stored": {
            "code": code_ref,
            "steps": steps_ref,
            "telemetry": telemetry_ref,
            "result": result_ref,
            "error": error_ref,
        }
    });
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
    let execution_record_ref = stored(store.store_json(&record_value));

    let envelope_logical_ref = logical_ref("envelope");
    let envelope_bundle = json!({
        "schema": "tokenzero.codemode.envelope.v2",
        "execution_id": id.clone(),
        "status": status_str,
        "ack": result.visible_ack.clone(),
        "telemetry": result.telemetry.clone(),
        "refs": result.refs.clone(),
        "execution_refs": logical_refs.clone(),
        "store": {
            "code_ref": code_ref.clone(),
            "steps_ref": steps_ref.clone(),
            "telemetry_ref": telemetry_ref.clone(),
            "result_ref": result_ref.clone(),
            "error_ref": error_ref.clone(),
            "execution_record_ref": execution_record_ref.clone(),
        }
    });
    let envelope_ref = stored(store.store_json(&envelope_bundle));
    if let Some(obj) = logical_refs.as_object_mut() {
        obj.insert("envelope".to_string(), json!(envelope_logical_ref.clone()));
        if let Some(stored) = obj.get_mut("stored").and_then(Value::as_object_mut) {
            stored.insert("envelope".to_string(), json!(envelope_ref.clone()));
        }
    }

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
            let _ = store.alias(logical, stored);
        }
    }

    if result.refs.len() < limits.max_refs_emitted {
        result.refs.push(execution_record_ref);
    }
    result.execution_refs = Some(logical_refs);
    guard_visible_output(&mut result, limits);
    result
}

fn record_visible_tokens(telemetry: &mut CodeModeTelemetry, visible: usize) {
    telemetry.visible_tokens = visible;
    if let Some(extra) = telemetry.extra.as_mut().and_then(Value::as_object_mut) {
        extra.insert("visible_tokens".to_string(), json!(visible));
    }
}

fn guard_visible_output(result: &mut CodeModeResult, limits: &CodeModeLimits) {
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
            result.value = Some(json!({
                "truncated": true,
                "message": "visible result exceeded CodeMode max_output_bytes; expand result ref from execution_refs.stored.result",
                "bytes": bytes,
            }));
            record_visible_tokens(&mut result.telemetry, count_tokens("C"));
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
