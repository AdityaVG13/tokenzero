//! Canonical shared content-addressed storage (CAS) for ZeroRef v1 blobs.
//!
//! Immutable objects are stored under `<root>/blobs/sha256/<first-two-hex>/<full-hash>`.
//! This adapter implements the shared-CAS tier for full-hash portable refs
//! (`tz://blob/<sha256>` and its `fz`/`gz` aliases). The legacy private JSON
//! recovery store in `RecoveryStore` remains available as a separate read tier
//! for migration.

use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Error taxonomy for the canonical shared CAS.
#[derive(Debug, Error)]
pub enum SharedCasError {
    /// Requested object is not present in the shared CAS.
    #[error("object not found")]
    NotFound,
    /// Underlying storage operation failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Stored object does not match its full-hash identity.
    #[error("corruption: object does not match expected hash")]
    Corruption,
    /// Policy denied access (e.g. not a regular file or size limit exceeded).
    #[error("policy violation")]
    Policy,
    /// Hash string is not a valid 64-character lowercase hex SHA-256.
    #[error("invalid hash: {0}")]
    InvalidHash(String),
}

/// Canonical shared CAS adapter with an injectable root path.
#[derive(Debug, Clone)]
pub struct SharedCas {
    root: PathBuf,
}

impl SharedCas {
    /// Create a shared CAS anchored at `root`. The effective ZeroStack root
    /// determines whether the store is project-local (default) or explicitly
    /// shared.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Resolve the shared CAS store root from a TokenZero cache path, without
    /// requiring the `blobs/` directory to already exist. Unified stores place
    /// the recovery cache at `<store-root>/tokenzero/recovery-cache.json` and
    /// immutable objects at `<store-root>/blobs/...`. Legacy project-private
    /// `.tokenzero` caches do not imply shared-CAS access. Returns `None` for
    /// flat/legacy private caches.
    pub fn resolve_cache_root(cache_path: &Path) -> Option<PathBuf> {
        let engine_dir = cache_path.parent()?;
        if engine_dir.file_name()? != "tokenzero" {
            return None;
        }
        let store_root = engine_dir.parent()?;
        Some(store_root.to_path_buf())
    }

    /// Derive the CAS attachment root for any explicit recovery cache path.
    /// Unified caches use `<store-root>`; flat caches use the cache parent.
    pub fn attach_root_for_cache_path(cache_path: &Path) -> PathBuf {
        Self::resolve_cache_root(cache_path)
            .or_else(|| cache_path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| cache_path.to_path_buf())
    }

    /// Detect the canonical shared CAS for a recovery cache path. Unified
    /// stores attach before `blobs/` exists; flat caches attach once migration
    /// has materialized the CAS directory beside the cache.
    pub fn detect_from_cache_path(cache_path: &Path) -> Option<Self> {
        let unified_root = Self::resolve_cache_root(cache_path);
        let root = unified_root
            .clone()
            .unwrap_or_else(|| Self::attach_root_for_cache_path(cache_path));
        (unified_root.is_some() || root.join("blobs").is_dir()).then(|| Self::new(root))
    }

    /// Return the effective root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Publish immutable bytes to the shared CAS and return the full SHA-256 hash.
    ///
    /// The write is performed by creating a unique sibling temp file, flushing
    /// and syncing it, then renaming it atomically into the canonical path. If
    /// the destination already exists, its content is verified against the
    /// expected digest and length; idempotent success is returned, otherwise
    /// `Corruption`.
    ///
    /// Parent directories are created lazily on first publish so that a
    /// `SharedCas` can be attached to a store root before any `blobs/` exist.
    pub fn publish(&self, bytes: &[u8]) -> Result<String, SharedCasError> {
        let full_hash = sha256_hex(bytes);
        let path = self.object_path(&full_hash);

        if path.exists() {
            return self.verify_existing(&path, bytes, &full_hash);
        }

        let parent = path
            .parent()
            .expect("object path always has a parent directory");
        fs::create_dir_all(parent)?;

        let tmp_path = parent.join(format!(".tmp-{}-{}.blob", full_hash, unique_suffix()));
        let mut tmp = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        tmp.write_all(bytes)?;
        tmp.flush()?;
        tmp.sync_all()?;
        drop(tmp);

        if let Err(err) = fs::rename(&tmp_path, &path) {
            let _ = fs::remove_file(&tmp_path);
            if path.exists() {
                return self.verify_existing(&path, bytes, &full_hash);
            }
            return Err(err.into());
        }

        #[cfg(unix)]
        if let Ok(parent_dir) = File::open(parent) {
            let _ = parent_dir.sync_all();
        }

        Ok(full_hash)
    }

