use tokenzero_core::count_tokens;
use tokenzero_engine::{
    CACHE_CROSSOVER_SCHEMA, CacheContentClass, CacheCrossoverAction, CacheCrossoverError,
    CacheCrossoverInput, CacheCrossoverReason, CacheProvider, TOKEN_COST_PPM_SCALE,
    decide_cache_crossover,
};
use tokenzero_recovery::prefix_stability::CacheModelTier;

fn input(provider: CacheProvider) -> CacheCrossoverInput {
    CacheCrossoverInput {
        provider,
        policy_id: "provider-model-revision/10pct-cache-read".to_owned(),
        token_unit_id: "estimator:tokenzero-count-tokens/v1".to_owned(),
        content_class: CacheContentClass::Stable,
        original_tokens: 10_000,
        compressed_tokens: 1_000,
        compression_admission_id: Some("test:byte-exact-fixture/v1".to_owned()),
        common_overhead_tokens: 375,
        cached_read_multiplier_ppm: 100_000,
        min_cacheable_tokens: 1_000,
        suffix_size_tokens: 0,
        compaction_cost_tokens: 0,
        remaining_reuse_horizon: 1,
    }
}

#[test]
fn provider_table_uses_the_strict_ten_x_crossover_with_complete_work() {
    for provider in [
        CacheProvider::Anthropic,
        CacheProvider::OpenAi,
        CacheProvider::Gemini,
    ] {
        let mut candidate = input(provider);
        candidate.compressed_tokens = 999;
        let receipt = decide_cache_crossover(&candidate).unwrap();
        assert_eq!(receipt.action, CacheCrossoverAction::Compress);
        assert_eq!(
            receipt.reason,
            CacheCrossoverReason::CompressionStrictlyBeatsCache
        );
        assert_eq!(receipt.schema, CACHE_CROSSOVER_SCHEMA);
        assert_eq!(
            receipt.cached_total_token_cost_ppm,
            1_375 * u128::from(TOKEN_COST_PPM_SCALE)
        );
        assert_eq!(
            receipt.compressed_total_token_cost_ppm,
            1_374 * u128::from(TOKEN_COST_PPM_SCALE)
        );

        candidate.compressed_tokens = 1_000;
        let equality = decide_cache_crossover(&candidate).unwrap();
        assert_eq!(equality.action, CacheCrossoverAction::CacheStable);
        assert_eq!(
            equality.reason,
            CacheCrossoverReason::CachedStableCheaperOrEqual
        );
        assert_eq!(
            equality.compressed_total_token_cost_ppm,
            equality.cached_total_token_cost_ppm
        );

        candidate.compressed_tokens = 1_001;
        assert_eq!(
            decide_cache_crossover(&candidate).unwrap().action,
            CacheCrossoverAction::CacheStable
        );
    }
}

#[test]
fn churn_and_below_floor_content_never_claim_cache_eligibility() {
    let floor = CacheModelTier::FableOrSonnet46
        .min_cacheable_estimator_tokens()
        .try_into()
        .unwrap();
    let mut candidate = input(CacheProvider::Anthropic);
    candidate.original_tokens = floor - 1;
    candidate.compressed_tokens = 100;
    candidate.min_cacheable_tokens = floor;
    let below_floor = decide_cache_crossover(&candidate).unwrap();
    assert!(!below_floor.cache_eligible);
    assert_eq!(below_floor.action, CacheCrossoverAction::Compress);
    assert_eq!(
        below_floor.reason,
        CacheCrossoverReason::BelowCacheableFloor
    );

    candidate.compressed_tokens = candidate.original_tokens + 1;
    assert_eq!(
        decide_cache_crossover(&candidate).unwrap().action,
        CacheCrossoverAction::KeepInline
    );

    candidate.compression_admission_id = None;
    candidate.compressed_tokens = 1;
    let unadmitted = decide_cache_crossover(&candidate).unwrap();
    assert_eq!(unadmitted.action, CacheCrossoverAction::KeepInline);
    assert_eq!(
        unadmitted.reason,
        CacheCrossoverReason::CompressionNotAdmitted
    );
    candidate.compression_admission_id = Some("test:byte-exact-fixture/v1".to_owned());

    candidate.content_class = CacheContentClass::Churn;
    candidate.original_tokens = floor + 1;
    candidate.compressed_tokens = 100;
    let churn = decide_cache_crossover(&candidate).unwrap();
    assert!(!churn.cache_eligible);
    assert_eq!(churn.action, CacheCrossoverAction::Compress);
    assert_eq!(churn.reason, CacheCrossoverReason::ChurnIsNotCacheable);

    candidate.compressed_tokens = candidate.original_tokens;
    assert_eq!(
        decide_cache_crossover(&candidate).unwrap().action,
        CacheCrossoverAction::KeepInline
    );
}

