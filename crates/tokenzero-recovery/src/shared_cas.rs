//! Canonical shared content-addressed storage (CAS) for ZeroRef v1 blobs.
//!
//! Immutable objects live at `<root>/blobs/sha256/<first-two-hex>/<full-hash>`.
//! Shared-CAS tier for full-hash portable refs (`tz://blob/<sha256>` and
//! `fz`/`gz` aliases). Legacy private JSON recovery remains a separate tier.

use fs4::FileExt;
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

    /// Enumerate all full-hash objects currently present in the shared CAS.
    /// Temp files, non-regular files, and prefix-directory symlinks are ignored.
    /// Listing never follows directory symlinks under the CAS root.
    pub fn list_objects(&self) -> Result<Vec<String>, SharedCasError> {
        let mut objects = Vec::new();
        let base = self.root.join("blobs").join("sha256");
        if !dir_is_real(&base) {
            return Ok(objects);
        }
        for prefix_entry in fs::read_dir(&base)? {
            let prefix_entry = prefix_entry?;
            let prefix_dir = prefix_entry.path();
            // Refuse to follow prefix symlinks that could escape the CAS root.
            if !entry_is_real_dir(&prefix_entry) {
                continue;
            }
            for entry in fs::read_dir(&prefix_dir)? {
                let entry = entry?;
                let path = entry.path();
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if ft.is_symlink() || !ft.is_file() {
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
                    // Containment: reconstructed object path must stay under CAS root
                    // and must not be a symlink.
                    if !self.path_is_contained_object(&path, name_str) {
                        continue;
                    }
                    objects.push(name_str.to_string());
                }
            }
        }
        Ok(objects)
    }

    /// Remove a full-hash object from the shared CAS. Idempotent: a missing
    /// object is not an error. Refuses to follow prefix symlinks or delete
    /// paths that resolve outside the CAS root.
    pub fn remove_object(&self, full_hash: &str) -> Result<(), SharedCasError> {
        self.validate_hash(full_hash)?;
        let path = self.object_path(full_hash);
        // Structural containment of the reconstructed path (no symlink parents).
        if !self.object_path_chain_is_safe(full_hash) {
            return Err(SharedCasError::Policy);
        }
        // Use symlink_metadata so we never follow a final-component symlink.
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if meta.file_type().is_symlink() || !meta.file_type().is_file() {
                    return Err(SharedCasError::Policy);
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        }
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
        self.root
            .join("blobs")
            .join("sha256")
            .join(&full_hash[..2])
            .join(full_hash)
    }

    /// True when `path` is the canonical object location for `full_hash` under
    /// this CAS root, no path component is a symlink, and the path does not
    /// escape the store root. Requires the final object to exist as a regular
    /// non-symlink file (used by listing).
    fn path_is_contained_object(&self, path: &Path, full_hash: &str) -> bool {
        if path != self.object_path(full_hash) {
            return false;
        }
        if !self.object_path_chain_is_safe(full_hash) {
            return false;
        }
        match fs::symlink_metadata(path) {
            Ok(meta) => !meta.file_type().is_symlink() && meta.file_type().is_file(),
            Err(_) => false,
        }
    }

    /// Verify the reconstructed object path stays under the CAS root and that
    /// no existing path component on the way is a symlink. Missing components
    /// are allowed (object may not exist yet).
    fn object_path_chain_is_safe(&self, full_hash: &str) -> bool {
        if self.validate_hash(full_hash).is_err() {
            return false;
        }
        let expected = self.object_path(full_hash);
        let relative = match expected.strip_prefix(&self.root) {
            Ok(rel) => rel,
            Err(_) => return false,
        };
        let mut cur = self.root.clone();
        // Root itself must not be a symlink.
        match fs::symlink_metadata(&cur) {
            Ok(meta) if meta.file_type().is_symlink() => return false,
            Ok(_) => {}
            Err(_) => return false,
        }
        for component in relative.components() {
            cur = cur.join(component);
            match fs::symlink_metadata(&cur) {
                Ok(meta) if meta.file_type().is_symlink() => return false,
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {
                    // Remaining path is absent; structural reconstruction is still
                    // under root and no symlink was observed.
                    return true;
                }
                Err(_) => return false,
            }
        }
        true
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

fn dir_is_real(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(meta) => meta.is_dir() && !meta.file_type().is_symlink(),
        Err(_) => false,
    }
}

