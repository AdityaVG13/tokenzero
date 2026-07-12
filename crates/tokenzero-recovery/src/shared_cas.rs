//! Canonical shared content-addressed storage (CAS) for ZeroRef v1 blobs.
//!
//! Immutable objects are stored under `<root>/blobs/sha256/<first-two-hex>/<full-hash>`.
//! This adapter implements the shared-CAS tier for full-hash portable refs
//! (`tz://blob/<sha256>` and its `fz`/`gz` aliases). The legacy private JSON
//! recovery store in `RecoveryStore` remains available as a separate read tier
//! for migration.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
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

    /// Resolve a sibling engine's recovery cache path under the same unified
    /// ZeroStack root. The current path must follow the layout
    /// `<root>/<engine>/recovery-cache.json`. Returns `None` for flat or
    /// non-unified layouts so that isolated stores stay isolated.
    pub fn sibling_engine_cache_path(cache_path: &Path, engine: &str) -> Option<PathBuf> {
        const ENGINES: &[&str] = &["tokenzero", "fszero", "graphzero"];
        let engine_dir = cache_path.parent()?;
        let name = engine_dir.file_name()?.to_str()?;
        if !ENGINES.contains(&name) {
            return None;
        }
        let store_root = engine_dir.parent()?;
        Some(store_root.join(engine).join("recovery-cache.json"))
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

    /// Enumerate all full-hash objects currently present in the shared CAS.
    /// Temp files and non-regular files are ignored.
    pub fn list_objects(&self) -> Result<Vec<String>, SharedCasError> {
        let mut objects = Vec::new();
        let base = self.root.join("blobs").join("sha256");
        if !base.is_dir() {
            return Ok(objects);
        }
        for prefix_entry in fs::read_dir(&base)? {
            let prefix_entry = prefix_entry?;
            let prefix_dir = prefix_entry.path();
            if !prefix_dir.is_dir() {
                continue;
            }
            for entry in fs::read_dir(&prefix_dir)? {
                let entry = entry?;
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let name = entry.file_name();
                let Some(name_str) = name.to_str() else {
                    continue;
                };
                if name_str.starts_with('.') {
                    continue;
                }
                if self.validate_hash(name_str).is_ok() {
                    objects.push(name_str.to_string());
                }
            }
        }
        Ok(objects)
    }

    /// Remove a full-hash object from the shared CAS. Idempotent: a missing
    /// object is not an error.
    pub fn remove_object(&self, full_hash: &str) -> Result<(), SharedCasError> {
        self.validate_hash(full_hash)?;
        let path = self.object_path(full_hash);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    /// Repair a missing or corrupt object by writing the correct bytes.
    /// Returns `true` if a repair was performed, `false` if the object was
    /// already valid. Returns an error if the provided bytes do not hash to
    /// `full_hash`.
    pub fn repair_object(&self, full_hash: &str, bytes: &[u8]) -> Result<bool, SharedCasError> {
        self.validate_hash(full_hash)?;
        let expected_hash = sha256_hex(bytes);
        if expected_hash != full_hash {
            return Err(SharedCasError::InvalidHash(format!(
                "provided bytes hash to {expected_hash}, expected {full_hash}"
            )));
        }
        let path = self.object_path(full_hash);
        if path.is_file() {
            match self.resolve(full_hash) {
                Ok(_) => return Ok(false),
                Err(SharedCasError::Corruption) => {
                    // Remove the corrupt object so publish can replace it.
                    fs::remove_file(&path)?;
                }
                Err(err) => return Err(err),
            }
        }
        self.publish(bytes)?;
        Ok(true)
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
                .any(|b| !b.is_ascii_digit() && !(b'a'..=b'f').contains(&b))
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

// ---------------------------------------------------------------------------
// Shared-CAS GC coordinator (zerostack.cas-gc.v1)
// ---------------------------------------------------------------------------

/// Frozen schema version for all shared-CAS GC records.
pub const GC_SCHEMA_VERSION: &str = "zerostack.cas-gc.v1";

/// Engine namespace for TokenZero.
pub const GC_ENGINE_TOKENZERO: &str = "tokenzero";

const GC_RECORD_TYPE_REACHABILITY: &str = "reachability-snapshot";
const GC_RECORD_TYPE_PIN: &str = "pin";
const GC_RECORD_TYPE_LEASE: &str = "lease";
const GC_RECORD_TYPE_DRY_RUN: &str = "dry-run-report";

/// Minimum lease grace period in seconds (schema requirement).
pub const GC_MIN_GRACE_SECONDS: u64 = 60;

/// Error taxonomy for the shared-CAS GC coordinator.
#[derive(Debug, Error)]
pub enum GcError {
    /// Underlying storage operation failed.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    /// JSON serialization or deserialization failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// Record violated the frozen v1 schema.
    #[error("schema violation: {0}")]
    SchemaViolation(String),
    /// Metadata was malformed, had wrong namespace, or unsupported version.
    #[error("corrupt metadata at {path}: {reason}")]
    CorruptMetadata { path: PathBuf, reason: String },
    /// A metadata read error or missing record prevented a safe deletion.
    #[error("uncertain metadata: {0}")]
    UncertainMetadata(String),
    /// Policy denied access (e.g. invalid engine or path too short).
    #[error("policy violation: {0}")]
    Policy(String),
    /// Injected fault boundary for crash-consistency tests.
    #[error("fault injected")]
    FaultInjected,
}

impl From<SharedCasError> for GcError {
    fn from(err: SharedCasError) -> Self {
        match err {
            SharedCasError::Io(e) => GcError::Io(e),
            SharedCasError::Corruption => GcError::CorruptMetadata {
                path: PathBuf::new(),
                reason: "CAS object corruption".into(),
            },
            SharedCasError::Policy => GcError::Policy("CAS policy violation".into()),
            SharedCasError::InvalidHash(s) => {
                GcError::SchemaViolation(format!("invalid CAS hash {s}"))
            }
            SharedCasError::NotFound => GcError::UncertainMetadata("CAS object not found".into()),
        }
    }
}

/// A reachability snapshot: the complete live blob-root set for one
/// engine/project namespace at a monotonically increasing epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilitySnapshot {
    pub schema_version: String,
    pub record_type: String,
    pub engine: String,
    pub project_id: String,
    pub epoch: u64,
    pub published_at: String,
    pub blob_hashes: Vec<String>,
}

/// A pin record: protects one blob independently of reachability snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinRecord {
    pub schema_version: String,
    pub record_type: String,
    pub engine: String,
    pub project_id: String,
    pub pin_id: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub blob_hash: String,
}

/// Owner identity for a lease record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseOwner {
    pub pid: u64,
    pub host: String,
}

