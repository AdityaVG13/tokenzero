use super::*;

fn event(failure: bool) -> PulseEvent {
    let mut event = PulseEvent::tool_call("read", "mcp", 10, 2, 0, 0, 1, None);
    event.failure = failure;
    event
}

#[test]
fn sustained_regression_trips_at_declared_alpha() {
    let mut monitor = AnytimeFailureMonitor::new(0.05, 0.1, 0.5).unwrap();
    let failures = (0..8).map(|_| event(true)).collect::<Vec<_>>();
    let snapshot = monitor.observe_stream(&failures);
    assert!(snapshot.tripped);
    assert!(snapshot.e_value >= snapshot.threshold);
    assert!(snapshot.crossing_event.is_some());
}

#[test]
fn adversarial_optional_stopping_respects_type_one_error() {
    let alpha = 0.05;
    let null_rate = 0.1;
    let horizon = 16;
    let mut crossing_probability = 0.0;

    // Enumerating every path and counting "ever crossed" is the strongest
    // stopping rule over this finite horizon: it may stop at any crossing.
    for bits in 0_u64..(1_u64 << horizon) {
        let mut monitor = AnytimeFailureMonitor::new(alpha, null_rate, 0.5).unwrap();
        let mut failures = 0_i32;
        for index in 0..horizon {
            let failed = bits & (1_u64 << index) != 0;
            failures += i32::from(failed);
            monitor.observe(&event(failed));
        }
        if monitor.snapshot().tripped {
            crossing_probability +=
                null_rate.powi(failures) * (1.0 - null_rate).powi(horizon - failures);
        }
    }

    assert!(
        crossing_probability <= alpha + 1e-12,
        "optional-stopping type-I error {crossing_probability} exceeded alpha {alpha}"
    );
}
