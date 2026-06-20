use super::*;
use std::sync::Arc;
use tempfile::tempdir;
use tokenzero_core::MCP_SCHEMA_VERSION;

#[test]
fn engine_construction_reclaims_orphan_tmp_and_aged_spills() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let set_age = |path: &Path, secs: u64| {
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs))
            .unwrap();
    };
    let orphan_tmp = dir.path().join("recovery-cache.json.999.dead.tmp");
    fs::write(&orphan_tmp, "orphan").unwrap();
    set_age(&orphan_tmp, 2 * 60 * 60);
    let spill_dir = shell_spill_dir(&cache);
    fs::create_dir_all(&spill_dir).unwrap();
    let aged_spill = spill_dir.join("tokenzero-1-1-stdout.log");
    fs::write(&aged_spill, "spill").unwrap();
    set_age(&aged_spill, 48 * 60 * 60);
    let fresh_spill = spill_dir.join("tokenzero-2-2-stdout.log");
    fs::write(&fresh_spill, "spill").unwrap();

    let _engine = TokenZeroEngine::new(EngineConfig {
        cache_path: cache,
        ..EngineConfig::for_root(dir.path())
    });

    assert!(!orphan_tmp.exists(), "orphan tmp must be swept on startup");
    assert!(!aged_spill.exists(), "aged spill must be pruned on startup");
    assert!(fresh_spill.exists(), "fresh spill must survive startup");
}

#[test]
fn read_expand_roundtrip_via_engine() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.txt");
    fs::write(&file, "alpha\nbeta\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let response = engine.read(&[file], Mode::Hybrid, None, None, false, 20, 4000);
    assert_eq!(response.status, "ok");
    let ref_id = response
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let expanded = engine.expand(&ref_id, Some("raw"), None, None, None, None);
    assert_eq!(expanded.visible.unwrap().text, "alpha\nbeta\n");
}

fn hunk(find: &str, replace: &str, replace_all: bool) -> EditHunk {
    EditHunk {
        find: find.to_string(),
        replace: replace.to_string(),
        replace_all,
    }
}

#[test]
fn edit_applies_multi_hunk_batches_byte_exact() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    // No trailing newline on purpose: the write must stay byte-exact.
    fs::write(&file, "alpha\nbeta\ngamma").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let edits = vec![
        hunk("alpha", "ALPHA", false),
        hunk("gamma", "gamma\ndelta", false),
    ];
    let response = engine.edit(&file, &edits, false, false, Mode::Auto, 4000);
    assert_eq!(response.status, "ok", "{:?}", response.error);
    assert_eq!(fs::read(&file).unwrap(), b"ALPHA\nbeta\ngamma\ndelta");

    let text = &response.visible.as_ref().unwrap().text;
    assert!(
        text.starts_with(&format!(
            "# edit {} — 2 hunks applied (+2 -1 lines)",
            file.display()
        )),
        "{text}"
    );
    assert!(text.contains("-alpha") && text.contains("+ALPHA"), "{text}");

    let kinds: Vec<&str> = response.refs.iter().map(|r| r.kind.as_str()).collect();
    for kind in ["blob", "file", "undo"] {
        assert!(kinds.contains(&kind), "missing {kind} ref: {kinds:?}");
    }
    let accounting = response.accounting.as_ref().unwrap();
    assert!(
        accounting.visible_tokens <= accounting.raw_tokens,
        "adaptive floor: visible must never cost more than raw"
    );
}

