use super::*;
use crate::{EngineConfig, LocalPayloadPolicy, Mode, TokenZeroEngine, local_payload_policy};

/// The estimator's `ByteThreshold` policy must reproduce the legacy
/// fixed-threshold rule exactly (ZS-VIEW-006 additive guarantee).
#[test]
fn threshold_policy_reproduces_byte_threshold_rule() {
    let estimator = AdmissionEstimator {
        exact_ref_threshold_bytes: 40 * 1024,
        ..AdmissionEstimator::default()
    };
    for payload_bytes in [1, 40 * 1024, 40 * 1024 + 1, 200_000] {
        let decision = estimator.decide_threshold(payload_bytes);
        let legacy = local_payload_policy(payload_bytes, 40 * 1024, Mode::Auto, true);
        assert_eq!(
            decision.admit_exact_ref,
            legacy == LocalPayloadPolicy::ExactRef,
            "payload_bytes={payload_bytes}"
        );
        assert_eq!(decision.policy, AdmissionPolicy::ByteThreshold);
    }
    // Boundary: exactly at the threshold stays inline, one byte above is
    // admitted -- the legacy `>` comparison.
    assert!(!estimator.decide_threshold(40 * 1024).admit_exact_ref);
    assert!(estimator.decide_threshold(40 * 1024 + 1).admit_exact_ref);
}

/// Horizon-cost admission: expected reuse value must cover the ref handling
/// cost, scaled by (1 - expansion probability).
#[test]
fn horizon_cost_admits_only_when_expected_savings_cover_handling() {
    let estimator = AdmissionEstimator {
        exact_ref_threshold_bytes: 40 * 1024,
        default_expansion_probability_milli: 100,
        default_horizon: 1,
    };
    // 16 KB payload = ~4096 tokens. p=0, horizon=1, handling=10:
    // savings = 4096 > 10 -> admit.
    let admitted = estimator.decide_horizon_cost(16 * 1024, Some(0), Some(1), 10);
    assert!(admitted.admit_exact_ref);
    assert_eq!(admitted.reason, AdmissionReason::RefAdmittedByHorizon);

    // Same payload but the handling cost exceeds expected savings -> inline.
    let denied = estimator.decide_horizon_cost(16 * 1024, Some(0), Some(1), 10_000);
    assert!(!denied.admit_exact_ref);
    assert_eq!(denied.reason, AdmissionReason::InlineCheaperThanRef);

    // No expected reuse -> never admitted.
    let no_reuse = estimator.decide_horizon_cost(16 * 1024, Some(0), Some(0), 1);
    assert!(!no_reuse.admit_exact_ref);
    assert_eq!(no_reuse.reason, AdmissionReason::NoExpectedReuse);

    // Always expands -> the ref never pays.
    let always = estimator.decide_horizon_cost(16 * 1024, Some(1000), Some(5), 1);
    assert!(!always.admit_exact_ref);
    assert_eq!(always.reason, AdmissionReason::ExpansionAlways);

    // Horizon flips a denial: handling 3000, p=500 milli, payload 8 KB
    // (2048 tokens): savings = 500/1000 * 2048 = 1024 < 3000 -> inline;
    // horizon 4: savings = 4096 > 3000 -> admit.
    let one = estimator.decide_horizon_cost(8 * 1024, Some(500), Some(1), 3000);
    assert!(!one.admit_exact_ref);
    let four = estimator.decide_horizon_cost(8 * 1024, Some(500), Some(4), 3000);
    assert!(four.admit_exact_ref);
}

