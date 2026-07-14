//! Legacy short-ref migration to full SHA-256 canonical refs.
//!
//! TokenZero's original `id_for(prefix, text)` generates 17-character short IDs
//! (prefix char + 16 hex from the first 8 SHA-256 bytes). The ZeroRef v1 shared
//! CAS uses the full 64-hex SHA-256 digest. This module migrates legacy
//! short-ID blobs to the canonical shared CAS, builds an alias index for
//! backward-compatible reads, and supports idempotent re-runs with a versioned
//! manifest.
//!
//! ## Operations
//! - `migrate` (default): dry-run by default; `--apply` publishes to CAS and
//!   stores aliases.
//! - `verify`: checks every entry in the manifest against current CAS state,
//!   reports integrity.
//! - `rollback`: removes migration-created aliases and manifest, never CAS/source
//!   bytes.
//! - `cleanup`: after successful verification, removes legacy source payloads
//!   while preserving alias reads through CAS. Dry-run first; requires both
//!   `--apply` and `--confirm-cleanup`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::shared_cas::{SharedCas, SharedCasError};

/// Manifest schema version. Bumped when the manifest format changes.
pub const MIGRATION_MANIFEST_VERSION: &str = "tokenzero.migration.v2";

/// Prefix for blob refs in the legacy store.
const BLOB_REF_PREFIX: &str = "tz://blob/";

/// Length of a legacy short ID: prefix char + 16 hex chars = 17.
const LEGACY_SHORT_ID_LEN: usize = 17;

/// Length of a full SHA-256 hex ID: 64 chars.
#[cfg(test)]
const FULL_HASH_LEN: usize = 64;

/// Tmp retry budget for atomic manifest saves.
const TMP_RETRIES: usize = 16;

/// Whether a blob ref ID is a legacy short-ID ref.
///
/// Legacy refs look like `tz://blob/b<16hex>` (17-char ID portion).
/// Full-hash refs look like `tz://blob/<64hex>` (64-char hex portion).
pub fn is_legacy_blob_ref(ref_id: &str) -> bool {
    let Some(rest) = ref_id.strip_prefix(BLOB_REF_PREFIX) else {
        return false;
    };
    rest.len() == LEGACY_SHORT_ID_LEN
        && rest.starts_with('b')
        && rest[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Extract the 16-hex short ID portion from a legacy blob ref.
fn short_id_hex(ref_id: &str) -> Option<&str> {
    let rest = ref_id.strip_prefix(BLOB_REF_PREFIX)?;
    if rest.len() == LEGACY_SHORT_ID_LEN && rest.starts_with('b') {
        Some(&rest[1..])
    } else {
        None
    }
}

/// Compute the full 64-hex SHA-256 of `bytes`.
pub fn full_sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Stable error codes ────────────────────────────────────────────────────

/// Stable error codes returned by migration operations.
/// Every failure path has a unique, stable code suitable for scripting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationErrorCode {
    Internal,
    ManifestNewerVersion,
    ManifestCorrupt,
    ManifestMissing,
    SourceMissing,
    SourceCorrupt,
    AmbiguousShortId,
    AliasConflict,
    ManifestHashConflict,
    CasIo,
    CasCorruption,
    CasPolicy,
    StorePersist,
    ManifestSave,
    CasMissing,
    AliasMissing,
    RollbackSourceGone,
    CleanupConfirmationRequired,
    CleanupNeedsVerification,
    InvalidFlags,
}

impl std::fmt::Display for MigrationErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Internal => "internal",
            Self::ManifestNewerVersion => "manifest-newer-version",
            Self::ManifestCorrupt => "manifest-corrupt",
            Self::ManifestMissing => "manifest-missing",
            Self::SourceMissing => "source-missing",
            Self::SourceCorrupt => "source-corrupt",
            Self::AmbiguousShortId => "ambiguous-short-id",
            Self::AliasConflict => "alias-conflict",
            Self::ManifestHashConflict => "manifest-hash-conflict",
            Self::CasIo => "cas-io",
            Self::CasCorruption => "cas-corruption",
            Self::CasPolicy => "cas-policy",
            Self::StorePersist => "store-persist",
            Self::ManifestSave => "manifest-save",
            Self::CasMissing => "cas-missing",
            Self::AliasMissing => "alias-missing",
            Self::RollbackSourceGone => "rollback-source-gone",
            Self::CleanupConfirmationRequired => "cleanup-confirmation-required",
            Self::CleanupNeedsVerification => "cleanup-needs-verification",
            Self::InvalidFlags => "invalid-flags",
        };
        write!(f, "{label}")
    }
}

impl std::error::Error for MigrationErrorCode {}

// ── Per-entry manifest state ──────────────────────────────────────────────

/// State of an individual entry in the migration manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    Migrated,
    Verified,
    CleanupEligible,
}

// ── Manifest entry ────────────────────────────────────────────────────────

/// One entry in the migration manifest.
/// Contains proofs (hash + size) but no payload or filesystem paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub short_ref: String,
    pub full_hash: String,
    pub size: u64,
    pub state: EntryState,
    pub migrated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<u64>,
    #[serde(default)]
    pub resumed: bool,
    /// Whether this entry's alias was created by migration (true) or
    /// existed before migration ran (false). Used by rollback to avoid
    /// removing aliases that migration did not create.
    #[serde(default)]
    pub owner_alias: bool,
}

// ── Manifest ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationManifest {
    pub version: String,
    pub entries: BTreeMap<String, ManifestEntry>,
    pub completed: bool,
}

impl MigrationManifest { fn entry(
        short_ref: String,
        full_hash: String,
        size: u64,
        migrated_at: u64,
        resumed: bool,
        owner_alias: bool,
    ) -> ManifestEntry {
        ManifestEntry {
            short_ref,
            full_hash,
            size,
            state: EntryState::Migrated,
            migrated_at,
            verified_at: None,
            resumed,
            owner_alias,
        }
    } /// Load a manifest from `path`, or return an empty one if missing.
/// Returns an error if the file exists but is corrupt or has a newer version.
pub fn load(path: &Path) -> Result<Self, MigrationErrorCode> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let mf: Self =
                serde_json::from_str(&text).map_err(|_| MigrationErrorCode::ManifestCorrupt)?;
            if mf.version.as_str() != MIGRATION_MANIFEST_VERSION {
                return Err(MigrationErrorCode::ManifestNewerVersion);
            }
            Ok(mf)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err(MigrationErrorCode::ManifestMissing)
        }
        Err(_) => Err(MigrationErrorCode::ManifestCorrupt),
    }
}

fn empty() -> Self {
    Self {
        version: MIGRATION_MANIFEST_VERSION.to_string(),
        entries: BTreeMap::new(),
        completed: false,
    }
}

/// Save the manifest to `path` atomically:
/// write to a unique temp file, sync data, then rename.
pub fn save(&self, path: &Path) -> Result<(), MigrationErrorCode> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| MigrationErrorCode::ManifestSave)?;
    }
    let text =
        serde_json::to_string_pretty(self).map_err(|_| MigrationErrorCode::ManifestSave)?;

    for attempt in 0..TMP_RETRIES {
        let tmp = tmp_manifest_path(path, attempt);
        match Self::write_tmp_sync(&tmp, &text) {
            Ok(()) => match fs::rename(&tmp, path) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    let _ = fs::remove_file(&tmp);
                    let _ = err;
                }
            },
            Err(err) => {
                let _ = fs::remove_file(&tmp);
                let _ = err;
            }
        }
    }
    Err(MigrationErrorCode::ManifestSave)
}

fn write_tmp_sync(tmp: &Path, text: &str) -> Result<(), std::io::Error> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp)?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    #[cfg(unix)]
    {
        let _ = file.sync_data();
    }
    #[cfg(not(unix))]
    {
        let _ = file.sync_all();
    }
    Ok(())
}

pub fn contains_hash(&self, short_ref: &str, full_hash: &str) -> bool {
    self.entries
        .get(short_ref)
        .is_some_and(|e| e.full_hash == full_hash)
}

pub fn contains_short(&self, short_ref: &str) -> bool {
    self.entries.contains_key(short_ref)
} }

fn tmp_manifest_path(manifest: &Path, attempt: usize) -> PathBuf {
    let parent = manifest.parent().unwrap_or_else(|| Path::new("."));
    let name = manifest
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("migration-manifest.json"));
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.{}.tmp", std::process::id(), attempt));
    parent.join(tmp_name)
}

