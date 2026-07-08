use super::*;
use std::sync::Arc;
use tempfile::tempdir;
use tokenzero_core::{MCP_SCHEMA_VERSION, Mode};

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
fn expand_repeated_serves_stay_byte_exact() {
    // Contract (recovery doctrine): an EXPLICIT expand always returns exact
    // bytes — never a dedup ack. The seen-set still records the serve for
    // the implicit read/find dedup paths and `since=` diffs, but the
    // page-fault handler must never come back empty-handed. (This test
    // previously asserted the opposite; that behavior broke the byte-exact
    // release-claim audits and cost a forced re-call with `fresh`.)
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
    let first = engine.expand(&blob_ref, Some("raw"), None, None, None, None);
    assert_eq!(first.status, "ok");
    assert_eq!(first.visible.as_ref().unwrap().text, content);
    let second = engine.expand(&blob_ref, Some("raw"), None, None, None, None);
    assert_eq!(second.status, "ok");
    assert_eq!(second.visible.as_ref().unwrap().text, content);
}

#[test]
fn expand_fresh_bypasses_seen_set() {
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
    engine.expand(&blob_ref, Some("raw"), None, None, None, None);
    let params = crate::expand_params::ExpandParams {
        ref_id: blob_ref.clone(),
        selector: Some("raw".to_string()),
        fresh: true,
        ..Default::default()
    };
    let again = engine.expand_with_params(params);
    assert_eq!(again.visible.as_ref().unwrap().text, content);
}

#[test]
fn expand_since_unchanged_and_diff() {
    use tokenzero_core::ContentType;
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let base = "alpha\nbeta\n";
    let since_ref = engine
        .ingest(base, ContentType::Unknown, Mode::Exact, "test-since")
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let changed = "alpha\nBETA\n";
    let target_ref = engine
        .ingest(changed, ContentType::Unknown, Mode::Exact, "test-target")
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let unchanged = engine.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: since_ref.clone(),
        since: Some(since_ref.clone()),
        ..Default::default()
    });
    assert_eq!(unchanged.status, "ok");
    assert!(
        unchanged
            .visible
            .as_ref()
            .unwrap()
            .text
            .contains("unchanged since")
    );
    let diffed = engine.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: target_ref,
        since: Some(since_ref),
        ..Default::default()
    });
    assert_eq!(diffed.status, "ok");
    let text = &diffed.visible.as_ref().unwrap().text;
    assert!(text.contains("diff since"));
    assert!(text.contains("-beta") || text.contains("+BETA"));
}

#[test]
fn expand_fresh_with_since_returns_full_content() {
    use tokenzero_core::ContentType;
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let base_ref = engine
        .ingest(
            "alpha\nbeta\n",
            ContentType::Unknown,
            Mode::Exact,
            "test-since",
        )
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let target_ref = engine
        .ingest(
            "alpha\nBETA\n",
            ContentType::Unknown,
            Mode::Exact,
            "test-target",
        )
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();

    let response = engine.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: target_ref,
        since: Some(base_ref),
        fresh: true,
        ..Default::default()
    });

    assert_eq!(response.status, "ok");
    assert_eq!(response.visible.as_ref().unwrap().text, "alpha\nBETA\n");
}

#[test]
fn expand_since_bad_ref_errors() {
    use tokenzero_core::ContentType;
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let target = engine
        .ingest("x", ContentType::Unknown, Mode::Exact, "test")
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let resp = engine.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: target,
        since: Some("tz://blob/deadbeefdeadbeef".to_string()),
        ..Default::default()
    });
    assert_eq!(resp.status, "error");
}

#[test]
fn codemode_expand_passes_symbol() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn alpha() {}\nfn beta() {}\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let read = read_ok(&engine, &file);
    let blob_ref = read
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let resp = engine.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: blob_ref,
        symbol: Some("alpha".to_string()),
        ..Default::default()
    });
    assert_eq!(resp.status, "ok");
    assert!(resp.visible.as_ref().unwrap().text.contains("alpha"));
}

#[test]
fn codemode_expand_many_mixed_windows() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("lines.txt");
    fs::write(&file, "one\ntwo\nthree\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let read = read_ok(&engine, &file);
    let blob_ref = read
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let r1 = engine.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: blob_ref.clone(),
        start_line: Some(1),
        end_line: Some(1),
        ..Default::default()
    });
    let r2 = engine.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: blob_ref,
        start_line: Some(2),
        end_line: Some(2),
        ..Default::default()
    });
    assert!(r1.visible.as_ref().unwrap().text.contains("one"));
    assert!(r2.visible.as_ref().unwrap().text.contains("two"));
}
#[test]
fn expand_dedup_off_serves_full_twice() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    let content = dedup_fixture_content();
    fs::write(&file, &content).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.session_dedup = false;
    let engine = TokenZeroEngine::new(config);
    let response = read_ok(&engine, &file);
    let blob_ref = response
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    for _ in 0..2 {
        let expanded = engine.expand(&blob_ref, Some("raw"), None, None, None, None);
        assert_eq!(expanded.status, "ok");
        assert_eq!(expanded.visible.as_ref().unwrap().text, content);
    }
}
#[test]
fn expand_changed_content_serves_full() {
    use tokenzero_core::ContentType;
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let v1 = engine
        .ingest("v1\n", ContentType::Unknown, Mode::Exact, "t")
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    engine.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: v1.clone(),
        ..Default::default()
    });
    let v2 = engine
        .ingest("v2\n", ContentType::Unknown, Mode::Exact, "t2")
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let second = engine.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: v2,
        ..Default::default()
    });
    assert_eq!(second.visible.as_ref().unwrap().text, "v2\n");
}

