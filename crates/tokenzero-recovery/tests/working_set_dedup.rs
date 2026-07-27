use proptest::prelude::*;
use std::path::PathBuf;
use tempfile::tempdir;
use tokenzero_recovery::{
    RecoveryStore,
    working_set::{
        ALREADY_RESIDENT_ATOM, SpanAnchor, WorkingSet, WorkingSetResponse, integrate_delta,
    },
};

fn anchor() -> SpanAnchor {
    SpanAnchor {
        path: PathBuf::from("src/lib.rs"),
        symbol: None,
        start_line: 1,
        end_line: 64,
    }
}

#[test]
fn resident_and_changed_reads_emit_minimal_responses() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("store")));
    let mut set = WorkingSet::new(4096);
    let original = (0..40)
        .map(|n| format!("line {n}
"))
        .collect::<String>();
    assert!(matches!(
        set.admit(&mut store, original.clone(), anchor())
            .unwrap()
            .response,
        WorkingSetResponse::Full
    ));
    let hit = set.admit(&mut store, original.clone(), anchor()).unwrap();
    assert_eq!(hit.response.visible_text(), Some(ALREADY_RESIDENT_ATOM));
    let changed = original.replace("line 20
", "line twenty
");
    let admission = set.admit(&mut store, changed.clone(), anchor()).unwrap();
    let WorkingSetResponse::Delta {
        acknowledgement,
        delta,
    } = admission.response
    else {
        panic!("expected delta")
    };
    assert!(acknowledgement.contains("-line 20"));
    assert!(acknowledgement.contains("+line twenty"));
    assert!(!acknowledgement.contains("line 0"));
    assert_eq!(
        integrate_delta(&original, &delta).as_deref(),
        Some(changed.as_str())
    );
}

proptest! {
    #[test]
    fn integrate_deltas_equals_batch_render(
        old in prop::collection::vec("[a-z]{0,12}", 1..8),
        new in prop::collection::vec("[a-z]{0,12}", 1..8),
    ) {
        let prefix = (0..30).map(|n| format!("stable prefix {n}")).collect::<Vec<_>>();
        let suffix = (0..30).map(|n| format!("stable suffix {n}")).collect::<Vec<_>>();
        let base = prefix.iter().chain(&old).chain(&suffix).map(|line| format!("{line}
")).collect::<String>();
        let batch = prefix.iter().chain(&new).chain(&suffix).map(|line| format!("{line}
")).collect::<String>();
        prop_assume!(base != batch);
        let dir = tempdir().unwrap();
        let mut store = RecoveryStore::new(Some(dir.path().join("store")));
        let mut set = WorkingSet::new(65_536);
        set.admit(&mut store, base.clone(), anchor()).unwrap();
        let admission = set.admit(&mut store, batch.clone(), anchor()).unwrap();
        let WorkingSetResponse::Delta { delta, .. } = admission.response else {
            return Err(TestCaseError::fail("expected delta response"));
        };
        let integrated = integrate_delta(&base, &delta);
        prop_assert_eq!(integrated.as_deref(), Some(batch.as_str()));
    }
}

#[test]
fn dedup_ta_registry_stays_within_bounds() {
    let registry: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/working-set-dedup-ta.json"
    ))
    .unwrap();
    for case in registry["cases"].as_array().unwrap() {
        let visible = case["visible_tokens"].as_f64().unwrap();
        let floor = case["floor_tokens"].as_f64().unwrap();
        let max_ta = case["max_ta"].as_f64().unwrap();
        assert!(
            visible / floor <= max_ta,
            "{} exceeded TA bound",
            case["op_class"]
        );
    }
}
