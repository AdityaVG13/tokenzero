use super::*;
use std::sync::Arc;
use tempfile::tempdir;
use tokenzero_core::MCP_SCHEMA_VERSION;

use super::support::*;

#[test]
fn grep_keeps_own_tool_name_and_exact_refs() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn alpha() {}\nfn beta() {}\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.grep("alpha", &[dir.path().to_path_buf()], Mode::Hybrid, 20, 4000);

    assert_eq!(response.tool, "grep");
    assert_eq!(response.status, "ok");
    assert!(response.visible.unwrap().text.contains("alpha"));
    assert!(response.refs.iter().any(|row| row.kind == "blob"));
    assert!(response.refs.iter().any(|row| row.kind == "search"));
}

#[test]
fn grep_zero_matches_renders_zero_hit_note() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn alpha() {}\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.grep("nomatch", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);

    assert_eq!(response.status, "ok");
    let text = response.visible.as_ref().unwrap().text.clone();
    assert_eq!(text, "# grep nomatch — 0 matches");
    let accounting = response.accounting.as_ref().unwrap();
    assert_eq!(accounting.raw_tokens, 0);
    assert_eq!(accounting.visible_tokens, count_tokens(&text));
    // Canonical stored payload stays the empty flat output, still
    // recoverable: expanding the blob ref must return the empty payload,
    // never the note.
    let blob_ref = response
        .refs
        .iter()
        .find(|row| row.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let expanded = engine.expand(&blob_ref, None, None, None, None, None);
    assert_eq!(expanded.status, "ok");
    assert_eq!(expanded.visible.as_ref().unwrap().text, "");
}

#[test]
fn zero_hit_note_clamps_multiline_and_long_queries() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn alpha() {}\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let multiline = engine.find(
        "fn alpha() {\n    unreachable\n}",
        &[dir.path().to_path_buf()],
        Mode::Auto,
        20,
        4000,
    );
    assert_eq!(
        multiline.visible.as_ref().unwrap().text,
        "# find fn alpha() {... — 0 matches"
    );

    let long_query = "x".repeat(120);
    let long = engine.grep(
        &long_query,
        &[dir.path().to_path_buf()],
        Mode::Auto,
        20,
        4000,
    );
    assert_eq!(
        long.visible.as_ref().unwrap().text,
        format!("# grep {}... — 0 matches", "x".repeat(48))
    );
}

#[test]
fn glob_zero_result_budget_notes_truncated_scan() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "lib").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.glob(
        "**/*.rs",
        &[dir.path().to_path_buf()],
        false,
        Mode::Auto,
        0,
        4000,
    );

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.visible.as_ref().unwrap().text,
        "# glob **/*.rs — 0 matches (scan truncated)"
    );
}

#[test]
fn tree_zero_depth_notes_truncated_scan() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "lib").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.tree(&[dir.path().to_path_buf()], 0, false, Mode::Auto, 200, 4000);

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.visible.as_ref().unwrap().text,
        "# tree — 0 entries (scan truncated)"
    );
}

#[test]
fn find_zero_matches_under_truncated_scan_says_so() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn alpha() {}\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.find("alpha", &[dir.path().to_path_buf()], Mode::Auto, 0, 4000);

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.visible.as_ref().unwrap().text,
        "# find alpha — 0 matches (scan truncated)"
    );
}

#[test]
fn glob_zero_matches_renders_zero_hit_note() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "lib").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.glob(
        "**/*.zig",
        &[dir.path().to_path_buf()],
        false,
        Mode::Auto,
        20,
        4000,
    );

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.visible.as_ref().unwrap().text,
        "# glob **/*.zig — 0 matches"
    );
}

#[test]
fn tree_of_empty_root_renders_zero_hit_note() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.tree(&[dir.path().to_path_buf()], 3, false, Mode::Auto, 200, 4000);

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.visible.as_ref().unwrap().text,
        "# tree — 0 entries"
    );
}

#[test]
fn search_exact_ref_tokens_match_refs() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn alpha() {}\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.find("alpha", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    let expected = response
        .refs
        .iter()
        .map(|record| count_tokens(&record.ref_id))
        .sum::<usize>();

    assert_eq!(response.status, "ok");
    assert!(expected > 2);
    assert_eq!(
        response.accounting.as_ref().unwrap().exact_ref_tokens,
        Some(expected)
    );
}

