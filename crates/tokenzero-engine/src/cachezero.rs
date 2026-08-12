//! Shadow ActionCache observation: classify and journal, never serve.

use crate::TokenZeroEngine;
use crate::action_cache_key::{ActionCacheKeyInput, ConsistencyClass, action_cache_key};
use serde_json::Value;
use tokenzero_core::{ToolResponse, sha256_hex};
use tokenzero_recovery::{
    ActionCacheEntry, ActionCacheIndex, CacheStatus, CachezeroMode, ShadowDecision,
    classify_would_be_status, record_shadow_decision, store_root_from_cache_path,
};

pub fn observe_action_cache(
    engine: &TokenZeroEngine,
    op: &str,
    args: &Value,
    wall_ns: u64,
    response: &mut ToolResponse,
) {
    observe_with_mode(
        engine,
        op,
        args,
        wall_ns,
        response,
        CachezeroMode::from_env(),
    );
}

pub fn observe_with_mode(
    engine: &TokenZeroEngine,
    op: &str,
    args: &Value,
    wall_ns: u64,
    response: &mut ToolResponse,
    mode: CachezeroMode,
) {
    if !mode.is_shadow() {
        return;
    }
    let store_root = store_root_from_cache_path(&engine.config.cache_path);
    let consistency = ConsistencyClass::parse(
        args.get("consistency_class")
            .and_then(Value::as_str)
            .or_else(|| args.get("consistency").and_then(Value::as_str)),
    );
    let model_id = args.get("model_id").and_then(Value::as_str);
    let key = action_cache_key(ActionCacheKeyInput {
        op,
        args,
        store_root: &store_root,
        model_id,
        consistency_class: Some(consistency),
    });
    let index = ActionCacheIndex::open(&store_root);
    let entry = index.get(&key).ok().flatten();
    let result_digest = result_digest(response);
    let in_flight = index.has_in_flight_serve(&key);
    // Without an FSZero journal we cannot prove a bookmark is still live.
    // A present bookmark is treated as intersect so we never invent causal-hit.
    let blast_intersect = entry
        .as_ref()
        .is_some_and(|item| item.fszero_bookmark.is_some());
    let status =
        classify_would_be_status(entry.as_ref(), &result_digest, in_flight, blast_intersect);
    let result_tokens = response
        .accounting
        .as_ref()
        .map(|acct| acct.visible_tokens as u64)
        .unwrap_or(0);
    let saved = if status.would_have_hit() {
        result_tokens
    } else {
        0
    };
    response.cache_status = Some(status.as_str().to_string());
    response.saved_tokens_estimate = Some(saved);

    let bookmark = entry.as_ref().and_then(|item| item.fszero_bookmark.clone());
    let decision = ShadowDecision {
        key: key.clone(),
        bookmark,
        blast_intersect,
        result_digest: result_digest.clone(),
        result_tokens,
        wall_ms: wall_ns / 1_000_000,
        would_be_status: status,
        artifact_class: artifact_class(op).to_string(),
        saved_tokens_estimate: saved,
    };
    let _ = record_shadow_decision(&store_root, &decision);
    // Write-through only. The just-computed body is what the caller already has.
    let _ = index.put(ActionCacheEntry {
        key,
        artifact_ref: format!("tz://blob/{result_digest}"),
        fszero_bookmark: None,
        dep_closure_ref: None,
        class: consistency.as_str().to_string(),
        verified: response.status == "ok",
        world_id: None,
        tombstone: false,
        tombstoned_at_unix: None,
    });
}

fn result_digest(response: &ToolResponse) -> String {
    for record in &response.refs {
        if record.kind != "blob" {
            continue;
        }
        if let Some(hash) = record.ref_id.rsplit('/').next().filter(|part| {
            part.len() == 64
                && part
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        }) {
            return hash.to_string();
        }
    }
    let text = response
        .visible
        .as_ref()
        .map(|visible| visible.text.as_str())
        .unwrap_or("");
    sha256_hex(text)
}

fn artifact_class(op: &str) -> &str {
    op.strip_prefix("tz_")
        .or_else(|| op.strip_prefix("zero.token."))
        .or_else(|| op.strip_prefix("zero."))
        .unwrap_or(op)
}

#[cfg(test)]
mod cachezero_observe_tests {
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
}
