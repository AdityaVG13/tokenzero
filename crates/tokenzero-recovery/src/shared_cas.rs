//! Canonical shared CAS for ZeroRef v1 blobs at
//! `<root>/blobs/sha256/<first-two-hex>/<full-hash>` (`tz`/`fz`/`gz` full-hash refs).

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

#[derive(Debug, Clone)]
pub struct SharedCas {
    root: PathBuf,
}

impl SharedCas {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn resolve_cache_root(cache_path: &Path) -> Option<PathBuf> {
        let engine_dir = cache_path.parent()?;
        (engine_dir.file_name()? == "tokenzero")
            .then(|| engine_dir.parent().map(Path::to_path_buf))
            .flatten()
    }

    pub fn attach_root_for_cache_path(cache_path: &Path) -> PathBuf {
        Self::resolve_cache_root(cache_path)
            .or_else(|| cache_path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| cache_path.to_path_buf())
    }

    pub fn sibling_engine_cache_path(cache_path: &Path, engine: &str) -> Option<PathBuf> {
        let engine_dir = cache_path.parent()?;
        let name = engine_dir.file_name()?.to_str()?;
        if !GC_ENGINES.contains(&name) {
            return None;
        }
        Some(
            engine_dir
                .parent()?
                .join(engine)
                .join("recovery-cache.json"),
        )
    }