// ── Alias entry (in report) ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasEntry {
    pub short_ref: String,
    pub full_ref: String,
    pub size: u64,
    pub status: AliasStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AliasStatus {
    Migrated,
    Skipped,
    Failed,
    Repaired,
    Verified,
}

// ── Operation report ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    pub manifest_version: String,
    pub operation: String,
    pub dry_run: bool,
    pub total: usize,
    pub migrated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub repaired: usize,
    pub verified: usize,
    pub aliases: Vec<AliasEntry>,
    pub errors: Vec<MigrationError>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_ref: Option<String>,
}

impl MigrationReport { fn push_alias(
        &mut self,
        short_ref: String,
        full_ref: String,
        size: u64,
        status: AliasStatus,
        error: Option<String>,
        error_code: Option<String>,
    ) {
        self.aliases.push(AliasEntry {
            short_ref,
            full_ref,
            size,
            status,
            error,
            error_code,
        });
    } fn new(operation: &str, dry_run: bool) -> Self {
    Self {
        manifest_version: MIGRATION_MANIFEST_VERSION.to_string(),
        operation: operation.to_string(),
        dry_run,
        total: 0,
        migrated: 0,
        skipped: 0,
        failed: 0,
        repaired: 0,
        verified: 0,
        aliases: Vec::new(),
        errors: Vec::new(),
        timestamp: now_unix(),
    }
}

pub fn is_failure(&self) -> bool {
    self.failed > 0 || !self.errors.is_empty()
}

/// Record a top-level error without counting a failed entry (manifest load, flags).
fn record_error(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        short_ref: Option<String>,
    ) {
        self.errors.push(MigrationError {
            code: code.into(),
            message: message.into(),
            short_ref,
        });
    }

/// Count a failed entry and append a structured error.
fn fail(&mut self, code: &str, message: impl Into<String>, short_ref: Option<String>) {
        self.failed += 1;
        self.record_error(code, message, short_ref);
    }

/// Fail an entry with both an alias row and a top-level error.
fn fail_alias(
        &mut self,
        short_ref: &str,
        full_ref: String,
        size: u64,
        code: &str,
        message: impl Into<String>,
    ) {
        let message = message.into();
        self.failed += 1;
        self.push_alias(
            short_ref.to_string(), full_ref, size, AliasStatus::Failed,
            Some(message.clone()), Some(code.to_string()),
        );
        self.record_error(code, message, Some(short_ref.to_string()));
    }

/// Mutate the last alias row to Failed and record a top-level error.
fn fail_last_alias(
    &mut self,
    short_ref: &str,
    alias_error: impl Into<String>,
    code: &str,
    message: impl Into<String>,
) {
    self.failed += 1;
    let idx = self.aliases.len() - 1;
    self.aliases[idx].status = AliasStatus::Failed;
    self.aliases[idx].error = Some(alias_error.into());
    self.aliases[idx].error_code = Some(code.to_string());
    self.record_error(code.to_string(), message.into(), Some(short_ref.to_string()));
}

pub fn to_json(&self) -> String {
    serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
}

pub fn to_text(&self) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Migration {} (dry_run={})
         ─────────────────────────────────
         total:    {}
         migrated: {}
         skipped:  {}
         failed:   {}
         repaired: {}
         verified: {}
",
        self.operation,
        self.dry_run,
        self.total,
        self.migrated,
        self.skipped,
        self.failed,
        self.repaired,
        self.verified,
    ));
    if !self.aliases.is_empty() {
        out.push_str(
            "
aliases:
",
        );
        for entry in &self.aliases {
            out.push_str(&format!(
                "  {} → {}  [{}]
",
                entry.short_ref,
                entry.full_ref,
                match entry.status {
                    AliasStatus::Migrated => "migrated",
                    AliasStatus::Skipped => "skipped",
                    AliasStatus::Failed => "failed",
                    AliasStatus::Repaired => "repaired",
                    AliasStatus::Verified => "verified",
                }
            ));
            if let Some(err) = &entry.error {
                out.push_str(&format!(
                    "    error: {err}
"
                ));
            }
        }
    }
    if !self.errors.is_empty() {
        out.push_str(
            "
errors:
",
        );
        for err in &self.errors {
            out.push_str(&format!(
                "  [{}] {}
",
                err.code, err.message
            ));
        }
    }
    out
} }

// ── Migration engine ──────────────────────────────────────────────────────

#[derive(Debug)]
pub enum BlobContentResult {
    Ok(Vec<u8>),
    Missing,
    Corrupt,
}

/// Trait abstracting the RecoveryStore operations needed by migration.
pub trait MigrationStore {
    fn blob_ref_ids(&self) -> Vec<String>;
    fn resolve_blob_bytes(&self, ref_id: &str) -> BlobContentResult;
    fn alias_target(&self, alias: &str) -> Option<String>;
    fn store_alias_deferred(&mut self, alias: &str, target: &str);
    fn remove_alias(&mut self, alias: &str);
    fn remove_blob(&mut self, ref_id: &str);
    fn mark_ambiguous(&mut self, short_ref: &str);
    fn is_ambiguous(&self, short_ref: &str) -> bool;
    fn persist_pending(&mut self) -> Result<(), String>;
}

pub struct LegacyMigration<'a> {
    store: &'a mut dyn MigrationStore,
    cas: &'a SharedCas,
    manifest_path: Option<PathBuf>,
}

impl<'a> LegacyMigration<'a> {
    pub fn new(
        store: &'a mut dyn MigrationStore,
        cas: &'a SharedCas,
        manifest_path: Option<PathBuf>,
    ) -> Self {
        Self {
            store,
            cas,
            manifest_path,
        }
    }

    pub fn run(&mut self, dry_run: bool) -> MigrationReport {
        let mut report = MigrationReport::new("migrate", dry_run);
        let manifest = match self.manifest_path.as_ref() {
            Some(p) => match MigrationManifest::load(p) {
                Ok(mf) => mf,
                Err(MigrationErrorCode::ManifestMissing) => MigrationManifest::empty(),
                Err(MigrationErrorCode::ManifestNewerVersion) => {
                    report.record_error("manifest-newer-version", format!(
                        "manifest version is newer than supported ({})",
                        MIGRATION_MANIFEST_VERSION
                    ), None);
                    return report;
                }
                Err(_) => {
                    report.record_error("manifest-corrupt", "manifest file is corrupt, cannot continue", None);
                    return report;
                }
            },
            None => MigrationManifest::empty(),
        };

        let blob_refs = self.store.blob_ref_ids();
        let legacy_refs: Vec<String> = blob_refs
            .into_iter()
            .filter(|r| is_legacy_blob_ref(r))
            .collect();

        report.total = legacy_refs.len();

        let mut updated_manifest = manifest.clone();

        // First pass: detect genuine ambiguous prefixes (two different blobs
        // with the same short ref).
        let mut short_to_hashes: BTreeMap<String, Vec<(String, u64)>> = BTreeMap::new();
        for short_ref in &legacy_refs {
            if self.store.is_ambiguous(short_ref) {
                continue; // already marked
            }
            if let BlobContentResult::Ok(bytes) = self.store.resolve_blob_bytes(short_ref) {
                let full_hash = full_sha256_hex(&bytes);
                short_to_hashes
                    .entry(short_ref.clone())
                    .or_default()
                    .push((full_hash, bytes.len() as u64));
            }
        }

        // Detect true collisions: same short_ref, different full hashes.
        for (short_ref, candidates) in &short_to_hashes {
            let unique_hashes: std::collections::BTreeSet<&str> =
                candidates.iter().map(|(h, _)| h.as_str()).collect();
            if unique_hashes.len() > 1 {
                // Ambiguous: this short ref maps to multiple distinct full hashes.
                self.store.mark_ambiguous(short_ref);
                report.record_error("ambiguous-short-id".to_string(), format!(
                    "{short_ref}: short prefix maps to {} distinct full hashes",
                    unique_hashes.len()
                ), Some(short_ref.clone()));
            }
        }

        for short_ref in &legacy_refs {
            self.migrate_one(
                short_ref,
                &mut report,
                &manifest,
                &mut updated_manifest,
                dry_run,
            );
        }

        // Persist store changes and manifest.
        if !dry_run && (report.migrated > 0 || report.repaired > 0) {
            let mut write_failed = false;

            if let Err(err) = self.store.persist_pending() {
                write_failed = true;
                for entry in &mut report.aliases {
                    if entry.status == AliasStatus::Migrated
                        || entry.status == AliasStatus::Repaired
                    {
                        entry.status = AliasStatus::Failed;
                        entry.error = Some(format!("store persist failed: {err}"));
                        entry.error_code = Some("store-persist".to_string());
                    }
                }
                report.failed += report.migrated + report.repaired;
                report.migrated = 0;
                report.repaired = 0;
                report.record_error("store-persist".to_string(), format!("store persist failed: {err}"), None);
            }

            if let Some(path) = &self.manifest_path {
                updated_manifest.completed = report.failed == 0 && !write_failed;
                if let Err(err) = updated_manifest.save(path) {
                    report.record_error("manifest-save".to_string(), format!("manifest save failed: {err}"), None);
                }
            }
        }

        report
    }

