use super::*;
use tempfile::tempdir;
use tokenzero_core::Mode;

use tokenzero_core::ContentType;
use tokenzero_recovery::RecoveryStore;

use super::support::*;
struct RefIndexDisabledOverrideGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl RefIndexDisabledOverrideGuard {
    fn new() -> Self {
        let lock = super::REF_INDEX_OVERRIDE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        tokenzero_recovery::set_ref_index_disabled_override(true);
        Self { _lock: lock }
    }
}

impl Drop for RefIndexDisabledOverrideGuard {
    fn drop(&mut self) {
        tokenzero_recovery::set_ref_index_disabled_override(false);
    }
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
fn recall_unreadable_cache_degrades_cleanly() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache.json");
    fs::write(&cache, "{broken").unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache;
    let engine = TokenZeroEngine::new(config);

    let response = engine.recall("x", 10, Mode::Auto, 4000);

    assert_eq!(response.status, "ok");
    assert!(
        response.error.is_none(),
        "unreadable cache must recover or degrade without surfacing a tool error"
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
fn expand_terminal_receipt_and_secret_gate() {
    // yevj acceptance on the standalone MCP adapter surface: successful
    // expands carry the ToolResponse recovery receipt (terminal +
    // do-not-recompact); default expands mask unambiguous credentials while
    // `raw: true` is the explicit authorization returning exact bytes.
    let dir = tempdir().unwrap();
    let file = dir.path().join("deploy.txt");
    let secret = format!("ghp_{}", "a1B2".repeat(9));
    let content = format!("deploy token = {secret}\ntrailer\n");
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

    let masked = engine.expand_with_params(tokenzero_engine::expand_params::ExpandParams {
        ref_id: blob_ref.clone(),
        ..Default::default()
    });
    assert_eq!(masked.status, "ok");
    let body = &masked.visible.as_ref().unwrap().text;
    assert!(body.contains("[tz-masked:github-pat]"), "{body}");
    assert!(!body.contains(&secret), "secret leaked: {body}");
    let receipt = masked.recovery.expect("terminal receipt on expand");
    assert!(receipt.terminal && receipt.do_not_recompact);
    assert!(!receipt.exact_bytes, "masked body is not byte-exact");

    let exact = engine.expand_with_params(tokenzero_engine::expand_params::ExpandParams {
        ref_id: blob_ref,
        raw: true,
        ..Default::default()
    });
    assert_eq!(exact.status, "ok");
    assert_eq!(exact.visible.as_ref().unwrap().text, content);
    let receipt = exact.recovery.expect("terminal receipt on raw expand");
    assert!(receipt.terminal && receipt.do_not_recompact && receipt.exact_bytes);
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
    let _override_lock = super::REF_INDEX_OVERRIDE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    // Keep this payload distinct from adjacent parallel expand tests. Shared
    // ref-index entries must not point at another test's short-lived store.
    let base = "since-diff-alpha\nbeta\n";
    let since_ref = engine
        .ingest(base, ContentType::Unknown, Mode::Exact, "test-since")
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let changed = "since-diff-alpha\nBETA\n";
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
fn expand_b_fragment_returns_exact_byte_window() {
    // cqr.1: #B fragment must not return the full payload; must return unsupported_fragment.
    let dir = tempdir().unwrap();
    let file = dir.path().join("bfrag.txt");
    let content = "byte-range test\nline two\n";
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
    let b_ref = format!("{blob_ref}#B0-1");
    let expanded = engine.expand(&b_ref, Some("raw"), None, None, None, None);
    assert_eq!(expanded.status, "ok");
    assert_eq!(expanded.visible.as_ref().unwrap().text, &content[0..1]);
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
fn expand_stale_persisted_sha_rejects_payload_mutation() {
    use tokenzero_core::ContentType;
    let _disabled_override = RefIndexDisabledOverrideGuard::new();
    // This contract targets the LOCAL snapshot: with the pay-once user CAS
    // attached, blobs publish there and the snapshot never holds the bytes.
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
    assert_eq!(resp.status, "error", "{:?}", resp.error);
    assert!(
        resp.visible.is_none(),
        "hash-mismatched portable refs must not expose mutated bytes"
    );
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
fn expand_same_store_scheme_alias_fz_gz_byte_exact() {
    // cqr.1: engine expand accepts fz:// and gz:// as same-store scheme aliases.
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
    assert!(
        err.message
            .starts_with(&format!("-{missing} (unavailable)")),
        "missing recoverable refs must emit a tombstone line: {}",
        err.message
    );
}

#[test]
fn same_session_codemode_default_store_expands_without_rerun() {
    // wqw.8: mint via codemode engine (shared default store) then expand on a
    // fresh engine pointed at the same cache_path — no re-run of producer.
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let cache = crate::workspace::default_recovery_cache_path(&root);
    let mint_engine = {
        let mut config = EngineConfig::for_root(&root);
        config.cache_path = cache.clone();
        TokenZeroEngine::new(config)
    };
    let mint = mint_engine.ingest(
        "same-session payload hello",
        tokenzero_core::ContentType::Unknown,
        Mode::Auto,
        "test",
    );
    assert_eq!(mint.status, "ok", "{:?}", mint.error);
    let blob_ref = mint
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .expect("blob ref")
        .ref_id
        .clone();
    drop(mint_engine);

    let expand_engine = {
        let mut config = EngineConfig::for_root(&root);
        config.cache_path = cache;
        TokenZeroEngine::new(config)
    };
    let expanded = expand_engine.expand(&blob_ref, Some("raw"), None, None, None, None);
    assert_eq!(expanded.status, "ok", "{:?}", expanded.error);
    assert_eq!(
        expanded.visible.as_ref().unwrap().text,
        "same-session payload hello"
    );
}

#[test]
fn wrong_cache_path_recovers_via_internal_sibling_retry() {
    // Replaced store_mismatch agent lecture with engine-internal sibling expand.
    let dir = tempdir().unwrap();
    let root = dir.path();
    let consumer_cache = root.join("recovery-cache.json");
    let sibling = root.join("codemode-recovery.json");
    let payload = "mismatch-payload";
    let blob_id = tokenzero_core::id_for('b', payload);
    let blob_ref = format!("tz://blob/{blob_id}");
    let producer_state = serde_json::json!({
        "version": 1,
        "max_blobs": 1024,
        "max_files": 1024,
        "max_units": 1024,
        "max_search_hits": 1024,
        "max_bytes": 64 * 1024 * 1024,
        "blobs": { blob_ref.clone(): payload },
        "files": {},
        "units": {},
        "search_hits": {},
        "aliases": {},
        "order": [blob_ref.clone()],
        "shell_outcomes": {},
        "shell_outcome_seq": 0
    });
    fs::write(
        &sibling,
        serde_json::to_vec_pretty(&producer_state).unwrap(),
    )
    .unwrap();
    // Empty consumer store file (exists so probe sees both paths as files).
    fs::write(
        &consumer_cache,
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "max_blobs": 1024,
            "max_files": 1024,
            "max_units": 1024,
            "max_search_hits": 1024,
            "max_bytes": 64 * 1024 * 1024,
            "blobs": {},
            "files": {},
            "units": {},
            "search_hits": {},
            "aliases": {},
            "order": [],
            "shell_outcomes": {},
            "shell_outcome_seq": 0
        }))
        .unwrap(),
    )
    .unwrap();

    let mut consumer = EngineConfig::for_root(root);
    consumer.cache_path = consumer_cache.clone();
    let expand_engine = TokenZeroEngine::new(consumer);
    let response = expand_engine.expand(&blob_ref, Some("raw"), None, None, None, None);

    assert_eq!(response.status, "ok", "{:?}", response.error);
    assert_eq!(
        response.visible.as_ref().map(|v| v.text.as_str()),
        Some(payload)
    );
}

#[test]
fn windowed_expand_same_store_and_oob_code() {
    // zq9: same-store window is exact; OOB is window_out_of_range not ref_not_found.
    let dir = tempdir().unwrap();
    let file = dir.path().join("lines.txt");
    let mut body = String::new();
    for i in 1..=200 {
        body.push_str(&format!("line-{i}\n"));
    }
    fs::write(&file, &body).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let response = read_ok(&engine, &file);
    let blob_ref = response
        .refs
        .iter()
        .find(|record| record.kind == "blob")
        .unwrap()
        .ref_id
        .clone();

    let window = engine.expand(&blob_ref, Some("raw"), Some(120), Some(190), None, None);
    assert_eq!(window.status, "ok", "{:?}", window.error);
    let text = window.visible.as_ref().unwrap().text.clone();
    assert!(text.starts_with("line-120\n"), "{text}");
    assert!(text.contains("line-190\n"), "{text}");
    assert!(!text.contains("line-119\n"));
    let win_tokens = window.accounting.as_ref().unwrap().visible_tokens;
    let full = engine.expand(&blob_ref, Some("raw"), None, None, None, None);
    let full_tokens = full.accounting.as_ref().unwrap().visible_tokens;
    assert!(
        win_tokens < full_tokens / 2,
        "window tokens {win_tokens} vs full {full_tokens}"
    );

    let oob = engine.expand(&blob_ref, Some("raw"), Some(500), Some(510), None, None);
    assert_eq!(oob.status, "error");
    let err = oob.error.as_ref().unwrap();
    assert_eq!(
        err.code, "window_out_of_range",
        "OOB must not be ref_not_found: {:?}",
        err
    );
    assert!(err.message.contains(&blob_ref), "{}", err.message);
}

#[test]
fn codemode_tools_list_excludes_perop_even_when_unhealthy() {
    use tokenzero_core::McpToolSurface;
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.tool_surface = McpToolSurface::CodeMode;
    let engine = TokenZeroEngine::new(config);
    engine.mark_lifecycle_ready_for_tests();
    engine.surface_health().record_codemode_expand_x0();
    assert!(!engine.surface_health().is_healthy());

    let listed = handle_jsonrpc(
        &engine,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {}
        })
        .to_string(),
    )
    .unwrap();
    let parsed: Value = serde_json::from_str(&listed).unwrap();
    let names: Vec<&str> = parsed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        names.contains(&"tz_execute_code") && names.contains(&"tz_codemode_search"),
        "primary tools must stay listed: {names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|n| *n == "tz_expand" || *n == "expand" || *n == "tz_read" || *n == "tz_shell"),
        "per-op / recovery must stay hidden for whole CodeMode session: {names:?}"
    );
}

