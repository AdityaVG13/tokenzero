//! CodeMode result envelope and options.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub const CODEMODE_SCHEMA: &str = "tokenzero.codemode.v1";

// ─── Result types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeModeResult {
    pub schema: &'static str,
    pub status: CodeModeStatus,
    pub visible_ack: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_refs: Option<Value>,
    pub telemetry: CodeModeTelemetry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CodeModeError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodeModeStatus {
    Completed,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeModeError {
    pub kind: String,
    pub message: String,
    pub retryable: bool,
}

impl CodeModeError {
    pub fn new(kind: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            retryable,
        }
    }

    pub fn contains(&self, needle: &str) -> bool {
        self.message.contains(needle)
    }
}

impl std::ops::Deref for CodeModeError {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.message
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeModeTelemetry {
    #[serde(skip)]
    pub operations: usize,
    #[serde(skip)]
    pub visible_tokens: usize,
    #[serde(skip)]
    pub raw_tokens: usize,
    #[serde(skip)]
    pub steps_run: Option<usize>,
    #[serde(skip)]
    pub parallel_groups: Option<usize>,
    #[serde(skip)]
    pub refs_count: Option<usize>,
    #[serde(skip)]
    pub equivalent_calls: Option<usize>,
    pub kind: String,
    pub status: String,
    pub logical_ops: usize,
    pub physical_ops: usize,
    pub batched_ops: usize,
    pub internal_actions: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    /// Per-session prefix cache hit count (provider cached_tokens when available;
    /// otherwise byte-prefix estimate). Serde default keeps old telemetry readable.
    #[serde(default)]
    pub prefix_cache_hits: usize,
    /// Per-session prefix cache denominator for the hit-rate metric.
    #[serde(default)]
    pub prefix_cache_total: usize,
    pub store_writes: usize,
    pub wall_ms: u64,
    pub bytes_materialized: usize,
    pub envelope_tokens: usize,
    pub payload_tokens: usize,
    /// Token attribution buckets for envelope overhead audit (6ot).
    pub ack_tokens: usize,
    pub ref_string_tokens: usize,
    pub framing_tokens: usize,
    pub preview_tokens: usize,
    /// Counterfactual prevented-read bytes: bytes that would have been read
    /// if graph queries, search hits, or ref expansion had not satisfied the
    /// request without a full file read. Measured as a lower-bound estimate
    /// from available accounting (raw vs. visible tokens, plus exact expand
    /// payload bytes); see exec.rs for the counterfactual methodology.
    pub prevented_read_bytes: usize,
    /// Count of expand calls that returned a capsule instead of the full body (wqw.13).
    #[serde(default)]
    pub prevented_full_body_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Value>,
}

impl CodeModeTelemetry {
    pub fn operations(&self) -> usize {
        self.operations
    }

    pub fn visible_tokens(&self) -> usize {
        self.visible_tokens
    }

