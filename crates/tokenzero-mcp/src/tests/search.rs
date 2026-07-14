use super::*;
use super::support::*;

#[test]
fn grep_keeps_own_tool_name_and_exact_refs() {
    let (dir, _file, engine) = setup_file("lib.rs", "fn alpha() {}\nfn beta() {}\n");
    let response = engine.grep("alpha", &[dir.path().to_path_buf()], Mode::Hybrid, 20, 4000);
    assert_eq!(response.tool, "grep");
    assert_status_ok(&response);
    assert!(visible_text(&response).contains("alpha"));
    assert!(response.refs.iter().any(|row| row.kind == "blob"));
    assert!(response.refs.iter().any(|row| row.kind == "search"));
}

#[test]
fn zero_hit_note_clamps_multiline_and_long_queries() {
    let (dir, _file, engine) = setup_file("lib.rs", "fn alpha() {}\n");
    let roots = vec![dir.path().to_path_buf()];
    assert_eq!(
        visible_text(&engine.find("fn alpha() {\n    unreachable\n}", &roots, Mode::Auto, 20, 4000)),
        "# find fn alpha() {... — 0 matches"
    );
    let long_query = "x".repeat(120);
    assert_eq!(
        visible_text(&engine.grep(&long_query, &roots, Mode::Auto, 20, 4000)),
        format!("# grep {}... — 0 matches", "x".repeat(48))
    );
}

#[test]
fn truncated_scan_notes_for_glob_tree_find() {
    let (dir, _file, engine) = setup_file("lib.rs", "fn alpha() {}\n");
    let roots = vec![dir.path().to_path_buf()];
    let cases = [
        ("glob", engine.glob("**/*.rs", &roots, false, Mode::Auto, 0, 4000)),
        ("tree", engine.tree(&roots, 0, false, Mode::Auto, 200, 4000)),
        ("find", engine.find("alpha", &roots, Mode::Auto, 0, 4000)),
    ];
    for (label, resp) in &cases {
        assert_status_ok(resp);
        assert!(
            visible_text(resp).contains("(scan truncated)"),
            "{label}: {}",
            visible_text(resp)
        );
    }
}

#[test]
fn zero_hit_notes_for_glob_tree_recall() {
    let (dir, _file, engine) = setup_file("lib.rs", "lib");
    let roots = vec![dir.path().to_path_buf()];
    let mut cases = vec![
        ("glob", engine.glob("**/*.zig", &roots, false, Mode::Auto, 20, 4000)),
        ("recall", engine.recall("zz_nothing", 10, Mode::Auto, 4000)),
    ];
    let (empty_dir, empty_engine) = setup_default();
    cases.push((
        "tree",
        empty_engine.tree(&[empty_dir.path().to_path_buf()], 3, false, Mode::Auto, 200, 4000),
    ));
    for (label, resp) in &cases {
        assert_status_ok(resp);
        let text = visible_text(resp);
        assert!(
            text.contains("0 matches") || text.contains("0 entries"),
            "{label}: {text}"
        );
    }
}

#[test]
fn search_exact_ref_tokens_match_refs() {
    let (dir, _file, engine) = setup_file("lib.rs", "fn alpha() {}\n");
    let response = engine.find("alpha", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    let expected = response.refs.iter().map(|r| count_tokens(&r.ref_id)).sum::<usize>();
    assert_status_ok(&response);
    assert!(expected > 2);
    assert_eq!(response.accounting.as_ref().unwrap().exact_ref_tokens, Some(expected));
}

#[test]
fn glob_keeps_degraded_telemetry_when_recovery_cache_is_unwritable() {
    let (dir, _file, engine) = setup_unwritable("lib.rs", "fn alpha() {}\n");
    let response = engine.glob("**/*.rs", &[dir.path().to_path_buf()], false, Mode::Auto, 20, 4000);
    assert_status_ok(&response);
    assert_eq!(response.diagnostic.as_ref().unwrap().code, "cache_write_failed");
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["degraded"], true);
    assert_eq!(telemetry["transport_status"], "degraded");
    assert!(telemetry["storage_error"].as_str().is_some());
    assert_eq!(telemetry["exact_refs_available"], false);
    assert_eq!(telemetry["pattern"], "**/*.rs");
    assert_eq!(telemetry["matches"], 1);
}

