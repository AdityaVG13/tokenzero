use super::*;
use proptest::prelude::*;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{LazyLock, Mutex};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Shared helpers (used in ≥3 tests each)
// ---------------------------------------------------------------------------

/// Create a persisted RecoveryStore in a fresh temp directory.
/// Returns `(store, cache_path, temp_dir)`. Caller must keep the `TempDir` alive.
fn temp_store() -> (RecoveryStore, PathBuf, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let store = RecoveryStore::new(Some(cache.clone()));
    (store, cache, dir)
}

static REF_INDEX_ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

struct RefIndexEnvGuard {
    old: Option<(bool, PathBuf)>,
}

impl RefIndexEnvGuard {
    fn set(index_path: &Path, enabled: bool) -> Self {
        let old = set_ref_index_test_override(Some((enabled, index_path.to_path_buf())));
        Self { old }
    }
}

impl Drop for RefIndexEnvGuard {
    fn drop(&mut self) {
        let _ = set_ref_index_test_override(self.old.take());
    }
}

fn with_ref_index_env<R>(index_path: &Path, enabled: bool, f: impl FnOnce() -> R) -> R {
    let _lock = REF_INDEX_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _guard = RefIndexEnvGuard::set(index_path, enabled);
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(payload) => resume_unwind(payload),
    }
}

/// Create an in-memory RecoveryStore (no persistence) with custom config.
fn mem_store(config: RecoveryConfig) -> RecoveryStore {
    RecoveryStore::with_config(None, config)
}

fn write_aged_file(path: &Path, bytes: usize, age: Duration) {
    fs::write(path, vec![b'x'; bytes]).unwrap();
    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
}

fn setup_tmp_sweep_files(
    dir: &Path,
    cache_name: &str,
    stale_age: Duration,
) -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf) {
    let _cache = dir.join(cache_name);
    let hidden = dir.join(format!(".{cache_name}.123.7.tmp"));
    let legacy = dir.join(format!("{cache_name}.456.a1b2c3.tmp"));
    let fresh = dir.join(format!(".{cache_name}.789.8.tmp"));
    let lock = dir.join(format!("{cache_name}.lock"));
    let unrelated = dir.join("other-file.tmp");
    write_aged_file(&hidden, 5, stale_age);
    write_aged_file(&legacy, 7, stale_age);
    write_aged_file(&fresh, 3, Duration::from_secs(1));
    write_aged_file(&lock, 2, stale_age);
    write_aged_file(&unrelated, 9, stale_age);
    (hidden, legacy, fresh, lock, unrelated)
}

fn assert_tmp_sweep_report(
    report: &TmpSweepReport,
    dry_run: bool,
    hidden: &Path,
    legacy: &Path,
    fresh: &Path,
    lock: &Path,
    unrelated: &Path,
) {
    assert_eq!(report.dry_run, dry_run);
    assert_eq!(report.scanned, 3);
    assert_eq!(report.removed, 2);
    assert_eq!(report.removed_bytes, 12);
    assert_eq!(report.failed, 0);
    if dry_run {
        assert!(hidden.exists(), "dry run must not unlink");
        assert!(legacy.exists(), "dry run must not unlink");
    } else {
        assert!(
            !hidden.exists(),
            "stale hidden-shape orphan must be reclaimed"
        );
        assert!(
            !legacy.exists(),
            "stale legacy-shape orphan must be reclaimed"
        );
    }
    assert!(
        fresh.exists(),
        "an in-flight writer's temp file must survive"
    );
    assert!(lock.exists(), "the lock anchor must never be touched");
    assert!(unrelated.exists(), "unrelated temp files must survive");
}

// ---------------------------------------------------------------------------
// Roundtrip / persistence
// ---------------------------------------------------------------------------

#[test]
fn restart_expand_is_byte_exact() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let text = "alpha\nbeta\ngamma\n";
    let stored = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload(text, ContentType::Code, None, None, None)
            .unwrap()
    };
    let mut restarted = RecoveryStore::new(Some(cache));
    let expanded = restarted.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
    assert!(expanded.found);
    assert_eq!(expanded.content, text);
}

#[test]
fn ref_index_expands_blob_across_cache_roots() {
    let index_dir = tempdir().unwrap();
    with_ref_index_env(index_dir.path(), true, || {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let cache_a = dir_a.path().join("recovery-cache.json");
        let cache_b = dir_b.path().join("recovery-cache.json");
        let text = "alpha\nbeta\nbytes exact\n";
        let stored = {
            let mut store = RecoveryStore::new(Some(cache_a));
            store
                .store_payload(text, ContentType::Unknown, None, None, None)
                .unwrap()
        };

        let mut other_root = RecoveryStore::new(Some(cache_b));
        let expanded = other_root.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
        assert!(expanded.found);
        assert_eq!(expanded.content, text);
    });
}