#[test]
fn glob_keeps_degraded_telemetry_when_recovery_cache_is_unwritable() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn alpha() {}\n").unwrap();
    let cache_dir = dir.path().join("cache-as-directory");
    fs::create_dir_all(&cache_dir).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache_dir;
    let engine = TokenZeroEngine::new(config);

    let response = engine.glob(
        "**/*.rs",
        &[dir.path().to_path_buf()],
        false,
        Mode::Auto,
        20,
        4000,
    );

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.diagnostic.as_ref().unwrap().code,
        "cache_write_failed"
    );
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
    // visited_files counting is internal-scanner behavior; rg reports
    // matched files only.
    let engine = engine_with_backend(dir.path(), SearchBackend::Internal);

    let response = engine.find("needle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);

    assert_eq!(response.status, "ok");
    assert!(response.visible.as_ref().unwrap().text.contains("needle"));
    assert!(
        response.telemetry.as_ref().unwrap()["visited_files"]
            .as_u64()
            .unwrap()
            > 20
    );
    assert_eq!(
        response.telemetry.as_ref().unwrap()["truncated_by_visit"],
        false
    );
}

#[cfg(unix)]
#[test]
fn internal_find_and_glob_terminate_on_a_symlink_cycle() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/file.rs"), "needle here\n").unwrap();
    // A directory symlink pointing back at its own parent: following it would
    // recurse forever (sub/loop/loop/loop/...).
    symlink(dir.path().join("sub"), dir.path().join("sub/loop")).unwrap();

    let engine = engine_with_backend(dir.path(), SearchBackend::Internal);

    // The bug was unbounded recursion / stack overflow; reaching these
    // assertions at all is the proof it terminates.
    let found = engine.find("needle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_eq!(found.status, "ok");
    let found_text = expanded_flat_output(&engine, &found);
    assert!(found_text.contains("needle"), "real file still matched");
    assert!(
        !found_text.contains("loop/loop"),
        "the symlink cycle must not be traversed: {found_text}"
    );

    let globbed = engine.glob(
        "**/*.rs",
        &[dir.path().to_path_buf()],
        false,
        Mode::Auto,
        20,
        4000,
    );
    assert_eq!(globbed.status, "ok");
    let glob_text = expanded_flat_output(&engine, &globbed);
    assert!(
        !glob_text.contains("loop/loop"),
        "glob must not descend the symlink cycle: {glob_text}"
    );
}

#[test]
fn rg_and_internal_backends_return_identical_search_output() {
    if !rg_or_skip("rg_and_internal_backends_return_identical_search_output") {
        return;
    }
    let dir = tempdir().unwrap();
    search_backend_fixture(dir.path());
    let roots = vec![dir.path().to_path_buf()];

    let internal_engine = engine_with_backend(dir.path(), SearchBackend::Internal);
    let rg_engine = engine_with_backend(dir.path(), SearchBackend::Rg);
    let internal = internal_engine.find("needle", &roots, Mode::Auto, 20, 4000);
    let rg = rg_engine.find("needle", &roots, Mode::Auto, 20, 4000);

    assert_eq!(internal.status, "ok");
    assert_eq!(rg.status, "ok");
    assert_eq!(
        internal.telemetry.as_ref().unwrap()["search_backend"],
        "internal"
    );
    assert_eq!(rg.telemetry.as_ref().unwrap()["search_backend"], "rg");
    assert!(
        rg.telemetry
            .as_ref()
            .unwrap()
            .get("fallback_reason")
            .is_none()
    );

    let internal_flat = expanded_flat_output(&internal_engine, &internal);
    let rg_flat = expanded_flat_output(&rg_engine, &rg);
    assert_eq!(internal_flat, rg_flat);
    let normalized_flat = internal_flat.replace('\\', "/");
    assert!(normalized_flat.contains("alpha.rs:2:let needle = 1;"));
    assert!(normalized_flat.contains("sub/beta.rs:1:needle here"));
    assert!(normalized_flat.contains("sub/beta.rs:3:needle again"));
    // Both backends skip hidden entries (including the recovery cache
    // dir) and the target/__pycache__ build dirs.
    assert!(!rg_flat.contains(".hidden"));
    assert!(!rg_flat.contains("target"));
    // Grouped/flat rendering operates on the same SearchMatch rows, so
    // the visible capsule is identical too.
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

    assert_eq!(response.status, "ok");
    assert_eq!(response.telemetry.as_ref().unwrap()["search_backend"], "rg");
    let flat = expanded_flat_output(&engine, &response);
    assert!(
        flat.replace('\\', "/")
            .contains("sub/beta.rs:1:needle here")
    );

    // Zero-hit notes render identically under the rg backend.
    let zero = engine.grep(
        "no_such_thing_xyz",
        &[dir.path().to_path_buf()],
        Mode::Auto,
        20,
        4000,
    );
    assert_eq!(zero.status, "ok");
    assert_eq!(
        zero.visible.as_ref().unwrap().text,
        "# grep no_such_thing_xyz — 0 matches"
    );
}