#[test]
fn expand_across_engine_respawn_stays_byte_exact() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    let content = dedup_fixture_content();
    fs::write(&file, &content).unwrap();
    let config = EngineConfig::for_root(dir.path());
    let blob_ref = {
        let engine = TokenZeroEngine::new(config.clone());
        let response = read_ok(&engine, &file);
        let blob_ref = response
            .refs
            .iter()
            .find(|record| record.kind == "blob")
            .unwrap()
            .ref_id
            .clone();
        let first = engine.expand(&blob_ref, Some("raw"), None, None, None, None);
        assert_eq!(first.visible.as_ref().unwrap().text, content);
        let second = engine.expand(&blob_ref, Some("raw"), None, None, None, None);
        // Explicit expand always returns bytes (recovery contract).
        assert_eq!(second.visible.as_ref().unwrap().text, content);
        blob_ref
    };
    // A fresh engine on the same store must also recover exact bytes: the
    // persisted seen-set informs read/find dedup, never expand delivery.
    let engine2 = TokenZeroEngine::new(config);
    let third = engine2.expand(&blob_ref, Some("raw"), None, None, None, None);
    assert_eq!(third.status, "ok");
    assert_eq!(third.visible.as_ref().unwrap().text, content);
}

#[test]
fn expand_stale_persisted_sha_serves_full_after_payload_mutation() {
    use tokenzero_core::ContentType;
    let dir = tempdir().unwrap();
    let config = EngineConfig::for_root(dir.path());
    let cache_path = config.cache_path.clone();
    let (blob_ref, _engine) = {
        let engine = TokenZeroEngine::new(config.clone());
        let v1 = engine
            .ingest("version_one\n", ContentType::Unknown, Mode::Exact, "t")
            .refs
            .iter()
            .find(|r| r.kind == "blob")
            .unwrap()
            .ref_id
            .clone();
        engine.expand_with_params(crate::expand_params::ExpandParams {
            ref_id: v1.clone(),
            ..Default::default()
        });
        (v1, ())
    };
    let text = fs::read_to_string(&cache_path).unwrap();
    let mut state: serde_json::Value = serde_json::from_str(&text).unwrap();
    let blobs = state
        .get_mut("blobs")
        .and_then(|v| v.as_object_mut())
        .unwrap();
    let entry = blobs
        .get_mut(&blob_ref)
        .expect("blob entry in recovery cache");
    assert_eq!(entry.as_str().unwrap(), "version_one\n");
    *entry = serde_json::Value::String("version_two\n".to_string());
    fs::write(&cache_path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    // The persisted state is snapshot ⊕ journal since the journaled-persist
    // change; drop the journal so the hand-mutated snapshot IS the effective
    // state (otherwise replay restores the original payload and defeats the
    // mutation this test simulates).
    let journal = {
        let mut os = cache_path.clone().into_os_string();
        os.push(".journal");
        std::path::PathBuf::from(os)
    };
    let _ = fs::remove_file(journal);

    let engine2 = TokenZeroEngine::new(config);
    let resp = engine2.expand_with_params(crate::expand_params::ExpandParams {
        ref_id: blob_ref,
        ..Default::default()
    });
    assert_eq!(resp.visible.as_ref().unwrap().text, "version_two\n");
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
    let response = read_ok(&engine, &file);
    let blob_ref = response
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    engine.expand(&blob_ref, Some("raw"), None, None, None, None);
    engine.expand(&blob_ref, Some("raw"), None, None, None, None);
    assert!(
        !memory_path.exists(),
        "dedup off must not create {}",
        memory_path.display()
    );
}

#[test]
fn expand_cross_scheme_fz_and_gz_blob_byte_exact() {
    // wqw.1: engine expand accepts fz:// and gz:// with shared blob identity.
    let dir = tempdir().unwrap();
    let file = dir.path().join("cross.txt");
    let content = "engine cross-scheme body\nsecond line\n";
    fs::write(&file, content).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let response = read_ok(&engine, &file);
    let blob_ref = response
        .refs
        .iter()
        .find(|record| record.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let id = blob_ref.strip_prefix("tz://blob/").unwrap();
    for scheme_ref in [
        blob_ref.clone(),
        format!("fz://blob/{id}"),
        format!("gz://blob/{id}"),
    ] {
        let expanded = engine.expand(&scheme_ref, Some("raw"), None, None, None, None);
        assert_eq!(expanded.status, "ok", "{scheme_ref}: {:?}", expanded.error);
        assert_eq!(expanded.visible.as_ref().unwrap().text, content);
    }
}

#[test]
fn expand_garbage_scheme_error_keeps_full_ref() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let long = "xx://blob/bdeadbeefcafebabe0123456789abcdef_long_hash_tail_for_truncation_check";
    let response = engine.expand(long, Some("raw"), None, None, None, None);
    assert_eq!(response.status, "error");
    let err = response.error.as_ref().expect("error payload");
    assert_eq!(err.code, "invalid_ref");
    assert!(
        err.message.contains(long),
        "error must include full ref (no mid-hash truncation): {}",
        err.message
    );
}

#[test]
fn expand_missing_fz_blob_error_includes_full_ref() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let missing = "fz://blob/b0123456789abcdef";
    let response = engine.expand(missing, Some("raw"), None, None, None, None);
    assert_eq!(response.status, "error");
    let err = response.error.as_ref().expect("error payload");
    assert!(
        err.message.contains(missing),
        "missing-ref error must name full ref: {}",
        err.message
    );
}
