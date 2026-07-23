use super::*;
use crate::shared_cas::SharedCas;
use proptest::prelude::*;
use std::collections::BTreeMap;
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
fn session_visible_alias_expands_after_store_restart() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let text = "durable session alias payload";
    let alias = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        let stored = store
            .store_payload(text, ContentType::Code, None, None, None)
            .unwrap();
        store.ensure_session_visible_alias(&stored.blob_ref)
    };

    assert!(alias.starts_with("tz://s/"));
    let mut restarted = RecoveryStore::new(Some(cache));
    let expanded = restarted.expand(&alias, Some("raw"), None, None, None, None);
    assert!(expanded.found, "{}", expanded.reason);
    assert_eq!(expanded.content, text);
}

#[test]
fn ambiguous_alias_persists_when_pending_changes_flush() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store.mark_ambiguous("tz://blob/ambiguous");
        store.persist_pending().unwrap();
    }

    let restarted = RecoveryStore::new(Some(cache));
    assert!(restarted.is_alias_ambiguous("tz://blob/ambiguous"));
}

#[test]
fn old_string_blob_cache_round_trips_without_shape_rewrite() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let text = "legacy
bytes
";
    let ref_id = format!("tz://blob/{}", sha256_hex(text));
    let mut json = serde_json::to_value(RecoveryState::empty(&RecoveryConfig::default())).unwrap();
    json["blobs"][&ref_id] = serde_json::Value::String(text.to_string());
    json["order"] = serde_json::json!([ref_id]);
    fs::write(&cache, serde_json::to_vec(&json).unwrap()).unwrap();

    let mut store = RecoveryStore::new(Some(cache.clone()));
    let expanded = store.expand(&ref_id, Some("raw"), None, None, None, None);
    assert!(expanded.found);
    assert_eq!(expanded.content, text);
    store.persist_pending().unwrap();

    let persisted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(cache).unwrap()).unwrap();
    assert_eq!(persisted["blobs"][&ref_id].as_str(), Some(text));
}

#[test]
fn file_backed_blob_resolves_exact_source_slice_after_restart() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.txt");
    let cache = dir.path().join("cache.json");
    fs::write(
        &source,
        "one
two
three
",
    )
    .unwrap();
    let ref_id = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_file_backed_blob(&source, 2, 3, ContentType::Code)
            .unwrap()
    };
    let snapshot: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cache).unwrap()).unwrap();
    assert!(snapshot["blobs"][&ref_id].is_object());

    let mut restarted = RecoveryStore::new(Some(cache));
    let expanded = restarted.expand(&ref_id, Some("raw"), None, None, None, None);
    assert!(expanded.found);
    assert_eq!(
        expanded.content,
        "two
three
"
    );
}

#[test]
fn file_backed_blob_reports_stale_when_source_changes_or_disappears() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.txt");
    let cache = dir.path().join("cache.json");
    fs::write(
        &source, "stable
",
    )
    .unwrap();
    let ref_id = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_file_backed_blob(&source, 1, 1, ContentType::Unknown)
            .unwrap()
    };

    fs::write(
        &source, "changed
",
    )
    .unwrap();
    let mut changed = RecoveryStore::new(Some(cache.clone()));
    let expanded = changed.expand(&ref_id, Some("raw"), None, None, None, None);
    assert!(!expanded.found);
    assert_eq!(expanded.reason, "stale-ref");

    fs::remove_file(&source).unwrap();
    let mut missing = RecoveryStore::new(Some(cache));
    let expanded = missing.expand(&ref_id, Some("raw"), None, None, None, None);
    assert!(!expanded.found);
    assert_eq!(expanded.reason, "stale-ref");
}

#[test]
fn source_backed_payload_avoids_inline_file_copy_and_detects_staleness() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("large.txt");
    let cache = dir.path().join("cache.json");
    let text = format!("{}\n", "stable payload ".repeat(8_000));
    fs::write(&source, &text).unwrap();

    let stored = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        let stored =
            store.store_source_backed_payload_deferred_batch(&text, ContentType::Unknown, &source);
        store.persist_pending().unwrap();
        stored
    };
    let snapshot: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cache).unwrap()).unwrap();
    assert!(snapshot["blobs"][&stored.blob_ref].is_object());
    assert_eq!(snapshot["files"][&stored.file_ref]["source_backed"], true);
    assert_eq!(snapshot["files"][&stored.file_ref]["text"], "");

    let mut restarted = RecoveryStore::new(Some(cache.clone()));
    for ref_id in [&stored.blob_ref, &stored.file_ref] {
        let expanded = restarted.expand(ref_id, Some("raw"), None, None, None, None);
        assert!(expanded.found);
        assert_eq!(expanded.content, text);
    }

    fs::write(&source, "changed\n").unwrap();
    let mut changed = RecoveryStore::new(Some(cache));
    for ref_id in [&stored.blob_ref, &stored.file_ref] {
        let expanded = changed.expand(ref_id, Some("raw"), None, None, None, None);
        assert!(!expanded.found);
        assert_eq!(expanded.reason, "stale-ref");
    }
}

#[test]
fn inline_blob_survives_source_deletion() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("ephemeral.txt");
    let cache = dir.path().join("cache.json");
    let text = "ephemeral bytes