#[test]
fn ref_index_disabled_preserves_local_only_miss() {
    let index_dir = tempdir().unwrap();
    with_ref_index_env(index_dir.path(), false, || {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let stored = {
            let mut store = RecoveryStore::new(Some(dir_a.path().join("cache.json")));
            store
                .store_payload("hidden\n", ContentType::Unknown, None, None, None)
                .unwrap()
        };

        let mut other = RecoveryStore::new(Some(dir_b.path().join("cache.json")));
        let expanded = other.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
        assert!(!expanded.found);
        assert!(expanded.reason.contains("per-user ref-index disabled"));
    });
}

#[test]
fn stale_ref_index_entry_is_pruned_and_reports_tiers() {
    let index_dir = tempdir().unwrap();
    with_ref_index_env(index_dir.path(), true, || {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let cache_a = dir_a.path().join("cache.json");
        let stored = {
            let mut store = RecoveryStore::new(Some(cache_a.clone()));
            store
                .store_payload("gone\n", ContentType::Unknown, None, None, None)
                .unwrap()
        };
        fs::remove_file(&cache_a).unwrap();

        let mut other = RecoveryStore::new(Some(dir_b.path().join("cache.json")));
        let expanded = other.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
        assert!(!expanded.found);
        assert!(expanded.reason.contains("explicit/env cache"));
        assert!(expanded.reason.contains("current-root store"));
        assert!(expanded.reason.contains("per-user ref-index"));

        let shard = ref_index_shard_path(index_dir.path(), &stored.blob_ref);
        let text = fs::read_to_string(shard).unwrap_or_default();
        assert!(!text.contains(&stored.blob_ref));
    });
}

#[test]
fn ref_index_compaction_keeps_newest_entry_per_ref() {
    let dir = tempdir().unwrap();
    let shard = dir.path().join("ba.ndjson");
    let ref_id = "tz://blob/ba_ref";
    append_ref_index_line(&shard, ref_id, Path::new("/old-store.json"), 1).unwrap();
    append_ref_index_line(
        &shard,
        "tz://blob/ba_other",
        Path::new("/other-store.json"),
        1,
    )
    .unwrap();
    append_ref_index_line(&shard, ref_id, Path::new("/new-store.json"), 2).unwrap();

    compact_ref_index_shard(&shard).unwrap();

    let text = fs::read_to_string(shard).unwrap();
    assert!(text.contains("/new-store.json"));
    assert!(text.contains("/other-store.json"));
    assert!(!text.contains("/old-store.json"));
    assert_eq!(text.lines().count(), 2);
}

