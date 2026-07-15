use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn anchor(path: &str) -> SpanAnchor {
    SpanAnchor {
        path: PathBuf::from(path),
        symbol: Some("parse args".to_string()),
        start_line: 3,
        end_line: 9,
    }
}

fn large(label: &str) -> String {
    (0..160)
        .map(|index| format!("{label}-{index}"))
        .collect::<Vec<_>>()
        .join(" ")
        + "
"
}

fn replacement_tokens(path: &str) -> usize {
    count_tokens(&format_ref_line(
        format!("tz://blob/{}", "0".repeat(64)),
        &anchor(path),
    ))
}

#[test]
fn under_budget_is_a_noop() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let body = large("resident");
    let mut set = WorkingSet::new(count_tokens(&body));
    let admission = set
        .admit(&mut store, body.clone(), anchor("src/a.rs"))
        .unwrap();

    assert!(admission.evicted.is_empty());
    assert_eq!(set.visible_lines(), vec![body.as_str()]);
    assert_eq!(
        set.telemetry(),
        WorkingSetTelemetry {
            admissions: 1,
            churn: 1,
            ..WorkingSetTelemetry::default()
        }
    );
}

#[test]
fn over_budget_replaces_oldest_with_documented_ref_line() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let first = large("first");
    let second = large("second");
    let mut set = WorkingSet::new(count_tokens(&first) + count_tokens(&second) + 256);
    let first_admission = set
        .admit(&mut store, first.clone(), anchor("src/a file.rs"))
        .unwrap();
    set.admit(&mut store, second, anchor("src/b.rs")).unwrap();
    let third = set
        .admit(&mut store, large("third"), anchor("src/c.rs"))
        .unwrap();

    assert_eq!(third.evicted.len(), 1);
    assert_eq!(third.evicted[0].id, first_admission.id);
    assert_eq!(
        third.evicted[0].replacement,
        format!(
            r#"TZ-EVICT/1 ref={} path="src/a file.rs" symbol="parse args" lines=3-9"#,
            third.evicted[0].ref_id
        )
    );
    assert_eq!(set.visible_lines()[0], third.evicted[0].replacement);
    assert_eq!(
        set.telemetry(),
        WorkingSetTelemetry {
            admissions: 3,
            evictions: 1,
            bytes_evicted: first.len() as u64,
            refs_created: 1,
            churn: 4,
            ..WorkingSetTelemetry::default()
        }
    );
}

#[test]
fn expand_round_trips_crlf_and_trailing_newline_byte_exactly() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let payload = "alpha
beta
gamma
"
    .repeat(80);
    let mut set = WorkingSet::new(replacement_tokens("src/crlf.txt"));
    let admission = set
        .admit(&mut store, payload.clone(), anchor("src/crlf.txt"))
        .unwrap();
    let evicted = &admission.evicted[0];
    let expanded = store.expand(&evicted.ref_id, None, None, None, None, None);

    assert!(expanded.found, "{}", expanded.reason);
    assert_eq!(expanded.content.as_bytes(), payload.as_bytes());
    assert!(expanded.content.ends_with(
        "
"
    ));
}

#[test]
fn touch_makes_recency_order_deterministic() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let body = large("same");
    let mut set = WorkingSet::new(count_tokens(&body) * 2 + 256);
    let a = set
        .admit(&mut store, body.clone(), anchor("src/a.rs"))
        .unwrap();
    let b = set
        .admit(&mut store, body.clone(), anchor("src/b.rs"))
        .unwrap();
    assert!(set.touch(a.id));
    let admission = set.admit(&mut store, body, anchor("src/c.rs")).unwrap();

    assert_eq!(admission.evicted.len(), 1);
    assert_eq!(admission.evicted[0].id, b.id);
}

#[test]
fn full_fault_rehydrates_and_enforces_lru_budget() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let first = large("first-fault");
    let second = large("second-resident");
    let budget = count_tokens(&first) + replacement_tokens("src/a.rs") + 32;
    let mut set = WorkingSet::new(budget);
    let first_admission = set
        .admit(&mut store, first.clone(), anchor("src/a.rs"))
        .unwrap();
    let second_admission = set.admit(&mut store, second, anchor("src/b.rs")).unwrap();
    let first_eviction = first_admission
        .evicted
        .first()
        .or_else(|| {
            second_admission
                .evicted
                .iter()
                .find(|span| span.id == first_admission.id)
        })
        .expect("first span should be evicted");

    let fault = set
        .rehydrate_ref(&mut store, &first_eviction.ref_id, None, None)
        .unwrap()
        .expect("owned ref should fault");

    assert!(!fault.partial);
    assert_eq!(fault.id, first_admission.id);
    assert_eq!(set.visible_lines()[0].as_bytes(), first.as_bytes());
    assert_eq!(
        fault.evicted.len(),
        1,
        "rehydration must enforce the budget"
    );
    assert_eq!(fault.evicted[0].id, second_admission.id);
    let telemetry = set.telemetry();
    assert_eq!(telemetry.lookups, 1);
    assert_eq!(telemetry.faults, 1);
    assert_eq!(telemetry.fault_rate, 1.0);
    assert_eq!(telemetry.rehydrations, 1);
}