";
    fs::write(&source, text).unwrap();
    let blob_ref = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload(text, ContentType::Unknown, Some(&source), None, None)
            .unwrap()
            .blob_ref
    };
    fs::remove_file(source).unwrap();

    let mut restarted = RecoveryStore::new(Some(cache));
    let expanded = restarted.expand(&blob_ref, Some("raw"), None, None, None, None);
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
fn ref_index_pay_once_reuses_one_user_cas_object_across_sessions() {
    let index_dir = tempdir().unwrap();
    with_ref_index_env(index_dir.path(), true, || {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let cache_a = dir_a.path().join("cache.json");
        let cache_b = dir_b.path().join("cache.json");
        let payload = "pay once across sessions
with exact bytes
";

        let first = {
            let mut store = RecoveryStore::new(Some(cache_a.clone()));
            store
                .store_payload(payload, ContentType::Unknown, None, None, None)
                .unwrap()
        };
        let second = {
            let mut store = RecoveryStore::new(Some(cache_b.clone()));
            store
                .store_payload(payload, ContentType::Unknown, None, None, None)
                .unwrap()
        };

        assert_eq!(first.blob_ref, second.blob_ref);
        let hash = ref_index_id_part(&first.blob_ref).unwrap();
        let cas = SharedCas::new(index_dir.path().to_path_buf());
        assert!(cas.contains(hash));
        let object_dir = index_dir
            .path()
            .join("blobs")
            .join("sha256")
            .join(&hash[..2]);
        assert_eq!(fs::read_dir(object_dir).unwrap().count(), 1);

        for cache in [&cache_a, &cache_b] {
            let snapshot: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(cache).unwrap()).unwrap();
            assert_eq!(snapshot["blobs"].as_object().unwrap().len(), 0);
        }

        let shard = ref_index_shard_path(index_dir.path(), &first.blob_ref);
        let entries =
            ref_index_entries_for_ref(&fs::read_to_string(shard).unwrap(), &first.blob_ref);
        assert_eq!(entries.len(), 2);
        assert!(matches!(
            resolve_blob_from_ref_index(&first.blob_ref, &RecoveryConfig::default()),
            RefResolve::Found(content) if content == payload
        ));
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
        // Remove the SharedCas blob so expansion can only reach the ref index.
        let _ = fs::remove_dir_all(index_dir.path().join("blobs"));

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
    append_ref_index_line(
        &shard,
        ref_id,
        Path::new("/old-store.json"),
        1,
        ContentClass::Unknown,
        false,
        0,
        None,
    )
    .unwrap();
    append_ref_index_line(
        &shard,
        "tz://blob/ba_other",
        Path::new("/other-store.json"),
        1,
        ContentClass::Unknown,
        false,
        0,
        None,
    )
    .unwrap();
    append_ref_index_line(
        &shard,
        ref_id,
        Path::new("/new-store.json"),
        2,
        ContentClass::Unknown,
        false,
        0,
        None,
    )
    .unwrap();

    compact_ref_index_shard(&shard).unwrap();

    let text = fs::read_to_string(shard).unwrap();
    assert!(text.contains("/new-store.json"));
    assert!(text.contains("/other-store.json"));
    assert!(!text.contains("/old-store.json"));
    assert_eq!(text.lines().count(), 2);
}

#[test]
fn repeated_persist_of_same_blob_ref_does_not_duplicate_newest_store_entry() {
    let index_dir = tempdir().unwrap();
    with_ref_index_env(index_dir.path(), true, || {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("cache.json");
        let text = "same ref repeated\n";
        let mut store = RecoveryStore::new(Some(cache));
        let stored = store
            .store_payload(text, ContentType::Unknown, None, None, None)
            .unwrap();
        let repeated = store
            .store_payload(text, ContentType::Unknown, None, None, None)
            .unwrap();
        assert_eq!(stored.blob_ref, repeated.blob_ref);

        let shard = ref_index_shard_path(index_dir.path(), &stored.blob_ref);
        let shard_text = fs::read_to_string(shard).unwrap();
        let matching = shard_text
            .lines()
            .filter(|line| line.contains(&stored.blob_ref))
            .count();
        assert_eq!(
            matching, 1,
            "newest same-store entry should be append-deduped"
        );
    });
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
                    append_blob_refs_to_ref_index(&store_path, &[ref_id], None);
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
            source_backed: false,
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

fn rotated_journal_fixture() -> (tempfile::TempDir, PathBuf, StoredPayload, StoredPayload) {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload("base\n", ContentType::Unknown, None, None, None)
            .unwrap();
    }

    let make_entry = |text: &str| {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        let stored = store.store_payload_deferred(text, ContentType::Unknown, None, None, None);
        let entry = JournalEntry {
            refs: store.session_refs.clone(),
            state: session_delta(&store.state, &store.session_refs, &store.config),
            deleted_blob_refs: Vec::new(),
            deleted_aliases: Vec::new(),
        };
        (stored, entry)
    };
    let (first, first_entry) = make_entry("sealed\n");
    let (second, second_entry) = make_entry("active\n");
    let first_len = serde_json::to_vec(&first_entry).unwrap().len() as u64 + 1;
    let second_len = serde_json::to_vec(&second_entry).unwrap().len() as u64 + 1;
    let segment_limit = first_len.max(second_len);

    assert_eq!(
        append_journal(&cache, &first_entry, segment_limit).unwrap(),
        JournalAppend::Appended
    );
    assert_eq!(
        append_journal(&cache, &second_entry, segment_limit).unwrap(),
        JournalAppend::Appended
    );
    (dir, cache, first, second)
}

#[test]
fn journal_rotates_when_active_segment_reaches_limit() {
    let (_dir, cache, _first, _second) = rotated_journal_fixture();
    assert!(
        journal_segment_path(&cache, 1).exists(),
        "full active segment must be sealed"
    );
    assert!(
        journal_path(&cache).exists(),
        "rotation must leave a writable active segment"
    );
    assert!(
        !journal_segment_path(&cache, 2).exists(),
        "one rotation must create exactly one sealed segment"
    );
}

#[test]
fn journal_replays_sealed_segments_before_active_segment() {
    let (_dir, cache, sealed, active) = rotated_journal_fixture();
    let mut restarted = RecoveryStore::new(Some(cache));
    for (stored, text) in [(&sealed, "sealed\n"), (&active, "active\n")] {
        let expanded = restarted.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
        assert!(expanded.found, "journal segment lost {}", stored.blob_ref);
        assert_eq!(expanded.content, text);
    }
}

#[test]
fn journal_torn_tail_in_newest_segment_preserves_all_complete_segments() {
    let (_dir, cache, sealed, active) = rotated_journal_fixture();
    let mut file = OpenOptions::new()
        .append(true)
        .open(journal_path(&cache))
        .unwrap();
    file.write_all(b"{\"refs\":[\"tz://blob/torn").unwrap();
    drop(file);

    let mut restarted = RecoveryStore::new(Some(cache));
    for (stored, text) in [(&sealed, "sealed\n"), (&active, "active\n")] {
        let expanded = restarted.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
        assert!(
            expanded.found,
            "torn newest tail poisoned {}",
            stored.blob_ref
        );
        assert_eq!(expanded.content, text);
    }
}

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
fn torn_deferred_batch_never_exposes_partial_aliases() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let target = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        let stored = store.store_payload_deferred_batch(
            "committed\n",
            ContentType::Unknown,
            None,
            None,
            None,
        );
        store.store_alias_deferred("tz://batch/committed", &stored.blob_ref);
        store.persist_pending_durable().unwrap();
        stored.blob_ref
    };

    let journal = journal_path(&cache);
    let torn = format!(
        "{{\"refs\":[],\"state\":{{\"aliases\":{{\"tz://batch/torn\":\"{target}\""
    );
    fs::write(&journal, torn).unwrap();

    let mut restarted = RecoveryStore::new(Some(cache));
    let committed = restarted.expand(
        "tz://batch/committed",
        Some("raw"),
        None,
        None,
        None,
        None,
    );
    assert!(committed.found, "{}", committed.reason);
    assert_eq!(committed.content, "committed\n");
    let partial = restarted.expand("tz://batch/torn", Some("raw"), None, None, None, None);
    assert!(!partial.found, "torn alias batch became visible");
}