fn entry_is_real_dir(entry: &fs::DirEntry) -> bool {
    match entry.file_type() {
        Ok(ft) => ft.is_dir() && !ft.is_symlink(),
        Err(_) => false,
    }
}

/// Parse an RFC 3339 date-time string into a `SystemTime`.
/// Validates calendar field ranges, applies signed numeric offsets, and
/// rejects malformed timestamps. Offset and `Z` are required.
fn parse_rfc3339(s: &str) -> Option<SystemTime> {
    if s.len() < 20 {
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
    if !(1..=12).contains(&month) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let max_day = days_in_month(year, month)?;
    if day == 0 || day > max_day {
        return None;
    }
    let tail = &s[19..];
    let (nanos, tail) = if tail.starts_with('.') {
        let rest = tail.strip_prefix('.').unwrap();
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        // Cap fractional precision at nanoseconds; extra digits are truncated.
        let take = digits.min(9);
        let frac = &rest[..take];
        let mut nano = frac.parse::<u64>().ok()?;
        let scale = 10u64.pow(9 - take as u32);
        nano *= scale;
        (nano, &rest[digits..])
    } else {
        (0u64, tail)
    };
    let offset_secs: i64 = if tail == "Z" || tail == "z" {
        0
    } else {
        if tail.len() != 6
            || !(tail.starts_with('+') || tail.starts_with('-'))
            || tail.as_bytes().get(3) != Some(&b':')
        {
            return None;
        }
        let sign: i64 = if tail.starts_with('+') { 1 } else { -1 };
        let off_h: i64 = tail[1..3].parse().ok()?;
        let off_m: i64 = tail[4..6].parse().ok()?;
        if off_h > 23 || off_m > 59 {
            return None;
        }
        sign * (off_h * 3600 + off_m * 60)
    };
    let days = civil_to_days(year, month, day);
    let local_secs = days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64;
    // Convert local civil time to UTC by subtracting the offset.
    let utc_secs = local_secs.checked_sub(offset_secs)?;
    if utc_secs < 0 {
        return None;
    }
    Some(UNIX_EPOCH + std::time::Duration::new(utc_secs as u64, nanos as u32))
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month(year: i64, month: u32) -> Option<u32> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 => Some(if is_leap_year(year) { 29 } else { 28 }),
        _ => None,
    }
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
        // No roots namespace at all: treat as missing reachability metadata.
        // Conservative retention — never interpret absence as authoritative empty.
        state.uncertain = true;
        state
            .global_evidence
            .push("missing gc/roots directory; reachability metadata absent".into());
        return Ok(());
    }
    let mut saw_any_project = false;
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
            saw_any_project = true;
            let current = project_dir.join("current.json");
            if !current.is_file() {
                // Project namespace exists but current snapshot is missing:
                // uncertain, not authoritative empty.
                state.uncertain = true;
                state.global_evidence.push(format!(
                    "missing reachability snapshot {}",
                    current.display()
                ));
                continue;
            }
            match read_reachability_snapshot(&current) {
                Ok(snap) => {
                    // Present valid snapshot is authoritative for this project,
                    // including blob_hashes=[] (true empty live set).
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
    if !saw_any_project {
        state.uncertain = true;
        state
            .global_evidence
            .push("gc/roots has no project namespaces; reachability metadata absent".into());
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
    validate_sweep_progress(&progress, path)?;
    Ok(progress)
}

const GC_RECORD_TYPE_SWEEP_PROGRESS: &str = "sweep-progress";

fn validate_sweep_progress(progress: &SweepProgress, path: &Path) -> Result<(), GcError> {
    if progress.schema_version != GC_SCHEMA_VERSION {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("unsupported schema_version {}", progress.schema_version),
        });
    }
    if progress.record_type != GC_RECORD_TYPE_SWEEP_PROGRESS {
        return Err(GcError::CorruptMetadata {
            path: path.to_path_buf(),
            reason: format!("record_type {}", progress.record_type),
        });
    }
    if progress.run_id.is_empty() {
        return Err(GcError::SchemaViolation("run_id empty".into()));
    }
    if progress.store_root.is_empty() {
        return Err(GcError::SchemaViolation("store_root empty".into()));
    }
    for h in progress.objects.iter().chain(progress.deleted.iter()) {
        if !is_valid_hash(h) {
            return Err(GcError::CorruptMetadata {
                path: path.to_path_buf(),
                reason: format!("invalid blob hash {h}"),
            });
        }
    }
    Ok(())
}