#[test]
fn repository_corpus_measurement_validates_the_compression_side() {
    let source = include_str!("../../crates/tokenzero-engine/src/cache_meter.rs");
    let raw_corpus = source.repeat(12);
    let compressed = format!("repeat:12\n{source}");
    let recovered = compressed
        .strip_prefix("repeat:12\n")
        .expect("fixture codec header")
        .repeat(12);
    assert_eq!(recovered.as_bytes(), raw_corpus.as_bytes());
    let raw_tokens = u64::try_from(count_tokens(&raw_corpus)).unwrap();
    let compressed_tokens = u64::try_from(count_tokens(&compressed)).unwrap();
    let floor =
        u64::try_from(CacheModelTier::FableOrSonnet46.min_cacheable_estimator_tokens()).unwrap();
    assert!(
        raw_tokens >= floor,
        "repository corpus must cross the cache floor"
    );

    let raw_ratio_hundredths = u128::from(raw_tokens) * 100;
    let compressed_ratio_low = u128::from(compressed_tokens) * 1_198;
    let compressed_ratio_high = u128::from(compressed_tokens) * 1_200;
    assert!(
        (compressed_ratio_low..=compressed_ratio_high).contains(&raw_ratio_hundredths),
        "byte-exact repeat codec drifted outside 11.98x..=12.00x: raw={raw_tokens} compressed={compressed_tokens}"
    );

    let candidate = CacheCrossoverInput {
        provider: CacheProvider::OpenAi,
        policy_id: "openai/gpt-5.6/revision-pinned/10pct".to_owned(),
        token_unit_id: "estimator:tokenzero-count-tokens/v1".to_owned(),
        content_class: CacheContentClass::Stable,
        original_tokens: raw_tokens,
        compressed_tokens,
        compression_admission_id: Some("test:byte-exact-repeat-codec/v1".to_owned()),
        common_overhead_tokens: 211,
        cached_read_multiplier_ppm: 100_000,
        min_cacheable_tokens: floor,
        suffix_size_tokens: 0,
        compaction_cost_tokens: 0,
        remaining_reuse_horizon: 1,
    };
    let receipt = decide_cache_crossover(&candidate).unwrap();
    assert_eq!(receipt.action, CacheCrossoverAction::Compress);
    assert!(
        receipt.compressed_total_token_cost_ppm < receipt.cached_total_token_cost_ppm,
        "measured corpus must remain beyond the strict crossover"
    );
    let wire = serde_json::to_value(&receipt).unwrap();
    assert_eq!(wire["schema"], CACHE_CROSSOVER_SCHEMA);
    assert_eq!(wire["policy_id"], candidate.policy_id);
}

/// The three V6-T6 inputs at their defaults must reproduce the legacy
/// single-read receipts exactly: projected costs equal the single-read
/// totals and the action is unchanged.
#[test]
fn horizon_defaults_reproduce_legacy_single_read_receipts() {
    let candidate = input(CacheProvider::Anthropic);
    let receipt = decide_cache_crossover(&candidate).unwrap();
    assert_eq!(receipt.remaining_reuse_horizon, 1);
    assert_eq!(receipt.suffix_size_tokens, 0);
    assert_eq!(receipt.compaction_cost_tokens, 0);
    assert_eq!(
        receipt.inline_projected_token_cost_ppm,
        receipt.inline_total_token_cost_ppm
    );
    assert_eq!(
        receipt.compressed_projected_token_cost_ppm,
        receipt.compressed_total_token_cost_ppm
    );
    assert_eq!(
        receipt.cached_projected_token_cost_ppm,
        receipt.cached_total_token_cost_ppm
    );
    // Legacy outcome for this input: 10000 tokens at d=0.1 beats 1000
    // compressed.
    assert_eq!(receipt.action, CacheCrossoverAction::CacheStable);
}

