//! Embeddable TokenZero store handle (tokenzero-lwt).
//!
//! `TokenZeroStore` is a non-global, in-process handle that owns a durable
//! recovery store and an optional shared CAS tier. It is intended for the
//! future single ZeroStack binary where each engine (FSZero, GraphZero,
//! TokenZero) holds its own isolated store handle while sharing the same
//! canonical CAS layout under a single store root.
//!
//! The descriptor and contract produced by an embedded handle are derived
//! from the same RecoveryStore/SharedCas state used by the standalone CLI
//! and MCP sessions, so cross-mode behavior cannot drift.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use tokenzero_core::ContentType;

use crate::RecoveryStore;
use crate::shared_cas::SharedCas;

const DESCRIPTOR_SCHEMA_VERSION: &str = "tokenzero.recovery.capability.v1";
const DESCRIPTOR_VERSION: &str = "1.0.0";

/// Non-global embeddable TokenZero store handle.
///
/// * Two handles with different workspace roots but different store roots are
///   fully isolated in their durable JSON stores and CAS attachment.
/// * Two handles pointing at the same ZeroStack store root share the same
///   canonical CAS layout (`<root>/blobs/sha256/...`).
/// * The capability descriptor is derived from the same RecoveryStore/SharedCas
///   state used by the standalone CLI/MCP session, so embedded and standalone
///   modes advertise identical contracts.
pub struct TokenZeroStore {
    /// Workspace root that this handle answers filesystem ops relative to.
    /// May be `None` for a store-only handle (e.g. a sibling-engine context).
    pub root: Option<PathBuf>,
    recovery: RecoveryStore,
    shared_cas: Option<SharedCas>,
    /// Mirrors the standalone session's durable-degraded flag: true when the
    /// durable store could not be opened and the handle fell back to in-memory.
    pub durable_degraded: bool,
    /// Unique temporary CAS directory for `in_memory()` handles. Cleaned on drop.
    cas_temp_dir: Option<PathBuf>,
}

impl TokenZeroStore {
    /// Open an embedded handle using the same durable-store discovery as the
    /// CLI session: `<root>/.tokenzero/recovery-cache.json` or
    /// `<root>/.zerostack/tokenzero/recovery-cache.json`.
    ///
    /// A shared CAS is attached when the effective path follows the unified
    /// `.zerostack` layout, or when a legacy `.tokenzero` cache has a sibling
    /// `blobs/` directory.
    ///
    /// If the durable store cannot be opened, the handle degrades to an
    /// in-memory store with `durable_degraded` set to `true`.
    pub fn open(root: impl AsRef<Path>) -> Self {
        let root_path = root.as_ref().to_path_buf();
        match Self::try_open(&root_path) {
            Ok(s) => s,
            Err(_e) => Self {
                root: Some(root_path),
                recovery: RecoveryStore::new(None),
                shared_cas: None,
                durable_degraded: true,
                cas_temp_dir: None,
            },
        }
    }

    /// Open the handle, returning `Err` rather than degrading to in-memory.
    pub fn try_open(root: impl AsRef<Path>) -> Result<Self, String> {
        let root_path = root.as_ref().to_path_buf();
        let cache_path = default_recovery_cache_path(&root_path);
        let parent = cache_path
            .parent()
            .ok_or_else(|| "invalid cache path: no parent directory".to_string())?;
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create cache directory {}: {e}", parent.display()))?;
        let recovery = RecoveryStore::new(Some(cache_path));
        let shared_cas = recovery
            .persistence_path
            .as_deref()
            .and_then(SharedCas::detect_from_cache_path);
        Ok(Self {
            root: Some(root_path),
            recovery,
            shared_cas,
            durable_degraded: false,
            cas_temp_dir: None,
        })
    }

    /// Create an in-memory store handle backed by a temporary shared CAS
    /// directory under the process temp dir. Useful for tests and ephemeral
    /// sibling-engine contexts that do not need a durable store.
    ///
    /// The temporary CAS directory is removed when the handle is dropped.
    pub fn in_memory() -> Self {
        let temp_dir = temp_cas_dir();
        let cas = SharedCas::new(temp_dir.clone());
        Self {
            root: None,
            recovery: RecoveryStore::new(None),
            shared_cas: Some(cas),
            durable_degraded: false,
            cas_temp_dir: Some(temp_dir),
        }
    }

