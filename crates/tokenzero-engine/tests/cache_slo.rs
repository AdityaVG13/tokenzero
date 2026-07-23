use tokenzero_engine::{CacheMeter, CachePricing, CacheProvider, CacheSloConfig};

fn pricing() -> CachePricing {
    CachePricing { input_per_million: 3.0, cache_read_per_million: 0.3, cache_creation_per_million: 3.75, output_per_million: 15.0 }
}

#[test]
fn injected_uptime_regression_trips_burn_alert_and_reports_multiplier() {
    let mut meter = CacheMeter::default();
    meter.observe(CacheProvider::OpenAi, "novel request", &serde_json::json!({"usage":{"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":10}}}), pricing(), None).unwrap();
    let report = meter.report_with_slo(CacheSloConfig { target_hit_rate: 0.8, regression_hit_rate: 0.5, alpha: 0.05, novelty_budget_tokens: 50 }, Some(1_250)).unwrap();
    assert!(report.cache_uptime.burn_alert);
    assert_eq!(report.cache_uptime.error_budget_consumed_tokens, 90);
    assert_eq!(report.cache_uptime.novelty_budget_remaining_tokens, 0);
    assert_eq!(report.token_amplification_milli, Some(1_250));
    assert!((report.effective_rate_limit_multiplier - 100.0 / 90.0).abs() < 1e-12);
    let dashboard = serde_json::to_value(report).unwrap();
    assert_eq!(dashboard["cache_uptime"]["burn_alert"], true);
    assert!(dashboard.get("effective_rate_limit_multiplier").is_some());
}

#[test]
fn healthy_uptime_preserves_session_budgets() {
    let mut meter = CacheMeter::default();
    meter.observe(CacheProvider::OpenAi, "stable request", &serde_json::json!({"usage":{"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":90}}}), pricing(), None).unwrap();
    let report = meter.report_with_slo(CacheSloConfig { novelty_budget_tokens: 25, ..CacheSloConfig::default() }, None).unwrap();
    assert!(!report.cache_uptime.burn_alert);
    assert_eq!(report.cache_uptime.error_budget_remaining_tokens, 9);
    assert_eq!(report.cache_uptime.novelty_budget_remaining_tokens, 15);
    assert!((report.effective_rate_limit_multiplier - 10.0).abs() < 1e-12);
}
