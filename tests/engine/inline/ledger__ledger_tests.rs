use super::*;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

struct AppendPause {
    entered: Barrier,
    release: Barrier,
}

static APPEND_PAUSE: Mutex<Option<Arc<AppendPause>>> = Mutex::new(None);

pub(super) fn pause_during_append() {
    let pause = APPEND_PAUSE.lock().ok().and_then(|mut slot| slot.take());
    if let Some(pause) = pause {
        pause.entered.wait();
        pause.release.wait();
    }
}

fn test_writer(cache_path: &Path) -> LedgerWriter {
    LedgerWriter::with_max_bytes(
        cache_path,
        "session-test".to_owned(),
        "/workspace/repo".to_owned(),
        vec!["session_dedup:on".to_owned()],
        DEFAULT_MAX_LEDGER_BYTES,
        crate::config::RatcWeights::default(),
    )
}

fn test_record() -> LedgerRecord {
    serde_json::from_value(schema_example()).unwrap()
}

fn buffered_io(writer: &LedgerWriter) -> Arc<LedgerIo> {
    let mode = writer.io.lock().unwrap();
    let LedgerMode::Buffered(io) = &*mode else {
        panic!("writer has not entered buffered mode");
    };
    Arc::clone(io)
}

fn append_record(path: &Path, record: &LedgerRecord, max_bytes: u64) -> io::Result<()> {
    let mut line = serde_json::to_vec(record).map_err(io::Error::other)?;
    line.push(b'\n');
    let line_bytes = u64::try_from(line.len()).unwrap_or(u64::MAX);
    if line_bytes > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ledger record exceeds rotation limit",
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let observed_len = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if observed_len > 0 && observed_len.saturating_add(line_bytes) > max_bytes {
        let lock_path = PathBuf::from(format!("{}.rotation.lock", path.display()));
        let rotation_lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        FileExt::lock(&rotation_lock)?;
        let locked_len = fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if locked_len == observed_len
            && locked_len > 0
            && locked_len.saturating_add(line_bytes) > max_bytes
        {
            rotate_ledger(path, max_bytes)?;
        }
        let _ = FileExt::unlock(&rotation_lock);
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&line)?;
    enforce_ledger_total_bytes(path, max_bytes)
}

#[test]
fn missing_open_ledger_file_is_a_typed_io_error() {
    let mut open_file = None;
    let error = required_ledger_file(&mut open_file).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert!(
        error
            .to_string()
            .contains("ledger file handle is unavailable")
    );
}

#[test]
fn flush_thread_spawn_failure_is_a_typed_io_error() {
    let spawn: io::Result<std::thread::JoinHandle<()>> = Err(io::Error::new(
        io::ErrorKind::OutOfMemory,
        "synthetic spawn failure",
    ));
    let error = flush_thread_result(&spawn).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::OutOfMemory);
    assert!(
        error
            .to_string()
            .contains("failed to start ledger flush scheduler")
    );
}

/// Golden v2 fixture: the stored ratc must equal the RATC identity
/// computed arithmetically from the record's own fields.
const GOLDEN: &str = r#"{
    "schema": "tokenzero.ledger.v2",
    "timestamp_ms": 1700000000000,
    "session_id": "session-golden",
    "repo": "/workspace/repo",
    "agent": "claude-code",
    "version": {"crate": "1.4.0", "git_describe": null},
    "tool": "expand",
    "token_mass": {"visible_tokens": 120, "raw_tokens": 400, "prevented_tokens": 80, "saved_bytes": 1024},
    "cumulative_session_cost_tokens": 120,
    "optimization_tags": ["session_dedup:on"],
    "recovery_costs": {
        "visible_tokens": 120,
        "expand_tokens": 40,
        "expand_count": 2,
        "retry_count": 3,
        "fail_count": 1,
        "rho_fail": 2.5,
        "lambda_fail": 10.0,
        "task_success": true,
        "anchor_recall_ok": false,
        "dangling_ref_count": 1,
        "ratc": 177.5
    }
}"#;

