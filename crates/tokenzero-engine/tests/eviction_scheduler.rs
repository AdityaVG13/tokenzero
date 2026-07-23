use tokenzero_engine::{
    CacheProvider, EvictionCandidate, EvictionDecisionKind, EvictionReplayItem,
    EvictionSavingsLedger, OPENAI_MAX_RETENTION_SECONDS, PrefixTier, provider_breakpoints,
    schedule_evictions, simulate_eviction_replay, ttl_from_gaps,
};

fn candidate(id: &str, tier: PrefixTier, expected: u64, savings: u64, rewrite: u64) -> EvictionCandidate {
    EvictionCandidate {
        id: id.to_owned(),
        tier,
        prefix_tokens: 1_000,
        prefix_rewrite_cost_tokens: rewrite,
        expected_remaining_requests: expected,
        read_savings_tokens: savings,
    }
}

#[test]
fn provider_ttls_follow_measured_gaps_and_documented_break_evens() {
    let anthropic = provider_breakpoints(CacheProvider::Anthropic);
    assert_eq!((anthropic[0].ttl_seconds, anthropic[0].minimum_requests, anthropic[0].write_multiplier), (300, 2, 1.25));
    assert_eq!((anthropic[1].ttl_seconds, anthropic[1].minimum_requests, anthropic[1].write_multiplier), (3_600, 3, 2.0));
    assert_eq!(ttl_from_gaps(CacheProvider::Anthropic, &[120]).unwrap().ttl_seconds, 300);
    assert_eq!(ttl_from_gaps(CacheProvider::Anthropic, &[900, 900]).unwrap().ttl_seconds, 3_600);
    assert!(ttl_from_gaps(CacheProvider::Anthropic, &[4_000, 4_000]).is_none());
    assert_eq!(ttl_from_gaps(CacheProvider::OpenAi, &[80_000]).unwrap().ttl_seconds, OPENAI_MAX_RETENTION_SECONDS);
    assert_eq!(OPENAI_MAX_RETENTION_SECONDS, 86_400);
}

#[test]
fn strict_break_even_only_evicts_messages_and_batches_at_breakpoint() {
    let candidates = vec![
        candidate("system", PrefixTier::System, 10, 900, 1),
        candidate("tools", PrefixTier::Tools, 10, 900, 1),
        candidate("equal", PrefixTier::Messages, 2, 50, 100),
        candidate("positive-b", PrefixTier::Messages, 3, 100, 100),
        candidate("positive-a", PrefixTier::Messages, 3, 100, 100),
    ];
    let schedule = schedule_evictions(CacheProvider::Anthropic, 1_000, &[100], &candidates);
    assert_eq!(schedule.decisions[0].kind, EvictionDecisionKind::PreserveProtectedTier);
    assert_eq!(schedule.decisions[1].kind, EvictionDecisionKind::PreserveProtectedTier);
    assert_eq!(schedule.decisions[2].kind, EvictionDecisionKind::PreserveNegativeExpectedValue);
    assert_eq!(schedule.decisions[3].kind, EvictionDecisionKind::Evict);
    assert_eq!(schedule.batches.len(), 1);
    assert_eq!(schedule.batches[0].cache_breakpoint_at_seconds, 1_300);
    assert_eq!(schedule.batches[0].candidate_ids, ["positive-a", "positive-b"]);
}

#[test]
fn replay_scheduler_reduces_billed_tokens_against_naive_retention() {
    let replay = vec![EvictionReplayItem {
        candidate: candidate("messages", PrefixTier::Messages, 3, 800, 100),
        observed_remaining_requests: 3,
    }];
    let report = simulate_eviction_replay(CacheProvider::Anthropic, 0, &[60], &replay);
    assert_eq!(report.naive_billed_tokens, 3_000);
    assert_eq!(report.scheduled_billed_tokens, 700);
    assert_eq!(report.saved_billed_tokens, 2_300);
    assert!(report.scheduled_billed_tokens < report.naive_billed_tokens);
}

#[test]
fn ledger_never_double_books_retried_eviction_savings() {
    let mut ledger = EvictionSavingsLedger::default();
    assert!(ledger.record_once("session-1:span-7", 2_300));
    assert!(!ledger.record_once("session-1:span-7", 2_300));
    assert!(ledger.record_once("session-1:span-8", 100));
    assert_eq!(ledger.saved_billed_tokens(), 2_400);
    assert_eq!(ledger.eviction_amortization()["unique_events"], 2);
    assert_eq!(ledger.eviction_amortization()["saved_billed_tokens"], 2_400);
}