    pub fn raw_tokens(&self) -> usize {
        self.raw_tokens
    }
}

const DEFAULT_REF_FIRST_BUDGET: usize = 256;

#[derive(Debug, Clone)]
pub struct CodeModeOptions {
    pub root: Option<PathBuf>,
    pub allowed_roots: Vec<PathBuf>,
    pub cache_path: Option<PathBuf>,
    pub max_visible_tokens: usize,
    pub timeout_seconds: Option<u64>,
    pub max_output_bytes: usize,
    pub max_refs_emitted: usize,
    pub max_logical_ops: usize,
    pub max_physical_ops: usize,
    pub max_microtasks: usize,
    pub max_memory_bytes: usize,
    pub max_code_bytes: usize,
    /// Soft plan wall clock (ms). Defaults match product CodeModeLimits.
    pub max_wall_ms: u64,
    /// Hard plan wall clock (ms); plans abort past this even if soft is higher.
    pub hard_max_wall_ms: u64,
    /// Bounded in-plan Promise.all / fan-out width for QuickJS host ops.
    pub max_parallel_width: usize,
    pub envelope: Option<String>,
    pub ref_first: bool,
    pub ref_first_budget: usize,
    /// Session crash-only health shared with the MCP engine (wqw.9).
    /// When set, plan expand/read outcomes update the same gate as tools/call.
    pub surface_health: Option<std::sync::Arc<crate::surface_health::SurfaceHealth>>,
    /// Programmatic shareable usage-telemetry choice; `None` defers to env.
    pub telemetry_enabled: Option<bool>,
}

impl Default for CodeModeOptions {
    fn default() -> Self {
        Self {
            root: None,
            allowed_roots: Vec::new(),
            cache_path: None,
            max_visible_tokens: 4000,
            timeout_seconds: None,
            max_output_bytes: super::store::DEFAULT_MAX_OUTPUT_BYTES,
            max_refs_emitted: super::store::DEFAULT_MAX_REFS_EMITTED,
            max_logical_ops: super::store::DEFAULT_MAX_LOGICAL_OPS,
            max_physical_ops: super::store::DEFAULT_MAX_PHYSICAL_OPS,
            max_microtasks: super::store::DEFAULT_MAX_MICROTASKS,
            max_memory_bytes: super::store::DEFAULT_MAX_MEMORY_BYTES,
            max_code_bytes: super::store::DEFAULT_MAX_CODE_BYTES,
            max_wall_ms: super::store::hard_max_wall_ms(),
            hard_max_wall_ms: super::store::hard_max_wall_ms(),
            max_parallel_width: super::store::DEFAULT_MAX_PARALLEL_WIDTH,
            envelope: None,
            ref_first: true,
            ref_first_budget: DEFAULT_REF_FIRST_BUDGET,
            surface_health: None,
            telemetry_enabled: None,
        }
    }
}

fn telemetry(ops: usize, visible: usize, raw: usize, refs: usize, ok: bool) -> CodeModeTelemetry {
    let mut extra = serde_json::json!({
        "operations": ops, "visible_tokens": visible, "raw_tokens": raw,
        "refs_count": refs, "parallel_groups": 0, "envelope_tokens": 0,
        "payload_tokens": visible, "prevented_read_bytes": 0
    });
    if ok {
        extra["equivalent_calls"] = serde_json::json!(ops.saturating_add(1));
    }
    CodeModeTelemetry {
        operations: ops,
        visible_tokens: visible,
        raw_tokens: raw,
        steps_run: None,
        parallel_groups: Some(0),
        refs_count: Some(refs),
        equivalent_calls: ok.then(|| ops.saturating_add(1)),
        kind: "codemode.execute".into(),
        status: if ok { "ok" } else { "error" }.into(),
        logical_ops: ops,
        physical_ops: ops,
        batched_ops: 0,
        internal_actions: ops,
        cache_hits: 0,
        cache_misses: ops,
        prefix_cache_hits: 0,
        prefix_cache_total: 0,
        store_writes: if ok { refs } else { 0 },
        wall_ms: 0,
        bytes_materialized: raw,
        envelope_tokens: 0,
        payload_tokens: visible,
        ack_tokens: 0,
        ref_string_tokens: 0,
        framing_tokens: 0,
        preview_tokens: 0,
        prevented_read_bytes: 0,
        prevented_full_body_count: 0,
        extra: Some(extra),
    }
}

impl CodeModeResult {
    fn new(
        value: Option<Value>,
        refs: Vec<String>,
        telemetry: CodeModeTelemetry,
        error: Option<CodeModeError>,
    ) -> Self {
        let ok = error.is_none();
        Self {
            schema: CODEMODE_SCHEMA,
            status: if ok {
                CodeModeStatus::Completed
            } else {
                CodeModeStatus::Error
            },
            visible_ack: if ok { "C" } else { "X0" }.into(),
            execution_id: None,
            value,
            refs,
            execution_refs: None,
            telemetry,
            error,
        }
    }

    pub fn completed(
        value: Value,
        refs: Vec<String>,
        ops: usize,
        visible: usize,
        raw: usize,
    ) -> Self {
        let info = telemetry(ops, visible, raw, refs.len(), true);
        Self::new(Some(value), refs, info, None)
    }

