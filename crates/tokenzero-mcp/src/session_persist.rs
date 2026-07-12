//! Disk-backed session seen-set (survives MCP process respawn).
//!
//! Gated on `EngineConfig::session_dedup`; `TOKENZERO_MCP_DEDUP=off` skips load
//! and persist entirely. Scoped by `TOKENZERO_SESSION_SCOPE` when set, else a
//! per-cache-store bucket so unrelated engine configurations do not cross-suppress.

use crate::session::{ServeKey, ServedRecord, SessionMemory};
use fs4::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const SESSION_SCOPE_ENV: &str = "TOKENZERO_SESSION_SCOPE";
#[cfg(not(test))]
const REF_INDEX_PATH_ENV: &str = "TOKENZERO_REF_INDEX_PATH";
/// Max served-payload records per scope (aligned with recovery `max_units`).
pub const MAX_SESSION_MEMORY_RECORDS: usize = 2048;

const LOCK_RETRIES: usize = 240;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(25);
const STATE_VERSION: u32 = 2;

#[derive(Debug, Clone)]
pub(crate) struct SessionPersistence {
    path: PathBuf,
    cache_path: PathBuf,
    scope_id: String,
}

impl SessionPersistence {
    pub(crate) fn for_cache(cache_path: &Path, session_dedup: bool) -> Option<Self> {
        if !session_dedup {
            return None;
        }
        let path = session_memory_path(cache_path);
        let scope_id = session_scope_id(cache_path);
        Some(Self {
            path,
            cache_path: cache_path.to_path_buf(),
            scope_id,
        })
    }

    pub(crate) fn load_into(&self, memory: &mut SessionMemory) {
        let Ok(Some(state)) = load_state(&self.path) else {
            return;
        };
        let Some(scope) = state.scopes.get(&self.scope_id) else {
            return;
        };
        let store = tokenzero_recovery::RecoveryStore::new(Some(self.cache_path.clone()));
        let mut records = HashMap::new();
        // v1 has no watermark, so its first resumed turn must serve full. A
        // legacy state remains readable, but its unwatermarked seen-set is not
        // promoted into the v2 delta stream.
        if state.version >= STATE_VERSION {
            for entry in &scope.records {
                // Resume validation is fail-safe: if the content blob was
                // GC'd, forget the entry and force a full resend. The file ref
                // is diagnostic only and may be project-local.
                if !store.has_ref(&entry.record.blob_ref) {
                    continue;
                }
                let key = serve_key_from_persisted(&entry.key);
                records.insert(key, served_record_from_persisted(&entry.record));
            }
        }
        memory.restore_from_persist(
            records,
            scope.rollup.dedup_hits,
            scope.rollup.diff_hits,
            scope.rollup.visible_tokens_saved,
            scope.rollup.diff_tokens_saved,
            scope.session_hwm,
            scope.rollup.full_bytes,
            scope.rollup.delta_bytes,
        );
    }

    pub(crate) fn persist(&self, memory: &SessionMemory) {
        let _ = self.persist_inner(memory);
    }

    fn persist_inner(&self, memory: &SessionMemory) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _lock = SessionPersistLock::acquire(session_lock_path(&self.path))?;
        let mut state = load_state(&self.path)?.unwrap_or_default();
        let rollup = PersistedRollup {
            dedup_hits: memory.rollup_counters().0,
            diff_hits: memory.rollup_counters().1,
            visible_tokens_saved: memory.rollup_counters().2,
            diff_tokens_saved: memory.rollup_counters().3,
            full_bytes: memory.byte_rollup().0,
            delta_bytes: memory.byte_rollup().1,
        };
        let mut records: Vec<PersistedRecordEntry> = memory
            .records_snapshot()
            .iter()
            .enumerate()
            .map(|(idx, (key, record))| PersistedRecordEntry {
                key: persisted_key(key),
                record: persisted_record(record),
                seq: idx as u64 + 1,
            })
            .collect();
        records.sort_by_key(|entry| serde_json::to_string(&entry.key).unwrap_or_default());
        for (idx, entry) in records.iter_mut().enumerate() {
            entry.seq = idx as u64 + 1;
        }
        let mut merged: HashMap<PersistedServeKey, PersistedRecordEntry> = HashMap::new();
        if state.version >= STATE_VERSION {
            if let Some(existing) = state.scopes.get(&self.scope_id) {
                for entry in &existing.records {
                    merged.insert(entry.key.clone(), entry.clone());
                }
            }
        }
        for entry in records {
            merged.insert(entry.key.clone(), entry);
        }
        let mut scoped = PersistedScope {
            records: merged.into_values().collect(),
            rollup,
            session_hwm: memory.session_hwm(),
        };
        scoped
            .records
            .sort_by_key(|entry| serde_json::to_string(&entry.key).unwrap_or_default());
        evict_scope_records(&mut scoped, MAX_SESSION_MEMORY_RECORDS);
        state.scopes.insert(self.scope_id.clone(), scoped);
        atomic_write_json(&self.path, &state)
    }
}