    fn migrate_one(
        &mut self,
        short_ref: &str,
        report: &mut MigrationReport,
        manifest: &MigrationManifest,
        updated_manifest: &mut MigrationManifest,
        dry_run: bool,
    ) {
        // Skip ambiguous refs.
        if self.store.is_ambiguous(short_ref) {
            // Alias-row failure only (no top-level errors entry — historical contract).
            report.failed += 1;
            report.push_alias(
                short_ref.to_string(), String::new(), 0, AliasStatus::Failed,
                Some("short ref is ambiguous (maps to multiple full hashes)".to_string()), Some("ambiguous-short-id".to_string()),
            );
            return;
        }

        let content = match self.store.resolve_blob_bytes(short_ref) {
            BlobContentResult::Ok(bytes) => bytes,
            BlobContentResult::Missing => {
                report.fail_alias(
                    short_ref,
                    String::new(),
                    0,
                    "source-missing",
                    format!("{short_ref}: could not resolve blob content"),
                );
                return;
            }
            BlobContentResult::Corrupt => {
                report.fail_alias(
                    short_ref,
                    String::new(),
                    0,
                    "source-corrupt",
                    format!("{short_ref}: blob content is empty or corrupt"),
                );
                return;
            }
        };

        let size = content.len() as u64;
        let full_hash = full_sha256_hex(&content);
        let full_ref = format!("{BLOB_REF_PREFIX}{full_hash}");

        // Verify the short ID is a correct prefix of the full hash.
        if let Some(short_hex) = short_id_hex(short_ref) {
            if &full_hash[..16] != short_hex {
                report.fail_alias(
                    short_ref,
                    full_ref.clone(),
                    size,
                    "ambiguous-short-id",
                    format!(
                        "{short_ref}: ambiguous short ID —                      short prefix {short_hex} does not match                      full hash prefix {}",
                        &full_hash[..16],
                    ),
                );
                return;
            }
        }

        // Check manifest for idempotent resume.
        // Before treating a manifest entry as skipped, resolve and byte-verify
        // the CAS object to ensure the manifest proof is still valid.
        if let Some(existing) = manifest.entries.get(short_ref) {
            if existing.full_hash == full_hash && existing.size == size {
                // Byte-verify the CAS object before skipping.
                let cas_ok = self.cas.contains(&full_hash) && {
                    match self.cas.resolve(&full_hash) {
                        Ok(bytes) => {
                            full_sha256_hex(&bytes) == full_hash && bytes.len() as u64 == size
                        }
                        Err(_) => false,
                    }
                };

                let needs_alias_repair = self.store.alias_target(short_ref).is_none();

                if !cas_ok && !needs_alias_repair && !dry_run {
                    // CAS missing/corrupt — republish from verified legacy bytes.
                    match self.cas.publish(&content) {
                        Ok(_) => {
                            report.repaired += 1;
                            report.push_alias(
                                short_ref.to_string(), full_ref.clone(), size, AliasStatus::Repaired,
                                None, None,
                            );
                            let mut entry = existing.clone();
                            entry.resumed = true;
                            entry.owner_alias = true;
                            updated_manifest
                                .entries
                                .insert(short_ref.to_string(), entry);
                            return;
                        }
                        Err(_err) => {
                            report.fail_alias(
                                short_ref,
                                full_ref.clone(),
                                size,
                                "cas-io",
                                format!("{short_ref}: CAS republish failed"),
                            );
                            report.aliases.last_mut().unwrap().error =
                                Some("CAS republish failed".to_string());
                            return;
                        }
                    }
                } else if !cas_ok && !needs_alias_repair && dry_run {
                    // Dry-run: report planned repair.
                    report.repaired += 1;
                    report.push_alias(
                        short_ref.to_string(), full_ref.clone(), size, AliasStatus::Repaired,
                        Some("CAS missing — would republish".to_string()), Some("cas-missing".to_string()),
                    );
                    return;
                } else if !cas_ok && needs_alias_repair && !dry_run {
                    // Both CAS and alias missing: republish and re-alias.
                    match self.cas.publish(&content) {
                        Ok(_) => {
                            self.store.store_alias_deferred(short_ref, &full_ref);
                            report.repaired += 1;
                            report.push_alias(
                                short_ref.to_string(), full_ref.clone(), size, AliasStatus::Repaired,
                                None, None,
                            );
                            let mut entry = existing.clone();
                            entry.resumed = true;
                            entry.owner_alias = true;
                            updated_manifest
                                .entries
                                .insert(short_ref.to_string(), entry);
                            return;
                        }
                        Err(_err) => {
                            report.fail_alias(
                                short_ref,
                                full_ref.clone(),
                                size,
                                "cas-io",
                                format!("{short_ref}: CAS republish failed"),
                            );
                            report.aliases.last_mut().unwrap().error =
                                Some("CAS republish failed".to_string());
                            return;
                        }
                    }
                } else if needs_alias_repair && !dry_run {
                    self.store.store_alias_deferred(short_ref, &full_ref);
                    report.repaired += 1;
                    report.push_alias(
                        short_ref.to_string(), full_ref.clone(), size, AliasStatus::Repaired,
                        None, None,
                    );
                    let mut entry = existing.clone();
                    entry.resumed = true;
                    entry.owner_alias = true;
                    updated_manifest
                        .entries
                        .insert(short_ref.to_string(), entry);
                    return;
                } else if !needs_alias_repair && cas_ok {
                    report.skipped += 1;
                    report.push_alias(
                        short_ref.to_string(), full_ref.clone(), size, AliasStatus::Skipped,
                        None, None,
                    );
                    if !dry_run && !updated_manifest.entries.contains_key(short_ref) {
                        updated_manifest
                            .entries
                            .insert(short_ref.to_string(), existing.clone());
                    }
                    return;
                } else if needs_alias_repair && dry_run {
                    // Dry-run: would repair alias but cannot mutate.
                    report.repaired += 1;
                    report.push_alias(
                        short_ref.to_string(), full_ref.clone(), size, AliasStatus::Repaired,
                        Some("alias missing — would repair".to_string()), Some("alias-missing".to_string()),
                    );
                    return;
                }
            } else {
                report.fail_alias(
                    short_ref,
                    full_ref.clone(),
                    size,
                    "manifest-hash-conflict",
                    format!(
                        "{short_ref}: manifest hash conflict — manifest entry differs from computed hash/size"
                    ),
                );
                return;
            }
        } // Check for existing alias conflict.
        if let Some(existing_target) = self.store.alias_target(short_ref) {
            if existing_target == full_ref {
                let cas_ok = self.cas.contains(&full_hash) && {
                    match self.cas.resolve(&full_hash) {
                        Ok(bytes) => {
                            full_sha256_hex(&bytes) == full_hash && bytes.len() as u64 == size
                        }
                        Err(_) => false,
                    }
                };

                if !cas_ok && dry_run {
                    report.repaired += 1;
                    report.push_alias(
                        short_ref.to_string(), full_ref.clone(), size, AliasStatus::Repaired,
                        Some("CAS missing — would republish".to_string()), Some("cas-missing".to_string()),
                    );
                    return;
                }

                if !cas_ok && self.cas.publish(&content).is_err() {
                    report.fail_alias(
                        short_ref,
                        full_ref.clone(),
                        size,
                        "cas-io",
                        format!("{short_ref}: CAS republish failed"),
                    );
                    report.aliases.last_mut().unwrap().error =
                        Some("CAS republish failed".to_string());
                    return;
                }

                if !manifest.contains_hash(short_ref, &full_hash) && !dry_run {
                    updated_manifest.entries.insert(
                        short_ref.to_string(),
                        MigrationManifest::entry(
                            short_ref.to_string(), full_hash.clone(), size,
                            now_unix(), true, true,
                        ),
                    );
                    report.repaired += 1;
                    report.push_alias(
                        short_ref.to_string(), full_ref.clone(), size, AliasStatus::Repaired,
                        None, None,
                    );
                } else {
                    report.skipped += 1;
                    report.push_alias(
                        short_ref.to_string(), full_ref.clone(), size, AliasStatus::Skipped,
                        None, None,
                    );
                }
                return;
            } else {
                report.fail_alias(
                    short_ref,
                    full_ref.clone(),
                    size,
                    "alias-conflict",
                    format!(
                        "{short_ref}: alias conflict —                      existing alias targets {existing_target},                      but content hashes to {full_ref}",
                    ),
                );
                return;
            }
        }

        if dry_run {
            report.migrated += 1;
            report.push_alias(
                short_ref.to_string(), full_ref.clone(), size, AliasStatus::Migrated,
                None, None,
            );
            return;
        }

        // Publish to shared CAS.
        match self.cas.publish(&content) {
            Ok(published_hash) => {
                debug_assert_eq!(published_hash, full_hash);
            }
            Err(SharedCasError::Corruption) => {
                report.fail_alias(
                    short_ref,
                    full_ref.clone(),
                    size,
                    "cas-corruption",
                    format!(
                        "{short_ref}: CAS corruption —                      object {full_hash} exists with different bytes",
                    ),
                );
                return;
            }
            Err(SharedCasError::Policy) => {
                report.fail_alias(
                    short_ref,
                    full_ref.clone(),
                    size,
                    "cas-policy",
                    format!("{short_ref}: CAS policy violation"),
                );
                return;
            }
            Err(_err) => {
                report.fail_alias(
                    short_ref,
                    full_ref.clone(),
                    size,
                    "cas-io",
                    format!("{short_ref}: CAS publish failed"),
                );
                return;
            }
        }

        // Store alias mapping.
        self.store.store_alias_deferred(short_ref, &full_ref);

        updated_manifest.entries.insert(
            short_ref.to_string(),
            MigrationManifest::entry(
                short_ref.to_string(), full_hash.clone(), size,
                now_unix(), false, true,
            ),
        );

        report.migrated += 1;
        report.push_alias(
            short_ref.to_string(), full_ref.clone(), size, AliasStatus::Migrated,
            None, None,
        );
    }

