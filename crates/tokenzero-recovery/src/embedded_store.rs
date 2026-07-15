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
        // Refuse a symlinked cache ancestor: a symlinked `.tokenzero` /
        // `.zerostack` directory would let durable writes escape the workspace
        // root. Checked before create_dir_all, which would otherwise follow the
        // link and succeed.
        reject_symlinks_below(&root_path, parent)?;
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
        let (recovery, durable_degraded) = match &root {
            Some(root_path) => {
                let cache_path = default_recovery_cache_path(root_path);
                match cache_path.parent() {
                    Some(parent) if std::fs::create_dir_all(parent).is_ok() => {
                        (RecoveryStore::new(Some(cache_path)), false)
                    }
                    // The durable cache directory could not be created (e.g. the
                    // workspace root is a file). Degrade to in-memory rather than
                    // claim a durable path we cannot actually write.
                    _ => (RecoveryStore::new(None), true),
                }
            }
            None => (RecoveryStore::new(None), false),
        };
        Self {
            root,
            recovery,
            shared_cas: Some(shared_cas),
            durable_degraded,
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
        // Refuse to publish through a symlinked CAS `blobs` tree, which would let
        // content-addressed writes escape the shared store root.
        reject_symlinks_below(cas.root(), &cas.root().join("blobs"))?;
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
                if let Ok(bytes) = cas.resolve(hash) {
                    return Some(bytes);
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

/// Reject any symlinked or non-directory component between `root` (inclusive)
/// and `path` (inclusive). Missing components are allowed (they are created
/// under the canonical root on demand); an existing symlink or non-directory
/// anywhere in the chain is refused so durable/CAS writes cannot escape the
/// intended root. `root` must be a prefix of `path`.
fn reject_symlinks_below(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("path {} escapes root {}", path.display(), root.display()))?;
    let mut candidate = root.to_path_buf();
    for component in std::iter::once(None).chain(relative.components().map(Some)) {
        if let Some(component) = component {
            candidate.push(component.as_os_str());
        }
        match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(format!(
                    "refusing symlinked or non-directory path component: {}",
                    candidate.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot inspect {}: {error}", candidate.display())),
        }
    }
    Ok(())
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
mod restored_containment_tests {
    use super::TokenZeroStore;
    use crate::shared_cas::SharedCas;

    /// Restored from origin/main `embedded_store.rs` (dropped in the
    /// perf/loc-and-call-latency refactor): a symlinked cache ancestor and a
    /// symlinked CAS `blobs` tree must both be refused so durable/content-
    /// addressed writes cannot escape the intended root.
    #[cfg(unix)]
    #[test]
    fn symlinked_cache_and_cas_ancestors_are_rejected() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        let outside_cache = dir.path().join("outside-cache");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside_cache).unwrap();
        symlink(&outside_cache, workspace.join(".tokenzero")).unwrap();
        assert!(
            TokenZeroStore::try_open(&workspace).is_err(),
            "symlinked cache ancestor must be rejected, not followed"
        );

        let cas_root = dir.path().join("cas");
        let outside_blobs = dir.path().join("outside-blobs");
        std::fs::create_dir_all(&cas_root).unwrap();
        std::fs::create_dir_all(&outside_blobs).unwrap();
        symlink(&outside_blobs, cas_root.join("blobs")).unwrap();
        let cas = SharedCas::new(cas_root);
        let mut store = TokenZeroStore::with_shared_cas(None, cas);
        assert!(
            store.put(b"must stay contained", None).is_err(),
            "publish through a symlinked blobs dir must be refused"
        );
    }

    /// Restored from origin/main `embedded_store.rs`: when the durable cache
    /// directory cannot be created, the handle must degrade rather than claim a
    /// durable path it cannot write.
    #[test]
    fn with_shared_cas_mkdir_failure_sets_durable_degraded() {
        let dir = tempfile::tempdir().unwrap();
        // A file used as the workspace root cannot contain a cache directory.
        let file_root = dir.path().join("not-a-dir");
        std::fs::write(&file_root, b"x").unwrap();
        let cas = SharedCas::new(dir.path().join("cas"));
        let store = TokenZeroStore::with_shared_cas(Some(file_root), cas);
        assert!(
            store.durable_degraded,
            "mkdir failure must set durable_degraded"
        );
        assert!(
            store.recovery().persistence_path.is_none(),
            "must not claim a durable path after mkdir failure"
        );
        let report = store.root_report();
        assert_eq!(report["durable_degraded"], true);
        assert_eq!(report["store_health"]["durable"], false);
    }
}