#[test]
fn ref_index_concurrent_append_smoke() {
    let index_dir = tempdir().unwrap();
    with_ref_index_env(index_dir.path(), true, || {
        let store_path = index_dir.path().join("store.json");
        let threads: Vec<_> = (0..16)
            .map(|idx| {
                let store_path = store_path.clone();
                let index_path = index_dir.path().to_path_buf();
                thread::spawn(move || {
                    let _old = set_ref_index_test_override(Some((true, index_path)));
                    let ref_id = format!("tz://blob/ba{idx:030}");
                    append_blob_refs_to_ref_index(&store_path, &[ref_id]);
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }

        let shard = index_dir.path().join("ba.ndjson");
        let text = fs::read_to_string(shard).unwrap();
        let entries = newest_ref_index_entries(&text, None);
        assert_eq!(entries.len(), 16);
    });
}

#[test]
fn deferred_payloads_persist_in_one_batch() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let (first, second) = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        let first = store.store_payload_deferred("alpha\n", ContentType::Unknown, None, None, None);
        let second = store.store_payload_deferred("beta\n", ContentType::Unknown, None, None, None);
        assert!(!cache.exists());
        store.persist_pending().unwrap();
        (first, second)
    };

    let mut restarted = RecoveryStore::new(Some(cache));
    assert_eq!(
        restarted
            .expand(&first.blob_ref, Some("raw"), None, None, None, None)
            .content,
        "alpha\n"
    );
    assert_eq!(
        restarted
            .expand(&second.blob_ref, Some("raw"), None, None, None, None)
            .content,
        "beta\n"
    );
}

#[test]
fn expand_preserves_non_newline_terminated_content() {
    // F-004 regression: exact recovery must not add a trailing newline.
    let (mut store, _, _dir) = temp_store();
    let text = "no trailing newline";
    let stored = store
        .store_payload(text, ContentType::Unknown, None, None, None)
        .unwrap();
    let expanded = store.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
    assert_eq!(expanded.content, text);
    let file = store.expand(&stored.file_ref, Some("raw"), None, None, None, None);
    assert_eq!(file.content, text);
}

#[test]
fn range_fragment_selects_lines() {
    let (mut store, _, _dir) = temp_store();
    let stored = store
        .store_payload("a\nb\nc\n", ContentType::Unknown, None, None, None)
        .unwrap();
    let expanded = store.expand(
        &format!("{}#L2-L3", stored.file_ref),
        None,
        None,
        None,
        None,
        None,
    );
    assert_eq!(expanded.content, "b\nc\n");
}

#[test]
fn slice_preserves_trailing_blank_line() {
    // F-001 regression: a slice ending on a blank line keeps that line's newline.
    let (mut store, _, _dir) = temp_store();
    let stored = store
        .store_payload("a\nb\n\nc\n", ContentType::Unknown, None, None, None)
        .unwrap();
    let expanded = store.expand(
        &format!("{}#L1-L3", stored.file_ref),
        None,
        None,
        None,
        None,
        None,
    );
    assert_eq!(expanded.content, "a\nb\n\n");
}

// ---------------------------------------------------------------------------
// Shell outcomes
// ---------------------------------------------------------------------------

#[test]
fn shell_outcome_repeat_detection_tracks_content_and_exit_code() {
    let mut store = RecoveryStore::new(None);
    let first =
        store.record_shell_outcome_deferred(Some("/repo"), "cargo test", "all ok\n", Some(0));
    assert!(!first.unchanged);
    assert_eq!(first.seen, 1);

    let second =
        store.record_shell_outcome_deferred(Some("/repo"), "cargo test", "all ok\n", Some(0));
    assert!(second.unchanged);
    assert_eq!(second.seen, 2);

    let changed_output =
        store.record_shell_outcome_deferred(Some("/repo"), "cargo test", "all ok v2\n", Some(0));
    assert!(!changed_output.unchanged);
    assert_eq!(changed_output.seen, 1);

    let changed_exit =
        store.record_shell_outcome_deferred(Some("/repo"), "cargo test", "all ok v2\n", Some(1));
    assert!(!changed_exit.unchanged);

    let other_scope =
        store.record_shell_outcome_deferred(Some("/other"), "cargo test", "all ok v2\n", Some(1));
    assert!(
        !other_scope.unchanged,
        "scope must partition repeat detection"
    );
}

#[test]
fn shell_outcomes_are_bounded_and_evict_oldest() {
    let mut store = RecoveryStore::new(None);
    for idx in 0..(MAX_SHELL_OUTCOMES + 40) {
        store.record_shell_outcome_deferred(None, &format!("cmd-{idx}"), "out", Some(0));
    }
    assert!(store.state.shell_outcomes.len() <= MAX_SHELL_OUTCOMES);

    let recent = store.record_shell_outcome_deferred(
        None,
        &format!("cmd-{}", MAX_SHELL_OUTCOMES + 39),
        "out",
        Some(0),
    );
    assert!(
        recent.unchanged,
        "most recent entries must survive eviction"
    );

    let evicted = store.record_shell_outcome_deferred(None, "cmd-0", "out", Some(0));
    assert!(!evicted.unchanged, "oldest entry should have been evicted");
}

#[test]
fn shell_outcomes_survive_persistence_roundtrip() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        let first = store
            .record_shell_outcome(Some("/repo"), "git status", "clean\n", Some(0))
            .unwrap();
        assert!(!first.unchanged);
    }

    let mut restarted = RecoveryStore::new(Some(cache));
    let repeat = restarted
        .record_shell_outcome(Some("/repo"), "git status", "clean\n", Some(0))
        .unwrap();
    assert!(repeat.unchanged, "repeat state must survive a restart");
    assert_eq!(repeat.seen, 2);
}

// ---------------------------------------------------------------------------
// Concurrency / multi-writer
// ---------------------------------------------------------------------------

#[test]
fn concurrent_persistence_preserves_all_thread_payloads() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let workers = 8usize;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(workers));
    let handles: Vec<_> = (0..workers)
        .map(|worker| {
            let cache = cache.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut store = RecoveryStore::new(Some(cache));
                barrier.wait();
                let text = format!("payload-{worker}\n");
                let stored = store
                    .store_payload(&text, ContentType::Unknown, None, None, None)
                    .unwrap();
                (stored.blob_ref, text)
            })
        })
        .collect();
    let stored: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();

    let mut restarted = RecoveryStore::new(Some(cache));
    for (blob_ref, text) in stored {
        let expanded = restarted.expand(&blob_ref, Some("raw"), None, None, None, None);
        assert!(expanded.found, "missing {blob_ref}");
        assert_eq!(expanded.content, text);
    }
}

