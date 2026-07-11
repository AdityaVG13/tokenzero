//! Legacy short-ref migration to full SHA-256 canonical refs.
//!
//! TokenZero's original `id_for(prefix, text)` generates 17-character short IDs
//! (prefix char + 16 hex from the first 8 SHA-256 bytes). The ZeroRef v1 shared
//! CAS uses the full 64-hex SHA-256 digest. This module migrates legacy
//! short-ID blobs to the canonical shared CAS, builds an alias index for
//! backward-compatible reads, and supports idempotent re-runs with a versioned
//! manifest.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::RecoveryStore;
use crate::shared_cas::{SharedCas, SharedCasError};

/// Manifest schema version. Bumped when the manifest format changes.
pub const MIGRATION_MANIFEST_VERSION: &str = "tokenzero.migration.v1";

/// Prefix for blob refs in the legacy store.
const BLOB_REF_PREFIX: &str = "tz://blob/";

/// Length of a legacy short ID: prefix char + 16 hex chars = 17.
const LEGACY_SHORT_ID_LEN: usize = 17;

/// Length of a full SHA-256 hex ID: 64 chars.
const FULL_HASH_LEN: usize = 64;

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
fn full_sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// One alias entry in the migration report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasEntry {
    pub short_ref: String,
    pub full_ref: String,
    pub status: AliasStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Status of an individual alias during migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AliasStatus {
    Migrated,
    Skipped,
    Failed,
}

/// Summary report returned by `LegacyMigration::run`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    pub manifest_version: String,
    pub dry_run: bool,
    pub total: usize,
    pub migrated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub aliases: Vec<AliasEntry>,
    pub errors: Vec<String>,
    pub timestamp: u64,
}

impl MigrationReport {
    fn new(dry_run: bool) -> Self {
        Self {
            manifest_version: MIGRATION_MANIFEST_VERSION.to_string(),
            dry_run,
            total: 0,
            migrated: 0,
            skipped: 0,
            failed: 0,
            aliases: Vec::new(),
            errors: Vec::new(),
            timestamp: now_unix(),
        }
    }

    /// Render as compact JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Render as human-readable text.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Legacy ref migration (dry_run={})\n\
             ─────────────────────────────────\n\
             total:    {}\n\
             migrated: {}\n\
             skipped:  {}\n\
             failed:   {}\n",
            self.dry_run, self.total, self.migrated, self.skipped, self.failed,
        ));
        if !self.aliases.is_empty() {
            out.push_str("\naliases:\n");
            for entry in &self.aliases {
                out.push_str(&format!(
                    "  {} → {}  [{}]\n",
                    entry.short_ref,
                    entry.full_ref,
                    match entry.status {
                        AliasStatus::Migrated => "migrated",
                        AliasStatus::Skipped => "skipped",
                        AliasStatus::Failed => "failed",
                    }
                ));
                if let Some(err) = &entry.error {
                    out.push_str(&format!("    error: {err}\n"));
                }
            }
        }
        if !self.errors.is_empty() {
            out.push_str("\nerrors:\n");
            for err in &self.errors {
                out.push_str(&format!("  - {err}\n"));
            }
        }
        out
    }
}

/// Versioned manifest tracking completed migrations for resume support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationManifest {
    pub version: String,
    pub entries: BTreeMap<String, ManifestEntry>,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub short_ref: String,
    pub full_hash: String,
    pub migrated_at: u64,
}

impl MigrationManifest {
    /// Load a manifest from `path`, or return an empty one if missing.
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| Self::empty()),
            Err(_) => Self::empty(),
        }
    }

    fn empty() -> Self {
        Self {
            version: MIGRATION_MANIFEST_VERSION.to_string(),
            entries: BTreeMap::new(),
            completed: false,
        }
    }

    /// Save the manifest to `path`.
    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Check whether a short ref has already been migrated to the given full hash.
    pub fn contains(&self, short_ref: &str, full_hash: &str) -> bool {
        self.entries
            .get(short_ref)
            .is_some_and(|e| e.full_hash == full_hash)
    }
}

/// Legacy short-ref migration engine.
///
/// Scans a [`RecoveryStore`] for legacy short-ID blobs, publishes each to the
/// [`SharedCas`], and stores an alias mapping (`short_ref → full_ref`) for
/// backward-compatible reads.
pub struct LegacyMigration<'a> {
    store: &'a mut RecoveryStore,
    cas: &'a SharedCas,
    manifest_path: Option<PathBuf>,
}

