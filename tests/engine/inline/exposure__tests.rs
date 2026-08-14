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

#[test]
fn provider_history_is_append_only_with_dynamic_envelope_exclusion() {
    // Per-request header message (index 1) is the declared dynamic envelope.
    let policy = AppendOnlyHistoryPolicy::new(DynamicEnvelopeExclusion::message_indexes(vec![1]));

    let h1 = vec!["system".to_string(), "headers-v1".to_string()];
    let h2 = vec![
        "system".to_string(),
        "headers-v1".to_string(),
        "user".to_string(),
        "assistant".to_string(),
    ];

    // h2 extends h1: every earlier message unchanged, new messages appended,
    // so the LCP of the successive histories is the earlier history itself.
    assert_eq!(
        policy.check(&h1, &h2),
        Ok(2),
        "extension keeps LCP == earlier history"
    );

    // h3 mutates a non-excluded earlier message -> fail loud.
    let h3 = vec![
        "system EDITED".to_string(),
        "headers-v1".to_string(),
        "user".to_string(),
        "assistant".to_string(),
    ];
    assert_eq!(
        policy.check(&h2, &h3),
        Err(ProviderHistoryRewrite::RewroteMessage {
            index: 0,
            lcp: 0,
            previous: "system".to_string(),
            rewritten: "system EDITED".to_string(),
        })
    );

    // Dropping earlier content (truncation) also fails loud.
    assert_eq!(
        policy.check(&h2, &h1),
        Err(ProviderHistoryRewrite::Truncated {
            previous_len: 4,
            next_len: 2
        })
    );

    // A dynamic-envelope-excluded segment difference passes: the headers
    // message may differ while all non-excluded content stays unchanged.
    let h4 = vec![
        "system".to_string(),
        "headers-v2".to_string(),
        "user".to_string(),
        "assistant".to_string(),
    ];
    assert_eq!(policy.check(&h2, &h4), Ok(1), "envelope difference passes");

    // The session ledger enforces the same policy on the exposure path and
    // keeps the previous history on violation (fail loud, no partial state).
    let mut ledger = SessionExposureLedger::default();
    ledger.set_history_policy(policy);
    assert_eq!(
        ledger.record_provider_history(h1),
        Ok(0),
        "first history is accepted"
    );
    assert_eq!(
        ledger.record_provider_history(h3),
        Err(ProviderHistoryRewrite::RewroteMessage {
            index: 0,
            lcp: 0,
            previous: "system".to_string(),
            rewritten: "system EDITED".to_string(),
        }),
        "ledger rejects rewrites outside the declared exclusion"
    );
}