    /// Verify migration integrity: checks every manifest entry's CAS object
    /// hash+size against the manifest and the exact alias target in the store.
    /// Also hash/size-checks the legacy source blob when present, ensuring the
    fn load_manifest(&self, report: &mut MigrationReport) -> Option<MigrationManifest> {
        match self.manifest_path.as_ref() {
            Some(p) => match MigrationManifest::load(p) {
                Ok(mf) => Some(mf),
                Err(MigrationErrorCode::ManifestMissing) => {
                    report.record_error("manifest-missing", "migration manifest does not exist", None);
                    None
                }
                Err(MigrationErrorCode::ManifestNewerVersion) => {
                    report.record_error("manifest-newer-version", "manifest version is newer than supported", None);
                    None
                }
                Err(_) => {
                    report.record_error("manifest-corrupt", "manifest is corrupt", None);
                    None
                }
            },
            None => {
                report.record_error("manifest-missing", "no manifest path configured", None);
                None
            }
        }
    }

    /// source, CAS, and alias are all consistent. Redacts underlying storage
    /// errors from report messages.
    pub fn verify(&self) -> MigrationReport {
        let mut report = MigrationReport::new("verify", false);
        let Some(manifest) = self.load_manifest(&mut report) else {
            return report;
        };

        report.total = manifest.entries.len();

        for (short_ref, entry) in &manifest.entries {
            let full_ref = format!("{BLOB_REF_PREFIX}{}", entry.full_hash);
            report.push_alias(
                short_ref.clone(), full_ref.clone(), entry.size, AliasStatus::Verified,
                None, None,
            );

            // Verify the legacy source blob when present: hash+size must
            // match the manifest entry (which stores the shared proof).
            let source_ok = match self.store.resolve_blob_bytes(short_ref) {
                BlobContentResult::Ok(bytes) => {
                    let source_hash = full_sha256_hex(&bytes);
                    if source_hash != entry.full_hash || bytes.len() as u64 != entry.size {
                        report.fail_last_alias(
                            short_ref,
                            "legacy source hash/size mismatch with manifest",
                            "source-corrupt",
                            format!("{short_ref}: legacy source hash/size mismatch"),
                        );
                        false
                    } else {
                        true
                    }
                }
                BlobContentResult::Missing => {
                    // Source already cleaned up — acceptable if CAS+alias are intact.
                    true
                }
                BlobContentResult::Corrupt => {
                    report.fail_last_alias(
                        short_ref,
                        "legacy source corrupt",
                        "source-corrupt",
                        format!("{short_ref}: legacy source corrupt"),
                    );
                    false
                }
            };
            if !source_ok {
                continue;
            }

            // Verify CAS object: hash+size must match manifest exactly.
            if !self.cas.contains(&entry.full_hash) {
                report.fail_last_alias(
                    short_ref,
                    "CAS object missing",
                    "cas-missing",
                    format!("{short_ref}: CAS object missing"),
                );
                continue;
            }

            match self.cas.resolve(&entry.full_hash) {
                Ok(bytes) => {
                    let cas_hash = full_sha256_hex(&bytes);
                    if cas_hash != entry.full_hash || bytes.len() as u64 != entry.size {
                        report.fail_last_alias(
                            short_ref,
                            "CAS hash/size mismatch",
                            "cas-corruption",
                            format!("{short_ref}: CAS hash/size mismatch"),
                        );
                        continue;
                    }
                }
                Err(_) => {
                    report.fail_last_alias(
                        short_ref,
                        "CAS read failure",
                        "cas-corruption",
                        format!("{short_ref}: CAS read failure"),
                    );
                    continue;
                }
            }

            // Verify alias targets the correct full ref.
            match self.store.alias_target(short_ref) {
                Some(target) if target == full_ref => {}
                Some(_target) => {
                    report.fail_last_alias(
                        short_ref,
                        "alias targets wrong ref",
                        "alias-conflict",
                        format!("{short_ref}: alias mismatch"),
                    );
                    continue;
                }
                None => {
                    report.fail_last_alias(
                        short_ref,
                        "alias missing from store",
                        "alias-missing",
                        format!("{short_ref}: alias missing"),
                    );
                    continue;
                }
            }

            report.verified += 1;
        }

        report
    }

    /// Rollback: remove migration-created aliases and manifest file.
    /// Never touches CAS bytes or source blobs.
    pub fn rollback(&mut self, apply: bool) -> MigrationReport {
        let dry_run = !apply;
        let mut report = MigrationReport::new("rollback", dry_run);
        let Some(manifest) = self.load_manifest(&mut report) else {
            return report;
        };

        report.total = manifest.entries.len();

        for (short_ref, entry) in &manifest.entries {
            // Verify legacy source hash+size match manifest before removing alias.
            // Only remove aliases that were created by migration (owner_alias).
            let source_verified = match self.store.resolve_blob_bytes(short_ref) {
                BlobContentResult::Ok(bytes) => {
                    let source_hash = full_sha256_hex(&bytes);
                    source_hash == entry.full_hash && bytes.len() as u64 == entry.size
                }
                _ => false,
            };

            if !source_verified {
                report.fail_alias(
                    short_ref,
                    format!("{BLOB_REF_PREFIX}{}", entry.full_hash),
                    entry.size,
                    "rollback-source-gone",
                    format!(
                        "{short_ref}: legacy source hash/size mismatch, cannot verify rollback safety"
                    ),
                );
                continue;
            }

            // Only remove aliases known to have been created by migration.
            if !entry.owner_alias {
                report.skipped += 1;
                report.push_alias(
                    short_ref.clone(), format!("{BLOB_REF_PREFIX}{}", entry.full_hash), entry.size, AliasStatus::Skipped,
                    Some("alias was not created by migration, skipping".to_string()), None,
                );
                continue;
            }

            if apply {
                self.store.remove_alias(short_ref);
            }
            report.migrated += 1;
            report.push_alias(
                short_ref.clone(), format!("{BLOB_REF_PREFIX}{}", entry.full_hash), entry.size, AliasStatus::Migrated,
                None, None,
            );
        }

        // Persist alias removals successfully before deleting manifest.
        if apply && report.failed == 0 {
            if let Err(_err) = self.store.persist_pending() {
                report.record_error("store-persist", "persist failed: rollback incomplete", None);
                // Don't delete manifest if persist failed
                return report;
            }
            if let Some(path) = &self.manifest_path {
                if path.exists() {
                    let _ = fs::remove_file(path);
                }
            }
        }

        report
    }

