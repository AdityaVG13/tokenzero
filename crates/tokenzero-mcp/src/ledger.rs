//! Queryable, append-only accounting for served TokenZero responses.
//!
//! Every JSONL line is a tokenzero.ledger.v1 LedgerRecord. prevented_tokens is
//! derived only from existing per-response dedup.visible_tokens_saved and
//! diff.visible_tokens_saved telemetry. It is not a prevented-read estimate.
//! saved_bytes separately preserves session_delta.saved_bytes.
//!
//! Writes use O_APPEND without per-turn fsync and fail open. Before a write
//! would exceed DEFAULT_MAX_LEDGER_BYTES, the active file rotates to .jsonl.1.
//! Queries scan both generations and ignore malformed lines, including a torn
//! final line.

use fs4::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tokenzero_core::ToolResponse;

pub const LEDGER_SCHEMA: &str = "tokenzero.ledger.v1";
pub const TOKENZERO_AGENT_ENV: &str = "TOKENZERO_AGENT";
pub const DEFAULT_MAX_LEDGER_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionIdentity {
    #[serde(rename = "crate")]
    pub crate_version: String,
    pub git_describe: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenMass {
    pub visible_tokens: u64,
    pub raw_tokens: u64,
    /// Existing dedup/diff token savings only; never a prevented-read estimate.
    pub prevented_tokens: u64,
    pub saved_bytes: u64,
}

/// One served response in the versioned tokenzero.ledger.v1 JSONL schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerRecord {
    pub schema: String,
    pub timestamp_ms: u64,
    pub session_id: String,
    pub repo: String,
    pub agent: Option<String>,
    pub version: VersionIdentity,
    pub tool: String,
    pub token_mass: TokenMass,
    pub cumulative_session_cost_tokens: u64,
    pub optimization_tags: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct LedgerWriter {
    path: PathBuf,
    session_id: String,
    repo: String,
    agent: Option<String>,
    version: VersionIdentity,
    optimization_tags: Vec<String>,
    max_bytes: u64,
    cumulative_visible_tokens: Mutex<u64>,
}

impl LedgerWriter {
    pub(crate) fn new(
        cache_path: &Path,
        session_id: String,
        repo: String,
        optimization_tags: Vec<String>,
    ) -> Self {
        Self::with_max_bytes(
            cache_path,
            session_id,
            repo,
            optimization_tags,
            DEFAULT_MAX_LEDGER_BYTES,
        )
    }

    fn with_max_bytes(
        cache_path: &Path,
        session_id: String,
        repo: String,
        optimization_tags: Vec<String>,
        max_bytes: u64,
    ) -> Self {
        Self {
            path: ledger_path_for_cache(cache_path),
            session_id,
            repo,
            agent: std::env::var(TOKENZERO_AGENT_ENV)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty()),
            version: VersionIdentity {
                crate_version: env!("CARGO_PKG_VERSION").to_string(),
                git_describe: None,
            },
            optimization_tags,
            max_bytes,
            cumulative_visible_tokens: Mutex::new(0),
        }
    }

    /// Snapshot existing response accounting and append one record. Fail-open.
    pub(crate) fn record_response(&self, tool: &str, response: &ToolResponse) {
        let Some(accounting) = response.accounting.as_ref() else {
            return;
        };
        let telemetry = response.telemetry.as_ref();
        let get = |pointer: &str| {
            telemetry
                .and_then(|value| value.pointer(pointer))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };
        let visible_tokens = u64::try_from(accounting.visible_tokens).unwrap_or(u64::MAX);
        let Ok(mut cumulative) = self.cumulative_visible_tokens.lock() else {
            return;
        };
        *cumulative = cumulative.saturating_add(visible_tokens);
        let record = LedgerRecord {
            schema: LEDGER_SCHEMA.to_string(),
            timestamp_ms: now_ms(),
            session_id: self.session_id.clone(),
            repo: self.repo.clone(),
            agent: self.agent.clone(),
            version: self.version.clone(),
            tool: tool.to_string(),
            token_mass: TokenMass {
                visible_tokens,
                raw_tokens: u64::try_from(accounting.raw_tokens).unwrap_or(u64::MAX),
                prevented_tokens: get("/dedup/visible_tokens_saved")
                    .saturating_add(get("/diff/visible_tokens_saved")),
                saved_bytes: get("/session_delta/saved_bytes"),
            },
            cumulative_session_cost_tokens: *cumulative,
            optimization_tags: self.optimization_tags.clone(),
        };
        let _ = append_record(&self.path, &record, self.max_bytes);
    }
}

