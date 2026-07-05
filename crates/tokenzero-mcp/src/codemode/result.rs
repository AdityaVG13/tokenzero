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
    pub store_writes: usize,
    pub wall_ms: u64,
    pub bytes_materialized: usize,
    pub envelope_tokens: usize,
    pub payload_tokens: usize,
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
    pub envelope: Option<String>,
    pub ref_first: bool,
    pub ref_first_budget: usize,
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
            envelope: None,
            ref_first: true,
            ref_first_budget: 64,
        }
    }
}

impl CodeModeResult {
    pub fn completed(
        value: Value,
        refs: Vec<String>,
        ops: usize,
        visible: usize,
        raw: usize,
    ) -> Self {
        let refs_len = refs.len();
        Self {
            schema: CODEMODE_SCHEMA,
            status: CodeModeStatus::Completed,
            visible_ack: "C".to_string(),
            execution_id: None,
            value: Some(value),
            refs,
            execution_refs: None,
            telemetry: CodeModeTelemetry {
                operations: ops,
                visible_tokens: visible,
                raw_tokens: raw,
                steps_run: None,
                parallel_groups: Some(0),
                refs_count: Some(refs_len),
                equivalent_calls: Some(ops.saturating_add(1)),
                kind: "codemode.execute".to_string(),
                status: "ok".to_string(),
                logical_ops: ops,
                physical_ops: ops,
                batched_ops: 0,
                internal_actions: ops,
                cache_hits: 0,
                cache_misses: ops,
                store_writes: refs_len,
                wall_ms: 0,
                bytes_materialized: raw,
                envelope_tokens: 0,
                payload_tokens: visible,
                extra: Some(serde_json::json!({
                    "operations": ops,
                    "visible_tokens": visible,
                    "raw_tokens": raw,
                    "equivalent_calls": ops.saturating_add(1),
                    "refs_count": refs_len,
                    "parallel_groups": 0,
                    "envelope_tokens": 0,
                    "payload_tokens": visible
                })),
            },
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>, ops: usize) -> Self {
        let message = msg.into();
        let kind = classify_error_kind(&message);
        Self::error_with_kind(kind, message, ops, false)
    }

    pub fn error_with_kind(
        kind: impl Into<String>,
        msg: impl Into<String>,
        ops: usize,
        retryable: bool,
    ) -> Self {
        Self {
            schema: CODEMODE_SCHEMA,
            status: CodeModeStatus::Error,
            visible_ack: "X0".to_string(),
            execution_id: None,
            value: None,
            refs: Vec::new(),
            execution_refs: None,
            telemetry: CodeModeTelemetry {
                operations: ops,
                visible_tokens: 0,
                raw_tokens: 0,
                steps_run: None,
                parallel_groups: Some(0),
                refs_count: Some(0),
                equivalent_calls: None,
                kind: "codemode.execute".to_string(),
                status: "error".to_string(),
                logical_ops: ops,
                physical_ops: ops,
                batched_ops: 0,
                internal_actions: ops,
                cache_hits: 0,
                cache_misses: ops,
                store_writes: 0,
                wall_ms: 0,
                bytes_materialized: 0,
                envelope_tokens: 0,
                payload_tokens: 0,
                extra: Some(serde_json::json!({
                    "operations": ops,
                    "visible_tokens": 0,
                    "raw_tokens": 0,
                    "refs_count": 0,
                    "parallel_groups": 0,
                    "envelope_tokens": 0,
                    "payload_tokens": 0
                })),
            },
            error: Some(CodeModeError::new(kind, msg, retryable)),
        }
    }

    pub fn to_line(&self) -> String {
        match self.status {
            CodeModeStatus::Completed => {
                let refs_part = if self.refs.is_empty() {
                    String::new()
                } else {
                    format!(" refs={}", self.refs.join(","))
                };
                format!(
                    "codemode:ok C ops={} visible_tokens={} raw_tokens={}{}",
                    self.telemetry.operations(),
                    self.telemetry.visible_tokens(),
                    self.telemetry.raw_tokens(),
                    refs_part,
                )
            }
            CodeModeStatus::Error => {
                format!(
                    "codemode:error X0 ops={} {}",
                    self.telemetry.operations(),
                    self.error
                        .as_ref()
                        .map(|error| error.message.as_str())
                        .unwrap_or("unknown"),
                )
            }
        }
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