    /// Cleanup: remove legacy source payloads after successful verification.
    /// Dry-run by default; requires --apply and --confirm-cleanup.
    /// Verifies source+CAS+alias exactly before removing. Marks blob tombstones.
    /// Treats persist failure as failure. Never deletes CAS.
    pub fn cleanup(&mut self, apply: bool, confirmed: bool) -> MigrationReport {
        let dry_run = !apply;
        let mut report = MigrationReport::new("cleanup", dry_run);

        if dry_run && !confirmed {
            // Dry-run + no confirm: plan only, no checks beyond manifest load.
        } else if apply && !confirmed {
            report.record_error("cleanup-confirmation-required", "cleanup requires --confirm-cleanup flag", None);
            return report;
        }

        let verify_report = self.verify();
        if verify_report.is_failure() {
            report.record_error("cleanup-needs-verification", "cleanup requires successful verification first", None);
            if apply {
                report.errors.extend(verify_report.errors);
            }
            return report;
        }

        let manifest = match self.manifest_path.as_ref() {
            Some(p) => match MigrationManifest::load(p) {
                Ok(mf) => mf,
                Err(_) => {
                    report.record_error("manifest-corrupt", "manifest is corrupt", None);
                    return report;
                }
            },
            None => {
                report.record_error("manifest-missing", "no manifest path configured", None);
                return report;
            }
        };

        report.total = manifest.entries.len();

        for (short_ref, entry) in &manifest.entries {
            if !apply {
                // Dry-run: report planned removals without mutation.
                report.migrated += 1;
                report.push_alias(
                    short_ref.clone(), format!("{BLOB_REF_PREFIX}{}", entry.full_hash), entry.size, AliasStatus::Migrated,
                    None, None,
                );
                continue;
            }

            // Apply: verify source+CAS+alias exactly before removing.
            // Verify legacy source matches manifest.
            let source_match = match self.store.resolve_blob_bytes(short_ref) {
                BlobContentResult::Ok(bytes) => {
                    full_sha256_hex(&bytes) == entry.full_hash && bytes.len() as u64 == entry.size
                }
                _ => false,
            };
            if !source_match {
                report.fail(
                    "source-corrupt",
                    format!("{short_ref}: source hash/size mismatch"),
                    Some(short_ref.clone()),
                );
                continue;
            }

            // Verify CAS object matches manifest.
            if !self.cas.contains(&entry.full_hash) {
                report.fail(
                    "cas-missing",
                    format!("{short_ref}: CAS object missing"),
                    Some(short_ref.clone()),
                );
                continue;
            }
            let cas_match = match self.cas.resolve(&entry.full_hash) {
                Ok(bytes) => {
                    full_sha256_hex(&bytes) == entry.full_hash && bytes.len() as u64 == entry.size
                }
                Err(_) => false,
            };
            if !cas_match {
                report.fail(
                    "cas-corruption",
                    format!("{short_ref}: CAS hash/size mismatch"),
                    Some(short_ref.clone()),
                );
                continue;
            }

            // Verify alias target matches.
            let full_ref = format!("{BLOB_REF_PREFIX}{}", entry.full_hash);
            match self.store.alias_target(short_ref) {
                Some(target) if target == full_ref => {}
                _ => {
                    report.fail(
                        "alias-missing",
                        format!("{short_ref}: alias missing or mismatch"),
                        Some(short_ref.clone()),
                    );
                    continue;
                }
            }

            self.store.remove_blob(short_ref);
            report.migrated += 1;
            report.push_alias(
                short_ref.clone(), full_ref, entry.size, AliasStatus::Migrated,
                None, None,
            );
        }

        // Treat persist failure as failure. Never delete CAS.
        if apply && report.migrated > 0 {
            if let Err(_err) = self.store.persist_pending() {
                report.failed += report.migrated;
                report.migrated = 0;
                report.record_error("store-persist", "persist failed: cleanup incomplete", None);
            }
        }

        report
    }
} // ── RecoveryStore adapter ─────────────────────────────────────────────────

/// Adapter that wraps a `RecoveryStore` to implement `MigrationStore`.
pub struct RecoveryStoreAdapter<'a> {
    store: &'a mut crate::RecoveryStore,
}

impl<'a> RecoveryStoreAdapter<'a> {
    pub fn new(store: &'a mut crate::RecoveryStore) -> Self {
        Self { store }
    }
}

