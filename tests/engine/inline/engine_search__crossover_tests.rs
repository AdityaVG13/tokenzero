use super::*;
use tempfile::tempdir;

/// Default `EmissionCrossoverConfig` must reproduce the historical
/// `pick_cheaper` emission choice exactly: compress iff the compact form is
/// strictly cheaper in tokens.
#[test]
fn default_emission_crossover_reproduces_pick_cheaper() {
    let dir = tempfile::tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    // Flat form crosses the cacheable floor (~1200 tokens).
    let flat = (0..300)
        .map(|i| format!("path/{i:03}/file.rs"))
        .collect::<Vec<_>>()
        .join("\n");
    let compact = "path/ (300 files)\n";
    assert!(count_tokens(&flat) >= 1_000, "flat must cross the floor");
    let receipt = engine.decide_emission_crossover(&flat, &compact);
    let (_, grouped) = pick_cheaper(&flat, &compact);
    match receipt.action {
        CacheCrossoverAction::Compress => assert!(grouped, "compact strictly cheaper"),
        CacheCrossoverAction::CacheStable => assert!(!grouped),
        CacheCrossoverAction::KeepInline => assert!(!grouped),
    }
    assert!(receipt.cache_eligible);
}

/// Configured cache economics change the emission decision deliberately:
/// with d=0.1 and a reuse horizon, the flat stable form beats repeated
/// compaction when the compact form is only mildly cheaper in raw tokens.
#[test]
fn configured_cache_economics_prefer_flat_stable_form() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.emission_crossover.cached_read_multiplier_ppm = 100_000; // d = 0.1
    config.emission_crossover.remaining_reuse_horizon = 3;
    let engine = TokenZeroEngine::new(config);

    // Flat crosses the floor; grouped is mildly cheaper (60% of flat), so
    // pick_cheaper picks grouped, but the cached stable form wins under
    // d=0.1: cached_projected = 3 * 0.1 * flat < 3 * 0.6 * flat.
    let flat = (0..300)
        .map(|i| format!("path/{i:03}/file.rs"))
        .collect::<Vec<_>>()
        .join("\n");
    let compact = (0..300)
        .map(|i| format!("path/{i:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(count_tokens(&compact) < count_tokens(&flat));
    let receipt = engine.decide_emission_crossover(&flat, &compact);
    assert_eq!(
        receipt.action,
        CacheCrossoverAction::CacheStable,
        "cached stable form must beat per-call compaction: {receipt:?}"
    );
    assert_eq!(
        receipt.reason,
        CacheCrossoverReason::CachedStableCheaperOrEqual
    );
    // The default config must reach the opposite conclusion for the same
    // pair: pure token comparison picks the compact form.
    let legacy = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let legacy_receipt = legacy.decide_emission_crossover(&flat, &compact);
    assert_eq!(
        legacy_receipt.action,
        CacheCrossoverAction::Compress,
        "default config must keep the historical grouped choice"
    );
}

/// End-to-end glob emission: the crossover action drives the visible output
/// strategy, and the receipt is observable in telemetry. Two deep roots with
/// short relative names make the grouped rewrite strictly cheaper, so the
/// crossover picks Compress (below the cacheable floor, inline is more
/// expensive than the grouped form).
#[test]
fn glob_emission_consults_crossover() {
    let dir = tempfile::tempdir().unwrap();
    let root_a = dir.path().join("nest/a");
    let root_b = dir.path().join("nest/b");
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_b).unwrap();
    for i in 0..10 {
        std::fs::write(root_a.join(format!("f{i:02}.txt")), "content\n").unwrap();
        std::fs::write(root_b.join(format!("f{i:02}.txt")), "content\n").unwrap();
    }
    let config = EngineConfig::for_root(dir.path());
    let engine = TokenZeroEngine::new(config);
    let response = engine.glob(
        "*.txt",
        &[root_a.clone(), root_b.clone()],
        false,
        Mode::Auto,
        200,
        4000,
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    let telemetry = response.telemetry.as_ref().expect("glob telemetry");
    assert_eq!(telemetry["crossover_action"], "compress");
    assert_eq!(telemetry["crossover_reason"], "below_cacheable_floor");
    assert_eq!(
        telemetry["output_strategy"], "grouped_by_root",
        "compress emission serves the grouped artifact"
    );
    let visible = response.visible.unwrap().text;
    assert!(visible.contains("# root:"), "{visible}");
    assert!(visible.contains("f00.txt"), "{visible}");
}
