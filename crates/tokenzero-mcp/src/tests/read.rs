use super::*;
use std::sync::Arc;
use tempfile::tempdir;
use tokenzero_core::MCP_SCHEMA_VERSION;

use super::support::*;

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