#[test]
fn edit_rejects_ambiguous_and_missing_hunks_without_writing() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    let original = "dup\ndup\nkeep\n";
    fs::write(&file, original).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let ambiguous = engine.edit(
        &file,
        &[hunk("dup", "other", false)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(ambiguous.status, "error");
    assert_eq!(ambiguous.error.unwrap().code, "ambiguous_hunk");
    assert_eq!(fs::read_to_string(&file).unwrap(), original);

    // A failing hunk later in the batch rolls back the whole batch.
    let missing = engine.edit(
        &file,
        &[
            hunk("keep", "kept", false),
            hunk("absent", "anything", false),
        ],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(missing.status, "error");
    let error = missing.error.unwrap();
    assert_eq!(error.code, "hunk_not_found");
    assert!(error.message.contains("edits[1]"), "{}", error.message);
    assert_eq!(fs::read_to_string(&file).unwrap(), original);

    let no_op = engine.edit(
        &file,
        &[hunk("keep", "keep", false)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(no_op.status, "error");
    assert_eq!(no_op.error.unwrap().code, "no_op_hunk");
    assert_eq!(fs::read_to_string(&file).unwrap(), original);
}

#[test]
fn edit_hunk_not_found_hints_at_closest_line() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, "fn alpha() {}\nfn beta() {}\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.edit(
        &file,
        &[hunk("fn alpha() {}\nfn gamma() {}", "x", false)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(response.status, "error");
    let error = response.error.unwrap();
    assert_eq!(error.code, "hunk_not_found");
    assert!(
        error.message.contains("closest line 1: fn alpha() {}"),
        "{}",
        error.message
    );
}

#[test]
fn edit_replace_all_replaces_every_occurrence() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    fs::write(&file, "x = 1\nx = 2\nx = 3\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.edit(
        &file,
        &[hunk("x = ", "y = ", true)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(response.status, "ok", "{:?}", response.error);
    assert_eq!(fs::read_to_string(&file).unwrap(), "y = 1\ny = 2\ny = 3\n");
    let telemetry = response.telemetry.unwrap();
    assert_eq!(telemetry["lines_added"], 3);
    assert_eq!(telemetry["lines_removed"], 3);
}

#[test]
fn edit_create_writes_new_file_and_rejects_existing() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("new.txt");
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let create_hunk = [hunk("", "one\ntwo\n", false)];
    let response = engine.edit(&file, &create_hunk, true, false, Mode::Auto, 4000);
    assert_eq!(response.status, "ok", "{:?}", response.error);
    assert_eq!(fs::read_to_string(&file).unwrap(), "one\ntwo\n");

    let existing = engine.edit(&file, &create_hunk, true, false, Mode::Auto, 4000);
    assert_eq!(existing.status, "error");
    assert_eq!(existing.error.unwrap().code, "edit_failed");
    assert_eq!(fs::read_to_string(&file).unwrap(), "one\ntwo\n");

    let bad_shape = engine.edit(
        &dir.path().join("other.txt"),
        &[hunk("not-empty", "content", false)],
        true,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(bad_shape.status, "error");
    assert_eq!(bad_shape.error.unwrap().code, "edit_failed");
    assert!(!dir.path().join("other.txt").exists());
}

#[test]
fn edit_dry_run_previews_without_writing() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    fs::write(&file, "alpha\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.edit(
        &file,
        &[hunk("alpha", "beta", false)],
        false,
        true,
        Mode::Auto,
        4000,
    );
    assert_eq!(response.status, "ok", "{:?}", response.error);
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha\n");
    let text = &response.visible.as_ref().unwrap().text;
    assert!(
        text.starts_with(&format!(
            "# edit {} — dry-run: 1 hunks would apply",
            file.display()
        )),
        "{text}"
    );
    assert_eq!(response.telemetry.as_ref().unwrap()["dry_run"], true);

    // The post-image blob still recovers the would-be content.
    let post_ref = response
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let expanded = engine.expand(&post_ref, Some("raw"), None, None, None, None);
    assert_eq!(expanded.visible.unwrap().text, "beta\n");
}

#[test]
fn edit_undo_ref_recovers_exact_pre_image() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    let original = "alpha\nbeta";
    fs::write(&file, original).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.edit(
        &file,
        &[hunk("beta", "gamma", false)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(response.status, "ok", "{:?}", response.error);
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha\ngamma");

    let undo_ref = response
        .refs
        .iter()
        .find(|r| r.kind == "undo")
        .unwrap()
        .ref_id
        .clone();
    let expanded = engine.expand(&undo_ref, Some("raw"), None, None, None, None);
    assert_eq!(expanded.visible.unwrap().text, original);
}

#[test]
fn edit_outside_allowed_roots_is_rejected() {
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let file = outside.path().join("sample.txt");
    fs::write(&file, "alpha\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.edit(
        &file,
        &[hunk("alpha", "beta", false)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(response.status, "error");
    assert_eq!(response.error.unwrap().code, "path_not_allowed");
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha\n");
}

#[test]
fn path_allowed_rejects_prefix_sibling_of_allowed_root() {
    // /base/ws must not admit /base/wsbackup even though the latter is a
    // byte-prefix match; Path::starts_with compares whole components.
    let base = tempdir().unwrap();
    let root = base.path().join("ws");
    fs::create_dir(&root).unwrap();
    let sibling = base.path().join("wsbackup");
    fs::create_dir(&sibling).unwrap();
    let file = sibling.join("sample.txt");
    fs::write(&file, "alpha\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(&root));

    assert!(!engine.path_allowed(&file));
    assert!(engine.path_allowed(&root.join("inside.txt")));
}

#[test]
fn path_allowed_rejects_unresolved_parent_components() {
    // `..` behind a nonexistent component survives
    // canonicalize_existing_prefix; it must fail closed instead of
    // passing the component-wise root check.
    let base = tempdir().unwrap();
    let root = base.path().join("ws");
    fs::create_dir(&root).unwrap();
    let escape = root.join("missing").join("..").join("..").join("out.txt");
    let engine = TokenZeroEngine::new(EngineConfig::for_root(&root));

    assert!(!engine.path_allowed(&escape));
    // `..` behind an existing component still resolves and stays allowed.
    let sub = root.join("sub");
    fs::create_dir(&sub).unwrap();
    assert!(engine.path_allowed(&sub.join("..").join("inside.txt")));
}

#[test]
fn edit_rejects_non_utf8_files() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("blob.bin");
    fs::write(&file, [0xff, 0xfe, 0x00, 0x41]).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.edit(
        &file,
        &[hunk("a", "b", false)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(response.status, "error");
    assert_eq!(response.error.unwrap().code, "not_utf8");
    assert_eq!(fs::read(&file).unwrap(), vec![0xff, 0xfe, 0x00, 0x41]);
}

#[test]
fn malformed_json_returns_error_and_does_not_panic() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let response = handle_jsonrpc(&engine, "{bad").unwrap();
    assert!(response.contains("Parse error"));
}

#[test]
fn tools_list_includes_aliases() {
    let names: Vec<_> = tool_specs().into_iter().map(|t| t.name).collect();
    assert!(names.contains(&"tz_read".to_string()));
    assert!(names.contains(&"read".to_string()));
    assert!(names.contains(&"tz_grep".to_string()));
    assert!(names.contains(&"grep".to_string()));
    assert!(names.contains(&"tz_glob".to_string()));
    assert!(names.contains(&"glob".to_string()));
}

#[test]
fn tools_call_rejects_mixed_type_argv_arrays() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    for invalid_argv in [
        json!(["printf", 7, "ignored"]),
        json!(["printf", null, "ignored"]),
        json!(["printf", {"bad": true}, "ignored"]),
        json!(["printf", false, "ignored"]),
    ] {
        let request = json!({
            "jsonrpc": "2.0",
            "id": "bad-argv",
            "method": "tools/call",
            "params": {
                "name": "rewrite",
                "arguments": {"argv": invalid_argv}
            }
        });
        let response = handle_jsonrpc(&engine, &request.to_string()).unwrap();
        let parsed: Value = serde_json::from_str(&response).unwrap();

        assert_eq!(parsed["error"]["code"], -32602, "{parsed:#}");
        assert_eq!(parsed["error"]["data"]["kind"], "invalid_params");
        assert!(
            parsed["error"]["data"]["reason"]
                .as_str()
                .unwrap()
                .contains("array of strings"),
            "{parsed:#}"
        );
    }
}

#[test]
fn tools_call_rejects_mixed_type_path_arrays() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    for invalid_path in [
        json!(["missing.txt", 1]),
        json!(["missing.txt", null]),
        json!(["missing.txt", {"bad": true}]),
        json!(["missing.txt", false]),
    ] {
        let request = json!({
            "jsonrpc": "2.0",
            "id": "bad-path",
            "method": "tools/call",
            "params": {
                "name": "read",
                "arguments": {"path": invalid_path}
            }
        });
        let response = handle_jsonrpc(&engine, &request.to_string()).unwrap();
        let parsed: Value = serde_json::from_str(&response).unwrap();

        assert_eq!(parsed["error"]["code"], -32602, "{parsed:#}");
        assert_eq!(parsed["error"]["data"]["kind"], "invalid_params");
        assert!(
            parsed["error"]["data"]["reason"]
                .as_str()
                .unwrap()
                .contains("array of strings"),
            "{parsed:#}"
        );
    }
}

#[test]
fn mcp_idle_timeout_zero_disables_and_large_values_clamp() {
    assert_eq!(mcp_idle_timeout_from_secs(Some(0)), None);
    assert_eq!(
        mcp_idle_timeout_from_secs(Some(1)).unwrap(),
        Duration::from_secs(1)
    );
    assert_eq!(
        mcp_idle_timeout_from_secs(Some(u64::MAX)).unwrap(),
        Duration::from_secs(MAX_MCP_IDLE_TIMEOUT_SECS)
    );
    assert_eq!(mcp_idle_timeout_from_secs(None), None);
    assert_eq!(DEFAULT_MCP_IDLE_TIMEOUT_SECS, 0);
}

#[test]
fn compact_shell_text_render_omits_ref_footer() {
    let mut response = ToolResponse::ok(
        "shell",
        Mode::Passthrough,
        "11.12.1".to_string(),
        vec![
            ref_record("stdout", "tz://blob/stdout".to_string(), 8),
            ref_record("combined", "tz://blob/combined".to_string(), 45),
        ],
        Accounting {
            raw_tokens: 15,
            visible_tokens: 2,
            recovery_tokens: 0,
            exact_ref_tokens: Some(14),
        },
    );
    response.telemetry = Some(json!({
        "output_strategy": "compact_adaptive_shell"
    }));

    assert_eq!(render_text(&response), "11.12.1\n");
}

#[test]
fn full_shell_text_render_does_not_duplicate_header_refs() {
    // exact_first_adaptive_shell capsules carry stdout/stderr/combined
    // refs in their header; the trailer must only add refs the visible
    // text lacks (capture_ref), never repeat the anchored ones.
    let visible = "# shell\ncommand: seq 1 300\nstatus: command_success\n\
                       stdout_ref: tz://blob/bstdout\nstderr_ref: tz://blob/bstderr\n\
                       combined_ref: tz://blob/bcombined\n\n1\n2"
        .to_string();
    let mut response = ToolResponse::ok(
        "shell",
        Mode::Auto,
        visible,
        vec![
            ref_record("stdout", "tz://blob/bstdout".to_string(), 8),
            ref_record("stderr", "tz://blob/bstderr".to_string(), 0),
            ref_record("combined", "tz://blob/bcombined".to_string(), 45),
            ref_record("capture", "tz://blob/bcapture".to_string(), 60),
        ],
        Accounting {
            raw_tokens: 100,
            visible_tokens: 40,
            recovery_tokens: 0,
            exact_ref_tokens: Some(28),
        },
    );
    response.telemetry = Some(json!({
        "output_strategy": "exact_first_adaptive_shell"
    }));

    let text = render_text(&response);
    assert_eq!(text.matches("tz://blob/bstdout").count(), 1, "{text}");
    assert_eq!(text.matches("tz://blob/bstderr").count(), 1, "{text}");
    assert_eq!(text.matches("tz://blob/bcombined").count(), 1, "{text}");
    assert!(text.contains("capture_ref: tz://blob/bcapture"), "{text}");
}

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
fn read_reports_detected_content_type() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("state.md");
    fs::write(&file, "# Session\n\nmarkdown body\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.read(&[file], Mode::Auto, None, None, false, 20, 4000);

    assert_eq!(response.status, "ok");
    assert_eq!(response.content_type.as_deref(), Some("markdown"));
}

#[test]
fn read_line_range_only_stores_requested_slice() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("large.log");
    fs::write(&file, format!("one\n{}\nthree\n", "x".repeat(10_000))).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.read(&[file], Mode::Auto, Some(1), Some(1), true, 20, 4000);

    assert_eq!(response.status, "ok");
    assert_eq!(response.visible.as_ref().unwrap().text, "one");
    let blob = response
        .refs
        .iter()
        .find(|record| record.kind == "blob")
        .unwrap();
    assert_eq!(blob.bytes, 3);
}

#[test]
fn missing_read_inside_allowed_root_reports_read_failed() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("missing.md");
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.read(&[missing], Mode::Auto, None, None, false, 20, 4000);

    assert_eq!(response.status, "error");
    assert_eq!(response.error.unwrap().code, "read_failed");
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
fn read_degrades_when_recovery_cache_is_unwritable() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, "fn alpha() {}\n").unwrap();
    let cache_dir = dir.path().join("cache-as-directory");
    fs::create_dir_all(&cache_dir).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache_dir;
    let engine = TokenZeroEngine::new(config);

    let response = engine.read(&[file], Mode::Auto, None, None, false, 20, 4000);

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.diagnostic.as_ref().unwrap().code,
        "cache_write_failed"
    );
    assert!(response.visible.unwrap().text.contains("alpha"));
    assert!(response.refs.is_empty());
}

#[test]
fn ingest_degrades_when_recovery_cache_is_unwritable() {
    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join("cache-as-directory");
    fs::create_dir_all(&cache_dir).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache_dir;
    let engine = TokenZeroEngine::new(config);

    let response = engine.ingest("alpha\nbeta\n", ContentType::Logs, Mode::Auto, "logs");

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.diagnostic.as_ref().unwrap().code,
        "cache_write_failed"
    );
    assert!(response.visible.unwrap().text.contains("alpha"));
    assert!(response.refs.is_empty());
}

#[test]
fn read_of_empty_file_renders_zero_payload_note_and_roundtrips_exact_payload() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.read(&[file], Mode::Auto, None, None, false, 20, 4000);

    assert_eq!(response.status, "ok");
    let visible = response.visible.as_ref().unwrap().text.clone();
    assert!(visible.starts_with("# read "));
    assert!(visible.ends_with("— 0 bytes"));
    assert_eq!(
        response.accounting.as_ref().unwrap().visible_tokens,
        count_tokens(&visible)
    );
    // The note is visible-only: the stored payload stays the exact bytes.
    let ref_id = response
        .refs
        .iter()
        .find(|record| record.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let expanded = engine.expand(&ref_id, Some("raw"), None, None, None, None);
    assert_eq!(expanded.status, "ok");
    assert_eq!(expanded.visible.unwrap().text, "");
}

#[test]
fn read_of_past_eof_range_renders_zero_payload_note() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("short.txt");
    fs::write(&file, "one\ntwo\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.read(&[file], Mode::Auto, Some(10), Some(12), false, 20, 4000);

    assert_eq!(response.status, "ok");
    let visible = response.visible.as_ref().unwrap().text.clone();
    assert!(visible.starts_with("# read "));
    assert!(visible.ends_with("— 0 bytes"));
}

#[test]
fn raw_read_of_empty_file_stays_verbatim_without_note() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.read(&[file], Mode::Auto, None, None, true, 20, 4000);

    assert_eq!(response.status, "ok");
    assert_eq!(response.visible.as_ref().unwrap().text, "");
}

