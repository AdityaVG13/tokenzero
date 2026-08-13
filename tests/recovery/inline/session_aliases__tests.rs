use super::*;

#[test]
fn hmac_sha256_matches_rfc4231_case2_layout() {
    // RFC 4231 test case 2 layout: key "Jefe" zero-padded to our fixed
    // 32-byte key contract, data "what do ya want for nothing?". HMAC
    // zero-pads short keys to the block size, so HMAC-SHA256 over the
    // padded key equals this independently computed golden vector
    // (Python hmac.new(b"Jefe" + 28*NUL, msg, sha256)).
    let mut key = [0u8; ALIAS_KEY_BYTES];
    key[..4].copy_from_slice(b"Jefe");
    let mac = hmac_sha256(&key, b"what do ya want for nothing?");
    let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex,
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

#[test]
fn keyed_alias_is_opaque_and_deterministic() {
    // W4-OPAQUE-CAS-ALIAS: visible alias bytes must be independent of the
    // content hash (keyed derivation), deterministic per store key.
    let hash = "ab".repeat(32);
    let key_a = [7u8; ALIAS_KEY_BYTES];
    let key_b = [9u8; ALIAS_KEY_BYTES];
    let alias_a = session_alias_hex_keyed(&key_a, &hash);
    assert_eq!(alias_a.len(), SESSION_ALIAS_HEX_LEN);
    // Golden vector (python hmac.new(bytes([7]*32), b"abab...", sha256)):
    // the alias is the keyed MAC prefix, not the content-hash prefix.
    assert_eq!(alias_a, "c31200b508a114fd");
    assert_ne!(
        alias_a,
        hash[..SESSION_ALIAS_HEX_LEN],
        "alias must not be the content-hash prefix"
    );
    assert_eq!(
        alias_a,
        session_alias_hex_keyed(&key_a, &hash),
        "same key => same alias"
    );
    assert_eq!(
        session_alias_hex_keyed(&key_b, &hash),
        "c66bacbc69f7418b",
        "different key => different alias"
    );
    let full = format!("tz://blob/{hash}");
    let short = session_visible_blob_alias_keyed(&key_a, &full).unwrap();
    assert_eq!(short, format!("tz://s/{alias_a}"));
    let with_frag = session_visible_blob_alias_keyed(&key_a, &format!("{full}#B0-4")).unwrap();
    assert_eq!(with_frag, format!("tz://s/{alias_a}#B0-4"));
}

#[test]
fn aliases_full_hash_to_sixteen_hex_session_form() {
    let full = format!("tz://blob/{}", "ab".repeat(32));
    let short = session_visible_blob_alias(&full).unwrap();
    assert_eq!(short, format!("tz://s/{}", "ab".repeat(8)));
}

#[test]
fn preserves_byte_fragment() {
    let full = format!("fz://blob/{}#B0-12", "cd".repeat(32));
    let short = session_visible_blob_alias(&full).unwrap();
    assert_eq!(short, format!("tz://s/{}#B0-12", "cd".repeat(8)));
}

#[test]
fn leaves_non_full_hash_alone() {
    assert!(session_visible_blob_alias("tz://blob/b0123456789abcdef").is_none());
    assert!(session_visible_blob_alias("tz://s/abcdef0123456789").is_none());
    assert!(session_visible_blob_alias("tz://file/abc").is_none());
}

#[test]
fn rewrites_embedded_refs_in_text() {
    let hash = "11".repeat(32);
    let text = format!("stderr_ref: tz://blob/{hash}\nok");
    let out = rewrite_full_hash_blob_refs_in_text(&text);
    assert_eq!(out, format!("stderr_ref: tz://s/{}\nok", &hash[..16]));
}

#[test]
fn take_full_hash_reports_end_index() {
    let hash = "22".repeat(32);
    let text = format!("x tz://blob/{hash} y");
    let (end, full) = take_full_hash_blob_at(&text, 2).unwrap();
    assert_eq!(full, format!("tz://blob/{hash}"));
    assert_eq!(&text[end..], " y");
}

#[test]
fn ordinal_refs_are_generation_qualified() {
    let alias = session_ordinal_ref(7, 23);
    assert_eq!(alias, "tz://o/7/23");
    assert_eq!(parse_session_ordinal_bare(&alias), Some((7, 23)));
    assert!(!is_session_ordinal_bare("tz://o/0/1"));
    assert!(!is_session_ordinal_bare("tz://o/1/0"));
    assert!(!is_session_ordinal_bare("tz://o/1/2/3"));
}
