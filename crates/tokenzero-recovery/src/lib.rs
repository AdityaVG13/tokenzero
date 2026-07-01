#![forbid(unsafe_code)]

use fs4::{FileExt, TryLockError};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    LazyLock,
    atomic::{AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokenzero_core::{ContentType, count_tokens, error_block, id_for, sha256_hex, symbol_block};

const LOCK_RETRIES: usize = 240;
const MAX_SHELL_OUTCOMES: usize = 256;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(25);
const TMP_RETRIES: usize = 16;

/// `file:line[:col]` matcher for search-output ingestion. Compiled once on first
/// use — the pattern is a compile-time literal, so `expect` can only fire on a
/// programmer typo (caught by the unit tests below), never on user input. The
/// previous code rebuilt this regex on every `store_search_output` call.
static SEARCH_PATH_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<path>(?:[\w.@+-]+/)+[\w.@+-]+):(?P<line>\d+):?(?P<col>\d+)?")
        .expect("SEARCH_PATH_LINE is a valid compile-time regex literal")
});
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    pub max_blobs: usize,
    pub max_files: usize,
    pub max_units: usize,
    pub max_search_hits: usize,
    pub max_bytes: usize,
    pub max_load_bytes: usize,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            max_blobs: 128,
            max_files: 256,
            max_units: 2048,
            max_search_hits: 1024,
            max_bytes: 8_000_000,
            max_load_bytes: 16_000_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFile {
    pub ref_id: String,
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_identity: Option<String>,
    pub text: String,
    pub content_type: String,
    pub source_fingerprint: Option<SourceFingerprint>,
    pub source_start_line: Option<usize>,
    pub source_end_line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredUnit {
    pub ref_id: String,
    pub text: String,
    pub content_type: String,
    pub source_ref: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFingerprint {
    pub size: u64,
    pub mtime_ns: u128,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPayload {
    pub blob_ref: String,
    pub file_ref: String,
    pub unit_refs: Vec<String>,
    pub raw_tokens: usize,
    pub source_start_line: Option<usize>,
    pub source_end_line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpansionResult {
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub selector: Option<String>,
    pub content: String,
    pub tokens: usize,
    pub found: bool,
    pub reason: String,
}

impl ExpansionResult {
    pub fn ok(ref_id: String, selector: Option<String>, content: String) -> Self {
        let tokens = count_tokens(&content);
        Self {
            ref_id,
            selector,
            content,
            tokens,
            found: true,
            reason: "ok".to_string(),
        }
    }

    pub fn missing(ref_id: String, selector: Option<String>, reason: impl Into<String>) -> Self {
        Self {
            ref_id,
            selector,
            content: String::new(),
            tokens: 0,
            found: false,
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RecoveryState {
    pub version: u32,
    pub max_blobs: usize,
    pub max_files: usize,
    pub max_units: usize,
    pub max_search_hits: usize,
    pub max_bytes: usize,
    pub blobs: BTreeMap<String, String>,
    pub files: BTreeMap<String, StoredFile>,
    pub units: BTreeMap<String, StoredUnit>,
    pub search_hits: BTreeMap<String, StoredUnit>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    pub order: Vec<String>,
    #[serde(default)]
    pub shell_outcomes: BTreeMap<String, ShellOutcome>,
    #[serde(default)]
    pub shell_outcome_seq: u64,
}

/// Last observed result of a shell command, keyed by scope+command hash, so
/// repeat runs with byte-identical output can render as a delta instead of
/// re-paying for the full capsule. The combined blob itself lives in `blobs`
/// (content-addressed), so this index never duplicates payload bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ShellOutcome {
    pub combined_sha: String,
    pub exit_code: Option<i32>,
    pub seen: u32,
    pub seq: u64,
}

/// Repeat verdict for the command just recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellRepeat {
    pub unchanged: bool,
    pub seen: u32,
}

impl RecoveryState {
    fn empty(config: &RecoveryConfig) -> Self {
        Self {
            version: 1,
            max_blobs: config.max_blobs,
            max_files: config.max_files,
            max_units: config.max_units,
            max_search_hits: config.max_search_hits,
            max_bytes: config.max_bytes,
            blobs: BTreeMap::new(),
            files: BTreeMap::new(),
            units: BTreeMap::new(),
            search_hits: BTreeMap::new(),
            aliases: BTreeMap::new(),
            order: Vec::new(),
            shell_outcomes: BTreeMap::new(),
            shell_outcome_seq: 0,
        }
    }
}

#[derive(Debug)]
pub struct RecoveryStore {
    pub config: RecoveryConfig,
    pub persistence_path: Option<PathBuf>,
    state: RecoveryState,
    session_refs: Vec<String>,
    /// Identity of the cache file as last written by this store, captured
    /// while still holding the persist lock. `None` until the first persist;
    /// also reset to `None` whenever a write fails, so the next persist must
    /// take the full reload+merge path.
    disk_identity: Option<DiskIdentity>,
    pub recovery_count: usize,
    pub recovery_tokens: usize,
}

/// Cache-file identity used to detect foreign writes between persists.
/// `atomic_write_json` always replaces the file via rename, so any cooperating
/// writer changes the inode — mtime alone is never trusted (its granularity
/// can be a full second on some filesystems). Captured only under the persist
/// lock; any uncertainty (missing file, unreadable metadata) must yield `None`
/// and force the full reload+merge path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiskIdentity {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}

impl DiskIdentity {
    fn capture(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        if !meta.is_file() {
            return None;
        }
        let modified = meta.modified().ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some(Self {
                len: meta.len(),
                modified,
                dev: meta.dev(),
                ino: meta.ino(),
            })
        }
        #[cfg(not(unix))]
        Some(Self {
            len: meta.len(),
            modified,
        })
    }
}

impl RecoveryStore {
    pub fn new(persistence_path: Option<PathBuf>) -> Self {
        Self::with_config(persistence_path, RecoveryConfig::default())
    }

    pub fn with_config(persistence_path: Option<PathBuf>, config: RecoveryConfig) -> Self {
        let state = persistence_path
            .as_ref()
            .and_then(|path| load_state(path, &config).ok().flatten())
            .unwrap_or_else(|| RecoveryState::empty(&config));
        Self {
            config,
            persistence_path,
            state,
            session_refs: Vec::new(),
            // The constructor's load runs without the persist lock, so the
            // file could be replaced between read and stat; the identity is
            // only ever captured under the lock, after our own write.
            disk_identity: None,
            recovery_count: 0,
            recovery_tokens: 0,
        }
    }

    pub fn store_payload(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
    ) -> Result<StoredPayload, RecoveryError> {
        let stored = self.store_payload_deferred(
            text,
            content_type,
            path,
            source_start_line,
            source_end_line,
        );
        self.persist()?;
        Ok(stored)
    }

    pub fn store_payload_deferred(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
    ) -> StoredPayload {
        let stored = self.store_payload_deferred_batch(
            text,
            content_type,
            path,
            source_start_line,
            source_end_line,
        );
        self.evict();
        stored
    }

    pub fn store_payload_deferred_batch(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
    ) -> StoredPayload {
        let blob_ref = self.put_blob(text);
        let file_ref = self.put_file(text, content_type, path, source_start_line, source_end_line);
        let unit_refs = self.index_units(text, content_type, &file_ref);
        StoredPayload {
            blob_ref,
            file_ref,
            unit_refs,
            raw_tokens: count_tokens(text),
            source_start_line,
            source_end_line,
        }
    }

    pub fn persist_pending(&mut self) -> Result<(), RecoveryError> {
        self.persist()
    }

    pub fn store_alias(&mut self, alias: &str, target_ref: &str) -> Result<(), RecoveryError> {
        self.state
            .aliases
            .insert(alias.to_string(), target_ref.to_string());
        self.persist()
    }

    pub fn expected_refs(text: &str, path: Option<&Path>) -> (String, String) {
        let blob_ref = format!("tz://blob/{}", id_for('b', text));
        let file_ref = recovery_file_ref(text, path);
        (blob_ref, file_ref)
    }

    pub fn store_search_output(
        &mut self,
        output: &str,
        query: Option<&str>,
    ) -> Result<Vec<String>, RecoveryError> {
        let refs = self.store_search_output_deferred(output, query);
        self.persist()?;
        Ok(refs)
    }

    pub fn store_search_output_deferred(
        &mut self,
        output: &str,
        query: Option<&str>,
    ) -> Vec<String> {
        let path_line = &*SEARCH_PATH_LINE;
        let mut refs = Vec::new();
        for (idx, line) in output.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            if query.is_some_and(|q| !line.contains(q)) && !path_line.is_match(line) {
                continue;
            }
            let hit_id = id_for('h', &format!("search:{idx}:{line}"));
            let ref_id = format!("tz://search/{hit_id}");
            self.state.search_hits.insert(
                ref_id.clone(),
                StoredUnit {
                    ref_id: ref_id.clone(),
                    text: line.to_string(),
                    content_type: "search_result".to_string(),
                    source_ref: None,
                    start_line: Some(idx + 1),
                    end_line: Some(idx + 1),
                },
            );
            self.remember_ref(&ref_id);
            refs.push(ref_id);
        }
        self.evict();
        refs
    }

    pub fn expand(
        &mut self,
        ref_id: &str,
        selector: Option<&str>,
        start_line: Option<usize>,
        end_line: Option<usize>,
        anchor_kind: Option<&str>,
        symbol: Option<&str>,
    ) -> ExpansionResult {
        self.recovery_count += 1;
        let requested_ref = ref_id.to_string();
        let ref_id = self
            .resolve_alias_chain(ref_id)
            .unwrap_or_else(|| ref_id.to_string());
        let Some(parsed) = parse_ref(&ref_id) else {
            return ExpansionResult::missing(
                requested_ref,
                selector.map(str::to_string),
                "invalid-ref",
            );
        };
        let mut selected_start = start_line;
        let mut selected_end = end_line;
        if let Some(fragment) = parsed.fragment.as_deref().filter(|f| f.starts_with('L')) {
            let (start, end) = parse_line_fragment(fragment);
            selected_start = start;
            selected_end = end;
        }
        let Some(content) = self.resolve_ref(&parsed.kind, &parsed.bare) else {
            return ExpansionResult::missing(
                requested_ref,
                selector.map(str::to_string),
                "dangling-ref",
            );
        };
        if parsed.kind == "file" && self.file_ref_is_stale(&parsed.bare) {
            return ExpansionResult::missing(
                requested_ref,
                selector.map(str::to_string),
                "stale-ref",
            );
        }
        let selected = select_content(
            &content,
            selector,
            selected_start,
            selected_end,
            anchor_kind,
            symbol,
        );
        self.recovery_tokens += count_tokens(&selected);
        ExpansionResult::ok(requested_ref, selector.map(str::to_string), selected)
    }

    fn resolve_alias_chain(&self, ref_id: &str) -> Option<String> {
        let mut current = ref_id;
        for _ in 0..8 {
            let Some(next) = self.state.aliases.get(current) else {
                return (current != ref_id).then(|| current.to_string());
            };
            current = next;
        }
        None
    }

    pub fn has_ref(&self, ref_id: &str) -> bool {
        let Some(parsed) = parse_ref(ref_id) else {
            return false;
        };
        match parsed.kind.as_str() {
            "blob" => self.state.blobs.contains_key(&parsed.bare),
            "file" => self.state.files.contains_key(&parsed.bare),
            "unit" => self.state.units.contains_key(&parsed.bare),
            "search" => self.state.search_hits.contains_key(&parsed.bare),
            _ => false,
        }
    }

    pub fn export_status(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": "tokenzero.recovery.v1",
            "blobs": self.state.blobs.len(),
            "files": self.state.files.len(),
            "units": self.state.units.len(),
            "search_hits": self.state.search_hits.len(),
            "max_blobs": self.config.max_blobs,
            "max_files": self.config.max_files,
            "max_units": self.config.max_units,
            "max_search_hits": self.config.max_search_hits,
            "approx_bytes": self.approx_bytes(),
            "max_bytes": self.config.max_bytes,
            "recovery_count": self.recovery_count,
            "recovery_tokens": self.recovery_tokens,
            "persistent": self.persistence_path.is_some(),
            "persistence_path": self.persistence_path.as_ref().map(|p| p.display().to_string()),
        })
    }

    pub fn prune_stale(&mut self, dry_run: bool) -> Result<serde_json::Value, RecoveryError> {
        let stale: Vec<String> = self
            .state
            .files
            .keys()
            .filter(|ref_id| self.file_ref_is_stale(ref_id))
            .cloned()
            .collect();
        if !dry_run {
            for ref_id in &stale {
                self.drop_ref(ref_id);
            }
            self.persist()?;
        }
        Ok(serde_json::json!({
            "schema_version": "tokenzero.cache.v1",
            "status": "ok",
            "dry_run": dry_run,
            "candidates": stale.iter().map(|ref_id| serde_json::json!({"category": "exact", "ref": ref_id, "reason": "stale-source"})).collect::<Vec<_>>(),
            "reclaimed_bytes": if dry_run { 0 } else { stale.len() },
        }))
    }

    /// Record the outcome of a shell command and report whether it repeated
    /// the previous run byte-for-byte (same combined output, same exit code).
    /// Callers may render verified-unchanged successes as a tiny delta
    /// envelope; the content-addressed blob ref still recovers exact bytes.
    pub fn record_shell_outcome(
        &mut self,
        scope: Option<&str>,
        command: &str,
        combined: &str,
        exit_code: Option<i32>,
    ) -> Result<ShellRepeat, RecoveryError> {
        let repeat = self.record_shell_outcome_deferred(scope, command, combined, exit_code);
        self.persist()?;
        Ok(repeat)
    }

    pub fn record_shell_outcome_deferred(
        &mut self,
        scope: Option<&str>,
        command: &str,
        combined: &str,
        exit_code: Option<i32>,
    ) -> ShellRepeat {
        let key = id_for('s', &format!("{}\u{0}{command}", scope.unwrap_or("")));
        let combined_sha = sha256_hex(combined);
        let seq = self.state.shell_outcome_seq.wrapping_add(1);
        self.state.shell_outcome_seq = seq;
        let (unchanged, seen) = match self.state.shell_outcomes.get(&key) {
            Some(prev) if prev.combined_sha == combined_sha && prev.exit_code == exit_code => {
                (true, prev.seen.saturating_add(1))
            }
            _ => (false, 1),
        };
        self.state.shell_outcomes.insert(
            key,
            ShellOutcome {
                combined_sha,
                exit_code,
                seen,
                seq,
            },
        );
        while self.state.shell_outcomes.len() > MAX_SHELL_OUTCOMES {
            let victim = self
                .state
                .shell_outcomes
                .iter()
                .min_by_key(|(_, outcome)| outcome.seq)
                .map(|(key, _)| key.clone());
            match victim {
                Some(victim) => {
                    self.state.shell_outcomes.remove(&victim);
                }
                None => break,
            }
        }
        ShellRepeat { unchanged, seen }
    }

    fn put_blob(&mut self, text: &str) -> String {
        let ref_id = format!("tz://blob/{}", id_for('b', text));
        self.state.blobs.insert(ref_id.clone(), text.to_string());
        self.remember_ref(&ref_id);
        ref_id
    }

    fn put_file(
        &mut self,
        text: &str,
        content_type: ContentType,
        path: Option<&Path>,
        source_start_line: Option<usize>,
        source_end_line: Option<usize>,
    ) -> String {
        let ref_id = recovery_file_ref(text, path);
        self.state.files.insert(
            ref_id.clone(),
            StoredFile {
                ref_id: ref_id.clone(),
                path: path.map(|p| p.to_string_lossy().to_string()),
                path_identity: path.map(path_identity_text),
                text: text.to_string(),
                content_type: content_type.to_string(),
                source_fingerprint: fingerprint_for_stored_payload(
                    path,
                    source_start_line,
                    source_end_line,
                ),
                source_start_line,
                source_end_line,
            },
        );
        self.remember_ref(&ref_id);
        ref_id
    }

    fn index_units(
        &mut self,
        text: &str,
        content_type: ContentType,
        source_ref: &str,
    ) -> Vec<String> {
        let mut refs = Vec::new();
        for (idx, line) in text.lines().enumerate() {
            let stripped = line.trim();
            if stripped.len() >= 12 {
                refs.push(self.put_unit(
                    stripped,
                    content_type,
                    Some(source_ref),
                    Some(idx + 1),
                    Some(idx + 1),
                ));
            }
            if refs.len() >= 64 {
                break;
            }
        }
        refs
    }

    fn put_unit(
        &mut self,
        text: &str,
        content_type: ContentType,
        source_ref: Option<&str>,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> String {
        let ref_id = format!("tz://unit/{}", id_for('u', text));
        self.state
            .units
            .entry(ref_id.clone())
            .or_insert_with(|| StoredUnit {
                ref_id: ref_id.clone(),
                text: text.to_string(),
                content_type: content_type.to_string(),
                source_ref: source_ref.map(str::to_string),
                start_line,
                end_line,
            });
        self.remember_ref(&ref_id);
        ref_id
    }

    fn resolve_ref(&self, kind: &str, bare: &str) -> Option<String> {
        match kind {
            "blob" => self.state.blobs.get(bare).cloned(),
            "file" => self.state.files.get(bare).map(|f| f.text.clone()),
            "unit" => self.state.units.get(bare).map(|u| u.text.clone()),
            "search" => self.state.search_hits.get(bare).map(|u| u.text.clone()),
            _ => None,
        }
    }

    fn file_ref_is_stale(&self, bare: &str) -> bool {
        let Some(stored) = self.state.files.get(bare) else {
            return false;
        };
        let Some(path_text) = stored.path.as_deref() else {
            return false;
        };
        if path_text.starts_with("shell:") || path_text.starts_with("search:") {
            return false;
        }
        let Some(expected) = stored.source_fingerprint.as_ref() else {
            return false;
        };
        let source_path = stored
            .path_identity
            .as_deref()
            .and_then(path_from_identity_text)
            .unwrap_or_else(|| PathBuf::from(path_text));
        source_fingerprint(&source_path).is_none_or(|actual| actual != *expected)
    }

    fn remember_ref(&mut self, ref_id: &str) {
        self.state.order.push(ref_id.to_string());
        self.session_refs.push(ref_id.to_string());
    }

    fn evict(&mut self) {
        evict_prefix(
            &mut self.state.blobs,
            &mut self.state.order,
            "tz://blob/",
            self.config.max_blobs,
        );
        evict_prefix(
            &mut self.state.files,
            &mut self.state.order,
            "tz://file/",
            self.config.max_files,
        );
        evict_prefix(
            &mut self.state.units,
            &mut self.state.order,
            "tz://unit/",
            self.config.max_units,
        );
        evict_prefix(
            &mut self.state.search_hits,
            &mut self.state.order,
            "tz://search/",
            self.config.max_search_hits,
        );
        while self.approx_bytes() > self.config.max_bytes {
            let Some(victim) = self.state.order.iter().find(|r| self.has_ref(r)).cloned() else {
                break;
            };
            self.drop_ref(&victim);
        }
        self.compact_order();
    }

    fn drop_ref(&mut self, ref_id: &str) {
        self.state.blobs.remove(ref_id);
        self.state.files.remove(ref_id);
        self.state.units.remove(ref_id);
        self.state.search_hits.remove(ref_id);
    }

    fn compact_order(&mut self) {
        let live: HashSet<String> = self
            .state
            .blobs
            .keys()
            .chain(self.state.files.keys())
            .chain(self.state.units.keys())
            .chain(self.state.search_hits.keys())
            .cloned()
            .collect();
        let mut seen = HashSet::new();
        self.state
            .order
            .retain(|ref_id| live.contains(ref_id) && seen.insert(ref_id.clone()));
    }

    fn approx_bytes(&self) -> usize {
        let blob_bytes: usize = self.state.blobs.values().map(|v| v.len()).sum();
        let file_bytes: usize = self
            .state
            .files
            .values()
            .map(|v| v.text.len() + v.path.as_deref().unwrap_or_default().len())
            .sum();
        let unit_bytes: usize = self.state.units.values().map(|v| v.text.len()).sum();
        let search_bytes: usize = self.state.search_hits.values().map(|v| v.text.len()).sum();
        blob_bytes + file_bytes + unit_bytes + search_bytes
    }

    fn persist(&mut self) -> Result<(), RecoveryError> {
        let Some(path) = self.persistence_path.clone() else {
            self.evict();
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _lock = PersistLock::acquire(recovery_lock_path(&path))?;
        // Skip the reload+merge only when the file is byte-identical to our
        // last write under this lock: in-memory state is then a superset of
        // disk and authoritative. Any mismatch — another process persisted,
        // the file vanished, metadata is unreadable — falls back to the full
        // merge so multi-process semantics are preserved exactly.
        let unchanged_since_last_write = self
            .disk_identity
            .is_some_and(|identity| DiskIdentity::capture(&path) == Some(identity));
        if !unchanged_since_last_write {
            let existing = load_state(&path, &self.config)?
                .unwrap_or_else(|| RecoveryState::empty(&self.config));
            let current = std::mem::replace(&mut self.state, RecoveryState::empty(&self.config));
            self.state = merge_states(existing, current, &self.session_refs, &self.config);
        }
        self.evict();
        self.disk_identity = None;
        atomic_write_json(&path, &self.state)?;
        self.disk_identity = DiskIdentity::capture(&path);
        self.session_refs.clear();
        Ok(())
    }
}

#[derive(Debug)]
struct ParsedRef {
    kind: String,
    bare: String,
    fragment: Option<String>,
}

fn parse_ref(ref_id: &str) -> Option<ParsedRef> {
    let (bare, fragment) = ref_id
        .split_once('#')
        .map_or((ref_id, None), |(b, f)| (b, Some(f.to_string())));
    let rest = bare.strip_prefix("tz://")?;
    let (kind, id) = rest.split_once('/')?;
    if !matches!(kind, "blob" | "file" | "unit" | "search") || id.is_empty() {
        return None;
    }
    Some(ParsedRef {
        kind: kind.to_string(),
        bare: bare.to_string(),
        fragment,
    })
}

fn parse_line_fragment(fragment: &str) -> (Option<usize>, Option<usize>) {
    let value = fragment.trim().trim_start_matches('L');
    if let Some((start, end)) = value.split_once('-') {
        (
            start.trim_start_matches('L').parse().ok(),
            end.trim_start_matches('L').parse().ok(),
        )
    } else {
        let line = value.parse().ok();
        (line, line)
    }
}

fn parse_around_selector(value: &str) -> (Option<usize>, Option<usize>) {
    let (line_text, radius_text) = value
        .split_once(':')
        .or_else(|| value.split_once(','))
        .unwrap_or((value, "3"));
    let line = line_text
        .trim()
        .trim_start_matches('L')
        .parse::<usize>()
        .unwrap_or(1);
    let radius = radius_text.trim().parse::<usize>().unwrap_or(3);
    (
        Some(line.saturating_sub(radius).max(1)),
        Some(line.saturating_add(radius)),
    )
}

/// Exact line slice for recovery: returns the verbatim bytes of lines
/// `start..=end` (1-based, inclusive), preserving each line's trailing
/// newline — including a trailing blank line — so `expand` is byte-exact.
/// Unlike `tokenzero_core::line_range` (display: drops trailing newlines),
/// this is the recovery path and must reproduce the original bytes.
fn line_slice_exact(text: &str, start: usize, end: usize) -> String {
    let start = start.max(1);
    let end = end.max(start);
    let segments: Vec<&str> = text.split_inclusive('\n').collect();
    if start > segments.len() {
        return String::new();
    }
    let lo = start - 1;
    let hi = end.min(segments.len());
    segments[lo..hi].concat()
}

fn select_content(
    content: &str,
    selector: Option<&str>,
    start_line: Option<usize>,
    end_line: Option<usize>,
    anchor_kind: Option<&str>,
    symbol: Option<&str>,
) -> String {
    let mut selected_start = start_line;
    let mut selected_end = end_line;
    let mut selected_symbol = symbol.map(str::to_string);
    let mut selected_anchor = anchor_kind.map(str::to_string);
    match selector {
        Some("raw") | None => {}
        Some("error_block") => return error_block(content, 3),
        Some("summary") => return tokenzero_core::summarize_lines(content, 12, 8, ""),
        Some(value) if value.starts_with("anchor:") => {
            selected_anchor = Some(value["anchor:".len()..].to_string())
        }
        Some(value) if value.starts_with("symbol:") => {
            selected_symbol = Some(value["symbol:".len()..].to_string())
        }
        Some(value) if value.starts_with("range:") => {
            let (start, end) = parse_line_fragment(&value["range:".len()..]);
            selected_start = start;
            selected_end = end;
        }
        Some(value) if value.starts_with("lines:") => {
            let (start, end) = parse_line_fragment(&value["lines:".len()..]);
            selected_start = start;
            selected_end = end;
        }
        Some(value) if value.starts_with("line:") => {
            let (start, end) = parse_line_fragment(&value["line:".len()..]);
            selected_start = start;
            selected_end = end;
        }
        Some(value) if value.starts_with("around:") => {
            let (start, end) = parse_around_selector(&value["around:".len()..]);
            selected_start = start;
            selected_end = end;
        }
        Some(_) => {}
    }
    if let Some(start) = selected_start {
        return line_slice_exact(content, start, selected_end.unwrap_or(start));
    }
    if let Some(symbol) = selected_symbol {
        return symbol_block(content, &symbol);
    }
    if selected_anchor.is_some() {
        return content
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with("fn ")
                    || trimmed.starts_with("def ")
                    || trimmed.starts_with("class ")
                    || trimmed.starts_with("struct ")
                    || trimmed.starts_with("impl ")
                    || trimmed.starts_with("use ")
                    || trimmed.starts_with("import ")
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    content.to_string()
}

fn load_state(
    path: &Path,
    config: &RecoveryConfig,
) -> Result<Option<RecoveryState>, RecoveryError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Ok(None);
    }
    // Compare as u64 so a file larger than usize can't truncate and slip past
    // the load-size guard on 32-bit targets (which would risk an OOM on read).
    if meta.len() > config.max_load_bytes as u64 {
        return Ok(None);
    }
    let Some(text) = read_limited_utf8(file, config.max_load_bytes)? else {
        return Ok(None);
    };
    let Ok(mut state) = serde_json::from_str::<RecoveryState>(&text) else {
        return Ok(None);
    };
    state.max_blobs = config.max_blobs;
    state.max_files = config.max_files;
    state.max_units = config.max_units;
    state.max_search_hits = config.max_search_hits;
    state.max_bytes = config.max_bytes;
    Ok(Some(state))
}

fn read_limited_utf8<R: Read>(
    reader: R,
    max_load_bytes: usize,
) -> Result<Option<String>, RecoveryError> {
    let mut limited = reader.take((max_load_bytes as u64).saturating_add(1));
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes)?;
    if bytes.len() > max_load_bytes {
        return Ok(None);
    }
    Ok(String::from_utf8(bytes).ok())
}

fn merge_states(
    existing: RecoveryState,
    current: RecoveryState,
    session_refs: &[String],
    config: &RecoveryConfig,
) -> RecoveryState {
    let session: HashSet<&str> = session_refs.iter().map(String::as_str).collect();
    let mut merged = existing;
    for (ref_id, value) in current.blobs {
        if session.contains(ref_id.as_str()) || merged.blobs.contains_key(&ref_id) {
            merged.blobs.insert(ref_id, value);
        }
    }
    for (ref_id, value) in current.files {
        if session.contains(ref_id.as_str()) || merged.files.contains_key(&ref_id) {
            merged.files.insert(ref_id, value);
        }
    }
    for (ref_id, value) in current.units {
        if session.contains(ref_id.as_str()) || merged.units.contains_key(&ref_id) {
            merged.units.insert(ref_id, value);
        }
    }
    for (ref_id, value) in current.search_hits {
        if session.contains(ref_id.as_str()) || merged.search_hits.contains_key(&ref_id) {
            merged.search_hits.insert(ref_id, value);
        }
    }
    for (alias, target) in current.aliases {
        merged.aliases.insert(alias, target);
    }
    merged.order.extend(session_refs.iter().cloned());
    let mut seen = HashSet::new();
    merged.order.retain(|ref_id| seen.insert(ref_id.clone()));
    // Shell outcomes: this session's observations win per key; the verdict
    // itself is always recomputed from a content hash at record time, so a
    // merge can never fabricate an "unchanged" result.
    merged.shell_outcome_seq = merged.shell_outcome_seq.max(current.shell_outcome_seq);
    for (key, outcome) in current.shell_outcomes {
        merged.shell_outcomes.insert(key, outcome);
    }
    while merged.shell_outcomes.len() > MAX_SHELL_OUTCOMES {
        let victim = merged
            .shell_outcomes
            .iter()
            .min_by_key(|(_, outcome)| outcome.seq)
            .map(|(key, _)| key.clone());
        match victim {
            Some(victim) => {
                merged.shell_outcomes.remove(&victim);
            }
            None => break,
        }
    }
    merged.max_blobs = config.max_blobs;
    merged.max_files = config.max_files;
    merged.max_units = config.max_units;
    merged.max_search_hits = config.max_search_hits;
    merged.max_bytes = config.max_bytes;
    merged
}

fn evict_prefix<T>(
    items: &mut BTreeMap<String, T>,
    order: &mut Vec<String>,
    prefix: &str,
    limit: usize,
) {
    if items.len() <= limit {
        return;
    }

    let excess = items.len() - limit;
    let mut victims = Vec::with_capacity(excess);
    let mut victim_set = HashSet::with_capacity(excess);

    for ref_id in order.iter() {
        if victims.len() == excess {
            break;
        }
        if ref_id.starts_with(prefix)
            && items.contains_key(ref_id)
            && victim_set.insert(ref_id.clone())
        {
            victims.push(ref_id.clone());
        }
    }

    if victims.len() < excess {
        for ref_id in items.keys() {
            if victims.len() == excess {
                break;
            }
            if victim_set.insert(ref_id.clone()) {
                victims.push(ref_id.clone());
            }
        }
    }

    for victim in &victims {
        items.remove(victim);
    }
    order.retain(|item| !victim_set.contains(item));
}

fn atomic_write_json(path: &Path, state: &RecoveryState) -> Result<(), RecoveryError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut last_collision = None;
    for _ in 0..TMP_RETRIES {
        let tmp = recovery_tmp_path(path);
        match write_json_to_tmp(&tmp, state) {
            Ok(()) => {
                if let Err(err) = fs::rename(&tmp, path) {
                    // Best-effort cleanup: the rename error is the one worth
                    // surfacing; a failed unlink only strands a temp file.
                    let _ = fs::remove_file(&tmp);
                    return Err(err.into());
                }
                // No parent-dir fsync after rename: see write_json_to_tmp.
                // Atomicity (no torn read) is provided by rename itself;
                // power-loss durability of the rename is out of scope for a
                // reconstructible cache.
                return Ok(());
            }
            Err(RecoveryError::Io(err)) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                last_collision = Some(err);
            }
            Err(err) => {
                // Best-effort cleanup; propagate the write error unmasked.
                let _ = fs::remove_file(&tmp);
                return Err(err);
            }
        }
    }
    Err(last_collision
        .unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "could not allocate recovery temp file for {} after {TMP_RETRIES} attempts",
                    path.display()
                ),
            )
        })
        .into())
}

