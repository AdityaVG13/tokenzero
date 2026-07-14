use super::*;
use super::support::*;
use tokenzero_core::ContentType;
use tokenzero_recovery::RecoveryStore;

#[test]
fn recall_finds_previously_served_payloads() {
    let (_dir, file, engine) = setup_file("notes.md", "alpha\nrecall_target_token here\nomega\n");
    read_ok(&engine, &file);
    let response = engine.recall("RECALL_TARGET", 10, Mode::Auto, 4000);
    assert_status_ok(&response);
    let text = visible_text(&response);
    assert!(text.contains("recall_target_token"), "{text}");
    let hit_ref = text.split_whitespace().find(|w| w.starts_with("tz://")).unwrap().to_string();
    assert!(expand_raw(&engine, &hit_ref).visible.unwrap().text.contains("recall_target_token here"));
}

#[test]
fn recall_unreadable_cache_degrades_cleanly() {
    let (dir, engine) = setup_engine(|root| {
        let cache = root.join("cache.json");
        fs::write(&cache, "{broken").unwrap();
        let mut config = EngineConfig::for_root(root);
        config.cache_path = cache;
        config
    });
    let _ = dir;
    let response = engine.recall("x", 10, Mode::Auto, 4000);
    assert_status_ok(&response);
    assert_eq!(response.diagnostic.as_ref().unwrap().code, "recall_cache_unreadable");
}

#[test]
fn recall_caps_hits_and_reports_truncation() {
    let (_dir, file, engine) = setup_file("many.txt", "tok\n".repeat(20));
    read_ok(&engine, &file);
    let response = engine.recall("tok", 3, Mode::Auto, 4000);
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["hits"], 3);
    assert_eq!(telemetry["truncated_by_results"], true);
}

#[test]
fn expand_repeated_serves_stay_byte_exact() {
    let (_dir, file, engine, content) = setup_dedup("sample.rs");
    let blob = blob_ref(&read_ok(&engine, &file));
    for _ in 0..2 {
        assert_eq!(expand_ok(&engine, &blob), content);
    }
}

#[test]
fn expand_fresh_bypasses_seen_set() {
    let (_dir, file, engine, content) = setup_dedup("sample.rs");
    let blob = blob_ref(&read_ok(&engine, &file));
    let _ = expand_raw(&engine, &blob);
    let again = engine.expand_with_params(crate::expand_params::ExpandParams { ref_id: blob, selector: Some("raw".into()), fresh: true, ..Default::default() });
    assert_eq!(visible_text(&again), content);
}

#[test]
fn expand_since_unchanged_and_diff() {
    let (_dir, engine) = setup_default();
    let since_ref = ingest_blob(&engine, "alpha\nbeta\n", "test-since");
    let target_ref = ingest_blob(&engine, "alpha\nBETA\n", "test-target");
    let unchanged = engine.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: since_ref.clone(),
        since: Some(since_ref.clone()),
        ..Default::default()
    });
    assert_status_ok(&unchanged);
    assert!(visible_text(&unchanged).contains("unchanged since"));
    let diffed = engine.expand_with_params(crate::expand_params::ExpandParams { ref_id: target_ref, since: Some(since_ref), ..Default::default() });
    assert_status_ok(&diffed);
    let text = visible_text(&diffed);
    assert!(text.contains("diff since"));
    assert!(text.contains("-beta") || text.contains("+BETA"));
}

#[test]
fn expand_fresh_with_since_returns_full_content() {
    let (_dir, engine) = setup_default();
    let base_ref = ingest_blob(&engine, "alpha\nbeta\n", "test-since");
    let target_ref = ingest_blob(&engine, "alpha\nBETA\n", "test-target");
    let response = engine.expand_with_params(crate::expand_params::ExpandParams { ref_id: target_ref, since: Some(base_ref), fresh: true, ..Default::default() });
    assert_status_ok(&response);
    assert_eq!(visible_text(&response), "alpha\nBETA\n");
}

#[test]
fn expand_since_bad_ref_errors() {
    let (_dir, engine) = setup_default();
    let target = ingest_blob(&engine, "x", "test");
    let resp = engine.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: target,
        since: Some("tz://blob/deadbeefdeadbeef".to_string()),
        ..Default::default()
    });
    assert_eq!(resp.status, "error");
}

#[test]
fn codemode_expand_passes_symbol() {
    let (_dir, file, engine) = setup_file("lib.rs", "fn alpha() {}\nfn beta() {}\n");
    let resp = engine.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: blob_ref(&read_ok(&engine, &file)),
        symbol: Some("alpha".to_string()),
        ..Default::default()
    });
    assert_status_ok(&resp);
    assert!(visible_text(&resp).contains("alpha"));
}

