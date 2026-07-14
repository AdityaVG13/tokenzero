//! Session redundancy layer state (docs/routing.md §5): an in-memory
//! seen-set of payloads already served this session, keyed per file range or
//! per search output. The content hash of the exact served payload is the
//! only invalidation source; mtime is never consulted.
//!
//! When `session_dedup` is on, the map is also persisted under the store
//! root (`session-memory.json`, scoped by `TOKENZERO_SESSION_SCOPE`) so MCP
//! process respawn can still dedup. Dedup always re-checks `content_sha256`
//! against the current payload before suppressing. Lock poisoning fails open
//! the same way (full serve, no persist on that path).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

/// Identity of one served payload. File reads are keyed per canonicalized
/// path and requested line range; find/grep outputs are keyed per tool,
/// query, and canonicalized root set.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ServeKey {
    File {
        path: PathBuf,
        start: Option<usize>,
        end: Option<usize>,
    },
    Output {
        tool: String,
        query: String,
        roots: Vec<PathBuf>,
    },
    /// Ref-based delivery (expand / CodeMode expand): keyed by ref + window +
    /// selector/symbol normalization.
    Expand {
        ref_id: String,
        start_line: Option<usize>,
        end_line: Option<usize>,
        selector_norm: String,
        symbol_norm: String,
        anchor_kind_norm: String,
    },
}

/// What was served for a key. `content_sha256` is the hash of the exact
/// canonical payload text (the bytes behind `blob_ref`) — the invalidation
/// check. Refs are refreshed on every serve, so the stored ones always point
/// at recoverable content for the latest serve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ServedRecord {
    pub content_sha256: String,
    pub blob_ref: String,
    /// Kept for diagnostics: serving paths mint fresh refs per call, and
    /// content-addressing makes them identical to these on an unchanged hit.
    #[allow(dead_code)]
    pub file_ref: String,
    #[allow(dead_code)]
    pub raw_tokens: usize,
    pub line_count: usize,
    pub byte_len: usize,
    /// Telemetry only — never an invalidation input (the content hash is).
    #[allow(dead_code)]
    #[serde(rename = "served_at_unix_secs", default = "SystemTime::now", serialize_with = "serialize_served_at", deserialize_with = "deserialize_served_at")]
    pub served_at: SystemTime,
    pub serve_count: usize,
}

fn serialize_served_at<S: serde::Serializer>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error> {
    time.duration_since(SystemTime::UNIX_EPOCH).ok().map(|d| d.as_secs()).serialize(serializer)
}

fn deserialize_served_at<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<SystemTime, D::Error> {
    Ok(Option::<u64>::deserialize(deserializer)?
        .and_then(|secs| SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(secs)))
        .unwrap_or_else(SystemTime::now))
}

/// Lookup outcome for a key against the current payload hash.
#[derive(Debug, Clone)]
pub(crate) enum SeenState {
    Miss,
    /// Identical content already served; `serve_count` counts prior serves.
    Unchanged {
        serve_count: usize,
        cross_session: bool,
    },
    /// Same key, different content: the previously served record, used as
    /// the diff base.
    Changed {
        previous: ServedRecord,
    },
}

#[derive(Debug, Default)]
pub(crate) struct SessionMemory {
    records: HashMap<ServeKey, ServedRecord>,
    restored_content_hashes: HashSet<String>,
    dedup_hits: usize,
    diff_hits: usize,
    visible_tokens_saved: usize,
    diff_tokens_saved: usize,
    session_hwm: u64,
    full_bytes: usize,
    delta_bytes: usize,
}

impl SessionMemory {
    pub fn lookup(&self, key: &ServeKey, content_sha256: &str) -> SeenState {
        let cross_session = self.restored_content_hashes.contains(content_sha256);
        match self.records.get(key) {
            Some(record) if record.content_sha256 == content_sha256 => SeenState::Unchanged {
                serve_count: record.serve_count,
                cross_session,
            },
            Some(record) => SeenState::Changed {
                previous: record.clone(),
            },
            None => self
                .records
                .values()
                .filter(|record| record.content_sha256 == content_sha256)
                .map(|record| record.serve_count)
                .max()
                .map(|serve_count| SeenState::Unchanged {
                    serve_count,
                    cross_session,
                })
                .unwrap_or(SeenState::Miss),
        }
    }