#[test]
fn crash_only_expand_unknown_when_codemode_healthy() {
    use tokenzero_core::McpToolSurface;
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.tool_surface = McpToolSurface::CodeMode;
    let engine = TokenZeroEngine::new(config);
    engine.mark_lifecycle_ready_for_tests();
    assert!(engine.surface_health().is_healthy());
    assert!(engine.surface_health().primary_surface_healthy_claim());

    let blocked = handle_jsonrpc(
        &engine,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "tz_expand", "arguments": {"ref": "tz://blob/dead"}}
        })
        .to_string(),
    )
    .unwrap();
    let parsed: Value = serde_json::from_str(&blocked).unwrap();
    let data = &parsed["error"]["data"];
    assert_eq!(
        data["kind"], "unknown_tool",
        "per-op must be unknown_tool not policy lecture: {parsed}"
    );
    // Write/shell likewise unknown — not a policy lecture.
    let shell_blocked = handle_jsonrpc(
        &engine,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "tz_shell", "arguments": {"command": "true"}}
        })
        .to_string(),
    )
    .unwrap();
    let shell_parsed: Value = serde_json::from_str(&shell_blocked).unwrap();
    assert_eq!(
        shell_parsed["error"]["data"]["kind"], "unknown_tool",
        "{shell_parsed}"
    );
}