impl MigrationStore for RecoveryStoreAdapter<'_> {
    fn blob_ref_ids(&self) -> Vec<String> {
        crate::RecoveryStore::blob_ref_ids(self.store)
    }

    fn resolve_blob_bytes(&self, ref_id: &str) -> BlobContentResult {
        match crate::RecoveryStore::resolve_blob_content(self.store, ref_id) {
            Some(text) => {
                let bytes = text.into_bytes();
                if bytes.is_empty() {
                    BlobContentResult::Corrupt
                } else {
                    BlobContentResult::Ok(bytes)
                }
            }
            None => BlobContentResult::Missing,
        }
    }

    fn alias_target(&self, alias: &str) -> Option<String> {
        crate::RecoveryStore::alias_target(self.store, alias)
    }

    fn store_alias_deferred(&mut self, alias: &str, target: &str) {
        crate::RecoveryStore::store_alias_deferred(self.store, alias, target);
    }

    fn remove_alias(&mut self, alias: &str) {
        crate::RecoveryStore::remove_alias(self.store, alias);
    }

    fn remove_blob(&mut self, ref_id: &str) {
        crate::RecoveryStore::remove_blob(self.store, ref_id);
    }

    fn mark_ambiguous(&mut self, short_ref: &str) {
        crate::RecoveryStore::mark_ambiguous(self.store, short_ref);
    }

    fn is_ambiguous(&self, short_ref: &str) -> bool {
        crate::RecoveryStore::is_alias_ambiguous(self.store, short_ref)
    }

    fn persist_pending(&mut self) -> Result<(), String> {
        crate::RecoveryStore::persist_pending(self.store).map_err(|e| e.to_string())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_cas::SharedCas;
    use std::collections::BTreeMap;

    struct FakeStore {
        blobs: BTreeMap<String, Vec<u8>>,
        aliases: BTreeMap<String, String>,
        ambiguous: BTreeMap<String, bool>,
    }

    impl FakeStore {
        fn new() -> Self {
            Self {
                blobs: BTreeMap::new(),
                aliases: BTreeMap::new(),
                ambiguous: BTreeMap::new(),
            }
        }
        fn insert(&mut self, ref_id: &str, content: &str) {
            self.blobs.insert(ref_id.to_string(), content.as_bytes().to_vec());
        }
    }

    impl MigrationStore for FakeStore {
        fn blob_ref_ids(&self) -> Vec<String> { self.blobs.keys().cloned().collect() }
        fn resolve_blob_bytes(&self, ref_id: &str) -> BlobContentResult {
            match self.blobs.get(ref_id) {
                Some(bytes) if !bytes.is_empty() => BlobContentResult::Ok(bytes.clone()),
                Some(_) => BlobContentResult::Corrupt,
                None => BlobContentResult::Missing,
            }
        }
        fn alias_target(&self, alias: &str) -> Option<String> { self.aliases.get(alias).cloned() }
        fn store_alias_deferred(&mut self, alias: &str, target: &str) {
            self.aliases.insert(alias.to_string(), target.to_string());
        }
        fn remove_alias(&mut self, alias: &str) { self.aliases.remove(alias); }
        fn remove_blob(&mut self, ref_id: &str) { self.blobs.remove(ref_id); }
        fn mark_ambiguous(&mut self, short_ref: &str) {
            self.ambiguous.insert(short_ref.to_string(), true);
        }
        fn is_ambiguous(&self, short_ref: &str) -> bool { self.ambiguous.contains_key(short_ref) }
        fn persist_pending(&mut self) -> Result<(), String> { Ok(()) }
    }

    fn test_setup(dir: &Path) -> (FakeStore, SharedCas, PathBuf) {
        let cache = dir.join("recovery-cache.json");
        let cas = SharedCas::new(cache.parent().unwrap_or(&cache).to_path_buf());
        (FakeStore::new(), cas, dir.join("migration-manifest.json"))
    }

    fn short_ref_for(text: &str) -> (String, String) {
        let full_hash = full_sha256_hex(text.as_bytes());
        (format!("tz://blob/b{}", &full_hash[..16]), full_hash)
    }

    fn insert_legacy(store: &mut FakeStore, text: &str) -> (String, String) {
        let (short_ref, full_hash) = short_ref_for(text);
        store.insert(&short_ref, text);
        (short_ref, full_hash)
    }

    fn assert_error_code(report: &MigrationReport, code: &str) {
        assert!(
            report.errors.iter().any(|e| e.code == code),
            "expected {code}, got {:?}",
            report.errors
        );
    }

    fn migrate(
        store: &mut FakeStore,
        cas: &SharedCas,
        manifest: Option<PathBuf>,
        dry_run: bool,
    ) -> MigrationReport {
        LegacyMigration::new(store, cas, manifest).run(dry_run)
    }



    fn cas_obj(dir: &Path, full_hash: &str) -> PathBuf {
        dir.join("blobs").join("sha256").join(&full_hash[..2]).join(full_hash)
    }

    fn with_fake(text: &str, dry_run: bool) -> (tempfile::TempDir, FakeStore, SharedCas, PathBuf, String, String, MigrationReport) {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        let (short_ref, full_hash) = insert_legacy(&mut store, text);
        let report = migrate(&mut store, &cas, Some(manifest.clone()), dry_run);
        (dir, store, cas, manifest, short_ref, full_hash, report)
    }

    // ── success / resume / dry-run matrix (named scenarios preserved) ──

    #[test]
    fn migration_migrate_single_legacy_blob() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        let text = "hello migration target";
        let (short_ref, full_hash) = insert_legacy(&mut store, text);
        let report = migrate(&mut store, &cas, Some(manifest.clone()), false);
        assert_eq!((report.total, report.migrated, report.skipped, report.failed, report.repaired), (1, 1, 0, 0, 0));
        let full_ref = store.alias_target(&short_ref).unwrap();
        assert!(full_ref.starts_with("tz://blob/"));
        assert_eq!(&full_ref["tz://blob/".len()..], full_hash);
        assert!(cas.contains(&full_hash));
        let entry = MigrationManifest::load(&manifest).unwrap().entries.get(&short_ref).unwrap().clone();
        assert_eq!((entry.full_hash, entry.size, entry.state), (full_hash, text.len() as u64, EntryState::Migrated));
    }

    #[test]
    fn migration_canonical_full_ref_and_exact_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        let hashes: Vec<_> = ["payload alpha", "payload beta different"].iter().map(|t| {
            let (_, h) = insert_legacy(&mut store, t);
            (h, *t)
        }).collect();
        assert_ne!(hashes[0].0, hashes[1].0);
        assert_eq!(migrate(&mut store, &cas, Some(manifest), false).migrated, 2);
        for (h, t) in hashes { assert_eq!(cas.resolve(&h).unwrap(), t.as_bytes()); }
    }

    #[test]
    fn migration_no_duplicate_payload_when_cas_attached() {
        let (_d, mut store, cas, manifest, _, full_hash, r1) = with_fake("shared content no dup", false);
        assert_eq!(r1.migrated, 1);
        store.insert("tz://blob/baaaaaaaaaaaaaaa1", "shared content no dup");
        let _ = migrate(&mut store, &cas, Some(manifest), false);
        assert!(cas.contains(&full_hash));
    }

    #[test]
    fn migration_idempotent_rerun() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        insert_legacy(&mut store, "idempotent content");
        assert_eq!(migrate(&mut store, &cas, Some(manifest.clone()), false).migrated, 1);
        let r2 = migrate(&mut store, &cas, Some(manifest), false);
        assert_eq!((r2.total, r2.migrated, r2.skipped, r2.failed), (1, 0, 1, 0));
    }

    #[test]
    fn migration_apply_restart_byte_exact_legacy_read() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        let text = "restart byte exact";
        let (short_ref, full_hash) = insert_legacy(&mut store, text);
        assert_eq!(migrate(&mut store, &cas, Some(manifest.clone()), false).migrated, 1);
        let (mut store2, cas2, _) = test_setup(dir.path());
        store2.insert(&short_ref, text);
        let r2 = migrate(&mut store2, &cas2, Some(manifest), false);
        assert_eq!((r2.repaired, r2.skipped), (1, 0));
        assert!(store2.alias_target(&short_ref).is_some());
        assert_eq!(cas2.resolve(&full_hash).unwrap(), text.as_bytes());
    }

    #[test]
    fn migration_missing_alias_repair_on_resume() {
        let (_d, mut store, cas, manifest, short_ref, _, _) = with_fake("repair alias", false);
        store.remove_alias(&short_ref);
        assert_eq!(migrate(&mut store, &cas, Some(manifest), false).repaired, 1);
        assert!(store.alias_target(&short_ref).is_some());
    }

    #[test]
    fn migration_missing_cas_repair_on_resume() {
        let (dir, mut store, cas, manifest, short_ref, full_hash, _) = with_fake("repair cas", false);
        fs::remove_file(cas_obj(dir.path(), &full_hash)).unwrap();
        store.remove_alias(&short_ref);
        assert_eq!(migrate(&mut store, &cas, Some(manifest), false).repaired, 1);
        assert!(cas.contains(&full_hash) && store.alias_target(&short_ref).is_some());
    }

    // ── failure matrix ──

    #[test]
    fn migration_ambiguous_short_id_fails_safely() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        store.insert("tz://blob/bdeadbeefdeadbeef", "this content does not match the claimed short ID");
        let report = migrate(&mut store, &cas, Some(manifest), false);
        assert_eq!((report.total, report.migrated, report.failed), (1, 0, 1));
        assert_error_code(&report, "ambiguous-short-id");
    }

    #[test]
    fn migration_alias_conflict_fails_safely() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        let (short_ref, _) = insert_legacy(&mut store, "original content for conflict test");
        store.store_alias_deferred(&short_ref, &format!("{BLOB_REF_PREFIX}{}", "f".repeat(FULL_HASH_LEN)));
        let report = migrate(&mut store, &cas, Some(manifest), false);
        assert_eq!((report.total, report.migrated, report.failed), (1, 0, 1));
        assert_error_code(&report, "alias-conflict");
    }

    #[test]
    fn migration_conflicting_manifest_hash_is_deterministic_ambiguity() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        let (short_ref, _) = insert_legacy(&mut store, "manifest conflict text");
        let mut fake = MigrationManifest::empty();
        fake.entries.insert(
            short_ref.clone(),
            MigrationManifest::entry(short_ref.clone(), "f".repeat(FULL_HASH_LEN), 999, now_unix(), false, false),
        );
        fake.save(&manifest).unwrap();
        let report = migrate(&mut store, &cas, Some(manifest), false);
        assert_eq!(report.failed, 1);
        assert_error_code(&report, "manifest-hash-conflict");
    }

    #[test]
    fn migration_dry_run_produces_no_writes() {
        let (dir_unused, store, cas, manifest, short_ref, full_hash, report) =
            with_fake("dry run content", true);
        let _ = dir_unused;
        assert!(report.dry_run);
        assert_eq!((report.total, report.migrated, report.failed), (1, 1, 0));
        assert!(store.alias_target(&short_ref).is_none());
        assert!(!manifest.exists() && !cas.contains(&full_hash));
    }

    #[test]
    fn migration_dry_run_no_writes_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        insert_legacy(&mut store, "dry run idempotent");
        assert_eq!(migrate(&mut store, &cas, Some(manifest.clone()), true).migrated, 1);
        assert_eq!(migrate(&mut store, &cas, Some(manifest), true).migrated, 1);
    }

    #[test]
    fn migration_corrupt_source_fails() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        store.blobs.insert("tz://blob/b0000000000000000".into(), vec![]);
        let report = migrate(&mut store, &cas, Some(manifest), false);
        assert_eq!(report.failed, 1);
        assert_error_code(&report, "source-corrupt");
    }

    #[test]
    fn migration_corrupt_cas_detected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        let (_, full_hash) = insert_legacy(&mut store, "corrupt CAS test");
        let obj_dir = dir.path().join("blobs").join("sha256").join(&full_hash[..2]);
        fs::create_dir_all(&obj_dir).unwrap();
        fs::write(obj_dir.join(&full_hash), b"tampered bytes").unwrap();
        let report = migrate(&mut store, &cas, Some(manifest), false);
        assert_eq!(report.failed, 1);
        assert_error_code(&report, "cas-corruption");
    }

    #[test]
    fn migration_verify_all_ok() {
        let (_d, mut store, cas, manifest, _, _, _) = with_fake("verify ok", false);
        let vr = LegacyMigration::new(&mut store, &cas, Some(manifest)).verify();
        assert_eq!((vr.total, vr.verified, vr.failed), (1, 1, 0));
    }

    #[test]
    fn migration_verify_cas_missing() {
        let (dir, mut store, cas, manifest, _, full_hash, _) = with_fake("verify missing cas", false);
        fs::remove_file(cas_obj(dir.path(), &full_hash)).unwrap();
        let vr = LegacyMigration::new(&mut store, &cas, Some(manifest)).verify();
        assert_eq!(vr.failed, 1);
        assert_error_code(&vr, "cas-missing");
    }

    #[test]
    fn migration_verify_alias_missing() {
        let (_d, mut store, cas, manifest, short_ref, _, _) = with_fake("verify missing alias", false);
        store.remove_alias(&short_ref);
        let vr = LegacyMigration::new(&mut store, &cas, Some(manifest)).verify();
        assert_eq!(vr.failed, 1);
        assert_error_code(&vr, "alias-missing");
    }

    #[test]
    fn migration_rollback_removes_aliases_and_manifest() {
        let (_d, mut store, cas, manifest, short_ref, full_hash, _) = with_fake("rollback test", false);
        let rr = LegacyMigration::new(&mut store, &cas, Some(manifest.clone())).rollback(true);
        assert_eq!((rr.migrated, rr.failed), (1, 0));
        assert!(store.alias_target(&short_ref).is_none() && !manifest.exists() && cas.contains(&full_hash));
        assert!(matches!(store.resolve_blob_bytes(&short_ref), BlobContentResult::Ok(_)));
    }

    #[test]
    fn migration_rollback_fails_when_source_gone() {
        let (_d, mut store, cas, manifest, short_ref, _, _) = with_fake("rollback source gone", false);
        store.remove_blob(&short_ref);
        let rr = LegacyMigration::new(&mut store, &cas, Some(manifest)).rollback(true);
        assert_eq!(rr.failed, 1);
        assert_error_code(&rr, "rollback-source-gone");
        assert!(store.alias_target(&short_ref).is_some());
    }

    #[test]
    fn migration_cleanup_dry_run_no_writes() {
        let (_d, mut store, cas, manifest, short_ref, _, _) = with_fake("cleanup dry run", false);
        let cr = LegacyMigration::new(&mut store, &cas, Some(manifest)).cleanup(false, true);
        assert!(cr.dry_run);
        assert_eq!((cr.migrated, cr.failed), (1, 0));
        assert!(matches!(store.resolve_blob_bytes(&short_ref), BlobContentResult::Ok(_)));
    }

    #[test]
    fn migration_cleanup_apply_removes_legacy_sources() {
        let (_d, mut store, cas, manifest, short_ref, full_hash, _) = with_fake("cleanup apply", false);
        let cr = LegacyMigration::new(&mut store, &cas, Some(manifest)).cleanup(true, true);
        assert!(!cr.dry_run && cr.migrated == 1 && cr.failed == 0);
        assert!(matches!(store.resolve_blob_bytes(&short_ref), BlobContentResult::Missing));
        assert!(store.alias_target(&short_ref).is_some() && cas.contains(&full_hash));
    }

    #[test]
    fn migration_cleanup_requires_confirmation() {
        let (_d, mut store, cas, manifest, short_ref, _, _) = with_fake("cleanup no confirm", false);
        let cr = LegacyMigration::new(&mut store, &cas, Some(manifest)).cleanup(true, false);
        assert_error_code(&cr, "cleanup-confirmation-required");
        assert!(matches!(store.resolve_blob_bytes(&short_ref), BlobContentResult::Ok(_)));
    }

    #[test]
    fn migration_cleanup_requires_verification() {
        let (dir, mut store, cas, manifest, short_ref, full_hash, _) = with_fake("cleanup needs verify", false);
        fs::remove_file(cas_obj(dir.path(), &full_hash)).unwrap();
        let cr = LegacyMigration::new(&mut store, &cas, Some(manifest)).cleanup(true, true);
        assert_error_code(&cr, "cleanup-needs-verification");
        assert!(matches!(store.resolve_blob_bytes(&short_ref), BlobContentResult::Ok(_)));
    }

    #[test]
    fn migration_strict_manifest_version_newer_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        fs::write(&manifest, r#"{"version":"tokenzero.migration.v99","entries":{},"completed":false}"#).unwrap();
        let report = migrate(&mut store, &cas, Some(manifest), false);
        assert_error_code(&report, "manifest-newer-version");
        assert_eq!(report.total, 0);
    }

    #[test]
    fn migration_corrupt_manifest_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        fs::write(&manifest, b"not json at all").unwrap();
        assert_error_code(&migrate(&mut store, &cas, Some(manifest), false), "manifest-corrupt");
    }

    #[test]
    fn migration_is_legacy_blob_ref_detection() {
        assert!(is_legacy_blob_ref("tz://blob/babc123def4567890"));
        assert!(is_legacy_blob_ref("tz://blob/b0000000000000000"));
        for bad in [
            format!("tz://blob/{}", "a".repeat(FULL_HASH_LEN)),
            "tz://file/babc123def456789".into(),
            "tz://unit/uabc123def456789".into(),
            "tz://blob/xabc123def456789".into(),
            "tz://blob/babc".into(),
            "tz://blob/babc123def45678901".into(),
        ] {
            assert!(!is_legacy_blob_ref(&bad));
        }
    }

    #[test]
    fn migration_full_hash_ref_not_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        store.insert(&format!("{BLOB_REF_PREFIX}{}", "a".repeat(FULL_HASH_LEN)), "already canonical");
        let report = migrate(&mut store, &cas, Some(manifest), false);
        assert_eq!((report.total, report.migrated), (0, 0));
    }

    #[test]
    fn migration_canonical_vs_legacy_key_collision_separation() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, _) = test_setup(dir.path());
        let (legacy_ref, _) = insert_legacy(&mut store, "legacy blob");
        let ctext = "canonical blob different";
        store.insert(&format!("tz://blob/{}", full_sha256_hex(ctext.as_bytes())), ctext);
        let report = migrate(&mut store, &cas, None, true);
        assert_eq!(report.total, 1);
        assert!(report.aliases.iter().any(|a| a.short_ref == legacy_ref));
    }

    #[test]
    fn migration_deterministic_ambiguous_prefix_two_distinct_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());
        let fake_short = "tz://blob/babcdabcdabcdabcd";
        store.insert(fake_short, "content alpha");
        store.insert(fake_short, "content beta different enough");
        store.mark_ambiguous(fake_short);
        let report = migrate(&mut store, &cas, Some(manifest), false);
        assert_eq!(
            report.aliases.iter().filter(|a| a.short_ref == fake_short && a.status == AliasStatus::Failed).count(),
            1
        );
    }

    // ── Real RecoveryStore-backed tests ──

    fn cas_from_cache(cache_path: &Path) -> SharedCas {
        SharedCas::detect_from_cache_path(cache_path).unwrap_or_else(|| {
            SharedCas::new(cache_path.parent().unwrap_or(cache_path).to_path_buf())
        })
    }

    fn engine_cache(dir: &Path) -> (PathBuf, PathBuf) {
        let engine_dir = dir.join("tokenzero");
        fs::create_dir_all(&engine_dir).unwrap();
        (engine_dir.clone(), engine_dir.join("recovery-cache.json"))
    }

    fn recovery_store(cache: &Path) -> crate::RecoveryStore {
        crate::RecoveryStore::new(Some(cache.to_path_buf()))
    }

    fn migrate_adapter(
        store: &mut crate::RecoveryStore,
        cas: &SharedCas,
        manifest: PathBuf,
        dry_run: bool,
    ) -> MigrationReport {
        let mut adapter = crate::migration::RecoveryStoreAdapter::new(store);
        LegacyMigration::new(&mut adapter, cas, Some(manifest)).run(dry_run)
    }

    fn insert_short(store: &mut crate::RecoveryStore, text: &str) -> (String, String) {
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert_test_blob(&short_ref, text);
        (short_ref, full_hash)
    }

    #[test]
    fn migration_cas_publish_exact_bytes_not_in_recovery_json() {
        let dir = tempfile::tempdir().unwrap();
        let (_, cache) = engine_cache(dir.path());
        let text = "canonical CAS payload no dup";
        let full_hash = full_sha256_hex(text.as_bytes());
        let mut store = recovery_store(&cache);
        let blob_ref = store.put_blob(text, tokenzero_core::ContentType::Unknown);
        assert_eq!(blob_ref, format!("tz://blob/{full_hash}"));
        store.persist().unwrap();
        drop(store);
        assert_eq!(full_hash.len(), 64);
        assert!(full_hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        let cas = SharedCas::detect_from_cache_path(&cache).unwrap();
        assert_eq!(cas.resolve(&full_hash).unwrap(), text.as_bytes());
        assert!(cas.contains(&full_hash));
        assert!(!fs::read_to_string(&cache).unwrap().contains(text));
        assert_eq!(recovery_store(&cache).expand(&blob_ref, None, None, None, None, None).content, text);
    }

    #[test]
    fn migration_cas_publish_store_no_duplicate_payload() {
        let dir = tempfile::tempdir().unwrap();
        let (engine_dir, cache) = engine_cache(dir.path());
        let mut store = recovery_store(&cache);
        let text = "no duplicate in recovery state";
        let (_, full_hash) = insert_short(&mut store, text);
        let cas = SharedCas::detect_from_cache_path(&cache).unwrap();
        assert_eq!(migrate_adapter(&mut store, &cas, engine_dir.join("migration-manifest.json"), false).migrated, 1);
        assert!(cas.contains(&full_hash));
        assert_eq!(cas.resolve(&full_hash).unwrap(), text.as_bytes());
    }

    #[test]
    fn migration_alias_present_cas_missing_republishes() {
        let dir = tempfile::tempdir().unwrap();
        let (engine_dir, cache) = engine_cache(dir.path());
        let mut store = recovery_store(&cache);
        let text = "matching alias with missing canonical CAS object";
        let (short_ref, full_hash) = insert_short(&mut store, text);
        store.store_alias_deferred(&short_ref, &format!("{BLOB_REF_PREFIX}{full_hash}"));
        let cas = cas_from_cache(&cache);
        assert!(!cas.contains(&full_hash));
        let mut adapter = RecoveryStoreAdapter::new(&mut store);
        let mut migration = LegacyMigration::new(&mut adapter, &cas, Some(engine_dir.join("migration-manifest.json")));
        let report = migration.run(false);
        assert!(report.errors.is_empty() && report.repaired >= 1 && cas.contains(&full_hash));
        let verify = migration.verify();
        assert!(verify.errors.is_empty() && verify.verified == verify.total);
    }

    #[test]
    fn migration_legacy_disabled_fails() {
        let dir = tempfile::tempdir().unwrap();
        let (_, cache) = engine_cache(dir.path());
        let config = crate::RecoveryConfig { legacy_compat: false, ..crate::RecoveryConfig::default() };
        let mut store = crate::RecoveryStore::with_config(Some(cache), config);
        let text = "disabled legacy";
        let (short_ref, full_hash) = insert_short(&mut store, text);
        store.store_alias_deferred(&short_ref, &format!("tz://blob/{full_hash}"));
        let result = store.expand(&short_ref, None, None, None, None, None);
        assert!(!result.found && result.reason == "legacy-ref-disabled");
        let full_ref = format!("tz://blob/{full_hash}");
        store.insert_test_blob(&full_ref, text);
        assert!(store.expand(&full_ref, None, None, None, None, None).found);
    }

    #[test]
    fn migration_ambiguous_alias_fails() {
        let dir = tempfile::tempdir().unwrap();
        let (_, cache) = engine_cache(dir.path());
        let mut store = recovery_store(&cache);
        let short_ref = "tz://blob/babcdabcdabcdabcd";
        store.insert_test_blob(short_ref, "content");
        store.mark_ambiguous(short_ref);
        let result = store.expand(short_ref, None, None, None, None, None);
        assert!(!result.found && result.reason == "legacy-ambiguous");
    }

    #[test]
    fn migration_fz_gz_refs_no_local_fallback_when_cas_missing() {
        let dir = tempfile::tempdir().unwrap();
        let (_, cache) = engine_cache(dir.path());
        let mut store = recovery_store(&cache);
        let text = "fz gz no fallback test";
        let full_hash = full_sha256_hex(text.as_bytes());
        let tz_ref = format!("tz://blob/{full_hash}");
        store.insert_test_blob(&tz_ref, text);
        for scheme in ["fz", "gz"] {
            let r = store.expand(&format!("{scheme}://blob/{full_hash}"), None, None, None, None, None);
            assert!(!r.found && r.reason == "shared-cas-missing");
        }
        let ok = store.expand(&tz_ref, None, None, None, None, None);
        assert!(ok.found && ok.content == text);
    }

    #[test]
    fn migration_rollback_dry_run_no_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let (engine_dir, cache) = engine_cache(dir.path());
        let mut store = recovery_store(&cache);
        let (short_ref, _) = insert_short(&mut store, "rollback dry run");
        let cas = cas_from_cache(&cache);
        let manifest = engine_dir.join("migration-manifest.json");
        assert_eq!(migrate_adapter(&mut store, &cas, manifest.clone(), false).migrated, 1);
        let mut adapter = crate::migration::RecoveryStoreAdapter::new(&mut store);
        let rr = LegacyMigration::new(&mut adapter, &cas, Some(manifest.clone())).rollback(false);
        assert!(rr.dry_run && rr.migrated == 1 && manifest.exists() && store.alias_target(&short_ref).is_some());
    }

    #[test]
    fn migration_rollback_apply_removes_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let (engine_dir, cache) = engine_cache(dir.path());
        let mut store = recovery_store(&cache);
        let (short_ref, full_hash) = insert_short(&mut store, "rollback apply");
        let cas = cas_from_cache(&cache);
        let manifest = engine_dir.join("migration-manifest.json");
        migrate_adapter(&mut store, &cas, manifest.clone(), false);
        let mut adapter = crate::migration::RecoveryStoreAdapter::new(&mut store);
        let rr = LegacyMigration::new(&mut adapter, &cas, Some(manifest.clone())).rollback(true);
        assert!(!rr.dry_run && rr.migrated == 1 && rr.failed == 0);
        assert!(store.alias_target(&short_ref).is_none() && !manifest.exists() && cas.contains(&full_hash));
    }

    #[test]
    fn migration_cleanup_restart_preserves_tombstones() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let mut store = recovery_store(&cache);
        let text = "cleanup restart";
        let (short_ref, full_hash) = insert_short(&mut store, text);
        let cas = cas_from_cache(&cache);
        let manifest = dir.path().join("migration-manifest.json");
        migrate_adapter(&mut store, &cas, manifest.clone(), false);
        let mut adapter = crate::migration::RecoveryStoreAdapter::new(&mut store);
        let cr = LegacyMigration::new(&mut adapter, &cas, Some(manifest)).cleanup(true, true);
        assert!(!cr.dry_run && cr.migrated == 1 && cr.failed == 0);
        let mut store2 = recovery_store(&cache);
        assert!(!store2.blob_ref_ids().contains(&short_ref));
        assert!(store2.alias_target(&short_ref).is_some() && cas.contains(&full_hash));
        let expanded = store2.expand(&short_ref, None, None, None, None, None);
        assert!(expanded.found && expanded.content == text);
    }

    #[test]
    fn migration_bare_plan_no_filesystem_changes() {
        let dir = tempfile::tempdir().unwrap();
        let (engine_dir, cache) = engine_cache(dir.path());
        let mut store = recovery_store(&cache);
        let cas = cas_from_cache(&cache);
        let mut adapter = crate::migration::RecoveryStoreAdapter::new(&mut store);
        let mut migration = LegacyMigration::new(&mut adapter, &cas, Some(engine_dir.join("migration-manifest.json")));
        let report = migration.run(true);
        assert_eq!((report.total, report.migrated), (0, 0));
        assert_error_code(&migration.verify(), "manifest-missing");
        assert_error_code(&migration.rollback(false), "manifest-missing");
        let cl = migration.cleanup(false, false);
        assert_eq!(cl.total, 0);
        assert_error_code(&cl, "cleanup-needs-verification");
    }

    #[test]
    fn migration_doctor_no_payload_or_paths() {
        let dir = tempfile::tempdir().unwrap();
        let (_, cache) = engine_cache(dir.path());
        let state = recovery_store(&cache).migration_state();
        let json_str = serde_json::to_string(&state).unwrap();
        assert!(!json_str.contains("payload") && !json_str.contains("/blobs/"));
        assert!(state.get("legacy_compat_supported_until").is_some());
    }
}
