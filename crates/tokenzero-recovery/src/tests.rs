use super::*;
use proptest::prelude::*;
use tempfile::tempdir;

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

#[test]
fn persist_compacts_duplicate_order_entries() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        for _ in 0..5 {
            store.store_payload_deferred("same\n", ContentType::Unknown, None, None, None);
        }
        assert!(store.state.order.len() > 1);
        store.persist_pending().unwrap();
    }

    let restarted = RecoveryStore::new(Some(cache));
    let unique = restarted.state.order.iter().collect::<HashSet<_>>().len();
    assert_eq!(restarted.state.order.len(), unique);
}

#[test]
fn persist_lock_file_is_stable_anchor_not_deleted_on_drop() {
    let dir = tempdir().unwrap();
    let lock_path = dir.path().join("cache.json.lock");

    {
        let _lock = PersistLock::acquire(lock_path.clone()).unwrap();
        assert!(lock_path.exists());
        let err = match PersistLock::acquire_with_retries(lock_path.clone(), 1) {
            Ok(_) => panic!("second lock acquisition should block while OS lock is held"),
            Err(err) => err,
        };
        match err {
            RecoveryError::Io(err) => assert_eq!(err.kind(), std::io::ErrorKind::TimedOut),
            other => panic!("unexpected lock error: {other:?}"),
        }
    }

    assert!(
        lock_path.exists(),
        "OS file locks must keep one stable lock anchor"
    );
    drop(PersistLock::acquire(lock_path.clone()).unwrap());
    assert!(lock_path.exists());
}

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
    // Two live stores on the same cache file: each persist by one store
    // changes the file identity the other captured, so the other's next
    // persist must detect the foreign write and take the reload+merge path.
    // Skipping the merge here would silently drop the peer's refs.
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

    // One persist each picks up the peer's final round from disk.
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
    // With no foreign writes between persists, the second and later persists
    // take the skip path (identity captured under the lock still matches);
    // a restart must still expand every ref byte-exactly from the file the
    // skip path wrote.
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
        // The skip precondition must hold between single-writer persists.
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

#[test]
fn persisted_cache_is_compact_json() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let mut store = RecoveryStore::new(Some(cache.clone()));
    store
        .store_payload("alpha\n", ContentType::Unknown, None, None, None)
        .unwrap();

    let text = fs::read_to_string(cache).unwrap();
    assert!(serde_json::from_str::<RecoveryState>(&text).is_ok());
    assert_eq!(text.lines().count(), 1);
}

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
    let text = read_limited_utf8(std::io::Cursor::new([0xff]), 16).unwrap();

    assert!(text.is_none());
}

#[test]
fn recovery_tmp_paths_are_unique_within_process() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");

    let first = recovery_tmp_path(&cache);
    let second = recovery_tmp_path(&cache);

    assert_ne!(first, second);
    assert_eq!(first.parent(), Some(dir.path()));
    assert_eq!(second.parent(), Some(dir.path()));
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

#[test]
fn line_range_payloads_skip_source_fingerprint() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("large.txt");
    fs::write(&source, "one\ntwo\n").unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("cache.json")));
    let stored = store
        .store_payload(
            "one\n",
            ContentType::Unknown,
            Some(&source),
            Some(1),
            Some(1),
        )
        .unwrap();

    assert!(
        store
            .state
            .files
            .get(&stored.file_ref)
            .unwrap()
            .source_fingerprint
            .is_none()
    );
}

#[test]
fn virtual_paths_skip_source_fingerprint() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("cache.json")));
    let stored = store
        .store_payload(
            "output\n",
            ContentType::ShellOutput,
            Some(Path::new("shell:stdout:test")),
            None,
            None,
        )
        .unwrap();

    assert!(
        store
            .state
            .files
            .get(&stored.file_ref)
            .unwrap()
            .source_fingerprint
            .is_none()
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
}

#[cfg(unix)]
#[test]
fn native_path_identity_round_trips_non_utf8_path_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let path = PathBuf::from(OsString::from_vec(b"/tmp/source-\xff.rs".to_vec()));
    let round_tripped = path_from_identity_text(&path_identity_text(&path)).unwrap();

    assert_eq!(
        round_tripped.as_os_str().as_bytes(),
        path.as_os_str().as_bytes()
    );
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

#[test]
fn range_fragment_selects_lines() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let mut store = RecoveryStore::new(Some(cache));
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
fn around_selector_saturates_huge_line_and_radius() {
    let selector = format!("around:L{}:{}", usize::MAX, usize::MAX);

    let selected = select_content("a\nb\nc\n", Some(&selector), None, None, None, None);

    assert_eq!(selected, "a\nb\nc\n");
}

