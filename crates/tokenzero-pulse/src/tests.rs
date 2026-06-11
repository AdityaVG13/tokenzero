use super::*;
use proptest::prelude::*;
use tempfile::tempdir;

fn test_event(tool: &str) -> PulseEvent {
    PulseEvent::tool_call(tool, "hybrid", 100, 20, 0, 1, 1, None)
}

fn write_single_event(path: &Path, tool: &str) {
    let mut line = serde_json::to_string(&test_event(tool)).unwrap();
    line.push('\n');
    fs::write(path, line).unwrap();
}

fn event_line_with_schema(schema_version: &str) -> Vec<u8> {
    let mut event = test_event("read");
    event.schema_version = schema_version.to_string();
    let mut line = b" \t".to_vec();
    line.extend(serde_json::to_vec(&event).unwrap());
    line.extend_from_slice(b" \n");
    line
}

#[test]
fn records_without_raw_payload() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let event = PulseEvent::tool_call(
        "read",
        "hybrid",
        100,
        20,
        0,
        1,
        3,
        Some("secret raw payload"),
    );
    record_event(&path, &event).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("secret raw payload"));
    assert!(text.contains("source_hash"));
}

#[test]
fn aggregates_recovery_adjusted_savings() {
    let events = vec![
        PulseEvent::tool_call("read", "hybrid", 100, 20, 10, 1, 1, None),
        PulseEvent::tool_call("shell", "hybrid", 100, 30, 20, 1, 1, None),
    ];
    let report = aggregate(&events);
    assert_eq!(report.raw_tokens, 200);
    assert!(report.visible_savings > report.recovery_adjusted_savings);
}