    /// Insert or replace the record for a key, carrying the serve counter
    /// forward across content changes.
    pub fn record(&mut self, key: ServeKey, mut record: ServedRecord) {
        if let Some(existing) = self.records.get(&key) {
            record.serve_count = existing.serve_count + 1;
        }
        self.records.insert(key, record);
    }

    pub fn absorb(&mut self, summary: &SessionSummary) {
        self.dedup_hits += summary.dedup_notes;
        self.diff_hits += summary.diff_serves;
        self.visible_tokens_saved += summary.visible_saved;
        self.diff_tokens_saved += summary.diff_saved;
    }

    /// Restore disk-backed seen-set for this scope (dedup on only).
    pub(crate) fn restore_from_persist(
        &mut self,
        records: HashMap<ServeKey, ServedRecord>,
        dedup_hits: usize,
        diff_hits: usize,
        visible_tokens_saved: usize,
        diff_tokens_saved: usize,
        session_hwm: u64,
        full_bytes: usize,
        delta_bytes: usize,
    ) {
        self.restored_content_hashes = records
            .values()
            .map(|record| record.content_sha256.clone())
            .collect();
        self.records = records;
        self.dedup_hits = dedup_hits;
        self.diff_hits = diff_hits;
        self.visible_tokens_saved = visible_tokens_saved;
        self.diff_tokens_saved = diff_tokens_saved;
        self.session_hwm = session_hwm;
        self.full_bytes = full_bytes;
        self.delta_bytes = delta_bytes;
    }

    pub(crate) fn records_snapshot(&self) -> &HashMap<ServeKey, ServedRecord> {
        &self.records
    }

    pub(crate) fn session_hwm(&self) -> u64 {
        self.session_hwm
    }

    pub(crate) fn advance_hwm(&mut self) -> (u64, u64) {
        let from = self.session_hwm;
        self.session_hwm = self.session_hwm.saturating_add(1);
        (from, self.session_hwm)
    }

    pub(crate) fn note_bytes(&mut self, full: usize, delta: usize) {
        self.full_bytes = self.full_bytes.saturating_add(full);
        self.delta_bytes = self.delta_bytes.saturating_add(delta);
    }

    pub(crate) fn byte_rollup(&self) -> (usize, usize) {
        (self.full_bytes, self.delta_bytes)
    }

    pub(crate) fn rollup_counters(&self) -> (usize, usize, usize, usize) {
        (
            self.dedup_hits,
            self.diff_hits,
            self.visible_tokens_saved,
            self.diff_tokens_saved,
        )
    }

    pub fn rollup(&self) -> Value {
        json!({
            "records": self.records.len(),
            "dedup_hits": self.dedup_hits,
            "diff_hits": self.diff_hits,
            "visible_tokens_saved": self.visible_tokens_saved,
            "diff_tokens_saved": self.diff_tokens_saved,
            "session_hwm": self.session_hwm,
            "full_bytes": self.full_bytes,
            "delta_bytes": self.delta_bytes
        })
    }
}

/// Per-call accumulator for redundancy outcomes: feeds both the response
/// telemetry (merged, never clobbering existing keys' siblings) and the
/// session rollup counters.
#[derive(Debug, Default)]
pub(crate) struct SessionSummary {
    pub dedup_notes: usize,
    pub diff_serves: usize,
    pub visible_saved: usize,
    pub diff_saved: usize,
    pub serve_count: usize,
    pub cross_session_hits: usize,
    pub diff: Option<DiffTelemetry>,
    pub full_bytes: Option<usize>,
    pub delta_bytes: Option<usize>,
    pub from_hwm: u64,
    pub to_hwm: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct DiffTelemetry {
    pub hunks: usize,
    pub plus: usize,
    pub minus: usize,
    pub base_ref: String,
}

impl SessionSummary {
    pub fn note_dedup(&mut self, serve_count: usize, saved: usize, cross_session: bool) {
        self.dedup_notes += 1;
        self.serve_count = serve_count;
        self.visible_saved += saved;
        self.cross_session_hits += usize::from(cross_session);
    }