    pub fn detect_from_cache_path(cache_path: &Path) -> Option<Self> {
        let unified_root = Self::resolve_cache_root(cache_path);
        let is_unified = unified_root.is_some();
        let root = unified_root.unwrap_or_else(|| Self::attach_root_for_cache_path(cache_path));
        (is_unified || root.join("blobs").is_dir()).then(|| Self::new(root))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn publish(&self, bytes: &[u8]) -> Result<String, SharedCasError> {
        let full_hash = content_sha256_hex(bytes);
        let path = self.object_path(&full_hash);
        if path.exists() {
            return Self::verify_existing(&path, bytes, &full_hash);
        }
        let parent = path
            .parent()
            .expect("object path always has a parent directory");
        fs::create_dir_all(parent)?;
        let tmp_path = parent.join(format!(".tmp-{}-{}.blob", full_hash, unique_suffix()));
        {
            let mut tmp = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)?;
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

    pub fn resolve(&self, full_hash: &str) -> Result<Vec<u8>, SharedCasError> {
        self.validate_hash(full_hash)?;
        let (meta, bytes) = match read_regular_file(&self.object_path(full_hash)) {
            Ok(v) => v,
            Err(SharedCasError::Io(err)) if err.kind() == io::ErrorKind::NotFound => {
                return Err(SharedCasError::NotFound);
            }
            Err(err) => return Err(err),
        };
        if bytes.len() as u64 != meta.len() || content_sha256_hex(&bytes) != full_hash {
            return Err(SharedCasError::Corruption);
        }
        Ok(bytes)
    }

    pub fn contains(&self, full_hash: &str) -> bool {
        self.validate_hash(full_hash).is_ok() && self.object_path(full_hash).is_file()
    }

    pub(crate) fn is_pinned(&self, full_hash: &str) -> bool {
        if self.validate_hash(full_hash).is_err() {
            return false;
        }
        let mut state = MarkState::default();
        load_all_pins(&self.root, &mut state, SystemTime::now()).is_err()
            || state.uncertain
            || state.live.contains_key(full_hash)
    }

    pub fn list_objects(&self) -> Result<Vec<String>, SharedCasError> {
        let mut objects = Vec::new();
        let base = self.root.join("blobs").join("sha256");
        if !fs::symlink_metadata(&base)
            .map(|m| m.is_dir() && !m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Ok(objects);
        }
        for prefix_entry in fs::read_dir(&base)? {
            let prefix_entry = prefix_entry?;
            if !fs::symlink_metadata(prefix_entry.path())
                .map(|m| m.is_dir() && !m.file_type().is_symlink())
                .unwrap_or(false)
            {
                continue;
            }
            for entry in fs::read_dir(prefix_entry.path())? {
                let entry = entry?;
                let Ok(ft) = entry.file_type() else { continue };
                if ft.is_symlink() || !ft.is_file() {
                    continue;
                }
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if name.starts_with('.') || self.validate_hash(&name).is_err() {
                    continue;
                }
                if self.path_is_contained_object(&entry.path(), &name) {
                    objects.push(name);
                }
            }
        }
        Ok(objects)
    }

    pub fn remove_object(&self, full_hash: &str) -> Result<(), SharedCasError> {
        self.validate_hash(full_hash)?;
        let path = self.object_path(full_hash);
        if !self.object_path_chain_is_safe(full_hash) {
            return Err(SharedCasError::Policy);
        }
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.file_type().is_file() => {
                return Err(SharedCasError::Policy);
            }
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        }
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    pub fn repair_object(&self, full_hash: &str, bytes: &[u8]) -> Result<bool, SharedCasError> {
        self.validate_hash(full_hash)?;
        let expected_hash = content_sha256_hex(bytes);
        if expected_hash != full_hash {
            return Err(SharedCasError::InvalidHash(format!(
                "provided bytes hash to {expected_hash}, expected {full_hash}"
            )));
        }
        let path = self.object_path(full_hash);
        if path.is_file() {
            match self.resolve(full_hash) {
                Ok(_) => return Ok(false),
                Err(SharedCasError::Corruption) => fs::remove_file(&path)?,
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

    fn path_is_contained_object(&self, path: &Path, full_hash: &str) -> bool {
        path == self.object_path(full_hash)
            && self.object_path_chain_is_safe(full_hash)
            && fs::symlink_metadata(path)
                .is_ok_and(|m| !m.file_type().is_symlink() && m.file_type().is_file())
    }

    fn object_path_chain_is_safe(&self, full_hash: &str) -> bool {
        if self.validate_hash(full_hash).is_err() {
            return false;
        }
        let expected = self.object_path(full_hash);
        let Ok(relative) = expected.strip_prefix(&self.root) else {
            return false;
        };
        let mut cur = self.root.clone();
        let check = |path: &Path| -> Option<bool> {
            match fs::symlink_metadata(path) {
                Ok(meta) => Some(meta.file_type().is_symlink()),
                Err(err) if err.kind() == io::ErrorKind::NotFound => None,
                Err(_) => Some(true),
            }
        };
        if check(&cur) != Some(false) {
            return false;
        }
        for component in relative.components() {
            cur.push(component);
            match check(&cur) {
                Some(true) => return false,
                Some(false) => {}
                None => return true,
            }
        }
        true
    }

    fn validate_hash(&self, full_hash: &str) -> Result<(), SharedCasError> {
        is_valid_hash(full_hash)
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
            || content_sha256_hex(&actual) != expected_hash
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

pub const GC_SCHEMA_VERSION: &str = "zerostack.cas-gc.v1";
pub const GC_ENGINE_TOKENZERO: &str = "tokenzero";
const GC_ENGINES: &[&str] = &["tokenzero", "fszero", "graphzero"];
const GC_RECORD_TYPE_REACHABILITY: &str = "reachability-snapshot";
const GC_RECORD_TYPE_PIN: &str = "pin";
const GC_RECORD_TYPE_LEASE: &str = "lease";
const GC_RECORD_TYPE_DRY_RUN: &str = "dry-run-report";
const GC_RECORD_TYPE_SWEEP_PROGRESS: &str = "sweep-progress";
pub const GC_MIN_GRACE_SECONDS: u64 = 60;
pub const DEFAULT_GC_REPORT_LIMIT: usize = 32;

fn require_gc_engine(engine: &str) -> Result<(), GcError> {
    if GC_ENGINES.contains(&engine) {
        Ok(())
    } else {
        Err(GcError::Policy(format!("invalid engine {engine}")))
    }
}

#[derive(Debug, Error)]
pub enum GcError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("schema violation: {0}")]
    SchemaViolation(String),
    #[error("corrupt metadata at {path}: {reason}")]
    CorruptMetadata { path: PathBuf, reason: String },
    #[error("uncertain metadata: {0}")]
    UncertainMetadata(String),
    #[error("policy violation: {0}")]
    Policy(String),
    #[error("fault injected")]
    FaultInjected,
}

impl From<SharedCasError> for GcError {
    fn from(err: SharedCasError) -> Self {
        match err {
            SharedCasError::Io(e) => GcError::Io(e),
            SharedCasError::Corruption => corrupt(Path::new(""), "CAS object corruption".into()),
            SharedCasError::Policy => GcError::Policy("CAS policy violation".into()),
            SharedCasError::InvalidHash(s) => {
                GcError::SchemaViolation(format!("invalid CAS hash {s}"))
            }
            SharedCasError::NotFound => GcError::UncertainMetadata("CAS object not found".into()),
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseOwner {
    pub pid: u64,
    pub host: String,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GcVerdict {
    Retain,
    Collect,
    RetainUncertain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcCandidate {
    pub blob_hash: String,
    pub verdict: GcVerdict,
    pub reason_codes: Vec<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunReport {
    pub schema_version: String,
    pub record_type: String,
    pub run_id: String,
    pub store_root: String,
    pub evaluated_at: String,
    pub objects: Vec<GcCandidate>,
}

#[derive(Debug, Clone)]
pub struct GcConfig {
    pub run_id: String,
    pub grace_seconds: u64,
    pub min_age_seconds: u64,
    pub apply: bool,
    pub now: SystemTime,
    pub fault_after_deletes: Option<usize>,
    /// Maximum completed JSON reports retained in `gc/reports`.
    pub report_limit: usize,
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
            report_limit: DEFAULT_GC_REPORT_LIMIT,
        }
    }
}

pub fn project_id(store_root: &Path) -> Result<String, GcError> {
    let canonical = store_root.canonicalize().map_err(GcError::Io)?;
    Ok(content_sha256_hex(canonical.to_string_lossy().as_bytes()))
}

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

fn gc_join(store_root: &Path, parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(store_root.join("gc"), |p, part| p.join(part))
}

fn gc_record_path(store_root: &Path, subdir: &str, record: &impl GcRecord, id: &str) -> PathBuf {
    let (_, _, engine, project) = record.header();
    gc_join(
        store_root,
        &[subdir, engine, project, &format!("{id}.json")],
    )
}

fn validate_run_id(run_id: &str) -> Result<(), GcError> {
    if is_valid_pin_id(run_id) {
        Ok(())
    } else {
        Err(GcError::SchemaViolation(
            "run_id must be non-empty, <=128 chars, start with alphanumeric, and contain only alphanumeric, '.', '_', or '-'".into(),
        ))
    }
}

fn days_in_month(year: i64, month: u32) -> Option<u32> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => return None,
    })
}

fn civil_to_days(year: i64, month: u32, day: u32) -> i64 {
    let (mut y, mut m) = (year, month as i64);
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day as i64 - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
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
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
}

fn parse_rfc3339(s: &str) -> Option<SystemTime> {
    if s.len() < 20
        || s.as_bytes()[4] != b'-'
        || s.as_bytes()[7] != b'-'
        || s.as_bytes()[10] != b'T'
        || s.as_bytes()[13] != b':'
        || s.as_bytes()[16] != b':'
    {
        return None;
    }
    let year: i64 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    let hour: u32 = s[11..13].parse().ok()?;
    let minute: u32 = s[14..16].parse().ok()?;
    let second: u32 = s[17..19].parse().ok()?;
    if !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 60 || day == 0 {
        return None;
    }
    if day > days_in_month(year, month)? {
        return None;
    }
    let mut rest = &s[19..];
    let nanos = if let Some(frac) = rest.strip_prefix('.') {
        let n = frac.chars().take_while(|c| c.is_ascii_digit()).count();
        if n == 0 {
            return None;
        }
        let take = n.min(9);
        rest = &frac[n..];
        frac[..take].parse::<u64>().ok()? * 10u64.pow(9 - take as u32)
    } else {
        0
    };
    let offset = if rest.eq_ignore_ascii_case("Z") {
        0
    } else if rest.len() == 6
        && (rest.starts_with('+') || rest.starts_with('-'))
        && rest.as_bytes()[3] == b':'
    {
        let sign = if rest.starts_with('+') { 1i64 } else { -1 };
        let oh: i64 = rest[1..3].parse().ok()?;
        let om: i64 = rest[4..6].parse().ok()?;
        if oh > 23 || om > 59 {
            return None;
        }
        sign * (oh * 3600 + om * 60)
    } else {
        return None;
    };
    let local = civil_to_days(year, month, day) * 86400
        + hour as i64 * 3600
        + minute as i64 * 60
        + second as i64;
    let utc = local.checked_sub(offset)?;
    (utc >= 0).then(|| UNIX_EPOCH + std::time::Duration::new(utc as u64, nanos as u32))
}

/// Format `t` as second-precision UTC RFC3339 (`YYYY-MM-DDTHH:MM:SSZ`).
pub(crate) fn format_system_time(t: SystemTime) -> String {
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

/// Lowercase hex encoding of raw bytes (no separators).
pub(crate) fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

/// Full 64-char lowercase SHA-256 hex digest of `bytes`.
pub(crate) fn content_sha256_hex(bytes: &[u8]) -> String {
    lower_hex(Sha256::digest(bytes).as_ref())
}

fn is_valid_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_valid_pin_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

fn validate_namespace(path: &Path, engine: &str, project_id: &str) -> Result<(), GcError> {
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    if components.len() < 4 {
        return Err(corrupt(path, format!("path too short: {}", path.display())));
    }
    let (path_engine, path_project) = (
        components[components.len() - 3],
        components[components.len() - 2],
    );
    if path_engine != engine {
        return Err(corrupt(
            path,
            format!("engine mismatch: path {path_engine}, record {engine}"),
        ));
    }
    if path_project != project_id {
        return Err(corrupt(
            path,
            format!("project_id mismatch: path {path_project}, record {project_id}"),
        ));
    }
    Ok(())
}

fn corrupt(path: &Path, reason: String) -> GcError {
    GcError::CorruptMetadata {
        path: path.to_path_buf(),
        reason,
    }
}

fn require_rfc3339(s: &str, path: &Path, field: &str) -> Result<(), GcError> {
    parse_rfc3339(s)
        .map(|_| ())
        .ok_or_else(|| corrupt(path, format!("invalid {field}")))
}

fn require_hash(s: &str, path: &Path, field: &str) -> Result<(), GcError> {
    is_valid_hash(s)
        .then_some(())
        .ok_or_else(|| corrupt(path, format!("invalid {field} {s}")))
}

fn require_min(value: u64, min: u64, path: &Path, field: &str) -> Result<(), GcError> {
    (value >= min)
        .then_some(())
        .ok_or_else(|| corrupt(path, format!("{field} {value} < {min}")))
}

trait GcRecord {
    fn header(&self) -> (&str, &str, &str, &str);
}

macro_rules! impl_gc_record {
    ($T:ty) => {
        impl GcRecord for $T {
            fn header(&self) -> (&str, &str, &str, &str) {
                (
                    &self.schema_version,
                    &self.record_type,
                    &self.engine,
                    &self.project_id,
                )
            }
        }
    };
}

impl_gc_record!(ReachabilitySnapshot);
impl_gc_record!(PinRecord);
impl_gc_record!(LeaseRecord);

fn read_gc_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, GcError> {
    serde_json::from_str(&fs::read_to_string(path).map_err(GcError::Io)?).map_err(GcError::Json)
}

fn write_gc_json<T: Serialize>(path: &Path, value: &T) -> Result<(), GcError> {
    gc_atomic_write(path, &serde_json::to_vec_pretty(value)?).map_err(GcError::Io)
}

fn validate_record_schema(
    schema_version: &str,
    record_type: &str,
    path: &Path,
    expected_type: &str,
) -> Result<(), GcError> {
    let reason = if schema_version != GC_SCHEMA_VERSION {
        Some(format!("unsupported schema_version {schema_version}"))
    } else if record_type != expected_type {
        Some(format!("record_type {record_type}"))
    } else {
        None
    };
    reason.map_or(Ok(()), |reason| Err(corrupt(path, reason)))
}

fn validate_record_common<R: GcRecord>(
    record: &R,
    path: &Path,
    expected_type: &str,
) -> Result<(), GcError> {
    let (schema_version, record_type, engine, project_id) = record.header();
    validate_record_schema(schema_version, record_type, path, expected_type)?;
    if !GC_ENGINES.contains(&engine) {
        return Err(corrupt(path, format!("invalid engine {engine}")));
    }
    validate_namespace(path, engine, project_id)
}

fn read_reachability_snapshot(path: &Path) -> Result<ReachabilitySnapshot, GcError> {
    let snap: ReachabilitySnapshot = read_gc_json(path)?;
    validate_record_common(&snap, path, GC_RECORD_TYPE_REACHABILITY)?;
    require_min(snap.epoch, 1, path, "epoch")?;
    require_rfc3339(&snap.published_at, path, "published_at")?;
    for h in &snap.blob_hashes {
        require_hash(h, path, "blob hash")?;
    }
    Ok(snap)
}

fn read_pin_record(path: &Path) -> Result<PinRecord, GcError> {
    let pin: PinRecord = read_gc_json(path)?;
    validate_record_common(&pin, path, GC_RECORD_TYPE_PIN)?;
    if !is_valid_pin_id(&pin.pin_id) {
        return Err(corrupt(path, format!("invalid pin_id {}", pin.pin_id)));
    }
    require_rfc3339(&pin.created_at, path, "created_at")?;
    if let Some(exp) = pin.expires_at.as_deref() {
        require_rfc3339(exp, path, "expires_at")?;
    }
    require_hash(&pin.blob_hash, path, "blob_hash")?;
    Ok(pin)
}

fn read_lease_record(path: &Path) -> Result<LeaseRecord, GcError> {
    let lease: LeaseRecord = read_gc_json(path)?;
    validate_record_common(&lease, path, GC_RECORD_TYPE_LEASE)?;
    if !is_valid_pin_id(&lease.operation_id) {
        return Err(corrupt(
            path,
            format!("invalid operation_id {}", lease.operation_id),
        ));
    }
    require_min(lease.epoch, 1, path, "epoch")?;
    require_rfc3339(&lease.started_at, path, "started_at")?;
    require_rfc3339(&lease.expires_at, path, "expires_at")?;
    require_min(
        lease.grace_seconds,
        GC_MIN_GRACE_SECONDS,
        path,
        "grace_seconds",
    )?;
    for h in &lease.blob_hashes {
        require_hash(h, path, "blob hash")?;
    }
    Ok(lease)
}

#[derive(Debug, Default)]
struct MarkState {
    live: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
    uncertain: bool,
    global_evidence: Vec<String>,
}

fn mark_hash(state: &mut MarkState, hash: &str, reason: &str, evidence: &str) {
    let meta = state.live.entry(hash.to_string()).or_default();
    meta.0.insert(reason.to_string());
    meta.1.insert(evidence.to_string());
}

fn mark_uncertain(state: &mut MarkState, evidence: String) {
    state.uncertain = true;
    state.global_evidence.push(evidence);
}

fn walk_gc_projects(
    store_root: &Path,
    subdir: &str,
    mut f: impl FnMut(&Path) -> Result<(), GcError>,
) -> Result<(), GcError> {
    let dir = store_root.join("gc").join(subdir);
    if !dir.is_dir() {
        return Ok(());
    }
    for engine_entry in fs::read_dir(&dir)? {
        let engine_dir = engine_entry?.path();
        if !engine_dir.is_dir() {
            continue;
        }
        for project_entry in fs::read_dir(&engine_dir)? {
            let project_dir = project_entry?.path();
            if project_dir.is_dir() {
                f(&project_dir)?;
            }
        }
    }
    Ok(())
}

fn walk_gc_json(
    store_root: &Path,
    subdir: &str,
    mut f: impl FnMut(&Path) -> Result<(), GcError>,
) -> Result<(), GcError> {
    walk_gc_projects(store_root, subdir, |project_dir| {
        for entry in fs::read_dir(project_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|v| v.to_str()) == Some("json") {
                f(&path)?;
            }
        }
        Ok(())
    })
}

fn walk_gc_records<T>(
    store_root: &Path,
    subdir: &str,
    state: &mut MarkState,
    read: fn(&Path) -> Result<T, GcError>,
    mut visit: impl FnMut(&Path, T, &mut MarkState),
) -> Result<(), GcError> {
    walk_gc_json(store_root, subdir, |path| {
        match read(path) {
            Ok(record) => visit(path, record, state),
            Err(err) => mark_uncertain(state, format!("{}: {err}", path.display())),
        }
        Ok(())
    })
}
fn load_all_pins(store_root: &Path, state: &mut MarkState, now: SystemTime) -> Result<(), GcError> {
    walk_gc_records(
        store_root,
        "pins",
        state,
        read_pin_record,
        |path, pin, state| {
            if pin
                .expires_at
                .as_deref()
                .and_then(parse_rfc3339)
                .is_some_and(|exp| exp <= now)
            {
                mark_uncertain(
                    state,
                    format!(
                        "expired pin {} retained on clock uncertainty",
                        path.display()
                    ),
                );
            }
            mark_hash(
                state,
                &pin.blob_hash,
                "pin",
                &format!("pin {}", path.display()),
            );
        },
    )
}

fn load_mark_state(
    store_root: &Path,
    now: SystemTime,
    grace_seconds: u64,
) -> Result<MarkState, GcError> {
    let mut state = MarkState::default();
    if !store_root.join("gc").join("roots").is_dir() {
        mark_uncertain(
            &mut state,
            "missing gc/roots directory; reachability metadata absent".into(),
        );
    } else {
        let mut saw_any_project = false;
        walk_gc_projects(store_root, "roots", |project_dir| {
            saw_any_project = true;
            let current = project_dir.join("current.json");
            if !current.is_file() {
                mark_uncertain(
                    &mut state,
                    format!("missing reachability snapshot {}", current.display()),
                );
                return Ok(());
            }
            match read_reachability_snapshot(&current) {
                Ok(snap) => {
                    let evidence = format!("root {} epoch {}", current.display(), snap.epoch);
                    for h in &snap.blob_hashes {
                        mark_hash(&mut state, h, "reachability-root", &evidence);
                    }
                }
                Err(err) => mark_uncertain(&mut state, format!("{}: {err}", current.display())),
            }
            Ok(())
        })?;
        if !saw_any_project {
            mark_uncertain(
                &mut state,
                "gc/roots has no project namespaces; reachability metadata absent".into(),
            );
        }
    }
    load_all_pins(store_root, &mut state, now)?;
    walk_gc_records(
        store_root,
        "leases",
        &mut state,
        read_lease_record,
        |path, lease, state| {
            let expires = parse_rfc3339(&lease.expires_at).unwrap_or(now);
            let grace_end =
                expires + std::time::Duration::from_secs(lease.grace_seconds.max(grace_seconds));
            let active = now <= expires;
            let in_grace = !active && now < grace_end;
            let reason = if active {
                "active-lease"
            } else {
                "stale-lease-grace"
            };
            let evidence = if active {
                format!("lease {}", path.display())
            } else if in_grace {
                format!("lease {} inside grace", path.display())
            } else {
                format!("lease {} retained on uncertain liveness", path.display())
            };
            if !active && !in_grace {
                mark_uncertain(
                    state,
                    format!(
                        "lease {} stale outside grace; owner liveness unverified",
                        path.display()
                    ),
                );
            }
            for h in &lease.blob_hashes {
                mark_hash(state, h, reason, &evidence);
            }
        },
    )?;
    Ok(state)
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
    for hash in cas.list_objects()? {
        let (verdict, mut reasons, evidence) = if let Some(meta) = state.live.get(&hash) {
            (
                GcVerdict::Retain,
                meta.0.iter().cloned().collect(),
                meta.1.iter().cloned().collect(),
            )
        } else if state.uncertain {
            (
                GcVerdict::RetainUncertain,
                vec!["uncertain-metadata".into()],
                state.global_evidence.clone(),
            )
        } else {
            let young = fs::metadata(cas.object_path(&hash))
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|m| now.duration_since(m).unwrap_or_default().as_secs() < min_age_seconds)
                .unwrap_or(true);
            if young {
                (
                    GcVerdict::RetainUncertain,
                    vec!["uncertain-metadata".into()],
                    vec![format!("object younger than {min_age_seconds} seconds")],
                )
            } else {
                (
                    GcVerdict::Collect,
                    vec!["no-live-reference".into()],
                    vec!["no reachable root, pin, or lease".into()],
                )
            }
        };
        if reasons.is_empty() {
            reasons.push("uncertain-metadata".into());
        }
        objects.push(GcCandidate {
            blob_hash: hash,
            verdict,
            reason_codes: reasons,
            evidence,
        });
    }
    objects.sort_by(|a, b| a.blob_hash.cmp(&b.blob_hash));
    Ok(DryRunReport {
        schema_version: GC_SCHEMA_VERSION.to_string(),
        record_type: GC_RECORD_TYPE_DRY_RUN.to_string(),
        run_id: run_id.to_string(),
        store_root: store_root.to_string_lossy().into_owned(),
        evaluated_at: format_system_time(SystemTime::now()),
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
    let progress: SweepProgress = read_gc_json(path)?;
    validate_record_schema(
        &progress.schema_version,
        &progress.record_type,
        path,
        GC_RECORD_TYPE_SWEEP_PROGRESS,
    )?;
    if progress.run_id.is_empty() {
        return Err(GcError::SchemaViolation("run_id empty".into()));
    }
    if progress.store_root.is_empty() {
        return Err(GcError::SchemaViolation("store_root empty".into()));
    }
    for h in progress.objects.iter().chain(progress.deleted.iter()) {
        require_hash(h, path, "blob hash")?;
    }
    Ok(progress)
}

struct GcCoordLock {
    file: File,
}

impl GcCoordLock {
    fn acquire(store_root: &Path) -> Result<Self, GcError> {
        let path = store_root.join("gc").join("coordinator.lock");
        fs::create_dir_all(path.parent().unwrap_or(store_root))?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(GcError::Io)?;
        FileExt::lock(&file).map_err(GcError::Io)?;
        Ok(Self { file })
    }
}

impl Drop for GcCoordLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn prune_gc_reports(store_root: &Path, keep: usize, current: &Path) -> Result<(), GcError> {
    let keep = keep.max(1);
    let reports_dir = store_root.join("gc").join("reports");
    let mut reports = Vec::new();
    for entry in fs::read_dir(&reports_dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if entry.file_type()?.is_file()
            && name.ends_with(".json")
            && !name.ends_with(".progress.json")
        {
            let modified = entry.metadata()?.modified().unwrap_or(UNIX_EPOCH);
            reports.push((modified, name.to_owned(), path));
        }
    }
    reports.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    while reports.len() > keep {
        let index = reports
            .iter()
            .position(|(_, _, path)| path != current)
            .unwrap_or(0);
        let (_, _, path) = reports.remove(index);
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn run_gc(store_root: &Path, config: &GcConfig) -> Result<DryRunReport, GcError> {
    validate_run_id(&config.run_id)?;
    let _coord = GcCoordLock::acquire(store_root)?;
    let cas = SharedCas::new(store_root.to_path_buf());
    let store_root_key = store_root.to_string_lossy().into_owned();
    let progress_path = gc_join(
        store_root,
        &["reports", &format!("{}.progress.json", config.run_id)],
    );
    let prior_progress = if progress_path.is_file() {
        let progress = read_sweep_progress(&progress_path)?;
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

    let state = load_mark_state(store_root, config.now, config.grace_seconds)?;
    let report = build_dry_run_report(
        store_root,
        &config.run_id,
        &cas,
        &state,
        config.min_age_seconds,
        config.now,
    )?;
    let report_path = gc_join(store_root, &["reports", &format!("{}.json", config.run_id)]);
    write_gc_json(&report_path, &report)?;
    prune_gc_reports(store_root, config.report_limit, &report_path)?;
    if !config.apply {
        return Ok(report);
    }

    let mut deleted: Vec<String> = prior_progress
        .as_ref()
        .map(|p| {
            p.deleted
                .iter()
                .filter(|h| !cas.contains(h))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let to_delete: Vec<String> = report
        .objects
        .iter()
        .filter(|o| o.verdict == GcVerdict::Collect)
        .map(|o| o.blob_hash.clone())
        .collect();
    let persist = |deleted: &[String]| -> Result<(), GcError> {
        write_gc_json(
            &progress_path,
            &SweepProgress {
                schema_version: GC_SCHEMA_VERSION.to_string(),
                record_type: GC_RECORD_TYPE_SWEEP_PROGRESS.to_string(),
                run_id: config.run_id.clone(),
                store_root: store_root_key.clone(),
                evaluated_at: report.evaluated_at.clone(),
                objects: to_delete.clone(),
                deleted: deleted.to_vec(),
                state: "sweeping".into(),
            },
        )
    };
    persist(&deleted)?;

    for hash in &to_delete {
        if deleted.contains(hash) {
            continue;
        }
        let re_state = load_mark_state(store_root, config.now, config.grace_seconds)?;
        if re_state.live.contains_key(hash) || re_state.uncertain {
            continue;
        }
        cas.remove_object(hash)?;
        deleted.push(hash.clone());
        persist(&deleted)?;
        if config.fault_after_deletes == Some(deleted.len()) {
            return Err(GcError::FaultInjected);
        }
    }

    let deleted_set: BTreeSet<_> = deleted.iter().cloned().collect();
    let mut final_report = report.clone();
    for obj in &mut final_report.objects {
        if obj.verdict != GcVerdict::Collect {
            continue;
        }
        if deleted_set.contains(&obj.blob_hash) {
            obj.evidence.push("deleted by this sweep".into());
            continue;
        }
        obj.verdict = GcVerdict::RetainUncertain;
        obj.reason_codes = vec!["uncertain-metadata".into()];
        obj.evidence = vec!["re-check before delete showed a live reference or uncertainty".into()];
    }
    write_gc_json(&report_path, &final_report)?;
    prune_gc_reports(store_root, config.report_limit, &report_path)?;
    let _ = fs::remove_file(&progress_path);
    Ok(final_report)
}

const DRY_RUN_FIELDS: &[&str] = &[
    "schema_version",
    "record_type",
    "run_id",
    "store_root",
    "evaluated_at",
    "objects",
];
const CANDIDATE_FIELDS: &[&str] = &["blob_hash", "verdict", "reason_codes", "evidence"];
const REASON_CODES: &[&str] = &[
    "reachability-root",
    "pin",
    "active-lease",
    "stale-lease-grace",
    "shared-root",
    "unknown-version",
    "corrupt-metadata",
    "uncertain-metadata",
    "unpublished-temp",
    "namespace-isolation",
    "no-live-reference",
];

fn require_str<'a>(value: &'a serde_json::Value, field: &str) -> Result<&'a str, GcError> {
    value
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| GcError::SchemaViolation(field.into()))
}

fn exact_keys(value: &serde_json::Value, fields: &[&str], err: &str) -> Result<(), GcError> {
    let obj = value
        .as_object()
        .ok_or_else(|| GcError::SchemaViolation(err.into()))?;
    let keys: BTreeSet<_> = obj.keys().cloned().collect();
    let expected: BTreeSet<_> = fields.iter().map(|s| (*s).to_string()).collect();
    for field in fields {
        if !keys.contains(*field) {
            return Err(GcError::SchemaViolation(format!("missing {field}")));
        }
    }
    if keys != expected {
        return Err(GcError::SchemaViolation(format!(
            "{err}: {:?}",
            keys.difference(&expected)
        )));
    }
    Ok(())
}

fn validate_list(
    value: &serde_json::Value,
    field: &str,
    allow: Option<&[&str]>,
) -> Result<(), GcError> {
    let items = value
        .get(field)
        .and_then(|v| v.as_array())
        .ok_or_else(|| GcError::SchemaViolation(field.into()))?;
    if field == "reason_codes" && items.is_empty() {
        return Err(GcError::SchemaViolation("reason_codes empty".into()));
    }
    let reasons = field == "reason_codes";
    let mut seen = BTreeSet::new();
    for item in items {
        let s = item.as_str().ok_or_else(|| {
            GcError::SchemaViolation(if reasons { "reason_code" } else { "evidence" }.into())
        })?;
        if !reasons && s.is_empty() {
            return Err(GcError::SchemaViolation("empty evidence".into()));
        }
        if allow.is_some_and(|a| !a.contains(&s)) {
            return Err(GcError::SchemaViolation(format!("reason_code {s}")));
        }
        if !seen.insert(s) {
            return Err(GcError::SchemaViolation(
                if reasons {
                    "duplicate reason_code"
                } else {
                    "duplicate evidence"
                }
                .into(),
            ));
        }
    }
    Ok(())
}

pub fn validate_dry_run_report(value: &serde_json::Value) -> Result<(), GcError> {
    exact_keys(value, DRY_RUN_FIELDS, "extra top-level keys")?;
    if value.get("schema_version").and_then(|v| v.as_str()) != Some(GC_SCHEMA_VERSION) {
        return Err(GcError::SchemaViolation("schema_version".into()));
    }
    if value.get("record_type").and_then(|v| v.as_str()) != Some(GC_RECORD_TYPE_DRY_RUN) {
        return Err(GcError::SchemaViolation("record_type".into()));
    }
    validate_run_id(require_str(value, "run_id")?)?;
    if require_str(value, "store_root")?.is_empty() {
        return Err(GcError::SchemaViolation("store_root empty".into()));
    }
    if parse_rfc3339(require_str(value, "evaluated_at")?).is_none() {
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
        exact_keys(obj, CANDIDATE_FIELDS, "extra object keys")?;
        if !is_valid_hash(require_str(obj, "blob_hash")?) {
            return Err(GcError::SchemaViolation("blob_hash".into()));
        }
        if !matches!(
            require_str(obj, "verdict")?,
            "retain" | "collect" | "retain-uncertain"
        ) {
            return Err(GcError::SchemaViolation("verdict".into()));
        }
        validate_list(obj, "reason_codes", Some(REASON_CODES))?;
        validate_list(obj, "evidence", None)?;
    }
    Ok(())
}

pub fn publish_reachability_snapshot(
    store_root: &Path,
    engine: &str,
    project_id: &str,
    epoch: u64,
    blob_hashes: &[String],
) -> Result<PathBuf, GcError> {
    require_gc_engine(engine)?;
    if !is_valid_hash(project_id) {
        return Err(GcError::SchemaViolation("project_id".into()));
    }
    if epoch == 0 {
        return Err(GcError::SchemaViolation("epoch must be >= 1".into()));
    }
    if let Some(h) = blob_hashes.iter().find(|h| !is_valid_hash(h)) {
        return Err(GcError::Policy(format!("invalid hash {h}")));
    }
    let _coord = GcCoordLock::acquire(store_root)?;
    let path = gc_join(store_root, &["roots", engine, project_id, "current.json"]);
    if path.is_file() {
        if let Ok(existing) = read_reachability_snapshot(&path) {
            if epoch <= existing.epoch {
                return Err(GcError::SchemaViolation(format!(
                    "epoch {epoch} must be strictly greater than current {}",
                    existing.epoch
                )));
            }
        }
    }
    let mut hashes = blob_hashes.to_vec();
    hashes.sort_unstable();
    hashes.dedup();
    write_gc_json(
        &path,
        &ReachabilitySnapshot {
            schema_version: GC_SCHEMA_VERSION.to_string(),
            record_type: GC_RECORD_TYPE_REACHABILITY.to_string(),
            engine: engine.to_string(),
            project_id: project_id.to_string(),
            epoch,
            published_at: format_system_time(SystemTime::now()),
            blob_hashes: hashes,
        },
    )?;
    Ok(path)
}

fn require_schema_field(valid: bool, field: &str) -> Result<(), GcError> {
    valid
        .then_some(())
        .ok_or_else(|| GcError::SchemaViolation(field.into()))
}

fn require_schema(schema_version: &str, record_type: &str, expected: &str) -> Result<(), GcError> {
    require_schema_field(schema_version == GC_SCHEMA_VERSION, "schema_version")?;
    require_schema_field(record_type == expected, "record_type")
}

pub fn publish_pin_record(store_root: &Path, pin: &PinRecord) -> Result<PathBuf, GcError> {
    require_schema(&pin.schema_version, &pin.record_type, GC_RECORD_TYPE_PIN)?;
    require_gc_engine(&pin.engine)?;
    require_schema_field(is_valid_hash(&pin.project_id), "project_id")?;
    require_schema_field(is_valid_pin_id(&pin.pin_id), "pin_id")?;
    require_schema_field(is_valid_hash(&pin.blob_hash), "blob_hash")?;
    let path = gc_record_path(store_root, "pins", pin, &pin.pin_id);
    let _coord = GcCoordLock::acquire(store_root)?;
    write_gc_json(&path, pin)?;
    Ok(path)
}

pub fn publish_lease_record(store_root: &Path, lease: &LeaseRecord) -> Result<PathBuf, GcError> {
    require_schema(
        &lease.schema_version,
        &lease.record_type,
        GC_RECORD_TYPE_LEASE,
    )?;
    require_gc_engine(&lease.engine)?;
    require_schema_field(is_valid_hash(&lease.project_id), "project_id")?;
    require_schema_field(is_valid_pin_id(&lease.operation_id), "operation_id")?;
    if lease.grace_seconds < GC_MIN_GRACE_SECONDS {
        return Err(GcError::SchemaViolation(format!(
            "grace_seconds < {}",
            GC_MIN_GRACE_SECONDS
        )));
    }
    for (field, stamp) in [
        ("expires_at", &lease.expires_at),
        ("started_at", &lease.started_at),
    ] {
        if parse_rfc3339(stamp).is_none() {
            return Err(GcError::SchemaViolation((*field).into()));
        }
    }
    require_schema_field(
        lease.blob_hashes.iter().all(|h| is_valid_hash(h)),
        "blob_hash",
    )?;
    let path = gc_record_path(store_root, "leases", lease, &lease.operation_id);
    let _coord = GcCoordLock::acquire(store_root)?;
    write_gc_json(&path, lease)?;
    Ok(path)
}

/// Process-unique suffix for temp object names (`{nanos}-{counter}`).
pub(crate) fn unique_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{ts}-{n}")
}
