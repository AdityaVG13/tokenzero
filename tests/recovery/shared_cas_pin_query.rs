use std::fs;
use tempfile::tempdir;
use tokenzero_recovery::shared_cas::{
    GC_SCHEMA_VERSION, PinRecord, SharedCas, SharedCasError, gc_contract_digest_hex,
};
use tokenzero_recovery::{ActionCacheEntry, ActionCacheIndex};

const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn tzgvxc_actioncache_live_ref_is_a_gc_root() {
    let root = tempdir().unwrap();
    let index = ActionCacheIndex::open(root.path());
    index
        .put(ActionCacheEntry {
            key: HASH.to_string(),
            artifact_ref: format!("tz://blob/{HASH}"),
            fszero_bookmark: None,
            dep_closure_ref: None,
            class: "must_block_revalidate".into(),
            verified: true,
            world_id: None,
            tombstone: false,
            tombstoned_at_unix: None,
        })
        .unwrap();
    let cas = SharedCas::new(root.path().to_path_buf());
    assert!(
        cas.is_pinned(HASH),
        "live ActionCache artifact must be a GC root"
    );
}

#[test]
fn missing_pin_namespace_is_not_pinned() {
    let root = tempdir().unwrap();
    let cas = SharedCas::new(root.path().to_path_buf());

    assert!(!cas.is_pinned(HASH));
}

#[test]
fn malformed_pin_namespace_fails_closed() {
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("gc")).unwrap();
    fs::write(root.path().join("gc/pins"), b"not a directory").unwrap();
    let cas = SharedCas::new(root.path().to_path_buf());

    assert!(cas.is_pinned(HASH));
}

#[test]
fn malformed_same_length_expiry_fails_closed() {
    let root = tempdir().unwrap();
    let pins = root.path().join("gc/pins/tokenzero/project");
    fs::create_dir_all(&pins).unwrap();
    let pin = PinRecord {
        schema_version: GC_SCHEMA_VERSION.into(),
        record_type: "pin".into(),
        engine: "tokenzero".into(),
        project_id: HASH.into(),
        store_contract_digest: Some(gc_contract_digest_hex()),
        pin_id: "bad-expiry".into(),
        created_at: "2026-08-12T00:00:00Z".into(),
        expires_at: Some("2026-13-45T99:99:99Z".into()),
        blob_hash: HASH.into(),
    };
    fs::write(
        pins.join("bad-expiry.json"),
        serde_json::to_vec(&pin).unwrap(),
    )
    .unwrap();
    let cas = SharedCas::new(root.path().to_path_buf());

    assert!(cas.is_pinned(HASH));
}

#[test]
fn repair_keeps_invalid_hash_error_taxonomy() {
    let root = tempdir().unwrap();
    let cas = SharedCas::new(root.path().to_path_buf());

    assert!(matches!(
        cas.repair_object("bad-hash", b"bytes"),
        Err(SharedCasError::InvalidHash(_))
    ));
    assert!(matches!(
        cas.repair_object(HASH, b"bytes"),
        Err(SharedCasError::InvalidHash(_))
    ));
}
