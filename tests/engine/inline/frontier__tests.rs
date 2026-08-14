use super::*;

fn object(
    root: &str,
    size: u64,
    weight: u64,
    latency: u64,
    invalidations: u64,
) -> FrontierPlanObject {
    FrontierPlanObject {
        object_root: root.to_string(),
        size_bytes: size,
        demand_weight: weight,
        valid: true,
        resident: false,
        estimated_latency_ms: latency,
        expected_invalidations: invalidations,
    }
}

fn budgets(capacity: u64, latency: u64, invalidations: u64) -> FrontierBudgets {
    FrontierBudgets {
        capacity_bytes: capacity,
        latency_budget_ms: latency,
        invalidation_budget: invalidations,
    }
}

/// The planner must never propose a resident set beyond capacity, and the
/// highest demand-density objects must be selected first.
#[test]
fn planner_respects_capacity_and_prefers_density() {
    let objects = vec![
        object("a", 1_000, 500, 10, 1), // density 0.5
        object("b", 2_000, 100, 5, 1),  // density 0.05
        object("c", 500, 400, 2, 1),    // density 0.8
        object("d", 3_000, 30, 20, 1),  // density 0.01
    ];
    let plan = plan_frontier_resident_set("l2", 0.9, &objects, &budgets(1_600, 10_000, 100));
    assert!(plan.resident_bytes <= plan.capacity_bytes, "plan: {plan:?}");
    assert_eq!(plan.resident_bytes, 1_500); // c (500) + a (1000) fit; b would overflow
    let mut resident = plan
        .objects
        .iter()
        .filter(|object| object.resident)
        .map(|object| object.object_root.as_str())
        .collect::<Vec<_>>();
    resident.sort();
    assert_eq!(resident, ["a", "c"]);
    // Demand (6500 bytes) cannot fit the 1600-byte budget, so the plan names
    // the shortfall instead of pretending to authorize it.
    assert!(
        plan.budget_violations.contains(&"capacity".to_string()),
        "{:?}",
        plan.budget_violations
    );
    // Proposal vocabulary mirrors the hub causal_residency_plan: optimizer
    // name, weights, capacity present.
    assert_eq!(plan.optimizer, FRONTIER_OPTIMIZER_NAME);
    assert_eq!(plan.total_demand_weight, 1_030);
    assert_eq!(plan.resident_valid_weight, 900);
}

/// Latency and invalidation budgets are hard constraints while building the
/// resident set, and budgets the demand cannot fit are reported as
/// violations instead of being silently ignored.
#[test]
fn planner_respects_latency_and_invalidation_budgets_and_reports_shortfalls() {
    let objects = vec![
        object("fast", 100, 100, 5, 1),
        object("slow", 100, 100, 200, 1),
        object("churn", 100, 100, 5, 50),
    ];
    // Latency budget keeps `slow` out; invalidation budget keeps `churn` out.
    let plan = plan_frontier_resident_set("l2", 0.5, &objects, &budgets(10_000, 20, 5));
    assert_eq!(plan.resident_bytes, 100);
    let resident = plan
        .objects
        .iter()
        .filter(|object| object.resident)
        .map(|object| object.object_root.as_str())
        .collect::<Vec<_>>();
    assert_eq!(resident, ["fast"]);
    // Total demand cannot fit inside the latency budget -> shortfall named.
    assert!(
        plan.budget_violations.contains(&"latency".to_string()),
        "{:?}",
        plan.budget_violations
    );
    // Plenty of capacity and latency, but invalidations exceed the budget.
    let plan = plan_frontier_resident_set("l2", 0.5, &objects, &budgets(10_000, 10_000, 10));
    assert!(
        plan.budget_violations
            .contains(&"invalidations".to_string()),
        "{:?}",
        plan.budget_violations
    );
}

