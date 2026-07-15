use crate::RecoveryStore;
use crate::segment_store::{SegmentMigrationPhase, SegmentStore};
use crate::shared_cas::{
    GC_ENGINE_TOKENZERO, GC_SCHEMA_VERSION, PinRecord, SharedCas, publish_pin_record,
};
use std::io::Write;
use tempfile::tempdir;
use tokenzero_core::ContentType;
#[test]
fn threshold_lazy_exact() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    let mut s = SegmentStore::create_shadow(p.clone(), None).unwrap();
    s.set_segment_bytes(180);
    s.put("a", b"first exact bytes", u64::MAX).unwrap();
    s.put("b", &[7; 16], u64::MAX).unwrap();
    assert!(s.manifest().cold.iter().all(|d| d.written_bytes <= 180));
    assert!(s.manifest().hot.written_bytes <= 180);
    assert!(!s.manifest().cold.is_empty());
    let mut r = SegmentStore::open(p, None).unwrap();
    assert_eq!(r.cold_indexes_loaded(), 0);
    assert_eq!(r.expand("a").unwrap().unwrap(), b"first exact bytes");
    assert_eq!(r.cold_indexes_loaded(), 1);
}

#[test]
fn segment_store_hashes_raw_non_utf8_bytes() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    let payload = vec![0, 0xff, 0x80, b'a'];
    let mut s = SegmentStore::create_shadow(p.clone(), None).unwrap();
    s.put("binary", &payload, u64::MAX).unwrap();

    let mut reopened = SegmentStore::open(p, None).unwrap();

    assert_eq!(reopened.expand("binary").unwrap().unwrap(), payload);
}

#[test]
fn corrupt_manifest_backup_and_torn_hot() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    let mut s = SegmentStore::create_shadow(p.clone(), None).unwrap();
    s.put("a", b"alpha", u64::MAX).unwrap();
    s.put("b", b"beta", u64::MAX).unwrap();
    let hot = d.path().join(&s.manifest().hot.data_file);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&hot)
        .unwrap()
        .write_all(&[1, 2, 3])
        .unwrap();
    std::fs::write(d.path().join(&s.manifest().hot.index_file), b"bad").unwrap();
    std::fs::write(SegmentStore::manifest_path(&p), b"bad").unwrap();
    let mut r = SegmentStore::open(p, None).unwrap();
    assert_eq!(r.expand("a").unwrap().unwrap(), b"alpha");
}
#[test]
fn ttl_pin_and_rollback() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    let cas = SharedCas::new(d.path().to_path_buf());
    let bytes = b"pinned";
    let hash = cas.publish(bytes).unwrap();
    let r = format!("tz://blob/{hash}");
    let mut s = SegmentStore::create_shadow(p, Some(cas.clone())).unwrap();
    s.put(&r, bytes, 10).unwrap();
    s.seal().unwrap();
    let pin_path = publish_pin_record(
        d.path(),
        &PinRecord {
            schema_version: GC_SCHEMA_VERSION.into(),
            record_type: "pin".into(),
            engine: GC_ENGINE_TOKENZERO.into(),
            project_id: hash.clone(),
            pin_id: "test-pin".into(),
            created_at: "2026-07-15T00:00:00Z".into(),
            expires_at: None,
            blob_hash: hash,
        },
    )
    .unwrap();
    assert_eq!(s.evict_expired(11).unwrap(), 0);
    std::fs::remove_file(pin_path).unwrap();
    assert_eq!(s.evict_expired(11).unwrap(), 1);
    s.activate().unwrap();
    s.rollback().unwrap();
    assert_eq!(s.manifest().phase, SegmentMigrationPhase::Legacy);
}
#[test]
fn concurrent_writers_merge() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    SegmentStore::create_shadow(p.clone(), None).unwrap();
    let a = p.clone();
    let b = p.clone();
    let x = std::thread::spawn(move || {
        SegmentStore::open(a, None)
            .unwrap()
            .put("a", b"a", u64::MAX)
            .unwrap()
    });
    let y = std::thread::spawn(move || {
        SegmentStore::open(b, None)
            .unwrap()
            .put("b", b"b", u64::MAX)
            .unwrap()
    });
    x.join().unwrap();
    y.join().unwrap();
    let mut s = SegmentStore::open(p, None).unwrap();
    assert_eq!(s.expand("a").unwrap().unwrap(), b"a");
    assert_eq!(s.expand("b").unwrap().unwrap(), b"b");
}

#[test]
fn migrates_legacy_blobs_and_preserves_rollback_source() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    let mut legacy = RecoveryStore::new(Some(p.clone()));
    let ref_id = legacy
        .store_blob("legacy payload", ContentType::Unknown)
        .unwrap();
    let legacy_bytes = std::fs::read(&p).unwrap();

    let migrated = SegmentStore::migrate_legacy(p.clone(), &mut legacy, None).unwrap();
    assert_eq!(migrated.manifest().phase, SegmentMigrationPhase::Active);
    assert_eq!(std::fs::read(&p).unwrap(), legacy_bytes);

    let mut reopened = SegmentStore::open(p, None).unwrap();
    assert_eq!(reopened.cold_indexes_loaded(), 0);
    assert_eq!(
        reopened.expand(&ref_id).unwrap().unwrap(),
        b"legacy payload"
    );
}

#[test]
fn whole_segment_eviction_retains_any_live_record() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    let mut s = SegmentStore::create_shadow(p, None).unwrap();
    s.put("expired", b"old", 10).unwrap();
    s.put("live", b"current", 20).unwrap();
    s.seal().unwrap();

    assert_eq!(s.evict_expired(11).unwrap(), 0);
    assert_eq!(s.expand("expired").unwrap().unwrap(), b"old");
    assert_eq!(s.expand("live").unwrap().unwrap(), b"current");
    assert_eq!(s.evict_expired(21).unwrap(), 1);
}

#[test]
fn binary_payload_round_trips_without_utf8_assumptions() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    let payload = [0, 0xff, 0x80, 97, 0];
    let mut store = SegmentStore::create_shadow(p.clone(), None).unwrap();
    store.put("binary", &payload, u64::MAX).unwrap();

    let mut reopened = SegmentStore::open(p, None).unwrap();
    assert_eq!(reopened.expand("binary").unwrap().unwrap(), payload);
}
