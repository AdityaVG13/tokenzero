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

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use tokenzero_core::ContentType;

use crate::shared_cas::{SharedCas, SharedCasError};
use crate::{parse_zeroref_v1_blob, RecoveryStore, ZeroRefError, ZeroRefFragment, ZeroRefV1Blob};

const DESCRIPTOR_SCHEMA_VERSION: &str = "tokenzero.recovery.capability.v1";
const DESCRIPTOR_VERSION: &str = "1.0.0";

/// Structured errors for the embeddable `TokenZeroStore` handle.
///
/// Portable full-hash blob refs never fall back to the legacy recovery tier.
/// Callers can distinguish missing objects from corruption, I/O, and policy
/// failures without inspecting free-form strings.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TokenZeroStoreError {
    /// Object is not present in the shared CAS (or no CAS is attached for a
    /// portable full-hash ref).
    #[error("object not found")]
    NotFound,
    /// Complete-object digest verification failed.
    #[error("corruption: object does not match expected hash")]
    Corruption,
    /// Underlying storage operation failed.
    #[error("io error: {0}")]
    Io(String),
    /// Policy denied access (e.g. not a regular file).
    #[error("policy violation")]
    Policy,
    /// Ref string is not a valid ZeroRef v1 portable blob ref.
    #[error("malformed ref")]
    Malformed,
    /// Fragment selector is invalid or out of range.
    #[error("fragment error: {0}")]
    Fragment(String),
    /// `#L` requested on non-UTF-8 content.
    #[error("non-utf8 line fragment")]
    NonUtf8Line,
    /// No shared CAS is attached for an operation that requires one.
    #[error("no shared CAS attached")]
    NoSharedCas,
    /// Payload exceeds the configured object size limit.
    #[error("payload {size} bytes exceeds limit {limit}")]
    PayloadTooLarge { size: u64, limit: u64 },
    /// Publishing was denied by filesystem permissions.
    #[error("shared CAS publish permission denied")]
    PublishPermission,
    /// The canonical publication subtree is blocked by a non-directory path.
    #[error("shared CAS publication path is not contained in a directory tree")]
    PublishContainment,
    /// The canonical destination conflicts with a non-file object.
    #[error("shared CAS publication destination conflicts with an existing object")]
    PublishConflict,
    /// Backward-compatible catch-all for publish failures that predate the
    /// structured categories above.
    #[error("shared CAS publish failed: {0}")]
    Publish(String),
    /// Durable cache directory could not be created.
    #[error("cannot create cache directory: {0}")]
    CacheDir(String),
    /// Legacy recovery expand did not find the ref.
    #[error("ref not found in recovery store")]
    LegacyNotFound,
}