/// Shared exclusive lock serializing GC check/delete with root/pin/lease publication.
/// Lock ordering is single-level: only this file is locked by metadata publishers
/// and GC, so there is no multi-lock deadlock.
fn gc_coord_lock_path(store_root: &Path) -> PathBuf {
    store_root.join("gc").join("coordinator.lock")
}

struct GcCoordLock {
    file: File,
}

impl GcCoordLock {
    fn acquire(store_root: &Path) -> Result<Self, GcError> {
        // Truncating the lock file is safe: the advisory lock is held on the
        // file descriptor, and the file contents are never read.
        let path = gc_coord_lock_path(store_root);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(GcError::Io)?;
        // Blocking exclusive lock; single lock file for all GC metadata writers.
        FileExt::lock(&file).map_err(GcError::Io)?;
        Ok(Self { file })
    }
}

impl Drop for GcCoordLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
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
    // Hold the shared coordinator lock for the entire mark/recheck/delete path
    // so root/pin/lease publishers cannot interleave between recheck and remove.
    let _coord = GcCoordLock::acquire(store_root)?;
    let cas = SharedCas::new(store_root.to_path_buf());
    let store_root_key = store_root.to_string_lossy().into_owned();

    let progress_path = gc_progress_path(store_root, &config.run_id);
    let prior_progress = if progress_path.is_file() {
        let progress = read_sweep_progress(&progress_path)?;
        // Resume only journals that match this run and store identity.
        if progress.run_id != config.run_id {
            return Err(GcError::SchemaViolation(format!(
                "progress run_id {} does not match config {}",
                progress.run_id, config.run_id
            )));
        }
        if progress.store_root != store_root_key {
            return Err(GcError::SchemaViolation(format!(
                "progress store_root {} does not match {}",
                progress.store_root, store_root_key
            )));
        }
        Some(progress)
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

    // Candidate set is from this evaluation. Prior deleted entries are only
    // trusted when the object is still absent; republished objects are
    // re-evaluated so resume reports stay truthful.
    let mut deleted: Vec<String> = Vec::new();
    if let Some(prior) = prior_progress.as_ref() {
        for h in &prior.deleted {
            if !cas.contains(h) {
                deleted.push(h.clone());
            }
            // If the hash was republished after a crash-delete, do not carry
            // it as deleted; it must be rechecked against live metadata.
        }
    }
    let to_delete: Vec<String> = report
        .objects
        .iter()
        .filter(|o| o.verdict == GcVerdict::Collect)
        .map(|o| o.blob_hash.clone())
        .collect();

    let progress = SweepProgress {
        schema_version: GC_SCHEMA_VERSION.to_string(),
        record_type: GC_RECORD_TYPE_SWEEP_PROGRESS.to_string(),
        run_id: config.run_id.clone(),
        store_root: store_root_key.clone(),
        evaluated_at: report.evaluated_at.clone(),
        objects: to_delete.clone(),
        deleted: deleted.clone(),
        state: "sweeping".to_string(),
    };
    gc_atomic_write(&progress_path, &serde_json::to_vec_pretty(&progress)?)?;

    for hash in &to_delete {
        let hash = hash.clone();
        if deleted.contains(&hash) {
            // Only skip when still absent after reconciliation above.
            continue;
        }
        // Immediate re-check under the same coordinator lock before deleting.
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
            record_type: GC_RECORD_TYPE_SWEEP_PROGRESS.to_string(),
            run_id: config.run_id.clone(),
            store_root: store_root_key.clone(),
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
/// Epoch must be >= 1 and strictly greater than any currently published epoch
/// for the same engine/project. Serialized against GC via the coordinator lock.
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
    if epoch == 0 {
        return Err(GcError::SchemaViolation("epoch must be >= 1".into()));
    }
    for h in blob_hashes {
        if !is_valid_hash(h) {
            return Err(GcError::Policy(format!("invalid hash {h}")));
        }
    }
    let _coord = GcCoordLock::acquire(store_root)?;
    let path = reachability_snapshot_path(store_root, engine, project_id);
    if path.is_file() {
        match read_reachability_snapshot(&path) {
            Ok(existing) => {
                if epoch <= existing.epoch {
                    return Err(GcError::SchemaViolation(format!(
                        "epoch {epoch} must be strictly greater than current {}",
                        existing.epoch
                    )));
                }
            }
            Err(_err) => {
                // Unreadable current snapshot: allow replacement with a fresh
                // valid positive epoch (already checked).
            }
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
    let _coord = GcCoordLock::acquire(store_root)?;
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
    let _coord = GcCoordLock::acquire(store_root)?;
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

        make_snapshot(
            &root,
            GC_ENGINE_TOKENZERO,
            &pid(&root),
            1,
            std::slice::from_ref(&h1),
        );

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
        make_snapshot(
            &root,
            GC_ENGINE_TOKENZERO,
            &pid(&root),
            1,
            std::slice::from_ref(&h1),
        );

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
        make_snapshot(&root, GC_ENGINE_TOKENZERO, &pid(&root), 1, &[]);

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
            std::slice::from_ref(&h1),
            future,
        );
        make_snapshot(&root, GC_ENGINE_TOKENZERO, &pid(&root), 1, &[]);

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
            std::slice::from_ref(&h1),
            past,
        );
        make_snapshot(&root, GC_ENGINE_TOKENZERO, &pid(&root), 1, &[]);

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
        // Authoritative empty reachability snapshot (valid current.json with
        // blob_hashes=[]) is required before GC may treat orphans as Collect.
        make_snapshot(&root, GC_ENGINE_TOKENZERO, &pid(&root), 1, &[]);
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
            std::slice::from_ref(&reachable),
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

    #[test]
    fn gc_missing_roots_current_is_uncertain_not_empty() {
        let (_dir, root, cas) = make_store();
        let legacy = cas.publish(b"legacy-live").unwrap();
        // Project roots dir without current.json must not authorize deletion.
        let project_dir = root
            .join("gc")
            .join("roots")
            .join(GC_ENGINE_TOKENZERO)
            .join(pid(&root));
        fs::create_dir_all(&project_dir).unwrap();
        let config = GcConfig {
            apply: true,
            ..GcConfig::default()
        };
        let report = run_gc(&root, &config).unwrap();
        let r = report
            .objects
            .iter()
            .find(|o| o.blob_hash == legacy)
            .unwrap();
        assert_eq!(r.verdict, GcVerdict::RetainUncertain);
        assert!(cas.contains(&legacy));
    }

    #[test]
    fn gc_authoritative_empty_snapshot_collects_orphans() {
        let (_dir, root, cas) = make_store();
        let orphan = cas.publish(b"true-orphan").unwrap();
        make_snapshot(&root, GC_ENGINE_TOKENZERO, &pid(&root), 1, &[]);
        let config = GcConfig {
            apply: true,
            ..GcConfig::default()
        };
        let report = run_gc(&root, &config).unwrap();
        let r = report
            .objects
            .iter()
            .find(|o| o.blob_hash == orphan)
            .unwrap();
        assert_eq!(r.verdict, GcVerdict::Collect);
        assert!(!cas.contains(&orphan));
    }

    #[test]
    fn publish_reachability_rejects_epoch_zero_and_regression() {
        let (_dir, root, _cas) = make_store();
        let project = pid(&root);
        let err = publish_reachability_snapshot(&root, GC_ENGINE_TOKENZERO, &project, 0, &[])
            .unwrap_err();
        assert!(matches!(err, GcError::SchemaViolation(_)));
        publish_reachability_snapshot(&root, GC_ENGINE_TOKENZERO, &project, 2, &[]).unwrap();
        let err = publish_reachability_snapshot(&root, GC_ENGINE_TOKENZERO, &project, 2, &[])
            .unwrap_err();
        assert!(matches!(err, GcError::SchemaViolation(_)));
        let err = publish_reachability_snapshot(&root, GC_ENGINE_TOKENZERO, &project, 1, &[])
            .unwrap_err();
        assert!(matches!(err, GcError::SchemaViolation(_)));
        publish_reachability_snapshot(&root, GC_ENGINE_TOKENZERO, &project, 3, &[]).unwrap();
    }

    #[test]
    fn list_and_remove_refuse_prefix_symlink_escape() {
        let (_dir, root, cas) = make_store();
        let hash = cas.publish(b"inside-cas").unwrap();
        let outside = root.parent().unwrap().join("outside-escape");
        fs::create_dir_all(&outside).unwrap();
        let outside_blob = outside.join(&hash);
        fs::write(&outside_blob, b"escaped").unwrap();

        let prefix = root.join("blobs").join("sha256").join(&hash[..2]);
        // Replace prefix directory with a symlink to outside.
        fs::remove_dir_all(&prefix).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, &prefix).unwrap();
        }
        #[cfg(not(unix))]
        {
            // On non-unix platforms this containment scenario is not exercised.
            return;
        }

        let listed = cas.list_objects().unwrap();
        assert!(
            !listed.contains(&hash),
            "prefix symlink objects must not be listed"
        );
        assert!(matches!(
            cas.remove_object(&hash),
            Err(SharedCasError::Policy)
        ));
        // External object must remain untouched.
        assert!(outside_blob.is_file());
        assert_eq!(fs::read(&outside_blob).unwrap(), b"escaped");
    }

    #[test]
    fn resume_journal_reconciles_republished_object() {
        let (_dir, root, cas) = make_store();
        let orphan = cas.publish(b"republish-me").unwrap();
        make_snapshot(&root, GC_ENGINE_TOKENZERO, &pid(&root), 1, &[]);

        // Simulate a crash journal that claims the hash was deleted.
        let progress_path = root
            .join("gc")
            .join("reports")
            .join("resume-run.progress.json");
        fs::create_dir_all(progress_path.parent().unwrap()).unwrap();
        let stale = SweepProgress {
            schema_version: GC_SCHEMA_VERSION.to_string(),
            record_type: "sweep-progress".to_string(),
            run_id: "resume-run".into(),
            store_root: root.to_string_lossy().into_owned(),
            evaluated_at: rfc3339_now(),
            objects: vec![orphan.clone()],
            deleted: vec![orphan.clone()],
            state: "sweeping".into(),
        };
        fs::write(&progress_path, serde_json::to_vec_pretty(&stale).unwrap()).unwrap();

        // Object is still present (republished / never actually deleted).
        assert!(cas.contains(&orphan));

        let config = GcConfig {
            run_id: "resume-run".into(),
            apply: true,
            ..GcConfig::default()
        };
        let report = run_gc(&root, &config).unwrap();
        // Journal claimed delete but object was live: re-evaluate and truthfully
        // delete under authoritative empty roots. Must not skip recheck.
        assert!(!cas.contains(&orphan));
        let r = report
            .objects
            .iter()
            .find(|o| o.blob_hash == orphan)
            .unwrap();
        assert_eq!(r.verdict, GcVerdict::Collect);
        assert!(r
            .evidence
            .iter()
            .any(|e| e.contains("deleted by this sweep")));
    }

    #[test]
    fn resume_journal_rejects_store_identity_mismatch() {
        let (_dir, root, cas) = make_store();
        let orphan = cas.publish(b"identity").unwrap();
        make_snapshot(&root, GC_ENGINE_TOKENZERO, &pid(&root), 1, &[]);
        let progress_path = root
            .join("gc")
            .join("reports")
            .join("bad-store.progress.json");
        fs::create_dir_all(progress_path.parent().unwrap()).unwrap();
        let stale = SweepProgress {
            schema_version: GC_SCHEMA_VERSION.to_string(),
            record_type: "sweep-progress".to_string(),
            run_id: "bad-store".into(),
            store_root: "/not/this/store".into(),
            evaluated_at: rfc3339_now(),
            objects: vec![orphan.clone()],
            deleted: vec![],
            state: "sweeping".into(),
        };
        fs::write(&progress_path, serde_json::to_vec_pretty(&stale).unwrap()).unwrap();
        let config = GcConfig {
            run_id: "bad-store".into(),
            apply: true,
            ..GcConfig::default()
        };
        let err = run_gc(&root, &config).unwrap_err();
        assert!(matches!(err, GcError::SchemaViolation(_)));
        assert!(cas.contains(&orphan));
    }

    #[test]
    fn parse_rfc3339_applies_offsets_and_rejects_ranges() {
        let z = parse_rfc3339("1970-01-01T00:00:00Z").unwrap();
        assert_eq!(z, UNIX_EPOCH);

        let plus = parse_rfc3339("1970-01-01T01:00:00+01:00").unwrap();
        assert_eq!(plus, UNIX_EPOCH);

        let minus = parse_rfc3339("1969-12-31T23:00:00-01:00").unwrap();
        assert_eq!(minus, UNIX_EPOCH);

        assert!(parse_rfc3339("2024-13-01T00:00:00Z").is_none());
        assert!(parse_rfc3339("2024-02-30T00:00:00Z").is_none());
        assert!(parse_rfc3339("2024-04-31T00:00:00Z").is_none());
        assert!(parse_rfc3339("2024-01-01T24:00:00Z").is_none());
        assert!(parse_rfc3339("2024-01-01T00:60:00Z").is_none());
        assert!(parse_rfc3339("2024-01-01T00:00:00+24:00").is_none());
        assert!(parse_rfc3339("2024-01-01T00:00:00").is_none());
        // Leap day accepted.
        assert!(parse_rfc3339("2024-02-29T12:00:00Z").is_some());
        assert!(parse_rfc3339("2023-02-29T12:00:00Z").is_none());
    }

    #[test]
    fn gc_recheck_under_lock_sees_concurrent_pin() {
        // Single-threaded stand-in for the race: recheck+delete are under the
        // coordinator lock shared with publish_pin_record. Publishing a pin
        // before apply recheck must retain the object.
        let (_dir, root, cas) = make_store();
        let h = cas.publish(b"race-pin").unwrap();
        make_snapshot(&root, GC_ENGINE_TOKENZERO, &pid(&root), 1, &[]);
        make_pin(&root, GC_ENGINE_TOKENZERO, &pid(&root), "late-pin", &h);
        let config = GcConfig {
            apply: true,
            ..GcConfig::default()
        };
        let report = run_gc(&root, &config).unwrap();
        let r = report.objects.iter().find(|o| o.blob_hash == h).unwrap();
        assert_eq!(r.verdict, GcVerdict::Retain);
        assert!(cas.contains(&h));
    }

    #[test]
    fn resume_journal_rejects_bad_schema() {
        let (_dir, root, cas) = make_store();
        let orphan = cas.publish(b"schema-journal").unwrap();
        make_snapshot(&root, GC_ENGINE_TOKENZERO, &pid(&root), 1, &[]);
        let progress_path = root
            .join("gc")
            .join("reports")
            .join("bad-schema.progress.json");
        fs::create_dir_all(progress_path.parent().unwrap()).unwrap();
        let stale = serde_json::json!({
            "schema_version": "zerostack.cas-gc.v0",
            "record_type": "sweep-progress",
            "run_id": "bad-schema",
            "store_root": root.to_string_lossy(),
            "evaluated_at": rfc3339_now(),
            "objects": [orphan],
            "deleted": [],
            "state": "sweeping"
        });
        fs::write(&progress_path, serde_json::to_vec_pretty(&stale).unwrap()).unwrap();
        let config = GcConfig {
            run_id: "bad-schema".into(),
            apply: true,
            ..GcConfig::default()
        };
        let err = run_gc(&root, &config).unwrap_err();
        assert!(matches!(err, GcError::CorruptMetadata { .. }));
        assert!(cas.contains(&orphan));
    }
}