pub fn ledger_path_for_cache(cache_path: &Path) -> PathBuf {
    cache_path.with_file_name("ledger.jsonl")
}

fn rotated_path(path: &Path) -> PathBuf {
    path.with_extension("jsonl.1")
}

fn append_record(path: &Path, record: &LedgerRecord, max_bytes: u64) -> io::Result<()> {
    let mut line = serde_json::to_vec(record).map_err(io::Error::other)?;
    line.push(b'\n');
    let line_bytes = u64::try_from(line.len()).unwrap_or(u64::MAX);
    if line_bytes > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ledger record exceeds rotation limit",
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let observed_len = fs::metadata(path).map(|metadata| metadata.len()).unwrap_or(0);
    if observed_len > 0 && observed_len.saturating_add(line_bytes) > max_bytes {
        let lock_path = PathBuf::from(format!("{}.rotation.lock", path.display()));
        let rotation_lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        FileExt::lock(&rotation_lock)?;
        let locked_len = fs::metadata(path).map(|metadata| metadata.len()).unwrap_or(0);
        if locked_len == observed_len
            && locked_len > 0
            && locked_len.saturating_add(line_bytes) > max_bytes
        {
            let rotated = rotated_path(path);
            if let Err(error) = fs::rename(path, rotated)
                && error.kind() != io::ErrorKind::NotFound
            {
                return Err(error);
            }
        }
        let _ = FileExt::unlock(&rotation_lock);
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&line)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerQuery {
    RepoCost {
        repo: String,
        since_ms: u64,
    },
    VersionDelta {
        baseline: String,
        candidate: String,
        since_ms: u64,
    },
    AgentSpend {
        since_ms: u64,
    },
}

/// Scan and aggregate the bounded JSONL ledger. Malformed/torn lines are ignored.
pub fn query_ledger(path: &Path, query: &LedgerQuery) -> io::Result<Value> {
    let records = read_records(path)?;
    let since = |ms: u64| records.iter().filter(move |r| r.timestamp_ms >= ms);
    match query {
        LedgerQuery::RepoCost { repo, since_ms } => {
            let (turns, visible, raw, prevented) = since(*since_ms)
                .filter(|r| r.repo == *repo)
                .fold((0_u64, 0_u64, 0_u64, 0_u64), |(t, v, raw, p), r| {
                    (
                        t + 1,
                        v.saturating_add(r.token_mass.visible_tokens),
                        raw.saturating_add(r.token_mass.raw_tokens),
                        p.saturating_add(r.token_mass.prevented_tokens),
                    )
                });
            Ok(json!({
                "schema": LEDGER_SCHEMA,
                "query": "cost_per_repo",
                "repo": repo,
                "since_ms": since_ms,
                "turns": turns,
                "visible_cost_tokens": visible,
                "raw_tokens": raw,
                "prevented_tokens": prevented
            }))
        }
        LedgerQuery::VersionDelta {
            baseline,
            candidate,
            since_ms,
        } => {
            let mut totals = BTreeMap::<&str, u64>::new();
            for r in since(*since_ms) {
                let total = totals.entry(r.version.crate_version.as_str()).or_default();
                *total = total.saturating_add(r.token_mass.visible_tokens);
            }
            let baseline_cost = totals.get(baseline.as_str()).copied().unwrap_or(0);
            let candidate_cost = totals.get(candidate.as_str()).copied().unwrap_or(0);
            Ok(json!({
                "schema": LEDGER_SCHEMA,
                "query": "version_delta",
                "since_ms": since_ms,
                "baseline": {"version": baseline, "visible_cost_tokens": baseline_cost},
                "candidate": {"version": candidate, "visible_cost_tokens": candidate_cost},
                "delta_visible_cost_tokens": i128::from(candidate_cost) - i128::from(baseline_cost)
            }))
        }
        LedgerQuery::AgentSpend { since_ms } => {
            let mut totals = BTreeMap::<&str, (u64, u64)>::new();
            for r in since(*since_ms) {
                let total = totals
                    .entry(r.agent.as_deref().unwrap_or("<unknown>"))
                    .or_default();
                total.0 = total.0.saturating_add(1);
                total.1 = total.1.saturating_add(r.token_mass.visible_tokens);
            }
            let agents = totals
                .into_iter()
                .map(|(agent, (turns, visible_cost_tokens))| {
                    json!({
                        "agent": agent,
                        "turns": turns,
                        "visible_cost_tokens": visible_cost_tokens
                    })
                })
                .collect::<Vec<_>>();
            Ok(json!({
                "schema": LEDGER_SCHEMA,
                "query": "per_agent_spend",
                "since_ms": since_ms,
                "agents": agents
            }))
        }
    }
}

fn read_records(path: &Path) -> io::Result<Vec<LedgerRecord>> {
    let mut records = Vec::new();
    for candidate in [rotated_path(path), path.to_path_buf()] {
        let file = match fs::File::open(candidate) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        for line in BufReader::new(file).lines() {
            let Ok(line) = line else { continue };
            let Ok(record) = serde_json::from_str::<LedgerRecord>(&line) else {
                continue;
            };
            if record.schema == LEDGER_SCHEMA {
                records.push(record);
            }
        }
    }
    Ok(records)
}

// Shareable usage telemetry lives in `usage_telemetry` (opt-in, three-field only).
pub use crate::config::{TELEMETRY_ENV, resolve_telemetry, telemetry_env_enabled};
pub use crate::usage_telemetry::{
    ExecutionPath, TelemetryInspection, UsageRecord, inspect_usage_telemetry as inspect_telemetry,
    usage_telemetry_path_for_cache,
};

/// Summarize every ledger entry's raw and prevented token mass.
pub fn aggregate_token_mass(path: &Path) -> io::Result<(u64, u64)> {
    let records = read_records(path)?;
    let (raw, saved) = records.iter().fold((0_u64, 0_u64), |(raw, saved), record| {
        (
            raw.saturating_add(record.token_mass.raw_tokens),
            saved.saturating_add(record.token_mass.prevented_tokens),
        )
    });
    Ok((raw, saved))
}

pub fn schema_example() -> Value {
    json!({
        "schema": LEDGER_SCHEMA,
        "timestamp_ms": 1_700_000_000_000_u64,
        "session_id": "session-123",
        "repo": "/workspace/repo",
        "agent": null,
        "version": {"crate": env!("CARGO_PKG_VERSION"), "git_describe": null},
        "tool": "read",
        "token_mass": {
            "visible_tokens": 120,
            "raw_tokens": 400,
            "prevented_tokens": 80,
            "saved_bytes": 1024
        },
        "cumulative_session_cost_tokens": 120,
        "optimization_tags": ["session_dedup:on", "diff_reads:on", "tool_surface:mcp"]
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}


#[cfg(test)]
mod rotation_tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use tempfile::tempdir;

    #[test]
    fn concurrent_rotation_does_not_rotate_twice() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let record: LedgerRecord = serde_json::from_value(schema_example()).unwrap();
        let line_len = serde_json::to_vec(&record).unwrap().len() as u64 + 1;
        let max_bytes = line_len + 8;
        let original = vec![b'x'; 9];
        fs::write(&path, &original).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let path = path.clone();
            let record = record.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                append_record(&path, &record, max_bytes).unwrap();
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(fs::read(rotated_path(&path)).unwrap(), original);
        assert_eq!(read_records(&path).unwrap().len(), 2);
    }
}
