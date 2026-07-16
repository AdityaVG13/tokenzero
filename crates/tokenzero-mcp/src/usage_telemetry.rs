//! Opt-in usage telemetry: token-accounting only.
//!
//! Disabled by default. When explicitly enabled, TokenZero may persist only
//! closed `{execution_path, raw_tokens, spent_tokens}` records. This path never
//! stores prompts, responses, commands, paths, refs, tool names, errors,
//! durations, timestamps, or identifiers.
//!
//! ## Counter semantics
//!
//! - `raw_tokens`: uncompressed source token mass from the authoritative
//!   accounting path (`Accounting.raw_tokens` for MCP; CodeMode plan
//!   `raw_tokens` for CodeMode).
//! - `spent_tokens`: tokens actually presented to the caller
//!   (`Accounting.visible_tokens` / CodeMode `visible_tokens`).
//!
//! The contract requires `spent_tokens <= raw_tokens`. Records that violate it
//! are rejected and not persisted.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::config::{TELEMETRY_ENV, resolve_telemetry, telemetry_env_enabled};

/// Execution surface that produced the token-accounting sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionPath {
    Mcp,
    Codemode,
}

/// Complete allowlisted usage-telemetry record. Closed schema — unknown fields
/// fail deserialization so new fields cannot slip in accidentally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageRecord {
    pub execution_path: ExecutionPath,
    pub raw_tokens: u64,
    pub spent_tokens: u64,
}

impl UsageRecord {
    /// Build a record when the accounting contract holds; otherwise reject.
    pub fn try_new(
        execution_path: ExecutionPath,
        raw_tokens: u64,
        spent_tokens: u64,
    ) -> Result<Self, UsageTelemetryError> {
        if spent_tokens > raw_tokens {
            return Err(UsageTelemetryError::SpentExceedsRaw {
                spent_tokens,
                raw_tokens,
            });
        }
        Ok(Self {
            execution_path,
            raw_tokens,
            spent_tokens,
        })
    }

    /// Field names that form the complete allowlist (schema/snapshot tests).
    pub const ALLOWLISTED_FIELDS: &'static [&'static str] =
        &["execution_path", "raw_tokens", "spent_tokens"];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageTelemetryError {
    SpentExceedsRaw { spent_tokens: u64, raw_tokens: u64 },
    Io(String),
}

impl std::fmt::Display for UsageTelemetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpentExceedsRaw {
                spent_tokens,
                raw_tokens,
            } => write!(
                f,
                "spent_tokens ({spent_tokens}) exceeds raw_tokens ({raw_tokens})"
            ),
            Self::Io(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for UsageTelemetryError {}

/// Resolve whether usage telemetry may record, using programmatic override then env.
pub fn usage_telemetry_enabled(programmatic: Option<bool>) -> bool {
    let env_value = std::env::var(TELEMETRY_ENV).ok();
    resolve_telemetry(false, false, programmatic, env_value.as_deref())
}

/// JSONL path beside the recovery cache for opt-in usage records.
pub fn usage_telemetry_path_for_cache(cache_path: &Path) -> PathBuf {
    cache_path.with_file_name("usage-telemetry.jsonl")
}

/// Persist one usage record when enabled. No-op when disabled (creates no file).
pub fn record_usage(
    path: &Path,
    enabled: bool,
    record: &UsageRecord,
) -> Result<(), UsageTelemetryError> {
    if !enabled {
        return Ok(());
    }
    if record.spent_tokens > record.raw_tokens {
        return Err(UsageTelemetryError::SpentExceedsRaw {
            spent_tokens: record.spent_tokens,
            raw_tokens: record.raw_tokens,
        });
    }
    append_record(path, record).map_err(|err| UsageTelemetryError::Io(err.to_string()))
}

/// Record MCP accounting when opted in. Fail-open on I/O; reject bad contracts.
pub fn record_mcp_accounting(
    cache_path: &Path,
    enabled: bool,
    raw_tokens: usize,
    spent_tokens: usize,
) {
    if !enabled {
        return;
    }
    let Ok(record) = UsageRecord::try_new(
        ExecutionPath::Mcp,
        u64::try_from(raw_tokens).unwrap_or(u64::MAX),
        u64::try_from(spent_tokens).unwrap_or(u64::MAX),
    ) else {
        return;
    };
    let path = usage_telemetry_path_for_cache(cache_path);
    let _ = record_usage(&path, true, &record);
}

/// Record CodeMode accounting when opted in. Fail-open on I/O; reject bad contracts.
pub fn record_codemode_accounting(
    cache_path: &Path,
    enabled: bool,
    raw_tokens: usize,
    spent_tokens: usize,
) {
    if !enabled {
        return;
    }
    let Ok(record) = UsageRecord::try_new(
        ExecutionPath::Codemode,
        u64::try_from(raw_tokens).unwrap_or(u64::MAX),
        u64::try_from(spent_tokens).unwrap_or(u64::MAX),
    ) else {
        return;
    };
    let path = usage_telemetry_path_for_cache(cache_path);
    let _ = record_usage(&path, true, &record);
}

/// Inspect opt-in usage telemetry. Never uploads; `exporter` is always `none`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TelemetryInspection {
    pub enabled: bool,
    pub exporter: &'static str,
    /// Allowlisted records only; empty when disabled or nothing recorded.
    pub records: Vec<UsageRecord>,
}