/// A mutable suffix is paid at full cost on every read and shrinks the
/// cacheable prefix below the floor: the crossover must stop claiming cache
/// eligibility and fall back to compress-vs-inline.
#[test]
fn suffix_shrinks_cacheable_prefix_and_flips_eligibility() {
    let mut candidate = input(CacheProvider::Anthropic);
    candidate.suffix_size_tokens = 9_900; // only 100 tokens cacheable
    candidate.compressed_tokens = 10_000; // no compression win either
    let receipt = decide_cache_crossover(&candidate).unwrap();
    assert!(!receipt.cache_eligible);
    assert_eq!(receipt.action, CacheCrossoverAction::KeepInline);
    assert_eq!(receipt.reason, CacheCrossoverReason::BelowCacheableFloor);
    // Single-read cached total pays the suffix at full cost.
    assert_eq!(
        receipt.cached_total_token_cost_ppm,
        375 * u128::from(TOKEN_COST_PPM_SCALE)
            + 9_900 * u128::from(TOKEN_COST_PPM_SCALE)
            + 100 * u128::from(100_000_u64)
    );
}

/// The one-time compaction cost amortizes over the reuse horizon: a rewrite
/// that is not worth it for a single read becomes the cheapest candidate
/// when the compact stable form is re-read many times.
#[test]
fn compaction_cost_amortizes_over_reuse_horizon() {
    let mut candidate = input(CacheProvider::Anthropic);
    // d = 0.5 cached read; compact per-read slightly cheaper than cached
    // read, but the rewrite itself costs more than the single-read saving.
    candidate.cached_read_multiplier_ppm = 500_000;
    candidate.original_tokens = 10_000;
    candidate.compressed_tokens = 4_900;
    candidate.compaction_cost_tokens = 300;
    candidate.suffix_size_tokens = 0;

    candidate.remaining_reuse_horizon = 1;
    let single = decide_cache_crossover(&candidate).unwrap();
    // Single read: cache (0.5 * 10000 = 5000) beats compress (300 + 4900).
    assert_eq!(single.action, CacheCrossoverAction::CacheStable);

    candidate.remaining_reuse_horizon = 10;
    let horizon = decide_cache_crossover(&candidate).unwrap();
    // Ten reads: the rewrite amortizes and the cheaper per-read compact
    // form (4900) beats ten cached reads (10 * 5000).
    assert_eq!(horizon.action, CacheCrossoverAction::Compress);
    assert_eq!(
        horizon.reason,
        CacheCrossoverReason::CompressionStrictlyBeatsCache
    );
    assert_eq!(
        horizon.compressed_projected_token_cost_ppm,
        375 * u128::from(TOKEN_COST_PPM_SCALE)
            + 300 * u128::from(TOKEN_COST_PPM_SCALE)
            + 10 * 4_900 * u128::from(TOKEN_COST_PPM_SCALE)
    );
}

