//! Cache-safe materialized context views over the recovery timeline.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use tokenzero_core::{ContentType, count_tokens, sha256_hex};

use crate::{RecoveryError, RecoveryStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsOf {
    Turn(u64),
    TimestampMillis(u64),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextRecord {
    pub id: u64,
    pub turn: u64,
    pub timestamp_millis: u64,
    pub ref_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextViewConfig {
    /// Maximum tokens after the provider-cacheable prefix.
    pub working_set_tokens: usize,
    /// Portion of the budget reserved for the rolling, uncached tail.
    pub hot_tail_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextProjection {
    pub rendered: String,
    pub stable_prefix: String,
    pub stable_prefix_sha256: String,
    pub stable_prefix_tokens: usize,
    pub input_tokens: usize,
    pub working_set_tokens: usize,
    pub working_set_ids: Vec<u64>,
    pub hot_tail_ids: Vec<u64>,
    pub evicted_ids: Vec<u64>,
    pub as_of: Option<AsOf>,
    pub cache_breakpoint: bool,
}

/// Session timeline whose model-visible window is re-materialized on demand.
/// The prefix is immutable. Ordinary projections never change residency; only
/// reproject_at_cache_breakpoint may replace working-set members.
#[derive(Debug)]
pub struct ContextView {
    stable_prefix: String,
    config: ContextViewConfig,
    records: Vec<ContextRecord>,
    next_id: u64,
    resident_ids: BTreeSet<u64>,
}

impl ContextView {
    pub fn new(stable_prefix: impl Into<String>, config: ContextViewConfig) -> Self {
        assert!(
            config.hot_tail_tokens <= config.working_set_tokens,
            "hot tail must fit inside the working-set budget"
        );
        Self {
            stable_prefix: stable_prefix.into(),
            config,
            records: Vec::new(),
            next_id: 1,
            resident_ids: BTreeSet::new(),
        }
    }

    /// Persist a timeline record before making it visible to projections.
    pub fn append(
        &mut self,
        store: &mut RecoveryStore,
        turn: u64,
        timestamp_millis: u64,
        text: impl Into<String>,
    ) -> Result<u64, RecoveryError> {
        let text = text.into();
        let ref_id = store.store_blob(&text, ContentType::Unknown)?;
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.records.push(ContextRecord {
            id,
            turn,
            timestamp_millis,
            ref_id,
            text,
        });
        Ok(id)
    }

    pub fn project(&self, as_of: Option<AsOf>) -> ContextProjection {
        self.render(as_of, false, &self.resident_ids, Vec::new())
    }

    /// Recompute residency at an explicit provider prompt-cache breakpoint.
    pub fn reproject_at_cache_breakpoint(&mut self, as_of: Option<AsOf>) -> ContextProjection {
        let eligible = self.eligible(as_of);
        let hot_tail = select_newest(eligible.iter().copied(), self.config.hot_tail_tokens);
        let hot_ids = hot_tail
            .iter()
            .map(|record| record.id)
            .collect::<BTreeSet<_>>();
        let resident_budget = self
            .config
            .working_set_tokens
            .saturating_sub(rendered_tokens(hot_tail.iter().copied()));
        let selected = select_newest(
            eligible
                .iter()
                .copied()
                .filter(|record| !hot_ids.contains(&record.id)),
            resident_budget,
        );
        let next = selected
            .iter()
            .map(|record| record.id)
            .collect::<BTreeSet<_>>();
        let evicted = self
            .resident_ids
            .difference(&next)
            .copied()
            .collect::<Vec<_>>();
        self.resident_ids = next;
        self.render(as_of, true, &self.resident_ids, evicted)
    }

    fn eligible(&self, as_of: Option<AsOf>) -> Vec<&ContextRecord> {
        self.records
            .iter()
            .filter(|record| match as_of {
                None => true,
                Some(AsOf::Turn(turn)) => record.turn <= turn,
                Some(AsOf::TimestampMillis(timestamp)) => record.timestamp_millis <= timestamp,
            })
            .collect()
    }

    fn render(
        &self,
        as_of: Option<AsOf>,
        cache_breakpoint: bool,
        resident_ids: &BTreeSet<u64>,
        evicted_ids: Vec<u64>,
    ) -> ContextProjection {
        let eligible = self.eligible(as_of);
        let hot_tail = select_newest(eligible.iter().copied(), self.config.hot_tail_tokens);
        let hot_ids = hot_tail
            .iter()
            .map(|record| record.id)
            .collect::<BTreeSet<_>>();
        let remaining_budget = self
            .config
            .working_set_tokens
            .saturating_sub(rendered_tokens(hot_tail.iter().copied()));
        let working_set = select_newest(
            eligible.iter().copied().filter(|record| {
                resident_ids.contains(&record.id) && !hot_ids.contains(&record.id)
            }),
            remaining_budget,
        );
        let mut rendered = self.stable_prefix.clone();
        for record in working_set.iter().chain(hot_tail.iter()) {
            rendered.push_str(&render_record(record));
        }
        let stable_prefix_sha256 = sha256_hex(&self.stable_prefix);
        let stable_prefix_tokens = count_tokens(&self.stable_prefix);
        let input_tokens = count_tokens(&rendered);
        ContextProjection {
            rendered,
            stable_prefix: self.stable_prefix.clone(),
            stable_prefix_sha256,
            stable_prefix_tokens,
            input_tokens,
            working_set_tokens: input_tokens.saturating_sub(stable_prefix_tokens),
            working_set_ids: working_set.iter().map(|record| record.id).collect(),
            hot_tail_ids: hot_tail.iter().map(|record| record.id).collect(),
            evicted_ids,
            as_of,
            cache_breakpoint,
        }
    }
}

fn render_record(record: &ContextRecord) -> String {
    format!(
        "\nTZ-VIEW/1 id={} turn={} t={} ref={}\n{}\n",
        record.id, record.turn, record.timestamp_millis, record.ref_id, record.text
    )
}

fn rendered_tokens<'a>(records: impl Iterator<Item = &'a ContextRecord>) -> usize {
    records
        .map(|record| count_tokens(&render_record(record)))
        .sum()
}

fn select_newest<'a>(
    records: impl Iterator<Item = &'a ContextRecord>,
    budget: usize,
) -> Vec<&'a ContextRecord> {
    let records = records.collect::<Vec<_>>();
    let mut used = 0usize;
    let mut selected = Vec::new();
    for record in records.into_iter().rev() {
        let tokens = count_tokens(&render_record(record));
        if used.saturating_add(tokens) <= budget {
            used += tokens;
            selected.push(record);
        }
    }
    selected.reverse();
    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn payload(turn: u64) -> String {
        format!("turn-{turn} ") + &"payload ".repeat(48)
    }

    #[test]
    fn as_of_reprojects_turn_and_timestamp_without_future_records() {
        let dir = tempdir().unwrap();
        let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
        let mut view = ContextView::new(
            "SYSTEM stable\n",
            ContextViewConfig {
                working_set_tokens: 512,
                hot_tail_tokens: 256,
            },
        );
        view.append(&mut store, 1, 100, "one").unwrap();
        view.append(&mut store, 2, 200, "two").unwrap();
        view.append(&mut store, 3, 300, "three").unwrap();
        let by_turn = view.project(Some(AsOf::Turn(2)));
        let by_time = view.project(Some(AsOf::TimestampMillis(200)));
        assert_eq!(by_turn.rendered, by_time.rendered);
        assert!(by_turn.rendered.contains("two"));
        assert!(!by_turn.rendered.contains("three"));
    }

    #[test]
    fn eviction_occurs_only_at_cache_breakpoints() {
        let dir = tempdir().unwrap();
        let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
        let mut view = ContextView::new(
            "SYSTEM stable\n",
            ContextViewConfig {
                working_set_tokens: 160,
                hot_tail_tokens: 80,
            },
        );
        for turn in 1..=3 {
            view.append(&mut store, turn, turn * 100, payload(turn))
                .unwrap();
        }
        let first = view.reproject_at_cache_breakpoint(None);
        view.append(&mut store, 4, 400, payload(4)).unwrap();
        let ordinary = view.project(None);
        assert!(!ordinary.cache_breakpoint);
        assert!(ordinary.evicted_ids.is_empty());
        let second = view.reproject_at_cache_breakpoint(None);
        assert!(second.cache_breakpoint);
        assert_ne!(first.working_set_ids, second.working_set_ids);
        assert!(!second.evicted_ids.is_empty());
    }

    #[test]
    fn replay_has_bounded_input_and_stable_cache_prefix() {
        let budget = 192;
        let dir = tempdir().unwrap();
        let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
        let mut view = ContextView::new(
            "SYSTEM tools manifest=v1\n",
            ContextViewConfig {
                working_set_tokens: budget,
                hot_tail_tokens: 96,
            },
        );
        let mut max_dynamic = 0;
        let mut prefix_digest = None;
        for turn in 1..=200 {
            view.append(&mut store, turn, turn * 1_000, payload(turn))
                .unwrap();
            let projection = if turn % 20 == 0 {
                view.reproject_at_cache_breakpoint(None)
            } else {
                view.project(None)
            };
            max_dynamic = max_dynamic.max(projection.working_set_tokens);
            assert!(projection.working_set_tokens <= budget);
            assert!(projection.rendered.starts_with(&projection.stable_prefix));
            assert_eq!(
                prefix_digest.get_or_insert_with(|| projection.stable_prefix_sha256.clone()),
                &projection.stable_prefix_sha256
            );
        }
        eprintln!(
            "context-view replay: turns=200 W={budget} max_dynamic_tokens={max_dynamic} stable_prefix=true"
        );
    }
}