fn write_json_to_tmp(tmp: &Path, state: &RecoveryState) -> Result<(), RecoveryError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    // The cache holds the exact bytes of everything served for this
    // workspace; keep it owner-only. The atomic rename preserves the mode.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(tmp)?;
    // Buffer the serializer: serde_json::to_writer on a raw File issues one
    // write(2) per JSON fragment, which profiled as 95% of warm-op wall time
    // (tests/artifacts/perf/2026-06-11-pushmax). Identical bytes, ~3 syscalls.
    let mut writer = std::io::BufWriter::with_capacity(1 << 20, file);
    serde_json::to_writer(&mut writer, state)?;
    writer.write_all(b"\n")?;
    let file = writer
        .into_inner()
        .map_err(std::io::IntoInnerError::into_error)?;
    // sync_data, not sync_all: the cache is reconstructible working state
    // (a lost entry reports dangling-ref on expand, never wrong bytes; see
    // docs/racc.md), so file metadata durability is not required — only that
    // the tmp file's *bytes* are on disk before the rename publishes it, so a
    // reader can never observe a torn/partial cache. On macOS sync_all maps to
    // F_FULLFSYNC (full device flush, ~8ms pair); sync_data is fdatasync and
    // skips it (~16x faster) while still ordering the data write ahead of the
    // rename.
    file.sync_data()?;
    Ok(())
}