/// Replay-data prediction: derive an expansion probability per operation
/// class from the `token-amplification-replay.json` fixture and feed it to
/// the estimator. The predicted probability must be bounded (per-mille) and
/// produce sane admission for a large payload.
#[test]
fn replay_fixture_predicts_expansion_probability_into_admission() {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/token-amplification-replay.json")).unwrap();
    let cases = value["cases"].as_array().expect("replay cases");
    assert!(!cases.is_empty());

    let estimator = AdmissionEstimator::default();
    for case in cases {
        let output = &case["output"];
        let input = &case["input"];
        let output_visible = output["visible"].as_u64().unwrap_or(0);
        let input_visible = input["visible"].as_u64().unwrap_or(0);
        // Expansion probability = fraction of the served body that was not
        // already present at call start, in per-mille.
        let p_milli = if output_visible > 0 {
            u32::try_from(
                output_visible
                    .saturating_sub(input_visible)
                    .saturating_mul(1000)
                    / output_visible.max(1),
            )
            .unwrap_or(0)
        } else {
            0
        };
        assert!(p_milli <= 1000, "probability must stay in per-mille");
        // A 1 MB payload with this predicted probability and a 3-read
        // horizon must clear a small handling cost.
        let decision = estimator.decide_horizon_cost(1 << 20, Some(p_milli), Some(3), 32);
        assert!(
            decision.admit_exact_ref,
            "1MB payload must be admitted at p={p_milli} milli: {decision:?}"
        );
        assert_eq!(decision.expansion_probability_milli, p_milli);
    }
}

/// Policy-disable test beyond raw/Exact bypass: the HorizonCost policy is
/// opt-in, and the default ByteThreshold policy restores the exact legacy
/// threshold behavior on the engine read path.
#[test]
fn policy_disable_restores_byte_threshold_on_engine_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.txt");
    let payload = "x".repeat(512);
    std::fs::write(&path, &payload).unwrap();

    // HorizonCost with an aggressive estimator: 512-byte payload (~128
    // tokens), p=0, horizon=16 -> savings = 16 * 128 tokens, far above any
    // ref handling cost -> ExactRef even though the payload is far below
    // the byte threshold.
    let mut aggressive = EngineConfig::for_root(dir.path());
    aggressive.capsule_exact_ref_threshold_bytes = 8 * 1024;
    aggressive.admission_policy = AdmissionPolicy::HorizonCost;
    aggressive.admission_estimator = AdmissionEstimator {
        exact_ref_threshold_bytes: 8 * 1024,
        default_expansion_probability_milli: 0,
        default_horizon: 16,
    };
    aggressive.session_dedup = false;
    let engine = TokenZeroEngine::new(aggressive);
    let response = engine.read(&[path.clone()], Mode::Auto, None, None, false, 1, 4000);
    let visible = response.visible.unwrap().text;
    assert!(
        !visible.contains(&payload),
        "estimator must admit the ref below the byte threshold: {visible}"
    );

    // Policy disabled (default): the same payload stays inline.
    let mut legacy = EngineConfig::for_root(dir.path());
    legacy.capsule_exact_ref_threshold_bytes = 8 * 1024;
    legacy.admission_policy = AdmissionPolicy::ByteThreshold;
    legacy.session_dedup = false;
    let engine = TokenZeroEngine::new(legacy);
    let response = engine.read(&[path], Mode::Auto, None, None, false, 1, 4000);
    let visible = response.visible.unwrap().text;
    assert!(
        visible.contains(&payload),
        "disabled policy must keep the legacy inline behavior: {visible}"
    );
}

/// Explicit modes bypass admission entirely (legacy contract): even the
/// aggressive estimator must not move an explicit-mode read to ExactRef.
#[test]
fn explicit_modes_bypass_estimator_like_legacy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("explicit.txt");
    std::fs::write(&path, "explicit mode payload").unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.admission_policy = AdmissionPolicy::HorizonCost;
    config.admission_estimator = AdmissionEstimator {
        exact_ref_threshold_bytes: 1,
        default_expansion_probability_milli: 0,
        default_horizon: 100,
    };
    config.session_dedup = false;
    let engine = TokenZeroEngine::new(config);
    // Passthrough is the verbatim-payload contract: the estimator must not
    // replace the payload with a ref, regardless of how aggressive it is.
    let response = engine.read(&[path], Mode::Passthrough, None, None, false, 1, 4000);
    assert!(
        response
            .visible
            .unwrap()
            .text
            .contains("explicit mode payload"),
        "explicit mode must bypass the estimator"
    );
}