impl<'a> LegacyMigration<'a> {
    /// Create a new migration engine.
    ///
    /// `manifest_path` is where the versioned manifest is read/written for
    /// resume support. Pass `None` to skip manifest persistence (useful for
    /// tests).
    pub fn new(
        store: &'a mut RecoveryStore,
        cas: &'a SharedCas,
        manifest_path: Option<PathBuf>,
    ) -> Self {
        Self {
            store,
            cas,
            manifest_path,
        }
    }

    /// Run the migration.
    ///
    /// - `dry_run = true`: report only, no writes to CAS, store, or manifest.
    /// - `dry_run = false`: publish to CAS, store aliases, update manifest.
    ///
    /// The migration is idempotent: running twice produces the same result.
    /// Already-migrated entries (matching alias or manifest) are skipped.
    pub fn run(&mut self, dry_run: bool) -> MigrationReport {
        let mut report = MigrationReport::new(dry_run);
        let manifest = self
            .manifest_path
            .as_ref()
            .map(|p| MigrationManifest::load(p))
            .unwrap_or_else(MigrationManifest::empty);

        // Collect blob refs first (borrow doesn't conflict with later mutable borrows).
        let blob_refs = self.store.blob_ref_ids();
        let legacy_refs: Vec<String> = blob_refs
            .into_iter()
            .filter(|r| is_legacy_blob_ref(r))
            .collect();

        report.total = legacy_refs.len();

        let mut updated_manifest = manifest.clone();

        for short_ref in &legacy_refs {
            // Resolve blob content.
            let Some(content) = self.store.resolve_blob_content(short_ref) else {
                report.failed += 1;
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.clone(),
                    full_ref: String::new(),
                    status: AliasStatus::Failed,
                    error: Some("could not resolve blob content".to_string()),
                });
                report
                    .errors
                    .push(format!("{short_ref}: could not resolve blob content"));
                continue;
            };

            // Compute full SHA-256 of the complete legacy bytes.
            let full_hash = full_sha256_hex(content.as_bytes());
            let full_ref = format!("{BLOB_REF_PREFIX}{full_hash}");