    /// Construct a handle with an explicit shared CAS. This is the path a
    /// sibling engine (FSZero, GraphZero) uses to hand off its CAS object so
    /// all engines in a single ZeroStack process publish/resolve to the same
    /// immutable object tier.
    ///
    /// If `root` is provided, a durable recovery cache is opened at the
    /// conventional TokenZero path under that root. If `root` is `None`, the
    /// handle is memory-only for recovery metadata.
    pub fn with_shared_cas(root: Option<PathBuf>, shared_cas: SharedCas) -> Self {
        let recovery = match &root {
            Some(root_path) => {
                let cache_path = default_recovery_cache_path(root_path);
                let _ =
                    std::fs::create_dir_all(cache_path.parent().expect("cache path has a parent"));
                RecoveryStore::new(Some(cache_path))
            }
            None => RecoveryStore::new(None),
        };
        Self {
            root,
            recovery,
            shared_cas: Some(shared_cas),
            durable_degraded: false,
            cas_temp_dir: None,
        }
    }

    /// Access the underlying recovery store. This is an escape hatch; most
    /// callers should use the typed methods on this handle.
    pub fn recovery(&self) -> &RecoveryStore {
        &self.recovery
    }

    /// Mutable access to the underlying recovery store.
    pub fn recovery_mut(&mut self) -> &mut RecoveryStore {
        &mut self.recovery
    }

    /// Borrow the shared CAS attached to this handle, if any.
    pub fn shared_cas(&self) -> Option<&SharedCas> {
        self.shared_cas.as_ref()
    }

    /// Store root for this handle (parent of the recovery cache, or unified
    /// `.zerostack` root). `None` for memory-only handles.
    pub fn store_root(&self) -> Option<PathBuf> {
        let cache_path = self.recovery.persistence_path.as_deref()?;
        Some(store_root_for_cache_path(cache_path))
    }

    /// Persist a byte payload to the shared CAS and return a durable
    /// `tz://blob/<sha256>` portable ref. Honors `max_object_bytes` if
    /// supplied.
    ///
    /// Requires a shared CAS to be attached. Callers that need to publish
    /// without a durable project root should use [`Self::in_memory`] or
    /// [`Self::with_shared_cas`].
    pub fn put(&mut self, bytes: &[u8], max_object_bytes: Option<u64>) -> Result<String, String> {
        if let Some(limit) = max_object_bytes {
            if bytes.len() as u64 > limit {
                return Err(format!(
                    "payload {} bytes exceeds limit {limit}",
                    bytes.len()
                ));
            }
        }
        let cas = self
            .shared_cas
            .as_ref()
            .ok_or_else(|| "no shared CAS attached".to_string())?;
        let hash = cas
            .publish(bytes)
            .map_err(|e| format!("shared CAS publish failed: {e}"))?;
        Ok(format!("tz://blob/{hash}"))
    }

    /// Expand a ref to its exact bytes. Returns `None` for unknown refs.
    ///
    /// Portable `tz://blob/<sha256>` refs (and their `fz://blob/` / `gz://blob/`
    /// aliases) are resolved from the shared CAS when one is attached. Other
    /// refs fall back to the RecoveryStore expand path.
    pub fn expand(&mut self, r: &str) -> Option<Vec<u8>> {
        // Use the handle's explicit shared CAS for portable blob refs first.
        if let Some(cas) = &self.shared_cas {
            if let Some(hash) = portable_blob_hash(r) {
                match cas.resolve(hash) {
                    Ok(bytes) => return Some(bytes),
                    Err(_) => {}
                }
            }
        }
        // Fall back to the RecoveryStore path (legacy store, aliases, ref-index).
        let result = self.recovery.expand(r, Some("raw"), None, None, None, None);
        if result.found {
            Some(result.content.into_bytes())
        } else {
            None
        }
    }

    /// ZeroRef v1 capability descriptor for this handle. Static fields come
    /// from RecoveryStore/SharedCas constants; the `shared_cas` section is
    /// probed live so a caller can distinguish local-only, shared, and
    /// degraded states before routing any payload.
    pub fn capability_descriptor(&self) -> Value {
        let cas_attached = self.shared_cas.is_some();
        let cas_writable = cas_attached; // SharedCas::publish is always writable
        serde_json::json!({
            "schema_version": DESCRIPTOR_SCHEMA_VERSION,
            "descriptor_version": DESCRIPTOR_VERSION,
            "engine": "tokenzero",
            "zeroref_v1": {
                "version": "v1",
                "enabled": true,
                "shared_cas": cas_attached,
                "shared_cas_writable": cas_writable,
                "blob_ref_expand": true,
                "ref_schemes": ["tz://", "fz://", "gz://"],
                "fragment_selectors": ["#B", "#L"],
                "features": [
                    "shared-content-addressable-storage",
                    "blob-ref-expand",
                    "fragment-selectors",
                    "cross-engine"
                ]
            },
            "recovery": {
                "durable": self.recovery.persistence_path.is_some() && !self.durable_degraded,
                "durable_degraded": self.durable_degraded,
                "persistent_path": self.recovery.persistence_path.as_ref().map(|p| p.display().to_string()),
                "store_root": self.store_root().as_ref().map(|p| p.display().to_string())
            }
        })
    }

