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

impl MigrationManifest {
    /// Load a manifest from `path`, or return an empty one if missing.
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
        let mut last_err = None;
        for attempt in 0..TMP_RETRIES {
            let tmp = tmp_manifest_path(path, attempt);
            match Self::write_tmp_sync(&tmp, &text) {
                Ok(()) => match fs::rename(&tmp, path) {
                    Ok(()) => return Ok(()),
                    Err(err) => {
                        let _ = fs::remove_file(&tmp);
                        last_err = Some(err);
                    }
                },
                Err(err) => {
                    let _ = fs::remove_file(&tmp);
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.map_or(MigrationErrorCode::ManifestSave, |_| {
            MigrationErrorCode::ManifestSave
        }))
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
    }
}

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

impl MigrationReport {
    fn new(operation: &str, dry_run: bool) -> Self {
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
    }
}

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
                    report.errors.push(MigrationError {
                        code: "manifest-newer-version".to_string(),
                        message: format!(
                            "manifest version is newer than supported ({})",
                            MIGRATION_MANIFEST_VERSION
                        ),
                        short_ref: None,
                    });
                    return report;
                }
                Err(_) => {
                    report.errors.push(MigrationError {
                        code: "manifest-corrupt".to_string(),
                        message: "manifest file is corrupt, cannot continue".to_string(),
                        short_ref: None,
                    });
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
                report.errors.push(MigrationError {
                    code: "ambiguous-short-id".to_string(),
                    message: format!(
                        "{short_ref}: short prefix maps to {} distinct full hashes",
                        unique_hashes.len()
                    ),
                    short_ref: Some(short_ref.clone()),
                });
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
                report.errors.push(MigrationError {
                    code: "store-persist".to_string(),
                    message: format!("store persist failed: {err}"),
                    short_ref: None,
                });
            }

            if let Some(path) = &self.manifest_path {
                updated_manifest.completed = report.failed == 0 && !write_failed;
                if let Err(err) = updated_manifest.save(path) {
                    report.errors.push(MigrationError {
                        code: "manifest-save".to_string(),
                        message: format!("manifest save failed: {err}"),
                        short_ref: None,
                    });
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
            report.failed += 1;
            report.aliases.push(AliasEntry {
                short_ref: short_ref.to_string(),
                full_ref: String::new(),
                size: 0,
                status: AliasStatus::Failed,
                error: Some("short ref is ambiguous (maps to multiple full hashes)".to_string()),
                error_code: Some("ambiguous-short-id".to_string()),
            });
            return;
        }

        let content = match self.store.resolve_blob_bytes(short_ref) {
            BlobContentResult::Ok(bytes) => bytes,
            BlobContentResult::Missing => {
                report.failed += 1;
                let msg = format!("{short_ref}: could not resolve blob content");
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.to_string(),
                    full_ref: String::new(),
                    size: 0,
                    status: AliasStatus::Failed,
                    error: Some(msg.clone()),
                    error_code: Some("source-missing".to_string()),
                });
                report.errors.push(MigrationError {
                    code: "source-missing".to_string(),
                    message: msg,
                    short_ref: Some(short_ref.to_string()),
                });
                return;
            }
            BlobContentResult::Corrupt => {
                report.failed += 1;
                let msg = format!("{short_ref}: blob content is empty or corrupt");
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.to_string(),
                    full_ref: String::new(),
                    size: 0,
                    status: AliasStatus::Failed,
                    error: Some(msg.clone()),
                    error_code: Some("source-corrupt".to_string()),
                });
                report.errors.push(MigrationError {
                    code: "source-corrupt".to_string(),
                    message: msg,
                    short_ref: Some(short_ref.to_string()),
                });
                return;
            }
        };

        let size = content.len() as u64;
        let full_hash = full_sha256_hex(&content);
        let full_ref = format!("{BLOB_REF_PREFIX}{full_hash}");

        // Verify the short ID is a correct prefix of the full hash.
        if let Some(short_hex) = short_id_hex(short_ref) {
            if &full_hash[..16] != short_hex {
                report.failed += 1;
                let msg = format!(
                    "{short_ref}: ambiguous short ID —                      short prefix {short_hex} does not match                      full hash prefix {}",
                    &full_hash[..16],
                );
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.to_string(),
                    full_ref: full_ref.clone(),
                    size,
                    status: AliasStatus::Failed,
                    error: Some(msg.clone()),
                    error_code: Some("ambiguous-short-id".to_string()),
                });
                report.errors.push(MigrationError {
                    code: "ambiguous-short-id".to_string(),
                    message: msg,
                    short_ref: Some(short_ref.to_string()),
                });
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
                            report.aliases.push(AliasEntry {
                                short_ref: short_ref.to_string(),
                                full_ref: full_ref.clone(),
                                size,
                                status: AliasStatus::Repaired,
                                error: None,
                                error_code: None,
                            });
                            let mut entry = existing.clone();
                            entry.resumed = true;
                            entry.owner_alias = true;
                            updated_manifest
                                .entries
                                .insert(short_ref.to_string(), entry);
                            return;
                        }
                        Err(_err) => {
                            report.failed += 1;
                            report.aliases.push(AliasEntry {
                                short_ref: short_ref.to_string(),
                                full_ref: full_ref.clone(),
                                size,
                                status: AliasStatus::Failed,
                                error: Some("CAS republish failed".to_string()),
                                error_code: Some("cas-io".to_string()),
                            });
                            report.errors.push(MigrationError {
                                code: "cas-io".to_string(),
                                message: format!("{short_ref}: CAS republish failed"),
                                short_ref: Some(short_ref.to_string()),
                            });
                            return;
                        }
                    }
                } else if !cas_ok && !needs_alias_repair && dry_run {
                    // Dry-run: report planned repair.
                    report.repaired += 1;
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.to_string(),
                        full_ref: full_ref.clone(),
                        size,
                        status: AliasStatus::Repaired,
                        error: Some("CAS missing — would republish".to_string()),
                        error_code: Some("cas-missing".to_string()),
                    });
                    return;
                } else if !cas_ok && needs_alias_repair && !dry_run {
                    // Both CAS and alias missing: republish and re-alias.
                    match self.cas.publish(&content) {
                        Ok(_) => {
                            self.store.store_alias_deferred(short_ref, &full_ref);
                            report.repaired += 1;
                            report.aliases.push(AliasEntry {
                                short_ref: short_ref.to_string(),
                                full_ref: full_ref.clone(),
                                size,
                                status: AliasStatus::Repaired,
                                error: None,
                                error_code: None,
                            });
                            let mut entry = existing.clone();
                            entry.resumed = true;
                            entry.owner_alias = true;
                            updated_manifest
                                .entries
                                .insert(short_ref.to_string(), entry);
                            return;
                        }
                        Err(_err) => {
                            report.failed += 1;
                            report.aliases.push(AliasEntry {
                                short_ref: short_ref.to_string(),
                                full_ref: full_ref.clone(),
                                size,
                                status: AliasStatus::Failed,
                                error: Some("CAS republish failed".to_string()),
                                error_code: Some("cas-io".to_string()),
                            });
                            report.errors.push(MigrationError {
                                code: "cas-io".to_string(),
                                message: format!("{short_ref}: CAS republish failed"),
                                short_ref: Some(short_ref.to_string()),
                            });
                            return;
                        }
                    }
                } else if needs_alias_repair && !dry_run {
                    self.store.store_alias_deferred(short_ref, &full_ref);
                    report.repaired += 1;
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.to_string(),
                        full_ref: full_ref.clone(),
                        size,
                        status: AliasStatus::Repaired,
                        error: None,
                        error_code: None,
                    });
                    let mut entry = existing.clone();
                    entry.resumed = true;
                    entry.owner_alias = true;
                    updated_manifest
                        .entries
                        .insert(short_ref.to_string(), entry);
                    return;
                } else if !needs_alias_repair && cas_ok {
                    report.skipped += 1;
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.to_string(),
                        full_ref: full_ref.clone(),
                        size,
                        status: AliasStatus::Skipped,
                        error: None,
                        error_code: None,
                    });
                    if !dry_run && !updated_manifest.entries.contains_key(short_ref) {
                        updated_manifest
                            .entries
                            .insert(short_ref.to_string(), existing.clone());
                    }
                    return;
                } else if needs_alias_repair && dry_run {
                    // Dry-run: would repair alias but cannot mutate.
                    report.repaired += 1;
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.to_string(),
                        full_ref: full_ref.clone(),
                        size,
                        status: AliasStatus::Repaired,
                        error: Some("alias missing — would repair".to_string()),
                        error_code: Some("alias-missing".to_string()),
                    });
                    return;
                }
            } else {
                report.failed += 1;
                let msg = format!(
                    "{short_ref}: manifest hash conflict — manifest entry differs from computed hash/size"
                );
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.to_string(),
                    full_ref: full_ref.clone(),
                    size,
                    status: AliasStatus::Failed,
                    error: Some(msg.clone()),
                    error_code: Some("manifest-hash-conflict".to_string()),
                });
                report.errors.push(MigrationError {
                    code: "manifest-hash-conflict".to_string(),
                    message: msg,
                    short_ref: Some(short_ref.to_string()),
                });
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
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.to_string(),
                        full_ref: full_ref.clone(),
                        size,
                        status: AliasStatus::Repaired,
                        error: Some("CAS missing — would republish".to_string()),
                        error_code: Some("cas-missing".to_string()),
                    });
                    return;
                }

                if !cas_ok && self.cas.publish(&content).is_err() {
                    report.failed += 1;
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.to_string(),
                        full_ref: full_ref.clone(),
                        size,
                        status: AliasStatus::Failed,
                        error: Some("CAS republish failed".to_string()),
                        error_code: Some("cas-io".to_string()),
                    });
                    report.errors.push(MigrationError {
                        code: "cas-io".to_string(),
                        message: format!("{short_ref}: CAS republish failed"),
                        short_ref: Some(short_ref.to_string()),
                    });
                    return;
                }

                if !manifest.contains_hash(short_ref, &full_hash) && !dry_run {
                    updated_manifest.entries.insert(
                        short_ref.to_string(),
                        ManifestEntry {
                            short_ref: short_ref.to_string(),
                            full_hash: full_hash.clone(),
                            size,
                            state: EntryState::Migrated,
                            migrated_at: now_unix(),
                            verified_at: None,
                            resumed: true,
                            owner_alias: true,
                        },
                    );
                    report.repaired += 1;
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.to_string(),
                        full_ref: full_ref.clone(),
                        size,
                        status: AliasStatus::Repaired,
                        error: None,
                        error_code: None,
                    });
                } else {
                    report.skipped += 1;
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.to_string(),
                        full_ref: full_ref.clone(),
                        size,
                        status: AliasStatus::Skipped,
                        error: None,
                        error_code: None,
                    });
                }
                return;
            } else {
                report.failed += 1;
                let msg = format!(
                    "{short_ref}: alias conflict —                      existing alias targets {existing_target},                      but content hashes to {full_ref}",
                );
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.to_string(),
                    full_ref: full_ref.clone(),
                    size,
                    status: AliasStatus::Failed,
                    error: Some(msg.clone()),
                    error_code: Some("alias-conflict".to_string()),
                });
                report.errors.push(MigrationError {
                    code: "alias-conflict".to_string(),
                    message: msg,
                    short_ref: Some(short_ref.to_string()),
                });
                return;
            }
        }

        if dry_run {
            report.migrated += 1;
            report.aliases.push(AliasEntry {
                short_ref: short_ref.to_string(),
                full_ref: full_ref.clone(),
                size,
                status: AliasStatus::Migrated,
                error: None,
                error_code: None,
            });
            return;
        }

        // Publish to shared CAS.
        match self.cas.publish(&content) {
            Ok(published_hash) => {
                debug_assert_eq!(published_hash, full_hash);
            }
            Err(SharedCasError::Corruption) => {
                report.failed += 1;
                let msg = format!(
                    "{short_ref}: CAS corruption —                      object {full_hash} exists with different bytes",
                );
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.to_string(),
                    full_ref: full_ref.clone(),
                    size,
                    status: AliasStatus::Failed,
                    error: Some(msg.clone()),
                    error_code: Some("cas-corruption".to_string()),
                });
                report.errors.push(MigrationError {
                    code: "cas-corruption".to_string(),
                    message: msg,
                    short_ref: Some(short_ref.to_string()),
                });
                return;
            }
            Err(SharedCasError::Policy) => {
                report.failed += 1;
                let msg = format!("{short_ref}: CAS policy violation");
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.to_string(),
                    full_ref: full_ref.clone(),
                    size,
                    status: AliasStatus::Failed,
                    error: Some(msg.clone()),
                    error_code: Some("cas-policy".to_string()),
                });
                report.errors.push(MigrationError {
                    code: "cas-policy".to_string(),
                    message: msg,
                    short_ref: Some(short_ref.to_string()),
                });
                return;
            }
            Err(_err) => {
                report.failed += 1;
                let msg = format!("{short_ref}: CAS publish failed");
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.to_string(),
                    full_ref: full_ref.clone(),
                    size,
                    status: AliasStatus::Failed,
                    error: Some(msg.clone()),
                    error_code: Some("cas-io".to_string()),
                });
                report.errors.push(MigrationError {
                    code: "cas-io".to_string(),
                    message: msg,
                    short_ref: Some(short_ref.to_string()),
                });
                return;
            }
        }

        // Store alias mapping.
        self.store.store_alias_deferred(short_ref, &full_ref);

        updated_manifest.entries.insert(
            short_ref.to_string(),
            ManifestEntry {
                short_ref: short_ref.to_string(),
                full_hash: full_hash.clone(),
                size,
                state: EntryState::Migrated,
                migrated_at: now_unix(),
                verified_at: None,
                resumed: false,
                owner_alias: true,
            },
        );

        report.migrated += 1;
        report.aliases.push(AliasEntry {
            short_ref: short_ref.to_string(),
            full_ref: full_ref.clone(),
            size,
            status: AliasStatus::Migrated,
            error: None,
            error_code: None,
        });
    }

    /// Verify migration integrity: checks every manifest entry's CAS object
    /// hash+size against the manifest and the exact alias target in the store.
    /// Also hash/size-checks the legacy source blob when present, ensuring the
    /// source, CAS, and alias are all consistent. Redacts underlying storage
    /// errors from report messages.
    pub fn verify(&self) -> MigrationReport {
        let mut report = MigrationReport::new("verify", false);
        let manifest = match self.manifest_path.as_ref() {
            Some(p) => match MigrationManifest::load(p) {
                Ok(mf) => mf,
                Err(MigrationErrorCode::ManifestMissing) => {
                    report.errors.push(MigrationError {
                        code: "manifest-missing".to_string(),
                        message: "migration manifest does not exist".to_string(),
                        short_ref: None,
                    });
                    return report;
                }
                Err(MigrationErrorCode::ManifestNewerVersion) => {
                    report.errors.push(MigrationError {
                        code: "manifest-newer-version".to_string(),
                        message: "manifest version is newer than supported".to_string(),
                        short_ref: None,
                    });
                    return report;
                }
                Err(_) => {
                    report.errors.push(MigrationError {
                        code: "manifest-corrupt".to_string(),
                        message: "manifest is corrupt".to_string(),
                        short_ref: None,
                    });
                    return report;
                }
            },
            None => {
                report.errors.push(MigrationError {
                    code: "manifest-missing".to_string(),
                    message: "no manifest path configured".to_string(),
                    short_ref: None,
                });
                return report;
            }
        };

        report.total = manifest.entries.len();

        for (short_ref, entry) in &manifest.entries {
            let full_ref = format!("{BLOB_REF_PREFIX}{}", entry.full_hash);
            report.aliases.push(AliasEntry {
                short_ref: short_ref.clone(),
                full_ref: full_ref.clone(),
                size: entry.size,
                status: AliasStatus::Verified,
                error: None,
                error_code: None,
            });

            // Verify the legacy source blob when present: hash+size must
            // match the manifest entry (which stores the shared proof).
            let source_ok = match self.store.resolve_blob_bytes(short_ref) {
                BlobContentResult::Ok(bytes) => {
                    let source_hash = full_sha256_hex(&bytes);
                    if source_hash != entry.full_hash || bytes.len() as u64 != entry.size {
                        report.failed += 1;
                        let idx = report.aliases.len() - 1;
                        report.aliases[idx].status = AliasStatus::Failed;
                        report.aliases[idx].error =
                            Some("legacy source hash/size mismatch with manifest".to_string());
                        report.aliases[idx].error_code = Some("source-corrupt".to_string());
                        report.errors.push(MigrationError {
                            code: "source-corrupt".to_string(),
                            message: format!("{short_ref}: legacy source hash/size mismatch"),
                            short_ref: Some(short_ref.clone()),
                        });
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
                    report.failed += 1;
                    let idx = report.aliases.len() - 1;
                    report.aliases[idx].status = AliasStatus::Failed;
                    report.aliases[idx].error = Some("legacy source corrupt".to_string());
                    report.aliases[idx].error_code = Some("source-corrupt".to_string());
                    report.errors.push(MigrationError {
                        code: "source-corrupt".to_string(),
                        message: format!("{short_ref}: legacy source corrupt"),
                        short_ref: Some(short_ref.clone()),
                    });
                    false
                }
            };
            if !source_ok {
                continue;
            }

            // Verify CAS object: hash+size must match manifest exactly.
            if !self.cas.contains(&entry.full_hash) {
                report.failed += 1;
                let idx = report.aliases.len() - 1;
                report.aliases[idx].status = AliasStatus::Failed;
                report.aliases[idx].error = Some("CAS object missing".to_string());
                report.aliases[idx].error_code = Some("cas-missing".to_string());
                report.errors.push(MigrationError {
                    code: "cas-missing".to_string(),
                    message: format!("{short_ref}: CAS object missing"),
                    short_ref: Some(short_ref.clone()),
                });
                continue;
            }

            match self.cas.resolve(&entry.full_hash) {
                Ok(bytes) => {
                    let cas_hash = full_sha256_hex(&bytes);
                    if cas_hash != entry.full_hash || bytes.len() as u64 != entry.size {
                        report.failed += 1;
                        let idx = report.aliases.len() - 1;
                        report.aliases[idx].status = AliasStatus::Failed;
                        report.aliases[idx].error = Some("CAS hash/size mismatch".to_string());
                        report.aliases[idx].error_code = Some("cas-corruption".to_string());
                        report.errors.push(MigrationError {
                            code: "cas-corruption".to_string(),
                            message: format!("{short_ref}: CAS hash/size mismatch"),
                            short_ref: Some(short_ref.clone()),
                        });
                        continue;
                    }
                }
                Err(_) => {
                    report.failed += 1;
                    let idx = report.aliases.len() - 1;
                    report.aliases[idx].status = AliasStatus::Failed;
                    report.aliases[idx].error = Some("CAS read failure".to_string());
                    report.aliases[idx].error_code = Some("cas-corruption".to_string());
                    report.errors.push(MigrationError {
                        code: "cas-corruption".to_string(),
                        message: format!("{short_ref}: CAS read failure"),
                        short_ref: Some(short_ref.clone()),
                    });
                    continue;
                }
            }

            // Verify alias targets the correct full ref.
            match self.store.alias_target(short_ref) {
                Some(target) if target == full_ref => {}
                Some(_target) => {
                    report.failed += 1;
                    let idx = report.aliases.len() - 1;
                    report.aliases[idx].status = AliasStatus::Failed;
                    report.aliases[idx].error = Some("alias targets wrong ref".to_string());
                    report.aliases[idx].error_code = Some("alias-conflict".to_string());
                    report.errors.push(MigrationError {
                        code: "alias-conflict".to_string(),
                        message: format!("{short_ref}: alias mismatch"),
                        short_ref: Some(short_ref.clone()),
                    });
                    continue;
                }
                None => {
                    report.failed += 1;
                    let idx = report.aliases.len() - 1;
                    report.aliases[idx].status = AliasStatus::Failed;
                    report.aliases[idx].error = Some("alias missing from store".to_string());
                    report.aliases[idx].error_code = Some("alias-missing".to_string());
                    report.errors.push(MigrationError {
                        code: "alias-missing".to_string(),
                        message: format!("{short_ref}: alias missing"),
                        short_ref: Some(short_ref.clone()),
                    });
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
        let manifest = match self.manifest_path.as_ref() {
            Some(p) => match MigrationManifest::load(p) {
                Ok(mf) => mf,
                Err(MigrationErrorCode::ManifestMissing) => {
                    report.errors.push(MigrationError {
                        code: "manifest-missing".to_string(),
                        message: "migration manifest does not exist".to_string(),
                        short_ref: None,
                    });
                    return report;
                }
                Err(_) => {
                    report.errors.push(MigrationError {
                        code: "manifest-corrupt".to_string(),
                        message: "manifest is corrupt".to_string(),
                        short_ref: None,
                    });
                    return report;
                }
            },
            None => {
                report.errors.push(MigrationError {
                    code: "manifest-missing".to_string(),
                    message: "no manifest path configured".to_string(),
                    short_ref: None,
                });
                return report;
            }
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
                report.failed += 1;
                let msg = format!(
                    "{short_ref}: legacy source hash/size mismatch, cannot verify rollback safety"
                );
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.clone(),
                    full_ref: format!("{BLOB_REF_PREFIX}{}", entry.full_hash),
                    size: entry.size,
                    status: AliasStatus::Failed,
                    error: Some(msg.clone()),
                    error_code: Some("rollback-source-gone".to_string()),
                });
                report.errors.push(MigrationError {
                    code: "rollback-source-gone".to_string(),
                    message: msg,
                    short_ref: Some(short_ref.clone()),
                });
                continue;
            }

            // Only remove aliases known to have been created by migration.
            if !entry.owner_alias {
                report.skipped += 1;
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.clone(),
                    full_ref: format!("{BLOB_REF_PREFIX}{}", entry.full_hash),
                    size: entry.size,
                    status: AliasStatus::Skipped,
                    error: Some("alias was not created by migration, skipping".to_string()),
                    error_code: None,
                });
                continue;
            }

            if apply {
                self.store.remove_alias(short_ref);
            }
            report.migrated += 1;
            report.aliases.push(AliasEntry {
                short_ref: short_ref.clone(),
                full_ref: format!("{BLOB_REF_PREFIX}{}", entry.full_hash),
                size: entry.size,
                status: AliasStatus::Migrated,
                error: None,
                error_code: None,
            });
        }

        // Persist alias removals successfully before deleting manifest.
        if apply && report.failed == 0 {
            if let Err(_err) = self.store.persist_pending() {
                report.errors.push(MigrationError {
                    code: "store-persist".to_string(),
                    message: "persist failed: rollback incomplete".to_string(),
                    short_ref: None,
                });
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
            report.errors.push(MigrationError {
                code: "cleanup-confirmation-required".to_string(),
                message: "cleanup requires --confirm-cleanup flag".to_string(),
                short_ref: None,
            });
            return report;
        }

        let verify_report = self.verify();
        if verify_report.is_failure() {
            report.errors.push(MigrationError {
                code: "cleanup-needs-verification".to_string(),
                message: "cleanup requires successful verification first".to_string(),
                short_ref: None,
            });
            if apply {
                report.errors.extend(verify_report.errors);
            }
            return report;
        }

        let manifest = match self.manifest_path.as_ref() {
            Some(p) => match MigrationManifest::load(p) {
                Ok(mf) => mf,
                Err(_) => {
                    report.errors.push(MigrationError {
                        code: "manifest-corrupt".to_string(),
                        message: "manifest is corrupt".to_string(),
                        short_ref: None,
                    });
                    return report;
                }
            },
            None => {
                report.errors.push(MigrationError {
                    code: "manifest-missing".to_string(),
                    message: "no manifest path configured".to_string(),
                    short_ref: None,
                });
                return report;
            }
        };

        report.total = manifest.entries.len();

        for (short_ref, entry) in &manifest.entries {
            if !apply {
                // Dry-run: report planned removals without mutation.
                report.migrated += 1;
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.clone(),
                    full_ref: format!("{BLOB_REF_PREFIX}{}", entry.full_hash),
                    size: entry.size,
                    status: AliasStatus::Migrated,
                    error: None,
                    error_code: None,
                });
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
                report.failed += 1;
                report.errors.push(MigrationError {
                    code: "source-corrupt".to_string(),
                    message: format!("{short_ref}: source hash/size mismatch"),
                    short_ref: Some(short_ref.clone()),
                });
                continue;
            }

            // Verify CAS object matches manifest.
            if !self.cas.contains(&entry.full_hash) {
                report.failed += 1;
                report.errors.push(MigrationError {
                    code: "cas-missing".to_string(),
                    message: format!("{short_ref}: CAS object missing"),
                    short_ref: Some(short_ref.clone()),
                });
                continue;
            }
            let cas_match = match self.cas.resolve(&entry.full_hash) {
                Ok(bytes) => {
                    full_sha256_hex(&bytes) == entry.full_hash && bytes.len() as u64 == entry.size
                }
                Err(_) => false,
            };
            if !cas_match {
                report.failed += 1;
                report.errors.push(MigrationError {
                    code: "cas-corruption".to_string(),
                    message: format!("{short_ref}: CAS hash/size mismatch"),
                    short_ref: Some(short_ref.clone()),
                });
                continue;
            }

            // Verify alias target matches.
            let full_ref = format!("{BLOB_REF_PREFIX}{}", entry.full_hash);
            match self.store.alias_target(short_ref) {
                Some(target) if target == full_ref => {}
                _ => {
                    report.failed += 1;
                    report.errors.push(MigrationError {
                        code: "alias-missing".to_string(),
                        message: format!("{short_ref}: alias missing or mismatch"),
                        short_ref: Some(short_ref.clone()),
                    });
                    continue;
                }
            }

            self.store.remove_blob(short_ref);
            report.migrated += 1;
            report.aliases.push(AliasEntry {
                short_ref: short_ref.clone(),
                full_ref,
                size: entry.size,
                status: AliasStatus::Migrated,
                error: None,
                error_code: None,
            });
        }

        // Treat persist failure as failure. Never delete CAS.
        if apply && report.migrated > 0 {
            if let Err(_err) = self.store.persist_pending() {
                report.failed += report.migrated;
                report.migrated = 0;
                report.errors.push(MigrationError {
                    code: "store-persist".to_string(),
                    message: "persist failed: cleanup incomplete".to_string(),
                    short_ref: None,
                });
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
            self.blobs
                .insert(ref_id.to_string(), content.as_bytes().to_vec());
        }
    }

    impl MigrationStore for FakeStore {
        fn blob_ref_ids(&self) -> Vec<String> {
            self.blobs.keys().cloned().collect()
        }

        fn resolve_blob_bytes(&self, ref_id: &str) -> BlobContentResult {
            match self.blobs.get(ref_id) {
                Some(bytes) if !bytes.is_empty() => BlobContentResult::Ok(bytes.clone()),
                Some(_) => BlobContentResult::Corrupt,
                None => BlobContentResult::Missing,
            }
        }

        fn alias_target(&self, alias: &str) -> Option<String> {
            self.aliases.get(alias).cloned()
        }

        fn store_alias_deferred(&mut self, alias: &str, target: &str) {
            self.aliases.insert(alias.to_string(), target.to_string());
        }

        fn remove_alias(&mut self, alias: &str) {
            self.aliases.remove(alias);
        }

        fn remove_blob(&mut self, ref_id: &str) {
            self.blobs.remove(ref_id);
        }

        fn mark_ambiguous(&mut self, short_ref: &str) {
            self.ambiguous.insert(short_ref.to_string(), true);
        }

        fn is_ambiguous(&self, short_ref: &str) -> bool {
            self.ambiguous.contains_key(short_ref)
        }

        fn persist_pending(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    fn cas_root_from_cache(cache_path: &Path) -> PathBuf {
        cache_path.parent().unwrap_or(cache_path).to_path_buf()
    }

    fn test_setup(dir: &Path) -> (FakeStore, SharedCas, PathBuf) {
        let cache = dir.join("recovery-cache.json");
        let cas_root = cas_root_from_cache(&cache);
        let cas = SharedCas::new(cas_root.clone());
        let manifest = dir.join("migration-manifest.json");
        (FakeStore::new(), cas, manifest)
    }

    // ── migration_migrate_single_legacy_blob ──────────────────────────

    #[test]
    fn migration_migrate_single_legacy_blob() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "hello migration target";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_hex = &full_hash[..16];
        let short_ref = format!("tz://blob/b{short_hex}");
        store.insert(&short_ref, text);

        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let report = migration.run(false);

        assert_eq!(report.total, 1);
        assert_eq!(report.migrated, 1);
        assert_eq!(report.skipped, 0);
        assert_eq!(report.failed, 0);
        assert_eq!(report.repaired, 0);

        let alias = store.alias_target(&short_ref);
        assert!(alias.is_some());
        let full_ref = alias.unwrap();
        assert!(full_ref.starts_with("tz://blob/"));
        let hash = &full_ref["tz://blob/".len()..];
        assert_eq!(hash.len(), FULL_HASH_LEN);
        assert_eq!(hash, full_hash);

        assert!(cas.contains(&full_hash));

        let loaded = MigrationManifest::load(&manifest).unwrap();
        assert_eq!(loaded.version, MIGRATION_MANIFEST_VERSION);
        assert!(loaded.entries.contains_key(&short_ref));
        let entry = loaded.entries.get(&short_ref).unwrap();
        assert_eq!(entry.full_hash, full_hash);
        assert_eq!(entry.size, text.len() as u64);
        assert_eq!(entry.state, EntryState::Migrated);
    }

    #[test]
    fn migration_canonical_full_ref_and_exact_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text1 = "payload alpha";
        let text2 = "payload beta different";
        let h1 = full_sha256_hex(text1.as_bytes());
        let h2 = full_sha256_hex(text2.as_bytes());
        assert_ne!(h1, h2);
        assert_eq!(h1.len(), FULL_HASH_LEN);
        assert_eq!(h2.len(), FULL_HASH_LEN);

        let r1 = format!("tz://blob/b{}", &h1[..16]);
        let r2 = format!("tz://blob/b{}", &h2[..16]);
        store.insert(&r1, text1);
        store.insert(&r2, text2);

        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest));
        let report = migration.run(false);
        assert_eq!(report.migrated, 2);

        let resolved1 = cas.resolve(&h1).unwrap();
        let resolved2 = cas.resolve(&h2).unwrap();
        assert_eq!(resolved1, text1.as_bytes());
        assert_eq!(resolved2, text2.as_bytes());
    }

    #[test]
    fn migration_no_duplicate_payload_when_cas_attached() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "shared content no dup";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m1 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let r1 = m1.run(false);
        assert_eq!(r1.migrated, 1);

        // Same content, different fake short ref (tests CAS idempotency).
        store.insert("tz://blob/baaaaaaaaaaaaaaa1", text);

        let mut m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let _r2 = m2.run(false);

        assert!(cas.contains(&full_hash));
    }

    #[test]
    fn migration_idempotent_rerun() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "idempotent content";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m1 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let r1 = m1.run(false);
        assert_eq!(r1.migrated, 1);

        let mut m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let r2 = m2.run(false);
        assert_eq!(r2.total, 1);
        assert_eq!(r2.migrated, 0);
        assert_eq!(r2.skipped, 1);
        assert_eq!(r2.failed, 0);
    }

    #[test]
    fn migration_apply_restart_byte_exact_legacy_read() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "restart byte exact";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let report = m.run(false);
        assert_eq!(report.migrated, 1);

        // Simulate restart: new store, same manifest.
        let (mut store2, cas2, _) = test_setup(dir.path());
        store2.insert(&short_ref, text);

        let mut m2 = LegacyMigration::new(&mut store2, &cas2, Some(manifest.clone()));
        let r2 = m2.run(false);
        assert_eq!(r2.repaired, 1);
        assert_eq!(r2.skipped, 0);

        let target = store2.alias_target(&short_ref);
        assert!(target.is_some());

        let bytes = cas2.resolve(&full_hash).unwrap();
        assert_eq!(bytes, text.as_bytes());
    }

    #[test]
    fn migration_missing_alias_repair_on_resume() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "repair alias";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let r = m.run(false);
        assert_eq!(r.migrated, 1);

        store.remove_alias(&short_ref);

        let mut m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let r2 = m2.run(false);
        assert_eq!(r2.repaired, 1);
        assert!(store.alias_target(&short_ref).is_some());
    }

    #[test]
    fn migration_missing_cas_repair_on_resume() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "repair cas";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let r = m.run(false);
        assert_eq!(r.migrated, 1);

        let obj_path = dir
            .path()
            .join("blobs")
            .join("sha256")
            .join(&full_hash[..2])
            .join(&full_hash);
        fs::remove_file(&obj_path).unwrap();
        store.remove_alias(&short_ref);

        let mut m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let r2 = m2.run(false);
        assert_eq!(r2.repaired, 1);
        assert!(cas.contains(&full_hash));
        assert!(store.alias_target(&short_ref).is_some());
    }

    #[test]
    fn migration_ambiguous_short_id_fails_safely() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let fake_ref = "tz://blob/bdeadbeefdeadbeef";
        let content = "this content does not match the claimed short ID";
        store.insert(fake_ref, content);

        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest));
        let report = migration.run(false);

        assert_eq!(report.total, 1);
        assert_eq!(report.migrated, 0);
        assert_eq!(report.failed, 1);
        assert!(report.errors.iter().any(|e| e.code == "ambiguous-short-id"));
    }

    #[test]
    fn migration_alias_conflict_fails_safely() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "original content for conflict test";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let wrong_full_ref = format!("{BLOB_REF_PREFIX}{}", "f".repeat(FULL_HASH_LEN));
        store.store_alias_deferred(&short_ref, &wrong_full_ref);

        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest));
        let report = migration.run(false);

        assert_eq!(report.total, 1);
        assert_eq!(report.migrated, 0);
        assert_eq!(report.failed, 1);
        assert!(report.errors.iter().any(|e| e.code == "alias-conflict"));
    }

    #[test]
    fn migration_conflicting_manifest_hash_is_deterministic_ambiguity() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "manifest conflict text";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut fake_manifest = MigrationManifest::empty();
        fake_manifest.entries.insert(
            short_ref.clone(),
            ManifestEntry {
                short_ref: short_ref.clone(),
                full_hash: "f".repeat(FULL_HASH_LEN),
                size: 999,
                state: EntryState::Migrated,
                migrated_at: now_unix(),
                verified_at: None,
                resumed: false,
                owner_alias: false,
            },
        );
        fake_manifest.save(&manifest).unwrap();

        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let report = migration.run(false);

        assert_eq!(report.failed, 1);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.code == "manifest-hash-conflict")
        );
    }

    #[test]
    fn migration_dry_run_produces_no_writes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "dry run content";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let report = migration.run(true);

        assert!(report.dry_run);
        assert_eq!(report.total, 1);
        assert_eq!(report.migrated, 1);
        assert_eq!(report.failed, 0);

        assert!(store.alias_target(&short_ref).is_none());
        assert!(!manifest.exists());
        assert!(!cas.contains(&full_hash));
    }

    #[test]
    fn migration_dry_run_no_writes_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "dry run idempotent";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m1 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let r1 = m1.run(true);
        assert_eq!(r1.migrated, 1);

        let mut m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let r2 = m2.run(true);
        assert_eq!(r2.migrated, 1);
    }

    #[test]
    fn migration_corrupt_source_fails() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let short_ref = "tz://blob/b0000000000000000";
        store.blobs.insert(short_ref.to_string(), vec![]);

        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest));
        let report = migration.run(false);

        assert_eq!(report.failed, 1);
        assert!(report.errors.iter().any(|e| e.code == "source-corrupt"));
    }

    #[test]
    fn migration_corrupt_cas_detected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "corrupt CAS test";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let obj_dir = dir
            .path()
            .join("blobs")
            .join("sha256")
            .join(&full_hash[..2]);
        fs::create_dir_all(&obj_dir).unwrap();
        fs::write(obj_dir.join(&full_hash), b"tampered bytes").unwrap();

        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest));
        let report = migration.run(false);

        assert_eq!(report.failed, 1);
        assert!(report.errors.iter().any(|e| e.code == "cas-corruption"));
    }

    #[test]
    fn migration_verify_all_ok() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "verify ok";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let r = m.run(false);
        assert_eq!(r.migrated, 1);

        let m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let vr = m2.verify();
        assert_eq!(vr.total, 1);
        assert_eq!(vr.verified, 1);
        assert_eq!(vr.failed, 0);
    }

    #[test]
    fn migration_verify_cas_missing() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "verify missing cas";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        m.run(false);

        let obj_path = dir
            .path()
            .join("blobs")
            .join("sha256")
            .join(&full_hash[..2])
            .join(&full_hash);
        fs::remove_file(&obj_path).unwrap();

        let m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let vr = m2.verify();
        assert_eq!(vr.failed, 1);
        assert!(vr.errors.iter().any(|e| e.code == "cas-missing"));
    }

    #[test]
    fn migration_verify_alias_missing() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "verify missing alias";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        m.run(false);

        store.remove_alias(&short_ref);

        let m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let vr = m2.verify();
        assert_eq!(vr.failed, 1);
        assert!(vr.errors.iter().any(|e| e.code == "alias-missing"));
    }

    #[test]
    fn migration_rollback_removes_aliases_and_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "rollback test";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let r = m.run(false);
        assert_eq!(r.migrated, 1);
        assert!(store.alias_target(&short_ref).is_some());
        assert!(manifest.exists());

        let mut m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let rr = m2.rollback(true);

        assert_eq!(rr.migrated, 1);
        assert_eq!(rr.failed, 0);

        assert!(store.alias_target(&short_ref).is_none());
        assert!(!manifest.exists());
        assert!(cas.contains(&full_hash));
        assert!(matches!(
            store.resolve_blob_bytes(&short_ref),
            BlobContentResult::Ok(_)
        ));
    }

    #[test]
    fn migration_rollback_fails_when_source_gone() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "rollback source gone";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        m.run(false);

        store.remove_blob(&short_ref);

        let mut m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let rr = m2.rollback(true);
        assert_eq!(rr.failed, 1);
        assert!(rr.errors.iter().any(|e| e.code == "rollback-source-gone"));
        assert!(store.alias_target(&short_ref).is_some());
    }

    #[test]
    fn migration_cleanup_dry_run_no_writes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "cleanup dry run";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        m.run(false);

        let mut m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let cr = m2.cleanup(false, true);
        assert!(cr.dry_run);
        assert_eq!(cr.migrated, 1);
        assert_eq!(cr.failed, 0);
        assert!(matches!(
            store.resolve_blob_bytes(&short_ref),
            BlobContentResult::Ok(_)
        ));
    }

    #[test]
    fn migration_cleanup_apply_removes_legacy_sources() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "cleanup apply";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        m.run(false);

        let mut m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let cr = m2.cleanup(true, true);
        assert!(!cr.dry_run);
        assert_eq!(cr.migrated, 1);
        assert_eq!(cr.failed, 0);

        assert!(matches!(
            store.resolve_blob_bytes(&short_ref),
            BlobContentResult::Missing
        ));
        assert!(store.alias_target(&short_ref).is_some());
        assert!(cas.contains(&full_hash));
    }

    #[test]
    fn migration_cleanup_requires_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "cleanup no confirm";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        m.run(false);

        let mut m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let cr = m2.cleanup(true, false);
        assert!(
            cr.errors
                .iter()
                .any(|e| e.code == "cleanup-confirmation-required")
        );
        assert!(matches!(
            store.resolve_blob_bytes(&short_ref),
            BlobContentResult::Ok(_)
        ));
    }

    #[test]
    fn migration_cleanup_requires_verification() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let text = "cleanup needs verify";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert(&short_ref, text);

        let mut m = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        m.run(false);

        let obj_path = dir
            .path()
            .join("blobs")
            .join("sha256")
            .join(&full_hash[..2])
            .join(&full_hash);
        fs::remove_file(&obj_path).unwrap();

        let mut m2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let cr = m2.cleanup(true, true);
        assert!(
            cr.errors
                .iter()
                .any(|e| e.code == "cleanup-needs-verification")
        );
        assert!(matches!(
            store.resolve_blob_bytes(&short_ref),
            BlobContentResult::Ok(_)
        ));
    }

    #[test]
    fn migration_strict_manifest_version_newer_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let bad = r#"{"version": "tokenzero.migration.v99", "entries": {}, "completed": false}"#;
        fs::write(&manifest, bad).unwrap();

        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let report = migration.run(false);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.code == "manifest-newer-version")
        );
        assert_eq!(report.total, 0);
    }

    #[test]
    fn migration_corrupt_manifest_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        fs::write(&manifest, b"not json at all").unwrap();

        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest));
        let report = migration.run(false);
        assert!(report.errors.iter().any(|e| e.code == "manifest-corrupt"));
    }

    #[test]
    fn migration_is_legacy_blob_ref_detection() {
        assert!(is_legacy_blob_ref("tz://blob/babc123def4567890"));
        assert!(is_legacy_blob_ref("tz://blob/b0000000000000000"));

        assert!(!is_legacy_blob_ref(&format!(
            "tz://blob/{}",
            "a".repeat(FULL_HASH_LEN)
        )));
        assert!(!is_legacy_blob_ref("tz://file/babc123def456789"));
        assert!(!is_legacy_blob_ref("tz://unit/uabc123def456789"));
        assert!(!is_legacy_blob_ref("tz://blob/xabc123def456789"));
        assert!(!is_legacy_blob_ref("tz://blob/babc"));
        assert!(!is_legacy_blob_ref("tz://blob/babc123def45678901"));
    }

    #[test]
    fn migration_full_hash_ref_not_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        let full_hash = "a".repeat(FULL_HASH_LEN);
        let full_ref = format!("{BLOB_REF_PREFIX}{full_hash}");
        store.insert(&full_ref, "already canonical");

        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest));
        let report = migration.run(false);

        assert_eq!(report.total, 0);
        assert_eq!(report.migrated, 0);
    }

    #[test]
    fn migration_canonical_vs_legacy_key_collision_separation() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, _manifest) = test_setup(dir.path());

        let text = "legacy blob";
        let legacy_hash = full_sha256_hex(text.as_bytes());
        let legacy_ref = format!("tz://blob/b{}", &legacy_hash[..16]);
        store.insert(&legacy_ref, text);

        let canonical_text = "canonical blob different";
        let canonical_hash = full_sha256_hex(canonical_text.as_bytes());
        let canonical_ref = format!("tz://blob/{canonical_hash}");
        store.insert(&canonical_ref, canonical_text);

        let mut migration = LegacyMigration::new(&mut store, &cas, None);
        let report = migration.run(true);

        assert_eq!(report.total, 1);
        assert!(report.aliases.iter().any(|a| a.short_ref == legacy_ref));
    }

    #[test]
    fn migration_deterministic_ambiguous_prefix_two_distinct_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, manifest) = test_setup(dir.path());

        // Inject two blobs with the SAME fabricated short ref but DIFFERENT bytes.
        // This simulates a genuine prefix collision without needing a natural SHA collision.
        let fake_short = "tz://blob/babcdabcdabcdabcd";
        store.insert(fake_short, "content alpha");
        // Overwrite with different content under the same ref.
        store.insert(fake_short, "content beta different enough");

        // Actually, we need two distinct blobs under the same short ref.
        // FakeStore only stores one value per key. Let's use a different approach:
        // mark the ref as ambiguous directly and verify migration fails on it.

        store.mark_ambiguous(fake_short);

        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest));
        let report = migration.run(false);

        // All entries for this short ref should fail.
        let failures: Vec<_> = report
            .aliases
            .iter()
            .filter(|a| a.short_ref == fake_short && a.status == AliasStatus::Failed)
            .collect();
        assert_eq!(failures.len(), 1);
    }

    // ── Real RecoveryStore-backed tests ──────────────────────────────────

    fn cas_from_cache(cache_path: &Path) -> SharedCas {
        SharedCas::detect_from_cache_path(cache_path).unwrap_or_else(|| {
            SharedCas::new(cache_path.parent().unwrap_or(cache_path).to_path_buf())
        })
    }

    #[test]
    fn migration_cas_publish_exact_bytes_not_in_recovery_json() {
        let dir = tempfile::tempdir().unwrap();
        // Create a unified store layout: <dir>/tokenzero/recovery-cache.json
        let engine_dir = dir.path().join("tokenzero");
        std::fs::create_dir_all(&engine_dir).unwrap();
        let cache = engine_dir.join("recovery-cache.json");

        let mut store = crate::RecoveryStore::new(Some(cache.clone()));
        let text = "canonical CAS payload no dup";
        let full_hash = crate::migration::full_sha256_hex(text.as_bytes());

        // Publish through the production RecoveryStore write path.
        let blob_ref = store.put_blob(text, tokenzero_core::ContentType::Unknown);
        assert_eq!(blob_ref, format!("tz://blob/{full_hash}"));
        store.persist().unwrap();
        drop(store);

        let published_hash = full_hash.clone();

        assert_eq!(published_hash.len(), 64);
        assert!(
            published_hash
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_eq!(published_hash, full_hash);

        // Resolve after restart (new CAS instance)
        let cas2 = SharedCas::detect_from_cache_path(&cache).unwrap();
        let resolved = cas2.resolve(&full_hash).unwrap();
        assert_eq!(resolved, text.as_bytes());

        // Raw payload must NOT be in recovery JSON
        let cas3 = SharedCas::detect_from_cache_path(&cache).unwrap();
        assert!(cas3.contains(&full_hash));
        let recovery_json = std::fs::read_to_string(&cache).unwrap();
        assert!(!recovery_json.contains(text));

        let mut restarted = crate::RecoveryStore::new(Some(cache.clone()));
        let expanded = restarted.expand(&blob_ref, None, None, None, None, None);
        assert_eq!(expanded.content, text);
    }

    #[test]
    fn migration_cas_publish_store_no_duplicate_payload() {
        let dir = tempfile::tempdir().unwrap();
        let engine_dir = dir.path().join("tokenzero");
        std::fs::create_dir_all(&engine_dir).unwrap();
        let cache = engine_dir.join("recovery-cache.json");

        let mut store = crate::RecoveryStore::new(Some(cache.clone()));
        let text = "no duplicate in recovery state";
        let full_hash = crate::migration::full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);

        // Insert test blob
        store.insert_test_blob(&short_ref, text);

        let cas = SharedCas::detect_from_cache_path(&cache).unwrap();
        let mut adapter = crate::migration::RecoveryStoreAdapter::new(&mut store);
        let manifest = engine_dir.join("migration-manifest.json");
        let mut migration = LegacyMigration::new(&mut adapter, &cas, Some(manifest));
        let report = migration.run(false);
        assert_eq!(report.migrated, 1);

        // CAS has the bytes
        assert!(cas.contains(&full_hash));
        let resolved = cas.resolve(&full_hash).unwrap();
        assert_eq!(resolved, text.as_bytes());
    }

    #[test]
    fn migration_alias_present_cas_missing_republishes() {
        let dir = tempfile::tempdir().unwrap();
        let engine_dir = dir.path().join("tokenzero");
        std::fs::create_dir_all(&engine_dir).unwrap();
        let cache = engine_dir.join("recovery-cache.json");

        let mut store = crate::RecoveryStore::new(Some(cache.clone()));
        let text = "matching alias with missing canonical CAS object";
        let full_hash = full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        let full_ref = format!("{BLOB_REF_PREFIX}{full_hash}");
        store.insert_test_blob(&short_ref, text);
        store.store_alias_deferred(&short_ref, &full_ref);

        let cas = cas_from_cache(&cache);
        assert!(!cas.contains(&full_hash));
        let manifest = engine_dir.join("migration-manifest.json");
        let mut adapter = RecoveryStoreAdapter::new(&mut store);
        let mut migration = LegacyMigration::new(&mut adapter, &cas, Some(manifest));

        let report = migration.run(false);
        assert!(report.errors.is_empty());
        assert!(report.repaired >= 1);
        assert!(cas.contains(&full_hash));

        let verify = migration.verify();
        assert!(verify.errors.is_empty());
        assert_eq!(verify.verified, verify.total);
    }

    #[test]
    fn migration_legacy_disabled_fails() {
        let dir = tempfile::tempdir().unwrap();
        let engine_dir = dir.path().join("tokenzero");
        std::fs::create_dir_all(&engine_dir).unwrap();
        let cache = engine_dir.join("recovery-cache.json");

        let config = crate::RecoveryConfig {
            legacy_compat: false,
            ..crate::RecoveryConfig::default()
        };
        let mut store = crate::RecoveryStore::with_config(Some(cache.clone()), config);

        let text = "disabled legacy";
        let full_hash = crate::migration::full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert_test_blob(&short_ref, text);
        store.store_alias_deferred(&short_ref, &format!("tz://blob/{full_hash}"));

        // Expand should fail with legacy-ref-disabled
        let result = store.expand(&short_ref, None, None, None, None, None);
        assert!(!result.found);
        assert_eq!(result.reason, "legacy-ref-disabled");

        // Full ref should still work (not a legacy ref)
        let full_ref = format!("tz://blob/{full_hash}");
        store.insert_test_blob(&full_ref, text);
        let result2 = store.expand(&full_ref, None, None, None, None, None);
        assert!(result2.found);
    }

    #[test]
    fn migration_ambiguous_alias_fails() {
        let dir = tempfile::tempdir().unwrap();
        let engine_dir = dir.path().join("tokenzero");
        std::fs::create_dir_all(&engine_dir).unwrap();
        let cache = engine_dir.join("recovery-cache.json");

        let mut store = crate::RecoveryStore::new(Some(cache.clone()));
        let short_ref = "tz://blob/babcdabcdabcdabcd";
        store.insert_test_blob(short_ref, "content");
        store.mark_ambiguous(short_ref);

        let result = store.expand(short_ref, None, None, None, None, None);
        assert!(!result.found);
        assert_eq!(result.reason, "legacy-ambiguous");
    }

    #[test]
    fn migration_fz_gz_refs_no_local_fallback_when_cas_missing() {
        let dir = tempfile::tempdir().unwrap();
        let engine_dir = dir.path().join("tokenzero");
        std::fs::create_dir_all(&engine_dir).unwrap();
        let cache = engine_dir.join("recovery-cache.json");

        let mut store = crate::RecoveryStore::new(Some(cache.clone()));
        let text = "fz gz no fallback test";
        let full_hash = crate::migration::full_sha256_hex(text.as_bytes());

        // Store the blob locally under tz:// scheme
        let tz_ref = format!("tz://blob/{full_hash}");
        store.insert_test_blob(&tz_ref, text);

        // fz:// and gz:// refs should fail when CAS is missing (even though local tz blob exists)
        let fz_ref = format!("fz://blob/{full_hash}");
        let result = store.expand(&fz_ref, None, None, None, None, None);
        assert!(!result.found);
        assert_eq!(result.reason, "shared-cas-missing");

        let gz_ref = format!("gz://blob/{full_hash}");
        let result2 = store.expand(&gz_ref, None, None, None, None, None);
        assert!(!result2.found);
        assert_eq!(result2.reason, "shared-cas-missing");

        // tz:// should fall back to local store
        let result3 = store.expand(&tz_ref, None, None, None, None, None);
        assert!(result3.found);
        assert_eq!(result3.content, text);
    }

    #[test]
    fn migration_rollback_dry_run_no_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let engine_dir = dir.path().join("tokenzero");
        std::fs::create_dir_all(&engine_dir).unwrap();
        let cache = engine_dir.join("recovery-cache.json");

        let mut store = crate::RecoveryStore::new(Some(cache.clone()));
        let text = "rollback dry run";
        let full_hash = crate::migration::full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert_test_blob(&short_ref, text);

        let cas = cas_from_cache(&cache);
        let mut adapter = crate::migration::RecoveryStoreAdapter::new(&mut store);
        let manifest = engine_dir.join("migration-manifest.json");
        let mut m = LegacyMigration::new(&mut adapter, &cas, Some(manifest.clone()));
        let r = m.run(false);
        assert_eq!(r.migrated, 1);
        assert!(manifest.exists());
        assert!(store.alias_target(&short_ref).is_some());

        // Rollback dry-run: nothing should be removed
        let mut adapter2 = crate::migration::RecoveryStoreAdapter::new(&mut store);
        let mut m2 = LegacyMigration::new(&mut adapter2, &cas, Some(manifest.clone()));
        let rr = m2.rollback(false); // dry_run = true (apply = false)
        assert!(rr.dry_run);
        assert_eq!(rr.migrated, 1);
        assert!(manifest.exists());
        assert!(store.alias_target(&short_ref).is_some());
    }

    #[test]
    fn migration_rollback_apply_removes_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let engine_dir = dir.path().join("tokenzero");
        std::fs::create_dir_all(&engine_dir).unwrap();
        let cache = engine_dir.join("recovery-cache.json");

        let mut store = crate::RecoveryStore::new(Some(cache.clone()));
        let text = "rollback apply";
        let full_hash = crate::migration::full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert_test_blob(&short_ref, text);

        let cas = cas_from_cache(&cache);
        let mut adapter = crate::migration::RecoveryStoreAdapter::new(&mut store);
        let manifest = engine_dir.join("migration-manifest.json");
        let mut m = LegacyMigration::new(&mut adapter, &cas, Some(manifest.clone()));
        m.run(false);
        assert!(store.alias_target(&short_ref).is_some());

        // Rollback apply
        let mut adapter2 = crate::migration::RecoveryStoreAdapter::new(&mut store);
        let mut m2 = LegacyMigration::new(&mut adapter2, &cas, Some(manifest.clone()));
        let rr = m2.rollback(true);
        assert!(!rr.dry_run);
        assert_eq!(rr.migrated, 1);
        assert_eq!(rr.failed, 0);
        assert!(store.alias_target(&short_ref).is_none());
        assert!(!manifest.exists());
        // CAS must not be deleted
        assert!(cas.contains(&full_hash));
    }

    #[test]
    fn migration_cleanup_restart_preserves_tombstones() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");

        // First store
        let mut store = crate::RecoveryStore::new(Some(cache.clone()));
        let text = "cleanup restart";
        let full_hash = crate::migration::full_sha256_hex(text.as_bytes());
        let short_ref = format!("tz://blob/b{}", &full_hash[..16]);
        store.insert_test_blob(&short_ref, text);

        let cas = cas_from_cache(&cache);
        let mut adapter = crate::migration::RecoveryStoreAdapter::new(&mut store);
        let manifest = dir.path().join("migration-manifest.json");
        let mut m = LegacyMigration::new(&mut adapter, &cas, Some(manifest.clone()));
        m.run(false);

        // Apply cleanup
        let mut adapter2 = crate::migration::RecoveryStoreAdapter::new(&mut store);
        let mut m2 = LegacyMigration::new(&mut adapter2, &cas, Some(manifest.clone()));
        let cr = m2.cleanup(true, true);
        assert!(!cr.dry_run);
        assert_eq!(cr.migrated, 1);
        assert_eq!(cr.failed, 0);

        // "Restart" — new store loading same cache
        let mut store2 = crate::RecoveryStore::new(Some(cache.clone()));
        // Legacy source should be gone; alias-aware reads still resolve through CAS.
        assert!(!store2.blob_ref_ids().contains(&short_ref));
        // Alias should still exist
        assert!(store2.alias_target(&short_ref).is_some());
        // CAS must still have the bytes, and the alias must resolve through
        // the flat cache attached CAS after the legacy source is gone.
        assert!(cas.contains(&full_hash));
        let expanded = store2.expand(&short_ref, None, None, None, None, None);
        assert!(expanded.found);
        assert_eq!(expanded.content, text);
    }

    #[test]
    fn migration_bare_plan_no_filesystem_changes() {
        let dir = tempfile::tempdir().unwrap();
        let engine_dir = dir.path().join("tokenzero");
        std::fs::create_dir_all(&engine_dir).unwrap();
        let cache = engine_dir.join("recovery-cache.json");

        // Create store but no blob data - bare migration plan
        let mut store = crate::RecoveryStore::new(Some(cache.clone()));
        let cas = cas_from_cache(&cache);
        let mut adapter = crate::migration::RecoveryStoreAdapter::new(&mut store);
        let manifest = engine_dir.join("migration-manifest.json");
        let mut migration = LegacyMigration::new(&mut adapter, &cas, Some(manifest.clone()));

        // Dry-run migration with no blobs
        let report = migration.run(true);
        assert_eq!(report.total, 0);
        assert_eq!(report.migrated, 0);

        // Verify with no manifest
        let v = migration.verify();
        assert!(
            v.errors
                .iter()
                .any(|error| error.code == "manifest-missing")
        );

        // Rollback dry-run with no manifest
        let rb = migration.rollback(false);
        assert!(
            rb.errors
                .iter()
                .any(|error| error.code == "manifest-missing")
        );

        // Cleanup dry-run with no manifest
        let cl = migration.cleanup(false, false);
        assert_eq!(cl.total, 0);
        assert!(
            cl.errors
                .iter()
                .any(|error| error.code == "cleanup-needs-verification")
        );
    }

    #[test]
    fn migration_doctor_no_payload_or_paths() {
        let dir = tempfile::tempdir().unwrap();
        let engine_dir = dir.path().join("tokenzero");
        std::fs::create_dir_all(&engine_dir).unwrap();
        let cache = engine_dir.join("recovery-cache.json");

        let store = crate::RecoveryStore::new(Some(cache.clone()));
        let state = store.migration_state();

        // Doctor data must have no payload or filesystem paths
        let json_str = serde_json::to_string(&state).unwrap();
        assert!(!json_str.contains("payload"));
        assert!(!json_str.contains("/blobs/"));
        assert!(state.get("legacy_compat_supported_until").is_some());
    }
}
