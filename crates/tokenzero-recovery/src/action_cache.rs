//! Segmented ActionCache index: key -> ref + optional sibling pointers.
//!
//! TokenZero owns the key and artifact ref. FSZero bookmarks and GraphZero
//! dep-closures are stored when present and stay `None` until those surfaces
//! exist. Live and in-grace tombstone entries are GC roots; `serve` pins
//! in-flight artifacts so concurrent eviction cannot dangle a returned ref.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::store_schema::{
    SchemaSkewError, StoreSchemaStamp, StoreSchemaVersion, admit_store_schema,
    recover_actioncache_segment, write_actioncache_segment,
};

pub const ACTIONCACHE_REL_DIR: &str = "tokenzero/actions";
/// Grace between index tombstone and CAS blob delete (tokenzero-gvxc).
pub const ACTIONCACHE_GC_GRACE_SECS: u64 = 60;

/// RAII pin: a live serve holds the artifact until drop.
#[derive(Debug)]
pub struct ServedArtifact {
    path: PathBuf,
}

impl Drop for ServedArtifact {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Result of index-before-CAS eviction planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobEvictionPlan {
    pub artifact_ref: String,
    pub tombstoned_keys: Vec<String>,
    pub waiting_grace: Vec<String>,
    pub may_delete_blob: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionCacheEntry {
    pub key: String,
    #[serde(rename = "ref")]
    pub artifact_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fszero_bookmark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dep_closure_ref: Option<String>,
    pub class: String,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tombstone: bool,
    /// Unix seconds when the entry was tombstoned. Required before CAS delete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tombstoned_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActionCacheSegment {
    schema: String,
    major: u16,
    minor: u16,
    entry: ActionCacheEntry,
}

#[derive(Debug)]
pub enum ActionCacheError {
    Io(io::Error),
    Json(serde_json::Error),
    Schema(SchemaSkewError),
    InvalidKey(String),
}

impl std::fmt::Display for ActionCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "actioncache io: {err}"),
            Self::Json(err) => write!(f, "actioncache json: {err}"),
            Self::Schema(err) => write!(f, "actioncache schema: {err}"),
            Self::InvalidKey(key) => write!(f, "actioncache key {key:?} is not 64 lowercase hex"),
        }
    }
}

impl std::error::Error for ActionCacheError {}

impl From<io::Error> for ActionCacheError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ActionCacheError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

/// On-disk ActionCache under `<store_root>/tokenzero/actions/`.
#[derive(Debug, Clone)]
pub struct ActionCacheIndex {
    root: PathBuf,
}

impl ActionCacheIndex {
    pub fn open(store_root: impl Into<PathBuf>) -> Self {
        Self {
            root: store_root.into().join(ACTIONCACHE_REL_DIR),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put(&self, entry: ActionCacheEntry) -> Result<(), ActionCacheError> {
        validate_key(&entry.key)?;
        let stamp = StoreSchemaVersion::CURRENT.stamp();
        let segment = ActionCacheSegment {
            schema: stamp.schema.to_string(),
            major: stamp.major,
            minor: stamp.minor,
            entry,
        };
        let bytes = serde_json::to_vec(&segment)?;
        write_actioncache_segment(&self.segment_path(&segment.entry.key), &bytes)?;
        Ok(())
    }

    /// Pin a live entry for serve. GC cannot delete the artifact while
    /// the returned guard is held.
    pub fn serve(
        &self,
        key: &str,
    ) -> Result<Option<(ActionCacheEntry, ServedArtifact)>, ActionCacheError> {
        let Some(entry) = self.get(key)? else {
            return Ok(None);
        };
        let path = self.serve_path(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, b"1")?;
        if self.get(key)?.is_none() {
            let _ = fs::remove_file(&path);
            return Ok(None);
        }
        Ok(Some((entry, ServedArtifact { path })))
    }

    pub fn get(&self, key: &str) -> Result<Option<ActionCacheEntry>, ActionCacheError> {
        validate_key(key)?;
        let path = self.segment_path(key);
        recover_actioncache_segment(&path)?;
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path)?;
        let segment: ActionCacheSegment = serde_json::from_slice(&bytes)?;
        admit_loaded_stamp(&segment)?;
        if segment.entry.tombstone {
            return Ok(None);
        }
        Ok(Some(segment.entry))
    }

    pub fn tombstone(&self, key: &str) -> Result<bool, ActionCacheError> {
        self.tombstone_at(key, unix_now())
    }

    pub fn tombstone_at(&self, key: &str, now_unix: u64) -> Result<bool, ActionCacheError> {
        let Some(mut entry) = self.get(key)? else {
            return Ok(false);
        };
        entry.tombstone = true;
        entry.tombstoned_at_unix = Some(now_unix);
        self.put(entry)?;
        Ok(true)
    }

    /// Eviction ordering: tombstone index entries first. The blob may be
    /// deleted only after every referencing entry is tombstoned and grace
    /// has elapsed.
    pub fn prepare_blob_eviction(
        &self,
        artifact_ref: &str,
        now_unix: u64,
        grace_secs: u64,
    ) -> Result<BlobEvictionPlan, ActionCacheError> {
        let live = self.keys_for_artifact(artifact_ref, false)?;
        let mut tombstoned_keys = Vec::new();
        for key in &live {
            if self.tombstone_at(key, now_unix)? {
                tombstoned_keys.push(key.clone());
            }
        }
        let mut waiting_grace = Vec::new();
        let mut may_delete_blob = true;
        for key in self.keys_for_artifact(artifact_ref, true)? {
            if self.serve_path(&key).exists() {
                may_delete_blob = false;
            }
        }
        for key in self.keys_for_artifact(artifact_ref, true)? {
            let Some(entry) = self.load_raw(&key)? else {
                continue;
            };
            if !entry.tombstone {
                may_delete_blob = false;
                continue;
            }
            match entry.tombstoned_at_unix {
                Some(at) if now_unix.saturating_sub(at) >= grace_secs => {}
                _ => {
                    may_delete_blob = false;
                    waiting_grace.push(key);
                }
            }
        }
        Ok(BlobEvictionPlan {
            artifact_ref: artifact_ref.to_string(),
            tombstoned_keys,
            waiting_grace,
            may_delete_blob,
        })
    }

    pub fn keys_for_artifact(
        &self,
        artifact_ref: &str,
        include_tombstones: bool,
    ) -> Result<Vec<String>, ActionCacheError> {
        let mut keys = Vec::new();
        for key in self.all_keys()? {
            let Some(entry) = self.load_raw(&key)? else {
                continue;
            };
            if entry.artifact_ref != artifact_ref {
                continue;
            }
            if entry.tombstone && !include_tombstones {
                continue;
            }
            keys.push(key);
        }
        keys.sort();
        Ok(keys)
    }

    fn load_raw(&self, key: &str) -> Result<Option<ActionCacheEntry>, ActionCacheError> {
        validate_key(key)?;
        let path = self.segment_path(key);
        recover_actioncache_segment(&path)?;
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path)?;
        let segment: ActionCacheSegment = serde_json::from_slice(&bytes)?;
        admit_loaded_stamp(&segment)?;
        Ok(Some(segment.entry))
    }

