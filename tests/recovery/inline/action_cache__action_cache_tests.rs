use super::*;
use tempfile::tempdir;

fn key(n: u8) -> String {
    format!("{n:064x}")
}

fn entry(n: u8, artifact: &str) -> ActionCacheEntry {
    ActionCacheEntry {
        key: key(n),
        artifact_ref: artifact.to_string(),
        fszero_bookmark: None,
        dep_closure_ref: None,
        class: "must_block_revalidate".into(),
        verified: true,
        world_id: Some("w1".into()),
        tombstone: false,
        tombstoned_at_unix: None,
        l3_cold: false,
        cold_since_unix: None,
    }
}

#[test]
fn tzqjfi_put_get_roundtrip_and_tombstone() {
    let dir = tempdir().unwrap();
    let index = ActionCacheIndex::open(dir.path());
    let first = entry(1, "tz://blob/aaa");
    index.put(first.clone()).unwrap();
    assert_eq!(index.get(&key(1)).unwrap().as_ref(), Some(&first));
    assert_eq!(index.live_keys().unwrap(), vec![key(1)]);
    assert_eq!(
        index.live_artifact_refs().unwrap(),
        vec!["tz://blob/aaa".to_string()]
    );

    assert!(index.tombstone(&key(1)).unwrap());
    assert!(index.get(&key(1)).unwrap().is_none());
    assert!(index.live_keys().unwrap().is_empty());
    assert!(index.live_artifact_refs().unwrap().is_empty());
    // Tombstones never delete the validity record (ZS-CACHE-013).
    let record = index.load_raw(&key(1)).unwrap().unwrap();
    assert!(record.tombstone, "tombstone record must persist on disk");
    assert_eq!(record.artifact_ref, "tz://blob/aaa");
}

#[test]
fn tzqjfi_refuses_newer_major_segment() {
    let dir = tempdir().unwrap();
    let index = ActionCacheIndex::open(dir.path());
    let item = entry(2, "tz://blob/bbb");
    let path = index.segment_path(&item.key);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bad = serde_json::json!({
        "schema": "tokenzero.store",
        "major": 9,
        "minor": 0,
        "entry": item,
    });
    fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
    let err = index.get(&item.key).unwrap_err();
    assert!(
        matches!(err, ActionCacheError::Schema(SchemaSkewError::NewerMajor { found }) if found.major == 9),
        "{err}"
    );
}

#[test]
fn tzqjfi_tokenzero_owned_fields_do_not_require_sibling_pointers() {
    let dir = tempdir().unwrap();
    let index = ActionCacheIndex::open(dir.path());
    let mut item = entry(3, "tz://blob/ccc");
    item.fszero_bookmark = None;
    item.dep_closure_ref = None;
    index.put(item.clone()).unwrap();
    let got = index.get(&key(3)).unwrap().unwrap();
    assert!(got.fszero_bookmark.is_none());
    assert!(got.dep_closure_ref.is_none());
    assert_eq!(got.artifact_ref, "tz://blob/ccc");
    assert!(got.verified);
}

#[test]
fn tzgvxc_eviction_marks_cold_before_blob_and_honors_grace() {
    let dir = tempdir().unwrap();
    let index = ActionCacheIndex::open(dir.path());
    let artifact = format!("tz://blob/{}", key(10));
    index.put(entry(10, &artifact)).unwrap();
    index.put(entry(11, &artifact)).unwrap();
    let slack = EvictionSlackGuard::new(100, 100).unwrap();

    let early = index
        .prepare_blob_eviction(&artifact, 1_000, 60, slack, 1)
        .unwrap();
    assert_eq!(early.cold_keys.len(), 2);
    assert!(!early.may_delete_blob, "grace has not elapsed");
    // L3 loss must not invalidate the logical entry (ZS-CACHE-013).
    let cold = index.get(&key(10)).unwrap().unwrap();
    assert!(cold.l3_cold, "entry keeps L2 validity, marked L3-cold");
    assert!(!cold.tombstone, "blob eviction must never tombstone");
    assert!(
        index
            .protects_hash(artifact_full_hash(&artifact).unwrap(), 1_000, 60)
            .unwrap(),
        "cold entries still pin during grace"
    );

    let ready = index
        .prepare_blob_eviction(&artifact, 1_070, 60, slack, 1)
        .unwrap();
    assert!(ready.may_delete_blob);
    assert!(ready.waiting_grace.is_empty());
    assert!(
        !index
            .protects_hash(artifact_full_hash(&artifact).unwrap(), 1_070, 60)
            .unwrap(),
        "approved cold eviction releases the blob after grace"
    );
}

