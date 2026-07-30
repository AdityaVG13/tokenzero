//! Opacity regression tests for visible ref aliases (W4-OPAQUE-CAS-ALIAS,
//! W4-DIRECT-HASH-KILL): a rendered transcript carrying `tz://s/` aliases
//! must contain neither the raw payload nor its content hash, and alias →
//! payload resolution must stay internal to the recovery store.

use tokenzero_core::ContentType;
use tokenzero_recovery::RecoveryStore;

#[test]
fn visible_alias_contains_neither_payload_nor_content_hash() {
    let payload = "super secret payload bytes: hunter2";
    let mut store = RecoveryStore::new(None);
    let full = store.store_blob(payload, ContentType::Unknown).unwrap();
    let hash = full.strip_prefix("tz://blob/").expect("blob ref");

    // The exact-ref visible form: text rewritten through the alias tier.
    let transcript = store.apply_session_visible_aliases_in_text(&format!("see {full} for details"));

    assert!(!transcript.contains(payload), "transcript leaks payload: {transcript}");
    assert!(!transcript.contains(hash), "transcript leaks content hash: {transcript}");
    assert!(
        !transcript.contains(&hash[..16]),
        "transcript leaks content-hash prefix: {transcript}"
    );
    assert!(transcript.contains("tz://s/"), "expected an alias: {transcript}");

    // Resolution stays internal: the alias expands back to the payload bytes.
    let alias = transcript
        .split_whitespace()
        .find(|t| t.starts_with("tz://s/"))
        .expect("alias token")
        .to_string();
    let expanded = store.expand(&alias, None, None, None, None, None);
    assert!(expanded.found, "alias must resolve internally: {}", expanded.reason);
    assert_eq!(expanded.content, payload);
}

#[test]
fn alias_agrees_across_store_restart_and_stays_opaque() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("recovery.json");
    let payload = "restart persistence check";

    let (full, alias) = {
        let mut store = RecoveryStore::new(Some(cache.clone()));
        let full = store.store_blob(payload, ContentType::Unknown).unwrap();
        let alias = store.ensure_session_visible_alias(&full);
        store.persist_pending().unwrap();
        (full, alias)
    };
    let hash = full.strip_prefix("tz://blob/").unwrap();
    assert!(!alias.contains(&hash[..16]), "alias is content-derived: {alias}");

    // A fresh process on the same store root derives the SAME alias (shared
    // persisted key) and resolves it to the same bytes.
    let mut restarted = RecoveryStore::new(Some(cache));
    let alias2 = restarted.register_session_visible_alias(&full);
    assert_eq!(alias, alias2, "shared store key must make aliases agree");
    let expanded = restarted.expand(&alias2, None, None, None, None, None);
    assert!(expanded.found, "{}", expanded.reason);
    assert_eq!(expanded.content, payload);
}

#[test]
fn distinct_stores_mint_distinct_aliases_for_identical_payload() {
    // Membership/identity opacity: the same payload in two unrelated stores
    // (distinct keys) must not produce correlatable visible handles.
    let payload = "identical content in both stores";
    let mut a = RecoveryStore::new(None);
    let mut b = RecoveryStore::new(None);
    let full_a = a.store_blob(payload, ContentType::Unknown).unwrap();
    let full_b = b.store_blob(payload, ContentType::Unknown).unwrap();
    assert_eq!(full_a, full_b, "CAS identity is internal and unchanged");
    let alias_a = a.register_session_visible_alias(&full_a);
    let alias_b = b.register_session_visible_alias(&full_b);
    assert_ne!(alias_a, alias_b, "visible handles must not correlate across stores");
}