#[test]
fn passthrough_read_of_empty_file_keeps_verbatim_empty_payload() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.read(&[file], Mode::Passthrough, None, None, false, 20, 4000);

    assert_eq!(response.status, "ok");
    assert_eq!(response.visible.as_ref().unwrap().text, "");
}

#[test]
fn ingest_of_empty_text_renders_zero_payload_note() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.ingest("", ContentType::Logs, Mode::Auto, "logs");

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.visible.as_ref().unwrap().text,
        "# ingest — 0 bytes"
    );
    assert_eq!(
        response.accounting.as_ref().unwrap().visible_tokens,
        count_tokens("# ingest — 0 bytes")
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
fn concurrent_record_fetch_keeps_every_entry() {
    let dir = tempdir().unwrap();
    let index_path = dir.path().join("fetch-cache.json");
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let path = index_path.clone();
            std::thread::spawn(move || {
                for j in 0..10 {
                    let url = format!("https://example.com/{i}/{j}");
                    record_fetch(&path, &url, &format!("tz://blob/b{i}{j}"), 1);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }

    let index = load_fetch_index(&index_path);
    assert_eq!(
        index.entries.len(),
        80,
        "every concurrent insert must survive the read-modify-write, got {}",
        index.entries.len()
    );
}

#[test]
fn truncated_fetch_index_does_not_mass_invalidate_via_atomic_write() {
    let dir = tempdir().unwrap();
    let index_path = dir.path().join("fetch-cache.json");
    record_fetch(&index_path, "https://example.com/a", "tz://blob/ba", 1);

    // No reader ever observes a torn file: a crash leaves either the prior
    // complete file or the new complete one, never a truncated index that
    // load_fetch_index would silently treat as empty. Assert the post-write
    // file is valid JSON and complete, and that no temp debris remains.
    let index = load_fetch_index(&index_path);
    assert!(index.entries.contains_key("https://example.com/a"));
    let debris: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(debris.is_empty(), "atomic write must leave no temp debris");
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

fn engine_with_backend(root: &Path, backend: SearchBackend) -> TokenZeroEngine {
    let mut config = EngineConfig::for_root(root);
    config.search_backend = backend;
    // Pin the PATH lookup regardless of ambient TOKENZERO_RG_PATH.
    config.rg_path_override = None;
    TokenZeroEngine::new(config)
}

fn search_backend_fixture(root: &Path) {
    fs::write(root.join("alpha.rs"), "fn alpha() {}\nlet needle = 1;\n").unwrap();
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(
        root.join("sub/beta.rs"),
        "needle here\nno match\nneedle again\n",
    )
    .unwrap();
    fs::create_dir_all(root.join(".hidden")).unwrap();
    fs::write(root.join(".hidden/skip.rs"), "needle hidden\n").unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    fs::write(root.join("target/skip.rs"), "needle target\n").unwrap();
}

fn expanded_flat_output(engine: &TokenZeroEngine, response: &ToolResponse) -> String {
    let blob_ref = response
        .refs
        .iter()
        .find(|row| row.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let expanded = engine.expand(&blob_ref, None, None, None, None, None);
    assert_eq!(expanded.status, "ok");
    expanded.visible.as_ref().unwrap().text.clone()
}

/// rg presence is a machine property, not a test property: skip the
/// rg-side assertions at runtime instead of `#[ignore]` so the parity
/// suite still runs everywhere rg is installed.
fn rg_or_skip(test: &str) -> bool {
    if find_rg_in_path().is_some() {
        return true;
    }
    eprintln!("skipping {test}: rg not found in PATH");
    false
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
fn shell_exact_first_stores_stream_refs_and_status_truth() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let (command, argv, expanded_needle) = if cfg!(windows) {
        (
            "powershell -NoProfile -Command [Console]::Out.Write('alpha'); [Console]::Error.Write('beta'); exit 7",
            Some(vec![
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "[Console]::Out.Write('alpha'); [Console]::Error.Write('beta'); exit 7".to_string(),
            ]),
            "$ powershell -NoProfile",
        )
    } else {
        ("false | true", None, "$ false | true")
    };

    let response = engine.shell(
        command,
        argv,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    assert_eq!(response.status, "ok");
    assert_eq!(response.mode.as_deref(), Some("diagnostic"));
    assert_eq!(
        response.telemetry.as_ref().unwrap()["transport_status"],
        "ok"
    );
    assert_eq!(
        response.telemetry.as_ref().unwrap()["command_success"],
        false
    );
    assert!(
        response
            .refs
            .iter()
            .any(|row| row.kind == "stdout" || row.kind == "stderr")
    );
    let combined_ref = response
        .refs
        .iter()
        .find(|row| row.kind == "combined")
        .unwrap()
        .ref_id
        .clone();
    let expanded = engine.expand(&combined_ref, Some("raw"), None, None, None, None);
    assert!(expanded.visible.unwrap().text.contains(expanded_needle));
}

#[test]
fn shell_command_strings_preserve_shell_operators() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.shell(
        "echo one && echo two",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.telemetry.as_ref().unwrap()["execution_mode"],
        "shell"
    );
    let stdout_preview = response.telemetry.as_ref().unwrap()["stdout_preview"]
        .as_str()
        .unwrap();
    assert_eq!(
        stdout_preview
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>(),
        vec!["one", "two"]
    );
    assert_eq!(
        response.telemetry.as_ref().unwrap()["command_success"],
        true
    );
}

#[test]
fn shell_capture_record_is_compact_json() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.shell(
        "echo compact",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    let capture_ref = response
        .refs
        .iter()
        .find(|record| record.kind == "capture")
        .unwrap()
        .ref_id
        .clone();
    let expanded = engine.expand(&capture_ref, Some("raw"), None, None, None, None);
    let capture_text = expanded.visible.unwrap().text;
    assert!(serde_json::from_str::<Value>(&capture_text).is_ok());
    assert_eq!(capture_text.lines().count(), 1);
}

#[cfg(not(windows))]
#[test]
fn shell_truncation_is_explicit_and_degraded() {
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.shell_capture_bytes = 12;
    config.shell_spill_bytes = 6;
    let engine = TokenZeroEngine::new(config);

    let response = engine.shell(
        "yes x | head -c 100",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.diagnostic.as_ref().unwrap().code,
        "shell_output_truncated"
    );
    assert!(
        response
            .visible
            .as_ref()
            .unwrap()
            .text
            .contains("tokenzero:stdout truncated")
    );
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["transport_status"], "degraded");
    assert_eq!(telemetry["output_truncated"], true);
    assert_eq!(telemetry["stdout_capture"]["truncated"], true);
    assert_eq!(telemetry["stdout_capture"]["bytes_seen"], 100);
    let spill_path = telemetry["stdout_capture"]["spill_path"].as_str().unwrap();
    assert_eq!(std::fs::metadata(spill_path).unwrap().len(), 100);
    assert_eq!(
        response.safety.as_ref().unwrap()["refs_cover_full_output"],
        false
    );
}

#[cfg(windows)]
#[test]
fn shell_command_string_adapts_raw_powershell_variables() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let script = "$tzTmp = Join-Path $env:TEMP 'tz-quote'; [Console]::Out.Write($tzTmp)";

    let response = engine.shell(
        script,
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    assert_eq!(response.status, "ok");
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["command_success"], true);
    assert_eq!(telemetry["execution_mode"], "shell");
    assert_eq!(telemetry["argv"][0], "powershell");
    assert!(
        telemetry["stdout_preview"]
            .as_str()
            .unwrap()
            .ends_with("tz-quote")
    );
}

#[test]
fn shell_accepts_common_command_argument_aliases() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    for args in [
        json!({"cmd": "echo alias"}),
        json!({"input": "echo input"}),
        json!({"args": ["echo", "one", "&&", "echo", "two"]}),
        json!(["echo", "array"]),
    ] {
        let response = call_tool(&engine, "shell", &args, None).unwrap();
        assert!(
            response.get("isError").is_none(),
            "alias args must execute successfully: {response}"
        );
        let text = response["content"][0]["text"].as_str().unwrap();
        assert!(!text.is_empty(), "{response}");
    }
}

#[test]
fn cache_pack_is_daemonless_and_deterministic() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("AGENTS.md"), "stable instructions\n").unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let first = engine.cache_pack("agent");
    let second = engine.cache_pack("agent");

    assert_eq!(first.status, "ok");
    assert_eq!(second.status, "ok");
    assert_eq!(first.telemetry.as_ref().unwrap()["daemon_required"], false);
    assert_eq!(
        first.telemetry.as_ref().unwrap()["content_digest"],
        second.telemetry.as_ref().unwrap()["content_digest"]
    );
    assert_eq!(
        second.telemetry.as_ref().unwrap()["invalidation_reason"],
        "unchanged"
    );
    assert!(first.refs.iter().any(|row| row.kind == "stable_prefix"));
    assert!(first.refs.iter().any(|row| row.kind == "volatile_tail"));
    let expected_ref_tokens = exact_ref_token_count(&first.refs);
    assert_eq!(
        first.accounting.as_ref().unwrap().exact_ref_tokens,
        Some(expected_ref_tokens)
    );
}