#[test]
fn crash_only_expand_stays_hidden_after_expand_x0() {
    use tokenzero_core::McpToolSurface;
    let dir = tempdir().unwrap();
    let file = dir.path().join("payload.txt");
    fs::write(&file, "recovery-ladder-bytes-wqw9\n").unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.tool_surface = McpToolSurface::CodeMode;
    let cache = dir.path().join(".tokenzero/recovery-cache.json");
    config.cache_path = cache.clone();
    let engine = TokenZeroEngine::new(config);

    let mut seed_cfg = EngineConfig::for_root(dir.path());
    seed_cfg.cache_path = cache;
    let seed = TokenZeroEngine::new(seed_cfg);
    let served = read_ok(&seed, &file);
    let blob_ref = served
        .refs
        .iter()
        .find(|r| r.kind == "blob" || r.kind == "file")
        .expect("read emits a recovery ref")
        .ref_id
        .clone();

    engine.mark_lifecycle_ready_for_tests();
    engine.surface_health().record_codemode_expand_x0();
    assert!(!engine.surface_health().is_healthy());
    assert!(!engine.surface_health().primary_surface_healthy_claim());

    // MCP tz_expand stays unknown even after X0 — fallback is engine-internal.
    let mcp = handle_jsonrpc(
        &engine,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "tz_expand",
                "arguments": {"ref": blob_ref.clone(), "selector": "raw"}
            }
        })
        .to_string(),
    )
    .unwrap();
    let parsed: Value = serde_json::from_str(&mcp).unwrap();
    assert_eq!(
        parsed["error"]["data"]["kind"], "unknown_tool",
        "tz_expand must stay off the agent surface: {parsed}"
    );

    // Engine-direct expand still recovers bytes (router / CodeMode path).
    let recovered = engine.expand(&blob_ref, Some("raw"), None, None, None, None);
    assert_eq!(recovered.status, "ok", "{:?}", recovered.error);
    assert!(
        recovered
            .visible
            .as_ref()
            .is_some_and(|v| v.text.contains("recovery-ladder-bytes-wqw9")),
        "must recover exact bytes via engine expand: {recovered:?}"
    );
}

