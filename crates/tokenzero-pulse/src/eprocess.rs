use serde::Serialize;
use std::fs;
use std::ops::Range;
use std::path::Path;

use crate::{PulseEvent, hex_sha256};

/// Typed failures for digest-before-fragment CAS serves.
///
/// Digest mismatch is fail-closed: no fragment bytes are returned once the
/// on-disk blob hash diverges from the requested identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentServeError {
    NotFound,
    Io(String),
    DigestMismatch { expected: String, actual: String },
    RangeOutOfBounds {
        len: usize,
        start: usize,
        end: usize,
    },
}

impl std::fmt::Display for FragmentServeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("cas fragment: object not found"),
            Self::Io(message) => write!(formatter, "cas fragment io: {message}"),
            Self::DigestMismatch { expected, actual } => write!(
                formatter,
                "cas fragment digest_mismatch: expected {expected}, got {actual}"
            ),
            Self::RangeOutOfBounds { len, start, end } => write!(
                formatter,
                "cas fragment range out of bounds: len={len} requested={start}..{end}"
            ),
        }
    }
}

impl std::error::Error for FragmentServeError {}

/// Load a CAS blob, verify its full SHA-256 identity, then slice a fragment.
///
/// Order is deliberate: digest is computed and compared before any fragment
/// bytes are returned. Corrupt on-disk bytes therefore fail closed as
/// [`FragmentServeError::DigestMismatch`] with no slice leak.
pub fn serve_fragment_after_digest(
    store: &zero_store::SharedCas,
    hash: &str,
    byte_range: Range<usize>,
) -> Result<Vec<u8>, FragmentServeError> {
    let path = store.object_path(hash);
    let bytes = read_cas_blob(&path)?;
    let actual = hex_sha256(&bytes);
    if actual != hash {
        return Err(FragmentServeError::DigestMismatch {
            expected: hash.to_string(),
            actual,
        });
    }
    if byte_range.end > bytes.len() || byte_range.start > byte_range.end {
        return Err(FragmentServeError::RangeOutOfBounds {
            len: bytes.len(),
            start: byte_range.start,
            end: byte_range.end,
        });
    }
    Ok(bytes[byte_range].to_vec())
}

fn read_cas_blob(path: &Path) -> Result<Vec<u8>, FragmentServeError> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(FragmentServeError::NotFound)
        }
        Err(error) => Err(FragmentServeError::Io(error.to_string())),
    }
}

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
        self.events = self
            .events
            .saturating_add(failures)
            .saturating_add(successes);
        self.failures = self.failures.saturating_add(failures);
        self.log_e_value +=
            failures as f64 * (self.alternative_failure_rate / self.null_failure_rate).ln();
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
#[path = "../../../tests/pulse/inline/eprocess__tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../tests/pulse/inline/digest_before_fragment__tests.rs"]
mod digest_before_fragment_tests;