#[test]
fn mcp_lists_and_calls_cache_pack_tool() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("AGENTS.md"), "stable\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let listed: Value = serde_json::from_str(
        &handle_jsonrpc(
            &engine,
            r#"{"jsonrpc":"2.0","id":10,"method":"tools/list","params":{}}"#,
        )
        .unwrap(),
    )
    .unwrap();
    let names: Vec<_> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(names.contains(&"tz_cache_pack"));
    assert!(names.contains(&"cache_pack"));

    let read_tool = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "tz_read")
        .unwrap();
    assert!(
        read_tool["inputSchema"].get("$schema").is_none(),
        "tools/list schemas stay lean; the dialect is implied"
    );
    assert_eq!(read_tool["inputSchema"]["additionalProperties"], false);
    assert_eq!(read_tool["inputSchema"]["required"][0], "path");
    let description = read_tool["description"].as_str().unwrap();
    assert!(
        !description.is_empty() && description.len() < 300,
        "tools/list descriptions stay compact: {description}"
    );
    assert!(description.contains("tz://"), "{description}");

    // Long-form docs moved to the catalog resource (progressive disclosure).
    let docs: Value = serde_json::from_str(
            &handle_jsonrpc(
                &engine,
                r#"{"jsonrpc":"2.0","id":12,"method":"resources/read","params":{"uri":"resource://tokenzero/tools"}}"#,
            )
            .unwrap(),
        )
        .unwrap();
    let docs_text = docs["result"]["contents"][0]["text"].as_str().unwrap();
    let docs_payload: Value = serde_json::from_str(docs_text).unwrap();
    let read_doc = docs_payload["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "tz_read")
        .unwrap();
    let read_doc_description = read_doc["description"].as_str().unwrap();
    for required_section in [
        "Discovery",
        "When to use",
        "Do / Don't",
        "Examples",
        "Common mistakes",
        "Idempotency",
    ] {
        assert!(
            read_doc_description.contains(required_section),
            "missing {required_section} in {read_doc_description}"
        );
    }

    let alias_tool = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "read")
        .unwrap();
    // Aliases advertise a permissive stub on the wire; the canonical schema
    // stays recoverable from the catalog resource.
    assert_eq!(alias_tool["inputSchema"], json!({"type": "object"}));
    let alias_doc = docs_payload["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "read")
        .unwrap();
    assert_eq!(alias_doc["inputSchema"], read_tool["inputSchema"]);

    let shell_tool = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "tz_shell")
        .unwrap();
    assert_eq!(shell_tool["inputSchema"]["additionalProperties"], false);
    // Top-level schema combinators make some MCP clients (Claude Code
    // among them) drop the tool from the model's tool list entirely;
    // every advertised schema must stay a plain object.
    for tool in listed["result"]["tools"].as_array().unwrap() {
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object", "tool {}", tool["name"]);
        for key in ["anyOf", "oneOf", "allOf"] {
            assert!(
                schema.get(key).is_none(),
                "tool {} advertises top-level {key}",
                tool["name"]
            );
        }
    }

    let called: Value = serde_json::from_str(
            &handle_jsonrpc(
                &engine,
                r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"cache_pack","arguments":{"scope":"agent"}}}"#,
            )
            .unwrap(),
        )
        .unwrap();
    assert!(
        called["result"].get("structuredContent").is_none(),
        "default envelope is text-only: {called}"
    );
    let called_text = called["result"]["content"][0]["text"].as_str().unwrap();
    assert!(called_text.contains("tz://"), "{called_text}");

    let pack = engine.cache_pack("agent");
    assert_eq!(pack.tool, "cache-pack");
    assert_eq!(
        pack.telemetry.as_ref().unwrap()["daemon_required"],
        false,
        "cache packs stay daemonless"
    );
}

