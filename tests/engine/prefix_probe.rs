//! ZS-BENCH-003 provider-prefix probe replay tests (schema
//! `tokenzero.prefix-probe.v1`).
//!
//! The probe is types + fixture replay only: no provider integration and no
//! network. LCP between successive arm histories is computed locally with the
//! shared `common_prefix_len` helper; declared facts (provider-reported hits,
//! harness-declared eligibility) are carried through verbatim and are never
//! conflated with measured prefix overlap.

use serde_json::Value;
use tokenzero_engine::{
    ArmTrial, HistoryChunk, ProbeArm, ProbeFixture, ProbeReport, QualitySlot, replay_prefix_probe,
};
use tokenzero_test_support::{GauntletIdentityPair, GauntletOracle};

const PREFIX_PROBE_FIXTURE_JSON: &str = include_str!("fixtures/prefix-probe-replay.json");

/// Live driver stamp: Subject vs Spec oracle. Never MCP `EngineIdentity::TokenZero`.
fn stamp_gauntlet_subject_ne_oracle() {
    GauntletIdentityPair::new(GauntletOracle::Spec).assert_distinct();
}

fn fixture() -> ProbeFixture {
    stamp_gauntlet_subject_ne_oracle();
    serde_json::from_str(PREFIX_PROBE_FIXTURE_JSON).expect("prefix-probe-replay.json must parse")
}

fn report_by_arm<'a>(reports: &'a [ProbeReport], arm: ProbeArm) -> &'a ProbeReport {
    reports
        .iter()
        .find(|report| report.arm == arm)
        .unwrap_or_else(|| panic!("missing report for arm {arm:?}"))
}

#[test]
fn gauntlet_subject_is_not_spec_oracle() {
    stamp_gauntlet_subject_ne_oracle();
}

#[test]
fn fixture_schema_is_prefix_probe() {
    assert_eq!(fixture().schema, "tokenzero.prefix-probe.v1");
    assert_eq!(fixture().arms.len(), 3);
}

#[test]
fn fixture_arms_keep_eligibility_and_hit_as_separate_declared_keys() {
    stamp_gauntlet_subject_ne_oracle();
    let raw: Value =
        serde_json::from_str(PREFIX_PROBE_FIXTURE_JSON).expect("prefix-probe JSON must parse");
    let arms = raw["arms"]
        .as_array()
        .expect("prefix-probe fixture must have an arms array");
    assert!(!arms.is_empty(), "prefix-probe fixture must have arms");
    for (index, arm) in arms.iter().enumerate() {
        let object = arm
            .as_object()
            .unwrap_or_else(|| panic!("arm {index} must be a JSON object"));
        assert!(
            object.contains_key("eligibility_declared"),
            "arm {index} is missing eligibility_declared as its own key (null is allowed; omission is not)"
        );
        assert!(
            object.contains_key("hit_declared_by_provider"),
            "arm {index} is missing hit_declared_by_provider as its own key (null is allowed; omission is not)"
        );
        assert!(
            !object.contains_key("lcp_tokens"),
            "arm {index}: fixture must not carry measured lcp_tokens; hit must not be derived from LCP"
        );
    }
}

#[test]
fn replay_covers_exactly_the_three_probe_arms() {
    let reports = replay_prefix_probe(&fixture());
    assert_eq!(reports.len(), 3);
    for arm in [
        ProbeArm::RawRetained,
        ProbeArm::RetrospectiveRewrite,
        ProbeArm::StableCapsule,
    ] {
        assert_eq!(report_by_arm(&reports, arm).arm, arm);
    }
}