/// A lease record: protects blobs used by one active operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRecord {
    pub schema_version: String,
    pub record_type: String,
    pub engine: String,
    pub project_id: String,
    pub operation_id: String,
    pub epoch: u64,
    pub owner: LeaseOwner,
    pub started_at: String,
    pub expires_at: String,
    pub grace_seconds: u64,
    pub blob_hashes: Vec<String>,
}

/// Verdict for a GC candidate object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GcVerdict {
    Retain,
    Collect,
    RetainUncertain,
}

/// One object entry in a dry-run report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcCandidate {
    pub blob_hash: String,
    pub verdict: GcVerdict,
    pub reason_codes: Vec<String>,
    pub evidence: Vec<String>,
}

/// A dry-run report conforming to `dry-run-report.schema.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunReport {
    pub schema_version: String,
    pub record_type: String,
    pub run_id: String,
    pub store_root: String,
    pub evaluated_at: String,
    pub objects: Vec<GcCandidate>,
}

/// Configuration for a GC run.
#[derive(Debug, Clone)]
pub struct GcConfig {
    pub run_id: String,
    pub grace_seconds: u64,
    pub min_age_seconds: u64,
    pub apply: bool,
    pub now: SystemTime,
    pub fault_after_deletes: Option<usize>,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            run_id: "gc-run".into(),
            grace_seconds: GC_MIN_GRACE_SECONDS,
            min_age_seconds: 0,
            apply: false,
            now: SystemTime::now(),
            fault_after_deletes: None,
        }
    }
}

/// Stable project identity: full lowercase SHA-256 of the canonicalized
/// absolute store-root path.
pub fn project_id(store_root: &Path) -> Result<String, GcError> {
    let canonical = store_root
        .canonicalize()
        .map_err(GcError::Io)?
        .to_string_lossy()
        .into_owned();
    Ok(sha256_hex(canonical.as_bytes()))
}

/// Atomic filesystem write: temp sibling, flush, rename, dirsync.
/// Reuses the plan-journal `atomic_write` conventions.
pub fn gc_atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|v| v.to_str()).unwrap_or("gc"),
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn gc_report_path(store_root: &Path, run_id: &str) -> PathBuf {
    store_root
        .join("gc")
        .join("reports")
        .join(format!("{}.json", run_id))
}

fn gc_progress_path(store_root: &Path, run_id: &str) -> PathBuf {
    store_root
        .join("gc")
        .join("reports")
        .join(format!("{}.progress.json", run_id))
}

fn reachability_snapshot_path(store_root: &Path, engine: &str, project_id: &str) -> PathBuf {
    store_root
        .join("gc")
        .join("roots")
        .join(engine)
        .join(project_id)
        .join("current.json")
}

fn pin_record_path(store_root: &Path, engine: &str, project_id: &str, pin_id: &str) -> PathBuf {
    store_root
        .join("gc")
        .join("pins")
        .join(engine)
        .join(project_id)
        .join(format!("{}.json", pin_id))
}

fn lease_record_path(
    store_root: &Path,
    engine: &str,
    project_id: &str,
    operation_id: &str,
) -> PathBuf {
    store_root
        .join("gc")
        .join("leases")
        .join(engine)
        .join(project_id)
        .join(format!("{}.json", operation_id))
}

fn validate_run_id(run_id: &str) -> Result<(), GcError> {
    if run_id.is_empty() {
        return Err(GcError::SchemaViolation("run_id empty".into()));
    }
    if run_id.len() > 128 {
        return Err(GcError::SchemaViolation("run_id too long".into()));
    }
    let bytes = run_id.as_bytes();
    let first = bytes[0];
    if !first.is_ascii_alphanumeric() {
        return Err(GcError::SchemaViolation(
            "run_id must start with alphanumeric".into(),
        ));
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'.' || *b == b'_' || *b == b'-')
    {
        return Err(GcError::SchemaViolation(
            "run_id contains invalid characters".into(),
        ));
    }
    Ok(())
}

/// Parse a relaxed RFC 3339 date-time string into a `SystemTime`.
fn parse_rfc3339(s: &str) -> Option<SystemTime> {
    if s.len() < 19 {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let year: i64 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    let hour: u32 = s[11..13].parse().ok()?;
    let minute: u32 = s[14..16].parse().ok()?;
    let second: u32 = s[17..19].parse().ok()?;
    let tail = &s[19..];
    let (nanos, tail) = if tail.starts_with('.') {
        let rest = &tail[1..];
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        let frac = &rest[..digits];
        let mut nano = frac.parse::<u64>().ok()?;
        let scale = 10u64.pow(9 - digits.min(9) as u32);
        nano *= scale;
        (nano, &rest[digits..])
    } else {
        (0u64, tail)
    };
    if tail != "Z" {
        if tail.len() != 6
            || !(tail.starts_with('+') || tail.starts_with('-'))
            || tail.as_bytes().get(3) != Some(&b':')
        {
            return None;
        }
    }
    let days = civil_to_days(year, month, day);
    let secs = days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64;
    Some(UNIX_EPOCH + std::time::Duration::new(secs as u64, nanos as u32))
}

fn civil_to_days(year: i64, month: u32, day: u32) -> i64 {
    let mut y = year;
    let mut m = month as i64;
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + (if m > 2 { -3 } else { 9 })) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn rfc3339_now() -> String {
    format_system_time(SystemTime::now())
}

fn format_system_time(t: SystemTime) -> String {
    let seconds = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day) = civil_from_days((seconds / 86_400) as i64);
    let rem = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

fn is_valid_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn is_valid_pin_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 128 {
        return false;
    }
    let first = s.as_bytes()[0];
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

fn is_valid_operation_id(s: &str) -> bool {
    is_valid_pin_id(s)
}

fn validate_namespace(path: &Path, engine: &str, project_id: &str) -> Result<(), GcError> {
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    if components.len() < 4 {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("path too short: {}", path.display()),
        });
    }
    let path_engine = components[components.len() - 3];
    let path_project = components[components.len() - 2];
    if path_engine != engine {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("engine mismatch: path {path_engine}, record {engine}"),
        });
    }
    if path_project != project_id {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("project_id mismatch: path {path_project}, record {project_id}"),
        });
    }
    Ok(())
}

fn read_json_file(path: &Path) -> Result<serde_json::Value, GcError> {
    let text = fs::read_to_string(path).map_err(GcError::Io)?;
    serde_json::from_str(&text).map_err(GcError::Json)
}