#[test]
fn golden_v2_ratc_identity_holds_arithmetically() {
    let record: LedgerRecord = serde_json::from_str(GOLDEN).unwrap();
    let rc = &record.recovery_costs;
    let expected = (rc.visible_tokens + rc.expand_tokens) as f64
        + rc.rho_fail * rc.retry_count as f64
        + rc.lambda_fail * rc.fail_count as f64;
    // 120 + 40 + 2.5*3 + 10.0*1 = 177.5
    assert_eq!(expected, 177.5);
    assert_eq!(rc.ratc, expected, "stored ratc must equal the identity");
    assert_eq!(rc.compute_ratc(), rc.ratc);
    assert_eq!(record.schema, LEDGER_SCHEMA_V2);
}

#[test]
fn golden_v2_round_trips_without_payload_keys() {
    let record: LedgerRecord = serde_json::from_str(GOLDEN).unwrap();
    let text = serde_json::to_string(&record).unwrap();
    // Telemetry-only rule: no payload bytes in ledger records.
    for key in ["payload", "content", "bytes_b64", "text"] {
        assert!(
            !text.contains(&format!("\"{key}\"")),
            "ledger record must not carry a '{key}' key: {text}"
        );
    }
    let back: LedgerRecord = serde_json::from_str(&text).unwrap();
    assert_eq!(back, record);
}

#[test]
fn v1_lines_remain_readable_with_defaulted_recovery_costs() {
    let v1_line = r#"{
        "schema": "tokenzero.ledger.v1",
        "timestamp_ms": 1700000000000,
        "session_id": "session-old",
        "repo": "/workspace/repo",
        "agent": null,
        "version": {"crate": "1.3.0", "git_describe": null},
        "tool": "read",
        "token_mass": {"visible_tokens": 10, "raw_tokens": 20, "prevented_tokens": 5, "saved_bytes": 64},
        "cumulative_session_cost_tokens": 10,
        "optimization_tags": []
    }"#;
    let record: LedgerRecord = serde_json::from_str(v1_line).unwrap();
    assert_eq!(record.schema, LEDGER_SCHEMA);
    let rc = &record.recovery_costs;
    assert_eq!(rc.expand_tokens, 0);
    assert_eq!(rc.rho_fail, DEFAULT_RHO_FAIL);
    assert_eq!(rc.task_success, None);
    assert_eq!(rc.ratc, 0.0);
    // And read_records accepts the v1 tag on disk (JSONL: one compact
    // record per line).
    let directory = tempdir().unwrap();
    let ledger = directory.path().join("ledger.jsonl");
    let compact = serde_json::to_string(&record).unwrap();
    let compact = compact.replace(LEDGER_SCHEMA_V2, LEDGER_SCHEMA);
    fs::write(&ledger, format!("{compact}\n")).unwrap();
    let records = read_records(&ledger).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].schema, LEDGER_SCHEMA);
}

#[allow(clippy::too_many_arguments)]
fn task_record(
    task_id: &str,
    visible: u64,
    expand: u64,
    retries: u64,
    fails: u64,
    success: Option<bool>,
    expand_count: u64,
    dangling_refs: u64,
) -> LedgerRecord {
    let mut record = test_record();
    record.session_id = task_id.to_owned();
    record.token_mass.visible_tokens = visible;
    record.recovery_costs = RecoveryCosts {
        visible_tokens: visible,
        expand_tokens: expand,
        expand_count,
        retry_count: retries,
        fail_count: fails,
        rho_fail: 2.0,
        lambda_fail: 10.0,
        task_success: success,
        anchor_recall_ok: None,
        dangling_ref_count: dangling_refs,
        ratc: 0.0,
    }
    .with_ratc();
    record
}