// Skipped under Miri: this test relies on POSIX O_APPEND kernel atomicity
// (concurrent small writes land whole at EOF), which Miri does not model — it
// shims append as seek+write, so records can interleave under Miri even though
// they never do on a real OS. The single-writer corruption path stays covered
// under Miri by load_counts_corrupt_lines.
#[test]
#[cfg_attr(
    miri,
    ignore = "depends on real-OS O_APPEND atomicity Miri cannot model"
)]
fn concurrent_appends_stay_whole_records() {
    use std::sync::Arc;
    use std::thread;
    let dir = tempdir().unwrap();
    let path = Arc::new(dir.path().join("events.jsonl"));
    let threads = 8;
    let per_thread = 64;
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let path = Arc::clone(&path);
            thread::spawn(move || {
                for _ in 0..per_thread {
                    let event = PulseEvent::tool_call("read", "hybrid", 100, 20, 0, 1, 1, None);
                    record_event(&path, &event).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    // Every appended record must be intact: no torn lines, none interleaved.
    let scan = scan_jsonl(&path, |_| Ok(())).unwrap();
    let skipped = scan.skipped_lines;
    let events = scan.event_count;
    assert_eq!(skipped, 0, "atomic append must not produce corrupt lines");
    assert_eq!(events, threads * per_thread);
}

#[test]
fn load_counts_corrupt_lines() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let event = PulseEvent::tool_call("read", "hybrid", 100, 20, 0, 1, 1, None);
    record_event(&path, &event).unwrap();
    // Append a torn/garbage record like a crash or bad hand-edit would leave.
    let existing = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{existing}{{not valid json\n")).unwrap();
    let scan = scan_jsonl(&path, |_| Ok(())).unwrap();
    assert_eq!(scan.event_count, 1);
    assert_eq!(scan.skipped_lines, 1);
    let report = report_for_path(&path).unwrap();
    assert_eq!(report.skipped_lines, 1);
    assert!(render_text(&report).contains("corrupt ledger line"));
}

#[test]
fn parse_event_line_rejects_wrong_schema_version() {
    assert!(parse_event_line(&event_line_with_schema("pulse-v1")).is_err());
    assert!(parse_event_line(&event_line_with_schema("tokenzero.pulse.v0")).is_err());

    let parsed = parse_event_line(&event_line_with_schema(PULSE_SCHEMA_VERSION))
        .unwrap()
        .unwrap();
    assert_eq!(parsed.schema_version, PULSE_SCHEMA_VERSION);
}

#[test]
fn scan_jsonl_counts_wrong_schema_as_corrupt_line() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let mut bytes = event_line_with_schema("tokenzero.pulse.v0");
    bytes.extend(event_line_with_schema(PULSE_SCHEMA_VERSION));
    fs::write(&path, bytes).unwrap();

    let mut callback_count = 0usize;
    let scan = scan_jsonl(&path, |_| {
        callback_count += 1;
        Ok(())
    })
    .unwrap();

    assert_eq!(callback_count, 1);
    assert_eq!(scan.event_count, 1);
    assert_eq!(scan.skipped_lines, 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_event_lines_do_not_panic_and_only_accept_current_schema(
        data in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        if let Ok(Some(event)) = parse_event_line(&data) {
            prop_assert_eq!(event.schema_version, PULSE_SCHEMA_VERSION);
        }
    }

    #[test]
    fn generated_valid_shaped_event_lines_require_current_schema(
        schema in "[A-Za-z0-9._-]{0,80}",
        leading_spaces in 0usize..8,
        trailing_spaces in 0usize..8
    ) {
        let mut event = test_event("shell");
        event.schema_version = schema.clone();
        let mut line = vec![b' '; leading_spaces];
        line.extend(serde_json::to_vec(&event).unwrap());
        line.extend(std::iter::repeat_n(b' ', trailing_spaces));
        line.push(b'\n');

        let parsed = parse_event_line(&line);
        if schema == PULSE_SCHEMA_VERSION {
            prop_assert!(matches!(parsed, Ok(Some(_))));
        } else {
            prop_assert!(parsed.is_err());
        }
    }
}

#[test]
fn missing_ledger_scans_as_empty() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");

    let scan = scan_jsonl(&path, |_| Ok(())).unwrap();

    assert_eq!(scan.event_count, 0);
    assert_eq!(scan.skipped_lines, 0);
    assert_eq!(scan.ledger_sha256, hex_sha256(&[]));
}

#[test]
#[cfg(unix)]
#[cfg_attr(miri, ignore = "depends on Unix file permission enforcement")]
fn unreadable_ledger_errors_instead_of_syncing_empty_cache() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    record_event(&path, &test_event("read")).unwrap();
    let status = sync_jsonl_to_sqlite(&path).unwrap();
    assert_eq!(status.event_count, 1);

    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&path, perms).unwrap();

    let result = sync_jsonl_to_sqlite(&path);

    let mut restored = fs::metadata(&path).unwrap().permissions();
    restored.set_mode(0o600);
    fs::set_permissions(&path, restored).unwrap();

    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    let after = sync_jsonl_to_sqlite(&path).unwrap();
    assert_eq!(after.event_count, 1);
    assert_eq!(after.ledger_sha256, status.ledger_sha256);
}

#[test]
fn sync_writes_sqlite_and_matching_markers() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    record_event(
        &path,
        &PulseEvent::tool_call("read", "hybrid", 100, 20, 0, 1, 1, None),
    )
    .unwrap();
    record_event(
        &path,
        &PulseEvent::tool_call("shell", "hybrid", 80, 60, 0, 1, 2, None),
    )
    .unwrap();

    let status = sync_jsonl_to_sqlite(&path).unwrap();
    assert!(status.ok);
    assert_eq!(status.event_count, 2);
    assert!(status.sqlite_path.exists());
    assert!(status.meta_path.exists());

    let doctor = doctor_jsonl_sqlite(&path).unwrap();
    assert!(doctor.ok);
    assert_eq!(doctor.sqlite_integrity, "ok");
    assert!(doctor.marker_match);
    assert!(doctor.hot_index_used);
}

#[test]
fn export_jsonl_writes_snapshot_atomically() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let output = dir.path().join("snapshot.jsonl");
    record_event(
        &path,
        &PulseEvent::tool_call("read", "hybrid", 100, 20, 0, 1, 1, None),
    )
    .unwrap();

    let status = export_jsonl(&path, &output).unwrap();
    assert!(status.ok);
    assert_eq!(
        fs::read_to_string(&output).unwrap(),
        fs::read_to_string(&path).unwrap()
    );
    assert!(export_meta_path(&output).exists());
    assert_eq!(scan_jsonl(&output, |_| Ok(())).unwrap().event_count, 1);
}