fn read_reachability_snapshot(path: &Path) -> Result<ReachabilitySnapshot, GcError> {
    let value = read_json_file(path)?;
    let snap: ReachabilitySnapshot =
        serde_json::from_value(value.clone()).map_err(GcError::Json)?;
    if snap.schema_version != GC_SCHEMA_VERSION {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("unsupported schema_version {}", snap.schema_version),
        });
    }
    if snap.record_type != GC_RECORD_TYPE_REACHABILITY {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("record_type {}", snap.record_type),
        });
    }
    if !matches!(snap.engine.as_str(), "tokenzero" | "fszero" | "graphzero") {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("invalid engine {}", snap.engine),
        });
    }
    validate_namespace(path, &snap.engine, &snap.project_id)?;
    if snap.epoch == 0 {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: "epoch must be >= 1".into(),
        });
    }
    if parse_rfc3339(&snap.published_at).is_none() {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: "invalid published_at".into(),
        });
    }
    for h in &snap.blob_hashes {
        if !is_valid_hash(h) {
            return Err(GcError::CorruptMetadata {
                path: path.to_path_buf(),
                reason: format!("invalid blob hash {h}"),
            });
        }
    }
    Ok(snap)
}

fn read_pin_record(path: &Path) -> Result<PinRecord, GcError> {
    let value = read_json_file(path)?;
    let pin: PinRecord = serde_json::from_value(value.clone()).map_err(GcError::Json)?;
    if pin.schema_version != GC_SCHEMA_VERSION {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("unsupported schema_version {}", pin.schema_version),
        });
    }
    if pin.record_type != GC_RECORD_TYPE_PIN {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("record_type {}", pin.record_type),
        });
    }
    if !matches!(pin.engine.as_str(), "tokenzero" | "fszero" | "graphzero") {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("invalid engine {}", pin.engine),
        });
    }
    validate_namespace(path, &pin.engine, &pin.project_id)?;
    if !is_valid_pin_id(&pin.pin_id) {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("invalid pin_id {}", pin.pin_id),
        });
    }
    if parse_rfc3339(&pin.created_at).is_none() {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: "invalid created_at".into(),
        });
    }
    if pin
        .expires_at
        .as_deref()
        .is_some_and(|s| parse_rfc3339(s).is_none())
    {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: "invalid expires_at".into(),
        });
    }
    if !is_valid_hash(&pin.blob_hash) {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: "invalid blob_hash".into(),
        });
    }
    Ok(pin)
}

fn read_lease_record(path: &Path) -> Result<LeaseRecord, GcError> {
    let value = read_json_file(path)?;
    let lease: LeaseRecord = serde_json::from_value(value.clone()).map_err(GcError::Json)?;
    if lease.schema_version != GC_SCHEMA_VERSION {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("unsupported schema_version {}", lease.schema_version),
        });
    }
    if lease.record_type != GC_RECORD_TYPE_LEASE {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("record_type {}", lease.record_type),
        });
    }
    if !matches!(lease.engine.as_str(), "tokenzero" | "fszero" | "graphzero") {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("invalid engine {}", lease.engine),
        });
    }
    validate_namespace(path, &lease.engine, &lease.project_id)?;
    if !is_valid_operation_id(&lease.operation_id) {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("invalid operation_id {}", lease.operation_id),
        });
    }
    if lease.epoch == 0 {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: "epoch must be >= 1".into(),
        });
    }
    if parse_rfc3339(&lease.started_at).is_none() {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: "invalid started_at".into(),
        });
    }
    if parse_rfc3339(&lease.expires_at).is_none() {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: "invalid expires_at".into(),
        });
    }
    if lease.grace_seconds < GC_MIN_GRACE_SECONDS {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!(
                "grace_seconds {} < {}",
                lease.grace_seconds, GC_MIN_GRACE_SECONDS
            ),
        });
    }
    for h in &lease.blob_hashes {
        if !is_valid_hash(h) {
            return Err(GcError::CorruptMetadata {
                path: path.to_path_buf(),
                reason: format!("invalid blob hash {h}"),
            });
        }
    }
    Ok(lease)
}