#[test]
fn expand_preserves_non_newline_terminated_content() {
    // F-004 regression: exact recovery must not add a trailing newline.
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let mut store = RecoveryStore::new(Some(cache));
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
fn slice_preserves_trailing_blank_line() {
    // F-001 regression: a slice ending on a blank line keeps that line's newline.
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let mut store = RecoveryStore::new(Some(cache));
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

#[test]
fn eviction_bounds_blob_count() {
    let config = RecoveryConfig {
        max_blobs: 1,
        ..RecoveryConfig::default()
    };
    let mut store = RecoveryStore::with_config(None, config);
    let first = store
        .store_payload("first", ContentType::Unknown, None, None, None)
        .unwrap();
    let second = store
        .store_payload("second", ContentType::Unknown, None, None, None)
        .unwrap();
    assert!(!store.has_ref(&first.blob_ref));
    assert!(store.has_ref(&second.blob_ref));
}

#[test]
fn file_refs_expand_after_their_blob_is_evicted() {
    // Every ref kind stores its text inline; evicting a payload's blob
    // must not dangle the sibling file ref. Pins the absence of
    // inter-entry dependencies at expansion time.
    let config = RecoveryConfig {
        max_blobs: 1,
        ..RecoveryConfig::default()
    };
    let mut store = RecoveryStore::with_config(None, config);
    let first = store
        .store_payload("first payload\n", ContentType::Unknown, None, None, None)
        .unwrap();
    let _second = store
        .store_payload("second payload\n", ContentType::Unknown, None, None, None)
        .unwrap();
    assert!(!store.has_ref(&first.blob_ref));

    let expanded = store.expand(&first.file_ref, Some("raw"), None, None, None, None);
    assert!(expanded.found, "file ref must survive blob eviction");
    assert_eq!(expanded.content, "first payload\n");
}

#[test]
fn deferred_payload_enforces_limits_before_returning() {
    let config = RecoveryConfig {
        max_blobs: 1,
        ..RecoveryConfig::default()
    };
    let mut store = RecoveryStore::with_config(None, config);

    let first = store.store_payload_deferred("first", ContentType::Unknown, None, None, None);
    let second = store.store_payload_deferred("second", ContentType::Unknown, None, None, None);

    assert!(!store.has_ref(&first.blob_ref));
    assert!(store.has_ref(&second.blob_ref));
}

#[test]
fn batched_deferred_payloads_enforce_limits_on_persist_pending() {
    let config = RecoveryConfig {
        max_blobs: 1,
        ..RecoveryConfig::default()
    };
    let mut store = RecoveryStore::with_config(None, config);

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
    let mut immediate = RecoveryStore::with_config(None, config.clone());
    for payload in payloads {
        immediate.store_payload_deferred(payload, ContentType::Unknown, None, None, None);
    }

    let mut batched = RecoveryStore::with_config(None, config);
    for payload in payloads {
        batched.store_payload_deferred_batch(payload, ContentType::Unknown, None, None, None);
    }
    batched.persist_pending().unwrap();

    assert_eq!(
        immediate.state.blobs.keys().collect::<Vec<_>>(),
        batched.state.blobs.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        immediate.state.files.keys().collect::<Vec<_>>(),
        batched.state.files.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        immediate.state.units.keys().collect::<Vec<_>>(),
        batched.state.units.keys().collect::<Vec<_>>()
    );
    assert_eq!(immediate.state.order, batched.state.order);
}

#[test]
fn deferred_search_output_enforces_limits_before_returning() {
    let config = RecoveryConfig {
        max_search_hits: 1,
        ..RecoveryConfig::default()
    };
    let mut store = RecoveryStore::with_config(None, config);

    let refs = store.store_search_output_deferred("needle one\nneedle two\n", Some("needle"));

    assert_eq!(refs.len(), 2);
    assert!(!store.has_ref(&refs[0]));
    assert!(store.has_ref(&refs[1]));
}

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
        prop_assert_eq!(expanded.content, text);
    }

    #[test]
    fn generated_around_selectors_do_not_panic(line in any::<usize>(), radius in any::<usize>()) {
        let selector = format!("around:L{line}:{radius}");

        let selected = select_content("a\nb\nc\n", Some(&selector), None, None, None, None);

        prop_assert!(selected.is_empty() || selected == "a\n" || selected == "a\nb\n" || selected == "a\nb\nc\n");
    }
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

#[test]
fn tmp_sweep_reclaims_stale_orphans_of_both_shapes_only() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let stale_age = STALE_TMP_MAX_AGE * 2;
    let hidden = dir.path().join(".recovery-cache.json.123.7.tmp");
    let legacy = dir.path().join("recovery-cache.json.456.a1b2c3.tmp");
    let fresh = dir.path().join(".recovery-cache.json.789.8.tmp");
    let lock = dir.path().join("recovery-cache.json.lock");
    let unrelated = dir.path().join("other-file.tmp");
    write_aged_file(&hidden, 5, stale_age);
    write_aged_file(&legacy, 7, stale_age);
    write_aged_file(&fresh, 3, Duration::from_secs(1));
    write_aged_file(&lock, 2, stale_age);
    write_aged_file(&unrelated, 9, stale_age);
    let report = sweep_stale_tmp_files(&cache, STALE_TMP_MAX_AGE, false);
    assert!(
        !hidden.exists(),
        "stale hidden-shape orphan must be reclaimed"
    );
    assert!(
        !legacy.exists(),
        "stale legacy-shape orphan must be reclaimed"
    );
    assert!(
        fresh.exists(),
        "an in-flight writer's temp file must survive"
    );
    assert!(lock.exists(), "the lock anchor must never be touched");
    assert!(unrelated.exists(), "unrelated temp files must survive");
    assert_eq!(report.scanned, 3);
    assert_eq!(report.removed, 2);
    assert_eq!(report.removed_bytes, 12);
    assert_eq!(report.failed, 0);
}

