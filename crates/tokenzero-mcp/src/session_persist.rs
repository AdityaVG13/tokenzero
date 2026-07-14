//! Disk-backed, per-scope session seen-set.

use crate::session::{ServeKey, ServedRecord, SessionMemory};
use fs4::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SESSION_SCOPE_ENV: &str = "TOKENZERO_SESSION_SCOPE";
#[cfg(not(test))]
const REF_INDEX_PATH_ENV: &str = "TOKENZERO_REF_INDEX_PATH";
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
        session_dedup.then(|| Self {
            path: session_memory_path(cache_path),
            cache_path: cache_path.to_path_buf(),
            scope_id: session_scope_id(cache_path),
        })
    }

    pub(crate) fn load_into(&self, memory: &mut SessionMemory) {
        let Some(state) = load_state(&self.path) else { return };
        let Some(scope) = state.scopes.get(&self.scope_id) else { return };
        let store = tokenzero_recovery::RecoveryStore::new(Some(self.cache_path.clone()));
        // v1 has no watermark, so its first resumed turn must serve full.
        let records = if state.version >= STATE_VERSION {
            scope
                .records
                .iter()
                .filter(|entry| store.has_ref(&entry.record.blob_ref))
                .map(|entry| (entry.key.clone(), entry.record.clone()))
                .collect()
        } else {
            HashMap::new()
        };
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
        let mut state = load_state(&self.path).unwrap_or_default();
        let (dedup_hits, diff_hits, visible_tokens_saved, diff_tokens_saved) =
            memory.rollup_counters();
        let (full_bytes, delta_bytes) = memory.byte_rollup();
        let rollup = PersistedRollup {
            dedup_hits,
            diff_hits,
            visible_tokens_saved,
            diff_tokens_saved,
            full_bytes,
            delta_bytes,
        };
        let mut records: Vec<_> = memory
            .records_snapshot()
            .iter()
            .enumerate()
            .map(|(idx, (key, record))| PersistedRecordEntry {
                key: key.clone(),
                record: record.clone(),
                seq: idx as u64 + 1,
            })
            .collect();
        sort_records(&mut records);
        for (idx, entry) in records.iter_mut().enumerate() {
            entry.seq = idx as u64 + 1;
        }

        let mut merged = HashMap::new();
        if state.version >= STATE_VERSION {
            if let Some(existing) = state.scopes.get(&self.scope_id) {
                merged.extend(
                    existing
                        .records
                        .iter()
                        .cloned()
                        .map(|entry| (entry.key.clone(), entry)),
                );
            }
        }
        merged.extend(records.into_iter().map(|entry| (entry.key.clone(), entry)));
        let mut scope = PersistedScope {
            records: merged.into_values().collect(),
            rollup,
            session_hwm: memory.session_hwm(),
        };
        sort_records(&mut scope.records);
        evict_scope_records(&mut scope, MAX_SESSION_MEMORY_RECORDS);
        state.version = STATE_VERSION;
        state.scopes.insert(self.scope_id.clone(), scope);
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
        cache_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
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
                .map(|home| PathBuf::from(home).join(".tokenzero/ref-index"))
        })
        .unwrap_or_else(|| {
            cache_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        })
}

pub(crate) fn session_scope_id(_cache_path: &Path) -> String {
    std::env::var(SESSION_SCOPE_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "__user_global__".to_owned())
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
        result.unwrap_or_else(|payload| std::panic::resume_unwind(payload))
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
    #[serde(default)]
    session_hwm: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRecordEntry {
    key: ServeKey,
    record: ServedRecord,
    #[serde(default)]
    seq: u64,
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

fn load_state(path: &Path) -> Option<SessionMemoryState> {
    let mut state = serde_json::from_str::<SessionMemoryState>(&fs::read_to_string(path).ok()?).ok()?;
    if state.version == 0 {
        state.version = STATE_VERSION;
    }
    Some(state)
}

fn sort_records(records: &mut [PersistedRecordEntry]) {
    records.sort_by_key(|entry| serde_json::to_string(&entry.key).unwrap_or_default());
}

fn evict_scope_records(scope: &mut PersistedScope, limit: usize) {
    if scope.records.len() <= limit {
        return;
    }
    let excess = scope.records.len() - limit;
    scope.records.sort_by_key(|entry| entry.seq);
    scope.records.drain(..excess);
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
    let body = serde_json::to_string_pretty(state)?;
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
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

fn session_lock_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "session-memory.json".into());
    name.push(".lock");
    path.with_file_name(name)
}

struct SessionPersistLock(fs::File);

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
                    return Ok(Self(file));
                }
                Err(_) if attempt + 1 < LOCK_RETRIES => std::thread::sleep(LOCK_RETRY_DELAY),
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
        let _ = FileExt::unlock(&self.0);
    }
}