#[derive(Debug, Default)]
struct HashMeta {
    reasons: BTreeSet<String>,
    evidence: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct MarkState {
    live: BTreeMap<String, HashMeta>,
    uncertain: bool,
    global_evidence: Vec<String>,
}

fn mark_hash(state: &mut MarkState, hash: &str, reason: &str, evidence: &str) {
    let meta = state.live.entry(hash.to_string()).or_default();
    meta.reasons.insert(reason.to_string());
    meta.evidence.insert(evidence.to_string());
}

fn load_all_roots(store_root: &Path, state: &mut MarkState) -> Result<(), GcError> {
    let roots_dir = store_root.join("gc").join("roots");
    if !roots_dir.is_dir() {
        return Ok(());
    }
    for engine_entry in fs::read_dir(&roots_dir)? {
        let engine_entry = engine_entry?;
        let engine_dir = engine_entry.path();
        if !engine_dir.is_dir() {
            continue;
        }
        for project_entry in fs::read_dir(&engine_dir)? {
            let project_entry = project_entry?;
            let project_dir = project_entry.path();
            if !project_dir.is_dir() {
                continue;
            }
            let current = project_dir.join("current.json");
            if !current.is_file() {
                continue;
            }
            match read_reachability_snapshot(&current) {
                Ok(snap) => {
                    let evidence = format!("root {} epoch {}", current.display(), snap.epoch);
                    for h in &snap.blob_hashes {
                        mark_hash(state, h, "reachability-root", &evidence);
                    }
                }
                Err(err) => {
                    state.uncertain = true;
                    state
                        .global_evidence
                        .push(format!("{}: {}", current.display(), err));
                }
            }
        }
    }
    Ok(())
}

fn load_all_pins(store_root: &Path, state: &mut MarkState, now: SystemTime) -> Result<(), GcError> {
    let pins_dir = store_root.join("gc").join("pins");
    if !pins_dir.is_dir() {
        return Ok(());
    }
    for engine_entry in fs::read_dir(&pins_dir)? {
        let engine_entry = engine_entry?;
        let engine_dir = engine_entry.path();
        if !engine_dir.is_dir() {
            continue;
        }
        for project_entry in fs::read_dir(&engine_dir)? {
            let project_entry = project_entry?;
            let project_dir = project_entry.path();
            if !project_dir.is_dir() {
                continue;
            }
            for pin_entry in fs::read_dir(&project_dir)? {
                let pin_entry = pin_entry?;
                let path = pin_entry.path();
                if path.extension().and_then(|v| v.to_str()) != Some("json") {
                    continue;
                }
                match read_pin_record(&path) {
                    Ok(pin) => {
                        let evidence = format!("pin {}", path.display());
                        let expired = pin
                            .expires_at
                            .as_deref()
                            .and_then(parse_rfc3339)
                            .is_some_and(|exp| exp <= now);
                        if expired {
                            state.uncertain = true;
                            state.global_evidence.push(format!(
                                "expired pin {} retained on clock uncertainty",
                                path.display()
                            ));
                            mark_hash(state, &pin.blob_hash, "pin", &evidence);
                        } else {
                            mark_hash(state, &pin.blob_hash, "pin", &evidence);
                        }
                    }
                    Err(err) => {
                        state.uncertain = true;
                        state
                            .global_evidence
                            .push(format!("{}: {}", path.display(), err));
                    }
                }
            }
        }
    }
    Ok(())
}

fn load_all_leases(
    store_root: &Path,
    state: &mut MarkState,
    now: SystemTime,
    grace_seconds: u64,
) -> Result<(), GcError> {
    let leases_dir = store_root.join("gc").join("leases");
    if !leases_dir.is_dir() {
        return Ok(());
    }
    for engine_entry in fs::read_dir(&leases_dir)? {
        let engine_entry = engine_entry?;
        let engine_dir = engine_entry.path();
        if !engine_dir.is_dir() {
            continue;
        }
        for project_entry in fs::read_dir(&engine_dir)? {
            let project_entry = project_entry?;
            let project_dir = project_entry.path();
            if !project_dir.is_dir() {
                continue;
            }
            for lease_entry in fs::read_dir(&project_dir)? {
                let lease_entry = lease_entry?;
                let path = lease_entry.path();
                if path.extension().and_then(|v| v.to_str()) != Some("json") {
                    continue;
                }
                match read_lease_record(&path) {
                    Ok(lease) => {
                        let expires = parse_rfc3339(&lease.expires_at).unwrap_or(now);
                        let effective_grace = lease.grace_seconds.max(grace_seconds);
                        let grace_end = expires + std::time::Duration::from_secs(effective_grace);
                        if now <= expires {
                            for h in &lease.blob_hashes {
                                mark_hash(
                                    state,
                                    h,
                                    "active-lease",
                                    &format!("lease {}", path.display()),
                                );
                            }
                        } else if now < grace_end {
                            for h in &lease.blob_hashes {
                                mark_hash(
                                    state,
                                    h,
                                    "stale-lease-grace",
                                    &format!("lease {} inside grace", path.display()),
                                );
                            }
                        } else {
                            state.uncertain = true;
                            state.global_evidence.push(format!(
                                "lease {} stale outside grace; owner liveness unverified",
                                path.display()
                            ));
                            for h in &lease.blob_hashes {
                                mark_hash(
                                    state,
                                    h,
                                    "stale-lease-grace",
                                    &format!(
                                        "lease {} retained on uncertain liveness",
                                        path.display()
                                    ),
                                );
                            }
                        }
                    }
                    Err(err) => {
                        state.uncertain = true;
                        state
                            .global_evidence
                            .push(format!("{}: {}", path.display(), err));
                    }
                }
            }
        }
    }
    Ok(())
}

fn build_dry_run_report(
    store_root: &Path,
    run_id: &str,
    cas: &SharedCas,
    state: &MarkState,
    min_age_seconds: u64,
    now: SystemTime,
) -> Result<DryRunReport, GcError> {
    let mut objects = Vec::new();
    let cas_hashes = cas.list_objects()?;
    for hash in cas_hashes {
        let mut candidate = if let Some(meta) = state.live.get(&hash) {
            GcCandidate {
                blob_hash: hash,
                verdict: GcVerdict::Retain,
                reason_codes: meta.reasons.iter().cloned().collect(),
                evidence: meta.evidence.iter().cloned().collect(),
            }
        } else if state.uncertain {
            GcCandidate {
                blob_hash: hash,
                verdict: GcVerdict::RetainUncertain,
                reason_codes: vec!["uncertain-metadata".into()],
                evidence: state.global_evidence.clone(),
            }
        } else {
            let path = cas.object_path(&hash);
            let too_young = if let Ok(meta) = fs::metadata(&path) {
                if let Ok(modified) = meta.modified() {
                    let age = now.duration_since(modified).unwrap_or_default();
                    age.as_secs() < min_age_seconds
                } else {
                    true
                }
            } else {
                true
            };
            if too_young {
                GcCandidate {
                    blob_hash: hash,
                    verdict: GcVerdict::RetainUncertain,
                    reason_codes: vec!["uncertain-metadata".into()],
                    evidence: vec![format!("object younger than {} seconds", min_age_seconds)],
                }
            } else {
                GcCandidate {
                    blob_hash: hash,
                    verdict: GcVerdict::Collect,
                    reason_codes: vec!["no-live-reference".into()],
                    evidence: vec!["no reachable root, pin, or lease".into()],
                }
            }
        };
        if candidate.reason_codes.is_empty() {
            candidate.reason_codes.push("uncertain-metadata".into());
        }
        objects.push(candidate);
    }
    objects.sort_by(|a, b| a.blob_hash.cmp(&b.blob_hash));
    Ok(DryRunReport {
        schema_version: GC_SCHEMA_VERSION.to_string(),
        record_type: GC_RECORD_TYPE_DRY_RUN.to_string(),
        run_id: run_id.to_string(),
        store_root: store_root.to_string_lossy().into_owned(),
        evaluated_at: rfc3339_now(),
        objects,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SweepProgress {
    schema_version: String,
    record_type: String,
    run_id: String,
    store_root: String,
    evaluated_at: String,
    objects: Vec<String>,
    deleted: Vec<String>,
    state: String,
}

fn read_sweep_progress(path: &Path) -> Result<SweepProgress, GcError> {
    let text = fs::read_to_string(path).map_err(GcError::Io)?;
    let progress: SweepProgress = serde_json::from_str(&text).map_err(GcError::Json)?;
    Ok(progress)
}

fn build_final_report(report: &DryRunReport, deleted: &[String]) -> DryRunReport {
    let deleted_set: BTreeSet<String> = deleted.iter().cloned().collect();
    let mut final_report = report.clone();
    for obj in &mut final_report.objects {
        if obj.verdict == GcVerdict::Collect && deleted_set.contains(&obj.blob_hash) {
            obj.evidence.push("deleted by this sweep".into());
        } else if obj.verdict == GcVerdict::Collect && !deleted_set.contains(&obj.blob_hash) {
            obj.verdict = GcVerdict::RetainUncertain;
            obj.reason_codes = vec!["uncertain-metadata".into()];
            obj.evidence =
                vec!["re-check before delete showed a live reference or uncertainty".into()];
        }
    }
    final_report
}

/// Run a shared-CAS GC coordinator pass.
///
/// Default is dry-run: the report is written to `gc/reports/<run_id>.json`.
/// When `config.apply` is true, the function also deletes objects with verdict
/// `Collect` after an immediate re-check. A crash during sweep can be resumed
/// by calling `run_gc` again with the same `run_id`.
pub fn run_gc(store_root: &Path, config: &GcConfig) -> Result<DryRunReport, GcError> {
    validate_run_id(&config.run_id)?;
    let cas = SharedCas::new(store_root.to_path_buf());

    let progress_path = gc_progress_path(store_root, &config.run_id);
    let prior_progress = if progress_path.is_file() {
        Some(read_sweep_progress(&progress_path)?)
    } else {
        None
    };

    let mut state = MarkState::default();
    load_all_roots(store_root, &mut state)?;
    load_all_pins(store_root, &mut state, config.now)?;
    load_all_leases(store_root, &mut state, config.now, config.grace_seconds)?;

    let report = build_dry_run_report(
        store_root,
        &config.run_id,
        &cas,
        &state,
        config.min_age_seconds,
        config.now,
    )?;

    let report_path = gc_report_path(store_root, &config.run_id);
    let report_bytes = serde_json::to_vec_pretty(&report)?;
    gc_atomic_write(&report_path, &report_bytes)?;

    if !config.apply {
        return Ok(report);
    }

    let mut deleted = prior_progress
        .as_ref()
        .map(|p| p.deleted.clone())
        .unwrap_or_default();
    let to_delete: Vec<String> = report
        .objects
        .iter()
        .filter(|o| o.verdict == GcVerdict::Collect)
        .map(|o| o.blob_hash.clone())
        .collect();

    let progress = SweepProgress {
        schema_version: GC_SCHEMA_VERSION.to_string(),
        record_type: "sweep-progress".to_string(),
        run_id: config.run_id.clone(),
        store_root: store_root.to_string_lossy().into_owned(),
        evaluated_at: report.evaluated_at.clone(),
        objects: to_delete.clone(),
        deleted: deleted.clone(),
        state: "sweeping".to_string(),
    };
    gc_atomic_write(&progress_path, &serde_json::to_vec_pretty(&progress)?)?;

    for hash in &to_delete {
        let hash = hash.clone();
        if deleted.contains(&hash) {
            continue;
        }
        // Immediate re-check before deleting.
        let mut re_state = MarkState::default();
        load_all_roots(store_root, &mut re_state)?;
        load_all_pins(store_root, &mut re_state, config.now)?;
        load_all_leases(store_root, &mut re_state, config.now, config.grace_seconds)?;
        if re_state.live.contains_key(&hash) || re_state.uncertain {
            continue;
        }
        cas.remove_object(&hash)?;
        deleted.push(hash.clone());
        let progress = SweepProgress {
            schema_version: GC_SCHEMA_VERSION.to_string(),
            record_type: "sweep-progress".to_string(),
            run_id: config.run_id.clone(),
            store_root: store_root.to_string_lossy().into_owned(),
            evaluated_at: report.evaluated_at.clone(),
            objects: to_delete.clone(),
            deleted: deleted.clone(),
            state: "sweeping".to_string(),
        };
        gc_atomic_write(&progress_path, &serde_json::to_vec_pretty(&progress)?)?;
        if config.fault_after_deletes == Some(deleted.len()) {
            return Err(GcError::FaultInjected);
        }
    }

    let final_report = build_final_report(&report, &deleted);
    gc_atomic_write(&report_path, &serde_json::to_vec_pretty(&final_report)?)?;
    let _ = fs::remove_file(&progress_path);
    Ok(final_report)
}

/// Validate a `serde_json::Value` against the frozen dry-run-report schema.
/// This is a focused, exact validator for the v1 schema so tests can fail on
/// non-conforming output without adding heavy JSON Schema dependencies.
pub fn validate_dry_run_report(value: &serde_json::Value) -> Result<(), GcError> {
    for field in [
        "schema_version",
        "record_type",
        "run_id",
        "store_root",
        "evaluated_at",
        "objects",
    ] {
        if value.get(field).is_none() {
            return Err(GcError::SchemaViolation(format!("missing {field}")));
        }
    }
    if value.get("schema_version").and_then(|v| v.as_str()) != Some(GC_SCHEMA_VERSION) {
        return Err(GcError::SchemaViolation("schema_version".into()));
    }
    if value.get("record_type").and_then(|v| v.as_str()) != Some(GC_RECORD_TYPE_DRY_RUN) {
        return Err(GcError::SchemaViolation("record_type".into()));
    }
    let run_id = value
        .get("run_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GcError::SchemaViolation("run_id".into()))?;
    validate_run_id(run_id)?;
    let store_root = value
        .get("store_root")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GcError::SchemaViolation("store_root".into()))?;
    if store_root.is_empty() {
        return Err(GcError::SchemaViolation("store_root empty".into()));
    }
    let evaluated_at = value
        .get("evaluated_at")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GcError::SchemaViolation("evaluated_at".into()))?;
    if parse_rfc3339(evaluated_at).is_none() {
        return Err(GcError::SchemaViolation("evaluated_at".into()));
    }
    let objects = value
        .get("objects")
        .and_then(|v| v.as_array())
        .ok_or_else(|| GcError::SchemaViolation("objects".into()))?;
    let mut seen = BTreeSet::new();
    for obj in objects {
        if !seen.insert(obj.to_string()) {
            return Err(GcError::SchemaViolation("duplicate object".into()));
        }
        validate_candidate(obj)?;
    }
    let keys: BTreeSet<String> = value.as_object().unwrap().keys().cloned().collect();
    let expected: BTreeSet<String> = [
        "schema_version",
        "record_type",
        "run_id",
        "store_root",
        "evaluated_at",
        "objects",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if keys != expected {
        return Err(GcError::SchemaViolation(format!(
            "extra top-level keys: {:?}",
            keys.difference(&expected)
        )));
    }
    Ok(())
}

fn validate_candidate(value: &serde_json::Value) -> Result<(), GcError> {
    for field in ["blob_hash", "verdict", "reason_codes", "evidence"] {
        if value.get(field).is_none() {
            return Err(GcError::SchemaViolation(format!("missing {field}")));
        }
    }
    let blob_hash = value
        .get("blob_hash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GcError::SchemaViolation("blob_hash".into()))?;
    if !is_valid_hash(blob_hash) {
        return Err(GcError::SchemaViolation("blob_hash".into()));
    }
    let verdict = value
        .get("verdict")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GcError::SchemaViolation("verdict".into()))?;
    if !matches!(verdict, "retain" | "collect" | "retain-uncertain") {
        return Err(GcError::SchemaViolation("verdict".into()));
    }
    let reason_codes = value
        .get("reason_codes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| GcError::SchemaViolation("reason_codes".into()))?;
    if reason_codes.is_empty() {
        return Err(GcError::SchemaViolation("reason_codes empty".into()));
    }
    let mut reasons = BTreeSet::new();
    for code in reason_codes {
        let s = code
            .as_str()
            .ok_or_else(|| GcError::SchemaViolation("reason_code".into()))?;
        if !matches!(
            s,
            "reachability-root"
                | "pin"
                | "active-lease"
                | "stale-lease-grace"
                | "shared-root"
                | "unknown-version"
                | "corrupt-metadata"
                | "uncertain-metadata"
                | "unpublished-temp"
                | "namespace-isolation"
                | "no-live-reference"
        ) {
            return Err(GcError::SchemaViolation(format!("reason_code {s}")));
        }
        if !reasons.insert(s) {
            return Err(GcError::SchemaViolation("duplicate reason_code".into()));
        }
    }
    let evidence = value
        .get("evidence")
        .and_then(|v| v.as_array())
        .ok_or_else(|| GcError::SchemaViolation("evidence".into()))?;
    let mut ev = BTreeSet::new();
    for e in evidence {
        let s = e
            .as_str()
            .ok_or_else(|| GcError::SchemaViolation("evidence".into()))?;
        if s.is_empty() {
            return Err(GcError::SchemaViolation("empty evidence".into()));
        }
        if !ev.insert(s) {
            return Err(GcError::SchemaViolation("duplicate evidence".into()));
        }
    }
    let keys: BTreeSet<String> = value.as_object().unwrap().keys().cloned().collect();
    let expected: BTreeSet<String> = ["blob_hash", "verdict", "reason_codes", "evidence"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if keys != expected {
        return Err(GcError::SchemaViolation("extra object keys".into()));
    }
    Ok(())
}

/// Publish a reachability snapshot in the shared-CAS GC namespace.
pub fn publish_reachability_snapshot(
    store_root: &Path,
    engine: &str,
    project_id: &str,
    epoch: u64,
    blob_hashes: &[String],
) -> Result<PathBuf, GcError> {
    if !matches!(engine, "tokenzero" | "fszero" | "graphzero") {
        return Err(GcError::Policy(format!("invalid engine {engine}")));
    }
    if !is_valid_hash(project_id) {
        return Err(GcError::SchemaViolation("project_id".into()));
    }
    for h in blob_hashes {
        if !is_valid_hash(h) {
            return Err(GcError::Policy(format!("invalid hash {h}")));
        }
    }
    let mut hashes = blob_hashes.to_vec();
    hashes.sort_unstable();
    hashes.dedup();
    let snap = ReachabilitySnapshot {
        schema_version: GC_SCHEMA_VERSION.to_string(),
        record_type: GC_RECORD_TYPE_REACHABILITY.to_string(),
        engine: engine.to_string(),
        project_id: project_id.to_string(),
        epoch,
        published_at: rfc3339_now(),
        blob_hashes: hashes,
    };
    let path = reachability_snapshot_path(store_root, engine, project_id);
    let bytes = serde_json::to_vec_pretty(&snap)?;
    gc_atomic_write(&path, &bytes)?;
    Ok(path)
}

/// Publish a pin record in the shared-CAS GC namespace.
pub fn publish_pin_record(store_root: &Path, pin: &PinRecord) -> Result<PathBuf, GcError> {
    if pin.schema_version != GC_SCHEMA_VERSION {
        return Err(GcError::SchemaViolation("schema_version".into()));
    }
    if pin.record_type != GC_RECORD_TYPE_PIN {
        return Err(GcError::SchemaViolation("record_type".into()));
    }
    if !matches!(pin.engine.as_str(), "tokenzero" | "fszero" | "graphzero") {
        return Err(GcError::Policy(format!("invalid engine {}", pin.engine)));
    }
    if !is_valid_hash(&pin.project_id) {
        return Err(GcError::SchemaViolation("project_id".into()));
    }
    if !is_valid_pin_id(&pin.pin_id) {
        return Err(GcError::SchemaViolation("pin_id".into()));
    }
    if !is_valid_hash(&pin.blob_hash) {
        return Err(GcError::SchemaViolation("blob_hash".into()));
    }
    let path = pin_record_path(store_root, &pin.engine, &pin.project_id, &pin.pin_id);
    let bytes = serde_json::to_vec_pretty(pin)?;
    gc_atomic_write(&path, &bytes)?;
    Ok(path)
}

/// Publish a lease record in the shared-CAS GC namespace.
pub fn publish_lease_record(store_root: &Path, lease: &LeaseRecord) -> Result<PathBuf, GcError> {
    if lease.schema_version != GC_SCHEMA_VERSION {
        return Err(GcError::SchemaViolation("schema_version".into()));
    }
    if lease.record_type != GC_RECORD_TYPE_LEASE {
        return Err(GcError::SchemaViolation("record_type".into()));
    }
    if !matches!(lease.engine.as_str(), "tokenzero" | "fszero" | "graphzero") {
        return Err(GcError::Policy(format!("invalid engine {}", lease.engine)));
    }
    if !is_valid_hash(&lease.project_id) {
        return Err(GcError::SchemaViolation("project_id".into()));
    }
    if !is_valid_operation_id(&lease.operation_id) {
        return Err(GcError::SchemaViolation("operation_id".into()));
    }
    if lease.grace_seconds < GC_MIN_GRACE_SECONDS {
        return Err(GcError::SchemaViolation(format!(
            "grace_seconds < {}",
            GC_MIN_GRACE_SECONDS
        )));
    }
    if parse_rfc3339(&lease.expires_at).is_none() {
        return Err(GcError::SchemaViolation("expires_at".into()));
    }
    if parse_rfc3339(&lease.started_at).is_none() {
        return Err(GcError::SchemaViolation("started_at".into()));
    }
    for h in &lease.blob_hashes {
        if !is_valid_hash(h) {
            return Err(GcError::SchemaViolation("blob_hash".into()));
        }
    }
    let path = lease_record_path(
        store_root,
        &lease.engine,
        &lease.project_id,
        &lease.operation_id,
    );
    let bytes = serde_json::to_vec_pretty(lease)?;
    gc_atomic_write(&path, &bytes)?;
    Ok(path)
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

    use std::time::Duration;

    fn hash_bytes(bytes: &[u8]) -> String {
        sha256_hex(bytes)
    }

    fn make_store() -> (tempfile::TempDir, PathBuf, SharedCas) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let cas = SharedCas::new(root.clone());
        (dir, root, cas)
    }

    fn pid(root: &Path) -> String {
        project_id(root).unwrap()
    }

    fn make_snapshot(
        store_root: &Path,
        engine: &str,
        project_id: &str,
        epoch: u64,
        blob_hashes: &[String],
    ) {
        publish_reachability_snapshot(store_root, engine, project_id, epoch, blob_hashes).unwrap();
    }

    fn make_pin(store_root: &Path, engine: &str, project_id: &str, pin_id: &str, blob_hash: &str) {
        let pin = PinRecord {
            schema_version: GC_SCHEMA_VERSION.to_string(),
            record_type: GC_RECORD_TYPE_PIN.to_string(),
            engine: engine.to_string(),
            project_id: project_id.to_string(),
            pin_id: pin_id.to_string(),
            created_at: rfc3339_now(),
            expires_at: None,
            blob_hash: blob_hash.to_string(),
        };
        publish_pin_record(store_root, &pin).unwrap();
    }

    fn make_lease(
        store_root: &Path,
        engine: &str,
        project_id: &str,
        operation_id: &str,
        blob_hashes: &[String],
        expires_at: SystemTime,
    ) {
        let started_at = format_system_time(UNIX_EPOCH + Duration::from_secs(0));
        let expires_at = format_system_time(expires_at);
        let lease = LeaseRecord {
            schema_version: GC_SCHEMA_VERSION.to_string(),
            record_type: GC_RECORD_TYPE_LEASE.to_string(),
            engine: engine.to_string(),
            project_id: project_id.to_string(),
            operation_id: operation_id.to_string(),
            epoch: 1,
            owner: LeaseOwner {
                pid: 1,
                host: "localhost".to_string(),
            },
            started_at,
            expires_at,
            grace_seconds: GC_MIN_GRACE_SECONDS,
            blob_hashes: blob_hashes.to_vec(),
        };
        publish_lease_record(store_root, &lease).unwrap();
    }

    #[test]
    fn gc_dry_run_report_validates_against_schema() {
        let (_dir, root, cas) = make_store();
        let h1 = cas.publish(b"reachable").unwrap();
        cas.publish(b"orphan").unwrap();

        make_snapshot(&root, GC_ENGINE_TOKENZERO, &pid(&root), 1, &[h1.clone()]);

        let config = GcConfig::default();
        let report = run_gc(&root, &config).unwrap();
        let value = serde_json::to_value(&report).unwrap();
        validate_dry_run_report(&value).unwrap();

        let path = root.join("gc").join("reports").join("gc-run.json");
        assert!(path.is_file());
        let on_disk: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        validate_dry_run_report(&on_disk).unwrap();
    }

    #[test]
    fn gc_retain_reachable_from_roots() {
        let (_dir, root, cas) = make_store();
        let h1 = cas.publish(b"reachable").unwrap();
        let h2 = cas.publish(b"orphan").unwrap();
        make_snapshot(&root, GC_ENGINE_TOKENZERO, &pid(&root), 1, &[h1.clone()]);

        let report = run_gc(&root, &GcConfig::default()).unwrap();
        let r1 = report.objects.iter().find(|o| o.blob_hash == h1).unwrap();
        let r2 = report.objects.iter().find(|o| o.blob_hash == h2).unwrap();
        assert_eq!(r1.verdict, GcVerdict::Retain);
        assert!(r1.reason_codes.contains(&"reachability-root".to_string()));
        assert_eq!(r2.verdict, GcVerdict::Collect);
    }

    #[test]
    fn gc_retain_pinned_blobs() {
        let (_dir, root, cas) = make_store();
        let h1 = cas.publish(b"pinned").unwrap();
        let h2 = cas.publish(b"orphan").unwrap();
        make_pin(&root, GC_ENGINE_TOKENZERO, &pid(&root), "pin-1", &h1);

        let report = run_gc(&root, &GcConfig::default()).unwrap();
        let r1 = report.objects.iter().find(|o| o.blob_hash == h1).unwrap();
        let r2 = report.objects.iter().find(|o| o.blob_hash == h2).unwrap();
        assert_eq!(r1.verdict, GcVerdict::Retain);
        assert!(r1.reason_codes.contains(&"pin".to_string()));
        assert_eq!(r2.verdict, GcVerdict::Collect);
    }

    #[test]
    fn gc_retain_active_leases() {
        let (_dir, root, cas) = make_store();
        let h1 = cas.publish(b"leased").unwrap();
        let h2 = cas.publish(b"orphan").unwrap();
        let future = SystemTime::now() + Duration::from_secs(3600);
        make_lease(
            &root,
            GC_ENGINE_TOKENZERO,
            &pid(&root),
            "lease-1",
            &[h1.clone()],
            future,
        );

        let report = run_gc(&root, &GcConfig::default()).unwrap();
        let r1 = report.objects.iter().find(|o| o.blob_hash == h1).unwrap();
        let r2 = report.objects.iter().find(|o| o.blob_hash == h2).unwrap();
        assert_eq!(r1.verdict, GcVerdict::Retain);
        assert!(r1.reason_codes.contains(&"active-lease".to_string()));
        assert_eq!(r2.verdict, GcVerdict::Collect);
    }

    #[test]
    fn gc_retain_stale_leases_inside_grace() {
        let (_dir, root, cas) = make_store();
        let h1 = cas.publish(b"stale-inside-grace").unwrap();
        let h2 = cas.publish(b"orphan").unwrap();
        let past = SystemTime::now() - Duration::from_secs(30);
        make_lease(
            &root,
            GC_ENGINE_TOKENZERO,
            &pid(&root),
            "lease-1",
            &[h1.clone()],
            past,
        );

        let report = run_gc(&root, &GcConfig::default()).unwrap();
        let r1 = report.objects.iter().find(|o| o.blob_hash == h1).unwrap();
        let r2 = report.objects.iter().find(|o| o.blob_hash == h2).unwrap();
        assert_eq!(r1.verdict, GcVerdict::Retain);
        assert!(r1.reason_codes.contains(&"stale-lease-grace".to_string()));
        assert_eq!(r2.verdict, GcVerdict::Collect);
    }

    #[test]
    fn gc_collect_unreachable_aged() {
        let (_dir, root, cas) = make_store();
        let h1 = cas.publish(b"orphan").unwrap();
        let config = GcConfig {
            apply: true,
            ..GcConfig::default()
        };
        let report = run_gc(&root, &config).unwrap();
        let r1 = report.objects.iter().find(|o| o.blob_hash == h1).unwrap();
        assert_eq!(r1.verdict, GcVerdict::Collect);
        assert!(!cas.contains(&h1));
    }

    #[test]
    fn gc_retain_uncertain_on_corrupt_metadata() {
        let (_dir, root, cas) = make_store();
        let h1 = cas.publish(b"orphan").unwrap();
        let corrupt_path = root
            .join("gc")
            .join("roots")
            .join(GC_ENGINE_TOKENZERO)
            .join(pid(&root))
            .join("current.json");
        fs::create_dir_all(corrupt_path.parent().unwrap()).unwrap();
        fs::write(&corrupt_path, b"not json").unwrap();

        let report = run_gc(&root, &GcConfig::default()).unwrap();
        let r1 = report.objects.iter().find(|o| o.blob_hash == h1).unwrap();
        assert_eq!(r1.verdict, GcVerdict::RetainUncertain);
        assert!(r1.reason_codes.contains(&"uncertain-metadata".to_string()));
    }

    #[test]
    fn gc_retain_uncertain_on_unknown_version() {
        let (_dir, root, cas) = make_store();
        let h1 = cas.publish(b"orphan").unwrap();
        let path =
            publish_reachability_snapshot(&root, GC_ENGINE_TOKENZERO, &pid(&root), 1, &[]).unwrap();
        let snap = ReachabilitySnapshot {
            schema_version: "zerostack.cas-gc.v2".to_string(),
            record_type: GC_RECORD_TYPE_REACHABILITY.to_string(),
            engine: GC_ENGINE_TOKENZERO.to_string(),
            project_id: pid(&root),
            epoch: 1,
            published_at: rfc3339_now(),
            blob_hashes: vec![],
        };
        fs::write(&path, serde_json::to_vec_pretty(&snap).unwrap()).unwrap();

        let report = run_gc(&root, &GcConfig::default()).unwrap();
        let r1 = report.objects.iter().find(|o| o.blob_hash == h1).unwrap();
        assert_eq!(r1.verdict, GcVerdict::RetainUncertain);
    }

    #[test]
    fn gc_namespace_isolation_independent() {
        let (_dir, root, cas) = make_store();
        let h1 = cas.publish(b"shared").unwrap();
        let pid1 = pid(&root);
        let sibling = root.join("sibling");
        fs::create_dir_all(&sibling).unwrap();
        let pid2 = project_id(&sibling).unwrap();
        make_snapshot(&root, "fszero", &pid2, 1, &[]);
        make_pin(&root, "fszero", &pid2, "pin-2", &h1);
        make_snapshot(&root, GC_ENGINE_TOKENZERO, &pid1, 1, &[]);

        let report = run_gc(&root, &GcConfig::default()).unwrap();
        let r1 = report.objects.iter().find(|o| o.blob_hash == h1).unwrap();
        assert_eq!(r1.verdict, GcVerdict::Retain);
        assert!(r1.reason_codes.contains(&"pin".to_string()));
    }

    #[test]
    fn gc_fault_injection_mid_sweep_resumes_consistent() {
        let (_dir, root, cas) = make_store();
        let reachable = cas.publish(b"reachable").unwrap();
        let orphan1 = cas.publish(b"orphan1").unwrap();
        let orphan2 = cas.publish(b"orphan2").unwrap();
        make_snapshot(
            &root,
            GC_ENGINE_TOKENZERO,
            &pid(&root),
            1,
            &[reachable.clone()],
        );

        let mut config = GcConfig {
            apply: true,
            fault_after_deletes: Some(1),
            ..GcConfig::default()
        };
        config.run_id = "fault-run".into();
        let result = run_gc(&root, &config);
        assert!(matches!(result, Err(GcError::FaultInjected)));

        let remaining = cas.list_objects().unwrap();
        assert!(remaining.contains(&reachable));
        assert!(remaining.contains(&orphan1) || remaining.contains(&orphan2));
        assert_eq!(remaining.len(), 2);

        config.fault_after_deletes = None;
        let _report = run_gc(&root, &config).unwrap();
        let remaining = cas.list_objects().unwrap();
        assert!(remaining.contains(&reachable));
        assert!(!remaining.contains(&orphan1));
        assert!(!remaining.contains(&orphan2));
        assert_eq!(remaining.len(), 1);

        // The final report may still list the last-deleted orphan as Collect
        // (it was a valid candidate at the start of the resumed sweep). What
        // matters is the store ends with only the reachable object.
        let remaining = cas.list_objects().unwrap();
        assert_eq!(remaining, vec![reachable.clone()]);
    }

    #[test]
    fn repair_missing_blob() {
        let (_dir, _root, cas) = make_store();
        let bytes = b"repair me";
        let hash = hash_bytes(bytes);
        let repaired = cas.repair_object(&hash, bytes).unwrap();
        assert!(repaired);
        assert!(cas.contains(&hash));
        assert_eq!(cas.resolve(&hash).unwrap(), bytes);
    }

    #[test]
    fn repair_corrupt_blob() {
        let (_dir, _root, cas) = make_store();
        let bytes = b"repair me";
        let hash = cas.publish(bytes).unwrap();
        let path = cas.object_path(&hash);
        fs::write(&path, b"corrupted").unwrap();
        assert!(matches!(
            cas.resolve(&hash),
            Err(SharedCasError::Corruption)
        ));
        let repaired = cas.repair_object(&hash, bytes).unwrap();
        assert!(repaired);
        assert_eq!(cas.resolve(&hash).unwrap(), bytes);
    }

    #[test]
    fn repair_valid_blob_is_no_op() {
        let (_dir, _root, cas) = make_store();
        let bytes = b"repair me";
        let hash = cas.publish(bytes).unwrap();
        let repaired = cas.repair_object(&hash, bytes).unwrap();
        assert!(!repaired);
    }
}
