use super::*;

fn accounting(raw: usize, visible: usize, recovery: usize) -> Accounting {
    Accounting {
        raw_tokens: raw,
        visible_tokens: visible,
        recovery_tokens: recovery,
        billed_tokens: visible,
        ..Accounting::default()
    }
}

#[test]
fn tzg0vj_charge_from_accounting_classifies_read_and_expand() {
    let read = charge_from_accounting("read", &accounting(200, 50, 30), false);
    assert_eq!(read.raw_input_tokens, 200);
    assert_eq!(read.billed_tokens, 50);
    assert_eq!(read.recovery_tokens, 30);
    assert_eq!(read.input_tokens, 80);
    assert_eq!(read.reexpansion_tokens, 0);
    read.check_classification().expect("read classifies");

    let expand = charge_from_accounting("expand", &accounting(80, 80, 80), false);
    assert_eq!(expand.billed_tokens, 0);
    assert_eq!(expand.recovery_tokens, 80);
    assert_eq!(expand.reexpansion_tokens, 0);
    expand.check_classification().expect("expand classifies");

    let again = charge_from_accounting("expand", &accounting(80, 80, 80), true);
    assert_eq!(again.recovery_tokens, 0);
    assert_eq!(again.reexpansion_tokens, 80);
    again.check_classification().expect("reexpand classifies");
}

#[test]
fn tzg0vj_session_gauge_charges_reexpand_as_reexpansion() {
    let mut gauge = SessionRaccGauge::with_lexical_identity();
    let acc = accounting(40, 40, 40);
    let first = gauge
        .charge_response("expand", &acc, Some("tz://blob/aaaa"))
        .expect("first expand");
    let second = gauge
        .charge_response("expand", &acc, Some("tz://blob/aaaa"))
        .expect("reexpand");
    assert_eq!(first.recovery_tokens, 40);
    assert_eq!(first.reexpansion_tokens, 0);
    assert_eq!(second.recovery_tokens, 0);
    assert_eq!(second.reexpansion_tokens, 40);
    assert_eq!(gauge.charge_count(), 2);
}

#[test]
fn tzg0vj_dominance_receipt_exact_phase_valid_is_recomputable() {
    let mut gauge = SessionRaccGauge::with_lexical_identity();
    gauge
        .charge_response("read", &accounting(1000, 100, 0), None)
        .expect("charge");
    let receipt =
        seal_with_labeled_evidence(&gauge, 200_000, "archive", "policy", "task").expect("seal");
    assert!(receipt.meets_token_target());
    assert!(receipt.exact_phase_valid());
    assert!(receipt.exact_phase_valid(), "predicate is recomputable");
    assert_eq!(receipt.racc_input_tokens, 100);
}

#[test]
fn tzg0vj_uncertified_lossy_requires_expand_or_raw_fallback() {
    assert_eq!(
        classify_compression(Mode::Passthrough, false),
        Ok(CompressionRoute::Passthrough)
    );
    assert_eq!(
        classify_compression(Mode::Auto, false),
        Ok(CompressionRoute::Compact)
    );
    assert_eq!(
        classify_compression(Mode::Lossy, true),
        Ok(CompressionRoute::LossyExpandEligible)
    );
    assert_eq!(
        classify_compression(Mode::Lossy, false),
        Err("uncertified lossy without Expand/RawFallback")
    );
}
