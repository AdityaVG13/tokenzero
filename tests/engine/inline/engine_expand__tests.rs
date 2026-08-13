use super::*;

#[test]
fn engine_expands_alias_persisted_after_engine_construction() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache.clone();
    config.session_dedup = false;
    let engine = TokenZeroEngine::new(config);

    let text = "payload persisted by another store view";
    let alias = {
        let mut other_process_store = RecoveryStore::new(Some(cache));
        let stored = other_process_store
            .store_payload(text, ContentType::Unknown, None, None, None)
            .unwrap();
        other_process_store.ensure_session_visible_alias(&stored.blob_ref)
    };

    let response = engine.expand(&alias, Some("raw"), None, None, None, None);
    assert!(response.error.is_none(), "{:?}", response.error);
    assert_eq!(response.visible.as_ref().unwrap().text, text);
}

fn engine_with_store(dir: &tempfile::TempDir) -> (TokenZeroEngine, RecoveryStore) {
    let cache = dir.path().join("recovery-cache.json");
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache.clone();
    config.session_dedup = false;
    let store = RecoveryStore::new(Some(cache));
    (TokenZeroEngine::new(config), store)
}

fn store_blob(store: &mut RecoveryStore, text: &str) -> String {
    store
        .store_payload(text, ContentType::Unknown, None, None, None)
        .unwrap()
        .blob_ref
}

fn expand_raw(engine: &TokenZeroEngine, ref_id: &str, raw: bool) -> ToolResponse {
    engine.expand_with_params(ExpandParams {
        ref_id: ref_id.to_string(),
        selector: None,
        start_line: None,
        end_line: None,
        anchor_kind: None,
        symbol: None,
        since: None,
        fresh: false,
        raw,
    })
}

#[test]
fn raw_expand_over_cap_fails_typed_with_fragment_repair_hint() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, mut store) = engine_with_store(&dir);
    let payload = "x".repeat(EXPAND_RAW_MAX_BYTES + 64);
    let blob = store_blob(&mut store, &payload);
    let response = expand_raw(&engine, &blob, true);
    let error = response.error.expect("over-cap raw expand must fail typed");
    assert_eq!(error.code, "expand_raw_cap_exceeded");
    assert!(
        error.repair.as_deref().unwrap_or_default().contains("#B"),
        "repair hint must point at byte fragments: {:?}",
        error.repair
    );
    // The cap gates explicit raw recovery only; a normal expand still
    // returns the body (masked-gate aside, plain payload here).
    let response = expand_raw(&engine, &blob, false);
    assert!(response.error.is_none(), "{:?}", response.error);
    assert_eq!(response.visible.as_ref().unwrap().text, payload);
}

#[test]
fn expand_raw_cap_env_parse_contract() {
    assert_eq!(expand_raw_max_bytes_from(None), EXPAND_RAW_MAX_BYTES);
    assert_eq!(expand_raw_max_bytes_from(Some("1024")), 1024);
    assert_eq!(expand_raw_max_bytes_from(Some("0")), EXPAND_RAW_MAX_BYTES);
    assert_eq!(
        expand_raw_max_bytes_from(Some("junk")),
        EXPAND_RAW_MAX_BYTES
    );
}

#[test]
fn expand_masks_unambiguous_secret_unless_raw_authorized() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, mut store) = engine_with_store(&dir);
    let secret = format!("ghp_{}", "a1B2".repeat(9));
    let text = format!("deploy token: {secret} eof");
    let blob = store_blob(&mut store, &text);

    let masked = expand_raw(&engine, &blob, false);
    assert!(masked.error.is_none(), "{:?}", masked.error);
    let body = &masked.visible.as_ref().unwrap().text;
    assert!(body.contains("[tz-masked:github-pat]"), "{body}");
    assert!(!body.contains(&secret), "secret must not leak: {body}");
    let receipt = masked.recovery.as_ref().expect("terminal receipt");
    assert!(receipt.terminal && receipt.do_not_recompact);
    assert!(!receipt.exact_bytes, "masked body is not byte-exact");
    let telemetry = masked.telemetry.expect("masking telemetry");
    assert_eq!(telemetry["secret_masking"]["masked_spans"], 1);
    assert_eq!(telemetry["secret_masking"]["stored_bytes_modified"], false);

    // raw=true is the explicit authorization: exact bytes, and proves the
    // store itself was never modified by the masked expand.
    let exact = expand_raw(&engine, &blob, true);
    assert!(exact.error.is_none(), "{:?}", exact.error);
    assert_eq!(exact.visible.as_ref().unwrap().text, text);
    let receipt = exact.recovery.as_ref().expect("terminal receipt");
    assert!(receipt.terminal && receipt.do_not_recompact && receipt.exact_bytes);
}

#[test]
fn expand_masks_pem_private_key_block() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, mut store) = engine_with_store(&dir);
    let text = "-----BEGIN PRIVATE KEY-----\nMIIEvgIBADANBg==\n-----END PRIVATE KEY-----\nafter";
    let blob = store_blob(&mut store, text);
    let masked = expand_raw(&engine, &blob, false);
    let body = &masked.visible.as_ref().unwrap().text;
    assert!(body.contains("[tz-masked:private-key-block]"), "{body}");
    assert!(!body.contains("MIIEvgIBADANBg=="), "{body}");
    assert!(body.contains("after"), "{body}");
}

#[test]
fn masking_ignores_prose_and_short_lookalikes() {
    let (out, count) = mask_expansion_secrets("ask- politely, sk-short, ghp_abc, AKIA123 done");
    assert_eq!(count, 0, "{out}");
    assert_eq!(out, "ask- politely, sk-short, ghp_abc, AKIA123 done");
}