#[test]
fn export_jsonl_streams_clean_snapshot_from_sqlite() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let output = dir.path().join("snapshot.jsonl");
    record_event(
        &path,
        &PulseEvent::tool_call("read", "hybrid", 100, 20, 0, 1, 1, None),
    )
    .unwrap();
    let existing = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{existing}{{not valid json\n")).unwrap();

    let status = export_jsonl(&path, &output).unwrap();
    assert!(!status.ok);
    let scan = scan_jsonl(&output, |_| Ok(())).unwrap();
    assert_eq!(scan.event_count, 1);
    assert_eq!(scan.skipped_lines, 0);
}

#[test]
fn import_rejects_crashed_jsonl_and_can_retry_recovery() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let input = dir.path().join("bad.jsonl");
    let retry = dir.path().join("retry.jsonl");
    let original = PulseEvent::tool_call("read", "hybrid", 100, 20, 0, 1, 1, None);
    record_event(&path, &original).unwrap();
    let before = fs::read_to_string(&path).unwrap();
    fs::write(&input, "{not valid json\n").unwrap();
    fs::write(&retry, &before).unwrap();

    let err = import_jsonl(&input, &path).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    let mut first_tool = None;
    let scan = scan_jsonl(&path, |event| {
        first_tool = Some(event.tool.clone());
        Ok(())
    })
    .unwrap();
    assert_eq!(scan.event_count, 1);
    assert_eq!(first_tool.as_deref(), Some("read"));

    let recovered = import_jsonl(&retry, &path).unwrap();
    assert!(recovered.ok);
    assert_eq!(recovered.event_count, 1);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn import_preserves_verified_snapshot_bytes_without_trailing_newline() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let input = dir.path().join("snapshot.jsonl");
    let line = serde_json::to_string(&test_event("shell")).unwrap();
    fs::write(&input, line.as_bytes()).unwrap();
    let expected = scan_jsonl(&input, |_| Ok(())).unwrap();

    let status = import_jsonl(&input, &path).unwrap();

    assert_eq!(fs::read(&path).unwrap(), fs::read(&input).unwrap());
    assert_eq!(status.ledger_sha256, expected.ledger_sha256);
    assert_eq!(status.event_count, expected.event_count);
    assert_eq!(scan_jsonl(&path, |_| Ok(())).unwrap(), expected);
}

#[test]
fn import_copy_rejects_source_drift_before_replacing_ledger() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.jsonl");
    let output = dir.path().join("events.jsonl");
    let expected_source = dir.path().join("expected.jsonl");
    write_single_event(&input, "shell");
    write_single_event(&output, "read");
    write_single_event(&expected_source, "tree");
    let expected = scan_jsonl(&expected_source, |_| Ok(())).unwrap();
    let before = fs::read(&output).unwrap();

    let err = atomic_import_valid_jsonl(&input, &output, &expected).unwrap_err();

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("changed"));
    assert_eq!(fs::read(&output).unwrap(), before);
}

#[test]
fn import_newer_marked_snapshot_recovers_corrupt_current_ledger() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let input = dir.path().join("snapshot.jsonl");
    record_event(&path, &test_event("read")).unwrap();
    let current = sync_jsonl_to_sqlite(&path).unwrap();
    let current_meta = read_sidecar_meta(&current.meta_path).unwrap();
    write_single_event(&input, "shell");
    let input_scan = scan_jsonl(&input, |_| Ok(())).unwrap();
    write_sidecar_meta(
        &export_meta_path(&input),
        &PulseSyncMeta {
            schema_version: PULSE_SYNC_SCHEMA_VERSION.to_string(),
            source_of_truth: PULSE_SOURCE_OF_TRUTH.to_string(),
            ledger_sha256: input_scan.ledger_sha256.clone(),
            event_count: input_scan.event_count,
            skipped_lines: input_scan.skipped_lines,
            updated_unix: current_meta.updated_unix + 1,
        },
    )
    .unwrap();
    let corrupt = format!("{}{{not valid json\n", fs::read_to_string(&path).unwrap());
    fs::write(&path, corrupt).unwrap();
    assert_eq!(scan_jsonl(&path, |_| Ok(())).unwrap().skipped_lines, 1);

    let status = import_jsonl(&input, &path).unwrap();

    assert!(status.ok);
    assert_eq!(status.event_count, 1);
    assert_eq!(status.ledger_sha256, input_scan.ledger_sha256);
    assert_eq!(fs::read(&path).unwrap(), fs::read(&input).unwrap());
    let doctor = doctor_jsonl_sqlite(&path).unwrap();
    assert!(doctor.ok);
    assert!(doctor.marker_match);
}

