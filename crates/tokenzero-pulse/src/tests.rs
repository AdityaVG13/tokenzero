use super::*;
use proptest::prelude::*;
use tempfile::tempdir;

/// Create a temp directory and a Pulse ledger with one "read" event.
/// Returns `(dir, path)` — caller holds `dir` to keep the temp alive.
fn setup_ledger() -> (tempfile::TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    record_event(&path, &test_event("read")).unwrap();
    (dir, path)
}

/// Assert that `import_jsonl(input, path)` fails with `InvalidData` or
/// `InvalidInput` and that the error message contains `expected_fragment`.
fn assert_import_rejected(input: &Path, path: &Path, expected_fragment: &str) {
    let err = import_jsonl(input, path).unwrap_err();
    assert!(
        err.kind() == std::io::ErrorKind::InvalidData
            || err.kind() == std::io::ErrorKind::InvalidInput,
        "expected InvalidData or InvalidInput, got {:?}",
        err.kind()
    );
    assert!(
        err.to_string().contains(expected_fragment),
        "expected error to contain '{}', got '{}'",
        expected_fragment,
        err
    );
}

/// Write a sidecar meta for `input` with `updated_unix` set relative to
/// `current_meta.updated_unix + delta_secs`.
fn write_marked_snapshot(
    input: &Path,
    input_scan: &JsonlScan,
    current_meta: &PulseSyncMeta,
    delta_secs: i64,
) {
    let updated = if delta_secs >= 0 {
        current_meta.updated_unix.saturating_add(delta_secs as u64)
    } else {
        current_meta
            .updated_unix
            .saturating_sub((-delta_secs) as u64)
    };
    write_sidecar_meta(
        &export_meta_path(input),
        &PulseSyncMeta {
            schema_version: PULSE_SYNC_SCHEMA_VERSION.to_string(),
            source_of_truth: PULSE_SOURCE_OF_TRUTH.to_string(),
            ledger_sha256: input_scan.ledger_sha256.clone(),
            event_count: input_scan.event_count,
            skipped_lines: input_scan.skipped_lines,
            updated_unix: updated,
        },
    )
    .unwrap();
}

fn test_event(tool: &str) -> PulseEvent {
    PulseEvent::tool_call(tool, "hybrid", 100, 20, 0, 1, 1, None)
}

fn write_single_event(path: &Path, tool: &str) {
    let mut line = serde_json::to_string(&test_event(tool)).unwrap();
    line.push('\n');
    fs::write(path, line).unwrap();
}

/// Compute the same report the public `report_for_path` produces, but from
/// an in-memory event slice rather than requiring callers to write a temp
/// ledger in every aggregate assertion.
fn aggregate(events: &[PulseEvent]) -> PulseReport {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    for event in events {
        record_event(&path, event).unwrap();
    }
    report_for_path(&path).unwrap()
}

fn write_sidecar_with_scan(input: &Path, input_scan: &JsonlScan, updated_unix: u64) {
    write_sidecar_meta(
        &export_meta_path(input),
        &PulseSyncMeta {
            schema_version: PULSE_SYNC_SCHEMA_VERSION.to_string(),
            source_of_truth: PULSE_SOURCE_OF_TRUTH.to_string(),
            ledger_sha256: input_scan.ledger_sha256.clone(),
            event_count: input_scan.event_count,
            skipped_lines: input_scan.skipped_lines,
            updated_unix,
        },
    )
    .unwrap();
}

fn setup_import_test() -> (
    tempfile::TempDir,
    PathBuf,
    PathBuf,
    PulseSyncMeta,
    String,
    Vec<u8>,
) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let input = dir.path().join("input.jsonl");
    record_event(&path, &test_event("read")).unwrap();
    let current = sync_jsonl_to_sqlite(&path).unwrap();
    let current_meta = read_sidecar_meta(&current.meta_path).unwrap();
    let before_sha = current.ledger_sha256.clone();
    let before_bytes = fs::read(&path).unwrap();
    (dir, path, input, current_meta, before_sha, before_bytes)
}