#[test]
fn codemode_expand_many_mixed_windows() {
    let (_dir, file, engine) = setup_file("lines.txt", "one\ntwo\nthree\n");
    let blob = blob_ref(&read_ok(&engine, &file));
    let r1 = engine.expand_with_params(crate::expand_params::ExpandParams { ref_id: blob.clone(), start_line: Some(1), end_line: Some(1), ..Default::default() });
    let r2 = engine.expand_with_params(crate::expand_params::ExpandParams { ref_id: blob, start_line: Some(2), end_line: Some(2), ..Default::default() });
    assert!(visible_text(&r1).contains("one"));
    assert!(visible_text(&r2).contains("two"));
}

#[test]
fn expand_dedup_off_serves_full_twice() {
    let (_dir, file, engine, content) = setup_dedup_off("sample.rs");
    let blob = blob_ref(&read_ok(&engine, &file));
    for _ in 0..2 {
        assert_eq!(expand_ok(&engine, &blob), content);
    }
}

#[test]
fn expand_changed_content_serves_full() {
    let (_dir, engine) = setup_default();
    let v1 = ingest_blob(&engine, "v1\n", "t");
    engine.expand_with_params(crate::expand_params::ExpandParams { ref_id: v1, ..Default::default() });
    let v2 = ingest_blob(&engine, "v2\n", "t2");
    let second = engine.expand_with_params(crate::expand_params::ExpandParams { ref_id: v2, ..Default::default() });
    assert_eq!(visible_text(&second), "v2\n");
}

#[test]
fn expand_b_fragment_returns_exact_byte_window() {
    let content = "byte-range test\nline two\n";
    let (_dir, file, engine) = setup_file("bfrag.txt", content);
    let b_ref = format!("{}#B0-1", blob_ref(&read_ok(&engine, &file)));
    let expanded = expand_raw(&engine, &b_ref);
    assert_status_ok(&expanded);
    assert_eq!(visible_text(&expanded), &content[0..1]);
}

#[test]
fn expand_across_engine_respawn_stays_byte_exact() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    let content = dedup_fixture_content();
    fs::write(&file, &content).unwrap();
    let config = EngineConfig::for_root(dir.path());
    let blob = {
        let engine = TokenZeroEngine::new(config.clone());
        let blob = blob_ref(&read_ok(&engine, &file));
        assert_eq!(expand_ok(&engine, &blob), content);
        assert_eq!(expand_ok(&engine, &blob), content);
        blob
    };
    assert_eq!(expand_ok(&TokenZeroEngine::new(config), &blob), content);
}

#[test]
fn expand_stale_persisted_sha_serves_full_after_payload_mutation() {
    tokenzero_recovery::set_ref_index_disabled_override(true);
    let dir = tempdir().unwrap();
    let config = EngineConfig::for_root(dir.path());
    let cache_path = config.cache_path.clone();
    let blob = {
        let engine = TokenZeroEngine::new(config.clone());
        let v1 = ingest_blob(&engine, "version_one\n", "t");
        engine.expand_with_params(crate::expand_params::ExpandParams { ref_id: v1.clone(), ..Default::default() });
        v1
    };
    let mut state: Value = serde_json::from_str(&fs::read_to_string(&cache_path).unwrap()).unwrap();
    let entry = state.get_mut("blobs").and_then(|v| v.as_object_mut()).unwrap().get_mut(&blob).expect("blob entry");
    assert_eq!(entry.as_str().unwrap(), "version_one\n");
    *entry = json!("version_two\n");
    fs::write(&cache_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    let mut journal = cache_path.clone().into_os_string();
    journal.push(".journal");
    let _ = fs::remove_file(PathBuf::from(journal));
    let resp = TokenZeroEngine::new(config).expand_with_params(crate::expand_params::ExpandParams { ref_id: blob, ..Default::default() });
    assert_eq!(visible_text(&resp), "version_two\n");
    tokenzero_recovery::set_ref_index_disabled_override(false);
}

#[test]
fn session_dedup_off_does_not_write_session_memory_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, "hello\n").unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.session_dedup = false;
    let memory_path = crate::session_persist::session_memory_path(&config.cache_path);
    let engine = TokenZeroEngine::new(config);
    let blob = blob_ref(&read_ok(&engine, &file));
    let _ = expand_raw(&engine, &blob);
    let _ = expand_raw(&engine, &blob);
    assert!(!memory_path.exists(), "dedup off must not create {}", memory_path.display());
}