#[test]
fn doctor_rebuilds_corrupt_sqlite_cache_from_jsonl() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    record_event(
        &path,
        &PulseEvent::tool_call("read", "hybrid", 100, 20, 0, 1, 1, None),
    )
    .unwrap();
    let status = sync_jsonl_to_sqlite(&path).unwrap();
    fs::write(&status.sqlite_path, b"not sqlite").unwrap();

    let doctor = doctor_jsonl_sqlite(&path).unwrap();
    assert!(doctor.ok);
    assert_eq!(doctor.event_count, 1);
    assert_eq!(doctor.sqlite_integrity, "ok");
    assert!(doctor.marker_match);
}

#[test]
fn sync_rebuilds_incompatible_sqlite_cache_schema() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    record_event(&path, &test_event("read")).unwrap();
    let sqlite_path = sqlite_path_for_ledger(&path);
    let conn = Connection::open(&sqlite_path).unwrap();
    conn.execute_batch(
        "
            CREATE TABLE events (
                line_no INTEGER PRIMARY KEY
            );
            CREATE TABLE meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
    )
    .unwrap();
    drop(conn);

    let status = sync_jsonl_to_sqlite(&path).unwrap();

    assert!(status.ok);
    assert_eq!(status.event_count, 1);
    let doctor = doctor_jsonl_sqlite(&path).unwrap();
    assert!(doctor.ok);
    assert_eq!(doctor.sqlite_integrity, "ok");
    assert!(doctor.marker_match);
}

#[test]
fn sqlite_cache_rebuild_removes_sidecars() {
    let dir = tempdir().unwrap();
    let sqlite_path = dir.path().join("events.sqlite");
    let wal_path = sqlite_sidecar_path(&sqlite_path, "-wal");
    let shm_path = sqlite_sidecar_path(&sqlite_path, "-shm");

    fs::write(&sqlite_path, b"corrupt").unwrap();
    fs::write(&wal_path, b"wal").unwrap();
    fs::write(&shm_path, b"shm").unwrap();

    remove_sqlite_cache_files(&sqlite_path).unwrap();

    assert!(!sqlite_path.exists());
    assert!(!wal_path.exists());
    assert!(!shm_path.exists());
}

#[test]
#[cfg(unix)]
fn sqlite_sidecar_path_preserves_non_utf8_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let path = PathBuf::from(OsString::from_vec(b"/tmp/events-\xff.sqlite".to_vec()));
    let sidecar = sqlite_sidecar_path(&path, "-wal");

    assert_eq!(
        sidecar.as_os_str().as_bytes(),
        b"/tmp/events-\xff.sqlite-wal"
    );
}

#[test]
fn import_rejects_older_marked_snapshot() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let input = dir.path().join("older.jsonl");
    record_event(
        &path,
        &PulseEvent::tool_call("read", "hybrid", 100, 20, 0, 1, 1, None),
    )
    .unwrap();
    let current = sync_jsonl_to_sqlite(&path).unwrap();
    write_single_event(&input, "shell");
    let input_scan = scan_jsonl(&input, |_| Ok(())).unwrap();
    let older_meta = PulseSyncMeta {
        schema_version: PULSE_SYNC_SCHEMA_VERSION.to_string(),
        source_of_truth: PULSE_SOURCE_OF_TRUTH.to_string(),
        ledger_sha256: input_scan.ledger_sha256,
        event_count: input_scan.event_count,
        skipped_lines: input_scan.skipped_lines,
        updated_unix: 0,
    };
    fs::write(
        export_meta_path(&input),
        serde_json::to_vec(&older_meta).unwrap(),
    )
    .unwrap();

    let err = import_jsonl(&input, &path).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    let after = sync_jsonl_to_sqlite(&path).unwrap();
    assert_eq!(after.ledger_sha256, current.ledger_sha256);
    let mut first_tool = None;
    scan_jsonl(&path, |event| {
        first_tool = Some(event.tool.clone());
        Ok(())
    })
    .unwrap();
    assert_eq!(first_tool.as_deref(), Some("read"));
}

#[test]
fn import_rejects_snapshot_when_current_ledger_changed_after_marker() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let snapshot = dir.path().join("snapshot.jsonl");
    record_event(&path, &test_event("read")).unwrap();
    export_jsonl(&path, &snapshot).unwrap();
    record_event(&path, &test_event("shell")).unwrap();
    let before = fs::read_to_string(&path).unwrap();

    let err = import_jsonl(&snapshot, &path).unwrap_err();

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("unsynced changes"));
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    let status = sync_jsonl_to_sqlite(&path).unwrap();
    assert_eq!(status.event_count, 2);
}

