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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
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
}

#[derive(Debug, Clone)]
pub struct CodeModeOptions {
    pub root: Option<PathBuf>,
    pub allowed_roots: Vec<PathBuf>,
    pub cache_path: Option<PathBuf>,
    pub max_visible_tokens: usize,
    pub timeout_seconds: Option<u64>,
}

impl Default for CodeModeOptions {
    fn default() -> Self {
        Self {
            root: None,
            allowed_roots: Vec::new(),
            cache_path: None,
            max_visible_tokens: 4000,
            timeout_seconds: None,
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
        Self {
            schema: CODEMODE_SCHEMA,
            status: CodeModeStatus::Completed,
            value: Some(value),
            refs,
            telemetry: CodeModeTelemetry {
                operations: ops,
                visible_tokens: visible,
                raw_tokens: raw,
                equivalent_calls: Some(ops.saturating_add(1)),
            },
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>, ops: usize) -> Self {
        Self {
            schema: CODEMODE_SCHEMA,
            status: CodeModeStatus::Error,
            value: None,
            refs: Vec::new(),
            telemetry: CodeModeTelemetry {
                operations: ops,
                visible_tokens: 0,
                raw_tokens: 0,
                equivalent_calls: None,
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
                    "codemode:ok ops={} visible_tokens={} raw_tokens={}{}",
                    self.telemetry.operations,
                    self.telemetry.visible_tokens,
                    self.telemetry.raw_tokens,
                    refs_part,
                )
            }
            CodeModeStatus::Error => {
                format!(
                    "codemode:error ops={} {}",
                    self.telemetry.operations,
                    self.error.as_deref().unwrap_or("unknown"),
                )
            }
        }
    }
}