// No sync_parent_dir: the directory entry created by rename is not flushed.
// Crash *consistency* does not need it — rename is atomic within a single
// directory, so a concurrent or post-crash reader sees either the old inode or
// the new one, never a torn file. Only power-loss *durability* of the rename
// would need a parent-dir fsync, and that is explicitly out of scope here: the
// cache is reconstructible working state. On macOS this fsync was a second
// F_FULLFSYNC, doubling persist cost for a guarantee the cache does not claim.
// Accepted power-loss window: a crash in the unflushed interval can revert the
// cache to the previous consistent state, degrading affected refs to
// dangling-ref — the designed-safe outcome (docs/racc.md), never wrong bytes.

fn recovery_file_ref(text: &str, path: Option<&Path>) -> String {
    let path_identity = path.map(path_identity_text).unwrap_or_default();
    format!(
        "tz://file/{}",
        id_for('f', &format!("{path_identity}:{text}"))
    )
}

#[cfg(unix)]
fn path_identity_text(path: &Path) -> String {
    use std::fmt::Write as _;
    use std::os::unix::ffi::OsStrExt;

    let mut encoded = String::from("unix:");
    for byte in path.as_os_str().as_bytes() {
        write!(&mut encoded, "{byte:02x}").expect("writing hex bytes into String cannot fail");
    }
    encoded
}