    pub fn note_diff(&mut self, info: DiffTelemetry, saved: usize) {
        self.diff_serves += 1;
        self.diff_saved += saved;
        self.diff = Some(info);
    }

    pub fn note_wire_bytes(&mut self, full_bytes: usize, delta_bytes: usize) {
        self.full_bytes = Some(full_bytes);
        self.delta_bytes = Some(delta_bytes);
    }

    pub fn set_watermark(&mut self, from_hwm: u64, to_hwm: u64) {
        self.from_hwm = from_hwm;
        self.to_hwm = to_hwm;
    }

    /// Telemetry fragment to merge into the tool response, or `None` when
    /// the call served everything full.
    pub fn telemetry(&self) -> Option<Value> {
        let strategy = match (self.dedup_notes > 0, self.diff_serves > 0) {
            (true, true) => "seen_set_dedup+diff_since_served",
            (true, false) => "seen_set_dedup",
            (false, true) => "diff_since_served",
            (false, false) if self.full_bytes.is_some() => "full",
            (false, false) => return None,
        };
        let mut value = json!({
            "output_strategy": strategy,
            "cache_hit": self.dedup_notes > 0 || self.diff_serves > 0
        });
        if let (Some(full_bytes), Some(delta_bytes)) = (self.full_bytes, self.delta_bytes) {
            value["session_delta"] = json!({
                "from_hwm": self.from_hwm,
                "to_hwm": self.to_hwm,
                "full_bytes": full_bytes,
                "delta_bytes": delta_bytes,
                "saved_bytes": full_bytes.saturating_sub(delta_bytes)
            });
        }
        if self.dedup_notes > 0 {
            let cross_session_bytes_saved = if self.cross_session_hits > 0 {
                self.full_bytes
                    .zip(self.delta_bytes)
                    .map(|(full, delta)| full.saturating_sub(delta))
                    .unwrap_or(0)
            } else {
                0
            };
            value["dedup"] = json!({
                "hits": self.dedup_notes,
                "serve_count": self.serve_count,
                "visible_tokens_saved": self.visible_saved,
                "cross_session_hits": self.cross_session_hits,
                "cross_session_bytes_saved": cross_session_bytes_saved
            });
        }
        if let Some(diff) = &self.diff {
            value["diff"] = json!({ "hunks": diff.hunks, "plus": diff.plus, "minus": diff.minus, "base_ref": diff.base_ref, "visible_tokens_saved": self.diff_saved });
        }
        Some(value)
    }
}

#[cfg(test)]
mod delta_tests {
    use super::*;
    #[test]
    fn telemetry_reports_watermark_and_wire_bytes() {
        let mut summary = SessionSummary::default();
        summary.note_wire_bytes(240, 32);
        summary.set_watermark(7, 8);
        let telemetry = summary.telemetry().expect("delta telemetry");
        assert_eq!(telemetry["session_delta"]["from_hwm"], 7);
        assert_eq!(telemetry["session_delta"]["to_hwm"], 8);
        assert_eq!(telemetry["session_delta"]["full_bytes"], 240);
        assert_eq!(telemetry["session_delta"]["delta_bytes"], 32);
        assert_eq!(telemetry["session_delta"]["saved_bytes"], 208);
    }
    #[test]
    fn watermark_is_monotonic() {
        let mut memory = SessionMemory::default();
        assert_eq!(memory.advance_hwm(), (0, 1));
        assert_eq!(memory.advance_hwm(), (1, 2));
        assert_eq!(memory.session_hwm(), 2);
    }
}
