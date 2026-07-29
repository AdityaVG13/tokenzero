//! Greenfield property-oracle for the capsule emission path.
//! Invariants (tokenzero-54br, CONF-NEG-PROP-001):
//! - prop_visible_le_raw: a capsule must never cost more visible tokens than
//!   the raw text it renders. Counterexample before the fix: visible=33
//!   raw=15 budget=1 (lossy declaration alone cost 33 tokens).
//! - prop_budget_monotonic: visible token cost is non-decreasing in the
//!   visible budget. Counterexample before the fix: vis_low=33@budget=22
//!   vs vis_high=23@budget=441.
use proptest::prelude::*;
use tokenzero_core::{count_tokens, make_capsule, Mode};

fn text_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::collection::vec(any::<char>(), 0..64usize)
            .prop_map(|cs| cs.into_iter().collect::<String>()),
        prop::collection::vec("[a-z]{1,8}( [a-z]{1,8}){0,4}", 0..24usize)
            .prop_map(|ls| ls.join("\n")),
        "[ -~]{0,200}".prop_map(|s| s),
        prop::collection::vec(0u8..32, 1..16usize)
            .prop_map(|ns| ns.iter().map(u8::to_string).collect::<Vec<_>>().join(" ")),
    ]
}

fn budget_strategy() -> impl Strategy<Value = usize> {
    prop_oneof![0usize..8, 8..64usize, 64..512usize]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn prop_visible_le_raw(text in text_strategy(), budget in budget_strategy()) {
        let raw = count_tokens(&text);
        let capsule = make_capsule(&text, Mode::Auto, budget, Some("probe"));
        prop_assert!(
            capsule.visible_tokens <= raw,
            "visible={} raw={} budget={} text={:?}",
            capsule.visible_tokens, raw, budget, text
        );
    }

    #[test]
    fn prop_budget_monotonic(
        text in text_strategy(),
        (lo, hi) in (0usize..256).prop_flat_map(|lo| (Just(lo), lo..512usize)),
    ) {
        let low = make_capsule(&text, Mode::Auto, lo, Some("probe"));
        let high = make_capsule(&text, Mode::Auto, hi, Some("probe"));
        prop_assert!(
            high.visible_tokens >= low.visible_tokens,
            "budget {} -> visible {}, budget {} -> visible {} for text {:?}",
            lo, low.visible_tokens, hi, high.visible_tokens, text
        );
    }
}

#[test]
fn regression_visible_33_raw_15_budget_1() {
    let text = (1..=15).map(|i| i.to_string()).collect::<Vec<_>>().join(" ");
    let raw = count_tokens(&text);
    assert_eq!(raw, 15);
    let capsule = make_capsule(&text, Mode::Auto, 1, Some("probe"));
    assert!(capsule.visible_tokens <= raw, "visible={} raw={raw}", capsule.visible_tokens);
    let high = make_capsule(&text, Mode::Auto, 441, Some("probe"));
    assert!(high.visible_tokens >= capsule.visible_tokens);
}
