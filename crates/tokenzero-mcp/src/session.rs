//! Session redundancy layer state (docs/routing.md §5): an in-memory
//! seen-set of payloads already served this session, keyed per file range or
//! per search output. The content hash of the exact served payload is the
//! only invalidation source; mtime is never consulted.
//!
//! The map is process-memory only, by design: a sidecar surviving restart
//! would assert "served earlier this session" to a fresh session whose
//! context never contained the bytes, and two agents sharing one repo would
//! cross-contaminate. Supervisor respawn loses the map and degrades to full
//! serves — never wrong, only un-optimized. Lock poisoning fails open the
//! same way.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

/// Identity of one served payload. File reads are keyed per canonicalized
/// path and requested line range; find/grep outputs are keyed per tool,
/// query, and canonicalized root set.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone)]
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
    pub served_at: SystemTime,
    pub serve_count: usize,
}

/// Lookup outcome for a key against the current payload hash.
#[derive(Debug, Clone)]
pub(crate) enum SeenState {
    Miss,
    /// Identical content already served; `serve_count` counts prior serves.
    Unchanged {
        serve_count: usize,
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
    dedup_hits: usize,
    diff_hits: usize,
    visible_tokens_saved: usize,
    diff_tokens_saved: usize,
}

impl SessionMemory {
    pub fn lookup(&self, key: &ServeKey, content_sha256: &str) -> SeenState {
        match self.records.get(key) {
            None => SeenState::Miss,
            Some(record) if record.content_sha256 == content_sha256 => SeenState::Unchanged {
                serve_count: record.serve_count,
            },
            Some(record) => SeenState::Changed {
                previous: record.clone(),
            },
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

    pub fn rollup(&self) -> Value {
        json!({
            "records": self.records.len(),
            "dedup_hits": self.dedup_hits,
            "diff_hits": self.diff_hits,
            "visible_tokens_saved": self.visible_tokens_saved,
            "diff_tokens_saved": self.diff_tokens_saved
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
    pub diff: Option<DiffTelemetry>,
}

#[derive(Debug, Clone)]
pub(crate) struct DiffTelemetry {
    pub hunks: usize,
    pub plus: usize,
    pub minus: usize,
    pub base_ref: String,
}

impl SessionSummary {
    pub fn note_dedup(&mut self, serve_count: usize, saved: usize) {
        self.dedup_notes += 1;
        self.serve_count = serve_count;
        self.visible_saved += saved;
    }

    pub fn note_diff(&mut self, info: DiffTelemetry, saved: usize) {
        self.diff_serves += 1;
        self.diff_saved += saved;
        self.diff = Some(info);
    }

    /// Telemetry fragment to merge into the tool response, or `None` when
    /// the call served everything full.
    pub fn telemetry(&self) -> Option<Value> {
        let strategy = match (self.dedup_notes > 0, self.diff_serves > 0) {
            (true, true) => "seen_set_dedup+diff_since_served",
            (true, false) => "seen_set_dedup",
            (false, true) => "diff_since_served",
            (false, false) => return None,
        };
        let mut value = json!({
            "output_strategy": strategy,
            "cache_hit": true
        });
        if self.dedup_notes > 0 {
            value["dedup"] = json!({
                "hits": self.dedup_notes,
                "serve_count": self.serve_count,
                "visible_tokens_saved": self.visible_saved
            });
        }
        if let Some(diff) = &self.diff {
            value["diff"] = json!({
                "hunks": diff.hunks,
                "plus": diff.plus,
                "minus": diff.minus,
                "base_ref": diff.base_ref
            });
        }
        Some(value)
    }
}