#[test]
fn alternating_writers_on_one_cache_path_still_merge() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let mut left = RecoveryStore::new(Some(cache.clone()));
    let mut right = RecoveryStore::new(Some(cache.clone()));

    let mut expected = Vec::new();
    for round in 0..4 {
        let text = format!("left-{round}\n");
        let stored = left
            .store_payload(&text, ContentType::Unknown, None, None, None)
            .unwrap();
        expected.push((stored.blob_ref, text));
        let text = format!("right-{round}\n");
        let stored = right
            .store_payload(&text, ContentType::Unknown, None, None, None)
            .unwrap();
        expected.push((stored.blob_ref, text));
    }

    left.persist_pending().unwrap();
    right.persist_pending().unwrap();

    let mut restarted = RecoveryStore::new(Some(cache));
    for (blob_ref, text) in &expected {
        for store in [&mut left, &mut right, &mut restarted] {
            let expanded = store.expand(blob_ref, Some("raw"), None, None, None, None);
            assert!(expanded.found, "missing {blob_ref}");
            assert_eq!(&expanded.content, text);
        }
    }
}

#[test]
fn single_writer_repeat_persists_skip_reload_and_stay_byte_exact() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let mut store = RecoveryStore::new(Some(cache.clone()));

    let mut expected = Vec::new();
    for round in 0..4 {
        let text = format!("solo-{round}\n");
        let stored = store
            .store_payload(&text, ContentType::Unknown, None, None, None)
            .unwrap();
        expected.push((stored.blob_ref, text));
        let identity = store.disk_identity.expect("identity captured after write");
        assert_eq!(DiskIdentity::capture(&cache), Some(identity));
    }

    let mut restarted = RecoveryStore::new(Some(cache));
    for (blob_ref, text) in &expected {
        let expanded = restarted.expand(blob_ref, Some("raw"), None, None, None, None);
        assert!(expanded.found, "missing {blob_ref}");
        assert_eq!(&expanded.content, text);
    }
}

// ---------------------------------------------------------------------------
// Load state / IO guards
// ---------------------------------------------------------------------------

#[test]
fn load_state_rejects_reader_growth_past_max_load_bytes() {
    let state = RecoveryState::empty(&RecoveryConfig::default());
    let json = serde_json::to_string(&state).unwrap();
    assert!(json.len() > 8);

    let text = read_limited_utf8(std::io::Cursor::new(json.as_bytes()), json.len() - 1).unwrap();

    assert!(text.is_none());
}

#[test]
fn load_state_ignores_invalid_utf8_cache() {
    // Unit: read_limited_utf8 rejects non-UTF-8 bytes.
    let text = read_limited_utf8(std::io::Cursor::new([0xff]), 16).unwrap();
    assert!(text.is_none());

    // Integration: a cache file containing invalid UTF-8 is treated as empty.
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    fs::write(&cache, [0xff, 0xfe, 0x00]).unwrap();
    let store = RecoveryStore::new(Some(cache));
    assert!(
        store.state.blobs.is_empty(),
        "invalid-UTF-8 cache must load as empty state"
    );
}

// ---------------------------------------------------------------------------
// Temp paths / atomic write
// ---------------------------------------------------------------------------

#[test]
fn recovery_tmp_paths_are_unique_within_process() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");

    let paths: Vec<PathBuf> = (0..100).map(|_| recovery_tmp_path(&cache)).collect();
    let unique: HashSet<&PathBuf> = paths.iter().collect();
    assert_eq!(unique.len(), 100, "all tmp paths must be unique");

    for path in &paths {
        assert_eq!(path.parent(), Some(dir.path()));
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(
            name.starts_with(".cache.json."),
            "name must start with dot-prefixed cache name: {name}"
        );
        assert!(name.ends_with(".tmp"), "name must end with .tmp: {name}");
    }
}

#[test]
fn atomic_write_json_does_not_reuse_stale_temp_path() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let stale_tmp = recovery_tmp_path(&cache);
    fs::write(&stale_tmp, "stale temp").unwrap();

    atomic_write_json(&cache, &RecoveryState::empty(&RecoveryConfig::default())).unwrap();

    assert_eq!(fs::read_to_string(&stale_tmp).unwrap(), "stale temp");
    let text = fs::read_to_string(cache).unwrap();
    assert!(serde_json::from_str::<RecoveryState>(&text).is_ok());
}

// ---------------------------------------------------------------------------
// Eviction
// ---------------------------------------------------------------------------