#[test]
fn durable_batch_propagates_before_during_and_final_sync_failures() {
    for point in [
        DurableCommitFailPoint::BeforePersist,
        DurableCommitFailPoint::BeforeFileSync,
        DurableCommitFailPoint::BeforeDirectorySync,
    ] {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("cache.json");
        let mut store = RecoveryStore::new(Some(cache));
        let stored = store.store_payload_deferred_batch(
            "faulted\n",
            ContentType::Unknown,
            None,
            None,
            None,
        );
        store.store_alias_deferred("tz://batch/faulted", &stored.blob_ref);
        DURABLE_COMMIT_FAIL_POINT.with(|configured| configured.set(Some(point)));
        let error = store.persist_pending_durable().unwrap_err();
        DURABLE_COMMIT_FAIL_POINT.with(|configured| configured.set(None));
        assert!(error.to_string().contains("durable commit fault injected"));
    }
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

#[test]
fn streaming_chunk_boundary_preserves_exact_utf8_and_file_ref_bytes() {
    let dir = tempdir().unwrap();
    let payload_path = dir.path().join("boundary.txt");
    let mut payload = "a".repeat(STREAM_READ_BUFFER_BYTES - 1);
    payload.push('é');
    payload.push_str(&"z".repeat(STREAM_READ_BUFFER_BYTES + 3));
    fs::write(&payload_path, &payload).unwrap();

    let (streamed, hash) = read_utf8_hashed(&payload_path, Some(payload.len())).unwrap();
    assert_eq!(streamed, payload);
    assert_eq!(hash, sha256_hex(&payload));

    let line_path = dir.path().join("lines.txt");
    let first = "x".repeat(STREAM_READ_BUFFER_BYTES - 2);
    let selected = format!("{}\nthird\n", "y".repeat(STREAM_READ_BUFFER_BYTES + 7));
    fs::write(&line_path, format!("{first}\n{selected}tail")).unwrap();
    let (streamed_lines, line_hash) = read_utf8_line_range_hashed(&line_path, 2, 3).unwrap();
    assert_eq!(streamed_lines, selected);
    assert_eq!(line_hash, sha256_hex(&selected));
}

#[test]
fn streaming_corrupt_sidecar_is_rejected_as_decode_failure() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let payload = "verified".repeat(BLOB_EXTERNALIZE_MIN_BYTES / 4);
    let stored = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload(&payload, ContentType::Unknown, None, None, None)
            .unwrap()
    };
    let sidecar = fs::read_dir(blob_sidecar_dir(&cache))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(sidecar, "tampered").unwrap();

    let expanded = RecoveryStore::new(Some(cache)).expand(
        &stored.blob_ref,
        Some("raw"),
        None,
        None,
        None,
        None,
    );
    assert!(!expanded.found);
    assert_eq!(expanded.reason, "decode-failed");
    assert!(expanded.content.is_empty());
}

#[test]
fn streaming_externalization_threshold_keeps_small_payload_inline() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let small = "s".repeat(BLOB_EXTERNALIZE_MIN_BYTES - 1);
    let at_threshold = "b".repeat(BLOB_EXTERNALIZE_MIN_BYTES);
    let mut store = RecoveryStore::new(Some(cache));
    let small_ref = store
        .store_payload(&small, ContentType::Unknown, None, None, None)
        .unwrap()
        .blob_ref;
    let big_ref = store
        .store_payload(&at_threshold, ContentType::Unknown, None, None, None)
        .unwrap()
        .blob_ref;

    assert_eq!(
        store.state.blobs.get(&small_ref),
        Some(&BlobEntry::Inline(small))
    );
    let Some(BlobEntry::Inline(marker)) = store.state.blobs.get(&big_ref) else {
        panic!("threshold payload must use an inline sidecar marker");
    };
    assert!(marker.starts_with(BLOB_MARKER_PREFIX));
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
        let selected = select_content(text.to_string(), Some(&selector), None, None, None, None);

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

// ---------------------------------------------------------------------------
// Same-store scheme alias expand (fz:// / gz:// rewritten to tz://) — cqr.1
// ---------------------------------------------------------------------------

#[test]
fn same_store_scheme_alias_fz_gz_blob_expand_byte_exact() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "cross-scheme payload\nline two\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    assert!(stored.blob_ref.starts_with("tz://blob/"));
    let id = stored.blob_ref.strip_prefix("tz://blob/").unwrap();
    let fz = format!("fz://blob/{id}");
    let gz = format!("gz://blob/{id}");

    for scheme_ref in [&fz, &gz, &stored.blob_ref] {
        let expanded = store.expand(scheme_ref, Some("raw"), None, None, None, None);
        assert!(
            expanded.found,
            "scheme ref must expand: {scheme_ref} reason={}",
            expanded.reason
        );
        assert_eq!(expanded.content, payload);
        assert_eq!(expanded.ref_id, *scheme_ref);
    }
}

#[test]
fn foreign_non_blob_ref_is_not_reinterpreted_as_tokenzero_key() {
    let (mut store, _cache, _dir) = temp_store();
    let via_fz = "fz://codemode/execution/test-exec-1/error";
    let expanded = store.expand(via_fz, Some("raw"), None, None, None, None);
    assert!(!expanded.found);
    assert_eq!(expanded.reason, "unsupported-ref-kind");
    assert_eq!(expanded.ref_id, via_fz);
}
#[test]
fn garbage_scheme_is_invalid_ref_with_full_id_preserved() {
    let (mut store, _cache, _dir) = temp_store();
    let long = "xx://blob/b0123456789abcdef0123456789abcdef_extra_hash_tail";
    let expanded = store.expand(long, Some("raw"), None, None, None, None);
    assert!(!expanded.found);
    assert_eq!(expanded.reason, "invalid-ref");
    assert_eq!(
        expanded.ref_id, long,
        "requested ref must be preserved in full (no mid-hash truncation)"
    );
}

#[test]
fn canonicalize_and_is_expandable_helpers() {
    assert!(is_expandable_ref("tz://blob/babc"));
    assert!(is_expandable_ref("fz://blob/babc"));
    assert!(is_expandable_ref("gz://blob/babc"));
    assert!(!is_expandable_ref("http://blob/babc"));
    assert!(!is_expandable_ref("not-a-ref"));
    assert_eq!(
        canonicalize_expand_ref("fz://blob/babc").as_deref(),
        Some("tz://blob/babc")
    );
    assert!(canonicalize_expand_ref("gz://codemode/execution/x/error").is_none());
    assert!(canonicalize_expand_ref("http://nope").is_none());
}

