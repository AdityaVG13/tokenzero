use serde::Serialize;

use crate::PulseEvent;

/// Anytime-valid Bernoulli e-process for the live Pulse failure stream.
///
/// Under the null that the failure probability is at most
/// `null_failure_rate`, the e-value is a non-negative supermartingale.
/// Therefore crossing `1 / alpha` controls type-I error at `alpha` even
/// when callers stop after any data-dependent Pulse event.
#[derive(Debug, Clone)]
pub struct AnytimeFailureMonitor {
    alpha: f64,
    null_failure_rate: f64,
    alternative_failure_rate: f64,
    log_e_value: f64,
    events: u64,
    failures: u64,
    crossing_event: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EProcessSnapshot {
    pub alpha: f64,
    pub null_failure_rate: f64,
    pub alternative_failure_rate: f64,
    pub events: u64,
    pub failures: u64,
    pub log_e_value: f64,
    pub e_value: f64,
    pub threshold: f64,
    pub tripped: bool,
    pub crossing_event: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorConfigError;

impl std::fmt::Display for MonitorConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "alpha and rates must be finite with 0 < alpha < 1 and 0 < null < alternative < 1",
        )
    }
}

impl std::error::Error for MonitorConfigError {}

impl AnytimeFailureMonitor {
    pub fn new(
        alpha: f64,
        null_failure_rate: f64,
        alternative_failure_rate: f64,
    ) -> Result<Self, MonitorConfigError> {
        if !alpha.is_finite()
            || !null_failure_rate.is_finite()
            || !alternative_failure_rate.is_finite()
            || !(0.0 < alpha && alpha < 1.0)
            || !(0.0 < null_failure_rate
                && null_failure_rate < alternative_failure_rate
                && alternative_failure_rate < 1.0)
        {
            return Err(MonitorConfigError);
        }
        Ok(Self {
            alpha,
            null_failure_rate,
            alternative_failure_rate,
            log_e_value: 0.0,
            events: 0,
            failures: 0,
            crossing_event: None,
        })
    }

    /// Consume one event from a live Pulse stream.
    pub fn observe(&mut self, event: &PulseEvent) -> EProcessSnapshot {
        self.observe_outcome(event.failure)
    }

    /// Consume one Bernoulli outcome from another live reliability stream.
    pub fn observe_outcome(&mut self, failure: bool) -> EProcessSnapshot {
        self.observe_counts(u64::from(failure), u64::from(!failure))
    }

    /// Consume aggregated Bernoulli outcomes without expanding token-level streams.
    pub fn observe_counts(&mut self, failures: u64, successes: u64) -> EProcessSnapshot {
        self.events = self.events.saturating_add(failures).saturating_add(successes);
        self.failures = self.failures.saturating_add(failures);
        self.log_e_value += failures as f64
            * (self.alternative_failure_rate / self.null_failure_rate).ln();
        self.log_e_value += successes as f64
            * ((1.0 - self.alternative_failure_rate) / (1.0 - self.null_failure_rate)).ln();
        if self.crossing_event.is_none() && self.log_e_value >= (1.0 / self.alpha).ln() {
            self.crossing_event = Some(self.events);
        }
        self.snapshot()
    }

    pub fn observe_stream<'a>(
        &mut self,
        events: impl IntoIterator<Item = &'a PulseEvent>,
    ) -> EProcessSnapshot {
        for event in events {
            self.observe(event);
        }
        self.snapshot()
    }

    pub fn snapshot(&self) -> EProcessSnapshot {
        let threshold = 1.0 / self.alpha;
        EProcessSnapshot {
            alpha: self.alpha,
            null_failure_rate: self.null_failure_rate,
            alternative_failure_rate: self.alternative_failure_rate,
            events: self.events,
            failures: self.failures,
            log_e_value: self.log_e_value,
            e_value: self.log_e_value.exp(),
            threshold,
            tripped: self.crossing_event.is_some(),
            crossing_event: self.crossing_event,
        }
    }
}

#[cfg(test)]
mod tests {
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
                crossing_probability += null_rate.powi(failures)
                    * (1.0 - null_rate).powi(horizon - failures);
            }
        }

        assert!(
            crossing_probability <= alpha + 1e-12,
            "optional-stopping type-I error {crossing_probability} exceeded alpha {alpha}"
        );
    }
}