    /// Resolve a full-hash blob from the shared CAS.
    ///
    /// The path must be a regular file, and the returned bytes are verified
    /// against the requested hash. Any mismatch is `Corruption`; there is no
    /// fallback to another store tier.
    pub fn resolve(&self, full_hash: &str) -> Result<Vec<u8>, SharedCasError> {
        self.validate_hash(full_hash)?;
        let path = self.object_path(full_hash);

        let meta = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Err(SharedCasError::NotFound);
            }
            Err(err) => return Err(err.into()),
        };

        if !meta.is_file() {
            return Err(SharedCasError::Policy);
        }

        let mut file = File::open(&path)?;
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        file.read_to_end(&mut bytes)?;

        if bytes.len() as u64 != meta.len() {
            return Err(SharedCasError::Corruption);
        }

        let actual_hash = sha256_hex(&bytes);
        if actual_hash != full_hash {
            return Err(SharedCasError::Corruption);
        }

        Ok(bytes)
    }

    /// Check whether a valid full-hash object exists in the shared CAS without
    /// reading its contents.
    pub fn contains(&self, full_hash: &str) -> bool {
        self.validate_hash(full_hash).is_ok() && self.object_path(full_hash).is_file()
    }

    fn object_path(&self, full_hash: &str) -> PathBuf {
        let prefix = &full_hash[..2];
        self.root
            .join("blobs")
            .join("sha256")
            .join(prefix)
            .join(full_hash)
    }

    fn validate_hash(&self, full_hash: &str) -> Result<(), SharedCasError> {
        if full_hash.len() != 64
            || full_hash
                .bytes()
                .any(|b| !(b'0'..=b'9').contains(&b) && !(b'a'..=b'f').contains(&b))
        {
            return Err(SharedCasError::InvalidHash(full_hash.to_string()));
        }
        Ok(())
    }

    fn verify_existing(
        &self,
        path: &Path,
        expected_bytes: &[u8],
        expected_hash: &str,
    ) -> Result<String, SharedCasError> {
        let meta = fs::metadata(path)?;
        if !meta.is_file() {
            return Err(SharedCasError::Policy);
        }
        if meta.len() != expected_bytes.len() as u64 {
            return Err(SharedCasError::Corruption);
        }

        let mut file = File::open(path)?;
        let mut actual = Vec::with_capacity(meta.len() as usize);
        file.read_to_end(&mut actual)?;

        if actual != expected_bytes || sha256_hex(&actual) != expected_hash {
            return Err(SharedCasError::Corruption);
        }

        Ok(expected_hash.to_string())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn unique_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{}", ts, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let cas = SharedCas::new(dir.path().to_path_buf());
        let bytes = b"hello canonical shared CAS";

        let hash = cas.publish(bytes).unwrap();
        assert_eq!(hash.len(), 64);
        assert!(cas.contains(&hash));

        let resolved = cas.resolve(&hash).unwrap();
        assert_eq!(resolved, bytes);
    }

    #[test]
    fn idempotent_publish() {
        let dir = tempfile::tempdir().unwrap();
        let cas = SharedCas::new(dir.path().to_path_buf());
        let bytes = b"idempotent content";

        let hash1 = cas.publish(bytes).unwrap();
        let hash2 = cas.publish(bytes).unwrap();
        assert_eq!(hash1, hash2);

        let resolved = cas.resolve(&hash1).unwrap();
        assert_eq!(resolved, bytes);
    }

    #[test]
    fn corruption_detected_on_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let cas = SharedCas::new(dir.path().to_path_buf());
        let bytes = b"corrupt me";

        let hash = cas.publish(bytes).unwrap();
        let path = dir
            .path()
            .join("blobs")
            .join("sha256")
            .join(&hash[..2])
            .join(&hash);
        fs::write(&path, b"tampered bytes").unwrap();

        assert!(matches!(
            cas.resolve(&hash),
            Err(SharedCasError::Corruption)
        ));
    }

    #[test]
    fn corruption_detected_on_existing_publish() {
        let dir = tempfile::tempdir().unwrap();
        let cas = SharedCas::new(dir.path().to_path_buf());
        let bytes = b"do not overwrite";

        let hash = cas.publish(bytes).unwrap();
        let path = dir
            .path()
            .join("blobs")
            .join("sha256")
            .join(&hash[..2])
            .join(&hash);
        fs::write(&path, b"different bytes").unwrap();

        assert!(matches!(
            cas.publish(bytes),
            Err(SharedCasError::Corruption)
        ));
    }

    #[test]
    fn invalid_hash_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let cas = SharedCas::new(dir.path().to_path_buf());

        assert!(matches!(
            cas.resolve("not-a-hash"),
            Err(SharedCasError::InvalidHash(_))
        ));
        assert!(matches!(
            cas.resolve("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde"),
            Err(SharedCasError::InvalidHash(_))
        ));
        assert!(matches!(
            cas.resolve("0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF"),
            Err(SharedCasError::InvalidHash(_))
        ));
    }

    #[test]
    fn not_found_for_missing_hash() {
        let dir = tempfile::tempdir().unwrap();
        let cas = SharedCas::new(dir.path().to_path_buf());
        let missing = "0000000000000000000000000000000000000000000000000000000000000000";

        assert!(matches!(
            cas.resolve(missing),
            Err(SharedCasError::NotFound)
        ));
    }

    #[test]
    fn resolve_cache_root_unified_path() {
        let dir = tempfile::tempdir().unwrap();
        let engine_dir = dir.path().join("tokenzero");
        fs::create_dir_all(&engine_dir).unwrap();
        let cache = engine_dir.join("recovery-cache.json");
        // blobs/ does not exist yet — resolver should still work
        let root = SharedCas::resolve_cache_root(&cache);
        assert!(root.is_some());
        assert_eq!(root.unwrap(), dir.path().to_path_buf());
    }

    #[test]
    fn resolve_cache_root_legacy_flat_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join(".tokenzero");
        fs::create_dir_all(&legacy).unwrap();
        let cache = legacy.join("recovery-cache.json");
        let root = SharedCas::resolve_cache_root(&cache);
        assert!(root.is_none());
    }

    #[test]
    fn detect_without_blobs_dir_works() {
        let dir = tempfile::tempdir().unwrap();
        let engine_dir = dir.path().join("tokenzero");
        fs::create_dir_all(&engine_dir).unwrap();
        let cache = engine_dir.join("recovery-cache.json");
        // No blobs/ directory exists yet
        let cas = SharedCas::detect_from_cache_path(&cache);
        assert!(cas.is_some());
        // Publish should lazily create blobs/
        let cas = cas.unwrap();
        let bytes = b"lazy create test";
        let hash = cas.publish(bytes).unwrap();
        assert!(cas.contains(&hash));
    }
}