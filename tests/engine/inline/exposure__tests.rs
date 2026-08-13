use super::*;

#[test]
fn record_then_hit_then_reexpand() {
    let mut ledger = SessionExposureLedger::default();
    ledger.next_turn();
    assert!(ledger.record("tz://blob/aa", None, 2048));
    assert!(
        !ledger.record("tz://blob/aa", None, 2048),
        "second record is a hit"
    );
    let row = ledger.exposure("tz://blob/aa", None).expect("row present");
    assert_eq!(row.first_exposure_turn, 1);
    assert_eq!(row.byte_len, 2048);
    assert_eq!(ledger.record_reexpansion("tz://blob/aa", None), Some(1));
    assert_eq!(ledger.record_reexpansion("tz://blob/aa", None), Some(2));
    assert_eq!(
        ledger.exposure("tz://blob/aa", None).unwrap().reexpansions,
        2
    );
    assert_eq!(
        ledger.record_reexpansion("tz://blob/foreign", None),
        None,
        "foreign refs are not session replays"
    );
}

#[test]
fn spans_are_distinct_exposures() {
    let mut ledger = SessionExposureLedger::default();
    assert!(ledger.record("tz://blob/bb", Some("L1-L20".into()), 400));
    assert!(ledger.record("tz://blob/bb", Some("L21-L40".into()), 400));
    assert!(!ledger.record("tz://blob/bb", Some("L1-L20".into()), 400));
    assert!(ledger.exposure("tz://blob/bb", None).is_none());
    assert_eq!(ledger.len(), 2);
}

#[test]
fn registry_shares_within_scope_and_isolates_across_scopes() {
    let a1 = session_exposure_ledger("scope-a-test");
    let a2 = session_exposure_ledger("scope-a-test");
    let b = session_exposure_ledger("scope-b-test");
    a1.lock().unwrap().record("tz://blob/cc", None, 10);
    assert!(a2.lock().unwrap().exposure("tz://blob/cc", None).is_some());
    assert!(b.lock().unwrap().exposure("tz://blob/cc", None).is_none());
}
