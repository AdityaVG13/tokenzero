use super::*;
use super::support::*;
use std::sync::Arc;

#[test]
fn pipelined_identical_reads_dedup_exactly_once() {
    let (dir, engine) = setup_default();
    let file = dir.path().join("big.rs");
    let body: String = (0..400).map(|i| format!("line {i} content here\n")).collect();
    fs::write(&file, &body).unwrap();
    let engine = Arc::new(engine);
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
    assert_eq!(dedup_notes, 1, "exactly one of two concurrent identical reads must dedup");
}

#[test]
fn unchanged_note_ref_expands_to_full_bytes() {
    let (_dir, file, engine, content) = setup_dedup("sample.rs");
    read_ok(&engine, &file);
    let second = read_ok(&engine, &file);
    let note = visible_text(&second);
    let blob = blob_ref(&second);
    assert!(note.contains(&blob), "{note}");
    assert_eq!(expand_ok(&engine, &blob), content);
}

#[test]
fn tiny_file_roi_guard_serves_full() {
    let (_dir, file, engine) = setup_file("tiny.txt", "hi\n");
    read_ok(&engine, &file);
    let second = read_ok(&engine, &file);
    assert_eq!(visible_text(&second), "hi");
    let delta = &second.telemetry.as_ref().unwrap()["session_delta"];
    assert_eq!(delta["full_bytes"], delta["delta_bytes"]);
}

#[test]
fn session_dedup_config_off_serves_full_and_records_nothing() {
    let (_dir, file, engine, _) = setup_dedup_off("sample.rs");
    read_ok(&engine, &file);
    let second = read_ok(&engine, &file);
    assert!(visible_text(&second).contains("line 01"));
    assert!(!visible_text(&second).contains("unchanged:"));
    assert_eq!(mem_status(&engine)["session_dedup"]["records"], 0);
}

#[test]
fn mtime_touch_with_same_bytes_still_dedups() {
    let (_dir, file, engine, content) = setup_dedup("sample.rs");
    read_ok(&engine, &file);
    fs::write(&file, &content).unwrap();
    assert!(visible_text(&read_ok(&engine, &file)).starts_with("unchanged:"));
}

#[test]
fn changed_file_serves_diff_when_cheaper() {
    let (_dir, file, engine, _) = setup_dedup("sample.rs");
    read_ok(&engine, &file);
    let changed = dedup_fixture_content().replace(
        "line 20: session redundancy fixture content wide enough to out-cost a note",
        "line 20: MODIFIED for the diff-aware re-read test",
    );
    fs::write(&file, &changed).unwrap();
    let second = read_ok(&engine, &file);
    let text = visible_text(&second);
    for needle in [
        "changed since served this session (diff vs tz://blob/",
        "@@",
        "+line 20: MODIFIED",
        "-line 20: session redundancy",
        "full file: expand tz://blob/",
    ] {
        assert!(text.contains(needle), "{text}");
    }
    let accounting = second.accounting.as_ref().unwrap();
    assert!(accounting.visible_tokens < accounting.raw_tokens);
    assert!(accounting.recovery_tokens > 0);
    let telemetry = second.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["output_strategy"], "diff_since_served");
    assert_eq!(telemetry["cache_hit"], true);
    for key in ["hunks", "plus", "minus"] {
        assert!(telemetry["diff"][key].as_u64().unwrap() >= 1);
    }
    assert!(telemetry["diff"]["base_ref"].as_str().unwrap().starts_with("tz://blob/"));
}

#[test]
fn fully_rewritten_file_serves_full() {
    let (_dir, file, engine, _) = setup_dedup("sample.rs");
    read_ok(&engine, &file);
    let rewritten = (1..=40).map(|i| format!("row {i:02}: a complete rewrite shares no line with the original")).collect::<Vec<_>>().join("\n") + "\n";
    fs::write(&file, &rewritten).unwrap();
    let text = visible_text(&read_ok(&engine, &file));
    assert!(text.contains("row 01"), "{text}");
    assert!(!text.contains("changed since served this session"), "{text}");
    assert!(!text.contains("unchanged:"), "{text}");
}

