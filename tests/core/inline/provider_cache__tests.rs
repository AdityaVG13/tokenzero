use super::*;

#[test]
fn provider_telemetry_presence_never_turns_unavailable_into_miss() {
    assert_eq!(
        ProviderCacheTelemetry::from_reported_cached_tokens(None),
        ProviderCacheTelemetry::Unavailable
    );
    assert_eq!(
        ProviderCacheTelemetry::from_reported_cached_tokens(Some(0)),
        ProviderCacheTelemetry::ReportedMiss
    );
    assert_eq!(
        ProviderCacheTelemetry::from_reported_cached_tokens(Some(17)),
        ProviderCacheTelemetry::ReportedHit {
            cached_input_tokens: NonZeroU64::new(17).unwrap()
        }
    );
    assert!(!ProviderCacheTelemetry::Unavailable.is_hit_rate_observation());
    assert!(ProviderCacheTelemetry::ReportedMiss.is_hit_rate_observation());
}

#[test]
fn eligibility_requires_an_explicit_named_policy() {
    assert_eq!(
        ProviderCacheEligibility::ineligible("", "below provider floor"),
        Err(ProviderCacheError::EmptyPolicyId)
    );
    assert_eq!(
        ProviderCacheEligibility::ineligible("provider-v1", ""),
        Err(ProviderCacheError::EmptyIneligibilityReason)
    );
    let not_evaluated = ProviderCacheEligibility::not_evaluated();
    assert!(!not_evaluated.is_evaluated());
    assert!(!not_evaluated.is_eligible());
}

#[test]
fn serde_refuses_fabricated_or_inconsistent_cache_facts() {
    assert!(
        serde_json::from_value::<ProviderCacheTelemetry>(serde_json::json!({
            "status": "reported_hit",
            "cached_input_tokens": 0
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<ProviderCacheTelemetry>(serde_json::json!({
            "status": "reported_miss",
            "cached_input_tokens": 1
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<ProviderCacheEligibility>(serde_json::json!({
            "status": "eligible",
            "policy_id": "provider-v1",
            "prefix_geometry_digest": null,
            "breakpoint_after_tokens": 10,
            "reason": null
        }))
        .is_err()
    );
    let ineligible =
        ProviderCacheEligibility::ineligible("provider-v1", "below provider minimum").unwrap();
    let round_trip: ProviderCacheEligibility =
        serde_json::from_value(serde_json::to_value(&ineligible).unwrap()).unwrap();
    assert_eq!(round_trip, ineligible);
}