fn assert_import_post_rejection(
    case_name: &str,
    path: &Path,
    ledger_changed: bool,
    before_sha: &str,
    before_bytes: &[u8],
) {
    if ledger_changed {
        let after = sync_jsonl_to_sqlite(path).unwrap();
        assert_ne!(
            after.ledger_sha256, before_sha,
            "[{}] ledger hash must advance after new event",
            case_name
        );
    } else {
        assert_eq!(
            fs::read(path).unwrap(),
            before_bytes,
            "[{}] original ledger must be unchanged after rejection",
            case_name
        );
    }
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

/// Mutation killed: any error in the recovery-adjusted savings formula
/// (e.g. forgetting to include recovery_tokens in the denominator, or
/// computing `visible_savings - recovery_tokens` instead of `savings_ratio`).
#[test]
fn aggregates_recovery_adjusted_savings() {
    let events = vec![
        PulseEvent::tool_call("read", "hybrid", 100, 20, 10, 1, 1, None),
        PulseEvent::tool_call("shell", "hybrid", 100, 30, 20, 1, 1, None),
    ];
    let report = aggregate(&events);
    assert_eq!(report.raw_tokens, 200);
    assert_eq!(report.visible_tokens, 50);
    assert_eq!(report.recovery_tokens, 30);
    assert_eq!(report.event_count, 2);
    // visible_savings = (200 - 50) / 200 = 0.75
    assert_eq!(report.visible_savings, 0.75);
    // recovery_adjusted_savings = (200 - (50 + 30)) / 200 = 0.60
    assert_eq!(report.recovery_adjusted_savings, 0.60);
    // Recovery-adjusted is strictly worse than visible-only.
    assert!(report.visible_savings > report.recovery_adjusted_savings);
}

#[test]
fn aggregate_saturates_like_file_backed_report() {
    let mut first =
        PulseEvent::tool_call("read", "hybrid", usize::MAX, usize::MAX - 1, 10, 1, 0, None);
    first.task_lossless = true;
    first.exact_ref_count = usize::MAX;
    let mut second = PulseEvent::tool_call("expand", "hybrid", 10, 10, 10, 1, 1, None);
    second.task_lossless = true;

    let report = aggregate(&[first, second]);
    assert_eq!(report.raw_tokens, usize::MAX);
    assert_eq!(report.visible_tokens, usize::MAX);
    assert_eq!(report.recovery_tokens, 20);
    assert_eq!(report.task_lossless_tokens, usize::MAX);
    assert_eq!(report.exact_ref_count, usize::MAX);
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

/// Mutation killed: scan_jsonl counting wrong-schema lines as valid events
/// (must be counted as skipped, and the callback must not fire).
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

/// Mutation killed: import allowing a corrupt JSONL file to pass, or
/// failing to restore the original ledger after rejection.
#[test]
fn import_rejects_crashed_jsonl_and_preserves_original() {
    let (_dir, path) = setup_ledger();
    let input = path.parent().unwrap().join("bad.jsonl");
    let before = fs::read_to_string(&path).unwrap();
    fs::write(&input, "{not valid json\n").unwrap();

    assert_import_rejected(&input, &path, "corrupt");
    // Original ledger must be untouched after rejection.
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    let scan = scan_jsonl(&path, |_| Ok(())).unwrap();
    assert_eq!(scan.event_count, 1);
}

/// Mutation killed: import accepting a valid JSONL that the caller already
/// verified, then retrying after a prior rejection.
#[test]
fn import_accepts_valid_jsonl_after_previous_rejection() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let input = dir.path().join("good.jsonl");
    let event = PulseEvent::tool_call("read", "hybrid", 100, 20, 0, 1, 1, None);
    record_event(&path, &event).unwrap();
    let payload = serde_json::to_string(&event).unwrap();
    fs::write(&input, format!("{payload}\n")).unwrap();

    let recovered = import_jsonl(&input, &path).unwrap();
    assert!(recovered.ok);
    assert_eq!(recovered.event_count, 1);
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

/// Mutation killed: import accepting a snapshot that is stale (older timestamp),
/// or import refusing a newer snapshot when the current ledger is corrupt.
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
    write_marked_snapshot(&input, &input_scan, &current_meta, 1);
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

/// Parametrized import-rejection test.
/// Each case: create a ledger with one "read" event, sync it, write an
/// input snapshot, apply a specific mutation to the sidecar or ledger,
/// then assert import rejects with the expected message.
///
/// Mutations killed:
///   - accepting older markers (accepts_older_snapshot)
///   - accepting stale markers when ledger has unsynced additions
///     (accepts_snapshot_when_ledger_changed_after_marker)
///   - accepting same-second markers with a different hash
///     (accepts_same_second_snapshot)
///   - accepting mismatched marker hashes
///     (accepts_marker_not_matching_jsonl)
type SnapshotSetupFn = Box<
    dyn Fn(
        &Path,          // dir
        &Path,          // ledger path (may be mutated)
        &Path,          // input snapshot path
        &PulseSyncMeta, // current sidecar meta
    ) -> String,
>;

#[test]
fn import_rejects_snapshot_with_stale_or_mismatched_marker() {
    struct Case {
        name: &'static str,
        ledger_changed: bool,
        setup: SnapshotSetupFn,
    }

    let cases: Vec<Case> = vec![
        Case {
            name: "older_marker",
            ledger_changed: false,
            setup: Box::new(|_dir, _path, input, _current_meta| {
                write_single_event(input, "shell");
                let input_scan = scan_jsonl(input, |_| Ok(())).unwrap();
                write_sidecar_with_scan(input, &input_scan, 0);
                "not newer".to_string()
            }),
        },
        Case {
            name: "ledger_changed_after_marker",
            ledger_changed: true,
            setup: Box::new(|_dir, path, input, _current_meta| {
                export_jsonl(path, input).unwrap();
                record_event(path, &test_event("shell")).unwrap();
                "unsynced changes".to_string()
            }),
        },
        Case {
            name: "same_second_marker",
            ledger_changed: false,
            setup: Box::new(|_dir, _path, input, current_meta| {
                write_single_event(input, "shell");
                let input_scan = scan_jsonl(input, |_| Ok(())).unwrap();
                write_sidecar_with_scan(input, &input_scan, current_meta.updated_unix);
                "not newer".to_string()
            }),
        },
        Case {
            name: "marker_hash_mismatch",
            ledger_changed: false,
            setup: Box::new(|_dir, _path, input, current_meta| {
                write_single_event(input, "shell");
                write_sidecar_meta(
                    &export_meta_path(input),
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
                "marker does not match".to_string()
            }),
        },
    ];

    for case in &cases {
        let (_dir, path, input, current_meta, before_sha, before_bytes) = setup_import_test();
        let expected_fragment = (case.setup)(_dir.path(), &path, &input, &current_meta);
        assert_import_rejected(&input, &path, &expected_fragment);
        assert_import_post_rejection(
            case.name,
            &path,
            case.ledger_changed,
            &before_sha,
            &before_bytes,
        );
    }
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

// ---------------------------------------------------------------------------
// Session Ledger (bfu)
// ---------------------------------------------------------------------------

#[test]
fn session_ledger_groups_by_session_id() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let mut e1 = test_event("read");
    e1.session_id = Some("sess-a".to_string());
    let mut e2 = test_event("expand");
    e2.session_id = Some("sess-a".to_string());
    let mut e3 = test_event("read");
    e3.session_id = Some("sess-b".to_string());
    record_event(&path, &e1).unwrap();
    record_event(&path, &e2).unwrap();
    record_event(&path, &e3).unwrap();

    let report = SessionLedgerReport::from_ledger(&path).unwrap();
    assert_eq!(report.total_sessions, 2);
    assert_eq!(report.total_turns, 3);
    assert_eq!(report.schema_version, "session-ledger-v1");
    let sess_a = report
        .sessions
        .iter()
        .find(|s| s.session_id == "sess-a")
        .unwrap();
    assert_eq!(sess_a.turns, 2);
    assert_eq!(sess_a.tools.get("read"), Some(&1));
    assert_eq!(sess_a.tools.get("expand"), Some(&1));
}

#[test]
fn session_ledger_handles_no_session_id() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    record_event(&path, &test_event("read")).unwrap();
    let report = SessionLedgerReport::from_ledger(&path).unwrap();
    assert_eq!(report.total_sessions, 1);
    assert_eq!(report.sessions[0].session_id, "unknown");
}

#[test]
fn session_ledger_empty_ledger() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nonexistent.jsonl");
    let report = SessionLedgerReport::from_ledger(&path).unwrap();
    assert_eq!(report.total_sessions, 0);
    assert_eq!(report.total_turns, 0);
}

#[test]
fn session_ledger_schema_json_has_expected_fields() {
    let schema = SessionLedgerReport::schema_json();
    assert_eq!(
        schema["schema_version"],
        serde_json::json!("session-ledger-v1")
    );
    assert!(schema["entry"].is_object());
    assert!(schema["report"].is_object());
    assert!(schema["cli"].is_object());
}