#[test]
fn task_cost_report_matches_hand_computed_json_and_csv_golden() {
    let directory = tempdir().unwrap();
    let ledger = directory.path().join("ledger.jsonl");
    let json_output = directory.path().join("reports/tasks.json");
    let csv_output = directory.path().join("reports/tasks.csv");
    let records = [
        task_record("task,a", 10, 4, 1, 0, Some(true), 1, 1),
        task_record("task,a", 6, 1, 0, 0, None, 1, 0),
        task_record("task-b", 8, 2, 1, 1, Some(true), 1, 2),
    ];
    let fixture = records
        .iter()
        .map(|record| serde_json::to_string(record).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&ledger, format!("{fixture}\n")).unwrap();
    let report = write_task_cost_report(&ledger, &json_output, &csv_output).unwrap();
    assert_eq!(
        report,
        TaskCostReport {
            schema: "tokenzero.task-cost-report.v1".to_owned(),
            task_count: 2,
            successful_tasks: 1,
            success_rate: 0.5,
            tasks: vec![
                TaskCostSummary {
                    task_id: "task,a".to_owned(),
                    success: true,
                    visible: 16,
                    expand: 5,
                    retries: 1,
                    fails: 0,
                    ratc: 23.0,
                    expand_count: 2,
                    dangling_refs: 1
                },
                TaskCostSummary {
                    task_id: "task-b".to_owned(),
                    success: false,
                    visible: 8,
                    expand: 2,
                    retries: 1,
                    fails: 1,
                    ratc: 22.0,
                    expand_count: 1,
                    dangling_refs: 2
                },
            ],
        }
    );
    let json_back: TaskCostReport =
        serde_json::from_slice(&fs::read(json_output).unwrap()).unwrap();
    assert_eq!(json_back, report);
    let expected_csv = concat!(
        "task_id,success,visible,expand,retries,fails,ratc,expand_count,dangling_refs\n",
        "\"task,a\",true,16,5,1,0,23,2,1\n",
        "task-b,false,8,2,1,1,22,1,2\n",
    );
    assert_eq!(fs::read_to_string(csv_output).unwrap(), expected_csv);
}

#[test]
fn task_cost_report_kill_before_rename_keeps_previous_complete_files() {
    let directory = tempdir().unwrap();
    let ledger = directory.path().join("ledger.jsonl");
    let json_output = directory.path().join("reports/tasks.json");
    let csv_output = directory.path().join("reports/tasks.csv");
    let record = task_record("task-a", 10, 4, 1, 0, Some(true), 1, 1);
    fs::write(
        &ledger,
        format!("{}\n", serde_json::to_string(&record).unwrap()),
    )
    .unwrap();
    write_task_cost_report(&ledger, &json_output, &csv_output).unwrap();
    let previous_json = fs::read(&json_output).unwrap();
    let previous_csv = fs::read(&csv_output).unwrap();
    serde_json::from_slice::<TaskCostReport>(&previous_json).expect("live json must parse");

    // Kill point: P1 wrote new report bytes to tmp and died before rename.
    // A later reader of tasks.json / tasks.csv must still see the previous
    // complete report, not an empty/partial file from truncate-in-place.
    let torn_json = json_output.with_file_name(".tasks.json.kill-mid-rename.tmp");
    let torn_csv = csv_output.with_file_name(".tasks.csv.kill-mid-rename.tmp");
    fs::write(&torn_json, b"{\"schema\":").unwrap();
    fs::write(&torn_csv, b"task_id,").unwrap();
    assert_eq!(fs::read(&json_output).unwrap(), previous_json);
    assert_eq!(fs::read(&csv_output).unwrap(), previous_csv);
    serde_json::from_slice::<TaskCostReport>(&fs::read(&json_output).unwrap()).unwrap();

    // Contrast: the old `fs::write` kill window truncates dest first.
    let truncated = directory.path().join("reports/truncated.json");
    fs::write(&truncated, &previous_json).unwrap();
    OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&truncated)
        .unwrap();
    assert!(
        serde_json::from_slice::<TaskCostReport>(&fs::read(&truncated).unwrap()).is_err(),
        "truncate(true) on the live report is the torn-file kill window"
    );

    write_task_cost_report(&ledger, &json_output, &csv_output).unwrap();
    serde_json::from_slice::<TaskCostReport>(&fs::read(&json_output).unwrap()).unwrap();
    let temps: Vec<_> = fs::read_dir(json_output.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-"))
        .collect();
    assert!(
        temps.is_empty(),
        "successful publish must not leave zero_store temps: {temps:?}"
    );
}

