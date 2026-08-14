use tokenzero_engine::{
    ANTHROPIC_CACHE_DIAGNOSIS_BETA, AnthropicCacheDiagnosisRequest, CacheMeter, CacheMeterError,
    CacheObservation, CachePricing, CacheProvider, ProviderCacheEligibility,
    ProviderCacheEligibilityStatus, ProviderCacheTelemetry, cache_miss_attribution,
    parse_provider_usage,
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
fn normalizes_anthropic_openai_and_gemini_usage() {
    let cases = [
        (
            CacheProvider::Anthropic,
            serde_json::json!({"usage":{"input_tokens":80,"output_tokens":9,"cache_read_input_tokens":15,"cache_creation_input_tokens":5}}),
            (80, 15, 5),
        ),
        (
            CacheProvider::OpenAi,
            serde_json::json!({"usage":{"prompt_tokens":100,"completion_tokens":9,"prompt_tokens_details":{"cached_tokens":25}}}),
            (75, 25, 0),
        ),
        (
            CacheProvider::Gemini,
            serde_json::json!({"usageMetadata":{"promptTokenCount":120,"candidatesTokenCount":9,"cachedContentTokenCount":20}}),
            (100, 20, 0),
        ),
    ];
    for (provider, value, expected) in cases {
        let usage = parse_provider_usage(provider, &value).unwrap();
        assert_eq!(
            (
                usage.input_tokens,
                usage.cache_read_input_tokens,
                usage.cache_creation_input_tokens
            ),
            expected
        );
        assert_eq!(
            usage.total_input_tokens(),
            expected.0 + expected.1 + expected.2
        );
        assert!(usage.cache_read_input_tokens_reported);
    }
}
#[test]
fn session_report_emits_psr_churn_hit_rate_and_realized_cost() {
    let mut meter = CacheMeter::default();
    meter
        .observe(
            CacheProvider::Anthropic,
            "stable prefix alpha",
            &serde_json::json!({"usage":{"input_tokens":10,"cache_read_input_tokens":0}}),
            pricing(),
            None,
        )
        .unwrap();
    meter.observe(CacheProvider::OpenAi, "stable prefix beta", &serde_json::json!({"usage":{"prompt_tokens":20,"prompt_tokens_details":{"cached_tokens":10}}}), pricing(), None).unwrap();
    let report = meter.report();
    assert_eq!(report.requests, 2);
    assert!(report.prefix_stability_ratio > 0.0 && report.prefix_stability_ratio < 1.0);
    assert!(report.average_churn_depth_tokens > 0.0);
    assert!((report.hit_rate - (10.0 / 30.0)).abs() < 1e-12);
    assert!(report.realized_dollars_per_request > 0.0);
}
#[test]
fn anthropic_cache_diagnosis_contract_carries_previous_message_id() {
    let request = AnthropicCacheDiagnosisRequest {
        previous_message_id: "msg_sanitized_previous".into(),
    };
    assert_eq!(
        request.headers(),
        [("anthropic-beta", ANTHROPIC_CACHE_DIAGNOSIS_BETA)]
    );
    assert_eq!(
        request.body()["previous_message_id"],
        "msg_sanitized_previous"
    );
    let response = serde_json::json!({"cache_diagnosis":{"cache_miss_reason":"prefix_changed"}});
    assert_eq!(
        cache_miss_attribution(&response).as_deref(),
        Some("prefix_changed")
    );
}
#[test]
fn sanitized_real_session_demo_replays_all_provider_shapes() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/cache-meter-session-demo.json")).unwrap();
    assert_eq!(fixture["source"]["session_id"], "tz-39598-18c1624c22cf65e8");
    let mut meter = CacheMeter::default();
    for turn in fixture["turns"].as_array().unwrap() {
        let provider = serde_json::from_value(turn["provider"].clone()).unwrap();
        meter
            .observe(
                provider,
                turn["request_projection"].as_str().unwrap(),
                &turn["response"],
                pricing(),
                turn.get("diagnosis"),
            )
            .unwrap();
    }
    let report = meter.report();
    assert_eq!(report.requests, 3);
    assert!(report.prefix_stability_ratio > 0.0);
    assert!(report.hit_rate > 0.0);
    assert_eq!(report.exact_miss_attributions, ["prefix_changed"]);
}