impl From<SharedCasError> for TokenZeroStoreError {
    fn from(err: SharedCasError) -> Self {
        match err {
            SharedCasError::NotFound => Self::NotFound,
            SharedCasError::Corruption => Self::Corruption,
            SharedCasError::Io(e) => Self::Io(e.to_string()),
            SharedCasError::Policy => Self::Policy,
            SharedCasError::InvalidHash(_) => Self::Malformed,
        }
    }
}

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
    /// True only for an explicitly supplied CAS or the intentionally shared
    /// temporary CAS created by `in_memory()`. Ambient project-local CAS
    /// detection remains usable internally but is not advertised as shared.
    shared_cas_mode: bool,
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
                shared_cas_mode: false,
            },
        }
    }

    /// Open the handle, returning `Err` rather than degrading to in-memory.
    pub fn try_open(root: impl AsRef<Path>) -> Result<Self, TokenZeroStoreError> {
        let root_path = root.as_ref().to_path_buf();
        let cache_path = default_recovery_cache_path(&root_path);
        let parent = cache_path.parent().ok_or_else(|| {
            TokenZeroStoreError::CacheDir("invalid cache path: no parent directory".to_string())
        })?;
        std::fs::create_dir_all(parent).map_err(|e| {
            TokenZeroStoreError::CacheDir(format!(
                "cannot create cache directory {}: {e}",
                parent.display()
            ))
        })?;
        probe_durable_cache_target(&root_path, &cache_path)?;
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
            shared_cas_mode: false,
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
            shared_cas_mode: true,
        }
    }

    /// Construct a handle with an explicit shared CAS. This is the path a
    /// sibling engine (FSZero, GraphZero) uses to hand off its CAS object so
    /// all engines in a single ZeroStack process publish/resolve to the same
    /// immutable object tier.
    ///
    /// If `root` is provided, a durable recovery cache is opened at the
    /// conventional TokenZero path under that root. If directory creation
    /// fails, the handle is returned with an in-memory recovery store and
    /// `durable_degraded = true` rather than silently advertising durability.
    /// If `root` is `None`, the handle is memory-only for recovery metadata.
    pub fn with_shared_cas(root: Option<PathBuf>, shared_cas: SharedCas) -> Self {
        let (recovery, durable_degraded) = match &root {
            Some(root_path) => {
                let cache_path = default_recovery_cache_path(root_path);
                let usable = cache_path
                    .parent()
                    .and_then(|parent| std::fs::create_dir_all(parent).ok())
                    .and_then(|()| probe_durable_cache_target(root_path, &cache_path).ok())
                    .is_some();
                if usable {
                    (RecoveryStore::new(Some(cache_path)), false)
                } else {
                    (RecoveryStore::new(None), true)
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
            shared_cas_mode: true,
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
    pub fn put(
        &mut self,
        bytes: &[u8],
        max_object_bytes: Option<u64>,
    ) -> Result<String, TokenZeroStoreError> {
        if let Some(limit) = max_object_bytes {
            if bytes.len() as u64 > limit {
                return Err(TokenZeroStoreError::PayloadTooLarge {
                    size: bytes.len() as u64,
                    limit,
                });
            }
        }
        let cas = self
            .shared_cas
            .as_ref()
            .ok_or(TokenZeroStoreError::NoSharedCas)?;
        validate_publication_target(cas, bytes)?;
        let hash = cas.publish(bytes).map_err(classify_publish_error)?;
        Ok(format!("tz://blob/{hash}"))
    }

    /// Expand a ref to its exact bytes.
    ///
    /// Portable full-hash `tz://blob/<sha256>` refs (and their `fz://blob/` /
    /// `gz://blob/` aliases), including optional `#B`/`#L` fragment selectors,
    /// are resolved from the shared CAS when one is attached:
    /// 1. The whole object is verified against the full hash.
    /// 2. Fragment selectors are applied only after verification.
    /// 3. Missing/corruption/I/O/policy failures are returned as typed errors.
    /// 4. Valid full-hash refs never fall back to the legacy recovery store.
    ///
    /// Non-portable / legacy refs fall back to the RecoveryStore expand path.
    pub fn expand(&mut self, r: &str) -> Result<Vec<u8>, TokenZeroStoreError> {
        // Classify the bare identity without the fragment so a valid full-hash
        // ref with a bad selector yields Fragment errors, not Malformed.
        let (bare, fragment) = r.split_once('#').map_or((r, None), |(b, f)| (b, Some(f)));

        match parse_zeroref_v1_blob(bare, None) {
            Ok(mut parsed) => {
                if let Some(frag) = fragment {
                    // Dedicated fragment taxonomy before any CAS I/O.
                    parsed.fragment = Some(parse_fragment_to_zeroref(frag)?);
                } else {
                    parsed.fragment = None;
                }
                self.expand_portable_full_hash(parsed)
            }
            // Legacy short refs and non-blob kinds may use the recovery tier
            // (which owns its own fragment handling for legacy IDs).
            Err(ZeroRefError::LegacyAmbiguity) | Err(ZeroRefError::Unsupported) => {
                self.expand_legacy(r)
            }
            Err(ZeroRefError::Malformed)
            | Err(ZeroRefError::Missing)
            | Err(ZeroRefError::Io)
            | Err(ZeroRefError::Corruption)
            | Err(ZeroRefError::Policy)
            | Err(ZeroRefError::IncompatibleVersion) => Err(TokenZeroStoreError::Malformed),
        }
    }

    fn expand_portable_full_hash(
        &mut self,
        parsed: ZeroRefV1Blob,
    ) -> Result<Vec<u8>, TokenZeroStoreError> {
        let cas = self
            .shared_cas
            .as_ref()
            .ok_or(TokenZeroStoreError::NotFound)?;

        // Whole-object verification first — never fragment before integrity.
        let bytes = cas.resolve(&parsed.hash)?;

        match parsed.fragment {
            None => Ok(bytes),
            Some(fragment) => apply_fragment_to_bytes(&bytes, &fragment),
        }
    }

    fn expand_legacy(&mut self, r: &str) -> Result<Vec<u8>, TokenZeroStoreError> {
        let result = self.recovery.expand(r, Some("raw"), None, None, None, None);
        if result.found {
            Ok(result.content.into_bytes())
        } else {
            // Surface structured recovery reasons when they match the CAS taxonomy.
            match result.reason.as_str() {
                "shared-cas-missing" | "ref-not-found" => Err(TokenZeroStoreError::NotFound),
                "shared-cas-corruption" => Err(TokenZeroStoreError::Corruption),
                "shared-cas-io" => Err(TokenZeroStoreError::Io(result.reason)),
                "shared-cas-policy" => Err(TokenZeroStoreError::Policy),
                reason if reason.starts_with("fragment-") || reason.starts_with("window-") => {
                    Err(TokenZeroStoreError::Fragment(reason.to_string()))
                }
                _ => Err(TokenZeroStoreError::LegacyNotFound),
            }
        }
    }

    /// Probe whether the attached shared CAS can actually accept writes.
    ///
    /// Attachment alone is not writability: a read-only mount or permission
    /// failure must report `shared_cas_writable = false`.
    pub fn cas_writable(&self) -> bool {
        self.shared_cas_mode && self.shared_cas.as_ref().is_some_and(probe_cas_writable)
    }

    fn durable_usable(&self) -> bool {
        !self.durable_degraded
            && self
                .root
                .as_deref()
                .zip(self.recovery.persistence_path.as_deref())
                .is_some_and(|(root, cache)| probe_durable_cache_target(root, cache).is_ok())
    }

    /// Classify the effective store layout portably via path components and
    /// the shared-CAS structural resolver — never via substring matching on
    /// display paths (which misclassifies Windows separators and
    /// `.zerostack-old` lookalikes).
    pub fn effective_root_mode(&self) -> &'static str {
        let Some(cache_path) = self.recovery.persistence_path.as_deref() else {
            return "memory";
        };
        classify_root_mode(cache_path)
    }

    /// ZeroRef v1 capability descriptor for this handle. Static fields come
    /// from RecoveryStore/SharedCas constants; the `shared_cas` section is
    /// probed live so a caller can distinguish local-only, shared, and
    /// degraded states before routing any payload.
    pub fn capability_descriptor(&self) -> Value {
        let cas_attached = self.shared_cas_mode && self.shared_cas.is_some();
        let cas_writable = self.cas_writable();
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
                "durable": self.durable_usable(),
                "durable_degraded": self.durable_degraded,
                "persistent_path": self
                    .recovery
                    .persistence_path
                    .as_ref()
                    .map(|p| redact_path_identity(p)),
                "store_root": self.store_root().as_ref().map(|p| redact_path_identity(p))
            }
        })
    }

    /// Store health, root, and CAS summary for telemetry and the single-binary
    /// router. Does not leak absolute private paths — only non-reversible
    /// path identities and structural mode labels.
    pub fn root_report(&self) -> Value {
        let store_root = self
            .store_root()
            .map(|p| redact_path_identity(&p))
            .unwrap_or_else(|| "memory".to_string());
        let store_db = self
            .recovery
            .persistence_path
            .as_ref()
            .map(|p| redact_path_identity(p))
            .unwrap_or_else(|| "memory".to_string());
        let workspace_root = self
            .root
            .as_ref()
            .map(|p| redact_path_identity(p))
            .unwrap_or_else(|| "(none)".to_string());
        let effective_root_mode = self.effective_root_mode();
        let cas_attached = self.shared_cas_mode && self.shared_cas.is_some();
        let cas_writable = self.cas_writable();
        let cap = self.capability_descriptor();
        serde_json::json!({
            "workspace_root": workspace_root,
            "store_root": store_root,
            "store_db": store_db,
            "durable_degraded": self.durable_degraded,
            "effective_root_mode": effective_root_mode,
            "store_health": {
                "durable": self.durable_usable(),
                "cas_attached": cas_attached,
                "cas_writable": cas_writable,
            },
            "capabilities": cap,
            "last_integrity_error": null,
        })
    }

    /// Publish the capability descriptor through the store mode that this
    /// handle actually represents. Explicit and in-memory shared-CAS handles
    /// publish to that CAS; ambient/default isolated handles retain the
    /// recovery-store publication path. Best-effort; never blocks.
    pub fn publish_capabilities(&mut self) {
        let descriptor = self.capability_descriptor().to_string();
        if self.shared_cas_mode {
            let _ = self.put(descriptor.as_bytes(), None);
        } else {
            let _ = self
                .recovery
                .store_blob(&descriptor, ContentType::JsonConfig);
        }
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

fn unique_probe_name(kind: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(".{kind}-{}-{timestamp}-{n}", std::process::id())
}

fn reject_symlink_ancestors(path: &Path) -> Result<(), TokenZeroStoreError> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(TokenZeroStoreError::PublishContainment);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(TokenZeroStoreError::Io(error.to_string())),
        }
    }
    Ok(())
}