#[test]
fn import_rejects_same_second_different_marked_snapshot() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let input = dir.path().join("same-second.jsonl");
    record_event(&path, &test_event("read")).unwrap();
    let current = sync_jsonl_to_sqlite(&path).unwrap();
    let current_meta = read_sidecar_meta(&current.meta_path).unwrap();
    write_single_event(&input, "shell");
    let input_scan = scan_jsonl(&input, |_| Ok(())).unwrap();
    write_sidecar_meta(
        &export_meta_path(&input),
        &PulseSyncMeta {
            schema_version: PULSE_SYNC_SCHEMA_VERSION.to_string(),
            source_of_truth: PULSE_SOURCE_OF_TRUTH.to_string(),
            ledger_sha256: input_scan.ledger_sha256,
            event_count: input_scan.event_count,
            skipped_lines: input_scan.skipped_lines,
            updated_unix: current_meta.updated_unix,
        },
    )
    .unwrap();

    let err = import_jsonl(&input, &path).unwrap_err();

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("not newer"));
    let after = sync_jsonl_to_sqlite(&path).unwrap();
    assert_eq!(after.ledger_sha256, current.ledger_sha256);
}

#[test]
fn import_rejects_snapshot_marker_that_does_not_match_jsonl() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let input = dir.path().join("stale-sidecar.jsonl");
    record_event(&path, &test_event("read")).unwrap();
    let current = sync_jsonl_to_sqlite(&path).unwrap();
    let current_meta = read_sidecar_meta(&current.meta_path).unwrap();
    write_single_event(&input, "shell");
    write_sidecar_meta(
        &export_meta_path(&input),
        &PulseSyncMeta {
            schema_version: PULSE_SYNC_SCHEMA_VERSION.to_string(),
            source_of_truth: PULSE_SOURCE_OF_TRUTH.to_string(),
            ledger_sha256: "not-the-jsonl-hash".to_string(),
            event_count: 1,
            skipped_lines: 0,
            updated_unix: current_meta.updated_unix.saturating_add(1),
        },
    )
    .unwrap();

    let err = import_jsonl(&input, &path).unwrap_err();

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("marker does not match"));
    let after = sync_jsonl_to_sqlite(&path).unwrap();
    assert_eq!(after.ledger_sha256, current.ledger_sha256);
}

#[test]
fn lock_file_is_stable_anchor_not_deleted_on_drop() {
    let dir = tempdir().unwrap();
    let ledger = dir.path().join("events.jsonl");
    let lock_path = lock_path_for_ledger(&ledger);

    {
        let _lock = acquire_pulse_lock(&ledger).unwrap();
        assert!(lock_path.exists());
        let err = match acquire_pulse_lock_wait(&ledger, Duration::from_millis(10)) {
            Ok(_) => panic!("second lock acquisition should block while OS lock is held"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
    }

    assert!(
        lock_path.exists(),
        "OS file locks must keep one stable lock anchor"
    );
    drop(acquire_pulse_lock(&ledger).unwrap());
    assert!(lock_path.exists());
}

#[test]
fn sync_waits_for_transient_lock_contention() {
    let dir = tempdir().unwrap();
    let ledger = dir.path().join("events.jsonl");
    record_event(&ledger, &test_event("read")).unwrap();
    let lock = acquire_pulse_lock(&ledger).unwrap();
    let ledger_for_sync = ledger.clone();
    let handle = std::thread::spawn(move || sync_jsonl_to_sqlite(&ledger_for_sync));

    std::thread::sleep(Duration::from_millis(25));
    drop(lock);
    let status = handle.join().unwrap().unwrap();

    assert!(status.ok);
    assert_eq!(status.event_count, 1);
}

#[test]
fn lock_wait_retries_platform_lock_contention_errors() {
    let would_block = std::io::Error::new(std::io::ErrorKind::WouldBlock, "held");
    let invalid_input = std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid argument");
    let interrupted = std::io::Error::new(std::io::ErrorKind::Interrupted, "signal");

    assert!(retryable_pulse_lock_wait_error(&would_block));
    assert!(retryable_pulse_lock_wait_error(&invalid_input));
    assert!(!retryable_pulse_lock_wait_error(&interrupted));
}