pub fn inspect_usage_telemetry(path: &Path, enabled: bool) -> io::Result<TelemetryInspection> {
    let records = if enabled {
        read_records(path)?
    } else {
        Vec::new()
    };
    Ok(TelemetryInspection {
        enabled,
        exporter: "none",
        records,
    })
}

fn append_record(path: &Path, record: &UsageRecord) -> io::Result<()> {
    let mut line = serde_json::to_vec(record).map_err(io::Error::other)?;
    line.push(b'\n');
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(&line)
}

fn read_records(path: &Path) -> io::Result<Vec<UsageRecord>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut records = Vec::new();
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        let Ok(record) = serde_json::from_str::<UsageRecord>(&line) else {
            continue;
        };
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn default_and_invalid_opt_in_stay_disabled() {
        assert!(!telemetry_env_enabled(None));
        assert!(!telemetry_env_enabled(Some("")));
        assert!(!telemetry_env_enabled(Some("0")));
        assert!(!telemetry_env_enabled(Some("false")));
        assert!(!telemetry_env_enabled(Some("off")));
        assert!(!telemetry_env_enabled(Some("no")));
        assert!(!telemetry_env_enabled(Some("invalid")));
        assert!(!telemetry_env_enabled(Some("maybe")));
        assert!(telemetry_env_enabled(Some("1")));
        assert!(telemetry_env_enabled(Some("ON")));
        assert!(telemetry_env_enabled(Some(" true ")));
        assert!(telemetry_env_enabled(Some("Yes")));
    }

    #[test]
    fn disabled_recording_creates_no_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("usage-telemetry.jsonl");
        let record = UsageRecord::try_new(ExecutionPath::Mcp, 100, 40).unwrap();
        record_usage(&path, false, &record).unwrap();
        assert!(!path.exists(), "disabled path must not create a file");
        let inspection = inspect_usage_telemetry(&path, false).unwrap();
        assert!(!inspection.enabled);
        assert_eq!(inspection.exporter, "none");
        assert!(inspection.records.is_empty());
    }

    #[test]
    fn explicit_opt_in_records_exactly_three_fields_for_mcp_and_codemode() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let path = usage_telemetry_path_for_cache(&cache);

        record_mcp_accounting(&cache, true, 200, 50);
        record_codemode_accounting(&cache, true, 400, 120);

        let inspection = inspect_usage_telemetry(&path, true).unwrap();
        assert!(inspection.enabled);
        assert_eq!(
            inspection.records,
            vec![
                UsageRecord {
                    execution_path: ExecutionPath::Mcp,
                    raw_tokens: 200,
                    spent_tokens: 50,
                },
                UsageRecord {
                    execution_path: ExecutionPath::Codemode,
                    raw_tokens: 400,
                    spent_tokens: 120,
                },
            ]
        );

        let raw = fs::read_to_string(&path).unwrap();
        for line in raw.lines() {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            let obj = value.as_object().unwrap();
            let mut keys: Vec<_> = obj.keys().cloned().collect();
            keys.sort();
            assert_eq!(
                keys,
                vec![
                    "execution_path".to_string(),
                    "raw_tokens".to_string(),
                    "spent_tokens".to_string()
                ]
            );
        }
    }

    #[test]
    fn spent_exceeding_raw_is_rejected() {
        let err = UsageRecord::try_new(ExecutionPath::Mcp, 10, 11).unwrap_err();
        assert_eq!(
            err,
            UsageTelemetryError::SpentExceedsRaw {
                spent_tokens: 11,
                raw_tokens: 10,
            }
        );
        let dir = tempdir().unwrap();
        let path = dir.path().join("usage-telemetry.jsonl");
        let bad = UsageRecord {
            execution_path: ExecutionPath::Mcp,
            raw_tokens: 10,
            spent_tokens: 11,
        };
        let err = record_usage(&path, true, &bad).unwrap_err();
        assert!(matches!(err, UsageTelemetryError::SpentExceedsRaw { .. }));
        assert!(!path.exists());
    }

    #[test]
    fn schema_rejects_non_allowlisted_fields() {
        assert_eq!(
            UsageRecord::ALLOWLISTED_FIELDS,
            &["execution_path", "raw_tokens", "spent_tokens"]
        );
        let with_extra = json!({
            "execution_path": "mcp",
            "raw_tokens": 1,
            "spent_tokens": 1,
            "tool": "read"
        });
        let err = serde_json::from_value::<UsageRecord>(with_extra).unwrap_err();
        assert!(
            err.to_string().contains("unknown field"),
            "expected deny_unknown_fields, got {err}"
        );

        let with_session = json!({
            "execution_path": "codemode",
            "raw_tokens": 2,
            "spent_tokens": 1,
            "session_id": "secret"
        });
        assert!(serde_json::from_value::<UsageRecord>(with_session).is_err());
    }

    #[test]
    fn allowlisted_round_trip_snapshot() {
        let record = UsageRecord::try_new(ExecutionPath::Codemode, 99, 33).unwrap();
        let value = serde_json::to_value(&record).unwrap();
        assert_eq!(
            value,
            json!({
                "execution_path": "codemode",
                "raw_tokens": 99,
                "spent_tokens": 33
            })
        );
        let back: UsageRecord = serde_json::from_value(value).unwrap();
        assert_eq!(back, record);
    }
}
