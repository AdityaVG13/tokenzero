use super::*;
use tempfile::tempdir;

fn payload(turn: u64) -> String {
    format!("turn-{turn} ") + &"payload ".repeat(48)
}

#[test]
fn invalid_hot_tail_budget_returns_typed_error() {
    let error = ContextView::new(
        "SYSTEM stable\n",
        ContextViewConfig {
            working_set_tokens: 64,
            hot_tail_tokens: 65,
        },
    )
    .unwrap_err();

    assert_eq!(
        error,
        ContextViewConfigError::HotTailExceedsWorkingSet {
            hot_tail_tokens: 65,
            working_set_tokens: 64,
        }
    );
    assert_eq!(
        error.to_string(),
        "hot_tail_tokens (65) must not exceed working_set_tokens (64)"
    );
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
    )
    .unwrap();
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
    )
    .unwrap();
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
fn production_projection_runs_through_prefix_guard() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let mut view = ContextView::new(
        "stable ".repeat(1_500),
        ContextViewConfig {
            working_set_tokens: 256,
            hot_tail_tokens: 64,
        },
    )
    .unwrap();
    view.append(&mut store, 1, 1_000, "real renderer evidence")
        .unwrap();

    let projection = view.reproject_at_cache_breakpoint(None);
    assert!(projection.rendered.contains("real renderer evidence"));
    assert_eq!(view.guard_observation_counts(), (1, 1));
    let replay = view.project(None);
    assert_eq!(projection.rendered, replay.rendered);
    assert_eq!(view.guard_observation_counts(), (2, 1));
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
    )
    .unwrap();
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