#[test]
fn mcp_envelope_is_text_only_by_default() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response: Value = serde_json::from_str(
            &handle_jsonrpc(
                &engine,
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"shell","arguments":{"command":"echo compact-envelope-check"}}}"#,
            )
            .unwrap(),
        )
        .unwrap();

    let result = &response["result"];
    assert!(
        result.get("structuredContent").is_none(),
        "default tool results are text-only: {result}"
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("compact-envelope-check"), "{text}");
    assert!(
        text.contains("combined_ref: tz://") || text.contains("refs: tz://"),
        "shell text must keep a recovery anchor: {text}"
    );

    // Reads carry their recovery refs in a text footer instead of a
    // structured envelope.
    fs::write(dir.path().join("sample.txt"), "alpha\nbeta\n").unwrap();
    // JSON-encode the path so Windows backslashes survive the raw envelope.
    let sample_path =
        serde_json::to_string(&dir.path().join("sample.txt").display().to_string()).unwrap();
    let read: Value = serde_json::from_str(
            &handle_jsonrpc(
                &engine,
                &format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"read","arguments":{{"path":{sample_path}}}}}}}"#,
                ),
            )
            .unwrap(),
        )
        .unwrap();
    let read_text = read["result"]["content"][0]["text"].as_str().unwrap();
    assert!(read_text.contains("alpha"), "{read_text}");
    assert!(read_text.contains("refs: tz://blob/"), "{read_text}");
    // The edit hint rides the refs footer on read responses only: it steers
    // agents to tz_edit instead of a doomed native-Edit-after-tz_read loop.
    assert!(read_text.contains("edit: tz_edit"), "{read_text}");
    assert!(
        !text.contains("edit: tz_edit"),
        "shell responses must not carry the read edit hint: {text}"
    );

    // The opt-in compact envelope still prunes payload duplicates and
    // forensic telemetry.
    let shell = engine.shell(
        "echo compact-envelope-check",
        None,
        None,
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );
    let cli = tools::compact_cli_envelope(&shell);
    assert_eq!(cli["telemetry"]["command_success"], true);
    assert!(cli["accounting"].is_object(), "{cli}");
    assert!(
        cli.get("visible").is_none(),
        "capsule text must not be duplicated in the envelope: {cli}"
    );
    for pruned in ["argv", "stdout_preview", "stderr_preview", "stdout_capture"] {
        assert!(
            cli["telemetry"].get(pruned).is_none(),
            "telemetry.{pruned} should be pruned: {cli}"
        );
    }
}

#[test]
fn initialize_echoes_supported_stable_protocol() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let response = handle_jsonrpc(
            &engine,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"conformance-client","version":"1.0.0"}}}"#,
        )
        .unwrap();
    let parsed: Value = serde_json::from_str(&response).unwrap();

    assert_eq!(parsed["result"]["protocolVersion"], "2025-06-18");
    assert!(parsed["result"]["capabilities"]["tools"].is_object());
}

