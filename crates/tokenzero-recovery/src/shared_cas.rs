//! Canonical shared content-addressed storage (CAS) for ZeroRef v1 blobs.
//!
//! Immutable objects live at `<root>/blobs/sha256/<first-two-hex>/<full-hash>`.
//! Shared-CAS tier for full-hash portable refs (`tz://blob/<sha256>` and
//! `fz`/`gz` aliases). Legacy private JSON recovery remains a separate tier.

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
    #[error("object not found")]
    NotFound,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("corruption: object does not match expected hash")]
    Corruption,
    #[error("policy violation")]
    Policy,
    #[error("invalid hash: {0}")]
    InvalidHash(String),
}

/// Canonical shared CAS adapter with an injectable root path.
#[derive(Debug, Clone)]
pub struct SharedCas {
    root: PathBuf,
}

impl SharedCas {
    /// Create a shared CAS anchored at `root`.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Resolve store root from a TokenZero cache path without requiring `blobs/`.
    /// Unified: `<store-root>/tokenzero/recovery-cache.json` → `<store-root>`.
    /// Legacy flat `.tokenzero` caches return `None`.
    pub fn resolve_cache_root(cache_path: &Path) -> Option<PathBuf> {
        let engine_dir = cache_path.parent()?;
        if engine_dir.file_name()? != "tokenzero" {
            return None;
        }
        Some(engine_dir.parent()?.to_path_buf())
    }

    /// Attachment root for any recovery cache path (unified store root or parent).
    pub fn attach_root_for_cache_path(cache_path: &Path) -> PathBuf {
        Self::resolve_cache_root(cache_path)
            .or_else(|| cache_path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| cache_path.to_path_buf())
    }

    /// Sibling engine recovery cache under the same unified root.
    /// Layout `<root>/<engine>/recovery-cache.json`; `None` keeps flat stores isolated.
    pub fn sibling_engine_cache_path(cache_path: &Path, engine: &str) -> Option<PathBuf> {
        const ENGINES: &[&str] = &["tokenzero", "fszero", "graphzero"];
        let engine_dir = cache_path.parent()?;
        let name = engine_dir.file_name()?.to_str()?;
        if !ENGINES.contains(&name) {
            return None;
        }
        Some(engine_dir.parent()?.join(engine).join("recovery-cache.json"))
    }

    /// Detect shared CAS. Unified attaches before `blobs/`; flat needs `blobs/`.
    pub fn detect_from_cache_path(cache_path: &Path) -> Option<Self> {
            let unified_root = Self::resolve_cache_root(cache_path);
            let is_unified = unified_root.is_some();
            let root = unified_root.unwrap_or_else(|| Self::attach_root_for_cache_path(cache_path));
            (is_unified || root.join("blobs").is_dir()).then(|| Self::new(root))
        }

    /// Effective root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Publish immutable bytes; return full SHA-256. Atomic temp + rename.
    /// Existing destinations are byte/hash verified (`Corruption` on mismatch).
    /// Parents are created lazily so attachment can precede `blobs/`.
    pub fn publish(&self, bytes: &[u8]) -> Result<String, SharedCasError> {
        let full_hash = sha256_hex(bytes);
        let path = self.object_path(&full_hash);
        if path.exists() {
            return Self::verify_existing(&path, bytes, &full_hash);
        }
        let parent = path.parent().expect("object path always has a parent directory");
        fs::create_dir_all(parent)?;
        let tmp_path = parent.join(format!(".tmp-{}-{}.blob", full_hash, unique_suffix()));
        {
            let mut tmp = OpenOptions::new().write(true).create_new(true).open(&tmp_path)?;
            tmp.write_all(bytes)?;
            tmp.flush()?;
            tmp.sync_all()?;
        }
        if let Err(err) = fs::rename(&tmp_path, &path) {
            let _ = fs::remove_file(&tmp_path);
            return if path.exists() {
                Self::verify_existing(&path, bytes, &full_hash)
            } else {
                Err(err.into())
            };
        }
        #[cfg(unix)]
        if let Ok(parent_dir) = File::open(parent) {
            let _ = parent_dir.sync_all();
        }
        Ok(full_hash)
    }

    /// Resolve full-hash blob. Regular file only; hash mismatch → `Corruption`.
    pub fn resolve(&self, full_hash: &str) -> Result<Vec<u8>, SharedCasError> {
        self.validate_hash(full_hash)?;
        let path = self.object_path(full_hash);
        let (meta, bytes) = match read_regular_file(&path) {
            Ok(v) => v,
            Err(SharedCasError::Io(err)) if err.kind() == io::ErrorKind::NotFound => {
                return Err(SharedCasError::NotFound);
            }
            Err(err) => return Err(err),
        };
        if bytes.len() as u64 != meta.len() || sha256_hex(&bytes) != full_hash {
            return Err(SharedCasError::Corruption);
        }
        Ok(bytes)
    }