/// When demand fits every budget, the proposal keeps everything resident and
/// the targeted retained-valid mass (the hub checker's threshold) is
/// trivially satisfiable -- the checker-authority mirror of the hub
/// just-below/just-above pair.
#[test]
fn demand_that_fits_all_budgets_is_fully_resident() {
    let objects = vec![
        object("root/a", 100, 400, 5, 0),
        object("root/b", 100, 400, 5, 0),
        object("root/c", 200, 200, 5, 0),
    ];
    let plan = plan_frontier_resident_set("l2", 0.95, &objects, &budgets(1_000, 100, 10));
    assert_eq!(plan.resident_bytes, 400);
    assert!(
        plan.budget_violations.is_empty(),
        "{:?}",
        plan.budget_violations
    );
    assert!(plan.objects.iter().all(|object| object.resident));
    let retained = plan.resident_valid_weight as f64 / plan.total_demand_weight.max(1) as f64;
    assert!(retained >= plan.threshold);
}

/// Tree-shaped demand: nested object roots must not affect the deterministic
/// density ordering, and invalid/undemanded objects are never resident.
#[test]
fn tree_shaped_demand_keeps_density_order_and_excludes_invalid() {
    // Tree: a/{x,y}, b, c/{z,w}. Shuffled on purpose; ordering must not
    // matter because selection is density-sorted with a stable tie-break.
    let mut objects = vec![
        FrontierPlanObject {
            object_root: "c/z".to_string(),
            size_bytes: 500,
            demand_weight: 500,
            valid: true,
            resident: false,
            estimated_latency_ms: 1,
            expected_invalidations: 0,
        },
        FrontierPlanObject {
            object_root: "a/x".to_string(),
            size_bytes: 100,
            demand_weight: 100,
            valid: true,
            resident: false,
            estimated_latency_ms: 1,
            expected_invalidations: 0,
        },
        FrontierPlanObject {
            object_root: "b".to_string(),
            size_bytes: 2_000,
            demand_weight: 400,
            valid: true,
            resident: false,
            estimated_latency_ms: 1,
            expected_invalidations: 0,
        },
        FrontierPlanObject {
            object_root: "stale".to_string(),
            size_bytes: 100,
            demand_weight: 900,
            valid: false, // invalidated; must never be resident
            resident: false,
            estimated_latency_ms: 1,
            expected_invalidations: 0,
        },
        FrontierPlanObject {
            object_root: "c/w".to_string(),
            size_bytes: 500,
            demand_weight: 100,
            valid: true,
            resident: false,
            estimated_latency_ms: 1,
            expected_invalidations: 0,
        },
        FrontierPlanObject {
            object_root: "a/y".to_string(),
            size_bytes: 100,
            demand_weight: 300,
            valid: true,
            resident: false,
            estimated_latency_ms: 1,
            expected_invalidations: 0,
        },
    ];
    let first = plan_frontier_resident_set("l2", 0.8, &objects, &budgets(700, 1_000, 10));
    let first_resident = first
        .objects
        .iter()
        .filter(|object| object.resident)
        .map(|object| object.object_root.as_str())
        .collect::<Vec<_>>();
    // Density order: c/z (1.0), a/y (3.0)? No: 300/100=3 beats 500/500=1.
    // Sorted: a/y (3.0), a/x (1.0), c/z (1.0), c/w (0.2), b (0.2).
    // Greedy at capacity 700: a/y (100) + a/x (100) + c/z (500) = 700.
    let mut first_resident = first_resident;
    first_resident.sort();
    assert_eq!(first_resident, ["a/x", "a/y", "c/z"]);
    assert!(
        !first
            .objects
            .iter()
            .any(|object| object.object_root == "stale" && object.resident),
        "invalid objects must never be resident"
    );

    // A second planning pass over a re-shuffled object list must produce the
    // same resident set (order independence).
    objects.reverse();
    let second = plan_frontier_resident_set("l2", 0.8, &objects, &budgets(700, 1_000, 10));
    let second_resident = second
        .objects
        .iter()
        .filter(|object| object.resident)
        .map(|object| object.object_root.as_str())
        .collect::<Vec<_>>();
    let mut second_resident = second_resident;
    second_resident.sort();
    assert_eq!(second_resident, first_resident);
}
