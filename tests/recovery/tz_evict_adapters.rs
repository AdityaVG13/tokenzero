use std::path::PathBuf;

use tempfile::tempdir;
use tokenzero_recovery::{
    RecoveryStore,
    working_set::{SpanAnchor, WorkingSet},
};

#[test]
fn tz_evict_context_edit_shrinks_window_and_fault_hook_rehydrates() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("store")));
    let mut working_set = WorkingSet::new(1);
    let body = (1..=160)
        .map(|line| format!("adapter line {line}: durable context bytes\n"))
        .collect::<String>();
    let anchor = SpanAnchor {
        path: PathBuf::from("src/context.rs"),
        symbol: Some("Context".to_string()),
        start_line: 1,
        end_line: 160,
    };

    let admission = working_set
        .apply_context_edit(&mut store, body.clone(), anchor)
        .unwrap();
    let eviction = admission.evicted.first().expect("context must page out");
    assert!(
        eviction
            .replacement
            .starts_with("TZ-EVICT/1 ref=tz://blob/")
    );
    assert!(working_set.used_tokens() < tokenzero_core::count_tokens(&body));

    let rehydration = working_set
        .handle_fault_hook(&mut store, &eviction.ref_id, None, None)
        .unwrap()
        .expect("fault hook must rehydrate owned ref");
    assert!(!rehydration.partial);
    let telemetry = working_set.telemetry();
    assert_eq!(telemetry.context_edits, 1);
    assert_eq!(telemetry.fault_hook_calls, 1);
    assert_eq!(telemetry.rehydrations, 1);
    assert_eq!(telemetry.eviction_accounting.p_fault, 1.0);
    assert!(telemetry.eviction_accounting.expected_rehydration_tokens > 0.0);
    assert!(telemetry.eviction_accounting.amortized_tokens_per_access > 0.0);
    assert!(telemetry.eviction_accounting.actual_rehydration_tokens > 0);
    assert!(telemetry.eviction_accounting.thrash_worst_case_tokens > 0);
    assert!(telemetry.eviction_accounting.alarm);
}