#[test]
fn evict_prefix_removes_fifo_victims_once() {
    let mut items = BTreeMap::from([
        ("tz://unit/a".to_string(), 1),
        ("tz://unit/b".to_string(), 2),
        ("tz://unit/c".to_string(), 3),
        ("tz://unit/d".to_string(), 4),
    ]);
    let mut order = vec![
        "tz://unit/a".to_string(),
        "tz://unit/a".to_string(),
        "tz://file/live".to_string(),
        "tz://unit/b".to_string(),
        "tz://unit/c".to_string(),
        "tz://unit/d".to_string(),
    ];

    evict_prefix(&mut items, &mut order, "tz://unit/", 2);

    assert_eq!(
        items.keys().cloned().collect::<Vec<_>>(),
        vec!["tz://unit/c".to_string(), "tz://unit/d".to_string(),]
    );
    assert_eq!(
        order,
        vec![
            "tz://file/live".to_string(),
            "tz://unit/c".to_string(),
            "tz://unit/d".to_string(),
        ]
    );
}

#[test]
fn evict_prefix_falls_back_to_key_order_without_order_entries() {
    let mut items = BTreeMap::from([
        ("tz://unit/a".to_string(), 1),
        ("tz://unit/b".to_string(), 2),
        ("tz://unit/c".to_string(), 3),
    ]);
    let mut order = Vec::new();

    evict_prefix(&mut items, &mut order, "tz://unit/", 1);

    assert_eq!(
        items.keys().cloned().collect::<Vec<_>>(),
        vec!["tz://unit/c".to_string()]
    );
    assert!(order.is_empty());
}

/// Covers: blob eviction bounds, file_ref survival after blob eviction,
/// and deferred-store eviction timing. Kills mutations that:
///  - skip eviction in store_payload_deferred
///  - drop file entries when their blob is evicted
///  - mis-count the blob limit
macro_rules! test_eviction_on_overflow {
    ($name:ident, $store_fn:expr) => {
        #[test]
        fn $name() {
            let config = RecoveryConfig {
                max_blobs: 1,
                ..RecoveryConfig::default()
            };
            let mut store = mem_store(config);
            let first = $store_fn(&mut store, "first payload\n");
            let second = $store_fn(&mut store, "second payload\n");

            // Oldest blob evicted, newest retained.
            assert!(
                !store.has_ref(&first.blob_ref),
                "oldest blob must be evicted"
            );
            assert!(store.has_ref(&second.blob_ref), "newest blob must survive");

            // File ref survives blob eviction — stores inline text.
            let expanded = store.expand(&first.file_ref, Some("raw"), None, None, None, None);
            assert!(expanded.found, "file ref must survive blob eviction");
            assert_eq!(expanded.content, "first payload\n");
        }
    };
}

fn do_store(store: &mut RecoveryStore, text: &str) -> StoredPayload {
    store
        .store_payload(text, ContentType::Unknown, None, None, None)
        .unwrap()
}

fn do_deferred(store: &mut RecoveryStore, text: &str) -> StoredPayload {
    store.store_payload_deferred(text, ContentType::Unknown, None, None, None)
}

fn assert_state_keys_match(a: &RecoveryStore, b: &RecoveryStore) {
    assert_eq!(
        a.state.blobs.keys().collect::<Vec<_>>(),
        b.state.blobs.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        a.state.files.keys().collect::<Vec<_>>(),
        b.state.files.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        a.state.units.keys().collect::<Vec<_>>(),
        b.state.units.keys().collect::<Vec<_>>()
    );
    assert_eq!(a.state.order, b.state.order);
}

test_eviction_on_overflow!(eviction_bounds_blob_count, do_store);
test_eviction_on_overflow!(
    deferred_payload_enforces_limits_before_returning,
    do_deferred
);

#[test]
fn batched_deferred_payloads_enforce_limits_on_persist_pending() {
    let config = RecoveryConfig {
        max_blobs: 1,
        ..RecoveryConfig::default()
    };
    let mut store = mem_store(config);

    let first = store.store_payload_deferred_batch("first", ContentType::Unknown, None, None, None);
    let second =
        store.store_payload_deferred_batch("second", ContentType::Unknown, None, None, None);

    assert!(store.has_ref(&first.blob_ref));
    assert!(store.has_ref(&second.blob_ref));

    store.persist_pending().unwrap();

    assert!(!store.has_ref(&first.blob_ref));
    assert!(store.has_ref(&second.blob_ref));
}