#[cfg(unix)]
fn path_from_identity_text(identity: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    let bytes = decode_hex_bytes(identity.strip_prefix("unix:")?)?;
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(windows)]
fn path_identity_text(path: &Path) -> String {
    use std::fmt::Write as _;
    use std::os::windows::ffi::OsStrExt;

    let mut encoded = String::from("windows:");
    for unit in path.as_os_str().encode_wide() {
        write!(&mut encoded, "{unit:04x}").expect("writing hex units into String cannot fail");
    }
    encoded
}

#[cfg(windows)]
fn path_from_identity_text(identity: &str) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    let bytes = decode_hex_bytes(identity.strip_prefix("windows:")?)?;
    let mut chunks = bytes.chunks_exact(2);
    let units = chunks
        .by_ref()
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    if !chunks.remainder().is_empty() {
        return None;
    }
    Some(PathBuf::from(OsString::from_wide(&units)))
}

#[cfg(not(any(unix, windows)))]
fn path_identity_text(path: &Path) -> String {
    format!("display:{}", path.to_string_lossy())
}

#[cfg(not(any(unix, windows)))]
fn path_from_identity_text(identity: &str) -> Option<PathBuf> {
    Some(PathBuf::from(identity.strip_prefix("display:")?))
}

fn decode_hex_bytes(hex: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut chunks = hex.as_bytes().chunks_exact(2);
    for pair in chunks.by_ref() {
        let high = hex_value(pair[0])?;
        let low = hex_value(pair[1])?;
        bytes.push((high << 4) | low);
    }
    if !chunks.remainder().is_empty() {
        return None;
    }
    Some(bytes)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn recovery_lock_path(path: &Path) -> PathBuf {
    append_file_name_suffix(path, ".lock")
}

fn recovery_tmp_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp_name = OsString::from(".");
    tmp_name.push(
        path.file_name()
            .map(OsString::from)
            .unwrap_or_else(|| OsString::from("recovery")),
    );
    let nonce = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    tmp_name.push(format!(".{}.{nonce}.tmp", std::process::id()));
    parent.join(tmp_name)
}