#[test]
fn tzgvxc_l3_loss_preserves_l2_validity_and_refetch_restores() {
    let dir = tempdir().unwrap();
    let index = ActionCacheIndex::open(dir.path());
    let artifact = format!("tz://blob/{}", key(30));
    index.put(entry(30, &artifact)).unwrap();

    assert!(index.mark_l3_loss(&key(30), 9_000).unwrap());
    let cold = index.get(&key(30)).unwrap().unwrap();
    assert!(cold.l3_cold, "L2 validity preserved, needs-refetch");
    assert_eq!(cold.cold_since_unix, Some(9_000));
    assert!(!cold.tombstone, "L3 loss is never a logical invalidation");

    // A later refetch of identical bytes restores L3 without rediscovery:
    // same key, same artifact identity, no re-derivation.
    assert!(index.complete_refetch(&key(30)).unwrap());
    let restored = index.get(&key(30)).unwrap().unwrap();
    assert!(!restored.l3_cold);
    assert_eq!(restored.key, cold.key);
    assert_eq!(restored.artifact_ref, cold.artifact_ref);
    assert!(
        index
            .protects_hash(artifact_full_hash(&artifact).unwrap(), 9_100, 60)
            .unwrap(),
        "restored L3 pins the blob again"
    );
    // Idempotence: refetch on a non-cold entry is a no-op.
    assert!(!index.complete_refetch(&key(30)).unwrap());
}

#[test]
fn tzgvxc_eviction_slack_refuses_below_the_ninety_nine_percent_floor() {
    let dir = tempdir().unwrap();
    let index = ActionCacheIndex::open(dir.path());
    let artifact = format!("tz://blob/{}", key(40));
    index.put(entry(40, &artifact)).unwrap();

    // Just above the floor: retained 100 - 1 = 99 >= floor(99).
    let ok_guard = EvictionSlackGuard::new(100, 100).unwrap();
    assert_eq!(ok_guard.slack_ppm(), 10_000);
    let plan = index
        .prepare_blob_eviction(&artifact, 1_000, 0, ok_guard, 1)
        .unwrap();
    assert!(plan.may_delete_blob);
    assert!(index.get(&key(40)).unwrap().unwrap().l3_cold);

    // Just below the floor: retained 100 - 2 = 98 < floor(99): fail loud,
    // with zero side effects on the index (no cold marking, still protected).
    let artifact2 = format!("tz://blob/{}", key(41));
    index.put(entry(41, &artifact2)).unwrap();
    let below = EvictionSlackGuard::new(100, 100).unwrap();
    let err = index
        .prepare_blob_eviction(&artifact2, 2_000, 0, below, 2)
        .unwrap_err();
    match err {
        ActionCacheError::EvictionRefused {
            resident_mass,
            demanded_mass,
            evict_weight,
            slack_ppm,
        } => {
            assert_eq!((resident_mass, demanded_mass, evict_weight), (100, 100, 2));
            assert_eq!(slack_ppm, 10_000);
        }
        other => panic!("expected EvictionRefused, got {other:?}"),
    }
    let untouched = index.get(&key(41)).unwrap().unwrap();
    assert!(!untouched.l3_cold, "refused eviction must not mark anything cold");
    assert!(!untouched.tombstone);
    assert!(
        index
            .protects_hash(artifact_full_hash(&artifact2).unwrap(), 2_000, 0)
            .unwrap(),
        "refused eviction leaves the blob protected"
    );

    // A zero demanded mass cannot anchor a floor.
    assert!(matches!(
        EvictionSlackGuard::new(50, 0),
        Err(ActionCacheError::InvalidDemandMass)
    ));
}