#[test]
fn batched_deferred_payloads_match_immediate_final_live_refs() {
    let config = RecoveryConfig {
        max_blobs: 2,
        max_files: 2,
        max_units: 4,
        max_bytes: 32_000,
        ..RecoveryConfig::default()
    };
    let payloads = [
        "alpha payload line one\nalpha payload line two\n",
        "beta payload line one\nbeta payload line two\n",
        "gamma payload line one\ngamma payload line two\n",
        "delta payload line one\ndelta payload line two\n",
    ];
    let mut immediate = mem_store(config.clone());
    for payload in payloads {
        immediate.store_payload_deferred(payload, ContentType::Unknown, None, None, None);
    }

    let mut batched = mem_store(config);
    for payload in payloads {
        batched.store_payload_deferred_batch(payload, ContentType::Unknown, None, None, None);
    }
    batched.persist_pending().unwrap();

    assert_state_keys_match(&immediate, &batched);
}

#[test]
fn deferred_search_output_enforces_limits_before_returning() {
    let config = RecoveryConfig {
        max_search_hits: 1,
        ..RecoveryConfig::default()
    };
    let mut store = mem_store(config);

    let refs = store.store_search_output_deferred("needle one\nneedle two\n", Some("needle"));

    assert_eq!(refs.len(), 2);
    assert!(!store.has_ref(&refs[0]));
    assert!(store.has_ref(&refs[1]));
}

// ---------------------------------------------------------------------------
// File refs / stale detection / non-UTF-8 paths
// ---------------------------------------------------------------------------

#[test]
fn file_ref_reports_stale_after_source_changes() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("a.txt");
    fs::write(&source, "one\n").unwrap();
    let cache = dir.path().join("cache.json");
    let stored = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload("one\n", ContentType::Unknown, Some(&source), None, None)
            .unwrap()
    };
    fs::write(&source, "two\n").unwrap();
    let mut restarted = RecoveryStore::new(Some(cache));
    let expanded = restarted.expand(&stored.file_ref, Some("raw"), None, None, None, None);
    assert!(!expanded.found);
    assert_eq!(expanded.reason, "stale-ref");
}

#[test]
fn stale_check_uses_native_path_identity_before_display_path() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.txt");
    fs::write(&source, "same\n").unwrap();
    let source_fingerprint = source_fingerprint(&source).unwrap();
    let ref_id = "tz://file/manual".to_string();
    let mut store = RecoveryStore::with_config(None, RecoveryConfig::default());

    store.state.files.insert(
        ref_id.clone(),
        StoredFile {
            ref_id: ref_id.clone(),
            path: Some(
                dir.path()
                    .join("missing-display-path.txt")
                    .display()
                    .to_string(),
            ),
            path_identity: Some(path_identity_text(&source)),
            text: "same\n".to_string(),
            content_type: ContentType::Unknown.to_string(),
            source_fingerprint: Some(source_fingerprint),
            source_start_line: None,
            source_end_line: None,
        },
    );

    assert!(
        !store.file_ref_is_stale(&ref_id),
        "native path identity must drive stale checks when display text is lossy"
    );
}

#[cfg(unix)]
#[test]
fn file_refs_distinguish_non_utf8_path_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let path_a = PathBuf::from(OsString::from_vec(b"/tmp/source-\xff.rs".to_vec()));
    let path_b = PathBuf::from(OsString::from_vec(b"/tmp/source-\xfe.rs".to_vec()));
    let mut store = RecoveryStore::with_config(None, RecoveryConfig::default());

    let stored_a = store
        .store_payload("same\n", ContentType::Unknown, Some(&path_a), None, None)
        .unwrap();
    let stored_b = store
        .store_payload("same\n", ContentType::Unknown, Some(&path_b), None, None)
        .unwrap();
    let (_, expected_a) = RecoveryStore::expected_refs("same\n", Some(&path_a));
    let (_, expected_b) = RecoveryStore::expected_refs("same\n", Some(&path_b));

    assert_eq!(stored_a.file_ref, expected_a);
    assert_eq!(stored_b.file_ref, expected_b);
    assert_ne!(stored_a.file_ref, stored_b.file_ref);

    // Implicit roundtrip: expand recovers the original content through the
    // non-UTF-8 path identity encoding.
    let expanded_a = store.expand(&stored_a.file_ref, Some("raw"), None, None, None, None);
    assert!(expanded_a.found, "non-UTF-8 path ref must expand");
    assert_eq!(expanded_a.content, "same\n");
    let expanded_b = store.expand(&stored_b.file_ref, Some("raw"), None, None, None, None);
    assert!(expanded_b.found);
    assert_eq!(expanded_b.content, "same\n");
}