#[test]
fn missing_diff_base_falls_back_to_full() {
    let dir = tempdir().unwrap();
    let ref_index = dir.path().join("ref-index");
    tokenzero_recovery::set_ref_index_root_override(Some(ref_index.clone()));
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = default_engine(dir.path());
    read_ok(&engine, &file);
    fs::remove_file(&engine.config.cache_path).unwrap();
    let _ = fs::remove_dir_all(&ref_index);
    fs::write(&file, dedup_fixture_content().replace("line 20:", "line 20 (changed):")).unwrap();
    let second = read_ok(&engine, &file);
    let text = visible_text(&second);
    assert!(!text.contains("changed since served this session"), "{text}");
    assert!(text.contains("line 20 (changed):"), "{text}");
    assert!(visible_text(&read_ok(&engine, &file)).starts_with("unchanged:"));
}

#[test]
fn range_keyed_reads_dedup_separately() {
    let (_dir, file, engine, _) = setup_dedup("sample.rs");
    let range_read = |start: usize, end: usize| {
        let response = engine.read(std::slice::from_ref(&file), Mode::Auto, Some(start), Some(end), false, 20, 4000);
        assert_status_ok(&response);
        response
    };
    range_read(1, 5);
    assert!(visible_text(&range_read(1, 5)).starts_with("unchanged:"));
    assert!(!visible_text(&range_read(2, 6)).contains("unchanged:"));
    assert!(visible_text(&range_read(1, 5)).starts_with("unchanged:"));
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
    fs::write(&file, dedup_fixture_content().replace("line 20:", "line 20 (changed):")).unwrap();
    let text = visible_text(&read_ok(&engine, &file));
    assert!(!text.contains("changed since served this session"), "{text}");
    assert!(text.contains("line 20 (changed):"), "{text}");
    assert!(visible_text(&read_ok(&engine, &file)).starts_with("unchanged:"));
}

#[test]
fn mem_reports_session_dedup_rollup() {
    let (_dir, file, engine, _) = setup_dedup("sample.rs");
    read_ok(&engine, &file);
    read_ok(&engine, &file);
    let rollup = &mem_status(&engine)["session_dedup"];
    assert_eq!(rollup["records"], 1);
    assert_eq!(rollup["dedup_hits"], 1);
    assert_eq!(rollup["diff_hits"], 0);
    assert!(rollup["visible_tokens_saved"].as_u64().unwrap() > 0);
    assert_eq!(rollup["diff_tokens_saved"], 0);
}

#[test]
fn degraded_storage_serves_full_instead_of_dedup_note() {
    let (_dir, file, engine) = setup_unwritable("sample.rs", dedup_fixture_content());
    read_ok(&engine, &file);
    let second = read_ok(&engine, &file);
    assert_eq!(second.diagnostic.as_ref().unwrap().code, "cache_write_failed");
    let text = visible_text(&second);
    assert!(text.contains("line 01"), "{text}");
    assert!(!text.contains("unchanged:"), "{text}");
    let telemetry = second.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["degraded"], true);
    assert_eq!(telemetry["transport_status"], "degraded");
    assert!(telemetry.get("dedup").is_none_or(Value::is_null), "{telemetry}");
    let status = mem_status(&engine);
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
    assert!(!read_ok(&engine, &file).refs.is_empty());
    fs::remove_file(&cache_path).unwrap();
    fs::create_dir_all(&cache_path).unwrap();
    let second = read_ok(&engine, &file);
    let text = visible_text(&second);
    assert!(text.contains("line 01"), "{text}");
    assert!(!text.contains("unchanged:"), "{text}");
    assert_eq!(second.diagnostic.as_ref().unwrap().code, "cache_write_failed");
}

