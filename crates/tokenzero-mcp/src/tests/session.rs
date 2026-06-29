use super::*;
use std::sync::Arc;
use tempfile::tempdir;
use tokenzero_core::MCP_SCHEMA_VERSION;

use super::support::*;


#[test]
fn passthrough_zero_matches_keeps_verbatim_empty_payload() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn alpha() {}\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.grep(
        "nomatch",
        &[dir.path().to_path_buf()],
        Mode::Passthrough,
        20,
        4000,
    );

    assert_eq!(response.status, "ok");
    assert_eq!(response.visible.as_ref().unwrap().text, "");
}

#[test]
fn pipelined_identical_reads_dedup_exactly_once() {
    // Two reads of the same file issued concurrently on a shared engine must
    // not both serve full: the single-flight gate makes the second wait for
    // the first to record, so it dedups. Before the fix both raced the
    // seen-set and both served full (the unreproducible repeat-read bench).
    let dir = tempdir().unwrap();
    let file = dir.path().join("big.rs");
    let body: String = (0..400)
        .map(|i| format!("line {i} content here\n"))
        .collect();
    fs::write(&file, &body).unwrap();

    let engine = Arc::new(TokenZeroEngine::new(EngineConfig::for_root(dir.path())));
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let engine = Arc::clone(&engine);
            let barrier = Arc::clone(&barrier);
            let path = file.clone();
            std::thread::spawn(move || {
                barrier.wait();
                engine.read(&[path], Mode::Auto, None, None, false, 20, 4000)
            })
        })
        .collect();
    let responses: Vec<ToolResponse> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    let dedup_notes = responses
        .iter()
        .filter(|r| {
            r.telemetry
                .as_ref()
                .and_then(|t| t.get("output_strategy"))
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("seen_set_dedup"))
        })
        .count();
    assert_eq!(
        dedup_notes, 1,
        "exactly one of two concurrent identical reads must dedup"
    );
}

#[test]
fn unchanged_note_ref_expands_to_full_bytes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    let content = dedup_fixture_content();
    fs::write(&file, &content).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    let second = read_ok(&engine, &file);
    let note = visible_text(&second);
    // Refs are freshly minted per serve, so the note's embedded refs are
    // exactly the ones carried by the response.
    let blob_ref = second
        .refs
        .iter()
        .find(|record| record.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    assert!(note.contains(&blob_ref), "{note}");
    let expanded = engine.expand(&blob_ref, Some("raw"), None, None, None, None);
    assert_eq!(expanded.status, "ok");
    assert_eq!(expanded.visible.unwrap().text, content);
}

#[test]
fn tiny_file_roi_guard_serves_full() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("tiny.txt");
    fs::write(&file, "hi\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    let second = read_ok(&engine, &file);
    assert_eq!(visible_text(&second), "hi");
    // The rejected note leaves no dedup telemetry behind.
    assert!(second.telemetry.is_none(), "{:?}", second.telemetry);
}

#[test]
fn session_dedup_config_off_serves_full_and_records_nothing() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.session_dedup = false;
    let engine = TokenZeroEngine::new(config);

    read_ok(&engine, &file);
    let second = read_ok(&engine, &file);
    assert!(visible_text(&second).contains("line 01"));
    assert!(!visible_text(&second).contains("unchanged:"));
    let status: Value = serde_json::from_str(&engine.mem().visible.unwrap().text).unwrap();
    assert_eq!(status["session_dedup"]["records"], 0);
}

#[test]
fn mtime_touch_with_same_bytes_still_dedups() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    let content = dedup_fixture_content();
    fs::write(&file, &content).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    // Rewrite the identical bytes: mtime moves, the content hash — the
    // only invalidation source — does not.
    fs::write(&file, &content).unwrap();
    let second = read_ok(&engine, &file);
    assert!(visible_text(&second).starts_with("unchanged:"));
}

