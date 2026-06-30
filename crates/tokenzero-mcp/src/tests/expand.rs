use super::*;
use std::sync::Arc;
use tempfile::tempdir;
use tokenzero_core::MCP_SCHEMA_VERSION;

use super::support::*;

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