#[test]
fn poisoned_session_mutex_fails_open() {
    let (_dir, file, engine, _) = setup_dedup("sample.rs");
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = engine.session.lock().unwrap();
        panic!("poison the session mutex");
    }))
    .is_err());
    assert!(engine.session.lock().is_err());
    let first = read_ok(&engine, &file);
    let second = read_ok(&engine, &file);
    assert!(visible_text(&first).contains("line 01"));
    assert!(visible_text(&second).contains("line 01"));
    assert!(!visible_text(&second).contains("unchanged:"));
    assert_eq!(mem_status(&engine)["session_dedup"]["poisoned"], true);
}

#[test]
fn concurrent_reads_keep_session_consistent() {
    let (_dir, file, engine, _) = setup_dedup("shared.rs");
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for _ in 0..4 {
                    assert_status_ok(&engine.read(std::slice::from_ref(&file), Mode::Auto, None, None, false, 20, 4000));
                }
            });
        }
    });
    assert!(visible_text(&read_ok(&engine, &file)).starts_with("unchanged:"));
}

#[test]
fn session_boot_is_bounded_and_itemized() {
    let (_dir, engine) = setup_default();
    let boot = engine.session_boot_snapshot();
    assert_eq!(boot["schema"], "tokenzero.session-boot.v1");
    assert_eq!(boot["mode"], "manifest_delta");
    assert_eq!(boot["demand_paging"]["working_set_loaded"], false);
    assert!(boot["wire"].as_str().unwrap().starts_with("TZ/1 root="));
    let telemetry = &boot["telemetry"];
    let sum = telemetry["manifest"].as_u64().unwrap()
        + telemetry["delta"].as_u64().unwrap()
        + telemetry["toc_working_set"].as_u64().unwrap()
        + telemetry["other"].as_u64().unwrap();
    assert_eq!(sum, telemetry["total"].as_u64().unwrap());
    assert!(sum < 100, "{boot:#}");
    let _ = engine.session_rollup();
    assert_eq!(
        engine.session_boot_snapshot()["demand_paging"]["working_set_loaded"],
        true
    );
}

#[test]
fn turn_two_reports_smaller_delta_and_monotonic_watermark() {
    let (_dir, file, engine, _) = setup_dedup("delta.rs");
    let first = read_ok(&engine, &file);
    let second = read_ok(&engine, &file);
    let first_delta = &first.telemetry.as_ref().unwrap()["session_delta"];
    let second_delta = &second.telemetry.as_ref().unwrap()["session_delta"];
    assert_eq!(first_delta["from_hwm"], 0);
    assert_eq!(first_delta["to_hwm"], 1);
    assert_eq!(second_delta["from_hwm"], 1);
    assert_eq!(second_delta["to_hwm"], 2);
    assert!(second_delta["delta_bytes"].as_u64().unwrap() < second_delta["full_bytes"].as_u64().unwrap());
    let rollup = engine.session_rollup();
    assert_eq!(rollup["session_hwm"], 2);
    assert!(rollup["delta_bytes"].as_u64().unwrap() < rollup["full_bytes"].as_u64().unwrap());
}

