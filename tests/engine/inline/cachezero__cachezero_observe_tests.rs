use super::*;
use crate::TokenZeroEngine;
use crate::config::EngineConfig;
use serde_json::json;
use tempfile::tempdir;
use tokenzero_core::{Accounting, Mode, ToolResponse};
use tokenzero_recovery::{aggregate_cachezero, store_root_from_cache_path};

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
