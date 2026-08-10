//! Pin-verification tests for the hub ledger contract (mirror of hub 86qk.9).
//!
//! TokenZero emits classified charges through zero-ledger. Native durability
//! promotion and gate ownership remain exclusively in the hub's zero-gate.

use zero_ledger::{
    Digest, FreshWorkVector, LedgerConfig, LedgerError, ResourceGauge, TokenCharge,
    TokenizerIdentity,
};

fn test_identity() -> TokenizerIdentity {
    let digest =
        Digest::from_hex("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
            .expect("valid hex digest");
    TokenizerIdentity::new("tokenzero-test-tokenizer", digest)
}

fn representative_charge() -> TokenCharge {
    TokenCharge {
        raw_input_tokens: 1_000,
        input_tokens: 900,
        billed_tokens: 800,
        failed_trial_tokens: 0,
        retry_tokens: 0,
        recovery_tokens: 50,
        reexpansion_tokens: 50,
        fallback_tokens: 0,
        model_output_tokens: 120,
        model_calls: 1,
        retries: 0,
        fresh_work: FreshWorkVector::new(800, 0, 100, 0)
            .expect("fresh-work decomposition is exact"),
    }
}

#[test]
fn zero_ledger_charge_path_is_live_at_pinned_rev() {
    let identity = test_identity();
    let mut gauge = ResourceGauge::new(LedgerConfig::new(identity.clone()));
    assert_eq!(gauge.charge_count(), 0);

    gauge
        .charge(&identity, &representative_charge())
        .expect("classified charge accepted");
    gauge
        .charge(&identity, &representative_charge())
        .expect("second charge accepted");
    assert_eq!(gauge.charge_count(), 2);
}

#[test]
fn zero_ledger_rejects_foreign_tokenizer_identity() {
    let identity = test_identity();
    let mut gauge = ResourceGauge::new(LedgerConfig::new(identity));
    let foreign = TokenizerIdentity::new(
        "some-other-tokenizer",
        Digest::from_hex("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")
            .unwrap(),
    );
    let err = gauge
        .charge(&foreign, &representative_charge())
        .expect_err("foreign tokenizer must be a typed error");
    assert!(matches!(err, LedgerError::TokenizerIdentityMismatch { .. }));
}

#[test]
fn zero_ledger_rejects_unclassified_or_double_counted_input() {
    let identity = test_identity();
    let mut gauge = ResourceGauge::new(LedgerConfig::new(identity.clone()));

    let mut under = representative_charge();
    under.input_tokens = under.billed_tokens + under.recovery_tokens + under.reexpansion_tokens + 1;
    let err = gauge
        .charge(&identity, &under)
        .expect_err("under-classified input must fail");
    assert!(matches!(err, LedgerError::UnclassifiedInput { .. }));

    let mut over = representative_charge();
    over.billed_tokens += 1;
    let err = gauge
        .charge(&identity, &over)
        .expect_err("double-counted input must fail");
    assert!(matches!(err, LedgerError::DoubleCountedInput { .. }));
    assert_eq!(gauge.charge_count(), 0);
}