#[test]
fn resource_discovery_and_prompt_lists_are_supported() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let resources: Value = serde_json::from_str(
        &handle_jsonrpc(
            &engine,
            r#"{"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}"#,
        )
        .unwrap(),
    )
    .unwrap();
    let prompts: Value = serde_json::from_str(
        &handle_jsonrpc(
            &engine,
            r#"{"jsonrpc":"2.0","id":3,"method":"prompts/list","params":{}}"#,
        )
        .unwrap(),
    )
    .unwrap();

    let resource_uris = resources["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|resource| resource["uri"].as_str())
        .collect::<Vec<_>>();
    assert!(resource_uris.contains(&"resource://tokenzero/capabilities"));
    assert!(resource_uris.contains(&"resource://tokenzero/tools"));
    assert_eq!(resources["result"]["resultType"], "complete");
    assert_eq!(prompts["result"]["prompts"].as_array().unwrap().len(), 0);

    let capabilities: Value = serde_json::from_str(
            &handle_jsonrpc(
                &engine,
                r#"{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"resource://tokenzero/capabilities"}}"#,
            )
            .unwrap(),
        )
        .unwrap();
    let text = capabilities["result"]["contents"][0]["text"]
        .as_str()
        .unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();
    assert_eq!(payload["schema_version"], MCP_SCHEMA_VERSION);
    assert!(
        payload["tool_clusters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|cluster| cluster["cluster"] == "material")
    );
    assert!(payload["next_actions"].as_array().unwrap().len() >= 2);
}

#[test]
fn mcp_error_data_guides_unknown_tools_and_resources() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let unknown_tool: Value = serde_json::from_str(
            &handle_jsonrpc(
                &engine,
                r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"tz_reed","arguments":{}}}"#,
            )
            .unwrap(),
        )
        .unwrap();
    let tool_data = &unknown_tool["error"]["data"];
    assert_eq!(unknown_tool["error"]["code"], -32602);
    assert_eq!(tool_data["error_type"], "NOT_FOUND");
    assert_eq!(tool_data["recoverable"], true);
    assert_eq!(tool_data["entity_type"], "tool");
    assert_eq!(tool_data["provided"], "tz_reed");
    assert!(
        tool_data["available_options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool == "tz_read")
    );
    assert_eq!(
        tool_data["suggestions"][0]["value"], "tz_read",
        "{tool_data}"
    );
    assert!(
        tool_data["fix_hint"]
            .as_str()
            .unwrap()
            .contains("tools/list")
    );
    assert_eq!(tool_data["suggested_tool_calls"][0]["method"], "tools/list");

    let unknown_resource: Value = serde_json::from_str(
            &handle_jsonrpc(
                &engine,
                r#"{"jsonrpc":"2.0","id":13,"method":"resources/read","params":{"uri":"resource://tokenzero/toolz"}}"#,
            )
            .unwrap(),
        )
        .unwrap();
    let resource_data = &unknown_resource["error"]["data"];
    assert_eq!(unknown_resource["error"]["code"], -32602);
    assert_eq!(resource_data["error_type"], "NOT_FOUND");
    assert_eq!(resource_data["recoverable"], true);
    assert_eq!(resource_data["entity_type"], "resource");
    assert!(
        resource_data["available_options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|uri| uri == "resource://tokenzero/tools")
    );
    assert_eq!(
        resource_data["suggestions"][0]["value"], "resource://tokenzero/tools",
        "{resource_data}"
    );
    assert!(
        resource_data["fix_hint"]
            .as_str()
            .unwrap()
            .contains("resources/list")
    );
    assert_eq!(
        resource_data["suggested_tool_calls"][0]["method"],
        "resources/list"
    );
}

#[test]
fn mcp_error_data_guides_missing_params_and_unknown_methods() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let missing_tool_name: Value = serde_json::from_str(
        &handle_jsonrpc(
            &engine,
            r#"{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{}}"#,
        )
        .unwrap(),
    )
    .unwrap();
    let missing_data = &missing_tool_name["error"]["data"];
    assert_eq!(missing_tool_name["error"]["code"], -32602);
    assert_eq!(missing_data["error_type"], "INVALID_ARGUMENT");
    assert_eq!(missing_data["recoverable"], true);
    assert_eq!(missing_data["param"], "name");
    assert!(
        missing_data["available_options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool == "tz_read")
    );
    assert!(
        missing_data["fix_hint"]
            .as_str()
            .unwrap()
            .contains("tools/list")
    );
    assert_eq!(
        missing_data["suggested_tool_calls"][0]["method"],
        "tools/list"
    );

    let unknown_method: Value = serde_json::from_str(
        &handle_jsonrpc(
            &engine,
            r#"{"jsonrpc":"2.0","id":15,"method":"tools/lits","params":{}}"#,
        )
        .unwrap(),
    )
    .unwrap();
    let method_data = &unknown_method["error"]["data"];
    assert_eq!(unknown_method["error"]["code"], -32601);
    assert_eq!(method_data["error_type"], "NOT_FOUND");
    assert_eq!(method_data["recoverable"], true);
    assert_eq!(method_data["entity_type"], "method");
    assert_eq!(method_data["suggestions"][0]["value"], "tools/list");
    assert!(
        method_data["available_options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|method| method == "tools/call")
    );
    assert_eq!(
        method_data["suggested_tool_calls"][0]["method"],
        "server/discover"
    );
}

// ---- Session redundancy layer (docs/routing.md §5) ----

fn dedup_fixture_content() -> String {
    (1..=40)
        .map(|index| {
            format!(
                "line {index:02}: session redundancy fixture content wide enough to out-cost a note"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn read_ok(engine: &TokenZeroEngine, file: &Path) -> ToolResponse {
    let response = engine.read(
        &[file.to_path_buf()],
        Mode::Auto,
        None,
        None,
        false,
        20,
        4000,
    );
    assert_eq!(response.status, "ok", "{:?}", response.error);
    response
}

fn visible_text(response: &ToolResponse) -> String {
    response.visible.as_ref().unwrap().text.clone()
}

fn visible_tokens(response: &ToolResponse) -> usize {
    response.accounting.as_ref().unwrap().visible_tokens
}

#[test]
fn second_identical_read_collapses_to_unchanged_note() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let first = read_ok(&engine, &file);
    assert!(visible_text(&first).contains("line 01"));

    let second = read_ok(&engine, &file);
    let note = visible_text(&second);
    assert!(note.starts_with("unchanged: tz://file/"), "{note}");
    assert!(note.contains("(served earlier this session)"), "{note}");
    assert!(note.contains("— 40 lines"), "{note}");
    assert!(note.contains("full bytes: expand tz://blob/"), "{note}");
    assert!(
        visible_tokens(&second) < visible_tokens(&first),
        "note must be strictly cheaper: {} vs {}",
        visible_tokens(&second),
        visible_tokens(&first)
    );
    // Raw accounting stays the stored payload's size.
    assert_eq!(
        second.accounting.as_ref().unwrap().raw_tokens,
        first.accounting.as_ref().unwrap().raw_tokens
    );
    let telemetry = second.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["output_strategy"], "seen_set_dedup");
    assert_eq!(telemetry["cache_hit"], true);
    assert_eq!(telemetry["dedup"]["serve_count"], 2);
    assert_eq!(telemetry["dedup"]["hits"], 1);
    assert!(telemetry["dedup"]["visible_tokens_saved"].as_u64().unwrap() > 0);
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
fn raw_read_bypasses_note_but_still_records() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    let content = dedup_fixture_content();
    fs::write(&file, &content).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    let raw = engine.read(
        std::slice::from_ref(&file),
        Mode::Auto,
        None,
        None,
        true,
        20,
        4000,
    );
    assert_eq!(raw.status, "ok");
    assert_eq!(visible_text(&raw), content.trim_end());
    assert!(!visible_text(&raw).contains("unchanged:"));
    // The raw serve still recorded, so the next normal read dedups.
    let third = read_ok(&engine, &file);
    assert!(visible_text(&third).starts_with("unchanged:"));
}

#[test]
fn passthrough_read_bypasses_session_dedup() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    let passthrough = engine.read(
        std::slice::from_ref(&file),
        Mode::Passthrough,
        None,
        None,
        false,
        20,
        4000,
    );
    assert_eq!(passthrough.status, "ok");
    assert!(visible_text(&passthrough).contains("line 01"));
    assert!(!visible_text(&passthrough).contains("unchanged:"));
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
fn diff_then_unchanged_read_notes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    let changed = dedup_fixture_content().replace("line 20:", "line 20 (changed):");
    fs::write(&file, &changed).unwrap();
    let diff_serve = read_ok(&engine, &file);
    assert!(visible_text(&diff_serve).contains("changed since served this session"));
    // The record now holds the new hash: an identical re-read notes.
    let third = read_ok(&engine, &file);
    assert!(visible_text(&third).starts_with("unchanged:"));
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
fn multi_file_read_mixes_strategies() {
    let dir = tempdir().unwrap();
    let stable = dir.path().join("stable.rs");
    let moving = dir.path().join("moving.rs");
    fs::write(&stable, dedup_fixture_content()).unwrap();
    fs::write(&moving, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let both = vec![stable.clone(), moving.clone()];
    let first = engine.read(&both, Mode::Auto, None, None, false, 20, 4000);
    assert_eq!(first.status, "ok");

    let changed = dedup_fixture_content().replace("line 20:", "line 20 (changed):");
    fs::write(&moving, &changed).unwrap();
    let second = engine.read(&both, Mode::Auto, None, None, false, 20, 4000);
    assert_eq!(second.status, "ok");
    let text = visible_text(&second);
    assert!(text.contains("unchanged: tz://file/"), "{text}");
    assert!(text.contains("changed since served this session"), "{text}");
    let telemetry = second.telemetry.as_ref().unwrap();
    assert_eq!(
        telemetry["output_strategy"],
        "seen_set_dedup+diff_since_served"
    );
    assert!(telemetry["dedup"].is_object());
    assert!(telemetry["diff"].is_object());
}

#[test]
fn read_after_edit_serves_unchanged_note_not_diff() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    let edit = engine.edit(
        &file,
        &[EditHunk {
            find: "line 01".to_string(),
            replace: "line 01 edited".to_string(),
            replace_all: false,
        }],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(edit.status, "ok");

    // The edit seeded the seen-set with the post-image: the re-read is
    // an unchanged note, not a diff against the pre-edit serve.
    let reread = read_ok(&engine, &file);
    let text = visible_text(&reread);
    assert!(text.starts_with("unchanged:"), "{text}");
}

#[test]
fn recall_finds_previously_served_payloads() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("notes.md");
    fs::write(&file, "alpha\nrecall_target_token here\nomega\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    let response = engine.recall("RECALL_TARGET", 10, Mode::Auto, 4000);
    assert_eq!(response.status, "ok");
    let text = response.visible.as_ref().unwrap().text.clone();
    assert!(text.contains("recall_target_token"), "{text}");
    let hit_ref = text
        .split_whitespace()
        .find(|word| word.starts_with("tz://"))
        .unwrap()
        .to_string();
    // The hit's ref recovers the exact stored bytes — recall never
    // requires re-reading the source.
    let expanded = engine.expand(&hit_ref, None, None, None, None, None);
    assert!(
        expanded
            .visible
            .unwrap()
            .text
            .contains("recall_target_token here")
    );
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
fn recall_unreadable_cache_degrades_cleanly() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    fs::write(&cache, "{broken").unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache;
    let engine = TokenZeroEngine::new(config);

    let response = engine.recall("x", 10, Mode::Auto, 4000);

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.diagnostic.as_ref().unwrap().code,
        "recall_cache_unreadable"
    );
}

#[test]
fn recall_caps_hits_and_reports_truncation() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("many.txt");
    fs::write(&file, "tok\n".repeat(20)).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    let response = engine.recall("tok", 3, Mode::Auto, 4000);

    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["hits"], 3);
    assert_eq!(telemetry["truncated_by_results"], true);
}

#[cfg(unix)]
#[test]
fn fetch_caches_within_ttl_and_refetches_when_fresh() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let fake_curl = dir.path().join("fake-curl");
    let marker = dir.path().join("invocations.log");
    fs::write(
        &fake_curl,
        format!(
            "#!/bin/sh\necho invoked >> {}\nprintf 'fetched body line\\n'\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_curl, fs::Permissions::from_mode(0o755)).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.curl_path_override = Some(fake_curl);
    config.fetch_enabled = true;
    config.fetch_allow_hosts = vec!["example.com".to_string()];
    let engine = TokenZeroEngine::new(config);

    let first = engine.fetch("https://example.com/doc", None, false, Mode::Auto, 4000);
    assert_eq!(first.status, "ok");
    assert!(
        first
            .visible
            .as_ref()
            .unwrap()
            .text
            .contains("fetched body line")
    );
    assert!(first.refs.iter().any(|row| row.kind == "blob"));
    assert_eq!(first.telemetry.as_ref().unwrap()["cache_hit"], false);

    // Within the TTL the network is never touched: same body, no second
    // curl invocation.
    let second = engine.fetch("https://example.com/doc", None, false, Mode::Auto, 4000);
    assert_eq!(second.status, "ok");
    assert_eq!(second.telemetry.as_ref().unwrap()["cache_hit"], true);
    assert!(
        second
            .visible
            .as_ref()
            .unwrap()
            .text
            .contains("fetched body line")
    );
    assert!(second.refs.iter().any(|row| row.kind == "blob"));
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 1);

    // fresh=true bypasses the TTL.
    let third = engine.fetch("https://example.com/doc", None, true, Mode::Auto, 4000);
    assert_eq!(third.telemetry.as_ref().unwrap()["cache_hit"], false);
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 2);
}

#[cfg(unix)]
#[test]
fn fetch_cache_hits_still_obey_current_deny_policy() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let fake_curl = dir.path().join("fake-curl");
    let marker = dir.path().join("invocations.log");
    fs::write(
        &fake_curl,
        format!(
            "#!/bin/sh\necho invoked >> {}\nprintf 'cached sensitive body\\n'\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_curl, fs::Permissions::from_mode(0o755)).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.curl_path_override = Some(fake_curl);
    config.fetch_enabled = true;
    config.fetch_allow_hosts = vec!["example.com".to_string()];
    let engine = TokenZeroEngine::new(config.clone());

    let first = engine.fetch("https://example.com/doc", None, false, Mode::Auto, 4000);
    assert_eq!(first.status, "ok");
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 1);

    config.fetch_deny_hosts = vec!["example.com".to_string()];
    let denied_engine = TokenZeroEngine::new(config);
    let denied = denied_engine.fetch("https://example.com/doc", None, false, Mode::Auto, 4000);
    let error = denied.error.as_ref().unwrap();
    assert_eq!(error.code, "fetch_blocked");
    assert!(
        denied.visible.is_none(),
        "a fresh TTL cache hit must not bypass the current deny policy"
    );
    assert_eq!(
        fs::read_to_string(&marker).unwrap().lines().count(),
        1,
        "denied cached fetch must not re-enter curl"
    );
}

#[test]
fn fetch_rejects_non_http_and_reports_curl_failures() {
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.fetch_enabled = true;
    let engine = TokenZeroEngine::new(config);
    let bad = engine.fetch("file:///etc/passwd", None, false, Mode::Auto, 4000);
    assert_eq!(bad.error.as_ref().unwrap().code, "invalid_url");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let failing = dir.path().join("failing-curl");
        fs::write(
            &failing,
            "#!/bin/sh\necho 'could not resolve host' >&2\nexit 6\n",
        )
        .unwrap();
        fs::set_permissions(&failing, fs::Permissions::from_mode(0o755)).unwrap();
        let mut config = EngineConfig::for_root(dir.path());
        config.curl_path_override = Some(failing);
        config.fetch_enabled = true;
        config.fetch_allow_hosts = vec!["nope.invalid".to_string()];
        let engine = TokenZeroEngine::new(config);
        let response = engine.fetch("https://nope.invalid/x", None, false, Mode::Auto, 4000);
        let error = response.error.as_ref().unwrap();
        assert_eq!(error.code, "fetch_failed");
        assert!(
            error.message.contains("could not resolve host"),
            "{error:?}"
        );
    }
}