#[test]
fn tmp_sweep_dry_run_counts_without_unlinking() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let orphan = dir.path().join(".recovery-cache.json.123.7.tmp");
    write_aged_file(&orphan, 5, STALE_TMP_MAX_AGE * 2);
    let report = sweep_stale_tmp_files(&cache, STALE_TMP_MAX_AGE, true);
    assert!(orphan.exists(), "dry run must not unlink");
    assert!(report.dry_run);
    assert_eq!(report.removed, 1);
    assert_eq!(report.removed_bytes, 5);
}

#[test]
fn tmp_sweep_missing_dir_is_empty_report() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("missing").join("recovery-cache.json");
    let report = sweep_stale_tmp_files(&cache, STALE_TMP_MAX_AGE, false);
    assert_eq!(report.scanned, 0);
    assert_eq!(report.removed, 0);
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

    // Fresh process: loads the snapshot, captures identity, and its persist
    // must take the journal append path instead of rewriting the snapshot.
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
    // Two processes load the same disk state...
    let mut a = RecoveryStore::new(Some(cache.clone()));
    let mut b = RecoveryStore::new(Some(cache.clone()));
    // ...b persists first (journal append). a's identity is now stale, so
    // a's persist must detect the foreign append and take the merge path.
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
    // Simulate a torn append: garbage after the last complete line.
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
fn big_blob_externalizes_to_sidecar_and_roundtrips() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let big = "x".repeat(200 * 1024);
    let stored = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload(&big, ContentType::Unknown, None, None, None)
            .unwrap()
    };
    let sidecar = blob_sidecar_dir(&cache);
    assert!(sidecar.is_dir(), "sidecar dir must exist");
    assert!(
        fs::read_dir(&sidecar).unwrap().count() >= 1,
        "sidecar must hold the payload"
    );
    // The BLOBS map must hold a marker, not the payload. (The files/units
    // maps still inline their copies — beads tokenzero-ocx/a73 own that.)
    let snapshot: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cache).unwrap()).unwrap();
    let blob_value = snapshot["blobs"][&stored.blob_ref].as_str().unwrap();
    assert!(
        blob_value.starts_with('\u{0}'),
        "blob value must be an externalized marker"
    );
    assert!(blob_value.len() < 128, "marker must be tiny");
    let mut restarted = RecoveryStore::new(Some(cache));
    let expanded = restarted.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
    assert!(expanded.found);
    assert_eq!(expanded.content, big);
}

#[test]
fn small_blob_stays_inline() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let mut store = RecoveryStore::new(Some(cache.clone()));
    store
        .store_payload("small\n", ContentType::Unknown, None, None, None)
        .unwrap();
    assert!(
        !blob_sidecar_dir(&cache).exists(),
        "no sidecar for small payloads"
    );
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
fn oversized_journal_compacts_into_fresh_snapshot() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    let small = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        store
            .store_payload("tiny\n", ContentType::Unknown, None, None, None)
            .unwrap()
    };
    // Snapshot is tiny, so the compaction threshold is the 64KB floor; a
    // payload larger than that must fold journal + snapshot into a fresh
    // snapshot and remove the journal.
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
