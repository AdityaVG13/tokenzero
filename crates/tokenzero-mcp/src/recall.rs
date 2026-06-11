//! `tz_recall`: lossless full-text search over the on-disk recovery cache.
//!
//! The recovery store's in-memory state is private to `tokenzero-recovery`,
//! so recall reads the persisted cache file as tolerant JSON: entries that
//! match the published `StoredFile` shape (`ref_id` + `text` + optional
//! `path`/`content_type`) are searched, anything else is skipped, and any
//! read/parse failure degrades to zero hits with a diagnostic. The store is
//! never mutated — recall is a read-only meta-query, and every hit line
//! carries the exact `tz://` ref so the full payload stays one `tz_expand`
//! away.

use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

/// One matched line inside a stored payload.
pub(crate) struct RecallHit {
    pub ref_id: String,
    /// Source path when the payload had one, else its content type.
    pub label: String,
    pub line: usize,
    pub text: String,
}

pub(crate) struct RecallOutcome {
    pub hits: Vec<RecallHit>,
    pub payloads_searched: usize,
    pub truncated: bool,
    /// A cache that exists but cannot be read or parsed. A missing cache is
    /// an empty store, not an error.
    pub unreadable: bool,
}

const MAX_HIT_LINE_CHARS: usize = 160;

/// Recall parses the whole cache file per query — a linear scan bounded by
/// the store's own eviction ceiling (8 MB by default). This guard mirrors
/// the recovery crate's `max_load_bytes` so a foreign or corrupted file at
/// the cache path degrades to `unreadable` instead of an unbounded parse.
const MAX_RECALL_LOAD_BYTES: u64 = 16_000_000;

/// Case-insensitive substring search across every stored payload's lines.
/// `files` entries are searched first (they carry source paths); `blobs`
/// whose exact text was already covered by a file entry are skipped so the
/// same content never reports twice.
pub(crate) fn recall_search(cache_path: &Path, query: &str, max_hits: usize) -> RecallOutcome {
    let mut outcome = RecallOutcome {
        hits: Vec::new(),
        payloads_searched: 0,
        truncated: false,
        unreadable: false,
    };
    if std::fs::metadata(cache_path).is_ok_and(|meta| meta.len() > MAX_RECALL_LOAD_BYTES) {
        outcome.unreadable = true;
        return outcome;
    }
    let raw = match std::fs::read_to_string(cache_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return outcome,
        Err(_) => {
            outcome.unreadable = true;
            return outcome;
        }
    };
    let Ok(snapshot) = serde_json::from_str::<Value>(&raw) else {
        outcome.unreadable = true;
        return outcome;
    };
    let needle = query.to_lowercase();
    let mut seen_texts: HashSet<u64> = HashSet::new();
    for section in ["files", "blobs"] {
        let Some(entries) = snapshot.get(section).and_then(Value::as_object) else {
            continue;
        };
        for entry in entries.values() {
            let Some(text) = entry.get("text").and_then(Value::as_str) else {
                continue;
            };
            let Some(ref_id) = entry.get("ref_id").and_then(Value::as_str) else {
                continue;
            };
            if !seen_texts.insert(text_fingerprint(text)) {
                continue;
            }
            outcome.payloads_searched += 1;
            let label = entry
                .get("path")
                .and_then(Value::as_str)
                .or_else(|| entry.get("content_type").and_then(Value::as_str))
                .unwrap_or("payload")
                .to_string();
            for (idx, line) in text.lines().enumerate() {
                if !line.to_lowercase().contains(&needle) {
                    continue;
                }
                if outcome.hits.len() >= max_hits {
                    outcome.truncated = true;
                    return outcome;
                }
                outcome.hits.push(RecallHit {
                    ref_id: ref_id.to_string(),
                    label: label.clone(),
                    line: idx + 1,
                    text: clamp_line(line),
                });
            }
        }
    }
    outcome
}

/// Compact "what TokenZero already served this workspace" listing for
/// post-compaction recovery: most-recent stored payloads first, each with
/// its exact `tz://` ref, token-budgeted. Pure read over the persisted
/// cache; returns `None` when there is nothing to restore.
pub(crate) fn build_session_pack(cache_path: &Path, max_tokens: usize) -> Option<String> {
    let raw = std::fs::read_to_string(cache_path).ok()?;
    let snapshot: Value = serde_json::from_str(&raw).ok()?;
    let mut entries: Vec<(&str, &Value)> = Vec::new();
    for section in ["files", "blobs"] {
        if let Some(map) = snapshot.get(section).and_then(Value::as_object) {
            for entry in map.values() {
                if let Some(ref_id) = entry.get("ref_id").and_then(Value::as_str) {
                    entries.push((ref_id, entry));
                }
            }
        }
    }
    if entries.is_empty() {
        return None;
    }
    let total_unique = entries
        .iter()
        .filter_map(|(_, entry)| entry.get("text").and_then(Value::as_str))
        .filter(|text| !text.is_empty())
        .map(text_fingerprint)
        .collect::<HashSet<u64>>()
        .len();
    let by_ref: std::collections::HashMap<&str, &Value> = entries.iter().copied().collect();
    // The order array is append-ordered; walk it backwards for recency and
    // dedup identical content (a file and its blob share bytes).
    let order: Vec<&str> = snapshot
        .get("order")
        .and_then(Value::as_array)
        .map(|refs| refs.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut seen_texts: HashSet<u64> = HashSet::new();
    let mut listed = 0usize;
    let header = "# tokenzero session pack — context already served in this workspace\n\
                  Recover any item exactly with `expand <ref>`; search all stored content with `recall <query>` — never re-read or re-run for content listed here.";
    let mut pack = header.to_string();
    let candidates = order
        .iter()
        .rev()
        .filter_map(|ref_id| by_ref.get(ref_id).map(|entry| (*ref_id, *entry)))
        .chain(entries.iter().map(|(ref_id, entry)| (*ref_id, *entry)));
    for (ref_id, entry) in candidates {
        let Some(text) = entry.get("text").and_then(Value::as_str) else {
            continue;
        };
        if text.is_empty() || !seen_texts.insert(text_fingerprint(text)) {
            continue;
        }
        let label = match entry.get("path").and_then(Value::as_str) {
            Some(path) if path.starts_with("shell:") => "shell capture".to_string(),
            Some(path) => path.to_string(),
            None => entry
                .get("content_type")
                .and_then(Value::as_str)
                .unwrap_or("payload")
                .to_string(),
        };
        let line = format!(
            "\n- {label} — {} lines, ~{} tok — {ref_id}",
            text.lines().count(),
            text.len() / 4
        );
        let extended = format!("{pack}{line}");
        if tokenzero_core::count_tokens(&extended) > max_tokens {
            break;
        }
        pack = extended;
        listed += 1;
    }
    if listed == 0 {
        return None;
    }
    let remainder = total_unique.saturating_sub(listed);
    if remainder > 0 {
        pack.push_str(&format!(
            "\n(+ {remainder} more stored payloads — `recall <query>` searches them all)"
        ));
    }
    Some(pack)
}

fn clamp_line(line: &str) -> String {
    let trimmed = line.trim();
    let clamped: String = trimmed.chars().take(MAX_HIT_LINE_CHARS).collect();
    if clamped.chars().count() < trimmed.chars().count() {
        format!("{clamped}...")
    } else {
        clamped
    }
}

fn text_fingerprint(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests;
