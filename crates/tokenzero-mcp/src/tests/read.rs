use super::*;
use super::support::*;

#[test]
fn read_expand_roundtrip_via_engine() {
    let (_dir, file, engine) = setup_file("a.txt", "alpha\nbeta\n");
    let response = engine.read(&[file], Mode::Hybrid, None, None, false, 20, 4000);
    assert_status_ok(&response);
    assert_eq!(expand_ok(&engine, &blob_ref(&response)), "alpha\nbeta\n");
}

#[test]
fn read_reports_detected_content_type() {
    let (_dir, file, engine) = setup_file("state.md", "# Session\n\nmarkdown body\n");
    let response = engine.read(&[file], Mode::Auto, None, None, false, 20, 4000);
    assert_status_ok(&response);
    assert_eq!(response.content_type.as_deref(), Some("markdown"));
}

#[test]
fn read_line_range_only_stores_requested_slice() {
    let (_dir, file, engine) = setup_file("large.log", format!("one\n{}\nthree\n", "x".repeat(10_000)));
    let response = engine.read(&[file], Mode::Auto, Some(1), Some(1), true, 20, 4000);
    assert_status_ok(&response);
    assert_eq!(visible_text(&response), "one");
    assert_eq!(
        response.refs.iter().find(|r| r.kind == "blob").unwrap().bytes,
        3
    );
}

#[test]
fn missing_read_inside_allowed_root_reports_read_failed() {
    let (dir, engine) = setup_default();
    let response = engine.read(&[dir.path().join("missing.md")], Mode::Auto, None, None, false, 20, 4000);
    assert_error_code(&response, "read_failed");
}

#[test]
fn read_and_ingest_degrade_when_recovery_cache_is_unwritable() {
    let (_dir, file, engine) = setup_unwritable("sample.rs", "fn alpha() {}\n");
    for (label, response) in [
        (
            "read",
            engine.read(&[file.clone()], Mode::Auto, None, None, false, 20, 4000),
        ),
        (
            "ingest",
            engine.ingest("alpha\nbeta\n", ContentType::Logs, Mode::Auto, "logs"),
        ),
    ] {
        assert_status_ok(&response);
        assert_eq!(
            response.diagnostic.as_ref().unwrap().code,
            "cache_write_failed",
            "{label}"
        );
        assert!(visible_text(&response).contains("alpha"), "{label}");
        assert!(response.refs.is_empty(), "{label}");
    }
}

#[test]
fn empty_and_range_zero_payload_notes() {
    let (dir, engine) = setup_default();
    let empty = dir.path().join("empty.txt");
    fs::write(&empty, "").unwrap();
    let short = dir.path().join("short.txt");
    fs::write(&short, "one\ntwo\n").unwrap();

    let cases: &[(&str, ToolResponse, &str)] = &[
        ("empty_note", engine.read(&[empty.clone()], Mode::Auto, None, None, false, 20, 4000), "note"),
        ("past_eof", engine.read(&[short], Mode::Auto, Some(10), Some(12), false, 20, 4000), "note"),
        ("raw_empty", engine.read(&[empty.clone()], Mode::Auto, None, None, true, 20, 4000), "raw"),
        ("passthrough_empty", engine.read(&[empty.clone()], Mode::Passthrough, None, None, false, 20, 4000), "raw"),
        ("ingest_empty", engine.ingest("", ContentType::Logs, Mode::Auto, "logs"), "ingest"),
    ];
    for (label, response, kind) in cases {
        assert_status_ok(response);
        let text = visible_text(response);
        match *kind {
            "note" => {
                assert!(text.starts_with("# read "), "{label}: {text}");
                assert!(text.ends_with("— 0 bytes"), "{label}: {text}");
            }
            "raw" => assert_eq!(text, "", "{label}"),
            "ingest" => {
                assert_eq!(text, "# ingest — 0 bytes");
                assert_eq!(
                    response.accounting.as_ref().unwrap().visible_tokens,
                    count_tokens("# ingest — 0 bytes")
                );
            }
            _ => unreachable!(),
        }
    }
    // Empty-file note still stores exact empty payload.
    let noted = engine.read(&[empty], Mode::Auto, None, None, false, 20, 4000);
    assert_eq!(
        noted.accounting.as_ref().unwrap().visible_tokens,
        count_tokens(&visible_text(&noted))
    );
    assert_eq!(expand_ok(&engine, &blob_ref(&noted)), "");
}