#[test]
fn search_traverses_beyond_default_result_limit() {
    let dir = tempdir().unwrap();
    for index in 0..30 {
        fs::write(dir.path().join(format!("a{index:03}.txt")), "hay\n").unwrap();
    }
    fs::write(dir.path().join("zmatch.txt"), "needle\n").unwrap();
    let engine = engine_with_backend(dir.path(), SearchBackend::Internal);
    let response = engine.find("needle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_status_ok(&response);
    assert!(visible_text(&response).contains("needle"));
    assert!(response.telemetry.as_ref().unwrap()["visited_files"].as_u64().unwrap() > 20);
    assert_eq!(response.telemetry.as_ref().unwrap()["truncated_by_visit"], false);
}

#[cfg(unix)]
#[test]
fn internal_find_and_glob_terminate_on_a_symlink_cycle() {
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/file.rs"), "needle here\n").unwrap();
    symlink(dir.path().join("sub"), dir.path().join("sub/loop")).unwrap();
    let engine = engine_with_backend(dir.path(), SearchBackend::Internal);
    let found = engine.find("needle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_status_ok(&found);
    let found_text = expanded_flat_output(&engine, &found);
    assert!(found_text.contains("needle"));
    assert!(!found_text.contains("loop/loop"), "{found_text}");
    let globbed = engine.glob("**/*.rs", &[dir.path().to_path_buf()], false, Mode::Auto, 20, 4000);
    assert_status_ok(&globbed);
    assert!(!expanded_flat_output(&engine, &globbed).contains("loop/loop"));
}

#[test]
fn rg_and_internal_backends_return_identical_search_output() {
    if !rg_or_skip("rg_and_internal_backends_return_identical_search_output") {
        return;
    }
    let dir = tempdir().unwrap();
    search_backend_fixture(dir.path());
    let roots = vec![dir.path().to_path_buf()];
    let mut internal_engine = engine_with_backend(dir.path(), SearchBackend::Internal);
    internal_engine.config.session_dedup = false;
    let mut rg_engine = engine_with_backend(dir.path(), SearchBackend::Rg);
    rg_engine.config.session_dedup = false;
    let internal = internal_engine.find("needle", &roots, Mode::Auto, 20, 4000);
    let rg = rg_engine.find("needle", &roots, Mode::Auto, 20, 4000);
    assert_status_ok(&internal);
    assert_status_ok(&rg);
    assert_eq!(internal.telemetry.as_ref().unwrap()["search_backend"], "internal");
    assert_eq!(rg.telemetry.as_ref().unwrap()["search_backend"], "rg");
    assert!(rg.telemetry.as_ref().unwrap().get("fallback_reason").is_none());
    let internal_flat = expanded_flat_output(&internal_engine, &internal);
    let rg_flat = expanded_flat_output(&rg_engine, &rg);
    assert_eq!(internal_flat, rg_flat);
    let normalized = internal_flat.replace('\\', "/");
    assert!(normalized.contains("alpha.rs:2:let needle = 1;"));
    assert!(normalized.contains("sub/beta.rs:1:needle here"));
    assert!(normalized.contains("sub/beta.rs:3:needle again"));
    assert!(!rg_flat.contains(".hidden"));
    assert!(!rg_flat.contains("target"));
    assert_eq!(
        internal.visible.as_ref().unwrap().text,
        rg.visible.as_ref().unwrap().text
    );
}

#[test]
fn grep_treats_pattern_as_regex_under_rg_backend() {
    if !rg_or_skip("grep_treats_pattern_as_regex_under_rg_backend") {
        return;
    }
    let dir = tempdir().unwrap();
    search_backend_fixture(dir.path());
    let engine = engine_with_backend(dir.path(), SearchBackend::Rg);
    let response = engine.grep("ne+dle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_status_ok(&response);
    assert_eq!(response.telemetry.as_ref().unwrap()["search_backend"], "rg");
    assert!(expanded_flat_output(&engine, &response).replace('\\', "/").contains("sub/beta.rs:1:needle here"));
    let zero = engine.grep("no_such_thing_xyz", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_status_ok(&zero);
    assert_eq!(visible_text(&zero), "# grep no_such_thing_xyz — 0 matches");
}

#[test]
fn grep_keeps_substring_semantics_under_internal_backend() {
    let (dir, _file, _) = setup_file("lib.rs", "ne+dle literal\nneedle plain\n");
    let engine = engine_with_backend(dir.path(), SearchBackend::Internal);
    let response = engine.grep("ne+dle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_status_ok(&response);
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["search_backend"], "internal");
    assert!(telemetry.get("fallback_reason").is_none());
    let flat = expanded_flat_output(&engine, &response);
    assert!(flat.contains("lib.rs:1:ne+dle literal"));
    assert!(!flat.contains("needle plain"));
}

#[test]
fn grep_invalid_regex_under_rg_backend_is_a_pattern_error() {
    if !rg_or_skip("grep_invalid_regex_under_rg_backend_is_a_pattern_error") {
        return;
    }
    let (dir, _file, _) = setup_file("lib.rs", "fn main( {}\n");
    let engine = engine_with_backend(dir.path(), SearchBackend::Rg);
    let response = engine.grep("fn main(", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_eq!(response.status, "error");
    let error = response.error.unwrap();
    assert_eq!(error.code, "invalid_pattern");
    assert!(error.message.contains("regex parse error"), "{}", error.message);
    let internal = engine_with_backend(dir.path(), SearchBackend::Internal);
    let substring = internal.grep("fn main(", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_status_ok(&substring);
    assert!(expanded_flat_output(&internal, &substring).contains("lib.rs:1:fn main( {}"));
}

#[test]
fn auto_backend_without_rg_falls_back_to_internal_with_telemetry() {
    let dir = tempdir().unwrap();
    search_backend_fixture(dir.path());
    let mut config = EngineConfig::for_root(dir.path());
    config.search_backend = SearchBackend::Auto;
    config.rg_path_override = Some(dir.path().join("missing-rg-binary"));
    let engine = TokenZeroEngine::new(config);
    let response = engine.find("needle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_status_ok(&response);
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["search_backend"], "internal");
    assert_eq!(telemetry["fallback_reason"], "rg_not_found");
    assert!(expanded_flat_output(&engine, &response).replace('\\', "/").contains("sub/beta.rs:1:needle here"));
}

#[cfg(unix)]
#[test]
fn broken_rg_falls_back_to_internal_with_telemetry() {
    let dir = tempdir().unwrap();
    search_backend_fixture(dir.path());
    write_executable(&dir.path().join("fake-rg"), "#!/bin/sh\necho boom >&2\nexit 2\n");
    let mut config = EngineConfig::for_root(dir.path());
    config.search_backend = SearchBackend::Rg;
    config.rg_path_override = Some(dir.path().join("fake-rg"));
    let engine = TokenZeroEngine::new(config);
    let response = engine.find("needle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_status_ok(&response);
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["search_backend"], "internal");
    assert!(telemetry["fallback_reason"].as_str().unwrap().contains("rg exited"), "{telemetry:#}");
    assert!(expanded_flat_output(&engine, &response).contains("sub/beta.rs:1:needle here"));
}

#[test]
fn explicit_rg_backend_grep_errors_when_rg_is_unusable() {
    let dir = tempdir().unwrap();
    search_backend_fixture(dir.path());
    let mut config = EngineConfig::for_root(dir.path());
    config.search_backend = SearchBackend::Rg;
    config.rg_path_override = Some(dir.path().join("missing-rg-binary"));
    let engine = TokenZeroEngine::new(config);
    let grep = engine.grep("ne+dle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_error_code(&grep, "backend_unavailable");
    assert_status_ok(&engine.find("needle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000));
}

#[cfg(unix)]
#[test]
fn rg_rows_with_colon_paths_parse_and_unparsed_rows_are_counted() {
    let dir = tempdir().unwrap();
    let tricky = dir.path().join("a:1:b.txt");
    fs::write(&tricky, "needle content\n").unwrap();
    let row = format!("{}/a:1:b.txt:1:needle content", dir.path().display());
    let parsed = parse_rg_line(&row, &dir.path().display().to_string()).unwrap();
    assert_eq!(parsed.rel, "a:1:b.txt");
    assert_eq!(parsed.line, 1);
    assert_eq!(parsed.text, "needle content");
    write_executable(
        &dir.path().join("fake-rg"),
        &format!(
            "#!/bin/sh\necho 'garbage row without numbers'\necho '{}/a:1:b.txt:1:needle content'\n",
            dir.path().display()
        ),
    );
    let mut config = EngineConfig::for_root(dir.path());
    config.search_backend = SearchBackend::Rg;
    config.rg_path_override = Some(dir.path().join("fake-rg"));
    let engine = TokenZeroEngine::new(config);
    let response = engine.grep("needle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_status_ok(&response);
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["search_backend"], "rg");
    assert_eq!(telemetry["rg_unparsed_rows"], 1);
    assert!(expanded_flat_output(&engine, &response).contains("a:1:b.txt:1:needle content"));
}

#[test]
fn glob_discovers_paths_and_roundtrips_exact_output() {
    let (dir, engine) = setup_default();
    fs::create_dir_all(dir.path().join("src/nested")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "lib").unwrap();
    fs::write(dir.path().join("src/nested/mod.rs"), "mod").unwrap();
    fs::write(dir.path().join("README.md"), "readme").unwrap();
    let response = engine.glob("**/*.rs", &[dir.path().to_path_buf()], false, Mode::Hybrid, 20, 4000);
    assert_eq!(response.tool, "glob");
    assert_status_ok(&response);
    let visible = visible_text(&response);
    assert!(visible.contains("src/lib.rs"));
    assert!(visible.contains("src/nested/mod.rs"));
    assert!(!visible.contains("README.md"));
    assert!(expand_ok(&engine, &blob_ref(&response)).contains("src/lib.rs"));
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["transport_status"], "ok");
    assert_eq!(telemetry["degraded"], false);
    assert_eq!(telemetry["storage_error"], Value::Null);
    assert_eq!(telemetry["exact_refs_available"], true);
}

#[test]
fn fresh_arg_bypasses_dedup_via_tools_call() {
    let (_dir, file, engine, _) = setup_dedup("sample.rs");
    let call = |id: u64, fresh: bool| -> String {
        let mut arguments = json!({"path": file.display().to_string()});
        if fresh {
            arguments["fresh"] = json!(true);
        }
        tools_call(&engine, json!(id), "read", arguments)["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let first = call(1, true);
    let second = call(2, true);
    assert!(first.contains("line 01"), "{first}");
    assert!(second.contains("line 01"), "{second}");
    assert!(!second.contains("unchanged:"), "{second}");
    assert!(call(3, false).contains("unchanged:"));
}

#[test]
fn search_dedup_notes_and_changed_output_serves_full() {
    let dir = tempdir().unwrap();
    let data = dir.path().join("data.rs");
    let rows = |count: usize| -> String {
        (1..=count).map(|i| format!("let needle_{i:02} = \"session redundancy search fixture row\";")).collect::<Vec<_>>().join("\n") + "\n"
    };
    fs::write(&data, rows(12)).unwrap();
    let engine = engine_with_backend(dir.path(), SearchBackend::Internal);
    let roots = vec![dir.path().to_path_buf()];
    let first = engine.find("needle", &roots, Mode::Auto, 20, 4000);
    assert_status_ok(&first);
    assert!(visible_text(&first).contains("needle_01"));
    let second = engine.find("needle", &roots, Mode::Auto, 20, 4000);
    let note = visible_text(&second);
    assert!(note.starts_with("unchanged: tz://file/") && note.contains("# find needle — 12 matches") && note.contains("full results: expand tz://blob/"), "{note}");
    assert!(visible_tokens(&second) < visible_tokens(&first));
    let telemetry = second.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["output_strategy"], "seen_set_dedup");
    assert_eq!(telemetry["cache_hit"], true);
    assert_eq!(telemetry["search_backend"], "internal");
    assert_eq!(telemetry["dedup"]["serve_count"], 2);
    fs::write(&data, rows(13)).unwrap();
    let third = engine.find("needle", &roots, Mode::Auto, 20, 4000);
    let full = visible_text(&third);
    assert!(!full.contains("unchanged:"), "{full}");
    assert!(full.contains("needle_13"), "{full}");
    assert!(visible_text(&engine.find("needle", &roots, Mode::Auto, 20, 4000)).starts_with("unchanged:"));
}

#[test]
fn degraded_storage_search_serves_full_not_note() {
    let (dir, _file, engine) = setup_unwritable("lib.rs", "fn alpha() {}\nfn beta() {}\n");
    let roots = vec![dir.path().to_path_buf()];
    assert_status_ok(&engine.grep("alpha", &roots, Mode::Auto, 20, 4000));
    let second = engine.grep("alpha", &roots, Mode::Auto, 20, 4000);
    let text = visible_text(&second);
    assert!(text.contains("alpha"), "{text}");
    assert!(!text.contains("unchanged:"), "{text}");
    assert_eq!(second.diagnostic.as_ref().unwrap().code, "cache_write_failed");
}