            // Verify the short ID is a correct prefix of the full hash.
            // The 16-hex short ID (after 'b') should equal the first 16 hex
            // chars of the full 64-hex hash. If not, the short ID is
            // ambiguous — the content does not match its claimed identity.
            if let Some(short_hex) = short_id_hex(short_ref) {
                if &full_hash[..16] != short_hex {
                    report.failed += 1;
                    let msg = format!(
                        "{short_ref}: ambiguous short ID — \
                         short prefix {short_hex} does not match \
                         full hash prefix {}",
                        &full_hash[..16],
                    );
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.clone(),
                        full_ref,
                        status: AliasStatus::Failed,
                        error: Some(msg.clone()),
                    });
                    report.errors.push(msg);
                    continue;
                }
            }

            // Check for existing alias conflict.
            if let Some(existing_target) = self.store.alias_target(short_ref) {
                if existing_target == full_ref {
                    // Already migrated to the correct full ref — skip.
                    report.skipped += 1;
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.clone(),
                        full_ref,
                        status: AliasStatus::Skipped,
                        error: None,
                    });
                    continue;
                } else {
                    // Alias exists but points to a different full ref — conflict.
                    report.failed += 1;
                    let msg = format!(
                        "{short_ref}: alias conflict — \
                         existing alias targets {existing_target}, \
                         but content hashes to {full_ref}",
                    );
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.clone(),
                        full_ref,
                        status: AliasStatus::Failed,
                        error: Some(msg.clone()),
                    });
                    report.errors.push(msg);
                    continue;
                }
            }

            // Check manifest for already-migrated entry.
            if manifest.contains(short_ref, &full_hash) {
                report.skipped += 1;
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.clone(),
                    full_ref,
                    status: AliasStatus::Skipped,
                    error: None,
                });
                continue;
            }

            if dry_run {
                // Dry-run: report as migrated but do not write.
                report.migrated += 1;
                report.aliases.push(AliasEntry {
                    short_ref: short_ref.clone(),
                    full_ref,
                    status: AliasStatus::Migrated,
                    error: None,
                });
                continue;
            }

            // Publish to shared CAS.
            match self.cas.publish(content.as_bytes()) {
                Ok(published_hash) => {
                    debug_assert_eq!(published_hash, full_hash);
                }
                Err(SharedCasError::Corruption) => {
                    // Object exists but with different bytes — conflict.
                    report.failed += 1;
                    let msg = format!(
                        "{short_ref}: CAS corruption — \
                         object {full_hash} exists with different bytes",
                    );
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.clone(),
                        full_ref,
                        status: AliasStatus::Failed,
                        error: Some(msg.clone()),
                    });
                    report.errors.push(msg);
                    continue;
                }
                Err(err) => {
                    report.failed += 1;
                    let msg = format!("{short_ref}: CAS publish failed: {err}");
                    report.aliases.push(AliasEntry {
                        short_ref: short_ref.clone(),
                        full_ref,
                        status: AliasStatus::Failed,
                        error: Some(msg.clone()),
                    });
                    report.errors.push(msg);
                    continue;
                }
            }

            // Store alias mapping for backward-compatible reads.
            self.store.store_alias_deferred(short_ref, &full_ref);

            // Record in manifest.
            updated_manifest.entries.insert(
                short_ref.clone(),
                ManifestEntry {
                    short_ref: short_ref.clone(),
                    full_hash: full_hash.clone(),
                    migrated_at: now_unix(),
                },
            );

            report.migrated += 1;
            report.aliases.push(AliasEntry {
                short_ref: short_ref.clone(),
                full_ref,
                status: AliasStatus::Migrated,
                error: None,
            });
        }

        // Persist store changes and manifest (non-dry-run only).
        if !dry_run && report.migrated > 0 {
            if let Err(err) = self.store.persist_pending() {
                report.errors.push(format!("store persist failed: {err}"));
            }

            if let Some(path) = &self.manifest_path {
                updated_manifest.completed = report.failed == 0;
                if let Err(err) = updated_manifest.save(path) {
                    report.errors.push(format!("manifest save failed: {err}"));
                }
            }
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RecoveryStore;
    use crate::shared_cas::SharedCas;
    use tokenzero_core::ContentType;
    /// Derive the default CAS root from a recovery cache path.
    /// CAS blobs live under `<parent>/blobs/sha256/...`.
    fn cas_root_from_cache(cache_path: &Path) -> PathBuf {
        cache_path.parent().unwrap_or(cache_path).to_path_buf()
    }

    /// Create a test store and CAS in a tempdir.
    fn test_store_and_cas(dir: &Path) -> (RecoveryStore, SharedCas, PathBuf, PathBuf) {
        let cache = dir.join("recovery-cache.json");
        let cas_root = cas_root_from_cache(&cache);
        let store = RecoveryStore::new(Some(cache.clone()));
        let cas = SharedCas::new(cas_root.clone());
        let manifest = dir.join("migration-manifest.json");
        (store, cas, cache, manifest)
    }

    #[test]
    fn migrate_single_legacy_blob() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, _cache, manifest) = test_store_and_cas(dir.path());

        // Store a payload normally (creates a legacy short-ID blob).
        let text = "hello migration target";
        let payload = store.store_payload_deferred(text, ContentType::Unknown, None, None, None);
        let blob_ref = payload.blob_ref;

        // Verify it's a legacy short ref.
        assert!(is_legacy_blob_ref(&blob_ref));

        // Run migration.
        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let report = migration.run(false);

        assert_eq!(report.total, 1);
        assert_eq!(report.migrated, 1);
        assert_eq!(report.skipped, 0);
        assert_eq!(report.failed, 0);
        assert!(!report.aliases.is_empty());

        // Verify the alias was stored.
        let alias_target = store.alias_target(&blob_ref);
        assert!(alias_target.is_some());
        let full_ref = alias_target.unwrap();
        assert!(full_ref.starts_with("tz://blob/"));
        let full_hash = &full_ref["tz://blob/".len()..];
        assert_eq!(full_hash.len(), FULL_HASH_LEN);

        // Verify the CAS contains the full-hash object.
        assert!(cas.contains(full_hash));

        // Verify the manifest was written.
        let loaded = MigrationManifest::load(&manifest);
        assert_eq!(loaded.version, MIGRATION_MANIFEST_VERSION);
        assert!(loaded.entries.contains_key(&blob_ref));
    }

    #[test]
    fn idempotent_rerun() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, _cache, manifest) = test_store_and_cas(dir.path());

        // Store a payload.
        let text = "idempotent migration content";
        let payload = store.store_payload_deferred(text, ContentType::Unknown, None, None, None);
        let blob_ref = payload.blob_ref;

        // First run.
        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let report1 = migration.run(false);
        assert_eq!(report1.migrated, 1);
        assert_eq!(report1.skipped, 0);

        // Second run — should skip (alias already exists and matches).
        let mut migration2 = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let report2 = migration2.run(false);
        assert_eq!(report2.total, 1);
        assert_eq!(report2.migrated, 0);
        assert_eq!(report2.skipped, 1);
        assert_eq!(report2.failed, 0);
    }

    #[test]
    fn dry_run_produces_no_writes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, _cache, manifest) = test_store_and_cas(dir.path());

        // Store a payload.
        let text = "dry run content";
        let payload = store.store_payload_deferred(text, ContentType::Unknown, None, None, None);
        let blob_ref = payload.blob_ref;

        // Run in dry-run mode.
        let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest.clone()));
        let report = migration.run(true);

        assert!(report.dry_run);
        assert_eq!(report.total, 1);
        assert_eq!(report.migrated, 1);
        assert_eq!(report.failed, 0);

        // Verify no alias was stored.
        assert!(store.alias_target(&blob_ref).is_none());

        // Verify no manifest file exists.
        assert!(!manifest.exists());

        // Verify the CAS does not contain the full-hash object.
        let full_hash = full_sha256_hex(text.as_bytes());
        assert!(!cas.contains(&full_hash));
    }

    #[test]
    fn ambiguous_short_id_fails_safely() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, _cache, _manifest) = test_store_and_cas(dir.path());

        // Insert a blob with a fabricated ref ID that doesn't match its content.
        // The ref claims short ID "bdeadbeefdeadbeef" but the content's SHA-256
        // prefix won't be "deadbeefdeadbeef", creating an ambiguity.
        let fake_ref = "tz://blob/bdeadbeefdeadbeef";
        let content = "this content does not match the claimed short ID";
        store.insert_test_blob(fake_ref, content);

        // Run migration.
        let mut migration = LegacyMigration::new(&mut store, &cas, None);
        let report = migration.run(false);

        assert_eq!(report.total, 1);
        assert_eq!(report.migrated, 0);
        assert_eq!(report.failed, 1);
        assert!(report.errors.iter().any(|e| e.contains("ambiguous")));
    }

    #[test]
    fn alias_conflict_fails_safely() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, _cache, _manifest) = test_store_and_cas(dir.path());

        // Store a payload normally.
        let text = "original content for conflict test";
        let payload = store.store_payload_deferred(text, ContentType::Unknown, None, None, None);
        let blob_ref = payload.blob_ref;

        // Pre-insert a conflicting alias pointing to a wrong full hash.
        let wrong_full_ref = format!("{BLOB_REF_PREFIX}{}", "f".repeat(FULL_HASH_LEN));
        store.insert_test_alias(&blob_ref, &wrong_full_ref);

        // Run migration.
        let mut migration = LegacyMigration::new(&mut store, &cas, None);
        let report = migration.run(false);

        assert_eq!(report.total, 1);
        assert_eq!(report.migrated, 0);
        assert_eq!(report.failed, 1);
        assert!(report.errors.iter().any(|e| e.contains("alias conflict")));
    }

    #[test]
    fn is_legacy_blob_ref_detection() {
        // Legacy: b + 16 hex = 17 chars
        assert!(is_legacy_blob_ref("tz://blob/babc123def456789"));
        assert!(is_legacy_blob_ref("tz://blob/b0000000000000000"));

        // Full hash: 64 hex chars
        assert!(!is_legacy_blob_ref(&format!(
            "tz://blob/{}",
            "a".repeat(FULL_HASH_LEN)
        )));

        // Non-blob refs
        assert!(!is_legacy_blob_ref("tz://file/babc123def456789"));
        assert!(!is_legacy_blob_ref("tz://unit/uabc123def456789"));

        // Wrong prefix char
        assert!(!is_legacy_blob_ref("tz://blob/xabc123def456789"));

        // Too short / too long
        assert!(!is_legacy_blob_ref("tz://blob/babc"));
        assert!(!is_legacy_blob_ref("tz://blob/babc123def4567890"));
    }

    #[test]
    fn full_hash_ref_not_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, cas, _cache, _manifest) = test_store_and_cas(dir.path());

        // Insert a full-hash ref directly (simulating already-migrated data).
        let full_hash = "a".repeat(FULL_HASH_LEN);
        let full_ref = format!("{BLOB_REF_PREFIX}{full_hash}");
        store.insert_test_blob(&full_ref, "already canonical");

        let mut migration = LegacyMigration::new(&mut store, &cas, None);
        let report = migration.run(false);

        // Full-hash refs are not legacy, so total should be 0.
        assert_eq!(report.total, 0);
        assert_eq!(report.migrated, 0);
    }
}