pub(crate) fn session_memory_path(cache_path: &Path) -> PathBuf {
    user_memory_root(cache_path).join("session-memory.json")
}

fn user_memory_root(cache_path: &Path) -> PathBuf {
    #[cfg(test)]
    {
        if let Some(path) = SESSION_ROOT_TEST_OVERRIDE.with(|slot| slot.borrow().clone()) {
            return path;
        }
        return cache_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
    }
    #[cfg(not(test))]
    {
        user_memory_root_from(
            cache_path,
            std::env::var_os(REF_INDEX_PATH_ENV),
            std::env::var_os("HOME"),
        )
    }
}

#[cfg(not(test))]
fn user_memory_root_from(
    cache_path: &Path,
    ref_index_path: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> PathBuf {
    ref_index_path
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".tokenzero").join("ref-index"))
        })
        .unwrap_or_else(|| {
            cache_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        })
}

pub(crate) fn session_scope_id(_cache_path: &Path) -> String {
    if let Ok(value) = std::env::var(SESSION_SCOPE_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    "__user_global__".to_string()
}

#[cfg(test)]
thread_local! {
    static SESSION_ROOT_TEST_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_session_root<R>(root: &Path, f: impl FnOnce() -> R) -> R {
    SESSION_ROOT_TEST_OVERRIDE.with(|slot| {
        let previous = slot.replace(Some(root.to_path_buf()));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        slot.replace(previous);
        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionMemoryState {
    version: u32,
    #[serde(default)]
    scopes: BTreeMap<String, PersistedScope>,
}

impl Default for SessionMemoryState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            scopes: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct PersistedScope {
    #[serde(default)]
    records: Vec<PersistedRecordEntry>,
    #[serde(default)]
    rollup: PersistedRollup,
    /// Monotonic per-scope turn watermark. Missing in v1 means 0/full resend.
    #[serde(default)]
    session_hwm: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRecordEntry {
    key: PersistedServeKey,
    record: PersistedServedRecord,
    #[serde(default)]
    seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PersistedServeKey {
    File {
        path: String,
        start: Option<usize>,
        end: Option<usize>,
    },
    Output {
        tool: String,
        query: String,
        roots: Vec<String>,
    },
    Expand {
        ref_id: String,
        start_line: Option<usize>,
        end_line: Option<usize>,
        selector_norm: String,
        symbol_norm: String,
        anchor_kind_norm: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedServedRecord {
    content_sha256: String,
    blob_ref: String,
    file_ref: String,
    raw_tokens: usize,
    line_count: usize,
    byte_len: usize,
    serve_count: usize,
    #[serde(default)]
    served_at_unix_secs: Option<u64>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct PersistedRollup {
    dedup_hits: usize,
    diff_hits: usize,
    visible_tokens_saved: usize,
    diff_tokens_saved: usize,
    #[serde(default)]
    full_bytes: usize,
    #[serde(default)]
    delta_bytes: usize,
}

fn load_state(path: &Path) -> std::io::Result<Option<SessionMemoryState>> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(None);
    };
    let Ok(mut state) = serde_json::from_str::<SessionMemoryState>(&text) else {
        return Ok(None);
    };
    if state.version == 0 {
        state.version = STATE_VERSION;
    }
    Ok(Some(state))
}

fn evict_scope_records(scope: &mut PersistedScope, limit: usize) {
    if scope.records.len() <= limit {
        return;
    }
    let excess = scope.records.len() - limit;
    scope.records.sort_by_key(|entry| entry.seq);
    scope.records.drain(0..excess);
    for (idx, entry) in scope.records.iter_mut().enumerate() {
        entry.seq = idx as u64 + 1;
    }
}

fn atomic_write_json(path: &Path, state: &SessionMemoryState) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let body = serde_json::to_string_pretty(&SessionMemoryState {
        version: STATE_VERSION,
        scopes: state.scopes.clone(),
    })?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut tmp_file = options.open(&tmp)?;
    tmp_file.write_all(body.as_bytes())?;
    tmp_file.flush()?;
    drop(tmp_file);
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            Err(err)
        }
    }
}
fn session_lock_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("session-memory.json"));
    name.push(".lock");
    path.with_file_name(name)
}

struct SessionPersistLock {
    file: fs::File,
}

impl SessionPersistLock {
    fn acquire(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        for attempt in 0..LOCK_RETRIES {
            match FileExt::try_lock(&file) {
                Ok(()) => {
                    let _ = writeln!(file, "{}", std::process::id());
                    return Ok(Self { file });
                }
                Err(_) if attempt + 1 < LOCK_RETRIES => {
                    std::thread::sleep(LOCK_RETRY_DELAY);
                }
                Err(err) => return Err(err.into()),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            format!("could not acquire session persist lock: {}", path.display()),
        ))
    }
}

impl Drop for SessionPersistLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn persisted_key(key: &ServeKey) -> PersistedServeKey {
    match key {
        ServeKey::File { path, start, end } => PersistedServeKey::File {
            path: path.to_string_lossy().into_owned(),
            start: *start,
            end: *end,
        },
        ServeKey::Output { tool, query, roots } => PersistedServeKey::Output {
            tool: tool.clone(),
            query: query.clone(),
            roots: roots
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        },
        ServeKey::Expand {
            ref_id,
            start_line,
            end_line,
            selector_norm,
            symbol_norm,
            anchor_kind_norm,
        } => PersistedServeKey::Expand {
            ref_id: ref_id.clone(),
            start_line: *start_line,
            end_line: *end_line,
            selector_norm: selector_norm.clone(),
            symbol_norm: symbol_norm.clone(),
            anchor_kind_norm: anchor_kind_norm.clone(),
        },
    }
}

fn serve_key_from_persisted(key: &PersistedServeKey) -> ServeKey {
    match key {
        PersistedServeKey::File { path, start, end } => ServeKey::File {
            path: PathBuf::from(path),
            start: *start,
            end: *end,
        },
        PersistedServeKey::Output { tool, query, roots } => ServeKey::Output {
            tool: tool.clone(),
            query: query.clone(),
            roots: roots.iter().map(PathBuf::from).collect(),
        },
        PersistedServeKey::Expand {
            ref_id,
            start_line,
            end_line,
            selector_norm,
            symbol_norm,
            anchor_kind_norm,
        } => ServeKey::Expand {
            ref_id: ref_id.clone(),
            start_line: *start_line,
            end_line: *end_line,
            selector_norm: selector_norm.clone(),
            symbol_norm: symbol_norm.clone(),
            anchor_kind_norm: anchor_kind_norm.clone(),
        },
    }
}

fn persisted_record(record: &ServedRecord) -> PersistedServedRecord {
    let served_at_unix_secs = record
        .served_at
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs());
    PersistedServedRecord {
        content_sha256: record.content_sha256.clone(),
        blob_ref: record.blob_ref.clone(),
        file_ref: record.file_ref.clone(),
        raw_tokens: record.raw_tokens,
        line_count: record.line_count,
        byte_len: record.byte_len,
        serve_count: record.serve_count,
        served_at_unix_secs,
    }
}

fn served_record_from_persisted(record: &PersistedServedRecord) -> ServedRecord {
    let served_at = record
        .served_at_unix_secs
        .and_then(|secs| UNIX_EPOCH.checked_add(Duration::from_secs(secs)))
        .unwrap_or_else(SystemTime::now);
    ServedRecord {
        content_sha256: record.content_sha256.clone(),
        blob_ref: record.blob_ref.clone(),
        file_ref: record.file_ref.clone(),
        raw_tokens: record.raw_tokens,
        line_count: record.line_count,
        byte_len: record.byte_len,
        served_at,
        serve_count: record.serve_count,
    }
}
