use super::*;
use crate::ActionCacheEntry;
use tempfile::tempdir;

fn entry(digest: &str, class: &str, bookmark: Option<&str>) -> ActionCacheEntry {
    ActionCacheEntry {
        key: "aa".repeat(32),
        artifact_ref: format!("tz://blob/{digest}"),
        fszero_bookmark: bookmark.map(str::to_string),
        dep_closure_ref: None,
        class: class.into(),
        verified: true,
        world_id: None,
        tombstone: false,
        tombstoned_at_unix: None,
    }
}

#[test]
fn tz0zjn_classify_miss_hit_stale_causal_and_collapsed() {
    let digest = "bb".repeat(32);
    assert_eq!(
        classify_would_be_status(None, &digest, false, false),
        CacheStatus::ForcedMiss
    );
    assert_eq!(
        classify_would_be_status(
            Some(&entry(&digest, "must_block_revalidate", None)),
            &digest,
            false,
            false
        ),
        CacheStatus::ExactHit
    );
    assert_eq!(
        classify_would_be_status(
            Some(&entry(&digest, "must_block_revalidate", Some("bm"))),
            &digest,
            false,
            false
        ),
        CacheStatus::CausalHit
    );
    let other = "cc".repeat(32);
    assert_eq!(
        classify_would_be_status(Some(&entry(&other, "swr", None)), &digest, false, false),
        CacheStatus::SwrStale
    );
    assert_eq!(
        classify_would_be_status(
            Some(&entry(&digest, "must_block_revalidate", None)),
            &digest,
            true,
            false
        ),
        CacheStatus::CollapsedWait
    );
}

#[test]
fn tz0zjn_shadow_ring_and_stats_graduation_gate() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let miss = ShadowDecision {
        key: "aa".repeat(32),
        bookmark: None,
        blast_intersect: false,
        result_digest: "dd".repeat(32),
        result_tokens: 80,
        wall_ms: 1,
        would_be_status: CacheStatus::ForcedMiss,
        artifact_class: "read".into(),
        saved_tokens_estimate: 0,
    };
    let causal = ShadowDecision {
        result_tokens: 15,
        would_be_status: CacheStatus::CausalHit,
        saved_tokens_estimate: 15,
        ..miss.clone()
    };
    record_shadow_decision(root, &miss).unwrap();
    record_shadow_decision(root, &causal).unwrap();
    let stats = aggregate_cachezero(root).unwrap();
    assert_eq!(stats.decisions, 2);
    assert_eq!(stats.would_have_hits, 1);
    assert_eq!(stats.session_mass, 95);
    assert_eq!(stats.causal_hit_mass, 15);
    assert!(stats.causal_hit_mass_pct < CACHEZERO_GRADUATION_PCT);
    assert!(!stats.graduation, "15/95 is under the 20% gate");
    assert_eq!(stats.by_class["read"].saved_tokens_estimate, 15);
    assert_eq!(shadow_jsonl_path(root), root.join("cachezero/shadow.jsonl"));
}
