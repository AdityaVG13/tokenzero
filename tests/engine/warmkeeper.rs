use tokenzero_engine::{
    CachePricing, CacheProvider, WarmDecisionKind, WarmLane, WarmLaneTier, WarmReplayLane,
    schedule_rewarms, simulate_warmkeeper,
};
fn pricing() -> CachePricing {
    CachePricing {
        input_per_million: 3.0,
        cache_read_per_million: 0.3,
        cache_creation_per_million: 3.75,
        output_per_million: 15.0,
    }
}
#[test]
fn scheduler_is_ttl_aware_ev_gated_and_paid_first() {
    let lanes = vec![
        WarmLane {
            provider: CacheProvider::Gemini,
            model: "self-hosted-compat".into(),
            tier: WarmLaneTier::SelfHosted,
            ttl_seconds: 86_400,
            prefix_tokens: 100_000,
            expected_reads_per_ttl: 100.0,
            pricing: pricing(),
            last_touch_at_seconds: None,
        },
        WarmLane {
            provider: CacheProvider::OpenAi,
            model: "sparse-paid".into(),
            tier: WarmLaneTier::PaidFrontier,
            ttl_seconds: 86_400,
            prefix_tokens: 100_000,
            expected_reads_per_ttl: 0.1,
            pricing: pricing(),
            last_touch_at_seconds: None,
        },
        WarmLane {
            provider: CacheProvider::Anthropic,
            model: "active-paid".into(),
            tier: WarmLaneTier::PaidFrontier,
            ttl_seconds: 86_400,
            prefix_tokens: 200_000,
            expected_reads_per_ttl: 8.0,
            pricing: pricing(),
            last_touch_at_seconds: Some(0),
        },
    ];
    let before_daily_boundary = schedule_rewarms(86_399, &lanes);
    assert_eq!(before_daily_boundary[0].kind, WarmDecisionKind::NotDue);
    let at_daily_boundary = schedule_rewarms(86_400, &lanes);
    assert_eq!(at_daily_boundary[0].kind, WarmDecisionKind::Touch);
    assert_eq!(
        at_daily_boundary[0]
            .touch
            .as_ref()
            .unwrap()
            .max_output_tokens,
        0
    );
    assert_eq!(
        at_daily_boundary[1].kind,
        WarmDecisionKind::NegativeExpectedValue
    );
    assert_eq!(
        at_daily_boundary[2].kind,
        WarmDecisionKind::CompatibilityOnly
    );
    assert_eq!(at_daily_boundary[2].tier, WarmLaneTier::SelfHosted);
    assert!(
        at_daily_boundary[0].expected_savings_dollars > at_daily_boundary[0].write_premium_dollars
    );
}
#[test]
fn replay_corpus_ev_gate_beats_no_warm_and_always_warm() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/cache-meter-session-demo.json")).unwrap();
    let lanes: Vec<WarmReplayLane> =
        serde_json::from_value(fixture["warmkeeper"]["lanes"].clone()).unwrap();
    let first = simulate_warmkeeper(&lanes);
    let second = simulate_warmkeeper(&lanes);
    assert_eq!(first, second);
    assert!(first.ev_gated_billed_dollars < first.no_warm_billed_dollars);
    assert!(first.ev_gated_billed_dollars < first.always_warm_billed_dollars);
    assert_eq!(first.decisions[0].kind, WarmDecisionKind::Touch);
    assert_eq!(
        first.decisions[1].kind,
        WarmDecisionKind::NegativeExpectedValue
    );
    assert_eq!(first.decisions[2].kind, WarmDecisionKind::CompatibilityOnly);
}

/// Prefetch hook (ZS-CACHE-008): high-demand lanes become prefetch targets,
/// the top quota gets Hot placement, and mismatched demand/lane lengths fail
/// loud.
#[test]
fn prefetch_selects_high_demand_lanes_and_hot_places_top_quota() {
    let lane = |model: &str, prefix_tokens: u64| WarmLane {
        provider: CacheProvider::Anthropic,
        model: model.into(),
        tier: WarmLaneTier::PaidFrontier,
        ttl_seconds: 86_400,
        prefix_tokens,
        expected_reads_per_ttl: 1.0,
        pricing: pricing(),
        last_touch_at_seconds: None,
    };
    let lanes = vec![
        lane("hot-closure", 200_000),
        lane("warm-closure", 100_000),
        lane("cold-closure", 50_000),
    ];
    let demand = vec![600, 350, 50];
    let targets = tokenzero_engine::select_prefetch_targets(&lanes, &demand, 100, 1);
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].model, "hot-closure");
    assert_eq!(targets[0].placement, tokenzero_engine::HotPlacement::Hot);
    assert_eq!(targets[0].prefix_tokens, 200_000);
    assert_eq!(targets[1].model, "warm-closure");
    assert_eq!(
        targets[1].placement,
        tokenzero_engine::HotPlacement::Standard
    );
    assert_eq!(targets[1].demand_milli, 350);
}

#[test]
#[should_panic(expected = "prefetch demand scores must cover every lane")]
fn prefetch_mismatched_scores_fail_loud() {
    let lane = tokenzero_engine::WarmLane {
        provider: CacheProvider::Anthropic,
        model: "m".into(),
        tier: WarmLaneTier::PaidFrontier,
        ttl_seconds: 86_400,
        prefix_tokens: 1_000,
        expected_reads_per_ttl: 1.0,
        pricing: pricing(),
        last_touch_at_seconds: None,
    };
    let _ = tokenzero_engine::select_prefetch_targets(&[lane], &[], 100, 1);
}
