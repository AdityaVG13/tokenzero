use super::*;

fn prefix(bytes: String, cache_breakpoint: bool, blocks: usize) -> CacheablePrefix {
    CacheablePrefix {
        bytes,
        cache_breakpoint,
        blocks_per_turn: BTreeMap::from([(7, blocks)]),
    }
}

#[test]
fn golden_prefix_is_monotone_between_breakpoints_and_injected_violation_fails() {
    let mut guard = PrefixStabilityGuard::default();
    let base = "cache ".repeat(1_100);
    guard
        .observe_prefix(&prefix(base.clone(), true, 1), CacheModelTier::OlderSonnet)
        .unwrap();
    guard
        .observe_prefix(
            &prefix(format!("{base}tail"), false, 1),
            CacheModelTier::OlderSonnet,
        )
        .unwrap();
    let violation = guard.observe_prefix(
        &prefix(format!("mutated-{base}"), false, 1),
        CacheModelTier::OlderSonnet,
    );
    assert_eq!(violation, Err(PrefixStabilityViolation::NonMonotonePrefix));
}

#[test]
fn golden_breakpoint_may_reset_to_a_non_extending_prefix() {
    let mut guard = PrefixStabilityGuard::default();
    guard
        .observe_prefix(
            &prefix("old provider prefix ".repeat(1_300), false, 1),
            CacheModelTier::OlderSonnet,
        )
        .unwrap();
    guard
        .observe_prefix(
            &prefix("replacement prefix ".repeat(1_300), true, 1),
            CacheModelTier::OlderSonnet,
        )
        .expect("an explicit provider breakpoint permits a prefix reset");
}

#[test]
fn golden_render_is_byte_identical_for_content_level_and_tokenizer() {
    let mut guard = PrefixStabilityGuard::default();
    let observation = RenderObservation {
        content: "same evidence",
        rendered: "stable capsule",
        level: "capsule",
        tokenizer_id: "claude-opus-4-8",
    };
    let first = guard.observe_render(observation.clone()).unwrap();
    let second = guard.observe_render(observation).unwrap();
    assert_eq!(first, second);
    let injected = guard.observe_render(RenderObservation {
        content: "same evidence",
        rendered: "changed capsule",
        level: "capsule",
        tokenizer_id: "claude-opus-4-8",
    });
    assert!(matches!(
        injected,
        Err(PrefixStabilityViolation::NonDeterministicRender { .. })
    ));
}

#[test]
fn golden_cache_block_budget_is_fifteen_per_turn() {
    let mut guard = PrefixStabilityGuard::default();
    guard
        .observe_prefix(
            &prefix("x ".repeat(1_100), true, 15),
            CacheModelTier::OlderSonnet,
        )
        .unwrap();
    assert_eq!(
        guard.observe_prefix(
            &prefix("x ".repeat(1_100), true, 16),
            CacheModelTier::OlderSonnet
        ),
        Err(PrefixStabilityViolation::BlockBudgetExceeded {
            turn: 7,
            blocks: 16,
            maximum: 15
        })
    );
}

#[test]
fn golden_model_floors_alert_before_caching_silently_stops() {
    for (tier, provider_floor, required) in [
        (CacheModelTier::Opus, 4_096, 5_120),
        (CacheModelTier::FableOrSonnet46, 2_048, 2_560),
        (CacheModelTier::OlderSonnet, 1_024, 1_280),
    ] {
        assert_eq!(tier.min_cacheable_tokens(), provider_floor);
        assert_eq!(tier.min_cacheable_estimator_tokens(), required);
        let mut guard = PrefixStabilityGuard::default();
        let alert = guard
            .observe_prefix(&prefix("too short".into(), true, 1), tier)
            .unwrap();
        assert_eq!(
            alert,
            Some(PrefixStabilityAlert::BelowCacheableFloor {
                observed_tokens: count_tokens("too short"),
                required_tokens: required,
                model_tier: tier,
            })
        );
    }
}