#[test]
fn expand_same_store_scheme_alias_fz_gz_byte_exact() {
    let content = "engine cross-scheme body\nsecond line\n";
    let (_dir, file, engine) = setup_file("cross.txt", content);
    let blob = blob_ref(&read_ok(&engine, &file));
    let id = blob.strip_prefix("tz://blob/").unwrap();
    for scheme_ref in [blob.clone(), format!("fz://blob/{id}"), format!("gz://blob/{id}")] {
        assert_eq!(expand_ok(&engine, &scheme_ref), content, "{scheme_ref}");
    }
}

#[test]
fn expand_error_matrix_keeps_full_ref() {
    let (_dir, engine) = setup_default();
    let cases: &[(&str, &str, &str)] = &[
        ("garbage_scheme", "xx://blob/bdeadbeefcafebabe0123456789abcdef_long_hash_tail_for_truncation_check", "invalid_ref"),
        ("missing_fz", "fz://blob/b0123456789abcdef", "any"),
    ];
    for (label, ref_id, code) in cases {
        let response = expand_raw(&engine, ref_id);
        assert_eq!(response.status, "error", "{label}");
        let err = response.error.as_ref().unwrap();
        if *code != "any" {
            assert_eq!(err.code, *code, "{label}");
        }
        assert!(err.message.contains(ref_id), "{label}: {}", err.message);
        if *label == "missing_fz" {
            assert!(
                err.message.starts_with(&format!("-{ref_id} (unavailable)")),
                "{label}: {}",
                err.message
            );
        }
    }
}

#[test]
fn same_session_codemode_default_store_expands_without_rerun() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let cache = crate::workspace::default_recovery_cache_path(&root);
    let mint = {
        let mut config = EngineConfig::for_root(&root);
        config.cache_path = cache.clone();
        TokenZeroEngine::new(config)
    };
    let blob = blob_ref(&mint.ingest(
        "same-session payload hello",
        ContentType::Unknown,
        Mode::Auto,
        "test",
    ));
    drop(mint);
    let mut config = EngineConfig::for_root(&root);
    config.cache_path = cache;
    assert_eq!(
        expand_ok(&TokenZeroEngine::new(config), &blob),
        "same-session payload hello"
    );
}

#[test]
fn wrong_cache_path_names_both_stores_on_miss() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let consumer_cache = root.join("recovery-cache.json");
    let sibling = root.join("codemode-recovery.json");
    let payload = "mismatch-payload";
    let blob_id = tokenzero_core::id_for('b', payload);
    let blob_ref = format!("tz://blob/{blob_id}");
        fs::write(&sibling, serde_json::to_vec_pretty(&recovery_state_json(json!({ blob_ref.clone(): payload }), json!([blob_ref.clone()]))).unwrap()).unwrap();
    fs::write(&consumer_cache, serde_json::to_vec_pretty(&recovery_state_json(json!({}), json!([]))).unwrap()).unwrap();
    let mut consumer = EngineConfig::for_root(root);
    consumer.cache_path = consumer_cache.clone();
    let response = expand_raw(&TokenZeroEngine::new(consumer), &blob_ref);
    assert_eq!(response.status, "error");
    let err = response.error.as_ref().unwrap();
    assert_eq!(err.code, "store_mismatch");
    assert!(
        err.message.contains(sibling.to_string_lossy().as_ref()) || err.message.contains("codemode-recovery"),
        "{}", err.message
    );
    assert!(
        err.message.contains(consumer_cache.to_string_lossy().as_ref()) || err.message.contains("recovery-cache"),
        "{}", err.message
    );
}

#[test]
fn windowed_expand_same_store_and_oob_code() {
    let mut body = String::new();
    for i in 1..=200 {
        body.push_str(&format!("line-{i}\n"));
    }
    let (_dir, file, engine) = setup_file("lines.txt", &body);
    let blob = blob_ref(&read_ok(&engine, &file));
    let window = engine.expand(&blob, Some("raw"), Some(120), Some(190), None, None);
    assert_status_ok(&window);
    let text = visible_text(&window);
    assert!(text.starts_with("line-120\n") && text.contains("line-190\n") && !text.contains("line-119\n"), "{text}");
    let win_tokens = window.accounting.as_ref().unwrap().visible_tokens;
    let full_tokens = expand_raw(&engine, &blob).accounting.as_ref().unwrap().visible_tokens;
    assert!(win_tokens < full_tokens / 2, "window {win_tokens} vs full {full_tokens}");
    let oob = engine.expand(&blob, Some("raw"), Some(500), Some(510), None, None);
    assert_eq!(oob.status, "error");
    let err = oob.error.as_ref().unwrap();
    assert_eq!(err.code, "window_out_of_range");
    assert!(err.message.contains(&blob), "{}", err.message);
}