#[test]
fn tz_evict_amortized_charge_round_trips_through_ledger() {
    let charge = test_record()
        .eviction_amortization
        .expect("eviction charge");
    assert_eq!(charge["amortized_tokens_per_access"], 20.0);
    assert_eq!(charge["thrash_worst_case_tokens"], 120);
    assert_eq!(charge["alarm"], false);
}

#[test]
fn first_record_is_persisted_without_scheduler_registration() {
    let directory = tempdir().unwrap();
    let cache_path = directory.path().join("cache.json");
    let ledger_path = ledger_path_for_cache(&cache_path);
    let writer = test_writer(&cache_path);

    writer.append_record(&test_record()).unwrap();

    let mode = writer.io.lock().unwrap();
    let LedgerMode::Direct {
        open_file,
        accepted_record,
    } = &*mode
    else {
        panic!("one record must not allocate buffered mode");
    };
    assert!(*accepted_record);
    assert!(open_file.is_some());
    drop(mode);
    assert_eq!(read_records(&ledger_path).unwrap(), vec![test_record()]);
}

#[test]
fn tzg0vj_record_response_charges_zero_ledger() {
    let directory = tempdir().unwrap();
    let cache_path = directory.path().join("cache.json");
    let ledger_path = ledger_path_for_cache(&cache_path);
    let writer = test_writer(&cache_path);
    let mut response = ToolResponse::default();
    response.accounting = Some(Accounting {
        raw_tokens: 200,
        visible_tokens: 50,
        recovery_tokens: 0,
        billed_tokens: 50,
        ..Accounting::default()
    });
    writer.record_response("read", &response);
    writer.flush();
    let records = read_records(&ledger_path).unwrap();
    assert_eq!(records.len(), 1);
    let charge = records[0]
        .racc_charge
        .as_ref()
        .expect("live charge fragment");
    assert_eq!(charge["schema"], "tokenzero.racc_charge.v1");
    assert_eq!(charge["billed_tokens"], 50);
    assert_eq!(charge["recovery_tokens"], 0);
    assert_eq!(charge["charge_count"], 1);
}

#[test]
fn typed_expand_miss_is_not_dropped_without_accounting() {
    let directory = tempdir().unwrap();
    let cache_path = directory.path().join("cache.json");
    let ledger_path = ledger_path_for_cache(&cache_path);
    let writer = test_writer(&cache_path);
    let mut response = ToolResponse::error(
        "expand",
        "dangling_ref",
        "dangling ref",
        Some("re-run producer".to_owned()),
    );
    response.telemetry = Some(json!({
        "expand": {
            "fail_count": 1,
            "dangling_ref_count": 1,
            "miss_kind": "dangling_ref",
        }
    }));

    writer.record_response("expand", &response);
    writer.flush();

    let records = read_records(&ledger_path).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].token_mass.visible_tokens, 0);
    assert_eq!(records[0].token_mass.raw_tokens, 0);
    assert_eq!(records[0].recovery_costs.fail_count, 1);
    assert_eq!(records[0].recovery_costs.dangling_ref_count, 1);
}

#[test]
fn flush_window_boundary_is_deterministic() {
    let directory = tempdir().unwrap();
    let cache_path = directory.path().join("cache.json");
    let ledger_path = ledger_path_for_cache(&cache_path);
    let writer = test_writer(&cache_path);
    writer.append_record(&test_record()).unwrap();
    writer.append_record(&test_record()).unwrap();
    let io = buffered_io(&writer);
    let buffered_at = io
        .state
        .lock()
        .unwrap()
        .buffered_at
        .expect("second record is buffered");

    io.flush_if_due(buffered_at + LEDGER_FLUSH_WINDOW - Duration::from_nanos(1));
    assert_eq!(read_records(&ledger_path).unwrap(), vec![test_record()]);
    io.flush_if_due(buffered_at + LEDGER_FLUSH_WINDOW);

    assert_eq!(
        read_records(&ledger_path).unwrap(),
        vec![test_record(), test_record()]
    );
}