#[test]
fn changed_file_serves_diff_when_cheaper() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    let changed = dedup_fixture_content().replace(
        "line 20: session redundancy fixture content wide enough to out-cost a note",
        "line 20: MODIFIED for the diff-aware re-read test",
    );
    fs::write(&file, &changed).unwrap();
    let second = read_ok(&engine, &file);
    let text = visible_text(&second);
    assert!(
        text.contains("changed since served this session (diff vs tz://blob/"),
        "{text}"
    );
    assert!(text.contains("@@"), "{text}");
    assert!(text.contains("+line 20: MODIFIED"), "{text}");
    assert!(text.contains("-line 20: session redundancy"), "{text}");
    assert!(text.contains("full file: expand tz://blob/"), "{text}");
    let accounting = second.accounting.as_ref().unwrap();
    assert!(accounting.visible_tokens < accounting.raw_tokens);
    // The base expansion is charged as recovery tokens.
    assert!(accounting.recovery_tokens > 0);
    let telemetry = second.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["output_strategy"], "diff_since_served");
    assert_eq!(telemetry["cache_hit"], true);
    assert!(telemetry["diff"]["hunks"].as_u64().unwrap() >= 1);
    assert!(telemetry["diff"]["plus"].as_u64().unwrap() >= 1);
    assert!(telemetry["diff"]["minus"].as_u64().unwrap() >= 1);
    assert!(
        telemetry["diff"]["base_ref"]
            .as_str()
            .unwrap()
            .starts_with("tz://blob/")
    );
}

#[test]
fn fully_rewritten_file_serves_full() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    let rewritten = (1..=40)
        .map(|index| format!("row {index:02}: a complete rewrite shares no line with the original"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(&file, &rewritten).unwrap();
    let second = read_ok(&engine, &file);
    let text = visible_text(&second);
    assert!(text.contains("row 01"), "{text}");
    assert!(
        !text.contains("changed since served this session"),
        "{text}"
    );
    assert!(!text.contains("unchanged:"), "{text}");
}

#[test]
fn missing_diff_base_falls_back_to_full() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    // Prune the recovery cache: the diff base is gone.
    fs::remove_file(&engine.config.cache_path).unwrap();
    let changed = dedup_fixture_content().replace("line 20:", "line 20 (changed):");
    fs::write(&file, &changed).unwrap();
    let second = read_ok(&engine, &file);
    let text = visible_text(&second);
    assert!(
        !text.contains("changed since served this session"),
        "{text}"
    );
    assert!(text.contains("line 20 (changed):"), "{text}");
    // The record was replaced: the next identical read dedups again.
    let third = read_ok(&engine, &file);
    assert!(visible_text(&third).starts_with("unchanged:"));
}

#[test]
fn range_keyed_reads_dedup_separately() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let range_read = |start: usize, end: usize| -> ToolResponse {
        let response = engine.read(
            std::slice::from_ref(&file),
            Mode::Auto,
            Some(start),
            Some(end),
            false,
            20,
            4000,
        );
        assert_eq!(response.status, "ok", "{:?}", response.error);
        response
    };

    range_read(1, 5);
    let repeat = range_read(1, 5);
    assert!(visible_text(&repeat).starts_with("unchanged:"));
    // A different range is a different key: no dedup.
    let other_range = range_read(2, 6);
    assert!(!visible_text(&other_range).contains("unchanged:"));
    // The original range still notes.
    let again = range_read(1, 5);
    assert!(visible_text(&again).starts_with("unchanged:"));
}

#[test]
fn diff_reads_config_off_serves_full_on_change() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.diff_reads = false;
    let engine = TokenZeroEngine::new(config);

    read_ok(&engine, &file);
    let changed = dedup_fixture_content().replace("line 20:", "line 20 (changed):");
    fs::write(&file, &changed).unwrap();
    let second = read_ok(&engine, &file);
    let text = visible_text(&second);
    assert!(
        !text.contains("changed since served this session"),
        "{text}"
    );
    assert!(text.contains("line 20 (changed):"), "{text}");
    // Seen-set dedup stays active with diffing off.
    let third = read_ok(&engine, &file);
    assert!(visible_text(&third).starts_with("unchanged:"));
}