#[test]
fn legacy_observation_deserializes_to_unknown_eligibility_and_unavailable_telemetry() {
    let observation: CacheObservation = serde_json::from_value(serde_json::json!({
        "provider": "open_ai",
        "request_tokens": 10,
        "stable_prefix_tokens": 5,
        "churn_depth_tokens": 5,
        "usage": {
            "input_tokens": 10,
            "output_tokens": 1,
            "cache_read_input_tokens": 0,
            "cache_creation_input_tokens": 0
        },
        "realized_dollars": 0.01
    }))
    .unwrap();
    assert_eq!(
        observation.eligibility.status(),
        ProviderCacheEligibilityStatus::NotEvaluated
    );
    assert_eq!(
        observation.provider_telemetry,
        ProviderCacheTelemetry::Unavailable
    );
    assert!(!observation.usage.cache_read_input_tokens_reported);
}

#[test]
fn absent_cache_tokens_are_unavailable_and_excluded_from_hit_denominators() {
    let missing = parse_provider_usage(
        CacheProvider::OpenAi,
        &serde_json::json!({"usage":{"prompt_tokens":1_000}}),
    )
    .unwrap();
    assert_eq!(missing.cache_read_input_tokens, 0);
    assert!(!missing.cache_read_input_tokens_reported);

    let mut meter = CacheMeter::default();
    meter
        .observe(
            CacheProvider::OpenAi,
            "stable prefix",
            &serde_json::json!({"usage":{"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":50}}}),
            pricing(),
            None,
        )
        .unwrap();
    let unavailable = meter
        .observe(
            CacheProvider::OpenAi,
            "stable prefix",
            &serde_json::json!({"usage":{"prompt_tokens":1_000}}),
            pricing(),
            None,
        )
        .unwrap();
    assert!(unavailable.stable_prefix_tokens > 0);
    assert_eq!(
        unavailable.eligibility.status(),
        ProviderCacheEligibilityStatus::NotEvaluated
    );
    assert_eq!(
        unavailable.provider_telemetry,
        ProviderCacheTelemetry::Unavailable
    );

    let report = meter.report();
    assert_eq!(report.requests, 2);
    assert_eq!(report.provider_telemetry_requests, 1);
    assert_eq!(report.provider_reported_hit_requests, 1);
    assert_eq!(report.provider_unavailable_requests, 1);
    assert_eq!(report.provider_reported_hit_rate, Some(1.0));
    assert_eq!(report.provider_reported_cached_token_ratio, Some(0.5));
    assert!((report.hit_rate - 50.0 / 1_100.0).abs() < 1e-12);
    assert_eq!(report.prefix_eligibility_rate, None);
    assert!(report.cache_uptime.provider_measurement_available);
    assert_eq!(report.cache_uptime.error_budget_consumed_tokens, 50);
}

#[test]
fn provider_eligibility_and_reported_results_remain_independent() {
    let mut meter = CacheMeter::default();
    let observation = meter
        .observe_with_eligibility(
            CacheProvider::Anthropic,
            "request",
            ProviderCacheEligibility::ineligible(
                "anthropic-cache-policy-v1",
                "below provider minimum",
            )
            .unwrap(),
            &serde_json::json!({"usage":{"input_tokens":50,"cache_read_input_tokens":50}}),
            pricing(),
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        observation.eligibility.status(),
        ProviderCacheEligibilityStatus::Ineligible
    );
    assert_eq!(
        observation.provider_telemetry,
        ProviderCacheTelemetry::ReportedHit {
            cached_input_tokens: 50.try_into().unwrap()
        }
    );

    let report = meter.report();
    assert_eq!(report.eligibility_evaluated_requests, 1);
    assert_eq!(report.eligible_requests, 0);
    assert_eq!(report.prefix_eligibility_rate, Some(0.0));
    assert_eq!(report.provider_reported_hit_requests, 1);
    assert_eq!(report.provider_reported_hit_rate, Some(1.0));
}