#[test]
fn low_volume_record_flushes_after_bounded_window() {
    let directory = tempdir().unwrap();
    let cache_path = directory.path().join("cache.json");
    let ledger_path = ledger_path_for_cache(&cache_path);
    let writer = test_writer(&cache_path);

    writer.append_record(&test_record()).unwrap();
    writer.append_record(&test_record()).unwrap();
    assert_eq!(read_records(&ledger_path).unwrap(), vec![test_record()]);

    let deadline = Instant::now() + LEDGER_FLUSH_WINDOW + Duration::from_secs(2);
    while read_records(&ledger_path).unwrap().len() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        read_records(&ledger_path).unwrap(),
        vec![test_record(), test_record()]
    );
}

#[test]
fn explicit_flush_drains_low_volume_record() {
    let directory = tempdir().unwrap();
    let cache_path = directory.path().join("cache.json");
    let ledger_path = ledger_path_for_cache(&cache_path);
    let writer = test_writer(&cache_path);

    writer.append_record(&test_record()).unwrap();
    writer.append_record(&test_record()).unwrap();
    writer.flush();

    assert_eq!(
        read_records(&ledger_path).unwrap(),
        vec![test_record(), test_record()]
    );
}

#[test]
fn failed_timed_flush_is_retained_for_explicit_retry() {
    let directory = tempdir().unwrap();
    let blocked_parent = directory.path().join("blocked");
    fs::write(&blocked_parent, b"not a directory").unwrap();
    let cache_path = blocked_parent.join("cache.json");
    let ledger_path = ledger_path_for_cache(&cache_path);
    let writer = test_writer(&cache_path);
    writer.append_record(&test_record()).unwrap();
    let io = buffered_io(&writer);
    let buffered_at = io
        .state
        .lock()
        .unwrap()
        .buffered_at
        .expect("record is buffered");

    let failed_at = buffered_at + LEDGER_FLUSH_WINDOW;
    io.flush_if_due(failed_at);
    assert_eq!(io.state.lock().unwrap().buffered_at, Some(failed_at));

    fs::remove_file(&blocked_parent).unwrap();
    fs::create_dir(&blocked_parent).unwrap();
    writer.flush();
    assert_eq!(read_records(&ledger_path).unwrap(), vec![test_record()]);
}

#[test]
fn drop_flushes_low_volume_record() {
    let directory = tempdir().unwrap();
    let cache_path = directory.path().join("cache.json");
    let ledger_path = ledger_path_for_cache(&cache_path);
    let writer = test_writer(&cache_path);

    writer.append_record(&test_record()).unwrap();
    writer.append_record(&test_record()).unwrap();
    drop(writer);

    assert_eq!(
        read_records(&ledger_path).unwrap(),
        vec![test_record(), test_record()]
    );
}

#[test]
fn retained_handle_reopens_after_external_rotation() {
    let directory = tempdir().unwrap();
    let cache_path = directory.path().join("cache.json");
    let path = ledger_path_for_cache(&cache_path);
    let rotated = rotated_path(&path);
    let first_writer = test_writer(&cache_path);

    first_writer.append_record(&test_record()).unwrap();
    fs::rename(&path, &rotated).unwrap();

    let second_writer = test_writer(&cache_path);
    second_writer.append_record(&test_record()).unwrap();
    first_writer.append_record(&test_record()).unwrap();
    first_writer.flush();

    assert_eq!(fs::read_to_string(&rotated).unwrap().lines().count(), 1);
    assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
}

#[test]
fn concurrent_rotation_does_not_rotate_twice() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("ledger.jsonl");
    let record: LedgerRecord = serde_json::from_value(schema_example()).unwrap();
    let line_len = serde_json::to_vec(&record).unwrap().len() as u64 + 1;
    let max_bytes = line_len + 8;
    let original = vec![b'x'; 9];
    fs::write(&path, &original).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let path = path.clone();
        let record = record.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            append_record(&path, &record, max_bytes).unwrap();
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(fs::read(rotated_path(&path)).unwrap(), original);
    assert_eq!(read_records(&path).unwrap().len(), 2);
}

