//! Pinned regressions caught by the dual-store fragment proptest (zzmd.3):
//! - `#B0` single-value byte selector must parse on every expand surface.
//! - An empty byte range `#B20-20` is valid even off a char boundary.
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
