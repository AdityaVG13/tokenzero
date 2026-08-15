//! Greenfield property-oracle for the capsule emission path.
//! Invariants (tokenzero-54br, CONF-NEG-PROP-001, tokenzero-pw33):
//! - prop_visible_le_raw: a capsule must never cost more visible tokens than
//!   the raw text it renders. Counterexample before the fix: visible=33
//!   raw=15 budget=1 (lossy declaration alone cost 33 tokens).
//! - prop_budget_monotonic_*: visible token cost is non-decreasing in the
//!   visible budget, and (except the marker-floor exception) never exceeds
//!   its own budget. Counterexample before the fix (CONF-H-006 / EXP-002):
//!   vis_low=33@budget=22 vs vis_high=23@budget=441.
//!
//! Exclusions (named, not silent):
//! - Mode::Passthrough: skips `enforce_token_budget` by contract.
//! - Mode::Exact: exempt from the inflation guard that keeps capsule cost
//!   monotone; its contract is to hide the payload.
//! - Bare `enforce_token_budget` across the truncation/fit boundary: the
//!   lossy marker can cost more than the raw text, so a higher budget that
//!   fits the full payload may serve fewer tokens than a lower budget that
//!   still truncates. `make_capsule`'s inflation guard closes that pathology
//!   for the capsule path; this suite only asserts same-regime pairs for
//!   the bare packer (both fit or both truncate).
//! - Budgets below `VISIBLE_BUDGET_LOSSY_DECLARATION` token cost: the marker
//!   is a correctness floor and may exceed an impossibly small budget
//!   (same exception as `visible_budget_never_exceeds`).
use proptest::prelude::*;
use tokenzero_core::{
    Mode, ShellRenderInput, VISIBLE_BUDGET_LOSSY_DECLARATION, count_tokens, enforce_token_budget,
    make_capsule, render_shell,
};

fn text_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::collection::vec(any::<char>(), 0..64usize)
            .prop_map(|cs| cs.into_iter().collect::<String>()),
        prop::collection::vec("[a-z]{1,8}( [a-z]{1,8}){0,4}", 0..24usize)
            .prop_map(|ls| ls.join("\n")),
        "[ -~]{0,200}".prop_map(|s| s),
        prop::collection::vec(0u8..32, 1..16usize).prop_map(|ns| ns
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(" ")),
    ]
}

fn budget_strategy() -> impl Strategy<Value = usize> {
    prop_oneof![0usize..8, 8..64usize, 64..512usize]
}

fn marker_floor() -> usize {
    count_tokens(VISIBLE_BUDGET_LOSSY_DECLARATION)
}

/// Budgets that can hold the lossy marker (never-exceeds in-scope).
fn budget_pair_above_marker() -> impl Strategy<Value = (usize, usize)> {
    let floor = marker_floor();
    (floor..=floor.saturating_add(256)).prop_flat_map(move |lo| (Just(lo), lo..=lo + 256))
}

fn assert_monotone_visible(op: &str, lo: usize, hi: usize, vis_lo: usize, vis_hi: usize, detail: &str) {
    assert!(
        vis_hi >= vis_lo,
        "{op}: budget {lo} -> visible {vis_lo}, budget {hi} -> visible {vis_hi} (expected non-decreasing); {detail}"
    );
}

fn assert_within_own_budget(op: &str, budget: usize, visible: usize, detail: &str) {
    assert!(
        visible <= budget,
        "{op}: budget {budget} exceeded: visible {visible}; {detail}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn prop_visible_le_raw(text in text_strategy(), budget in budget_strategy()) {
        let raw = count_tokens(&text);
        let capsule = make_capsule(&text, Mode::Auto, budget, Some("probe"))
            .expect("capsule should satisfy the omission rule");
        prop_assert!(
            capsule.visible_tokens <= raw,
            "visible={} raw={} budget={} text={:?}",
            capsule.visible_tokens, raw, budget, text
        );
    }

    /// Mode::Auto only — Passthrough/Exact excluded by name above.
    #[test]
    fn prop_budget_monotonic_make_capsule(
        text in text_strategy(),
        (lo, hi) in (0usize..256).prop_flat_map(|lo| (Just(lo), lo..512usize)),
    ) {
        const OP: &str = "make_capsule";
        let low = make_capsule(&text, Mode::Auto, lo, Some("probe"))
            .expect("capsule should satisfy the omission rule");
        let high = make_capsule(&text, Mode::Auto, hi, Some("probe"))
            .expect("capsule should satisfy the omission rule");
        // Capsule may exceed its budget only via the inflation-guard raw
        // fallback (keeps visible_le_raw + monotone); never-exceeds for the
        // bare packer is covered by prop_budget_monotonic_enforce_token_budget.
        prop_assert!(
            high.visible_tokens >= low.visible_tokens,
            "{OP}: budget {} -> visible {}, budget {} -> visible {} for text {:?}",
            lo, low.visible_tokens, hi, high.visible_tokens, text
        );
    }

    /// Same-regime pairs only (see module exclusions for truncation/fit).
    #[test]
    fn prop_budget_monotonic_enforce_token_budget(
        text in text_strategy(),
        (lo, hi) in budget_pair_above_marker(),
    ) {
        const OP: &str = "enforce_token_budget";
        let out_lo = enforce_token_budget(&text, lo);
        let out_hi = enforce_token_budget(&text, hi);
        let vis_lo = count_tokens(&out_lo);
        let vis_hi = count_tokens(&out_hi);
        prop_assert!(
            vis_lo <= lo,
            "{OP}: budget {} exceeded: visible {} for text {:?}",
            lo, vis_lo, text
        );
        prop_assert!(
            vis_hi <= hi,
            "{OP}: budget {} exceeded: visible {} for text {:?}",
            hi, vis_hi, text
        );
        let lo_fits = out_lo == text;
        let hi_fits = out_hi == text;
        // Truncation/fit boundary: excluded by name in the module docs.
        prop_assume!(lo_fits == hi_fits);
        prop_assert!(
            vis_hi >= vis_lo,
            "{OP}: budget {} -> visible {}, budget {} -> visible {} for text {:?}",
            lo, vis_lo, hi, vis_hi, text
        );
    }
}