#[test]
fn ttft_route_model_and_cache_key_round_trip_and_absence_stays_absent() {
    let mut meter = CacheMeter::default();
    let observed = meter
        .observe_with_eligibility(
            CacheProvider::Anthropic,
            "stable prefix",
            ProviderCacheEligibility::ineligible(
                "anthropic-cache-policy-v1",
                "telemetry test does not evaluate eligibility",
            )
            .unwrap(),
            &serde_json::json!({
                "model": "claude-3-7-sonnet-20250219",
                "ttft": 412,
                "usage": {"input_tokens": 100, "cache_read_input_tokens": 100}
            }),
            pricing(),
            None,
            Some("anthropic-cache:req-7f3a"),
        )
        .unwrap();
    assert_eq!(observed.ttft_ms, Some(412));
    assert_eq!(
        observed.model.as_deref(),
        Some("claude-3-7-sonnet-20250219")
    );
    assert_eq!(observed.route.as_deref(), Some("messages"));
    assert_eq!(observed.cache_key.as_deref(), Some("anthropic-cache:req-7f3a"));
    let round_trip: CacheObservation =
        serde_json::from_value(serde_json::to_value(observed).unwrap()).unwrap();
    assert_eq!(&round_trip, observed);

    // Absence is recorded as absent: never defaulted to zero or a name.
    let missing = meter
        .observe(
            CacheProvider::OpenAi,
            "stable prefix",
            &serde_json::json!({"usage":{"prompt_tokens":50}}),
            pricing(),
            None,
        )
        .unwrap();
    assert_eq!(missing.ttft_ms, None);
    assert_eq!(missing.model, None);
    assert_eq!(missing.cache_key, None);
    assert_eq!(missing.route.as_deref(), Some("chat.completions"));
    let serialized = serde_json::to_value(missing).unwrap();
    assert!(serialized.get("ttft_ms").is_none());
    assert!(serialized.get("model").is_none());
    assert!(serialized.get("cache_key").is_none());
}

#[test]
fn contradictory_provider_cache_receipts_fail_loudly() {
    let cases = [
        (1, "expired", "positive cached tokens reported with expiry"),
        (
            0,
            "unknown",
            "known cached-token count reported with unknown status",
        ),
        (
            1,
            "provider_unknown",
            "known cached-token count reported with unknown status",
        ),
    ];
    for (cached_input_tokens, reason, expected) in cases {
        let mut meter = CacheMeter::default();
        let error = meter
            .observe(
                CacheProvider::Anthropic,
                "request",
                &serde_json::json!({"usage":{
                    "input_tokens":100,
                    "cache_read_input_tokens":cached_input_tokens
                }}),
                pricing(),
                Some(&serde_json::json!({"cache_diagnosis":{
                    "cache_miss_reason":reason
                }})),
            )
            .unwrap_err();
        assert_eq!(error, CacheMeterError::ContradictoryTelemetry(expected));
        assert!(meter.observations().is_empty());
    }
}

#[test]
fn provider_expiry_is_not_counted_as_a_reported_miss() {
    let mut meter = CacheMeter::default();
    let observation = meter
        .observe(
            CacheProvider::Anthropic,
            "request",
            &serde_json::json!({"usage":{"input_tokens":100,"cache_read_input_tokens":0}}),
            pricing(),
            Some(&serde_json::json!({"cache_diagnosis":{"cache_miss_reason":"expired"}})),
        )
        .unwrap();
    assert_eq!(
        observation.provider_telemetry,
        ProviderCacheTelemetry::Expired
    );
    let report = meter.report();
    assert_eq!(report.provider_expired_requests, 1);
    assert_eq!(report.provider_telemetry_requests, 0);
    assert_eq!(report.provider_reported_miss_requests, 0);
    assert_eq!(report.provider_reported_hit_rate, None);
    assert_eq!(report.provider_reported_cached_token_ratio, None);
    assert!(!report.cache_uptime.provider_measurement_available);
    assert!(!report.rate_limit_multiplier_certified);
}

#[test]
fn unknown_without_cached_token_field_remains_unknown_not_miss() {
    let mut meter = CacheMeter::default();
    let observation = meter
        .observe(
            CacheProvider::Anthropic,
            "request",
            &serde_json::json!({"usage":{"input_tokens":100}}),
            pricing(),
            Some(&serde_json::json!({"cache_diagnosis":{"cache_miss_reason":"unknown"}})),
        )
        .unwrap();
    assert_eq!(
        observation.provider_telemetry,
        ProviderCacheTelemetry::ReportedUnknown
    );
    let report = meter.report();
    assert_eq!(report.provider_reported_unknown_requests, 1);
    assert_eq!(report.provider_telemetry_requests, 0);
    assert_eq!(report.provider_reported_hit_rate, None);
}
