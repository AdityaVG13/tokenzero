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
fn repeated_put_of_live_ref_does_not_append_payload() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    let mut store = SegmentStore::create_shadow(p, None).unwrap();
    store.put("same", b"payload", u64::MAX - 1).unwrap();
    let data_path = d.path().join(&store.manifest().hot.data_file);
    let first_len = std::fs::metadata(&data_path).unwrap().len();

    store.put("same", b"payload", u64::MAX).unwrap();

    assert_eq!(std::fs::metadata(data_path).unwrap().len(), first_len);
    assert_eq!(store.manifest().hot.ref_count, 1);
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

#[test]
fn hot_recovery_serializes_with_writer_and_rereads_manifest() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    let mut initial = SegmentStore::create_shadow(p.clone(), None).unwrap();
    initial.put("before", b"before", u64::MAX).unwrap();
    let index = d.path().join(&initial.manifest().hot.index_file);
    std::fs::write(index, b"force recovery").unwrap();

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let open_path = p.clone();
    let open_barrier = barrier.clone();
    let opener = std::thread::spawn(move || {
        open_barrier.wait();
        let mut store = SegmentStore::open(open_path, None).unwrap();
        assert_eq!(store.expand("before").unwrap().unwrap(), b"before");
    });
    let write_path = p.clone();
    let writer = std::thread::spawn(move || {
        barrier.wait();
        let mut store = SegmentStore::open(write_path, None).unwrap();
        store.put("during", b"during", u64::MAX).unwrap();
    });
    opener.join().unwrap();
    writer.join().unwrap();

    let mut reopened = SegmentStore::open(p, None).unwrap();
    assert_eq!(reopened.expand("before").unwrap().unwrap(), b"before");
    assert_eq!(reopened.expand("during").unwrap().unwrap(), b"during");
}

#[test]
fn gc_publishes_both_manifests_before_deleting_segments() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    let mut store = SegmentStore::create_shadow(p.clone(), None).unwrap();
    store.put("expired", b"expired", 1).unwrap();
    store.seal().unwrap();
    assert_eq!(store.evict_expired(2).unwrap(), 1);

    // Force restart through the backup manifest. It must describe the same
    // post-GC file set rather than referencing files already unlinked.
    std::fs::write(SegmentStore::manifest_path(&p), b"corrupt primary").unwrap();
    let mut reopened = SegmentStore::open(p, None).unwrap();
    assert_eq!(reopened.expand("expired").unwrap(), None);
}

#[test]
fn restart_cleans_gc_orphans_not_named_by_manifest() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    let mut store = SegmentStore::create_shadow(p.clone(), None).unwrap();
    store.put("expired", b"expired", 1).unwrap();
    store.seal().unwrap();
    let evicted = store.manifest().cold[0].clone();
    assert_eq!(store.evict_expired(2).unwrap(), 1);
    let data = d.path().join(&evicted.data_file);
    let index = d.path().join(&evicted.index_file);
    std::fs::write(&data, b"orphan data").unwrap();
    std::fs::write(&index, b"orphan index").unwrap();

    SegmentStore::open(p, None).unwrap();

    assert!(!data.exists());
    assert!(!index.exists());
}

#[test]
fn restart_removes_orphan_next_generation_and_can_seal_again() {
    let d = tempdir().unwrap();
    let p = d.path().join("recovery-cache.json");
    let mut store = SegmentStore::create_shadow(p.clone(), None).unwrap();
    store.put("kept", b"kept", u64::MAX).unwrap();
    let next = store.manifest().hot.generation + 1;
    let next_data = d.path().join(format!("recovery.{next}.segment"));
    let next_index = d.path().join(format!("recovery.{next}.segment.index"));
    std::fs::write(&next_data, b"TZSEG001").unwrap();
    std::fs::write(&next_index, b"{}").unwrap();

    let mut reopened = SegmentStore::open(p, None).unwrap();
    assert!(!next_data.exists());
    assert!(!next_index.exists());
    reopened.seal().unwrap();
    assert_eq!(reopened.expand("kept").unwrap().unwrap(), b"kept");
}