#[test]
fn grep_keeps_substring_semantics_under_internal_backend() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "ne+dle literal\nneedle plain\n").unwrap();
    let engine = engine_with_backend(dir.path(), SearchBackend::Internal);

    let response = engine.grep("ne+dle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);

    assert_eq!(response.status, "ok");
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
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn main( {}\n").unwrap();
    let engine = engine_with_backend(dir.path(), SearchBackend::Rg);

    let response = engine.grep(
        "fn main(",
        &[dir.path().to_path_buf()],
        Mode::Auto,
        20,
        4000,
    );

    assert_eq!(response.status, "error");
    let error = response.error.unwrap();
    assert_eq!(error.code, "invalid_pattern");
    assert!(
        error.message.contains("regex parse error"),
        "{}",
        error.message
    );

    // The same pattern stays a valid substring under the internal backend.
    let internal = engine_with_backend(dir.path(), SearchBackend::Internal);
    let substring = internal.grep(
        "fn main(",
        &[dir.path().to_path_buf()],
        Mode::Auto,
        20,
        4000,
    );
    assert_eq!(substring.status, "ok");
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

    assert_eq!(response.status, "ok");
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["search_backend"], "internal");
    assert_eq!(telemetry["fallback_reason"], "rg_not_found");
    let flat = expanded_flat_output(&engine, &response).replace('\\', "/");
    assert!(flat.contains("sub/beta.rs:1:needle here"));
}

#[cfg(unix)]
#[test]
fn broken_rg_falls_back_to_internal_with_telemetry() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    search_backend_fixture(dir.path());
    let fake_rg = dir.path().join("fake-rg");
    fs::write(&fake_rg, "#!/bin/sh\necho boom >&2\nexit 2\n").unwrap();
    fs::set_permissions(&fake_rg, fs::Permissions::from_mode(0o755)).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.search_backend = SearchBackend::Rg;
    config.rg_path_override = Some(fake_rg);
    let engine = TokenZeroEngine::new(config);

    let response = engine.find("needle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);

    assert_eq!(response.status, "ok");
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["search_backend"], "internal");
    assert!(
        telemetry["fallback_reason"]
            .as_str()
            .unwrap()
            .contains("rg exited"),
        "{telemetry:#}"
    );
    let flat = expanded_flat_output(&engine, &response);
    assert!(flat.contains("sub/beta.rs:1:needle here"));
}

#[test]
fn explicit_rg_backend_grep_errors_when_rg_is_unusable() {
    let dir = tempdir().unwrap();
    search_backend_fixture(dir.path());
    let mut config = EngineConfig::for_root(dir.path());
    config.search_backend = SearchBackend::Rg;
    config.rg_path_override = Some(dir.path().join("missing-rg-binary"));
    let engine = TokenZeroEngine::new(config);

    // grep's regex semantics cannot be honored without rg: hard error
    // instead of a silent flip to substring matching.
    let grep = engine.grep("ne+dle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_eq!(grep.status, "error");
    assert_eq!(grep.error.as_ref().unwrap().code, "backend_unavailable");

    // find's substring semantics are identical on both backends.
    let find = engine.find("needle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_eq!(find.status, "ok");
}

#[cfg(unix)]
#[test]
fn rg_rows_with_colon_paths_parse_and_unparsed_rows_are_counted() {
    use std::os::unix::fs::PermissionsExt;

    // A rel path containing `:<digits>:` is disambiguated by checking
    // the filesystem.
    let dir = tempdir().unwrap();
    let tricky = dir.path().join("a:1:b.txt");
    fs::write(&tricky, "needle content\n").unwrap();
    let row = format!("{}/a:1:b.txt:1:needle content", dir.path().display());
    let parsed = parse_rg_line(&row, &dir.path().display().to_string()).unwrap();
    assert_eq!(parsed.rel, "a:1:b.txt");
    assert_eq!(parsed.line, 1);
    assert_eq!(parsed.text, "needle content");

    // Unparseable rg rows surface as a telemetry parity canary instead
    // of vanishing silently.
    let fake_rg = dir.path().join("fake-rg");
    fs::write(
        &fake_rg,
        format!(
            "#!/bin/sh\necho 'garbage row without numbers'\necho '{}/a:1:b.txt:1:needle content'\n",
            dir.path().display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_rg, fs::Permissions::from_mode(0o755)).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.search_backend = SearchBackend::Rg;
    config.rg_path_override = Some(fake_rg);
    let engine = TokenZeroEngine::new(config);

    let response = engine.grep("needle", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_eq!(response.status, "ok");
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["search_backend"], "rg");
    assert_eq!(telemetry["rg_unparsed_rows"], 1);
    let flat = expanded_flat_output(&engine, &response);
    assert!(flat.contains("a:1:b.txt:1:needle content"), "{flat}");
}

#[test]
fn glob_discovers_paths_and_roundtrips_exact_output() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/nested")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "lib").unwrap();
    fs::write(dir.path().join("src/nested/mod.rs"), "mod").unwrap();
    fs::write(dir.path().join("README.md"), "readme").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.glob(
        "**/*.rs",
        &[dir.path().to_path_buf()],
        false,
        Mode::Hybrid,
        20,
        4000,
    );

    assert_eq!(response.tool, "glob");
    assert_eq!(response.status, "ok");
    let visible = response.visible.as_ref().unwrap().text.clone();
    assert!(visible.contains("src/lib.rs"));
    assert!(visible.contains("src/nested/mod.rs"));
    assert!(!visible.contains("README.md"));
    let ref_id = response
        .refs
        .iter()
        .find(|row| row.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let expanded = engine.expand(&ref_id, Some("raw"), None, None, None, None);
    assert!(expanded.visible.unwrap().text.contains("src/lib.rs"));
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["transport_status"], "ok");
    assert_eq!(telemetry["degraded"], false);
    assert_eq!(telemetry["storage_error"], Value::Null);
    assert_eq!(telemetry["exact_refs_available"], true);
}

#[test]
fn fresh_arg_bypasses_dedup_via_tools_call() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let call = |id: u64, fresh: bool| -> String {
        let mut arguments = json!({"path": file.display().to_string()});
        if fresh {
            arguments["fresh"] = json!(true);
        }
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": "read", "arguments": arguments}
        });
        let response: Value =
            serde_json::from_str(&handle_jsonrpc(&engine, &request.to_string()).unwrap()).unwrap();
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };

    let first = call(1, true);
    let second = call(2, true);
    assert!(first.contains("line 01"), "{first}");
    assert!(second.contains("line 01"), "{second}");
    assert!(!second.contains("unchanged:"), "{second}");
    // Fresh serves still record: the next normal call dedups.
    let third = call(3, false);
    assert!(third.contains("unchanged:"), "{third}");
}