fn probe_durable_cache_target(
    workspace_root: &Path,
    cache_path: &Path,
) -> Result<(), TokenZeroStoreError> {
    if cache_path.file_name().and_then(|name| name.to_str()) != Some("recovery-cache.json") {
        return Err(TokenZeroStoreError::CacheDir(
            "noncanonical recovery cache filename".to_string(),
        ));
    }
    let parent = cache_path.parent().ok_or_else(|| {
        TokenZeroStoreError::CacheDir("invalid cache path: no parent directory".to_string())
    })?;
    reject_symlink_ancestors(parent).map_err(|error| {
        TokenZeroStoreError::CacheDir(format!("cache parent is noncanonical: {error}"))
    })?;
    let canonical_root = std::fs::canonicalize(workspace_root).map_err(|error| {
        TokenZeroStoreError::CacheDir(format!("workspace root is unavailable: {error}"))
    })?;
    let canonical_parent = std::fs::canonicalize(parent).map_err(|error| {
        TokenZeroStoreError::CacheDir(format!("cache parent is unavailable: {error}"))
    })?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err(TokenZeroStoreError::CacheDir(
            "cache parent escapes canonical workspace root".to_string(),
        ));
    }
    match std::fs::symlink_metadata(cache_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(TokenZeroStoreError::CacheDir(
                "cache target is not a canonical regular file".to_string(),
            ))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(TokenZeroStoreError::CacheDir(format!(
                "cache target is unavailable: {error}"
            )))
        }
    }
    let probe = parent.join(unique_probe_name("recovery-cache-write-probe"));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)?;
        file.write_all(b"tokenzero durable cache probe")?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        std::fs::remove_file(&probe)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&probe);
    }
    result.map_err(|error| {
        TokenZeroStoreError::CacheDir(format!("cache sibling is not fully writable: {error}"))
    })
}

fn publication_target(cas: &SharedCas, bytes: &[u8]) -> PathBuf {
    let hash = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    cas.root()
        .join("blobs")
        .join("sha256")
        .join(&hash[..2])
        .join(hash)
}