/// Age after which an abandoned atomic-write temp file is reclaimable. A
/// live persist holds its temp file for milliseconds; an hour-old one belongs
/// to a process that died mid-write.
pub const STALE_TMP_MAX_AGE: Duration = Duration::from_secs(60 * 60);

/// Outcome of a stale temp-file sweep. With `dry_run`, `removed*` counts what
/// would be reclaimed without unlinking anything.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TmpSweepReport {
    pub dry_run: bool,
    pub scanned: usize,
    pub removed: usize,
    pub removed_bytes: u64,
    pub failed: usize,
}

/// Remove abandoned atomic-write temp files left beside the recovery cache by
/// processes that died mid-persist. Both the current hidden
/// `.{name}.{pid}.{n}.tmp` shape and the pre-1.0 visible `{name}.*.tmp` shape
/// are matched; the `.lock` anchor never is. Only files older than `max_age`
/// are touched, so an in-flight writer is never raced. Fail-open: per-file
/// failures are counted, a missing directory is an empty report.
pub fn sweep_stale_tmp_files(
    cache_path: &Path,
    max_age: Duration,
    dry_run: bool,
) -> TmpSweepReport {
    let mut report = TmpSweepReport {
        dry_run,
        ..TmpSweepReport::default()
    };
    let Some(parent) = cache_path.parent() else {
        return report;
    };
    let Some(cache_name) = cache_path.file_name().and_then(|name| name.to_str()) else {
        return report;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return report;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".tmp") || !name.contains(cache_name) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        report.scanned += 1;
        let expired = meta
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .map(|age| age > max_age)
            .unwrap_or(false);
        if !expired {
            continue;
        }
        if dry_run || fs::remove_file(&path).is_ok() {
            report.removed += 1;
            report.removed_bytes += meta.len();
        } else {
            report.failed += 1;
        }
    }
    report
}