fn canonical_shared_store() -> (RecoveryStore, PathBuf, tempfile::TempDir, SharedCas) {
    let dir = tempdir().unwrap();
    let root = dir.path().join(".zerostack");
    fs::create_dir_all(root.join("blobs").join("sha256")).unwrap();
    let cache_dir = root.join("tokenzero");
    fs::create_dir_all(&cache_dir).unwrap();
    let cache = cache_dir.join("recovery-cache.json");
    let cas = SharedCas::new(root.clone());
    let store = RecoveryStore::new(Some(cache.clone()));
    (store, cache, dir, cas)
}

// ---------------------------------------------------------------------------
// #B/#L fragment algebra (cqr.5)
// ---------------------------------------------------------------------------

#[test]
fn canonical_shared_store_serves_full_refs_and_fragments() {
    let (mut store, _cache, _dir, cas) = canonical_shared_store();
    let payload = b"alpha
beta
gamma
";
    let full_hash = cas.publish(payload).unwrap();
    let tz = format!("tz://blob/{full_hash}");
    let fz = format!("fz://blob/{full_hash}");
    let gz = format!("gz://blob/{full_hash}");

    for scheme in [&tz, &fz, &gz] {
        let expanded = store.expand(scheme, Some("raw"), None, None, None, None);
        assert!(expanded.found);
        assert_eq!(expanded.content.as_bytes(), payload);
        assert_eq!(expanded.ref_id, *scheme);
    }

    let b_ref = format!("{tz}#B0-5");
    let expanded_b = store.expand(&b_ref, Some("raw"), None, None, None, None);
    assert!(expanded_b.found);
    assert_eq!(expanded_b.content, "alpha");
    assert_eq!(expanded_b.ref_id, b_ref);

    let l_ref = format!("{tz}#L2-2");
    let expanded_l = store.expand(&l_ref, Some("raw"), None, None, None, None);
    assert!(expanded_l.found);
    assert_eq!(
        expanded_l.content,
        "beta
"
    );
    assert_eq!(expanded_l.ref_id, l_ref);
}

#[test]
fn shared_cas_missing_on_disjoint_roots() {
    let text = "alpha
beta
gamma
";
    let (mut producer, _cache, _dir, cas) = canonical_shared_store();
    let full_hash = cas.publish(text.as_bytes()).unwrap();
    let full_ref = format!("fz://blob/{full_hash}");
    let (mut consumer, _consumer_cache, _consumer_dir, _consumer_cas) = canonical_shared_store();

    let missing = consumer.expand(&full_ref, Some("raw"), None, None, None, None);
    assert!(!missing.found);
    assert_eq!(missing.reason, "shared-cas-missing");
    assert_eq!(missing.ref_id, full_ref);

    let present = producer.expand(&full_ref, Some("raw"), None, None, None, None);
    assert!(present.found);
    assert_eq!(present.content, text);
    assert_eq!(present.ref_id, full_ref);
}

#[test]
fn fz_blob_ref_falls_back_to_fszero_sibling_store() {
    // fszero-fz-ref-expand-broken-izj regression: an fz:// blob ref minted by
    // the fszero engine and stored only in the fszero JSON store must be
    // expandable by the tokenzero engine under the same unified ZeroStack root.
    let dir = tempdir().unwrap();
    let root = dir.path().join(".zerostack");
    let fszero_cache = root.join("fszero").join("recovery-cache.json");
    let tokenzero_cache = root.join("tokenzero").join("recovery-cache.json");
    fs::create_dir_all(fszero_cache.parent().unwrap()).unwrap();
    fs::create_dir_all(tokenzero_cache.parent().unwrap()).unwrap();

    let payload = "cross-engine blob from fszero
second line
";
    let fz_ref = format!("fz://blob/{}", tokenzero_core::sha256_hex(payload));

    // Store the payload using a flat cache path so it is written to the JSON
    // store rather than published to the shared CAS, then move the snapshot into
    // the unified fszero layout.
    let fszero_temp = dir.path().join("fszero-cache.json");
    let mut fszero_store = RecoveryStore::new(Some(fszero_temp.clone()));
    fszero_store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    fs::create_dir_all(fszero_cache.parent().unwrap()).unwrap();
    fs::rename(&fszero_temp, &fszero_cache).unwrap();

    let mut tokenzero_store = RecoveryStore::new(Some(tokenzero_cache));
    let expanded = tokenzero_store.expand(&fz_ref, Some("raw"), None, None, None, None);
    assert!(
        expanded.found,
        "fz blob ref must expand via sibling fszero store: reason={}",
        expanded.reason
    );
    assert_eq!(expanded.content, payload);
    assert_eq!(expanded.ref_id, fz_ref);
}

#[test]
fn gz_blob_ref_falls_back_to_graphzero_sibling_store() {
    let dir = tempdir().unwrap();
    let root = dir.path().join(".zerostack");
    let gz_cache = root.join("graphzero").join("recovery-cache.json");
    let tokenzero_cache = root.join("tokenzero").join("recovery-cache.json");
    fs::create_dir_all(gz_cache.parent().unwrap()).unwrap();
    fs::create_dir_all(tokenzero_cache.parent().unwrap()).unwrap();

    let payload = "cross-engine blob from graphzero
";
    let gz_ref = format!("gz://blob/{}", tokenzero_core::sha256_hex(payload));

    // Store the payload using a flat cache path so it is written to the JSON
    // store rather than published to the shared CAS, then move the snapshot into
    // the unified graphzero layout.
    let gz_temp = dir.path().join("graphzero-cache.json");
    let mut gz_store = RecoveryStore::new(Some(gz_temp.clone()));
    gz_store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    fs::create_dir_all(gz_cache.parent().unwrap()).unwrap();
    fs::rename(&gz_temp, &gz_cache).unwrap();

    let mut tokenzero_store = RecoveryStore::new(Some(tokenzero_cache));
    let expanded = tokenzero_store.expand(&gz_ref, Some("raw"), None, None, None, None);
    assert!(
        expanded.found,
        "gz blob ref must expand via sibling graphzero store: reason={}",
        expanded.reason
    );
    assert_eq!(expanded.content, payload);
    assert_eq!(expanded.ref_id, gz_ref);
}

#[test]
fn shared_cas_corruption_is_detected_via_fragment() {
    let (mut store, _cache, _dir, cas) = canonical_shared_store();
    let payload = b"alpha
beta
gamma
";
    let full_hash = cas.publish(payload).unwrap();
    let prefix = &full_hash[..2];
    let object_path = cas
        .root()
        .join("blobs")
        .join("sha256")
        .join(prefix)
        .join(&full_hash);
    fs::write(&object_path, b"corrupted").unwrap();

    let fragment = format!("tz://blob/{full_hash}#B0-5");
    let expanded = store.expand(&fragment, Some("raw"), None, None, None, None);
    assert!(!expanded.found);
    assert_eq!(expanded.reason, "shared-cas-corruption");
    assert_eq!(expanded.ref_id, fragment);
    assert!(expanded.content.is_empty());
}