#[test]
fn crash_only_healthy_claim_false_after_ref_not_found_surface_error() {
    use tokenzero_core::McpToolSurface;
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.tool_surface = McpToolSurface::CodeMode;
    let engine = TokenZeroEngine::new(config);
    // Direct expand of missing blob records surface failure (not invalid_ref).
    let miss = engine.expand(
        "tz://blob/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        Some("raw"),
        None,
        None,
        None,
        None,
    );
    assert_eq!(miss.status, "error");
    assert!(!engine.surface_health().primary_surface_healthy_claim());
    assert_eq!(
        engine
            .surface_health()
            .decide(McpToolSurface::CodeMode, "expand"),
        crate::surface_health::CrashOnlyDecision::Unlocked
    );
}

#[test]
fn expand_fz_blob_ref_from_sibling_fszero_store() {
    // Engine-level regression for fszero-fz-ref-expand-broken-izj: a blob ref
    // minted by the fszero engine and stored only in its JSON store must expand
    // through the TokenZeroEngine when both engines share a unified root.
    let dir = tempdir().unwrap();
    let root = dir.path().join(".zerostack");
    let fszero_cache = root.join("fszero").join("recovery-cache.json");
    let tokenzero_cache = root.join("tokenzero").join("recovery-cache.json");

    let payload = "fszero engine payload
line two
";
    let fz_ref = format!("fz://blob/{}", tokenzero_core::sha256_hex(payload));

    // Store via a flat path so the payload stays in the JSON store, then move
    // the snapshot into the unified fszero layout.
    let fszero_temp = dir.path().join("fszero-cache.json");
    let mut fszero_store = RecoveryStore::new(Some(fszero_temp.clone()));
    fszero_store
        .store_payload(payload, ContentType::Unknown, None, None, None)
        .unwrap();
    fs::create_dir_all(fszero_cache.parent().unwrap()).unwrap();
    fs::rename(&fszero_temp, &fszero_cache).unwrap();

    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = tokenzero_cache;
    config.allowed_roots = vec![dir.path().to_path_buf()];
    let engine = TokenZeroEngine::new(config);

    let response = engine.expand(&fz_ref, Some("raw"), None, None, None, None);
    assert_eq!(response.status, "ok", "{:?}", response.error);
    assert_eq!(response.visible.as_ref().unwrap().text, payload);
}