    fn all_keys(&self) -> Result<Vec<String>, ActionCacheError> {
        let mut keys = Vec::new();
        if !self.root.exists() {
            return Ok(keys);
        }
        for shard in fs::read_dir(&self.root)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
                continue;
            }
            if shard.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            for file in fs::read_dir(shard.path())? {
                let file = file?;
                let name = file.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                let Some(key) = name.strip_suffix(".json") else {
                    continue;
                };
                if validate_key(key).is_ok() {
                    keys.push(key.to_string());
                }
            }
        }
        keys.sort();
        Ok(keys)
    }

    /// Live artifact refs for GC root-set consumption.
    pub fn live_artifact_refs(&self) -> Result<Vec<String>, ActionCacheError> {
        let mut refs = Vec::new();
        for key in self.live_keys()? {
            if let Some(entry) = self.get(&key)? {
                refs.push(entry.artifact_ref);
            }
        }
        refs.sort();
        refs.dedup();
        Ok(refs)
    }

    pub fn protects_hash(
        &self,
        full_hash: &str,
        now_unix: u64,
        grace_secs: u64,
    ) -> Result<bool, ActionCacheError> {
        for key in self.all_keys()? {
            let Some(entry) = self.load_raw(&key)? else {
                continue;
            };
            if artifact_full_hash(&entry.artifact_ref) != Some(full_hash) {
                continue;
            }
            if !entry.tombstone {
                return Ok(true);
            }
            match entry.tombstoned_at_unix {
                Some(at) if now_unix.saturating_sub(at) >= grace_secs => {}
                _ => return Ok(true),
            }
        }
        Ok(false)
    }

    pub fn live_keys(&self) -> Result<Vec<String>, ActionCacheError> {
        let mut keys = Vec::new();
        for key in self.all_keys()? {
            if self.get(&key)?.is_some() {
                keys.push(key);
            }
        }
        Ok(keys)
    }

    fn segment_path(&self, key: &str) -> PathBuf {
        self.root.join(&key[..2]).join(format!("{key}.json"))
    }

    fn serve_path(&self, key: &str) -> PathBuf {
        self.root.join(".serves").join(key)
    }

    pub fn has_in_flight_serve(&self, key: &str) -> bool {
        validate_key(key).is_ok() && self.serve_path(key).exists()
    }
}

fn admit_loaded_stamp(segment: &ActionCacheSegment) -> Result<(), ActionCacheError> {
    if segment.schema != crate::store_schema::STORE_SCHEMA_NAME {
        return Err(ActionCacheError::Schema(SchemaSkewError::WrongSchema {
            found: segment.schema.clone(),
        }));
    }
    admit_store_schema(&StoreSchemaStamp {
        schema: crate::store_schema::STORE_SCHEMA_NAME,
        major: segment.major,
        minor: segment.minor,
    })
    .map(|_| ())
    .map_err(ActionCacheError::Schema)
}

/// Full CAS hash protected by a live ActionCache entry.
pub fn artifact_full_hash(artifact_ref: &str) -> Option<&str> {
    let rest = artifact_ref.strip_prefix("tz://blob/")?;
    let ok = rest.len() == 64
        && rest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    ok.then_some(rest)
}

/// Whether ActionCache still protects this CAS hash. Live entries and
/// tombstones inside the grace window both pin. Unreadable indexes report
/// protected so a sweep cannot collect through a damaged root set.
pub fn action_cache_protects_hash(store_root: &Path, full_hash: &str) -> bool {
    let index = ActionCacheIndex::open(store_root);
    index
        .protects_hash(full_hash, unix_now(), ACTIONCACHE_GC_GRACE_SECS)
        .unwrap_or(true)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn validate_key(key: &str) -> Result<(), ActionCacheError> {
    let ok = key.len() == 64
        && key
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    if ok {
        Ok(())
    } else {
        Err(ActionCacheError::InvalidKey(key.to_string()))
    }
}

#[cfg(test)]
#[path = "../../../tests/recovery/inline/action_cache__action_cache_tests.rs"]
mod action_cache_tests;
