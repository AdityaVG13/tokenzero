use super::*;
use tempfile::tempdir;

struct EmptyStore;

impl MigrationStore for EmptyStore {
    fn blob_ref_ids(&self) -> Vec<String> {
        Vec::new()
    }
    fn resolve_blob_bytes(&self, _ref_id: &str) -> BlobContentResult {
        BlobContentResult::Missing
    }
    fn alias_target(&self, _alias: &str) -> Option<String> {
        None
    }
    fn store_alias_deferred(&mut self, _alias: &str, _target: &str) {}
    fn remove_alias(&mut self, _alias: &str) {}
    fn remove_blob(&mut self, _ref_id: &str) {}
    fn mark_ambiguous(&mut self, _short_ref: &str) {}
    fn is_ambiguous(&self, _short_ref: &str) -> bool {
        false
    }
    fn persist_pending(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn cleanup_apply_without_confirm_refuses_before_verify() {
    let dir = tempdir().unwrap();
    let cas = SharedCas::new(dir.path().join("cas"));
    let mut store = EmptyStore;
    let mut migration = LegacyMigration::new(&mut store, &cas, None);
    let report = migration.cleanup(true, false);
    assert!(report.is_failure());
    assert!(!report.dry_run);
    assert_eq!(report.operation, "cleanup");
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].code, "cleanup-confirmation-required");
    assert_eq!(
        report.errors[0].message,
        "cleanup requires --confirm-cleanup flag"
    );
}

#[test]
fn cleanup_dry_run_without_manifest_requires_verification_and_does_not_extend_verify_errors() {
    let dir = tempdir().unwrap();
    let cas = SharedCas::new(dir.path().join("cas"));
    let mut store = EmptyStore;
    let mut migration = LegacyMigration::new(&mut store, &cas, None);
    let report = migration.cleanup(false, false);
    assert!(report.is_failure());
    assert!(report.dry_run);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].code, "cleanup-needs-verification");
    assert_eq!(
        report.errors[0].message,
        "cleanup requires successful verification first"
    );
}

#[test]
fn cleanup_apply_confirmed_without_manifest_extends_verify_errors() {
    let dir = tempdir().unwrap();
    let cas = SharedCas::new(dir.path().join("cas"));
    let mut store = EmptyStore;
    let mut migration = LegacyMigration::new(&mut store, &cas, None);
    let report = migration.cleanup(true, true);
    assert!(report.is_failure());
    assert_eq!(report.errors[0].code, "cleanup-needs-verification");
    assert!(
        report.errors.iter().any(|error| {
            error.code == "manifest-missing" && error.message == "no manifest path configured"
        }),
        "{:?}",
        report.errors
    );
}

#[test]
fn cleanup_dry_run_with_empty_manifest_plans_zero() {
    let dir = tempdir().unwrap();
    let manifest = dir.path().join("manifest.json");
    fs::write(
        &manifest,
        r#"{"version":"tokenzero.migration.v2","entries":{},"completed":false}"#,
    )
    .unwrap();
    let cas = SharedCas::new(dir.path().join("cas"));
    let mut store = EmptyStore;
    let mut migration = LegacyMigration::new(&mut store, &cas, Some(manifest));
    let report = migration.cleanup(false, false);
    assert!(!report.is_failure(), "{:?}", report.errors);
    assert!(report.dry_run);
    assert_eq!(report.total, 0);
    assert_eq!(report.migrated, 0);
}