#[test]
fn ledger_rotation_caps_generations_and_total_bytes() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("ledger.jsonl");
    for generation in 0..(DEFAULT_MAX_LEDGER_GENERATIONS + 2) {
        fs::write(&path, [generation as u8]).unwrap();
        rotate_ledger(&path, DEFAULT_MAX_LEDGER_BYTES).unwrap();
    }
    for generation in 1..=DEFAULT_MAX_LEDGER_GENERATIONS {
        assert!(rotated_path_at(&path, generation).is_file());
    }
    assert!(!rotated_path_at(&path, DEFAULT_MAX_LEDGER_GENERATIONS + 1).exists());

    fs::write(&path, [0]).unwrap();
    for generation in 1..=DEFAULT_MAX_LEDGER_GENERATIONS {
        OpenOptions::new()
            .write(true)
            .open(rotated_path_at(&path, generation))
            .unwrap()
            .set_len(10 * 1024 * 1024)
            .unwrap();
    }
    enforce_ledger_total_bytes(&path, DEFAULT_MAX_LEDGER_BYTES).unwrap();
    let total = std::iter::once(path.clone())
        .chain(
            (1..=DEFAULT_MAX_LEDGER_GENERATIONS)
                .map(|generation| rotated_path_at(&path, generation)),
        )
        .filter_map(|candidate| fs::metadata(candidate).ok())
        .map(|metadata| metadata.len())
        .sum::<u64>();
    assert!(total <= DEFAULT_MAX_LEDGER_TOTAL_BYTES);
}

#[test]
fn record_response_drops_cumulative_before_append_io() {
    let directory = tempdir().unwrap();
    let cache_path = directory.path().join("cache.json");
    let writer = Arc::new(test_writer(&cache_path));
    let pause = Arc::new(AppendPause {
        entered: Barrier::new(2),
        release: Barrier::new(2),
    });
    *APPEND_PAUSE.lock().unwrap() = Some(Arc::clone(&pause));

    let mut response = ToolResponse::default();
    response.accounting = Some(Accounting {
        raw_tokens: 200,
        visible_tokens: 50,
        recovery_tokens: 0,
        billed_tokens: 50,
        ..Accounting::default()
    });
    let serving = Arc::clone(&writer);
    let worker = thread::spawn(move || serving.record_response("read", &response));
    pause.entered.wait();
    let cumulative_free = writer.cumulative_visible_tokens.try_lock().is_ok();
    pause.release.wait();
    worker.join().unwrap();
    assert!(
        cumulative_free,
        "cumulative_visible_tokens must not stay held across append_record I/O"
    );
}

fn accounting_response(visible_tokens: usize) -> ToolResponse {
    let mut response = ToolResponse::default();
    response.accounting = Some(Accounting {
        raw_tokens: visible_tokens.saturating_mul(2),
        visible_tokens,
        recovery_tokens: 0,
        billed_tokens: visible_tokens,
        ..Accounting::default()
    });
    response
}

#[test]
fn concurrent_record_response_keeps_cumulative_prefix_sum() {
    let directory = tempdir().unwrap();
    let cache_path = directory.path().join("cache.json");
    let ledger_path = ledger_path_for_cache(&cache_path);
    let writer = Arc::new(test_writer(&cache_path));
    let pause = Arc::new(AppendPause {
        entered: Barrier::new(2),
        release: Barrier::new(2),
    });
    *APPEND_PAUSE.lock().unwrap() = Some(Arc::clone(&pause));

    let serving = Arc::clone(&writer);
    let worker = thread::spawn(move || serving.record_response("read", &accounting_response(50)));
    pause.entered.wait();
    // T1 is paused before the persist gate (`io`). T2 finishes a full
    // record_response in that window. Durable lines must still satisfy
    // prefix-sum(cumulative) regardless of which thread wins `io`.
    writer.record_response("read", &accounting_response(70));
    pause.release.wait();
    worker.join().unwrap();
    writer.flush();

    let records = read_records(&ledger_path).unwrap();
    assert_eq!(records.len(), 2);
    let mut prefix = 0_u64;
    for record in &records {
        prefix = prefix.saturating_add(record.token_mass.visible_tokens);
        assert_eq!(
            record.cumulative_session_cost_tokens, prefix,
            "JSONL running total must equal prefix-sum of visible_tokens in file order"
        );
    }
    assert_eq!(prefix, 120);
}