fn prepare_canonical_prefix(
    cas: &SharedCas,
    prefix: &str,
) -> Result<(PathBuf, [bool; 3]), TokenZeroStoreError> {
    reject_symlink_ancestors(cas.root())?;
    std::fs::create_dir_all(cas.root()).map_err(classify_io_publish_error)?;
    reject_symlink_ancestors(cas.root())?;
    let mut canonical_parent = std::fs::canonicalize(cas.root())
        .map_err(|error| TokenZeroStoreError::Io(error.to_string()))?;
    let blobs = cas.root().join("blobs");
    let sha256 = blobs.join("sha256");
    let prefix_dir = sha256.join(prefix);
    let existed = [blobs.exists(), sha256.exists(), prefix_dir.exists()];
    for child in [&blobs, &sha256, &prefix_dir] {
        match std::fs::symlink_metadata(child) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(TokenZeroStoreError::PublishContainment)
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(child).map_err(classify_io_publish_error)?
            }
            Err(error) => return Err(classify_io_publish_error(error)),
        }
        let metadata = std::fs::symlink_metadata(child).map_err(classify_io_publish_error)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(TokenZeroStoreError::PublishContainment);
        }
        let canonical_child = std::fs::canonicalize(child)
            .map_err(|error| TokenZeroStoreError::Io(error.to_string()))?;
        if canonical_child.parent() != Some(canonical_parent.as_path()) {
            return Err(TokenZeroStoreError::PublishContainment);
        }
        canonical_parent = canonical_child;
    }
    Ok((prefix_dir, existed))
}

fn validate_publication_target(cas: &SharedCas, bytes: &[u8]) -> Result<(), TokenZeroStoreError> {
    let target = publication_target(cas, bytes);
    let hash = target
        .file_name()
        .and_then(|name| name.to_str())
        .expect("publication target has hash filename");
    let _ = prepare_canonical_prefix(cas, &hash[..2])?;
    match std::fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(TokenZeroStoreError::PublishConflict)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(classify_io_publish_error(error)),
    }
}

fn classify_io_publish_error(error: std::io::Error) -> TokenZeroStoreError {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => TokenZeroStoreError::PublishPermission,
        std::io::ErrorKind::NotADirectory | std::io::ErrorKind::IsADirectory => {
            TokenZeroStoreError::PublishContainment
        }
        std::io::ErrorKind::AlreadyExists => TokenZeroStoreError::PublishConflict,
        _ => TokenZeroStoreError::Io(error.to_string()),
    }
}

fn classify_publish_error(error: SharedCasError) -> TokenZeroStoreError {
    match error {
        SharedCasError::Io(error) => classify_io_publish_error(error),
        SharedCasError::Policy => TokenZeroStoreError::PublishConflict,
        other => other.into(),
    }
}

/// Structural, portable root-mode classification.
///
/// Uses path components and [`SharedCas::resolve_cache_root`] rather than
/// substring matching on display strings, so Windows separators and
/// lookalike directories like `.zerostack-old` are handled correctly.
fn classify_root_mode(cache_path: &Path) -> &'static str {
    // Unified layout: <store-root>/tokenzero/recovery-cache.json where the
    // store root's final component is exactly ".zerostack".
    if let Some(store_root) = SharedCas::resolve_cache_root(cache_path) {
        if path_file_name_eq(&store_root, ".zerostack") {
            return "unified";
        }
        // Engine-namespaced cache under a non-.zerostack root is still the
        // structural unified CAS layout (e.g. explicit shared store root).
        // Treat only exact ".zerostack" component as the "unified" mode label
        // used by telemetry; everything else with engine namespacing that is
        // not under .zerostack reports as "legacy" unless it is pure flat.
        //
        // Historical CLI convention: only the `.zerostack` directory name
        // marks unified mode. Other engine-namespaced roots stay "legacy".
        return "legacy";
    }
    "legacy"
}

fn path_file_name_eq(path: &Path, expected: &str) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some(expected)
}

/// Non-reversible path identity for telemetry. Never emits absolute/private
/// path bytes or reversible encodings.
fn redact_path_identity(path: &Path) -> String {
    let mut hasher = Sha256::new();
    // Hash OS bytes when available so distinct paths stay distinct without
    // embedding the original string.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hasher.update(path.as_os_str().as_bytes());
    }
    #[cfg(not(unix))]
    {
        hasher.update(path.to_string_lossy().as_bytes());
    }
    let digest = hasher.finalize();
    let short = digest
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!("path:{short}")
}

/// Establish actual CAS writability by attempting a tiny create/write/delete
/// probe under the CAS root. Attachment is not sufficient.
fn probe_cas_writable(cas: &SharedCas) -> bool {
    let prefix_seed = unique_probe_name("cas-prefix");
    let hash = Sha256::digest(prefix_seed.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let Ok((prefix, existed)) = prepare_canonical_prefix(cas, &hash[..2]) else {
        return false;
    };
    let probe = prefix.join(unique_probe_name("cas-write-probe"));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)?;
        file.write_all(b"tokenzero CAS write probe")?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        std::fs::remove_file(&probe)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&probe);
    }
    if !existed[2] {
        let _ = std::fs::remove_dir(&prefix);
    }
    if !existed[1] {
        let _ = std::fs::remove_dir(cas.root().join("blobs").join("sha256"));
    }
    if !existed[0] {
        let _ = std::fs::remove_dir(cas.root().join("blobs"));
    }
    result.is_ok()
}