#[test]
fn shared_cas_rejects_malformed_hashes() {
    let (mut store, _cache, _dir, _cas) = canonical_shared_store();
    let uppercase_ref = format!("tz://blob/{}", "A".repeat(64));
    let invalid_refs = vec!["tz://blob/abc".to_string(), uppercase_ref];

    for (invalid_ref, reason) in invalid_refs
        .into_iter()
        .zip(["zeroref-legacy_ambiguity", "zeroref-malformed"])
    {
        let expanded = store.expand(&invalid_ref, Some("raw"), None, None, None, None);
        assert!(!expanded.found);
        assert_eq!(expanded.reason, reason);
        assert_eq!(expanded.ref_id, invalid_ref);
    }
}

#[test]
fn b_fragment_returns_empty_for_zero_range() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "hello world\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let b_ref = format!("{}#B0-0", stored.blob_ref);
    let expanded = store.expand(&b_ref, Some("raw"), None, None, None, None);
    assert!(expanded.found, "#B0-0 must succeed: {}", expanded.reason);
    assert_eq!(expanded.content, "");
    assert_eq!(expanded.ref_id, b_ref);
}

#[test]
fn b_fragment_returns_first_n_bytes() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "hello world\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let b_ref = format!("{}#B0-5", stored.blob_ref);
    let expanded = store.expand(&b_ref, Some("raw"), None, None, None, None);
    assert!(expanded.found, "#B0-5 must succeed: {}", expanded.reason);
    assert_eq!(expanded.content, "hello");
}

#[test]
fn b_fragment_returns_middle_slice() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "hello world\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let b_ref = format!("{}#B6-11", stored.blob_ref);
    let expanded = store.expand(&b_ref, Some("raw"), None, None, None, None);
    assert!(expanded.found, "#B6-11 must succeed: {}", expanded.reason);
    assert_eq!(expanded.content, "world");
}

#[test]
fn b_fragment_reversed_returns_error() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "hello\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let b_ref = format!("{}#B5-1", stored.blob_ref);
    let expanded = store.expand(&b_ref, Some("raw"), None, None, None, None);
    assert!(!expanded.found, "#B reversed must not succeed");
    assert_eq!(expanded.reason, "fragment-reversed");
    assert_eq!(expanded.ref_id, b_ref);
    assert!(expanded.content.is_empty());
}

#[test]
fn b_fragment_oob_returns_error() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "hello\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let b_ref = format!("{}#B0-100", stored.blob_ref);
    let expanded = store.expand(&b_ref, Some("raw"), None, None, None, None);
    assert!(!expanded.found, "#B oob must not succeed");
    assert!(
        expanded.reason.starts_with("fragment-out-of-range"),
        "got: {}",
        expanded.reason
    );
    assert_eq!(expanded.ref_id, b_ref);
    assert!(expanded.content.is_empty());
}

#[test]
fn b_fragment_malformed_returns_error() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "hello\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let b_ref = format!("{}#Babc", stored.blob_ref);
    let expanded = store.expand(&b_ref, Some("raw"), None, None, None, None);
    assert!(!expanded.found);
    assert_eq!(expanded.reason, "fragment-malformed");
}

#[test]
fn b_fragment_preserves_ref_id_in_error() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "hello\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let b_ref = format!("{}#B5-1", stored.blob_ref);
    let expanded = store.expand(&b_ref, Some("raw"), None, None, None, None);
    assert_eq!(expanded.ref_id, b_ref);
}

#[test]
fn b_fragment_full_range_returns_all_bytes() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "hello world\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let b_ref = format!("{}#B0-{}", stored.blob_ref, payload.len());
    let expanded = store.expand(&b_ref, Some("raw"), None, None, None, None);
    assert!(expanded.found);
    assert_eq!(expanded.content, payload);
}

#[test]
fn l_fragment_returns_first_three_lines() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "a\nb\nc\nd\ne\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let l_ref = format!("{}#L1-L3", stored.blob_ref);
    let expanded = store.expand(&l_ref, Some("raw"), None, None, None, None);
    assert!(expanded.found, "#L1-L3 must succeed: {}", expanded.reason);
    assert_eq!(expanded.content, "a\nb\nc\n");
}

#[test]
fn l_fragment_zero_line_returns_error() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "a\nb\nc\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let l_ref = format!("{}#L0", stored.blob_ref);
    let expanded = store.expand(&l_ref, Some("raw"), None, None, None, None);
    assert!(!expanded.found, "#L0 must not succeed");
    assert_eq!(expanded.reason, "fragment-malformed");
    assert_eq!(expanded.ref_id, l_ref);
}

#[test]
fn l_fragment_reversed_returns_error() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "a\nb\nc\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let l_ref = format!("{}#L5-L2", stored.blob_ref);
    let expanded = store.expand(&l_ref, Some("raw"), None, None, None, None);
    assert!(!expanded.found, "#L reversed must not succeed");
    assert_eq!(expanded.reason, "fragment-reversed");
    assert_eq!(expanded.ref_id, l_ref);
}

#[test]
fn l_fragment_oob_returns_error() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "a\nb\nc\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let l_ref = format!("{}#L1-L100", stored.blob_ref);
    let expanded = store.expand(&l_ref, Some("raw"), None, None, None, None);
    assert!(!expanded.found, "#L oob must not succeed");
    assert!(
        expanded.reason.starts_with("window-out-of-range"),
        "got: {}",
        expanded.reason
    );
}

#[test]
fn l_fragment_empty_file_returns_error() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let l_ref = format!("{}#L1", stored.blob_ref);
    let expanded = store.expand(&l_ref, Some("raw"), None, None, None, None);
    assert!(!expanded.found, "#L1 on empty file must not succeed");
    assert!(
        expanded.reason.starts_with("window-out-of-range"),
        "got: {}",
        expanded.reason
    );
}

#[test]
fn l_fragment_preserves_crlf() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "a\r\nb\r\nc\r\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let l_ref = format!("{}#L1-L2", stored.blob_ref);
    let expanded = store.expand(&l_ref, Some("raw"), None, None, None, None);
    assert!(
        expanded.found,
        "#L1-L2 CRLF must succeed: {}",
        expanded.reason
    );
    assert_eq!(expanded.content, "a\r\nb\r\n");
}

