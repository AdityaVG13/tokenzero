use super::*;
use crate::TokenZeroEngine;
use crate::config::EngineConfig;
use crate::action_cache_key::{ActionCacheKeyInput, ConsistencyClass, action_cache_key};
use serde_json::json;
use tempfile::tempdir;
use tokenzero_core::{Accounting, Mode, ToolResponse};
use tokenzero_recovery::{
    EvictionSlackGuard, aggregate_cachezero, store_root_from_cache_path,
};

fn engine_at(dir: &std::path::Path) -> TokenZeroEngine {
    let cache = dir.join("tokenzero").join("recovery-cache.json");
    let _ = std::fs::create_dir_all(cache.parent().unwrap());
    let mut config = EngineConfig::for_root(dir);
    config.cache_path = cache;
    config.allowed_roots = vec![dir.to_path_buf()];
    TokenZeroEngine::new(config)
}

fn ok_read(text: &str) -> ToolResponse {
    ToolResponse::ok(
        "read",
        Mode::Auto,
        text.to_string(),
        Vec::new(),
        Accounting {
            raw_tokens: 10,
            visible_tokens: 4,
            recovery_tokens: 0,
            billed_tokens: 4,
            cached_tokens: 0,
            exact_ref_tokens: None,
        },
    )
}

#[test]
fn tz0zjn_shadow_logs_without_replacing_body() {
    let dir = tempdir().unwrap();
    let engine = engine_at(dir.path());
    let args = json!({"path": "README.md"});
    let first_body = "hello-shadow";
    let mut first = ok_read(first_body);
    observe_with_mode(
        &engine,
        "read",
        &args,
        2_000_000,
        &mut first,
        CachezeroMode::Shadow,
    );
    assert_eq!(first.cache_status.as_deref(), Some("forced-miss"));
    assert_eq!(first.saved_tokens_estimate, Some(0));
    assert_eq!(first.visible.as_ref().unwrap().text, first_body);

    let mut second = ok_read(first_body);
    observe_with_mode(
        &engine,
        "read",
        &args,
        1_000_000,
        &mut second,
        CachezeroMode::Shadow,
    );
    assert_eq!(second.cache_status.as_deref(), Some("exact-hit"));
    assert_eq!(second.saved_tokens_estimate, Some(4));
    assert_eq!(
        second.visible.as_ref().unwrap().text,
        first_body,
        "shadow must not serve a cached body"
    );

    let stats =
        aggregate_cachezero(&store_root_from_cache_path(&engine.config.cache_path)).unwrap();
    assert_eq!(stats.decisions, 2);
    assert_eq!(stats.would_have_hits, 1);
    assert!(!stats.graduation);
}

#[test]
fn tz0zjn_cross_world_tenancy_denies_resolution() {
    let dir = tempdir().unwrap();
    let engine_a = engine_at(dir.path());
    let engine_b = engine_at(dir.path());
    let args = json!({"path": "README.md"});
    let body = "tenancy-body";
    let store_root = store_root_from_cache_path(&engine_a.config.cache_path);
    let key = action_cache_key(ActionCacheKeyInput {
        op: "read",
        args: &args,
        store_root: &store_root,
        model_id: None,
        consistency_class: Some(ConsistencyClass::parse(None)),
    });

    let mut first = ok_read(body);
    observe_with_mode(
        &engine_a,
        "read",
        &args,
        1_000_000,
        &mut first,
        CachezeroMode::Shadow,
    );
    assert_eq!(first.cache_status.as_deref(), Some("forced-miss"));

    // The write-through is attributed to the writing engine's session world.
    let index = tokenzero_recovery::ActionCacheIndex::open(&store_root);
    let written = index.get(&key).unwrap().unwrap();
    assert_eq!(
        written.world_id.as_deref(),
        Some(engine_a.session_id.as_str())
    );

    // World B must not resolve world A's entry, and B's write-through must
    // not clobber A's live validity record.
    let mut second = ok_read(body);
    observe_with_mode(
        &engine_b,
        "read",
        &args,
        1_000_000,
        &mut second,
        CachezeroMode::Shadow,
    );
    assert_eq!(second.cache_status.as_deref(), Some("forced-miss"));
    assert_eq!(
        index.get(&key).unwrap().unwrap().world_id.as_deref(),
        Some(engine_a.session_id.as_str()),
        "cross-world write-through must not replace world A's record"
    );
    assert!(
        index
            .resolve(&key, Some(engine_b.session_id.as_str()))
            .unwrap()
            .is_none()
    );

    // World A still resolves its own entry.
    let mut third = ok_read(body);
    observe_with_mode(
        &engine_a,
        "read",
        &args,
        1_000_000,
        &mut third,
        CachezeroMode::Shadow,
    );
    assert_eq!(third.cache_status.as_deref(), Some("exact-hit"));
}

#[test]
fn tz0zjn_l3_cold_classifies_refetch_then_write_through_restores() {
    let dir = tempdir().unwrap();
    let engine = engine_at(dir.path());
    let args = json!({"path": "README.md"});
    let body = "cold-body";
    let store_root = store_root_from_cache_path(&engine.config.cache_path);
    let key = action_cache_key(ActionCacheKeyInput {
        op: "read",
        args: &args,
        store_root: &store_root,
        model_id: None,
        consistency_class: Some(ConsistencyClass::parse(None)),
    });

    let mut first = ok_read(body);
    observe_with_mode(
        &engine,
        "read",
        &args,
        1_000_000,
        &mut first,
        CachezeroMode::Shadow,
    );
    assert_eq!(first.cache_status.as_deref(), Some("forced-miss"));

    let index = tokenzero_recovery::ActionCacheIndex::open(&store_root);
    let artifact = index.get(&key).unwrap().unwrap().artifact_ref.clone();

    // Evict the blob with an approving slack guard: the entry goes
    // L2-valid / L3-cold, never tombstoned.
    let plan = index
        .prepare_blob_eviction(
            &artifact,
            5_000,
            0,
            EvictionSlackGuard::new(100, 100).unwrap(),
            1,
        )
        .unwrap();
    assert!(plan.may_delete_blob);
    let cold = index.get(&key).unwrap().unwrap();
    assert!(cold.l3_cold && !cold.tombstone);

    // The refetching request must not claim a would-have-hit for a cold blob.
    let mut second = ok_read(body);
    observe_with_mode(
        &engine,
        "read",
        &args,
        1_000_000,
        &mut second,
        CachezeroMode::Shadow,
    );
    assert_eq!(
        second.cache_status.as_deref(),
        Some("forced-miss"),
        "L3-cold entry needs refetch; never an exact-hit claim"
    );

    // The write-through restored L3 (identical bytes, same key, no
    // rediscovery); the next identical request hits again.
    let mut third = ok_read(body);
    observe_with_mode(
        &engine,
        "read",
        &args,
        1_000_000,
        &mut third,
        CachezeroMode::Shadow,
    );
    assert_eq!(third.cache_status.as_deref(), Some("exact-hit"));
    let restored = index.get(&key).unwrap().unwrap();
    assert!(!restored.l3_cold);
    assert_eq!(restored.artifact_ref, artifact);
}