#[test]
fn crash_only_expand_blocked_when_codemode_healthy() {
    let (dir, engine) = setup_engine(|root| {
        let mut config = EngineConfig::for_root(root);
        config.tool_surface = tokenzero_core::McpToolSurface::CodeMode;
        config
    });
    let _ = dir;
    assert!(engine.surface_health().is_healthy());
    assert!(engine.surface_health().primary_surface_healthy_claim());
    let blocked = tools_call(&engine, json!(1), "tz_expand", json!({"ref": "tz://blob/dead"}));
    let data = &blocked["error"]["data"];
    assert!(data["message"].as_str().unwrap_or_default().contains("primary surface is healthy"), "{blocked}");
    assert_eq!(data["kind"], "policy_refusal");
    assert_eq!(engine.surface_health().telemetry()["telemetry"]["blocked_count"], 1);
    let shell_blocked = tools_call(&engine, json!(2), "tz_shell", json!({"command": "true"}));
    assert_eq!(shell_blocked["error"]["data"]["kind"], "policy_refusal");
    assert!(shell_blocked["error"]["data"]["message"].as_str().unwrap_or_default().contains("never unlocked"), "{shell_blocked}");
}

#[test]
fn crash_only_expand_unlocks_after_expand_x0_and_recovers_bytes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("payload.txt");
    fs::write(&file, "recovery-ladder-bytes-wqw9\n").unwrap();
    let cache = dir.path().join(".tokenzero/recovery-cache.json");
    let mut config = EngineConfig::for_root(dir.path());
    config.tool_surface = tokenzero_core::McpToolSurface::CodeMode;
    config.cache_path = cache.clone();
    let engine = TokenZeroEngine::new(config);
    let mut seed_cfg = EngineConfig::for_root(dir.path());
    seed_cfg.cache_path = cache;
    let blob = seed_blob_ref(&TokenZeroEngine::new(seed_cfg), &file);
    engine.surface_health().record_codemode_expand_x0();
    assert!(!engine.surface_health().is_healthy());
    assert!(!engine.surface_health().primary_surface_healthy_claim());
    let unlocked = tools_call(&engine, json!(3), "tz_expand", json!({"ref": blob, "selector": "raw"}));
    let unlocked_text = unlocked.to_string();
    assert!(unlocked_text.contains("recovery-ladder-bytes-wqw9"), "{unlocked_text}");
    assert!(
        engine.surface_health().telemetry()["telemetry"]["unlocked_count"]
            .as_u64()
            .unwrap_or(0)
            >= 1
    );
}

#[test]
fn crash_only_healthy_claim_false_after_ref_not_found_surface_error() {
    let (dir, engine) = setup_engine(|root| {
        let mut config = EngineConfig::for_root(root);
        config.tool_surface = tokenzero_core::McpToolSurface::CodeMode;
        config
    });
    let _ = dir;
    let miss = expand_raw(
        &engine,
        "tz://blob/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    assert_eq!(miss.status, "error");
    assert!(!engine.surface_health().primary_surface_healthy_claim());
    assert_eq!(
        engine.surface_health().decide(tokenzero_core::McpToolSurface::CodeMode, "expand"),
        crate::surface_health::CrashOnlyDecision::Unlocked
    );
}

#[test]
fn expand_fz_blob_ref_from_sibling_fszero_store() {
    let dir = tempdir().unwrap();
    let root = dir.path().join(".zerostack");
    let fszero_cache = root.join("fszero").join("recovery-cache.json");
    let tokenzero_cache = root.join("tokenzero").join("recovery-cache.json");
    let payload = "fszero engine payload\nline two\n";
    let fz_ref = format!("fz://blob/{}", tokenzero_core::sha256_hex(payload));
    let fszero_temp = dir.path().join("fszero-cache.json");
    let mut fszero_store = RecoveryStore::new(Some(fszero_temp.clone()));
    fszero_store.store_payload(payload, ContentType::Unknown, None, None, None).unwrap();
    fs::create_dir_all(fszero_cache.parent().unwrap()).unwrap();
    fs::rename(&fszero_temp, &fszero_cache).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = tokenzero_cache;
    config.allowed_roots = vec![dir.path().to_path_buf()];
    let engine = TokenZeroEngine::new(config);
    assert_eq!(expand_ok(&engine, &fz_ref), payload);
}