#[test]
fn mem_reports_session_dedup_rollup() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    read_ok(&engine, &file);
    let status: Value = serde_json::from_str(&engine.mem().visible.unwrap().text).unwrap();
    let rollup = &status["session_dedup"];
    assert_eq!(rollup["records"], 1);
    assert_eq!(rollup["dedup_hits"], 1);
    assert_eq!(rollup["diff_hits"], 0);
    assert!(rollup["visible_tokens_saved"].as_u64().unwrap() > 0);
    assert_eq!(rollup["diff_tokens_saved"], 0);
}

#[test]
fn degraded_storage_serves_full_instead_of_dedup_note() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let cache_dir = dir.path().join("cache-as-directory");
    fs::create_dir_all(&cache_dir).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache_dir;
    let engine = TokenZeroEngine::new(config);

    read_ok(&engine, &file);
    let second = read_ok(&engine, &file);
    assert_eq!(
        second.diagnostic.as_ref().unwrap().code,
        "cache_write_failed"
    );
    // A note would advertise refs that never persisted; degraded storage
    // must serve the full bytes and record nothing in the seen-set.
    let text = visible_text(&second);
    assert!(text.contains("line 01"), "{text}");
    assert!(!text.contains("unchanged:"), "{text}");
    let telemetry = second.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["degraded"], true);
    assert_eq!(telemetry["transport_status"], "degraded");
    assert!(
        telemetry.get("dedup").is_none_or(Value::is_null),
        "{telemetry}"
    );
    let status: Value = serde_json::from_str(&engine.mem().visible.unwrap().text).unwrap();
    assert_eq!(status["session_dedup"]["records"], 0);
    assert_eq!(status["session_dedup"]["dedup_hits"], 0);
}

#[test]
fn mid_session_degradation_serves_full_not_stale_note() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let cache_path = dir.path().join("cache.json");
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache_path.clone();
    let engine = TokenZeroEngine::new(config);

    let first = read_ok(&engine, &file);
    assert!(!first.refs.is_empty());
    // Storage dies between the serves: the seen-set still has the
    // record, but a note would advertise refs this call failed to mint.
    fs::remove_file(&cache_path).unwrap();
    fs::create_dir_all(&cache_path).unwrap();
    let second = read_ok(&engine, &file);
    let text = visible_text(&second);
    assert!(text.contains("line 01"), "{text}");
    assert!(!text.contains("unchanged:"), "{text}");
    assert_eq!(
        second.diagnostic.as_ref().unwrap().code,
        "cache_write_failed"
    );
}

#[test]
fn poisoned_session_mutex_fails_open() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let poisoner = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = engine.session.lock().unwrap();
        panic!("poison the session mutex");
    }));
    assert!(poisoner.is_err());
    assert!(engine.session.lock().is_err(), "mutex must be poisoned");

    let first = read_ok(&engine, &file);
    let second = read_ok(&engine, &file);
    assert!(visible_text(&first).contains("line 01"));
    assert!(visible_text(&second).contains("line 01"));
    assert!(!visible_text(&second).contains("unchanged:"));
    let status: Value = serde_json::from_str(&engine.mem().visible.unwrap().text).unwrap();
    assert_eq!(status["session_dedup"]["poisoned"], true);
}

#[test]
fn concurrent_reads_keep_session_consistent() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("shared.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for _ in 0..4 {
                    let response = engine.read(
                        std::slice::from_ref(&file),
                        Mode::Auto,
                        None,
                        None,
                        false,
                        20,
                        4000,
                    );
                    assert_eq!(response.status, "ok", "{:?}", response.error);
                }
            });
        }
    });
    // No deadlock, no poisoning: the seen-set still answers afterwards.
    let after = read_ok(&engine, &file);
    assert!(visible_text(&after).starts_with("unchanged:"));
}

#[test]
fn tool_metrics_records_session_calls() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("hello.txt");
    std::fs::write(&file, "hello metrics\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    for _ in 0..3 {
        call_tool(
            &engine,
            "read",
            &json!({ "path": file.display().to_string() }),
            None,
        )
        .unwrap();
    }

    let snap = engine.tool_metrics_snapshot();
    assert_eq!(snap["status"], "ok");
    assert!(snap["slow_threshold_ms"].as_u64().unwrap() > 0);
    assert_eq!(
        snap["session"]["tools"]["read"]["calls"].as_u64().unwrap(),
        3,
        "session counters track each read call"
    );
}