    /// Store health, root, and CAS summary for telemetry and the single-binary
    /// router. Does not leak absolute private paths.
    pub fn root_report(&self) -> Value {
        let store_root = self
            .store_root()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "memory".to_string());
        let store_db = self
            .recovery
            .persistence_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "memory".to_string());
        let effective_root_mode = if self.recovery.persistence_path.is_none() {
            "memory"
        } else if store_root.contains("/.zerostack") {
            "unified"
        } else {
            "legacy"
        };
        let cas_attached = self.shared_cas.is_some();
        let cap = self.capability_descriptor();
        serde_json::json!({
            "workspace_root": self.root.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(none)".to_string()),
            "store_root": store_root,
            "store_db": store_db,
            "durable_degraded": self.durable_degraded,
            "effective_root_mode": effective_root_mode,
            "store_health": {
                "durable": self.recovery.persistence_path.is_some() && !self.durable_degraded,
                "cas_attached": cas_attached,
                "cas_writable": cas_attached,
            },
            "capabilities": cap,
            "last_integrity_error": null,
        })
    }

    /// Publish the capability descriptor into the recovery store as a JSON blob.
    /// Best-effort; never blocks. The stored blob is content-addressed at
    /// `tz://blob/<sha256>`; callers can compute that ref from the descriptor
    /// or keep the one returned by `store_blob` if they need a stable handle.
    pub fn publish_capabilities(&mut self) {
        let descriptor = self.capability_descriptor().to_string();
        let _ = self
            .recovery
            .store_blob(&descriptor, ContentType::JsonConfig);
    }
}

impl Drop for TokenZeroStore {
    fn drop(&mut self) {
        if let Some(dir) = self.cas_temp_dir.take() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// Default recovery cache path for a workspace root, matching the CLI/MCP
/// convention without reading process env after construction.
///
/// * Unified layout: `<root>/.zerostack/tokenzero/recovery-cache.json`
/// * Legacy layout: `<root>/.tokenzero/recovery-cache.json`
///
/// The unified layout is chosen only when `.zerostack` already exists; this
/// mirrors `workspace::default_recovery_cache_path` but keeps the handle
/// env-independent after construction.
fn default_recovery_cache_path(root: &Path) -> PathBuf {
    let unified = root
        .join(".zerostack")
        .join("tokenzero")
        .join("recovery-cache.json");
    let legacy = root.join(".tokenzero").join("recovery-cache.json");
    if unified.exists() || root.join(".zerostack").is_dir() {
        unified
    } else {
        legacy
    }
}

fn store_root_for_cache_path(cache_path: &Path) -> PathBuf {
    if let Some(engine_dir) = cache_path.parent() {
        if engine_dir.file_name().and_then(|n| n.to_str()) == Some("tokenzero") {
            if let Some(store_root) = engine_dir.parent() {
                return store_root.to_path_buf();
            }
        }
    }
    cache_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| cache_path.to_path_buf())
}

/// Extract the full 64-hex SHA-256 from a portable blob ref, accepting the
/// canonical `tz://blob/<hash>` form and the `fz`/`gz` aliases.
fn portable_blob_hash(ref_id: &str) -> Option<&str> {
    let rest = ref_id
        .strip_prefix("tz://blob/")
        .or_else(|| ref_id.strip_prefix("fz://blob/"))
        .or_else(|| ref_id.strip_prefix("gz://blob/"))?;
    let hash = rest.split_once('#').map_or(rest, |(h, _)| h);
    if hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        Some(hash)
    } else {
        None
    }
}