#[test]
fn fetch_is_disabled_by_default() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig {
        fetch_enabled: false,
        ..EngineConfig::for_root(dir.path())
    });
    let response = engine.fetch("https://example.com/", None, false, Mode::Auto, 4000);
    let error = response.error.as_ref().unwrap();
    assert_eq!(error.code, "fetch_disabled");
}

#[cfg(unix)]
#[test]
fn fetch_blocks_internal_targets_before_any_network_call() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let fake_curl = dir.path().join("fake-curl");
    let marker = dir.path().join("invocations.log");
    fs::write(
        &fake_curl,
        format!(
            "#!/bin/sh\necho invoked >> {}\nprintf 'body'\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_curl, fs::Permissions::from_mode(0o755)).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.curl_path_override = Some(fake_curl);
    config.fetch_enabled = true;
    let engine = TokenZeroEngine::new(config);

    for url in [
        "http://169.254.169.254/latest/meta-data/",
        "http://127.0.0.1:8080/admin",
        "http://10.0.0.5/",
        "http://localhost:9999/",
    ] {
        let response = engine.fetch(url, None, false, Mode::Auto, 4000);
        let error = response.error.as_ref().unwrap();
        assert_eq!(error.code, "fetch_blocked", "{url}");
    }
    assert!(
        !marker.exists(),
        "curl must never be invoked for blocked targets"
    );
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

#[cfg(unix)]
#[test]
fn shell_children_inherit_tokenzero_inner_guard() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.shell(
        "sh -c 'echo INNER=$TOKENZERO_INNER'",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        true,
        None,
        None,
        None,
    );
    assert_eq!(response.status, "ok");
    let preview = response.telemetry.as_ref().unwrap()["stdout_preview"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(preview.contains("INNER=1"), "{preview}");
}

#[cfg(unix)]
#[test]
fn shell_caller_env_overrides_inner_guard() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let mut env = BTreeMap::new();
    env.insert("TOKENZERO_INNER".to_string(), "custom".to_string());
    let response = engine.shell(
        "sh -c 'echo INNER=$TOKENZERO_INNER'",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        true,
        Some(env),
        None,
        None,
    );
    assert_eq!(response.status, "ok");
    let preview = response.telemetry.as_ref().unwrap()["stdout_preview"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(preview.contains("INNER=custom"), "{preview}");
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
fn expand_is_never_deduped() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    let content = dedup_fixture_content();
    fs::write(&file, &content).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = read_ok(&engine, &file);
    let blob_ref = response
        .refs
        .iter()
        .find(|record| record.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    for _ in 0..2 {
        let expanded = engine.expand(&blob_ref, Some("raw"), None, None, None, None);
        assert_eq!(expanded.status, "ok");
        assert_eq!(expanded.visible.unwrap().text, content);
    }
}

#[test]
fn read_and_search_schemas_advertise_fresh() {
    let specs = tool_specs();
    for name in ["tz_read", "tz_find", "tz_grep"] {
        let spec = specs.iter().find(|spec| spec.name == name).unwrap();
        assert_eq!(
            spec.input_schema["properties"]["fresh"]["type"], "boolean",
            "{name} must advertise the fresh bypass"
        );
    }
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

mod session_props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(16))]
        /// The redundancy layer never costs more than a full serve: for
        /// any old/new content pair, the second read (note, diff, or
        /// full) is never more expensive than a fresh full render of the
        /// same state.
        #[test]
        fn session_layer_never_costs_more_than_full(
            old in proptest::collection::vec("[a-z ]{0,40}", 1..30usize),
            new in proptest::collection::vec("[a-z ]{0,40}", 1..30usize),
        ) {
            let dir = tempdir().unwrap();
            let file = dir.path().join("prop.txt");
            fs::write(&file, old.join("\n") + "\n").unwrap();
            let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

            let first = engine.read(std::slice::from_ref(&file), Mode::Auto, None, None, false, 20, 4000);
            prop_assert_eq!(first.status.as_str(), "ok");

            fs::write(&file, new.join("\n") + "\n").unwrap();
            let second = engine.read(std::slice::from_ref(&file), Mode::Auto, None, None, false, 20, 4000);
            prop_assert_eq!(second.status.as_str(), "ok");

            let fresh = engine.read_with_options(
                std::slice::from_ref(&file),
                Mode::Auto,
                None,
                None,
                false,
                20,
                4000,
                ServeOptions { fresh: true },
            );
            prop_assert_eq!(fresh.status.as_str(), "ok");
            prop_assert!(
                second.accounting.as_ref().unwrap().visible_tokens
                    <= fresh.accounting.as_ref().unwrap().visible_tokens,
                "redundancy serve may never out-cost the full render"
            );
        }
    }
}