#[test]
fn l_fragment_preserves_trailing_newline() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "a\nb\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let l_ref = format!("{}#L1-L2", stored.blob_ref);
    let expanded = store.expand(&l_ref, Some("raw"), None, None, None, None);
    assert!(expanded.found);
    assert_eq!(expanded.content, "a\nb\n");
    assert!(
        expanded.content.ends_with('\n'),
        "trailing newline must be preserved"
    );
}

#[test]
fn l_fragment_single_line_returns_one_line() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "a\nb\nc\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let l_ref = format!("{}#L2", stored.blob_ref);
    let expanded = store.expand(&l_ref, Some("raw"), None, None, None, None);
    assert!(expanded.found);
    assert_eq!(expanded.content, "b\n");
}

#[test]
fn unknown_fragment_kind_returns_error() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "hello\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let x_ref = format!("{}#X1-3", stored.blob_ref);
    let expanded = store.expand(&x_ref, Some("raw"), None, None, None, None);
    assert!(!expanded.found);
    assert_eq!(expanded.reason, "fragment-unknown-kind");
}

#[test]
fn duplicate_fragment_returns_error() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "hello\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let dup_ref = format!("{}#B0-3#L1-2", stored.blob_ref);
    let expanded = store.expand(&dup_ref, Some("raw"), None, None, None, None);
    assert!(!expanded.found);
    assert_eq!(expanded.reason, "fragment-duplicate");
}

#[test]
fn b_fragment_no_fallback_to_full_payload() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = "byte-range payload\nline two\n";
    let stored = store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    // #B0-1 should return only 1 byte, not the full payload
    let b_ref = format!("{}#B0-1", stored.blob_ref);
    let expanded = store.expand(&b_ref, Some("raw"), None, None, None, None);
    assert!(expanded.found);
    assert_eq!(expanded.content, "b");
    assert_ne!(expanded.content, payload);
}

// ---------------------------------------------------------------------------
// Negative fixture: valid foreign full SHA-256 hash absent from TokenZero
// ---------------------------------------------------------------------------

#[test]
fn same_store_scheme_alias_foreign_full_hash_absent_returns_missing() {
    let (mut store, _cache, _dir) = temp_store();
    // A valid 64-hex-char SHA-256 that was never stored in TokenZero.
    // Simulates a ref produced by a foreign engine (FSZero/GraphZero) through
    // its own store — TokenZero cannot resolve it (same-store alias only).
    let foreign_ref = "fz://blob/abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let expanded = store.expand(foreign_ref, Some("raw"), None, None, None, None);
    assert!(!expanded.found);
    assert_eq!(expanded.reason, "ref-not-found");
    assert_eq!(
        expanded.ref_id, foreign_ref,
        "foreign ref must be preserved in full (no truncation)"
    );
}

// ---------------------------------------------------------------------------
// Windowed expand (zq9) — same-store line windows
// ---------------------------------------------------------------------------