#[test]
fn regression_visible_33_raw_15_budget_1() {
    let text = (1..=15)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let raw = count_tokens(&text);
    assert_eq!(raw, 15);
    let capsule = make_capsule(&text, Mode::Auto, 1, Some("probe"))
        .expect("capsule should satisfy the omission rule");
    assert!(
        capsule.visible_tokens <= raw,
        "visible={} raw={raw}",
        capsule.visible_tokens
    );
    let high = make_capsule(&text, Mode::Auto, 441, Some("probe"))
        .expect("capsule should satisfy the omission rule");
    assert!(high.visible_tokens >= capsule.visible_tokens);
}

/// Historical EXP-002 / CONF-H-006 pair, named op on failure.
#[test]
fn budget_monotonic_make_capsule_exp002_regression() {
    const OP: &str = "make_capsule";
    let text = (1..=15)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let lo = 22usize;
    let hi = 441usize;
    let low = make_capsule(&text, Mode::Auto, lo, Some("probe"))
        .expect("capsule should satisfy the omission rule");
    let high = make_capsule(&text, Mode::Auto, hi, Some("probe"))
        .expect("capsule should satisfy the omission rule");
    assert_monotone_visible(
        OP,
        lo,
        hi,
        low.visible_tokens,
        high.visible_tokens,
        &format!("text={text:?}"),
    );
}

/// Multi-line failure shell: stays on PolicyBased (budget-enforced) path.
#[test]
fn budget_monotonic_render_shell_same_regime() {
    const OP: &str = "render_shell";
    let floor = marker_floor();
    let stdout = (0..40)
        .map(|i| format!("line_{i:02} error: widget {i} failed validation"))
        .collect::<Vec<_>>()
        .join("\n");
    let stderr = "error: build failed\n";
    let lo = floor + 8;
    let hi = floor + 80;
    let render_at = |budget: usize| {
        render_shell(ShellRenderInput {
            command: "cargo test -p demo",
            stdout: &stdout,
            stderr,
            exit_code: Some(101),
            timed_out: false,
            mode: Mode::Auto,
            max_visible_tokens: budget,
            stdout_ref: None,
            stderr_ref: None,
            combined_ref: None,
        })
    };
    let low = render_at(lo);
    let high = render_at(hi);
    let vis_lo = count_tokens(&low.visible);
    let vis_hi = count_tokens(&high.visible);
    assert_within_own_budget(OP, lo, vis_lo, &format!("visible={:?}", low.visible));
    assert_within_own_budget(OP, hi, vis_hi, &format!("visible={:?}", high.visible));
    // Same-regime: payload far exceeds hi, so both paths stay truncated.
    assert!(
        vis_lo <= lo && vis_hi <= hi,
        "{OP}: expected both budgets to bind"
    );
    assert_monotone_visible(
        OP,
        lo,
        hi,
        vis_lo,
        vis_hi,
        &format!("strategy_lo={} strategy_hi={}", low.output_strategy, high.output_strategy),
    );
}

#[test]
fn budget_monotonic_enforce_token_budget_both_truncate() {
    const OP: &str = "enforce_token_budget";
    let floor = marker_floor();
    // Keep growing until raw exceeds hi so both budgets stay in the truncate regime.
    let mut text = String::new();
    let lo = floor;
    let hi = floor + 40;
    for i in 0..500 {
        text.push_str(&format!(
            "payload_line_{i:03}_abcdefghijklmnopqrstuvwxyz_0123456789_extra_padding\n"
        ));
        if count_tokens(&text) > hi {
            break;
        }
    }
    assert!(
        count_tokens(&text) > hi,
        "{OP}: fixture must exceed hi={hi}"
    );
    let out_lo = enforce_token_budget(&text, lo);
    let out_hi = enforce_token_budget(&text, hi);
    let vis_lo = count_tokens(&out_lo);
    let vis_hi = count_tokens(&out_hi);
    assert_ne!(out_lo, text, "{OP}: expected truncation at lo={lo}");
    assert_ne!(out_hi, text, "{OP}: expected truncation at hi={hi}");
    assert_within_own_budget(OP, lo, vis_lo, &format!("text_tokens={}", count_tokens(&text)));
    assert_within_own_budget(OP, hi, vis_hi, &format!("text_tokens={}", count_tokens(&text)));
    assert_monotone_visible(OP, lo, hi, vis_lo, vis_hi, &format!("text_tokens={}", count_tokens(&text)));
}
