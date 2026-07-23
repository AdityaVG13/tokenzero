#[test]
fn boot_cost_lock_covers_real_small_and_23k_corpora() {
    let baseline: serde_json::Value = serde_json::from_str(include_str!(
        "../../../benchmarks/boot-cost/baseline.json",
    ))
    .expect("boot-cost baseline JSON");
    let thresholds = &baseline["thresholds"];
    assert_eq!(thresholds["max_visible_boot_tokens_exclusive"], 100);
    assert_eq!(thresholds["max_repo_size_growth_tokens"], 0);

    let small_name = thresholds["small_corpus"].as_str().expect("small corpus");
    let large_name = thresholds["large_corpus"].as_str().expect("large corpus");
    let small = baseline["components"][small_name]["total"]
        .as_u64()
        .expect("small total");
    let large = baseline["components"][large_name]["total"]
        .as_u64()
        .expect("large total");
    assert!(small < 100, "small-corpus boot cost exceeds lock: {small}");
    assert!(large < 100, "23k-corpus boot cost exceeds lock: {large}");
    assert_eq!(large.saturating_sub(small), 0, "boot cost grew with repo size");

    let gate = include_str!("../../../benchmarks/boot-cost.py");
    assert!(gate.contains("offending_component"));
    assert!(gate.contains("synthetic-23k"));
    assert!(gate.contains("--rebaseline"));
}