/// Apply a verified whole-object fragment selector. Byte ranges are zero-based
/// half-open; line ranges are one-based inclusive with exact newline retention.
fn apply_fragment_to_bytes(
    bytes: &[u8],
    fragment: &ZeroRefFragment,
) -> Result<Vec<u8>, TokenZeroStoreError> {
    match fragment {
        ZeroRefFragment::Byte { start, end } => {
            if *end > bytes.len() {
                return Err(TokenZeroStoreError::Fragment(format!(
                    "fragment-out-of-range; start={start} end={end} len={}",
                    bytes.len()
                )));
            }
            if start > end {
                return Err(TokenZeroStoreError::Fragment(
                    "fragment-reversed".to_string(),
                ));
            }
            Ok(bytes[*start..*end].to_vec())
        }
        ZeroRefFragment::Line { start, end } => {
            let text = std::str::from_utf8(bytes).map_err(|_| TokenZeroStoreError::NonUtf8Line)?;
            let segments: Vec<&str> = text.split_inclusive('\n').collect();
            let line_count = segments.len();
            if *start == 0 {
                return Err(TokenZeroStoreError::Fragment(
                    "fragment-malformed".to_string(),
                ));
            }
            if start > end {
                return Err(TokenZeroStoreError::Fragment(
                    "fragment-reversed".to_string(),
                ));
            }
            if *start > line_count || *end > line_count {
                return Err(TokenZeroStoreError::Fragment(format!(
                    "fragment-out-of-range; start={start} end={end} lines={line_count}"
                )));
            }
            let lo = start - 1;
            let hi = *end;
            Ok(segments[lo..hi].concat().into_bytes())
        }
    }
}