fn multi_line_fixture(n: usize) -> String {
    (1..=n)
        .map(|i| format!("line-{i}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn windowed_expand_middle_edges_and_full() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = multi_line_fixture(200);
    let stored = store
        .store_payload(&payload, ContentType::Unknown, None, None, None)
        .unwrap();

    let full = store.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
    assert!(full.found);
    assert_eq!(full.content, payload);

    // Middle window 120..=190 (1-based inclusive) — field verify path
    let mid = store.expand(
        &stored.blob_ref,
        Some("raw"),
        Some(120),
        Some(190),
        None,
        None,
    );
    assert!(mid.found, "{}", mid.reason);
    let expected: String = payload.split_inclusive('\n').skip(119).take(71).collect();
    assert_eq!(mid.content, expected);
    assert!(mid.content.starts_with("line-120\n"));
    assert!(mid.content.contains("line-190\n"));
    assert!(!mid.content.contains("line-119\n"));
    assert!(!mid.content.contains("line-191\n"));

    // Edges
    let first = store.expand(&stored.blob_ref, Some("raw"), Some(1), Some(1), None, None);
    assert_eq!(first.content, "line-1\n");
    let last = store.expand(
        &stored.blob_ref,
        Some("raw"),
        Some(200),
        Some(200),
        None,
        None,
    );
    assert_eq!(last.content, "line-200\n");
}

#[test]
fn windowed_expand_oob_is_structured_not_ref_not_found() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = multi_line_fixture(50);
    let stored = store
        .store_payload(&payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let oob = store.expand(
        &stored.blob_ref,
        Some("raw"),
        Some(500),
        Some(510),
        None,
        None,
    );
    assert!(!oob.found);
    assert!(
        oob.reason.starts_with("window-out-of-range"),
        "got {}",
        oob.reason
    );
    assert!(
        !oob.reason.contains("ref-not-found"),
        "OOB must not look like missing ref: {}",
        oob.reason
    );
    assert_eq!(oob.ref_id, stored.blob_ref);

    let inverted = store.expand(&stored.blob_ref, Some("raw"), Some(10), Some(5), None, None);
    assert!(!inverted.found);
    assert!(inverted.reason.starts_with("window-out-of-range"));

    let end_past_last_line = store.expand(
        &stored.blob_ref,
        Some("raw"),
        Some(40),
        Some(60),
        None,
        None,
    );
    assert!(!end_past_last_line.found);
    assert!(
        end_past_last_line.reason.starts_with("window-out-of-range"),
        "got {}",
        end_past_last_line.reason
    );
}

#[test]
fn selector_lines_oob_is_structured_not_empty_success() {
    let (mut store, _cache, _dir) = temp_store();
    let payload = multi_line_fixture(50);
    let stored = store
        .store_payload(&payload, ContentType::Unknown, None, None, None)
        .unwrap();
    let oob = store.expand(
        &stored.blob_ref,
        Some("lines:500-510"),
        None,
        None,
        None,
        None,
    );
    assert!(!oob.found, "selector OOB must not succeed empty");
    assert!(
        oob.reason.starts_with("window-out-of-range"),
        "got {}",
        oob.reason
    );

    let around = store.expand(
        &stored.blob_ref,
        Some("around:L500:2"),
        None,
        None,
        None,
        None,
    );
    assert!(!around.found);
    assert!(around.reason.starts_with("window-out-of-range"));

    let end_past_last_line = store.expand(
        &stored.blob_ref,
        Some("lines:40-60"),
        None,
        None,
        None,
        None,
    );
    assert!(!end_past_last_line.found);
    assert!(
        end_past_last_line.reason.starts_with("window-out-of-range"),
        "got {}",
        end_past_last_line.reason
    );
}

#[test]
fn windowed_expand_visible_tokens_much_less_than_full() {
    let (mut store, _cache, _dir) = temp_store();
    // ~200 lines × ~8 tokens-ish → full multi-k; 50-line window << full
    let payload = multi_line_fixture(200);
    let stored = store
        .store_payload(&payload, ContentType::Unknown, None, None, None)
        .unwrap();
    store.recovery_tokens = 0;
    let full = store.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
    let full_tokens = full.tokens;
    store.recovery_tokens = 0;
    let window = store.expand(
        &stored.blob_ref,
        Some("raw"),
        Some(120),
        Some(169), // 50 lines
        None,
        None,
    );
    assert!(window.found);
    let window_tokens = window.tokens;
    assert!(
        window_tokens < full_tokens / 2,
        "50-line window tokens ({window_tokens}) should be << full ({full_tokens})"
    );
    assert!(
        window_tokens * 3 < full_tokens,
        "window {window_tokens} vs full {full_tokens}"
    );
}

#[test]
fn classify_ref_maps_kind_and_content_type() {
    assert_eq!(
        classify_ref("tz://file/abc", Some(ContentType::Unknown)),
        ContentClass::SourceFile
    );
    assert_eq!(
        classify_ref("tz://search/abc", Some(ContentType::Unknown)),
        ContentClass::SearchHits
    );
    assert_eq!(
        classify_ref("tz://unit/abc", Some(ContentType::Diff)),
        ContentClass::Diff
    );
    assert_eq!(
        classify_ref("tz://unit/abc", Some(ContentType::ShellOutput)),
        ContentClass::ShellOutput
    );
    assert_eq!(
        classify_ref("tz://blob/abc", Some(ContentType::Code)),
        ContentClass::SourceFile
    );
    assert_eq!(
        classify_ref("tz://blob/abc", Some(ContentType::Diff)),
        ContentClass::Diff
    );
    assert_eq!(
        classify_ref("tz://blob/abc", Some(ContentType::ShellOutput)),
        ContentClass::ShellOutput
    );
    assert_eq!(
        classify_ref("tz://blob/abc", Some(ContentType::Markdown)),
        ContentClass::Doc
    );
    assert_eq!(
        classify_ref("tz://blob/abc", Some(ContentType::Unknown)),
        ContentClass::BinaryPreview
    );
    assert_eq!(
        classify_ref("tz://codemode/execution/x/code", None),
        ContentClass::Unknown
    );
}

#[test]
fn export_class_stats_reports_per_class_rates() {
    let dir = tempdir().unwrap();
    with_ref_index_env(dir.path(), true, || {
        let store_path = dir.path().join("store.json");
        let refs = vec![
            "tz://blob/codeblob".to_string(),
            "tz://blob/diffblob".to_string(),
            "tz://blob/diffblob".to_string(), // duplicate store entry
            "tz://blob/unknownblob".to_string(),
        ];
        let mut classes = BTreeMap::new();
        classes.insert("tz://blob/codeblob".to_string(), ContentClass::SourceFile);
        classes.insert("tz://blob/diffblob".to_string(), ContentClass::Diff);
        append_blob_refs_to_ref_index(&store_path, &refs, Some(&classes));

        record_ref_index_expanded(&store_path, "tz://blob/codeblob", ContentClass::SourceFile);

        let stats = export_class_stats();
        let classes = stats["classes"].as_array().unwrap();
        let source = classes
            .iter()
            .find(|c| c["content_class"] == "SourceFile")
            .unwrap();
        assert_eq!(source["total"], 1);
        assert_eq!(source["expanded"], 1);
        assert_eq!(source["rate"], 1.0);
        let diff = classes
            .iter()
            .find(|c| c["content_class"] == "Diff")
            .unwrap();
        assert_eq!(diff["total"], 1);
        assert_eq!(diff["expanded"], 0);
        let binary = classes
            .iter()
            .find(|c| c["content_class"] == "BinaryPreview")
            .unwrap();
        assert_eq!(binary["total"], 1);
        assert_eq!(binary["expanded"], 0);
        assert_eq!(stats["total_refs"], 3);
        assert_eq!(stats["total_expanded"], 1);
    });
}

#[test]
fn ref_index_records_content_class_on_persist() {
    let index_dir = tempdir().unwrap();
    with_ref_index_env(index_dir.path(), true, || {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("cache.json");
        let mut store = RecoveryStore::new(Some(cache));
        let stored = store
            .store_payload("fn main() {}", ContentType::Code, None, None, None)
            .unwrap();

        let shard = ref_index_shard_path(index_dir.path(), &stored.blob_ref);
        let text = fs::read_to_string(shard).unwrap();
        let line = text.lines().find(|l| l.contains(&stored.blob_ref)).unwrap();
        assert!(line.contains("SourceFile"));
        assert!(line.contains("\"expanded\":false"));
    });
}

// ---------------------------------------------------------------------------
// ZeroRef v1 contract
// ---------------------------------------------------------------------------

const FULL_HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[test]
fn zeroref_v1_round_trips_canonical_blob_refs() {
    for (scheme, expected) in [("tz", "tz"), ("fz", "fz"), ("gz", "gz")] {
        let input = format!("{scheme}://blob/{FULL_HASH}");
        let parsed = parse_zeroref_v1_blob(&input, None).unwrap();
        assert_eq!(parsed.scheme, expected);
        assert_eq!(parsed.hash, FULL_HASH);
        assert!(parsed.fragment.is_none());
    }
}

#[test]
fn zeroref_v1_round_trips_fragment_selectors() {
    let line_ref = format!("tz://blob/{FULL_HASH}#L2-L5");
    let parsed = parse_zeroref_v1_blob(&line_ref, None).unwrap();
    assert_eq!(
        parsed.fragment,
        Some(ZeroRefFragment::Line { start: 2, end: 5 })
    );

    let byte_ref = format!("fz://blob/{FULL_HASH}#B0-64");
    let parsed = parse_zeroref_v1_blob(&byte_ref, Some(64)).unwrap();
    assert_eq!(
        parsed.fragment,
        Some(ZeroRefFragment::Byte { start: 0, end: 64 })
    );

    let empty_byte_ref = format!("gz://blob/{FULL_HASH}#B7-7");
    let parsed = parse_zeroref_v1_blob(&empty_byte_ref, Some(128)).unwrap();
    assert_eq!(
        parsed.fragment,
        Some(ZeroRefFragment::Byte { start: 7, end: 7 })
    );
}

#[test]
fn zeroref_v1_rejects_golden_negative_vectors() {
    let cases = vec![
        (
            "tz://blob/ba_e3b0c44298fc1c149",
            ZeroRefError::LegacyAmbiguity,
        ),
        (
            "tz://blob/E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
            ZeroRefError::Malformed,
        ),
        (
            "tz://blob/e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b85g",
            ZeroRefError::Malformed,
        ),
        (
            "tz://blob/e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855/extra",
            ZeroRefError::Malformed,
        ),
        ("tz://blob/", ZeroRefError::Malformed),
        (
            "tz://blob/e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855#B10-5",
            ZeroRefError::Malformed,
        ),
        (
            "tz://blob/e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855#L0-L3",
            ZeroRefError::Malformed,
        ),
        (
            "tz://file/e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ZeroRefError::Unsupported,
        ),
    ];
    for (input, expected) in cases {
        let err = parse_zeroref_v1_blob(input, None).unwrap_err();
        assert_eq!(err, expected, "{input} should yield {expected:?}");
    }
}

#[test]
fn zeroref_v1_byte_oob_respects_byte_length() {
    let input = format!("tz://blob/{FULL_HASH}#B0-100");
    assert_eq!(
        parse_zeroref_v1_blob(&input, Some(64)).unwrap_err(),
        ZeroRefError::Malformed
    );
    assert_eq!(
        parse_zeroref_v1_blob(&input, Some(100)).unwrap().fragment,
        Some(ZeroRefFragment::Byte { start: 0, end: 100 })
    );
    assert!(parse_zeroref_v1_blob(&input, None).is_ok());
}

#[test]
fn zeroref_v1_legacy_short_ids_parsed_by_existing_parse_ref() {
    let short = "tz://blob/ba_e3b0c44298fc1c149";
    assert_eq!(
        parse_zeroref_v1_blob(short, None).unwrap_err(),
        ZeroRefError::LegacyAmbiguity
    );
    let canonicalized = canonicalize_expand_ref(short).unwrap();
    assert!(
        parse_ref(&canonicalized).is_some(),
        "legacy short IDs remain parseable via existing parse_ref"
    );
}

#[test]
fn zeroref_v1_rejects_repeated_fragment_prefixes() {
    for fragment in ["BB0-1", "LL1-L2"] {
        let input = format!("tz://blob/{FULL_HASH}#{fragment}");
        assert_eq!(
            parse_zeroref_v1_blob(&input, None).unwrap_err(),
            ZeroRefError::Malformed,
            "{input}"
        );
    }
}

#[test]
fn expand_rejects_full_portable_ref_payload_hash_mismatch() {
    let (mut store, _cache, _dir) = temp_store();
    let claimed = format!("tz://blob/{FULL_HASH}");
    store.state.blobs.insert(
        claimed.clone(),
        BlobEntry::Inline("not the claimed bytes".into()),
    );

    let expanded = store.expand(&claimed, Some("raw"), None, None, None, None);

    assert!(!expanded.found);
    assert_eq!(expanded.reason, "zeroref-corruption");
}

#[test]
fn expand_allows_legacy_migration_key_without_portable_hash_check() {
    let (mut store, _cache, _dir) = temp_store();
    let legacy = "tz://blob/b0123456789abcdef";
    store
        .state
        .blobs
        .insert(legacy.into(), BlobEntry::Inline("legacy payload".into()));

    let expanded = store.expand(legacy, Some("raw"), None, None, None, None);

    assert!(expanded.found, "{}", expanded.reason);
    assert_eq!(expanded.content, "legacy payload");
}

#[test]
fn repeated_payload_reuses_refs_without_persistent_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("recovery.json");
    let index = temp.path().join("ref-index.jsonl");
    let previous_override = set_ref_index_test_override(Some((true, index.clone())));
    let mut store = RecoveryStore::new(Some(cache.clone()));
    let text = (0..64)
        .map(|idx| format!("repeated payload unit {}", idx % 4))
        .collect::<Vec<_>>()
        .join("\n");

    let first = store
        .store_payload(
            &text,
            ContentType::Code,
            Some(Path::new("memo-source.rs")),
            None,
            None,
        )
        .unwrap();
    assert_eq!(first.unit_refs.len(), 64);
    assert_eq!(first.unit_refs[0], first.unit_refs[4]);
    let snapshot_identity = DiskIdentity::capture(&cache);
    let journal_identity = DiskIdentity::capture(&journal_path(&cache));
    let index_identity = DiskIdentity::capture(&index);

    let second = store
        .store_payload(
            &text,
            ContentType::Code,
            Some(Path::new("memo-source.rs")),
            None,
            None,
        )
        .unwrap();

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert_eq!(DiskIdentity::capture(&cache), snapshot_identity);
    assert_eq!(
        DiskIdentity::capture(&journal_path(&cache)),
        journal_identity
    );
    assert_eq!(DiskIdentity::capture(&index), index_identity);
    assert!(store.session_refs.is_empty());
    set_ref_index_test_override(previous_override);
}

#[test]
fn recovery_blob_prune_prefers_never_expanded_blobs() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join(".tokenzero/recovery-cache.json");
    let index = dir.path().join("ref-index");
| {|    with_ref_index_env(&index, false, || {|| {
        let expanded_text = format!("expanded:{}", "x".repeat(70_000));
        let cold_text = format!("cold:{}", "y".repeat(70_000));
        let mut store = RecoveryStore::new(Some(cache.clone()));
        let expanded = store.store_payload(&expanded_text, ContentType::Unknown, None, None, None).unwrap();
        let cold = store.store_payload(&cold_text, ContentType::Unknown, None, None, None).unwrap();
        assert!(store.expand(&expanded.blob_ref, None, None, None, None, None).found);
        let report = prune_recovery_blobs(&cache, 75_000, Duration::from_secs(86_400), false).unwrap();
        assert_eq!(report.removed_files, 1);
        assert!(report.freed_bytes >= 70_000);
        let restarted = RecoveryStore::new(Some(cache.clone()));
        assert!(restarted.has_ref_local(&expanded.blob_ref));
        assert!(!restarted.has_ref_local(&cold.blob_ref));
    });
}

#[test]
fn recovery_blob_age_cap_and_status_support_both_store_roots() {
    for relative in [".tokenzero/recovery-cache.json", ".zerostack/tokenzero/recovery-cache.json"] {
        let dir = tempdir().unwrap();
        let cache = dir.path().join(relative);
        let index = dir.path().join("ref-index");
        with_ref_index_env(&index, false, || {
            let text = format!("aged:{}", "z".repeat(70_000));
            RecoveryStore::new(Some(cache.clone()))
                .store_payload(&text, ContentType::Unknown, None, None, None).unwrap();
            let report = prune_recovery_blobs(&cache, u64::MAX, Duration::ZERO, false).unwrap();
            assert_eq!(report.expired_files, 1);
            assert!(report.freed_bytes >= 70_000);
            assert_eq!(recovery_blob_status(&cache)["bytes"], 0);
        });
    }
}
