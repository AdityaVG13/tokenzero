use tokenzero_engine::{
    ANTHROPIC_CACHE_DIAGNOSIS_BETA, AnthropicCacheDiagnosisRequest, CacheMeter,
    CachePricing, CacheProvider, cache_miss_attribution, parse_provider_usage,
};
fn pricing() -> CachePricing { CachePricing { input_per_million: 3.0, cache_read_per_million: 0.3, cache_creation_per_million: 3.75, output_per_million: 15.0 } }
#[test]
fn normalizes_anthropic_openai_and_gemini_usage() {
    let cases = [
        (CacheProvider::Anthropic, serde_json::json!({"usage":{"input_tokens":80,"output_tokens":9,"cache_read_input_tokens":15,"cache_creation_input_tokens":5}}), (80, 15, 5)),
        (CacheProvider::OpenAi, serde_json::json!({"usage":{"prompt_tokens":100,"completion_tokens":9,"prompt_tokens_details":{"cached_tokens":25}}}), (75, 25, 0)),
        (CacheProvider::Gemini, serde_json::json!({"usageMetadata":{"promptTokenCount":120,"candidatesTokenCount":9,"cachedContentTokenCount":20}}), (100, 20, 0)),
    ];
    for (provider, value, expected) in cases {
        let usage = parse_provider_usage(provider, &value).unwrap();
        assert_eq!((usage.input_tokens, usage.cache_read_input_tokens, usage.cache_creation_input_tokens), expected);
        assert_eq!(usage.total_input_tokens(), expected.0 + expected.1 + expected.2);
    }
}
#[test]
fn session_report_emits_psr_churn_hit_rate_and_realized_cost() {
    let mut meter = CacheMeter::default();
    meter.observe(CacheProvider::Anthropic, "stable prefix alpha", &serde_json::json!({"usage":{"input_tokens":10,"cache_read_input_tokens":0}}), pricing(), None).unwrap();
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
    let request = AnthropicCacheDiagnosisRequest { previous_message_id: "msg_sanitized_previous".into() };
    assert_eq!(request.headers(), [("anthropic-beta", ANTHROPIC_CACHE_DIAGNOSIS_BETA)]);
    assert_eq!(request.body()["previous_message_id"], "msg_sanitized_previous");
    let response = serde_json::json!({"cache_diagnosis":{"cache_miss_reason":"prefix_changed"}});
    assert_eq!(cache_miss_attribution(&response).as_deref(), Some("prefix_changed"));
}
#[test]
fn sanitized_real_session_demo_replays_all_provider_shapes() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!("fixtures/cache-meter-session-demo.json")).unwrap();
    assert_eq!(fixture["source"]["session_id"], "tz-39598-18c1624c22cf65e8");
    let mut meter = CacheMeter::default();
    for turn in fixture["turns"].as_array().unwrap() {
        let provider = serde_json::from_value(turn["provider"].clone()).unwrap();
        meter.observe(provider, turn["request_projection"].as_str().unwrap(), &turn["response"], pricing(), turn.get("diagnosis")).unwrap();
    }
    let report = meter.report();
    assert_eq!(report.requests, 3);
    assert!(report.prefix_stability_ratio > 0.0);
    assert!(report.hit_rate > 0.0);
    assert_eq!(report.exact_miss_attributions, ["prefix_changed"]);
}