/// Parse and validate a #B/#L fragment into a ZeroRefFragment.
/// Uses the same taxonomy as RecoveryStore so embedded and standalone agree.
fn parse_fragment_to_zeroref(fragment: &str) -> Result<ZeroRefFragment, TokenZeroStoreError> {
    if fragment.is_empty() {
        return Err(TokenZeroStoreError::Fragment(
            "fragment-malformed".to_string(),
        ));
    }
    if fragment.contains('#') {
        return Err(TokenZeroStoreError::Fragment(
            "fragment-duplicate".to_string(),
        ));
    }
    let kind = &fragment[..1];
    let rest = &fragment[1..];
    match kind {
        "B" => {
            let (start_str, end_str) = rest
                .split_once('-')
                .or_else(|| rest.split_once(','))
                .unwrap_or((rest, rest));
            let start = start_str
                .trim_start_matches('B')
                .parse::<usize>()
                .map_err(|_| TokenZeroStoreError::Fragment("fragment-malformed".to_string()))?;
            let end = end_str
                .trim_start_matches('B')
                .parse::<usize>()
                .map_err(|_| TokenZeroStoreError::Fragment("fragment-malformed".to_string()))?;
            if start > end {
                return Err(TokenZeroStoreError::Fragment(
                    "fragment-reversed".to_string(),
                ));
            }
            Ok(ZeroRefFragment::Byte { start, end })
        }
        "L" => {
            let (start_str, end_str) = rest.split_once('-').unwrap_or((rest, rest));
            let start = start_str
                .trim_start_matches('L')
                .parse::<usize>()
                .map_err(|_| TokenZeroStoreError::Fragment("fragment-malformed".to_string()))?;
            let end = end_str
                .trim_start_matches('L')
                .parse::<usize>()
                .map_err(|_| TokenZeroStoreError::Fragment("fragment-malformed".to_string()))?;
            if start == 0 {
                return Err(TokenZeroStoreError::Fragment(
                    "fragment-malformed".to_string(),
                ));
            }
            if start > end {
                return Err(TokenZeroStoreError::Fragment(
                    "fragment-reversed".to_string(),
                ));
            }
            Ok(ZeroRefFragment::Line { start, end })
        }
        _ => Err(TokenZeroStoreError::Fragment(
            "fragment-unknown-kind".to_string(),
        )),
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
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
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
        assert!(matches!(
            store_b.expand(&ref_a),
            Err(TokenZeroStoreError::NotFound)
        ));
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
        assert!(!cap["zeroref_v1"]["shared_cas"].as_bool().unwrap());
        assert!(!cap["zeroref_v1"]["shared_cas_writable"].as_bool().unwrap());
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
    fn explicit_capability_publication_round_trips_through_shared_cas() {
        let mut store = TokenZeroStore::in_memory();
        let descriptor = store.capability_descriptor().to_string();
        store.publish_capabilities();
        let hash = Sha256::digest(descriptor.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let expanded = store.expand(&format!("tz://blob/{hash}")).unwrap();
        assert_eq!(expanded, descriptor.as_bytes());
    }

    #[test]
    fn max_object_bytes_limit_enforced() {
        let mut store = TokenZeroStore::in_memory();
        let bytes = b"too big";
        let err = store.put(bytes, Some(2)).unwrap_err();
        assert!(matches!(
            err,
            TokenZeroStoreError::PayloadTooLarge { size: 7, limit: 2 }
        ));
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
        assert!(!unified_report["store_health"]["cas_attached"]
            .as_bool()
            .unwrap());
    }

    // --- PR22 review regressions ---

    #[test]
    fn expand_applies_byte_and_line_fragments_after_whole_object_verify() {
        let mut store = TokenZeroStore::in_memory();
        let payload = b"alpha\nbeta\ngamma\n";
        let ref_id = store.put(payload, None).unwrap();

        // #B0-5 → "alpha" (byte-exact)
        let b_ref = format!("{ref_id}#B0-5");
        assert_eq!(store.expand(&b_ref).unwrap(), b"alpha");

        // #L2-2 → "beta\n" with exact newline retention
        let l_ref = format!("{ref_id}#L2-2");
        assert_eq!(store.expand(&l_ref).unwrap(), b"beta\n");

        // Cross-engine schemes honor fragments too.
        let fz_b = format!(
            "fz://blob/{}#B6-11",
            ref_id.strip_prefix("tz://blob/").unwrap()
        );
        assert_eq!(store.expand(&fz_b).unwrap(), b"beta\n");

        let gz_l = format!(
            "gz://blob/{}#L1-L1",
            ref_id.strip_prefix("tz://blob/").unwrap()
        );
        assert_eq!(store.expand(&gz_l).unwrap(), b"alpha\n");
    }

    #[test]
    fn expand_fragment_out_of_range_is_typed_error() {
        let mut store = TokenZeroStore::in_memory();
        let ref_id = store.put(b"short", None).unwrap();
        let b_ref = format!("{ref_id}#B0-100");
        let err = store.expand(&b_ref).unwrap_err();
        match err {
            TokenZeroStoreError::Fragment(reason) => {
                assert!(
                    reason.starts_with("fragment-out-of-range"),
                    "reason={reason}"
                );
            }
            other => panic!("expected Fragment, got {other:?}"),
        }
    }

    #[test]
    fn expand_full_hash_missing_is_not_found_not_none_fallback() {
        let mut store = TokenZeroStore::in_memory();
        let missing = "tz://blob/0000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            store.expand(missing),
            Err(TokenZeroStoreError::NotFound)
        ));

        // Cross-engine full-hash missing also stays typed NotFound — no legacy
        // fallback that would return Ok/None-style silence.
        let missing_fz =
            "fz://blob/0000000000000000000000000000000000000000000000000000000000000000";
        assert!(matches!(
            store.expand(missing_fz),
            Err(TokenZeroStoreError::NotFound)
        ));
    }

    #[test]
    fn expand_full_hash_corruption_never_falls_back_to_legacy() {
        let cas_dir = tempdir().unwrap();
        let cas = SharedCas::new(cas_dir.path().to_path_buf());
        let mut store = TokenZeroStore::with_shared_cas(None, cas.clone());

        let payload = b"honest-bytes";
        let ref_id = store.put(payload, None).unwrap();
        let hash = ref_id.strip_prefix("tz://blob/").unwrap();
        let object_path = cas
            .root()
            .join("blobs")
            .join("sha256")
            .join(&hash[..2])
            .join(hash);
        std::fs::write(&object_path, b"tampered-content").unwrap();

        // Full ref and fragment form both report Corruption, never legacy bytes.
        assert!(matches!(
            store.expand(&ref_id),
            Err(TokenZeroStoreError::Corruption)
        ));
        let frag = format!("{ref_id}#B0-5");
        assert!(matches!(
            store.expand(&frag),
            Err(TokenZeroStoreError::Corruption)
        ));

        // Even if the recovery store has a same-hash alias payload, full-hash
        // portable refs must not fall back.
        let _ = store
            .recovery_mut()
            .store_blob("legacy-poison", ContentType::Unknown);
        assert!(matches!(
            store.expand(&ref_id),
            Err(TokenZeroStoreError::Corruption)
        ));
    }

    #[test]
    fn classify_root_mode_uses_path_components_not_substring() {
        // Exact .zerostack component → unified.
        let unified_cache = PathBuf::from("/tmp/project/.zerostack/tokenzero/recovery-cache.json");
        assert_eq!(classify_root_mode(&unified_cache), "unified");

        // Lookalike .zerostack-old must NOT classify as unified.
        let old_cache = PathBuf::from("/tmp/project/.zerostack-old/tokenzero/recovery-cache.json");
        assert_eq!(classify_root_mode(&old_cache), "legacy");

        // Windows-style separators still classify via components.
        let win = PathBuf::from(r"C:\Users\x\proj\.zerostack\tokenzero\recovery-cache.json");
        // On Unix this is a single component path string; structural check still
        // walks components, so a path whose file_name chain ends with
        // tokenzero under .zerostack is unified when components parse that way.
        // Build with join to guarantee component structure:
        let win_joined = PathBuf::from("C:")
            .join("Users")
            .join("x")
            .join("proj")
            .join(".zerostack")
            .join("tokenzero")
            .join("recovery-cache.json");
        assert_eq!(classify_root_mode(&win_joined), "unified");
        let _ = win; // silence unused in documentation of the bug class

        // Flat legacy cache.
        let legacy = PathBuf::from("/tmp/project/.tokenzero/recovery-cache.json");
        assert_eq!(classify_root_mode(&legacy), "legacy");
    }

    #[test]
    fn root_report_classifies_zerostack_old_as_legacy() {
        let dir = tempdir().unwrap();
        // Create a lookalike root that substring matching would misclassify.
        let lookalike = dir.path().join(".zerostack-old").join("tokenzero");
        std::fs::create_dir_all(&lookalike).unwrap();
        let cache = lookalike.join("recovery-cache.json");
        // Attach via with_shared_cas so we control the cache path through open
        // of a synthetic recovery store: open() would choose legacy/unified by
        // .zerostack existence. Instead assert classify_root_mode directly and
        // via a handle whose persistence path is the lookalike.
        let cas = SharedCas::new(dir.path().join(".zerostack-old"));
        let mut store = TokenZeroStore::with_shared_cas(None, cas);
        // Manually point recovery at the lookalike cache path.
        store.recovery = RecoveryStore::new(Some(cache));
        assert_eq!(store.effective_root_mode(), "legacy");
        assert_eq!(store.root_report()["effective_root_mode"], "legacy");
    }

    #[test]
    fn ambient_project_cas_is_not_advertised_as_shared() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".zerostack").join("tokenzero")).unwrap();
        let store = TokenZeroStore::open(dir.path());
        assert!(
            store.shared_cas().is_some(),
            "ambient CAS remains internally usable"
        );
        let cap = store.capability_descriptor();
        assert_eq!(cap["zeroref_v1"]["shared_cas"], false);
        assert_eq!(cap["zeroref_v1"]["shared_cas_writable"], false);

        let explicit = TokenZeroStore::in_memory();
        let cap = explicit.capability_descriptor();
        assert_eq!(cap["zeroref_v1"]["shared_cas"], true);
        assert_eq!(cap["zeroref_v1"]["shared_cas_writable"], true);
    }

    #[test]
    fn cas_probe_uses_canonical_subtree_and_leaves_no_artifacts() {
        let dir = tempdir().unwrap();
        let cas_root = dir.path().join("cas");
        let store = TokenZeroStore::with_shared_cas(None, SharedCas::new(cas_root.clone()));
        assert!(store.cas_writable());
        assert!(
            !cas_root.join("blobs").exists(),
            "probe-created canonical subtree must be removed"
        );
    }

    #[test]
    fn publish_preserves_containment_conflict_and_corruption_categories() {
        let dir = tempdir().unwrap();

        let contained_root = dir.path().join("contained");
        std::fs::create_dir_all(&contained_root).unwrap();
        std::fs::write(contained_root.join("blobs"), b"not-a-directory").unwrap();
        let mut contained = TokenZeroStore::with_shared_cas(None, SharedCas::new(contained_root));
        assert!(matches!(
            contained.put(b"payload", None),
            Err(TokenZeroStoreError::PublishContainment)
        ));

        let conflict_root = dir.path().join("conflict");
        let conflict_bytes = b"conflict";
        let hash = Sha256::digest(conflict_bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let target = conflict_root
            .join("blobs")
            .join("sha256")
            .join(&hash[..2])
            .join(&hash);
        std::fs::create_dir_all(&target).unwrap();
        let mut conflict = TokenZeroStore::with_shared_cas(None, SharedCas::new(conflict_root));
        assert!(matches!(
            conflict.put(conflict_bytes, None),
            Err(TokenZeroStoreError::PublishConflict)
        ));

        let corrupt_root = dir.path().join("corrupt");
        let target = corrupt_root
            .join("blobs")
            .join("sha256")
            .join(&hash[..2])
            .join(&hash);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"wrong").unwrap();
        let mut corrupt = TokenZeroStore::with_shared_cas(None, SharedCas::new(corrupt_root));
        assert!(matches!(
            corrupt.put(conflict_bytes, None),
            Err(TokenZeroStoreError::Corruption)
        ));
    }

    #[test]
    fn durable_is_false_when_cache_target_becomes_unusable() {
        let dir = tempdir().unwrap();
        let mut store = TokenZeroStore::try_open(dir.path()).unwrap();
        assert_eq!(store.capability_descriptor()["recovery"]["durable"], true);
        let cache = store.recovery().persistence_path.clone().unwrap();
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        assert_eq!(store.capability_descriptor()["recovery"]["durable"], false);
        assert_eq!(store.root_report()["store_health"]["durable"], false);
        store.recovery = RecoveryStore::new(None);
    }

    fn assert_no_probe_artifacts(path: &Path) {
        if !path.exists() {
            return;
        }
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            assert!(!name.contains("write-probe"), "left probe artifact: {name}");
            if entry.file_type().unwrap().is_dir() {
                assert_no_probe_artifacts(&entry.path());
            }
        }
    }

    #[test]
    fn cas_probe_preserves_existing_blob_and_is_concurrency_safe() {
        let dir = tempdir().unwrap();
        let cas = SharedCas::new(dir.path().join("cas"));
        let payload = b"existing canonical object";
        let hash = cas.publish(payload).unwrap();
        let object = cas
            .root()
            .join("blobs")
            .join("sha256")
            .join(&hash[..2])
            .join(&hash);

        let handles = (0..8)
            .map(|_| {
                let cas = cas.clone();
                std::thread::spawn(move || assert!(probe_cas_writable(&cas)))
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(std::fs::read(&object).unwrap(), payload);
        assert_no_probe_artifacts(cas.root());
    }

    #[test]
    fn durable_probe_preserves_live_cache_and_removes_owned_sibling() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let cache = root.join(".tokenzero").join("recovery-cache.json");
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        let original = b"{\"version\":1}";
        std::fs::write(&cache, original).unwrap();

        probe_durable_cache_target(root, &cache).unwrap();

        assert_eq!(std::fs::read(&cache).unwrap(), original);
        assert_no_probe_artifacts(cache.parent().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_cache_and_cas_ancestors_are_rejected() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        let outside_cache = dir.path().join("outside-cache");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside_cache).unwrap();
        symlink(&outside_cache, workspace.join(".tokenzero")).unwrap();
        assert!(matches!(
            TokenZeroStore::try_open(&workspace),
            Err(TokenZeroStoreError::CacheDir(_))
        ));

        let cas_root = dir.path().join("cas");
        let outside_blobs = dir.path().join("outside-blobs");
        std::fs::create_dir_all(&cas_root).unwrap();
        std::fs::create_dir_all(&outside_blobs).unwrap();
        symlink(&outside_blobs, cas_root.join("blobs")).unwrap();
        let cas = SharedCas::new(cas_root);
        assert!(!probe_cas_writable(&cas));
        let mut store = TokenZeroStore::with_shared_cas(None, cas);
        assert!(matches!(
            store.put(b"must stay contained", None),
            Err(TokenZeroStoreError::PublishContainment)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn cas_writable_false_when_root_not_writable() {
        let dir = tempdir().unwrap();
        let cas_root = dir.path().join("ro-cas");
        std::fs::create_dir_all(&cas_root).unwrap();
        // Make the CAS root read-only so create_dir_all(blobs/...) fails.
        let mut perms = std::fs::metadata(&cas_root).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&cas_root, perms).unwrap();

        let cas = SharedCas::new(cas_root.clone());
        let mut store = TokenZeroStore::with_shared_cas(None, cas);
        assert!(store.shared_cas().is_some());
        assert!(
            !store.cas_writable(),
            "read-only CAS root must not advertise writability"
        );
        assert!(matches!(
            store.put(b"permission-denied", None),
            Err(TokenZeroStoreError::PublishPermission)
        ));
        let cap = store.capability_descriptor();
        assert_eq!(cap["zeroref_v1"]["shared_cas"], true);
        assert_eq!(cap["zeroref_v1"]["shared_cas_writable"], false);
        let report = store.root_report();
        assert_eq!(report["store_health"]["cas_attached"], true);
        assert_eq!(report["store_health"]["cas_writable"], false);

        // Restore writability so tempdir cleanup succeeds.
        let mut perms = std::fs::metadata(&cas_root).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&cas_root, perms).unwrap();
    }

    #[test]
    fn with_shared_cas_mkdir_failure_sets_durable_degraded() {
        let dir = tempdir().unwrap();
        // Parent that cannot contain a new directory: use a file as "root".
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

    #[test]
    fn root_report_redacts_absolute_paths() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();
        let store = TokenZeroStore::open(root);
        let report = store.root_report();

        let workspace = report["workspace_root"].as_str().unwrap();
        let store_root = report["store_root"].as_str().unwrap();
        let store_db = report["store_db"].as_str().unwrap();
        let abs = root.to_string_lossy();

        assert!(
            !workspace.contains(abs.as_ref()),
            "workspace_root leaked absolute path: {workspace}"
        );
        assert!(
            !store_root.contains(abs.as_ref()),
            "store_root leaked absolute path: {store_root}"
        );
        assert!(
            !store_db.contains(abs.as_ref()),
            "store_db leaked absolute path: {store_db}"
        );
        assert!(
            workspace.starts_with("path:"),
            "workspace_root should be path: identity, got {workspace}"
        );
        assert!(
            store_root.starts_with("path:"),
            "store_root should be path: identity, got {store_root}"
        );
        assert!(
            store_db.starts_with("path:"),
            "store_db should be path: identity, got {store_db}"
        );

        // Nested capability descriptor must also avoid absolute paths.
        let cap = &report["capabilities"];
        if let Some(p) = cap["recovery"]["persistent_path"].as_str() {
            assert!(!p.contains(abs.as_ref()), "cap persistent_path leaked: {p}");
            assert!(p.starts_with("path:"), "cap persistent_path={p}");
        }
        if let Some(p) = cap["recovery"]["store_root"].as_str() {
            assert!(!p.contains(abs.as_ref()), "cap store_root leaked: {p}");
            assert!(p.starts_with("path:"), "cap store_root={p}");
        }

        // Redacted identity must not reverse to the original path string.
        assert_ne!(workspace, abs.as_ref());
        assert!(!workspace.contains('/'));
    }

    #[test]
    fn expand_malformed_fragment_is_typed() {
        let mut store = TokenZeroStore::in_memory();
        let ref_id = store.put(b"abc", None).unwrap();
        let bad = format!("{ref_id}#Babc");
        match store.expand(&bad).unwrap_err() {
            TokenZeroStoreError::Fragment(reason) => assert_eq!(reason, "fragment-malformed"),
            other => panic!("expected Fragment, got {other:?}"),
        }
        let dup = format!("{ref_id}#B0-1#L1");
        match store.expand(&dup).unwrap_err() {
            TokenZeroStoreError::Fragment(reason) => assert_eq!(reason, "fragment-duplicate"),
            other => panic!("expected Fragment, got {other:?}"),
        }
    }
}