/// Replay experiment: run the crossover over the measured token counts in
/// `token-amplification-replay.json` with realistic cache economics and
/// assert every case decides sanely (either cache-eligible or honestly
/// below the floor, always settling on a cache or compress action).
#[test]
fn replay_fixture_drives_crossover_experiment() {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/token-amplification-replay.json")).unwrap();
    let cases = value["cases"].as_array().expect("replay cases");
    assert!(!cases.is_empty());
    for case in cases {
        let input = &case["input"];
        let output = &case["output"];
        let original = output["billed"].as_u64().unwrap_or(0);
        // The amplified body is the stable artifact; a plausible compact
        // rewrite keeps the pointer/decision atoms and drops novel bytes.
        let compressed = input["billed"].as_u64().unwrap_or(0).saturating_add(1);
        assert!(compressed < original, "replay compact form must be cheaper");
        let receipt = decide_cache_crossover(&CacheCrossoverInput {
            provider: CacheProvider::Anthropic,
            policy_id: "replay:token-amplification/v1".to_owned(),
            token_unit_id: "estimator:tokenzero-count-tokens/v1".to_owned(),
            content_class: CacheContentClass::Stable,
            original_tokens: original,
            compressed_tokens: compressed,
            compression_admission_id: Some("replay:fixture-admitted/v1".to_owned()),
            common_overhead_tokens: 16,
            cached_read_multiplier_ppm: 100_000,
            min_cacheable_tokens: 10,
            suffix_size_tokens: 0,
            compaction_cost_tokens: 0,
            remaining_reuse_horizon: 1,
        })
        .unwrap();
        assert!(
            matches!(
                receipt.action,
                CacheCrossoverAction::CacheStable | CacheCrossoverAction::Compress
            ),
            "replay case must settle on a cache or compress action: {receipt:?}"
        );
        assert!(
            receipt.cache_eligible || receipt.reason == CacheCrossoverReason::BelowCacheableFloor,
            "replay case must be either cache-eligible or honestly below the floor"
        );
    }
}

/// New validation arms fail closed: a suffix larger than the content and a
/// zero reuse horizon are errors, never silent clamps.
#[test]
fn invalid_suffix_and_horizon_fail_closed() {
    let mut candidate = input(CacheProvider::Gemini);
    candidate.suffix_size_tokens = candidate.original_tokens + 1;
    assert_eq!(
        decide_cache_crossover(&candidate).unwrap_err(),
        CacheCrossoverError::InvalidSuffixSize
    );
    candidate.suffix_size_tokens = 0;
    candidate.remaining_reuse_horizon = 0;
    assert_eq!(
        decide_cache_crossover(&candidate).unwrap_err(),
        CacheCrossoverError::InvalidReuseHorizon
    );
}

#[test]
fn malformed_policy_inputs_fail_closed() {
    let mut candidate = input(CacheProvider::Gemini);
    let mut unknown = serde_json::to_value(&candidate).unwrap();
    unknown["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CacheCrossoverInput>(unknown).is_err());
    candidate.policy_id.clear();
    assert_eq!(
        decide_cache_crossover(&candidate),
        Err(CacheCrossoverError::InvalidPolicyId)
    );
    candidate.policy_id = "valid".to_owned();
    candidate.token_unit_id.clear();
    assert_eq!(
        decide_cache_crossover(&candidate),
        Err(CacheCrossoverError::InvalidTokenUnitId)
    );
    candidate.token_unit_id = "estimator:test/v1".to_owned();
    candidate.compression_admission_id = Some(String::new());
    assert_eq!(
        decide_cache_crossover(&candidate),
        Err(CacheCrossoverError::InvalidCompressionAdmissionId)
    );
    candidate.compression_admission_id = Some("test:exact/v1".to_owned());
    candidate.original_tokens = 0;
    assert_eq!(
        decide_cache_crossover(&candidate),
        Err(CacheCrossoverError::EmptyContent)
    );
    candidate.original_tokens = 1;
    candidate.cached_read_multiplier_ppm = 0;
    assert_eq!(
        decide_cache_crossover(&candidate),
        Err(CacheCrossoverError::InvalidCachedReadMultiplier)
    );
    candidate.cached_read_multiplier_ppm = TOKEN_COST_PPM_SCALE + 1;
    assert_eq!(
        decide_cache_crossover(&candidate),
        Err(CacheCrossoverError::InvalidCachedReadMultiplier)
    );
    candidate.cached_read_multiplier_ppm = 100_000;
    candidate.min_cacheable_tokens = 0;
    assert_eq!(
        decide_cache_crossover(&candidate),
        Err(CacheCrossoverError::InvalidCacheableFloor)
    );
}