fn temp_cas_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("tokenzero-in-memory-cas-{ts}-{n}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    #[test]
    fn lifecycle_open_try_open_in_memory() {
        let mem = TokenZeroStore::in_memory();
        assert!(mem.root.is_none());
        assert!(mem.shared_cas().is_some());
        assert!(!mem.durable_degraded);

        let dir = tempdir().unwrap();
        let root = dir.path();
        let opened = TokenZeroStore::open(root);
        assert_eq!(opened.root, Some(root.to_path_buf()));
        assert!(!opened.durable_degraded);

        // try_open on the same root succeeds and sets up the cache directory.
        let tried = TokenZeroStore::try_open(root).unwrap();
        assert!(tried.recovery.persistence_path.is_some());
    }

    #[test]
    fn put_expand_round_trip_byte_exact_via_shared_cas() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Create the unified .zerostack layout so the shared CAS is attached.
        std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();
        let mut store = TokenZeroStore::open(root);
        assert!(
            store.shared_cas().is_some(),
            "shared CAS should be attached in unified layout"
        );

        let bytes = b"hello ZeroRef v1 shared CAS";
        let ref_id = store.put(bytes, None).unwrap();
        assert!(ref_id.starts_with("tz://blob/"));
        assert_eq!(ref_id.len(), "tz://blob/".len() + 64);

        let resolved = store.expand(&ref_id).unwrap();
        assert_eq!(resolved, bytes);

        // Cross-engine alias schemes resolve the same bytes.
        let fz_ref = ref_id.replacen("tz://blob/", "fz://blob/", 1);
        let gz_ref = ref_id.replacen("tz://blob/", "gz://blob/", 1);
        assert_eq!(store.expand(&fz_ref).unwrap(), bytes);
        assert_eq!(store.expand(&gz_ref).unwrap(), bytes);
    }

    #[test]
    fn isolated_roots_do_not_share_cas() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        std::fs::create_dir_all(a.path().join(".zerostack").join("tokenzero")).unwrap();
        std::fs::create_dir_all(b.path().join(".zerostack").join("tokenzero")).unwrap();

        let mut store_a = TokenZeroStore::open(a.path());
        let mut store_b = TokenZeroStore::open(b.path());
        let bytes = b"isolated payload";
        let ref_a = store_a.put(bytes, None).unwrap();

        // Store B should not resolve the ref because it points at a different CAS.
        assert!(store_b.expand(&ref_a).is_none());
    }

    #[test]
    fn shared_root_shares_cas_between_handles() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();

        let mut first = TokenZeroStore::open(root);
        let mut second = TokenZeroStore::open(root);
        let bytes = b"shared root payload";
        let ref_id = first.put(bytes, None).unwrap();

        assert_eq!(second.expand(&ref_id).unwrap(), bytes);
    }

    #[test]
    fn explicit_shared_cas_is_shared_across_handles() {
        let cas_dir = tempdir().unwrap();
        let cas = SharedCas::new(cas_dir.path().to_path_buf());

        let mut first = TokenZeroStore::with_shared_cas(None, cas.clone());
        let mut second = TokenZeroStore::with_shared_cas(None, cas);
        let bytes = b"explicit shared CAS payload";
        let ref_id = first.put(bytes, None).unwrap();

        assert_eq!(second.expand(&ref_id).unwrap(), bytes);
    }

    #[test]
    fn capability_descriptor_is_valid_and_matches_state() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();
        let store = TokenZeroStore::open(root);

        let cap = store.capability_descriptor();
        assert_eq!(cap["schema_version"], DESCRIPTOR_SCHEMA_VERSION);
        assert_eq!(cap["descriptor_version"], DESCRIPTOR_VERSION);
        assert_eq!(cap["engine"], "tokenzero");
        assert_eq!(cap["zeroref_v1"]["version"], "v1");
        assert!(cap["zeroref_v1"]["enabled"].as_bool().unwrap());
        assert!(cap["zeroref_v1"]["shared_cas"].as_bool().unwrap());
        assert!(cap["zeroref_v1"]["shared_cas_writable"].as_bool().unwrap());
        assert!(cap["zeroref_v1"]["blob_ref_expand"].as_bool().unwrap());
        let schemes = cap["zeroref_v1"]["ref_schemes"].as_array().unwrap().clone();
        assert!(schemes.contains(&Value::String("tz://".to_string())));
        assert!(schemes.contains(&Value::String("fz://".to_string())));
        assert!(schemes.contains(&Value::String("gz://".to_string())));
    }

    #[test]
    fn publish_capabilities_round_trips() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();
        let mut store = TokenZeroStore::open(root);

        let descriptor = store.capability_descriptor();
        store.publish_capabilities();

        let digest = Sha256::digest(descriptor.to_string());
        let expected_hash = digest
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        let blob_ref = format!("tz://blob/{expected_hash}");
        let expanded = store.expand(&blob_ref).unwrap();
        let round_tripped: Value = serde_json::from_slice(&expanded).unwrap();
        assert_eq!(round_tripped["schema_version"], DESCRIPTOR_SCHEMA_VERSION);
        assert_eq!(round_tripped["engine"], "tokenzero");
    }

    #[test]
    fn max_object_bytes_limit_enforced() {
        let mut store = TokenZeroStore::in_memory();
        let bytes = b"too big";
        let err = store.put(bytes, Some(2)).unwrap_err();
        assert!(err.contains("exceeds limit"));
    }

    #[test]
    fn root_report_reflects_memory_and_unified_modes() {
        let mem = TokenZeroStore::in_memory();
        let mem_report = mem.root_report();
        assert_eq!(mem_report["effective_root_mode"], "memory");
        assert_eq!(mem_report["store_db"], "memory");

        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();
        let unified = TokenZeroStore::open(root);
        let unified_report = unified.root_report();
        assert_eq!(unified_report["effective_root_mode"], "unified");
        assert!(
            unified_report["store_health"]["cas_attached"]
                .as_bool()
                .unwrap()
        );
    }
}
