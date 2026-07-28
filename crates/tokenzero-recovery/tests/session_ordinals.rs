use std::sync::{Arc, Barrier};
use std::thread;

use tempfile::tempdir;
use tokenzero_core::ContentType;
use tokenzero_recovery::RecoveryStore;

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
    for (index, range) in ranges.iter().enumerate() {
        assert_eq!(range.generation, 1);
        assert_eq!(range.start, 1 + index as u64 * BATCH);
        assert_eq!(range.end_exclusive, range.start + BATCH);
    }
    let mut restarted = RecoveryStore::new(Some(path));
    assert_eq!(
        restarted.reserve_ordinal_range(1).unwrap().start,
        1 + WORKERS as u64 * BATCH
    );
}
