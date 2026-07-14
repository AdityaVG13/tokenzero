//! Prompt-resident working-set eviction backed by durable recovery refs.

use crate::{RecoveryError, RecoveryStore};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokenzero_core::{ContentType, count_tokens};

#[cfg(test)]
#[path = "working_set_tests.rs"]
mod tests;

pub const DEFAULT_WORKING_SET_TOKENS: usize = 8192;
pub const EVICTION_REF_LINE_PREFIX: &str = "TZ-EVICT/1";
const MAX_PREFETCH_HINTS_PER_FAULT: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanAnchor {
    pub path: PathBuf,
    pub symbol: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct RehydrationLatencyTelemetry {
    pub samples: u64,
    pub min_us: u64,
    pub mean_us: f64,
    pub max_us: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkingSetTelemetry {
    pub admissions: u64,
    pub evictions: u64,
    pub bytes_evicted: u64,
    pub refs_created: u64,
    pub lookups: u64,
    pub faults: u64,
    pub fault_rate: f64,
    pub rehydrations: u64,
    pub churn: u64,
    pub rehydration_latency: RehydrationLatencyTelemetry,
    #[serde(skip)]
    rehydration_latency_total_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictedSpan {
    pub id: u64,
    pub ref_id: String,
    pub replacement: String,
    pub bytes_evicted: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    pub id: u64,
    pub replacement: Option<String>,
    pub evicted: Vec<EvictedSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rehydration {
    pub id: u64,
    pub anchor: SpanAnchor,
    pub partial: bool,
    pub evicted: Vec<EvictedSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefetchCandidate {
    pub id: u64,
    pub ref_id: String,
    pub anchor: SpanAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefetchHint {
    pub ref_id: String,
    pub anchor: SpanAnchor,
}

pub trait PrefetchHook: std::fmt::Debug + Send + Sync {
    fn hints(&self, fault: &SpanAnchor, candidates: &[PrefetchCandidate]) -> Vec<PrefetchHint>;
}

#[derive(Debug, Default)]
pub struct NoopPrefetchHook;

impl PrefetchHook for NoopPrefetchHook {
    fn hints(&self, _: &SpanAnchor, _: &[PrefetchCandidate]) -> Vec<PrefetchHint> {
        Vec::new()
    }
}

/// Conservative opt-in policy: queue the nearest evicted span from the same
/// file. The working set only exposes the hint; callers decide whether and
/// when to perform I/O, so the default fault path never triggers speculative I/O.
#[derive(Debug, Default)]
pub struct SameFileNeighborPrefetch;

impl PrefetchHook for SameFileNeighborPrefetch {
    fn hints(&self, fault: &SpanAnchor, candidates: &[PrefetchCandidate]) -> Vec<PrefetchHint> {
        candidates
            .iter()
            .filter(|candidate| candidate.anchor.path == fault.path)
            .min_by_key(|candidate| {
                let distance = candidate.anchor.start_line.saturating_sub(fault.end_line)
                                    .max(fault.start_line.saturating_sub(candidate.anchor.end_line));
                (distance, candidate.id)
            })
            .map(|candidate| PrefetchHint {
                ref_id: candidate.ref_id.clone(),
                anchor: candidate.anchor.clone(),
            })
            .into_iter()
            .collect()
    }
}

#[derive(Debug)]
struct ResidentSpan {
    id: u64,
    last_touched: u64,
    anchor: SpanAnchor,
    body: SpanBody,
}

#[derive(Debug)]
enum SpanBody {
    Resident(String),
    Evicted { ref_id: String, replacement: String },
}

impl ResidentSpan {
    fn visible_text(&self) -> &str {
        match &self.body {
            SpanBody::Resident(text) => text,
            SpanBody::Evicted { replacement, .. } => replacement,
        }
    }

    fn visible_tokens(&self) -> usize {
        count_tokens(self.visible_text())
    }
}

/// Session-local prompt working set. Bodies are replaced only after their
/// exact bytes have been persisted through RecoveryStore's blob/CAS path.
#[derive(Debug)]
pub struct WorkingSet {
    budget_tokens: usize,
    sequence: u64,
    spans: Vec<ResidentSpan>,
    evicted_refs: HashMap<String, Vec<u64>>,
    telemetry: WorkingSetTelemetry,
    prefetch_hook: Box<dyn PrefetchHook>,
    prefetch_hints: VecDeque<PrefetchHint>,
}

impl WorkingSet {
    pub fn new(budget_tokens: usize) -> Self {
        Self {
            budget_tokens,
            sequence: 0,
            spans: Vec::new(),
            evicted_refs: HashMap::new(),
            telemetry: WorkingSetTelemetry::default(),
            prefetch_hook: Box::<NoopPrefetchHook>::default(),
            prefetch_hints: VecDeque::new(),
        }
    }

    pub fn register_prefetch_hook(&mut self, hook: Box<dyn PrefetchHook>) {
        self.prefetch_hook = hook;
    }

    pub fn enable_same_file_neighbor_prefetch(&mut self, enabled: bool) {
        self.prefetch_hook = if enabled {
            Box::<SameFileNeighborPrefetch>::default()
        } else {
            Box::<NoopPrefetchHook>::default()
        };
    }

    pub fn take_prefetch_hints(&mut self) -> Vec<PrefetchHint> {
        self.prefetch_hints.drain(..).collect()
    }

    pub fn admit(
        &mut self,
        store: &mut RecoveryStore,
        text: String,
        anchor: SpanAnchor,
    ) -> Result<Admission, RecoveryError> {
        let id = self.push_resident(text, anchor);
        let evicted = match self.enforce_budget(store) {
            Ok(evicted) => evicted,
            Err(error) => {
                self.spans.retain(|span| span.id != id);
                return Err(error);
            }
        };
        let replacement = self
            .spans
            .iter()
            .find(|span| span.id == id)
            .and_then(|span| match &span.body {
                SpanBody::Evicted { replacement, .. } => Some(replacement.clone()),
                SpanBody::Resident(_) => None,
            });
        Ok(Admission {
            id,
            replacement,
            evicted,
        })
    }

    /// Demand-page an evicted ref. A ref not owned by this working set costs
    /// one hash-map lookup and returns immediately without touching the store.
    pub fn rehydrate_ref(
        &mut self,
        store: &mut RecoveryStore,
        ref_id: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> Result<Option<Rehydration>, RecoveryError> {
        self.telemetry.lookups = self.telemetry.lookups.saturating_add(1);
        self.refresh_rates();
        let (lookup_ref, fragment_window) = match ref_id.split_once('#') {
            Some((base, fragment)) => {
                let Some(window) = parse_line_fragment(fragment) else {
                    return Ok(None);
                };
                (base, Some(window))
            }
            None => (ref_id, None),
        };
        let Some(id) = self
            .evicted_refs
            .get(lookup_ref)
            .and_then(|ids| ids.iter().copied().min())
        else {
            return Ok(None);
        };

        self.telemetry.faults = self.telemetry.faults.saturating_add(1);
        let started = Instant::now();
        let effective_start = start_line.or(fragment_window.map(|window| window.0));
        let effective_end = end_line.or(fragment_window.map(|window| window.1));
        let partial = effective_start.is_some() || effective_end.is_some();
        let result = store.expand(ref_id, Some("raw"), start_line, end_line, None, None);
        let elapsed_us = started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        self.record_rehydration_latency(elapsed_us);
        self.refresh_rates();
        if !result.found {
            return Ok(None);
        }

        let source_anchor = self
            .spans
            .iter()
            .find(|span| span.id == id)
            .map(|span| span.anchor.clone())
            .expect("evicted ref index must point at a span");
        let (resident_id, resident_anchor) = if partial {
            let relative_start = effective_start.unwrap_or(1).max(1);
            let returned_lines = result.content.lines().count().max(1);
            let requested_end =
                effective_end.unwrap_or_else(|| relative_start + returned_lines - 1);
            let relative_end = requested_end.max(relative_start);
            let absolute_start = source_anchor
                .start_line
                .saturating_add(relative_start.saturating_sub(1));
            let absolute_end = source_anchor
                .start_line
                .saturating_add(relative_end.saturating_sub(1))
                .min(source_anchor.end_line);
            let narrowed = SpanAnchor {
                path: source_anchor.path.clone(),
                symbol: source_anchor.symbol.clone(),
                start_line: absolute_start,
                end_line: absolute_end,
            };
            let resident_id = self.push_resident(result.content, narrowed.clone());
            (resident_id, narrowed)
        } else {
            self.sequence = self.sequence.saturating_add(1);
            let span = self
                .spans
                .iter_mut()
                .find(|span| span.id == id)
                .expect("evicted ref index must point at a span");
            span.body = SpanBody::Resident(result.content);
            span.last_touched = self.sequence;
            self.remove_evicted_ref(lookup_ref, id);
            self.note_admission();
            (id, source_anchor)
        };

        self.telemetry.rehydrations = self.telemetry.rehydrations.saturating_add(1);
        self.queue_prefetch_hints(&resident_anchor, resident_id, lookup_ref);
        let evicted = self.enforce_budget(store)?;
        Ok(Some(Rehydration {
            id: resident_id,
            anchor: resident_anchor,
            partial,
            evicted,
        }))
    }

    pub fn touch(&mut self, id: u64) -> bool {
        let Some(span) = self.spans.iter_mut().find(|span| span.id == id) else {
            return false;
        };
        self.sequence = self.sequence.saturating_add(1);
        span.last_touched = self.sequence;
        true
    }

    pub fn used_tokens(&self) -> usize {
        self.spans.iter().map(ResidentSpan::visible_tokens).sum()
    }

    pub fn visible_lines(&self) -> Vec<&str> {
        self.spans.iter().map(ResidentSpan::visible_text).collect()
    }

    pub fn telemetry(&self) -> WorkingSetTelemetry {
        self.telemetry
    }

    fn push_resident(&mut self, text: String, anchor: SpanAnchor) -> u64 {
        self.sequence = self.sequence.saturating_add(1);
        let id = self.sequence;
        self.spans.push(ResidentSpan {
            id,
            last_touched: self.sequence,
            anchor,
            body: SpanBody::Resident(text),
        });
        self.note_admission();
        id
    }

    fn note_admission(&mut self) {
        self.telemetry.admissions = self.telemetry.admissions.saturating_add(1);
        self.telemetry.churn = self.telemetry.churn.saturating_add(1);
    }

    fn record_rehydration_latency(&mut self, elapsed_us: u64) {
        let latency = &mut self.telemetry.rehydration_latency;
        latency.samples = latency.samples.saturating_add(1);
        latency.min_us = if latency.samples == 1 {
            elapsed_us
        } else {
            latency.min_us.min(elapsed_us)
        };
        latency.max_us = latency.max_us.max(elapsed_us);
        self.telemetry.rehydration_latency_total_us = self
            .telemetry
            .rehydration_latency_total_us
            .saturating_add(elapsed_us);
        latency.mean_us =
            self.telemetry.rehydration_latency_total_us as f64 / latency.samples as f64;
    }

    fn refresh_rates(&mut self) {
            self.telemetry.fault_rate = (self.telemetry.lookups != 0)
                .then(|| self.telemetry.faults as f64 / self.telemetry.lookups as f64)
                .unwrap_or_default();
        }

    fn queue_prefetch_hints(&mut self, fault: &SpanAnchor, fault_id: u64, fault_ref: &str) {
        let candidates = self
            .spans
            .iter()
            .filter_map(|span| match &span.body {
                SpanBody::Evicted { ref_id, .. } if span.id != fault_id && ref_id != fault_ref => {
                    Some(PrefetchCandidate {
                        id: span.id,
                        ref_id: ref_id.clone(),
                        anchor: span.anchor.clone(),
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        self.prefetch_hints.extend(
            self.prefetch_hook
                .hints(fault, &candidates)
                .into_iter()
                .take(MAX_PREFETCH_HINTS_PER_FAULT),
        );
    }

    fn remove_evicted_ref(&mut self, ref_id: &str, id: u64) {
            if self.evicted_refs.get_mut(ref_id).is_some_and(|ids| {
                ids.retain(|candidate| *candidate != id);
                ids.is_empty()
            }) {
                self.evicted_refs.remove(ref_id);
            }
        }

    fn enforce_budget(
        &mut self,
        store: &mut RecoveryStore,
    ) -> Result<Vec<EvictedSpan>, RecoveryError> {
        let mut evicted = Vec::new();
        while self.used_tokens() > self.budget_tokens {
            let victim = self
                .spans
                .iter()
                .enumerate()
                .filter_map(|(index, span)| match &span.body {
                    SpanBody::Resident(text) => {
                        let replacement_floor = format_ref_line(
                            "tz://blob/".to_string() + &"0".repeat(64),
                            &span.anchor,
                        );
                        (count_tokens(text) > replacement_floor.len()).then_some((
                            index,
                            span.last_touched,
                            count_tokens(text),
                            span.id,
                        ))
                    }
                    SpanBody::Evicted { .. } => None,
                })
                .min_by(|a, b| {
                    a.1.cmp(&b.1)
                        .then_with(|| b.2.cmp(&a.2))
                        .then_with(|| a.3.cmp(&b.3))
                })
                .map(|candidate| candidate.0);
            let Some(victim) = victim else { break };

            let (bytes, anchor, id) = {
                let span = &self.spans[victim];
                let SpanBody::Resident(text) = &span.body else {
                    unreachable!()
                };
                (text, &span.anchor, span.id)
            };
            let ref_id = store.store_blob(&bytes, ContentType::Unknown)?;
            let replacement = format_ref_line(ref_id.clone(), &anchor);
            let bytes_evicted = bytes.len();
            self.spans[victim].body = SpanBody::Evicted {
                ref_id: ref_id.clone(),
                replacement: replacement.clone(),
            };
            self.evicted_refs
                .entry(ref_id.clone())
                .or_default()
                .push(id);
            self.telemetry.evictions = self.telemetry.evictions.saturating_add(1);
            self.telemetry.churn = self.telemetry.churn.saturating_add(1);
            self.telemetry.bytes_evicted = self
                .telemetry
                .bytes_evicted
                .saturating_add(bytes_evicted as u64);
            self.telemetry.refs_created = self.telemetry.refs_created.saturating_add(1);
            evicted.push(EvictedSpan {
                id,
                ref_id,
                replacement,
                bytes_evicted,
            });
        }
        Ok(evicted)
    }
}

pub fn format_ref_line(ref_id: String, anchor: &SpanAnchor) -> String {
    let path =
        serde_json::to_string(&normalize_path(&anchor.path)).unwrap_or_else(|_| r#""#.to_string());
    let mut line = format!("{EVICTION_REF_LINE_PREFIX} ref={ref_id} path={path}");
    if let Some(symbol) = anchor.symbol.as_deref() {
        let symbol = serde_json::to_string(symbol).unwrap_or_else(|_| r#""#.to_string());
        line.push_str(" symbol=");
        line.push_str(&symbol);
    }
    line.push_str(&format!(" lines={}-{}", anchor.start_line, anchor.end_line));
    line
}

fn parse_line_fragment(fragment: &str) -> Option<(usize, usize)> {
    let range = fragment.strip_prefix('L')?;
    let (start, end) = match range.split_once('-') {
        Some((start, end)) => (start.parse().ok()?, end.parse().ok()?),
        None => {
            let line = range.parse().ok()?;
            (line, line)
        }
    };
    (start > 0 && start <= end).then_some((start, end))
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