#[test]
fn partial_fault_keeps_marker_and_admits_absolute_line_window() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let payload = (1..=160)
        .map(|line| format!("line-{line} payload payload payload\n"))
        .collect::<String>();
    let source_anchor = SpanAnchor {
        path: PathBuf::from("src/window.rs"),
        symbol: Some("window".to_string()),
        start_line: 100,
        end_line: 259,
    };
    let mut set = WorkingSet::new(replacement_tokens("src/window.rs") + 64);
    let admission = set.admit(&mut store, payload, source_anchor).unwrap();
    let evicted = admission.evicted.first().expect("fixture must evict");

    let fault = set
        .rehydrate_ref(&mut store, &evicted.ref_id, Some(3), Some(5))
        .unwrap()
        .expect("owned ref should fault");

    assert!(fault.partial);
    assert_eq!(fault.anchor.start_line, 102);
    assert_eq!(fault.anchor.end_line, 104);
    assert_eq!(set.visible_lines()[0], evicted.replacement);
    let window = set.visible_lines()[1];
    assert!(window.contains("line-3"), "{window}");
    assert!(window.contains("line-5"), "{window}");
    assert!(!window.contains("line-2"), "{window}");
    assert!(!window.contains("line-6"), "{window}");

    let fragment_ref = format!("{}#L6", evicted.ref_id);
    let second = set
        .rehydrate_ref(&mut store, &fragment_ref, None, None)
        .unwrap()
        .expect("line fragment must fault the base evicted ref");
    assert!(second.partial);
    assert_eq!(second.anchor.start_line, 105);
    assert_eq!(second.anchor.end_line, 105);
}

#[test]
fn unowned_ref_is_lookup_only_and_does_not_fault() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let mut set = WorkingSet::new(100);

    assert!(
        set.rehydrate_ref(
            &mut store,
            "tz://blob/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            None,
            None,
        )
        .unwrap()
        .is_none()
    );
    let telemetry = set.telemetry();
    assert_eq!(telemetry.lookups, 1);
    assert_eq!(telemetry.faults, 0);
    assert_eq!(telemetry.rehydration_latency.samples, 0);
}

#[derive(Debug)]
struct RecordingHook(Arc<AtomicUsize>);

impl PrefetchHook for RecordingHook {
    fn hints(&self, _: &SpanAnchor, candidates: &[PrefetchCandidate]) -> Vec<PrefetchHint> {
        self.0.fetch_add(1, Ordering::SeqCst);
        candidates
            .iter()
            .map(|candidate| PrefetchHint {
                ref_id: candidate.ref_id.clone(),
                anchor: candidate.anchor.clone(),
            })
            .collect()
    }
}

#[test]
fn prefetch_hook_fires_but_queue_is_bounded_and_default_is_noop() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let mut set = WorkingSet::new(0);
    let refs = (0..3)
        .map(|index| {
            let mut span_anchor = anchor("src/neighbors.rs");
            span_anchor.start_line = index * 20 + 1;
            span_anchor.end_line = index * 20 + 9;
            set.admit(&mut store, large(&format!("neighbor-{index}")), span_anchor)
                .unwrap()
                .evicted[0]
                .ref_id
                .clone()
        })
        .collect::<Vec<_>>();

    set.rehydrate_ref(&mut store, &refs[0], Some(1), Some(1))
        .unwrap();
    assert!(
        set.take_prefetch_hints().is_empty(),
        "default hook must be no-op"
    );

    let calls = Arc::new(AtomicUsize::new(0));
    set.register_prefetch_hook(Box::new(RecordingHook(Arc::clone(&calls))));
    set.rehydrate_ref(&mut store, &refs[0], Some(1), Some(1))
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        set.take_prefetch_hints().len(),
        1,
        "hints are capped per fault"
    );
}

#[test]
fn same_file_neighbor_prefetch_is_opt_in_and_conservative() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let mut set = WorkingSet::new(0);
    let first = set
        .admit(&mut store, large("a"), anchor("src/a.rs"))
        .unwrap();
    let neighbor = set
        .admit(&mut store, large("b"), anchor("src/a.rs"))
        .unwrap();
    set.admit(&mut store, large("other"), anchor("src/other.rs"))
        .unwrap();
    set.enable_same_file_neighbor_prefetch(true);

    set.rehydrate_ref(&mut store, &first.evicted[0].ref_id, Some(1), Some(1))
        .unwrap();
    let hints = set.take_prefetch_hints();
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].ref_id, neighbor.evicted[0].ref_id);
}

#[test]
fn store_served_rehydration_is_low_latency() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let mut set = WorkingSet::new(0);
    let admission = set
        .admit(&mut store, large("latency"), anchor("src/latency.rs"))
        .unwrap();
    let started = Instant::now();
    let result = set
        .rehydrate_ref(&mut store, &admission.evicted[0].ref_id, Some(1), Some(1))
        .unwrap();
    let observed = started.elapsed();
    assert!(result.is_some());
    eprintln!("observed store-served rehydration latency: {observed:?}");
    assert!(
        observed < Duration::from_millis(50),
        "small local-store fault took {observed:?}"
    );
    let latency = set.telemetry().rehydration_latency;
    assert_eq!(latency.samples, 1);
    assert!(latency.min_us <= latency.max_us);
    assert_eq!(latency.mean_us, latency.min_us as f64);
}