#[test]
fn mcp_tool_calls_are_pulse_accounted_with_attribution() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    fs::write(&file, "line one\nline two\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let read_request = serde_json::json!({
        "jsonrpc": "2.0", "id": 7, "method": "tools/call",
        "params": {"name": "tz_read", "arguments": {"path": file.display().to_string()}}
    });
    let read_response = handle_jsonrpc(&engine, &read_request.to_string()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&read_response).unwrap();
    let text = parsed["result"]["content"][0]["text"].as_str().unwrap();
    let ref_id = text
        .split_whitespace()
        .find(|word| word.starts_with("tz://blob/"))
        .expect("read response advertises a blob ref")
        .to_string();

    let expand_request = serde_json::json!({
        "jsonrpc": "2.0", "id": "call-8", "method": "tools/call",
        "params": {"name": "tz_expand", "arguments": {"ref": ref_id}}
    });
    handle_jsonrpc(&engine, &expand_request.to_string()).unwrap();
    let string_id_request = serde_json::json!({
        "jsonrpc": "2.0", "id": "7", "method": "tools/call",
        "params": {"name": "tz_read", "arguments": {"path": file.display().to_string(), "fresh": true}}
    });
    handle_jsonrpc(&engine, &string_id_request.to_string()).unwrap();

    let ledger = tokenzero_pulse::default_ledger_path(dir.path());
    let lines: Vec<tokenzero_pulse::PulseEvent> = fs::read_to_string(&ledger)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 3, "one event per tools/call");

    let read_event = &lines[0];
    assert_eq!(read_event.tool, "read");
    assert_eq!(read_event.session_id.as_deref(), Some(engine.session_id()));
    assert_eq!(read_event.call_id.as_deref(), Some("7"));
    assert!(read_event.ref_ids.contains(&ref_id));
    assert!(read_event.raw_tokens > 0);

    let expand_event = &lines[1];
    assert_eq!(expand_event.tool, "expand");
    assert_eq!(expand_event.call_id.as_deref(), Some("\"call-8\""));
    assert_eq!(
        expand_event.session_id.as_deref(),
        Some(engine.session_id())
    );
    assert!(
        expand_event.ref_ids.contains(&ref_id),
        "expand event must carry the expanded ref for attribution"
    );

    let string_id_event = &lines[2];
    assert_eq!(string_id_event.tool, "read");
    assert_eq!(string_id_event.call_id.as_deref(), Some("\"7\""));
    assert_ne!(
        read_event.call_id, string_id_event.call_id,
        "numeric JSON-RPC id 7 and string id \"7\" must not collide"
    );
    assert!(
        expand_event.recovery_tokens > 0,
        "recovery tokens must be charged on the MCP surface"
    );
}

#[test]
fn prune_dead_refs_drops_evicted_handles_and_reports_incomplete() {
    let mut store = RecoveryStore::new(None);
    let stored = store
        .store_payload("live payload\n", ContentType::Unknown, None, None, None)
        .unwrap();

    let mut refs = vec![
        ref_record("blob", stored.blob_ref.clone(), 13),
        ref_record("blob", "tz://blob/bdeadbeefdeadbeef".to_string(), 99),
    ];
    assert!(
        !prune_dead_refs(&store, &mut refs),
        "a dead ref must be reported"
    );
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].ref_id, stored.blob_ref);

    let mut live_only = vec![ref_record("blob", stored.blob_ref.clone(), 13)];
    assert!(prune_dead_refs(&store, &mut live_only));
    assert_eq!(live_only.len(), 1);
}

#[test]
fn response_never_advertises_a_ref_evicted_during_its_own_persist() {
    // A payload larger than the cache budget is evicted by the persist that
    // the serving call itself runs; the advertised refs must disappear with
    // it rather than dangle.
    let config = tokenzero_recovery::RecoveryConfig {
        max_bytes: 64,
        ..tokenzero_recovery::RecoveryConfig::default()
    };
    let mut store = RecoveryStore::with_config(None, config);
    let oversized = "x".repeat(4096);
    let stored = store.store_payload_deferred(&oversized, ContentType::Unknown, None, None, None);
    store.persist_pending().unwrap();

    let mut refs = vec![
        ref_record("blob", stored.blob_ref.clone(), oversized.len()),
        ref_record("file", stored.file_ref.clone(), oversized.len()),
    ];
    assert!(!prune_dead_refs(&store, &mut refs));
    assert!(
        refs.is_empty(),
        "refs evicted by the persist must not be advertised"
    );
}