#[test]
fn search_dedup_notes_and_changed_output_serves_full() {
    let dir = tempdir().unwrap();
    let data = dir.path().join("data.rs");
    let rows = |count: usize| -> String {
        (1..=count)
            .map(|index| {
                format!("let needle_{index:02} = \"session redundancy search fixture row\";")
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    };
    fs::write(&data, rows(12)).unwrap();
    let engine = engine_with_backend(dir.path(), SearchBackend::Internal);
    let roots = vec![dir.path().to_path_buf()];

    let first = engine.find("needle", &roots, Mode::Auto, 20, 4000);
    assert_eq!(first.status, "ok");
    assert!(visible_text(&first).contains("needle_01"));

    let second = engine.find("needle", &roots, Mode::Auto, 20, 4000);
    let note = visible_text(&second);
    assert!(note.starts_with("unchanged: tz://file/"), "{note}");
    assert!(note.contains("# find needle — 12 matches"), "{note}");
    assert!(note.contains("full results: expand tz://blob/"), "{note}");
    assert!(visible_tokens(&second) < visible_tokens(&first));
    let telemetry = second.telemetry.as_ref().unwrap();
    // Dedup keys merge with — never clobber — the search telemetry.
    assert_eq!(telemetry["output_strategy"], "seen_set_dedup");
    assert_eq!(telemetry["cache_hit"], true);
    assert_eq!(telemetry["search_backend"], "internal");
    assert_eq!(telemetry["dedup"]["serve_count"], 2);

    // Changed output is a full serve (search results are never diffed),
    // and the refreshed record notes again afterwards.
    fs::write(&data, rows(13)).unwrap();
    let third = engine.find("needle", &roots, Mode::Auto, 20, 4000);
    let full = visible_text(&third);
    assert!(!full.contains("unchanged:"), "{full}");
    assert!(full.contains("needle_13"), "{full}");
    let fourth = engine.find("needle", &roots, Mode::Auto, 20, 4000);
    assert!(visible_text(&fourth).starts_with("unchanged:"));
}

#[test]
fn recall_zero_hits_renders_note() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.recall("zz_nothing", 10, Mode::Auto, 4000);

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.visible.as_ref().unwrap().text,
        "# recall zz_nothing — 0 matches"
    );
}

#[test]
fn degraded_storage_search_serves_full_not_note() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();
    let cache_dir = dir.path().join("cache-as-directory");
    fs::create_dir_all(&cache_dir).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache_dir;
    let engine = TokenZeroEngine::new(config);

    let first = engine.grep("alpha", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    assert_eq!(first.status, "ok");
    let second = engine.grep("alpha", &[dir.path().to_path_buf()], Mode::Auto, 20, 4000);
    let text = second.visible.as_ref().unwrap().text.clone();
    assert!(text.contains("alpha"), "{text}");
    assert!(!text.contains("unchanged:"), "{text}");
    assert_eq!(
        second.diagnostic.as_ref().unwrap().code,
        "cache_write_failed"
    );
}
