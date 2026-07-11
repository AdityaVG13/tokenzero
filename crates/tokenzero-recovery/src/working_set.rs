//! Prompt-resident working-set eviction backed by durable recovery refs.

use crate::{RecoveryError, RecoveryStore};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokenzero_core::{ContentType, count_tokens};

#[cfg(test)]
#[path = "working_set_tests.rs"]
mod tests;

pub const DEFAULT_WORKING_SET_TOKENS: usize = 8192;
pub const EVICTION_REF_LINE_PREFIX: &str = "TZ-EVICT/1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanAnchor {
    pub path: PathBuf,
    pub symbol: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingSetTelemetry {
    pub evictions: u64,
    pub bytes_evicted: u64,
    pub refs_created: u64,
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
    Evicted { replacement: String },
}

impl ResidentSpan {
    fn visible_text(&self) -> &str {
        match &self.body {
            SpanBody::Resident(text) => text,
            SpanBody::Evicted { replacement } => replacement,
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
    telemetry: WorkingSetTelemetry,
}

impl WorkingSet {
    pub fn new(budget_tokens: usize) -> Self {
        Self {
            budget_tokens,
            sequence: 0,
            spans: Vec::new(),
            telemetry: WorkingSetTelemetry::default(),
        }
    }

    pub fn admit(
        &mut self,
        store: &mut RecoveryStore,
        text: String,
        anchor: SpanAnchor,
    ) -> Result<Admission, RecoveryError> {
        self.sequence = self.sequence.saturating_add(1);
        let id = self.sequence;
        self.spans.push(ResidentSpan {
            id,
            last_touched: self.sequence,
            anchor,
            body: SpanBody::Resident(text),
        });

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
                SpanBody::Evicted { replacement } => Some(replacement.clone()),
                SpanBody::Resident(_) => None,
            });
        Ok(Admission {
            id,
            replacement,
            evicted,
        })
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
            let Some(victim) = victim else {
                break;
            };

            let (bytes, anchor, id) = {
                let span = &self.spans[victim];
                let SpanBody::Resident(text) = &span.body else {
                    unreachable!();
                };
                (text.clone(), span.anchor.clone(), span.id)
            };
            let ref_id = store.store_blob(&bytes, ContentType::Unknown)?;
            let replacement = format_ref_line(ref_id.clone(), &anchor);
            let bytes_evicted = bytes.len();
            self.spans[victim].body = SpanBody::Evicted {
                replacement: replacement.clone(),
            };
            self.telemetry.evictions = self.telemetry.evictions.saturating_add(1);
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

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
