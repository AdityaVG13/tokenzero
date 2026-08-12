use std::fs;
use tempfile::tempdir;
use tokenzero_recovery::shared_cas::{
    gc_contract_digest_hex, PinRecord, SharedCas, SharedCasError, GC_SCHEMA_VERSION,
};

const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

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