#[test]
fn tzgvxc_cross_world_resolution_denied_and_write_never_clobbers() {
    let dir = tempdir().unwrap();
    let index = ActionCacheIndex::open(dir.path());
    let artifact = format!("tz://blob/{}", key(50));
    let mut a = entry(50, &artifact);
    a.world_id = Some("world-a".into());
    index.put(a.clone()).unwrap();

    // Same world resolves.
    let resolved = index.resolve(&key(50), Some("world-a")).unwrap().unwrap();
    assert_eq!(resolved.world_id.as_deref(), Some("world-a"));
    // Another world must not resolve it; a scoped entry never leaks to an
    // unscoped resolver either.
    assert!(index.resolve(&key(50), Some("world-b")).unwrap().is_none());
    assert!(index.resolve(&key(50), None).unwrap().is_none());

    // World B's write-through must not clobber world A's live record.
    let mut b = entry(50, &artifact);
    b.world_id = Some("world-b".into());
    index.put(b).unwrap();
    assert_eq!(
        index.get(&key(50)).unwrap().unwrap().world_id.as_deref(),
        Some("world-a"),
        "a live entry is only replaceable by its own world"
    );
    assert!(index.resolve(&key(50), Some("world-b")).unwrap().is_none());
    assert!(index.resolve(&key(50), Some("world-a")).unwrap().is_some());

    // Unscoped legacy entries stay global for any caller.
    let legacy_key = key(51);
    let mut legacy = entry(51, &format!("tz://blob/{legacy_key}"));
    legacy.world_id = None;
    index.put(legacy).unwrap();
    assert!(index.resolve(&legacy_key, None).unwrap().is_some());
    assert!(index.resolve(&legacy_key, Some("world-c")).unwrap().is_some());
}

#[test]
fn tzgvxc_concurrent_serve_never_sees_dangling_ref() {
    use std::sync::{Arc, Mutex};
    use std::thread;

    let dir = tempdir().unwrap();
    let index = Arc::new(ActionCacheIndex::open(dir.path()));
    let artifact = format!("tz://blob/{}", key(20));
    index.put(entry(20, &artifact)).unwrap();
    let blobs = Arc::new(Mutex::new(vec![artifact.clone()]));
    let dangling = Arc::new(Mutex::new(false));

    let server = {
        let index = Arc::clone(&index);
        let blobs = Arc::clone(&blobs);
        let dangling = Arc::clone(&dangling);
        let artifact = artifact.clone();
        thread::spawn(move || {
            for _ in 0..200 {
                match index.serve(&key(20)).unwrap() {
                    Some((entry, _pin)) => {
                        let live = blobs.lock().unwrap();
                        if !live.iter().any(|blob| blob == &entry.artifact_ref) {
                            *dangling.lock().unwrap() = true;
                        }
                        assert_eq!(entry.artifact_ref, artifact);
                    }
                    None => {}
                }
            }
        })
    };
    let gc = {
        let index = Arc::clone(&index);
        let blobs = Arc::clone(&blobs);
        thread::spawn(move || {
            let plan = index
                .prepare_blob_eviction(&artifact, 5_000, 0, EvictionSlackGuard::new(100, 100).unwrap(), 1)
                .unwrap();
            if plan.may_delete_blob {
                blobs.lock().unwrap().retain(|blob| blob != &artifact);
            }
        })
    };
    server.join().unwrap();
    gc.join().unwrap();
    assert!(
        !*dangling.lock().unwrap(),
        "serve must not observe a tombstoned or deleted blob"
    );
}