#[test]
fn second_identical_read_collapses_to_unchanged_note() {
    let (_dir, file, engine, _) = setup_dedup("sample.rs");
    let first = read_ok(&engine, &file);
    assert!(visible_text(&first).contains("line 01"));
    let second = read_ok(&engine, &file);
    let note = visible_text(&second);
    assert!(note.starts_with("unchanged: tz://file/"), "{note}");
    assert!(note.contains("(served earlier this session)"), "{note}");
    assert!(note.contains("— 40 lines"), "{note}");
    assert!(note.contains("full bytes: expand tz://blob/"), "{note}");
    assert!(visible_tokens(&second) < visible_tokens(&first));
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
fn raw_and_passthrough_bypass_note() {
    let (_dir, file, engine, content) = setup_dedup("sample.rs");
    read_ok(&engine, &file);
    let raw = engine.read(std::slice::from_ref(&file), Mode::Auto, None, None, true, 20, 4000);
    assert_status_ok(&raw);
    assert_eq!(visible_text(&raw), content.trim_end());
    assert!(!visible_text(&raw).contains("unchanged:"));
    assert!(visible_text(&read_ok(&engine, &file)).starts_with("unchanged:"));

    let (_dir2, file2, engine2, _) = setup_dedup("sample.rs");
    read_ok(&engine2, &file2);
    let passthrough = engine2.read(std::slice::from_ref(&file2), Mode::Passthrough, None, None, false, 20, 4000);
    assert_status_ok(&passthrough);
    assert!(visible_text(&passthrough).contains("line 01"));
    assert!(!visible_text(&passthrough).contains("unchanged:"));
}

#[test]
fn diff_then_unchanged_read_notes() {
    let (_dir, file, engine, _) = setup_dedup("sample.rs");
    read_ok(&engine, &file);
    fs::write(&file, dedup_fixture_content().replace("line 20:", "line 20 (changed):")).unwrap();
    assert!(visible_text(&read_ok(&engine, &file)).contains("changed since served this session"));
    assert!(visible_text(&read_ok(&engine, &file)).starts_with("unchanged:"));
}

#[test]
fn multi_file_read_mixes_strategies() {
    let (dir, engine) = setup_default();
    let stable = dir.path().join("stable.rs");
    let moving = dir.path().join("moving.rs");
    fs::write(&stable, dedup_fixture_content()).unwrap();
    fs::write(&moving, dedup_fixture_content()).unwrap();
    let both = vec![stable, moving.clone()];
    assert_status_ok(&engine.read(&both, Mode::Auto, None, None, false, 20, 4000));
    fs::write(&moving, dedup_fixture_content().replace("line 20:", "line 20 (changed):")).unwrap();
    let second = engine.read(&both, Mode::Auto, None, None, false, 20, 4000);
    assert_status_ok(&second);
    let text = visible_text(&second);
    assert!(text.contains("unchanged: tz://file/"), "{text}");
    assert!(text.contains("changed since served this session"), "{text}");
    let telemetry = second.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["output_strategy"], "seen_set_dedup+diff_since_served");
    assert!(telemetry["dedup"].is_object());
    assert!(telemetry["diff"].is_object());
}

#[test]
fn read_and_search_schemas_advertise_fresh() {
    let specs = tool_specs();
    for name in ["tz_read", "tz_find", "tz_grep"] {
        let spec = specs.iter().find(|spec| spec.name == name).unwrap();
        assert_eq!(
            spec.input_schema["properties"]["fresh"]["type"],
            "boolean",
            "{name} must advertise the fresh bypass"
        );
    }
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
    assert!(!prune_dead_refs(&store, &mut refs));
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].ref_id, stored.blob_ref);
    let mut live_only = vec![ref_record("blob", stored.blob_ref.clone(), 13)];
    assert!(prune_dead_refs(&store, &mut live_only));
    assert_eq!(live_only.len(), 1);
}
