use tokenzero_core::count_tokens;
use tokenzero_engine::{
    CACHE_CROSSOVER_SCHEMA_V1, CacheContentClass, CacheCrossoverAction, CacheCrossoverError,
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
        assert_eq!(receipt.schema, CACHE_CROSSOVER_SCHEMA_V1);
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
    };
    let receipt = decide_cache_crossover(&candidate).unwrap();
    assert_eq!(receipt.action, CacheCrossoverAction::Compress);
    assert!(
        receipt.compressed_total_token_cost_ppm < receipt.cached_total_token_cost_ppm,
        "measured corpus must remain beyond the strict crossover"
    );
    let wire = serde_json::to_value(&receipt).unwrap();
    assert_eq!(wire["schema"], CACHE_CROSSOVER_SCHEMA_V1);
    assert_eq!(wire["policy_id"], candidate.policy_id);
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