#[test]
fn raw_retained_arm_achieves_exact_reuse_and_keeps_declared_miss_distinct() {
    let reports = replay_prefix_probe(&fixture());
    let report = report_by_arm(&reports, ProbeArm::RawRetained);
    // Successive LCPs: 9 (h1 vs h2) + 11 (h2 vs h3) = 20 measured prefix tokens.
    assert_eq!(report.lcp_tokens, 20);
    assert_eq!(report.total_tokens, 13);
    assert_eq!(report.quality_slot, QualitySlot::ExactReuse);
    assert_eq!(report.cost_usd_milli, 12);
    assert_eq!(report.latency_ms, 312);
    assert_eq!(report.expansion_count, 0);
    // Honest-telemetry law: full measured prefix retention does NOT become a
    // reported hit. The provider declared a miss; the report carries both
    // facts unmerged.
    assert_eq!(report.hit_declared_by_provider, Some(false));
    assert_eq!(report.eligibility_declared, Some(true));
}

#[test]
fn retrospective_rewrite_arm_loses_prefix_and_expands() {
    let reports = replay_prefix_probe(&fixture());
    let report = report_by_arm(&reports, ProbeArm::RetrospectiveRewrite);
    // Successive LCPs: 6 (h1 vs h2, rewrite killed the middle) + 13 (h2 vs
    // h3) = 19 measured prefix tokens; reusable across pairs was 11 + 13 =
    // 24, so reuse is partial.
    assert_eq!(report.lcp_tokens, 19);
    assert_eq!(report.total_tokens, 15);
    assert_eq!(report.quality_slot, QualitySlot::PartialReuse);
    assert_eq!(report.expansion_count, 2);
    assert_eq!(report.hit_declared_by_provider, None);
    assert_eq!(report.eligibility_declared, Some(true));
}

#[test]
fn stable_capsule_arm_reuses_only_the_stable_prefix() {
    let reports = replay_prefix_probe(&fixture());
    let report = report_by_arm(&reports, ProbeArm::StableCapsule);
    // Successive LCPs: 6 + 6 = 12 measured prefix tokens (the stable capsule
    // only); the volatile tail changes every turn.
    assert_eq!(report.lcp_tokens, 12);
    assert_eq!(report.total_tokens, 10);
    assert_eq!(report.quality_slot, QualitySlot::PartialReuse);
    assert_eq!(report.expansion_count, 0);
    assert_eq!(report.hit_declared_by_provider, None);
}

#[test]
fn measured_overlap_never_implies_a_reported_hit() {
    for report in &replay_prefix_probe(&fixture()) {
        assert!(
            report.lcp_tokens > 0,
            "every fixture arm shows measurable prefix overlap"
        );
        // No provider claim of a hit exists anywhere in this fixture, and the
        // replay must not invent one from measured LCP.
        assert_ne!(
            report.hit_declared_by_provider,
            Some(true),
            "measured prefix overlap must never be presented as a provider-reported hit"
        );
        // Eligibility is declared by the harness and is never derived from
        // history comparison.
        assert_eq!(report.eligibility_declared, Some(true));
    }
}

#[test]
fn degenerate_single_history_arm_reports_no_reuse() {
    stamp_gauntlet_subject_ne_oracle();
    let fixture = ProbeFixture {
        schema: "tokenzero.prefix-probe.v1".to_string(),
        arms: vec![ArmTrial {
            arm: ProbeArm::StableCapsule,
            histories: vec![vec![HistoryChunk {
                text: "only one call".to_string(),
                tokens: 4,
            }]],
            cost_usd_milli: 1,
            latency_ms: 50,
            expansion_count: 0,
            hit_declared_by_provider: None,
            eligibility_declared: Some(false),
        }],
    };
    let reports = replay_prefix_probe(&fixture);
    let report = report_by_arm(&reports, ProbeArm::StableCapsule);
    assert_eq!(report.lcp_tokens, 0);
    assert_eq!(report.total_tokens, 4);
    assert_eq!(report.quality_slot, QualitySlot::NoReuse);
    // The declared ineligibility is carried verbatim, untouched by the
    // absence of reuse evidence.
    assert_eq!(report.eligibility_declared, Some(false));
}
