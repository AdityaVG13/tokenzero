//! Per-tool call observability for the MCP server.
//!
//! Tracks call counts, error counts, slow-call counts, and latency per
//! canonical tool. In-process counters cover the current session; a small
//! JSON sidecar next to the recovery cache accumulates the same counters
//! across sessions (each call merges its own one-call delta, so concurrent
//! processes accumulate rather than clobber — atomic rename prevents partial
//! writes; a lost increment under a rare read-modify-write race is acceptable
//! for approximate telemetry). Exposed via `resource://tokenzero/metrics`.
//!
//! All recording is fail-open: a poisoned lock or unwritable sidecar never
//! propagates an error into a tool call.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};
use tokenzero_core::MCP_SCHEMA_VERSION;

/// Default latency above which a call is flagged "slow". Override with
/// `TOKENZERO_SLOW_TOOL_MS`.
const DEFAULT_SLOW_TOOL_MS: u64 = 2000;

#[derive(Debug, Clone, Default)]
struct ToolStat {
    calls: u64,
    errors: u64,
    slow_calls: u64,
    total_ms: u64,
    max_ms: u64,
}

impl ToolStat {
    fn record(&mut self, ms: u64, is_error: bool, slow: bool) {
        self.calls += 1;
        self.total_ms += ms;
        self.max_ms = self.max_ms.max(ms);
        if is_error {
            self.errors += 1;
        }
        if slow {
            self.slow_calls += 1;
        }
    }

    fn from_json(value: &Value) -> Self {
        let u = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
        Self {
            calls: u("calls"),
            errors: u("errors"),
            slow_calls: u("slow_calls"),
            total_ms: u("total_ms"),
            max_ms: u("max_ms"),
        }
    }

    fn to_json(&self) -> Value {
        let avg_ms = self.total_ms.checked_div(self.calls).unwrap_or(0);
        json!({
            "calls": self.calls,
            "errors": self.errors,
            "slow_calls": self.slow_calls,
            "total_ms": self.total_ms,
            "max_ms": self.max_ms,
            "avg_ms": avg_ms,
        })
    }
}

#[derive(Debug)]
pub(crate) struct ToolMetrics {
    /// Sidecar JSON path, derived from the recovery-cache path.
    path: PathBuf,
    slow_ms: u64,
    /// This process's counters; resets when the server exits.
    session: Mutex<BTreeMap<String, ToolStat>>,
}

impl ToolMetrics {
    pub(crate) fn new(cache_path: &Path) -> Self {
        let path = cache_path.with_file_name("tool-metrics.json");
        let slow_ms = std::env::var("TOKENZERO_SLOW_TOOL_MS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|ms| *ms > 0)
            .unwrap_or(DEFAULT_SLOW_TOOL_MS);
        Self {
            path,
            slow_ms,
            session: Mutex::new(BTreeMap::new()),
        }
    }

    /// Record one tool call. Never errors (fail-open).
    pub(crate) fn record(&self, tool: &str, elapsed: Duration, is_error: bool) {
        let ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        let slow = ms >= self.slow_ms;

        if let Ok(mut session) = self.session.lock() {
            session
                .entry(tool.to_string())
                .or_default()
                .record(ms, is_error, slow);
        }

        // Merge this single call into the persistent sidecar.
        let mut persisted = self.load_persisted();
        persisted
            .entry(tool.to_string())
            .or_default()
            .record(ms, is_error, slow);
        let _ = self.write_persisted(&persisted);
    }

    /// Snapshot for `resource://tokenzero/metrics`.
    pub(crate) fn snapshot(&self) -> Value {
        let cumulative = Self::map_to_json(&self.load_persisted());
        let session = match self.session.lock() {
            Ok(session) => Self::map_to_json(&session),
            Err(_) => json!({}),
        };
        json!({
            "schema_version": MCP_SCHEMA_VERSION,
            "status": "ok",
            "slow_threshold_ms": self.slow_ms,
            "persistent_path": self.path.display().to_string(),
            "cumulative": cumulative,
            "session": session,
            "next_actions": [
                "cumulative counts persist across sessions in the sidecar next to the recovery cache; session counts reset when the server process exits.",
                "Set TOKENZERO_SLOW_TOOL_MS to change the slow-call threshold."
            ]
        })
    }

    fn map_to_json(stats: &BTreeMap<String, ToolStat>) -> Value {
        let tools: serde_json::Map<String, Value> = stats
            .iter()
            .map(|(name, stat)| (name.clone(), stat.to_json()))
            .collect();
        let totals = stats.values().fold(ToolStat::default(), |mut acc, stat| {
            acc.calls += stat.calls;
            acc.errors += stat.errors;
            acc.slow_calls += stat.slow_calls;
            acc.total_ms += stat.total_ms;
            acc.max_ms = acc.max_ms.max(stat.max_ms);
            acc
        });
        json!({ "tools": Value::Object(tools), "totals": totals.to_json() })
    }

    fn load_persisted(&self) -> BTreeMap<String, ToolStat> {
        let mut out = BTreeMap::new();
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return out;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return out; // corrupt sidecar: start fresh rather than fail
        };
        if let Some(tools) = value.get("tools").and_then(Value::as_object) {
            for (name, stat) in tools {
                out.insert(name.clone(), ToolStat::from_json(stat));
            }
        }
        out
    }

    fn write_persisted(&self, stats: &BTreeMap<String, ToolStat>) -> std::io::Result<()> {
        let payload = json!({
            "schema": 1,
            "slow_threshold_ms": self.slow_ms,
            "tools": stats
                .iter()
                .map(|(name, stat)| (name.clone(), stat.to_json()))
                .collect::<serde_json::Map<String, Value>>(),
        });
        let body = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string());
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Atomic-ish write: temp file in the same dir, then rename.
        let tmp = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        std::fs::write(&tmp, body.as_bytes())?;
        std::fs::rename(&tmp, &self.path)
    }
}
