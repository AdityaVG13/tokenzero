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
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodeModeStatus {
    Completed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeModeTelemetry {
    pub operations: usize,
    pub visible_tokens: usize,
    pub raw_tokens: usize,
    /// How many individual MCP tool calls this plan replaced (ops + plan overhead).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equivalent_calls: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_ack: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps_run: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_ops: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_ops: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batched_ops: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_hits: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_misses: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_writes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_actions: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_groups: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refs_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_materialized: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_leak: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wall_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u128>,
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
    pub max_code_bytes: usize,
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
            max_code_bytes: super::store::DEFAULT_MAX_CODE_BYTES,
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
                equivalent_calls: Some(ops.saturating_add(1)),
                execution_id: None,
                kind: None,
                status: None,
                visible_ack: None,
                steps_run: None,
                logical_ops: Some(ops),
                physical_ops: Some(ops),
                batched_ops: Some(0),
                cache_hits: Some(0),
                cache_misses: Some(ops),
                store_writes: Some(refs_len),
                internal_actions: Some(ops),
                parallel_groups: Some(0),
                refs_count: Some(refs_len),
                bytes_materialized: Some(raw),
                raw_leak: Some(false),
                wall_ms: None,
                started_at_ms: None,
                finished_at_ms: None,
            },
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>, ops: usize) -> Self {
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
                equivalent_calls: None,
                execution_id: None,
                kind: None,
                status: None,
                visible_ack: None,
                steps_run: None,
                logical_ops: Some(ops),
                physical_ops: Some(ops),
                batched_ops: Some(0),
                cache_hits: Some(0),
                cache_misses: Some(ops),
                store_writes: Some(0),
                internal_actions: Some(ops),
                parallel_groups: Some(0),
                refs_count: Some(0),
                bytes_materialized: Some(0),
                raw_leak: Some(false),
                wall_ms: None,
                started_at_ms: None,
                finished_at_ms: None,
            },
            error: Some(msg.into()),
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
                    self.telemetry.operations,
                    self.telemetry.visible_tokens,
                    self.telemetry.raw_tokens,
                    refs_part,
                )
            }
            CodeModeStatus::Error => {
                format!(
                    "codemode:error X0 ops={} {}",
                    self.telemetry.operations,
                    self.error.as_deref().unwrap_or("unknown"),
                )
            }
        }
    }
}