fn append_file_name_suffix(path: &Path, suffix: &str) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut file_name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("recovery"));
    file_name.push(suffix);
    parent.join(file_name)
}

struct PersistLock {
    file: fs::File,
}

impl PersistLock {
    fn acquire(path: PathBuf) -> Result<Self, RecoveryError> {
        Self::acquire_with_retries(path, LOCK_RETRIES)
    }

    fn acquire_with_retries(path: PathBuf, retries: usize) -> Result<Self, RecoveryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        for attempt in 0..retries {
            match FileExt::try_lock(&file) {
                Ok(()) => {
                    // SAFETY: This file is a stable OS-lock anchor. Do not
                    // unlink it while holding or releasing the lock: replacing
                    // the inode lets a second process lock the replacement
                    // while this process still owns the original.
                    //
                    // No sync_all here: flock is a VFS-level kernel lock that
                    // releases on process death regardless of on-disk
                    // durability. The PID written below is diagnostic, not
                    // required for correctness. Skipping the fsync saves
                    // ~5-15ms per persist on macOS where sync_all =
                    // F_FULLFSYNC.
                    file.set_len(0)?;
                    writeln!(file, "{}", std::process::id())?;
                    return Ok(Self { file });
                }
                Err(TryLockError::WouldBlock) => {
                    if attempt + 1 < retries {
                        thread::sleep(LOCK_RETRY_DELAY);
                    }
                }
                Err(TryLockError::Error(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if attempt + 1 < retries {
                        thread::sleep(LOCK_RETRY_DELAY);
                    }
                }
                Err(TryLockError::Error(err)) => return Err(err.into()),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("timed out waiting for lock {}", path.display()),
        )
        .into())
    }
}

impl Drop for PersistLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn fingerprint_for_stored_payload(
    path: Option<&Path>,
    source_start_line: Option<usize>,
    source_end_line: Option<usize>,
) -> Option<SourceFingerprint> {
    if source_start_line.is_some() || source_end_line.is_some() {
        return None;
    }
    let path = path?;
    let path_text = path.to_string_lossy();
    if path_text.starts_with("shell:") || path_text.starts_with("search:") {
        return None;
    }
    source_fingerprint(path)
}

fn source_fingerprint(path: &Path) -> Option<SourceFingerprint> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let sha256 = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    Some(SourceFingerprint {
        size: meta.len(),
        mtime_ns,
        sha256,
    })
}

#[cfg(test)]
mod tests;