#[cfg(unix)]
#[test]
fn recovery_sidecar_paths_preserve_non_utf8_file_name_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let dir = tempdir().unwrap();
    let mut file_name = b"cache.".to_vec();
    file_name.push(0xff);
    let path = dir.path().join(OsString::from_vec(file_name.clone()));

    let lock_name = recovery_lock_path(&path)
        .file_name()
        .expect("lock sidecar has file name")
        .as_bytes()
        .to_vec();
    let tmp_name = recovery_tmp_path(&path)
        .file_name()
        .expect("tmp sidecar has file name")
        .as_bytes()
        .to_vec();
    let plain_lock_name = recovery_lock_path(&dir.path().join("cache"))
        .file_name()
        .expect("plain lock sidecar has file name")
        .as_bytes()
        .to_vec();

    let mut expected_lock = file_name.clone();
    expected_lock.extend_from_slice(b".lock");
    let mut expected_tmp_prefix = b".".to_vec();
    expected_tmp_prefix.extend_from_slice(&file_name);
    expected_tmp_prefix.push(b'.');

    assert_eq!(lock_name, expected_lock);
    assert_ne!(lock_name, plain_lock_name);
    assert!(tmp_name.starts_with(&expected_tmp_prefix), "{tmp_name:?}");
    assert!(tmp_name.ends_with(b".tmp"), "{tmp_name:?}");
}

// ---------------------------------------------------------------------------
// Tmp sweep
// ---------------------------------------------------------------------------

/// Parametrized: real sweep and dry-run mode share the same fixture.
/// Kills mutations that unlink during dry_run, mis-count scanned/removed,
/// or reclaim files that are fresh, lock anchors, or unrelated.
fn tmp_sweep_fixture(dry_run: bool) {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let stale_age = STALE_TMP_MAX_AGE * 2;
    let (hidden, legacy, fresh, lock, unrelated) =
        setup_tmp_sweep_files(dir.path(), "recovery-cache.json", stale_age);
    let report = sweep_stale_tmp_files(&cache, STALE_TMP_MAX_AGE, dry_run);
    assert_tmp_sweep_report(
        &report, dry_run, &hidden, &legacy, &fresh, &lock, &unrelated,
    );
}

#[test]
fn tmp_sweep_reclaims_stale_orphans_of_both_shapes_only() {
    tmp_sweep_fixture(false);
}

#[test]
fn tmp_sweep_dry_run_counts_without_unlinking() {
    tmp_sweep_fixture(true);
}

#[test]
fn tmp_sweep_missing_dir_is_empty_report() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("missing").join("recovery-cache.json");
    let report = sweep_stale_tmp_files(&cache, STALE_TMP_MAX_AGE, false);
    assert!(!report.dry_run);
    assert_eq!(report.scanned, 0);
    assert_eq!(report.removed, 0);
    assert_eq!(report.removed_bytes, 0);
    assert_eq!(report.failed, 0);
}

// ---------------------------------------------------------------------------
// Journal / compaction
// ---------------------------------------------------------------------------

#[test]
fn second_process_persist_appends_journal_without_snapshot_rewrite() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let first = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload("alpha\n", ContentType::Unknown, None, None, None)
            .unwrap()
    };
    let snapshot_before = fs::read(&cache).unwrap();

    let second = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload("beta\n", ContentType::Unknown, None, None, None)
            .unwrap()
    };
    assert_eq!(
        fs::read(&cache).unwrap(),
        snapshot_before,
        "snapshot must be untouched by a journaled persist"
    );
    assert!(journal_path(&cache).exists(), "journal sibling must exist");

    let mut restarted = RecoveryStore::new(Some(cache));
    for (stored, text) in [(&first, "alpha\n"), (&second, "beta\n")] {
        let expanded = restarted.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
        assert!(expanded.found);
        assert_eq!(expanded.content, text);
    }
}

#[test]
fn foreign_journal_append_forces_merge_and_nothing_is_lost() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload("base\n", ContentType::Unknown, None, None, None)
            .unwrap();
    }
    let mut a = RecoveryStore::new(Some(cache.clone()));
    let mut b = RecoveryStore::new(Some(cache.clone()));
    let from_b = b
        .store_payload("from-b\n", ContentType::Unknown, None, None, None)
        .unwrap();
    let from_a = a
        .store_payload("from-a\n", ContentType::Unknown, None, None, None)
        .unwrap();

    let mut restarted = RecoveryStore::new(Some(cache));
    for (stored, text) in [(&from_b, "from-b\n"), (&from_a, "from-a\n")] {
        let expanded = restarted.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
        assert!(
            expanded.found,
            "lost {} after concurrent persists",
            stored.blob_ref
        );
        assert_eq!(expanded.content, text);
    }
}

