use std::sync::{Arc, Barrier};
use std::thread;

use tempfile::tempdir;
use tokenzero_core::ContentType;
use tokenzero_recovery::{RecoveryStore, session_ordinal_ref};

#[test]
fn ordinal_short_and_full_refs_expand_identical_bytes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("recovery.json");
    let mut store = RecoveryStore::new(Some(path));
    let full = store
        .store_blob("gauge-orbit-bytes\n", ContentType::Code)
        .unwrap();
    let short = store.ensure_session_visible_alias(&full);
    let range = store.reserve_ordinal_range(4).unwrap();
    let ordinal = store.store_ordinal_alias_deferred(range, 0, &full).unwrap();
    store.persist_pending().unwrap();

    let expected = store.expand(&full, Some("raw"), None, None, None, None);
    assert!(expected.found);
    for alias in [short, ordinal] {
        let expanded = store.expand(&alias, Some("raw"), None, None, None, None);
        assert!(expanded.found, "{alias}: {}", expanded.reason);
        assert_eq!(expanded.content.as_bytes(), expected.content.as_bytes());
    }
}

#[test]
fn concurrent_range_allocation_is_dense_and_linearizable() {
    const WORKERS: usize = 8;
    const BATCH: u64 = 8;
    let dir = tempdir().unwrap();
    let path = dir.path().join("recovery.json");
    let barrier = Arc::new(Barrier::new(WORKERS));
    let mut handles = Vec::new();
    for _ in 0..WORKERS {
        let path = path.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            let mut store = RecoveryStore::new(Some(path));
            barrier.wait();
            store.reserve_ordinal_range(BATCH).unwrap()
        }));
    }
    let mut ranges = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);
    let generation = ranges[0].generation;
    assert!(generation > 0);
    for (index, range) in ranges.iter().enumerate() {
        assert_eq!(range.generation, generation);
        assert_eq!(range.start, 1 + index as u64 * BATCH);
        assert_eq!(range.end_exclusive, range.start + BATCH);
    }
    let mut restarted = RecoveryStore::new(Some(path));
    assert_eq!(
        restarted.reserve_ordinal_range(1).unwrap().start,
        1 + WORKERS as u64 * BATCH
    );
}

#[test]
fn generation_sidecar_prevents_ordinal_aba_after_store_recreation() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("recovery.json");
    let mut first = RecoveryStore::new(Some(path.clone()));
    let first_blob = first.store_blob("first bytes", ContentType::Code).unwrap();
    let first_range = first.reserve_ordinal_range(1).unwrap();
    let old_alias = first
        .store_ordinal_alias_deferred(first_range, 0, &first_blob)
        .unwrap();
    first.persist_pending().unwrap();

    std::fs::remove_file(&path).unwrap();
    let mut second = RecoveryStore::new(Some(path));
    let second_blob = second
        .store_blob("second bytes", ContentType::Code)
        .unwrap();
    let second_range = second.reserve_ordinal_range(1).unwrap();
    assert!(second_range.generation > first_range.generation);
    let new_alias = second
        .store_ordinal_alias_deferred(second_range, 0, &second_blob)
        .unwrap();
    second.persist_pending().unwrap();
    assert_ne!(old_alias, new_alias);

    let stale = second.expand(&old_alias, Some("raw"), None, None, None, None);
    assert!(!stale.found);
    assert_eq!(stale.reason, "stale-ref");
    assert!(!stale.content.contains("second bytes"));

    let missing_current = session_ordinal_ref(second_range.generation, 999);
    let dangling = second.expand(&missing_current, Some("raw"), None, None, None, None);
    assert!(!dangling.found);
    assert_eq!(dangling.reason, "dangling-ref");

    let absent_target = format!("tz://blob/{}", "ab".repeat(32));
    let absent_range = second.reserve_ordinal_range(1).unwrap();
    let evicted_alias = second
        .store_ordinal_alias_deferred(absent_range, 0, &absent_target)
        .unwrap();
    second.persist_pending().unwrap();
    let evicted = second.expand(&evicted_alias, Some("raw"), None, None, None, None);
    assert!(!evicted.found);
    assert_eq!(evicted.reason, "dangling-ref");
    assert!(evicted.content.is_empty());
}