#[test]
fn v1_session_state_resumes_with_zero_watermark() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("resume.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let config = EngineConfig::for_root(dir.path());
    {
        let engine = TokenZeroEngine::new(config.clone());
        read_ok(&engine, &file);
    }
    let memory_path = crate::session_persist::session_memory_path(&config.cache_path);
    let mut state: Value = serde_json::from_str(&fs::read_to_string(&memory_path).unwrap()).unwrap();
    state["version"] = json!(1);
    for scope in state["scopes"].as_object_mut().unwrap().values_mut() {
        let scope = scope.as_object_mut().unwrap();
        scope.remove("session_hwm");
        if let Some(rollup) = scope.get_mut("rollup").and_then(|v| v.as_object_mut()) {
            rollup.remove("full_bytes");
            rollup.remove("delta_bytes");
        }
    }
    fs::write(&memory_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
    let resumed = TokenZeroEngine::new(config);
    let response = read_ok(&resumed, &file);
    assert!(!visible_text(&response).contains("unchanged:"));
    assert!(visible_text(&response).contains("line 01"));
    let delta = &response.telemetry.as_ref().unwrap()["session_delta"];
    assert_eq!(delta["from_hwm"], 0);
    assert_eq!(delta["to_hwm"], 1);
}

#[test]
fn resume_revalidates_gced_refs_and_resends_full() {
    let dir = tempdir().unwrap();
    let ref_index = dir.path().join("ref-index");
    tokenzero_recovery::set_ref_index_root_override(Some(ref_index.clone()));
    let file = dir.path().join("gc.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let config = EngineConfig::for_root(dir.path());
    {
        let engine = TokenZeroEngine::new(config.clone());
        read_ok(&engine, &file);
    }
    fs::remove_file(&config.cache_path).unwrap();
    let mut journal = config.cache_path.clone().into_os_string();
    journal.push(".journal");
    let _ = fs::remove_file(std::path::PathBuf::from(journal));
    let _ = fs::remove_file(crate::session_persist::session_memory_path(&config.cache_path));
    let _ = fs::remove_dir_all(&ref_index);
    let _ = fs::remove_dir_all(dir.path().join("blobs"));
    let response = read_ok(&TokenZeroEngine::new(config), &file);
    assert!(!visible_text(&response).contains("unchanged:"));
    assert!(visible_text(&response).contains("line 01"));
    let delta = &response.telemetry.as_ref().unwrap()["session_delta"];
    assert_eq!(delta["full_bytes"], delta["delta_bytes"]);
}

#[test]
fn tool_metrics_records_session_calls() {
    let (_dir, file, engine) = setup_file("hello.txt", "hello metrics\n");
    for _ in 0..3 {
        call_tool(&engine, "read", &json!({ "path": file.display().to_string() }), None).unwrap();
    }
    let snap = engine.tool_metrics_snapshot();
    assert_eq!(snap["status"], "ok");
    assert!(snap["slow_threshold_ms"].as_u64().unwrap() > 0);
    assert_eq!(snap["session"]["tools"]["read"]["calls"].as_u64().unwrap(), 3);
}

#[test]
fn persisted_memory_is_user_scoped_and_reports_cross_session_savings() {
    let project = tempdir().unwrap();
    let user_a = tempdir().unwrap();
    let user_b = tempdir().unwrap();
    let file = project.path().join("conversation.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let config = EngineConfig::for_root(project.path());

    crate::session_persist::with_session_root(user_a.path(), || {
        let first_session = TokenZeroEngine::new(config.clone());
        assert!(!visible_text(&read_ok(&first_session, &file)).starts_with("unchanged:"));
    });
    crate::session_persist::with_session_root(user_b.path(), || {
        let other_user = TokenZeroEngine::new(config.clone());
        let response = read_ok(&other_user, &file);
        assert!(!visible_text(&response).starts_with("unchanged:"));
        assert_eq!(
            response.telemetry.as_ref().unwrap()["dedup"]["cross_session_hits"],
            Value::Null
        );
    });
    crate::session_persist::with_session_root(user_a.path(), || {
        let resumed = TokenZeroEngine::new(config.clone());
        let response = read_ok(&resumed, &file);
        assert!(visible_text(&response).starts_with("unchanged:"));
        let dedup = &response.telemetry.as_ref().unwrap()["dedup"];
        assert_eq!(dedup["cross_session_hits"], 1);
        assert!(dedup["cross_session_bytes_saved"].as_u64().unwrap() > 0);
    });
    assert!(user_a.path().join("session-memory.json").is_file());
    assert!(user_b.path().join("session-memory.json").is_file());
}