    pub fn error(msg: impl Into<String>, ops: usize) -> Self {
        let message = msg.into();
        Self::error_with_kind(classify_error_kind(&message), message, ops, false)
    }

    pub fn error_with_kind(
        kind: impl Into<String>,
        msg: impl Into<String>,
        ops: usize,
        retryable: bool,
    ) -> Self {
        Self::new(
            None,
            Vec::new(),
            telemetry(ops, 0, 0, 0, false),
            Some(CodeModeError::new(kind, msg, retryable)),
        )
    }

    pub fn to_line(&self) -> String {
        match self.status {
            CodeModeStatus::Completed => {
                let refs = if !self.refs.is_empty() {
                    format!(" refs={}", self.refs.join(","))
                } else {
                    Default::default()
                };
                let mut line = format!(
                    "codemode:ok C ops={} visible_tokens={} raw_tokens={}{}",
                    self.telemetry.operations(),
                    self.telemetry.visible_tokens(),
                    self.telemetry.raw_tokens(),
                    refs
                );
                if let Some(warning) = self
                    .telemetry
                    .extra
                    .as_ref()
                    .and_then(|extra| extra.get("root_fallback_warning"))
                    .and_then(Value::as_str)
                {
                    line.push_str(&format!("\n# warning: root_fallback: {warning}"));
                }
                line
            }
            CodeModeStatus::Error => format!(
                "codemode:error X0 ops={} {}",
                self.telemetry.operations(),
                self.error
                    .as_ref()
                    .map(|error| structured_error_message(&error.message))
                    .unwrap_or_else(|| "unknown".to_string())
            ),
        }
    }
}

fn structured_error_message(message: &str) -> String {
    let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(message) else {
        return message.replace(['\n', '\r'], " ");
    };
    let Some(error) = fields.get("error") else {
        return message.replace(['\n', '\r'], " ");
    };
    let render = |value: &Value| match value {
        Value::String(text) => text.replace(['\n', '\r'], " "),
        Value::Array(values) => values
            .iter()
            .map(|item| {
                item.as_str()
                    .map_or_else(|| item.to_string(), str::to_string)
            })
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    };
    let mut rendered = render(error);
    if let Some(hint) = fields.get("hint") {
        rendered.push_str("; hint: ");
        rendered.push_str(&render(hint));
    }
    for (name, value) in fields {
        if name == "error" || name == "hint" {
            continue;
        }
        rendered.push_str("; ");
        rendered.push_str(&name);
        rendered.push_str(": ");
        rendered.push_str(&render(&value));
    }
    rendered
}

#[cfg(test)]
mod structured_error_tests {
    use super::*;

    #[test]
    fn structured_json_error_keeps_punctuation_and_lists() {
        let result = CodeModeResult::error(
            r#"{"error":"unknown surface: framework","hint":"choose a supported surface","valid_surfaces":["authoring","constructors","ops"]}"#,
            0,
        );
        assert_eq!(
            result.to_line(),
            "codemode:error X0 ops=0 unknown surface: framework; hint: choose a supported surface; valid_surfaces: authoring, constructors, ops"
        );
    }
}

fn classify_error_kind(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("mutating binding denied")
        || lower.contains("mutation")
        || lower.contains("edit denied")
    {
        "policy"
    } else if lower.starts_with("sandbox:") || lower.contains("denied") || lower.contains("quickjs")
    {
        "sandbox"
    } else if lower.contains("parse error")
        || lower.contains("invalid json")
        || lower.contains("empty plan")
        || lower.contains("missing method")
        || lower.contains("requires a steps array")
        || lower.contains("missing") && lower.contains("argument")
    {
        "validation"
    } else if lower.contains("outside allowed roots")
        || lower.contains("not found")
        || lower.contains("no such")
        || lower.contains("missing target")
        || lower.contains("missing_target")
    {
        "substrate"
    } else {
        "runtime"
    }
}
