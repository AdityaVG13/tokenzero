//! Segmented ActionCache index: key -> ref + optional sibling pointers.
//!
//! TokenZero owns the key and artifact ref. FSZero bookmarks and GraphZero
//! dep-closures are stored when present and stay `None` until those surfaces
//! exist. Serve is deferred; this is the storage substrate only.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::store_schema::{
    SchemaSkewError, StoreSchemaStamp, StoreSchemaVersion, admit_store_schema,
    recover_actioncache_segment, write_actioncache_segment,
};

pub const ACTIONCACHE_REL_DIR: &str = "tokenzero/actions";

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
        let Some(mut entry) = self.get(key)? else {
            return Ok(false);
        };
        entry.tombstone = true;
        self.put(entry)?;
        Ok(true)
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

    pub fn live_keys(&self) -> Result<Vec<String>, ActionCacheError> {
        let mut keys = Vec::new();
        if !self.root.exists() {
            return Ok(keys);
        }
        for shard in fs::read_dir(&self.root)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
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
                if validate_key(key).is_err() {
                    continue;
                }
                if self.get(key)?.is_some() {
                    keys.push(key.to_string());
                }
            }
        }
        keys.sort();
        Ok(keys)
    }

    fn segment_path(&self, key: &str) -> PathBuf {
        self.root.join(&key[..2]).join(format!("{key}.json"))
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
mod action_cache_tests {
    use super::*;
    use tempfile::tempdir;

    fn key(n: u8) -> String {
        format!("{n:064x}")
    }

    fn entry(n: u8, artifact: &str) -> ActionCacheEntry {
        ActionCacheEntry {
            key: key(n),
            artifact_ref: artifact.to_string(),
            fszero_bookmark: None,
            dep_closure_ref: None,
            class: "must_block_revalidate".into(),
            verified: true,
            world_id: Some("w1".into()),
            tombstone: false,
        }
    }

    #[test]
    fn tzqjfi_put_get_roundtrip_and_tombstone() {
        let dir = tempdir().unwrap();
        let index = ActionCacheIndex::open(dir.path());
        let first = entry(1, "tz://blob/aaa");
        index.put(first.clone()).unwrap();
        assert_eq!(index.get(&key(1)).unwrap().as_ref(), Some(&first));
        assert_eq!(index.live_keys().unwrap(), vec![key(1)]);
        assert_eq!(
            index.live_artifact_refs().unwrap(),
            vec!["tz://blob/aaa".to_string()]
        );

        assert!(index.tombstone(&key(1)).unwrap());
        assert!(index.get(&key(1)).unwrap().is_none());
        assert!(index.live_keys().unwrap().is_empty());
        assert!(index.live_artifact_refs().unwrap().is_empty());
    }

    #[test]
    fn tzqjfi_refuses_newer_major_segment() {
        let dir = tempdir().unwrap();
        let index = ActionCacheIndex::open(dir.path());
        let item = entry(2, "tz://blob/bbb");
        let path = index.segment_path(&item.key);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bad = serde_json::json!({
            "schema": "tokenzero.store",
            "major": 9,
            "minor": 0,
            "entry": item,
        });
        fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
        let err = index.get(&item.key).unwrap_err();
        assert!(
            matches!(err, ActionCacheError::Schema(SchemaSkewError::NewerMajor { found }) if found.major == 9),
            "{err}"
        );
    }

    #[test]
    fn tzqjfi_tokenzero_owned_fields_do_not_require_sibling_pointers() {
        let dir = tempdir().unwrap();
        let index = ActionCacheIndex::open(dir.path());
        let mut item = entry(3, "tz://blob/ccc");
        item.fszero_bookmark = None;
        item.dep_closure_ref = None;
        index.put(item.clone()).unwrap();
        let got = index.get(&key(3)).unwrap().unwrap();
        assert!(got.fszero_bookmark.is_none());
        assert!(got.dep_closure_ref.is_none());
        assert_eq!(got.artifact_ref, "tz://blob/ccc");
        assert!(got.verified);
    }
}