    /// True when a valid full-hash object exists (no content read).
    pub fn contains(&self, full_hash: &str) -> bool {
        self.validate_hash(full_hash).is_ok() && self.object_path(full_hash).is_file()
    }

    fn object_path(&self, full_hash: &str) -> PathBuf {
        self.root
            .join("blobs")
            .join("sha256")
            .join(&full_hash[..2])
            .join(full_hash)
    }

    fn validate_hash(&self, full_hash: &str) -> Result<(), SharedCasError> {
            (full_hash.len() == 64
                && full_hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
            .then_some(())
            .ok_or_else(|| SharedCasError::InvalidHash(full_hash.into()))
        }

    fn verify_existing(
            path: &Path,
            expected_bytes: &[u8],
            expected_hash: &str,
        ) -> Result<String, SharedCasError> {
            let (meta, actual) = read_regular_file(path)?;
            if meta.len() != expected_bytes.len() as u64
                || actual != expected_bytes
                || sha256_hex(&actual) != expected_hash
            {
                return Err(SharedCasError::Corruption);
            }
            Ok(expected_hash.into())
        }
}

fn read_regular_file(path: &Path) -> Result<(std::fs::Metadata, Vec<u8>), SharedCasError> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(SharedCasError::Policy);
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    File::open(path)?.read_to_end(&mut bytes)?;
    Ok((meta, bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn unique_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{ts}-{n}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cas() -> (tempfile::TempDir, SharedCas) {
        let dir = tempfile::tempdir().unwrap();
        let cas = SharedCas::new(dir.path().to_path_buf());
        (dir, cas)
    }

    fn blob_path(root: &Path, hash: &str) -> PathBuf {
        root.join("blobs").join("sha256").join(&hash[..2]).join(hash)
    }

    fn unified_cache(dir: &Path) -> PathBuf {
        let engine = dir.join("tokenzero");
        fs::create_dir_all(&engine).unwrap();
        engine.join("recovery-cache.json")
    }

    #[test]
    fn publish_resolve_matrix() {
        for (label, bytes, again) in [
            ("round_trip", b"hello canonical shared CAS".as_slice(), false),
            ("idempotent_publish", b"idempotent content".as_slice(), true),
        ] {
            let (_d, cas) = temp_cas();
            let hash = cas.publish(bytes).unwrap();
            assert_eq!(hash.len(), 64, "{label}");
            assert!(cas.contains(&hash), "{label}");
            if again {
                assert_eq!(cas.publish(bytes).unwrap(), hash, "{label}");
            }
            assert_eq!(cas.resolve(&hash).unwrap(), bytes, "{label}");
        }
    }

    #[test]
    fn corruption_matrix() {
        for (label, via_resolve, original, tampered) in [
            ("resolve", true, b"corrupt me".as_slice(), b"tampered bytes".as_slice()),
            (
                "existing_publish",
                false,
                b"do not overwrite".as_slice(),
                b"different bytes".as_slice(),
            ),
        ] {
            let (dir, cas) = temp_cas();
            let hash = cas.publish(original).unwrap();
            fs::write(blob_path(dir.path(), &hash), tampered).unwrap();
            let err = if via_resolve {
                cas.resolve(&hash).map(|_| ())
            } else {
                cas.publish(original).map(|_| ())
            };
            assert!(matches!(err, Err(SharedCasError::Corruption)), "{label}: {err:?}");
        }
    }

    #[test]
    fn invalid_hash_and_missing() {
        let (_d, cas) = temp_cas();
        for h in [
            "not-a-hash",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde",
            "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF",
        ] {
            assert!(
                matches!(cas.resolve(h), Err(SharedCasError::InvalidHash(_))),
                "{h}"
            );
        }
        let missing = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(cas.resolve(missing), Err(SharedCasError::NotFound)));
    }

    #[test]
    fn cache_root_detection_matrix() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            SharedCas::resolve_cache_root(&unified_cache(dir.path())).as_deref(),
            Some(dir.path())
        );
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join(".tokenzero");
        fs::create_dir_all(&legacy).unwrap();
        assert!(SharedCas::resolve_cache_root(&legacy.join("recovery-cache.json")).is_none());
        let dir = tempfile::tempdir().unwrap();
        let cas = SharedCas::detect_from_cache_path(&unified_cache(dir.path())).unwrap();
        let hash = cas.publish(b"lazy create test").unwrap();
        assert!(cas.contains(&hash));
    }
}