#[test]
fn corrupt_journal_tail_keeps_complete_entries() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload("alpha\n", ContentType::Unknown, None, None, None)
            .unwrap();
    }
    let good = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload("good\n", ContentType::Unknown, None, None, None)
            .unwrap()
    };
    let journal = journal_path(&cache);
    let mut bytes = fs::read(&journal).unwrap();
    bytes.extend_from_slice(b"{\"refs\":[\"tz://blob/torn");
    fs::write(&journal, bytes).unwrap();

    let mut restarted = RecoveryStore::new(Some(cache));
    let expanded = restarted.expand(&good.blob_ref, Some("raw"), None, None, None, None);
    assert!(
        expanded.found,
        "complete journal entry poisoned by torn tail"
    );
    assert_eq!(expanded.content, "good\n");
}

#[test]
fn oversized_journal_compacts_into_fresh_snapshot() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let small = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload("tiny\n", ContentType::Unknown, None, None, None)
            .unwrap()
    };
    let big_text = "x".repeat(80 * 1024);
    let big = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload(&big_text, ContentType::Unknown, None, None, None)
            .unwrap()
    };
    assert!(
        !journal_path(&cache).exists(),
        "journal must be removed after compaction"
    );
    let mut restarted = RecoveryStore::new(Some(cache));
    for (stored, text) in [(&small, "tiny\n"), (&big, big_text.as_str())] {
        let expanded = restarted.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
        assert!(expanded.found);
        assert_eq!(expanded.content, text);
    }
}

// ---------------------------------------------------------------------------
// Big blob / sidecar
// ---------------------------------------------------------------------------

#[test]
fn big_blob_externalizes_to_sidecar_and_roundtrips() {
    let (mut store, cache, _dir) = temp_store();
    let big = "x".repeat(200 * 1024);
    let stored = store
        .store_payload(&big, ContentType::Unknown, None, None, None)
        .unwrap();
    let sidecar = blob_sidecar_dir(&cache);
    assert!(sidecar.is_dir(), "sidecar dir must exist");
    assert!(
        fs::read_dir(&sidecar).unwrap().count() >= 1,
        "sidecar must hold the payload"
    );
    let snapshot: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cache).unwrap()).unwrap();
    let blob_value = snapshot["blobs"][&stored.blob_ref].as_str().unwrap();
    assert!(
        blob_value.starts_with('\u{0}'),
        "blob value must be an externalized marker"
    );
    assert!(blob_value.len() < 128, "marker must be tiny");
    drop(store);
    let mut restarted = RecoveryStore::new(Some(cache));
    let expanded = restarted.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
    assert!(expanded.found);
    assert_eq!(expanded.content, big);
}

#[test]
fn corrupt_blob_sidecar_is_a_miss_not_bad_bytes() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let big = "y".repeat(200 * 1024);
    let stored = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload(&big, ContentType::Unknown, None, None, None)
            .unwrap()
    };
    for entry in fs::read_dir(blob_sidecar_dir(&cache)).unwrap() {
        fs::write(entry.unwrap().path(), "tampered").unwrap();
    }
    let mut restarted = RecoveryStore::new(Some(cache));
    let expanded = restarted.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
    assert!(
        !expanded.found || expanded.content != big,
        "tampered sidecar must never serve as the original"
    );
    assert!(
        !expanded.content.contains("tampered") || !expanded.found,
        "tampered bytes must not be served as blob content"
    );
}

// ---------------------------------------------------------------------------
// Proptest
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn arbitrary_payload_roundtrips(text in ".*") {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("cache.json");
        let stored = {
            let mut store = RecoveryStore::new(Some(cache.clone()));
            store.store_payload(&text, ContentType::Unknown, None, None, None).unwrap()
        };
        let mut restarted = RecoveryStore::new(Some(cache));
        let expanded = restarted.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
        prop_assert!(expanded.found);
        prop_assert_eq!(&expanded.content, &text);
        let file_expanded = restarted.expand(&stored.file_ref, Some("raw"), None, None, None, None);
        prop_assert!(file_expanded.found);
        prop_assert_eq!(&file_expanded.content, &text);
    }

    #[test]
    fn generated_around_selectors_do_not_panic(line in any::<usize>(), radius in any::<usize>()) {
        let text = "a\nb\nc\n";
        let selector = format!("around:L{line}:{radius}");
        let selected = select_content(text, Some(&selector), None, None, None, None);

        let segments: Vec<&str> = text.split_inclusive('\n').collect();
        let num_lines = segments.len();
        let start = line.saturating_sub(radius).max(1);
        let end = line.saturating_add(radius);
        let expected = if start > num_lines {
            String::new()
        } else {
            let lo = start - 1;
            let hi = end.min(num_lines);
            segments[lo..hi].concat()
        };
        prop_assert_eq!(selected, expected);
    }
}
