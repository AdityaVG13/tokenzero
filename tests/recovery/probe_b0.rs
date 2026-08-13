//! Pinned regressions caught by the dual-store fragment proptest (zzmd.3)
//! and the differential fuzz target (uymp/3kmx.2):
//! - `#B0` single-value byte selector must parse on every expand surface.
//! - An empty byte range `#B20-20` is valid even off a char boundary.
//! - A fragment whose leading byte is non-ASCII must fail loudly with a
//!   typed error, never panic on a str char-boundary slice.
use tokenzero_core::ContentType;
use tokenzero_recovery::RecoveryStore;
use tokenzero_recovery::embedded_store::TokenZeroStore;

#[test]
fn single_value_byte_fragment_and_empty_range_agree_across_stores() {
    let payload = "\0Aa\u{10000}\0\u{10000}\u{10000}0A\0\u{800}\u{800}";
    let mut recovery = RecoveryStore::new(None);
    let ref_id = recovery.store_blob(payload, ContentType::Unknown).unwrap();
    let mut embedded = TokenZeroStore::in_memory();
    let embedded_ref = embedded.put(payload.as_bytes(), None).unwrap();

    for frag in ["B0", "B20", "B20-20"] {
        let r = recovery.expand(&format!("{ref_id}#{frag}"), None, None, None, None, None);
        let e = embedded.expand(&format!("{embedded_ref}#{frag}"));
        assert!(r.found, "recovery refused #{frag}: {}", r.reason);
        let e = e.unwrap_or_else(|err| panic!("embedded refused #{frag}: {err:?}"));
        assert_eq!(r.content.into_bytes(), e, "byte divergence on #{frag}");
    }
}

/// Found by the expand_fragment_differential fuzz target: a fragment whose
/// first byte is part of a multi-byte UTF-8 char panicked with
/// `str::slice_error_fail` at parse_fragment_spec (`&fragment[1..]` splits
/// the char). Non-ASCII leading bytes are not valid fragment kinds and must
/// surface as a typed fragment error on both stores.
#[test]
fn non_ascii_fragment_leading_byte_is_typed_error_not_panic() {
    let payload = "hello";
    let frag = "\u{fffd}'\0"; // leading replacement char (3 bytes)

    let mut recovery = RecoveryStore::new(None);
    let ref_id = recovery.store_blob(payload, ContentType::Unknown).unwrap();
    let r = recovery.expand(&format!("{ref_id}#{frag}"), None, None, None, None, None);
    assert!(!r.found, "recovery must reject non-ASCII fragment kind");
    assert!(
        r.reason.starts_with("fragment-"),
        "recovery reason must be a typed fragment error: {}",
        r.reason
    );

    let mut embedded = TokenZeroStore::in_memory();
    let embedded_ref = embedded.put(payload.as_bytes(), None).unwrap();
    let e = embedded.expand(&format!("{embedded_ref}#{frag}"));
    let err = e.expect_err("embedded must reject non-ASCII fragment kind");
    assert!(
        format!("{err:?}").contains("fragment-"),
        "embedded error must be a typed fragment error: {err:?}"
    );
}
