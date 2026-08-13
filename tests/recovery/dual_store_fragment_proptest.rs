//! Dual-store fragment conformance proptest (tokenzero-gnt-dual-path-expand-zzmd.3,
//! CC1-R3-004): TokenZeroStore (embedded) and RecoveryStore must agree on
//! arbitrary payload + #B/#L fragment combinations. Both stores share the
//! single crate::parse_fragment_spec grammar, so any divergence here is a
//! store-semantics bug, not a grammar drift.
//!
//! This harness would have caught the #L end-past-EOF clamp divergence
//! (zzmd.1) and the dual grammar drift (zzmd.2): it compares not only the
//! expanded bytes but the typed-error reason class.

use proptest::prelude::*;
use tokenzero_core::ContentType;
use tokenzero_recovery::RecoveryStore;
use tokenzero_recovery::embedded_store::TokenZeroStore;

fn payload_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Line-shaped payloads exercise #L clamp/EOF edges.
        prop::collection::vec("[a-z]{0,12}", 0..24usize)
            .prop_map(|ls| ls.into_iter().map(|l| format!("{l}\n")).collect::<String>()),
        // Arbitrary short UTF-8 exercises #B edges, including empty.
        prop::collection::vec(any::<char>(), 0..48usize)
            .prop_map(|cs| cs.into_iter().collect::<String>()),
    ]
}

/// Fragment shapes drawn from the contract grammar plus deliberate malformations.
fn fragment_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![
        3 => Just(None),
        4 => (0usize..64, 0usize..64).prop_map(|(a, b)| Some(format!("B{a}-{b}"))),
        2 => (0usize..64, 0usize..64).prop_map(|(a, b)| Some(format!("B{a},{b}"))),
        1 => (0usize..64, 0usize..64).prop_map(|(a, b)| Some(format!("B{a}+{b}"))),
        1 => (0usize..32).prop_map(|a| Some(format!("B{a}"))),
        4 => (0usize..32, 0usize..32).prop_map(|(a, b)| Some(format!("L{a}-L{b}"))),
        2 => (0usize..32, 0usize..32).prop_map(|(a, b)| Some(format!("L{a},{b}"))),
        1 => (0usize..32).prop_map(|a| Some(format!("L{a}"))),
        1 => prop_oneof![
            Just("".to_string()),
            Just("X1-2".to_string()),
            Just("Lx-Ly".to_string()),
            Just("B-5".to_string()),
            Just("L2#B0-1".to_string()),
        ].prop_map(Some),
    ]
}

/// Normalize a store outcome to Ok(bytes) or Err(reason-class string).
enum Outcome {
    Bytes(Vec<u8>),
    Reason(String),
}

fn embedded_expand(payload: &str, fragment: Option<&str>) -> Outcome {
    let mut store = TokenZeroStore::in_memory();
    let ref_id = store.put(payload.as_bytes(), None).unwrap();
    let full = match fragment {
        Some(f) => format!("{ref_id}#{f}"),
        None => ref_id,
    };
    match store.expand(&full) {
        Ok(bytes) => Outcome::Bytes(bytes),
        Err(err) => Outcome::Reason(format!("{err:?}")),
    }
}

fn recovery_expand(payload: &str, fragment: Option<&str>) -> Outcome {
    let mut store = RecoveryStore::new(None);
    let ref_id = store.store_blob(payload, ContentType::Unknown).unwrap();
    let full = match fragment {
        Some(f) => format!("{ref_id}#{f}"),
        None => ref_id,
    };
    let result = store.expand(&full, None, None, None, None, None);
    if result.found {
        Outcome::Bytes(result.content.into_bytes())
    } else {
        Outcome::Reason(result.reason)
    }
}

/// Reason classes must match across stores. TokenZeroStore error variants
/// embed the shared fragment taxonomy verbatim; RecoveryStore reports #L
/// window failures under its pinned `window-out-of-range` string (see
/// tests.rs::l_fragment_empty_file_returns_error and
/// portable_line_fragment_start_past_eof_remains_strict), which is the same
/// out-of-range class as the embedded `fragment-out-of-range`.
fn reason_class_matches(embedded: &str, recovery: &str) -> bool {
    if embedded.contains("fragment-out-of-range") {
        recovery.starts_with("fragment-out-of-range") || recovery.starts_with("window-out-of-range")
    } else if embedded.contains("Fragment(") {
        recovery.starts_with("fragment-")
    } else {
        // Whole-ref failures never expected here: both stores hold the blob.
        false
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn dual_store_fragment_proptest(
        payload in payload_strategy(),
        fragment in fragment_strategy(),
    ) {
        let embedded = embedded_expand(&payload, fragment.as_deref());
        let recovery = recovery_expand(&payload, fragment.as_deref());
        match (&embedded, &recovery) {
            (Outcome::Bytes(a), Outcome::Bytes(b)) => {
                prop_assert_eq!(
                    a, b,
                    "byte divergence payload={:?} fragment={:?}",
                    payload, fragment
                );
            }
            (Outcome::Reason(e), Outcome::Reason(r)) => {
                prop_assert!(
                    reason_class_matches(e, r),
                    "reason-class divergence payload={:?} fragment={:?}: embedded={} recovery={}",
                    payload, fragment, e, r
                );
            }
            (Outcome::Bytes(bytes), Outcome::Reason(r))
                if r.starts_with("fragment-not-utf8-boundary") =>
            {
                // Structural capability difference: TokenZeroStore expands raw
                // bytes, RecoveryStore returns String content and must fail
                // loudly when a #B range splits a UTF-8 char boundary. The
                // embedded bytes must indeed be invalid UTF-8.
                prop_assert!(
                    std::str::from_utf8(bytes).is_err(),
                    "recovery refused non-UTF8-boundary slice but embedded bytes were valid UTF-8: payload={:?} fragment={:?}",
                    payload, fragment
                );
            }
            _ => {
                // Line-fragment EOF clamp is shared, so found/missing must agree.
                prop_assert!(
                    false,
                    "ok/err divergence payload={:?} fragment={:?}: embedded_ok={} recovery_ok={}",
                    payload,
                    fragment,
                    matches!(&embedded, Outcome::Bytes(_)),
                    matches!(&recovery, Outcome::Bytes(_)),
                );
            }
        }
    }
}
