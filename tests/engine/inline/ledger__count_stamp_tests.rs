//! TokenMass count-method-version stamp tests (ZS-VIEW-008).
//!
//! Every recorded count carries a method-version stamp naming the tokenizer
//! family, counting method, and method version. Legacy lines without a stamp
//! deserialize to the explicit `unstamped-legacy` marker -- never a fake
//! exact identity -- and recorded records always carry a real, non-legacy
//! stamp.

use super::*;
use tempfile::tempdir;
use tokenzero_core::{Accounting, ToolResponse};

#[test]
fn legacy_token_mass_without_stamp_defaults_to_unstamped_legacy_marker() {
    let legacy: TokenMass = serde_json::from_value(json!({
        "visible_tokens": 10,
        "raw_tokens": 20,
        "prevented_tokens": 5,
        "saved_bytes": 64
    }))
    .unwrap();
    assert!(
        legacy.count_method_version.is_legacy_unstamped(),
        "legacy data must deserialize to the explicit unstamped-legacy marker"
    );
    assert_eq!(
        legacy.count_method_version.tokenizer_family,
        UNSTAMPED_LEGACY
    );
    // The marker must never collide with a real counting identity.
    assert_ne!(UNSTAMPED_LEGACY, "none");
    assert_ne!(UNSTAMPED_LEGACY, "cl100k");
    assert_ne!(UNSTAMPED_LEGACY, "o200k");
    assert_ne!(UNSTAMPED_LEGACY, "sentencepiece");
}

#[test]
fn default_token_mass_is_explicitly_unstamped() {
    let stamp = TokenMass::default().count_method_version;
    assert!(stamp.is_legacy_unstamped());
    assert_eq!(stamp.method, "unknown");
    assert_eq!(stamp.version, "0");
}

#[test]
fn current_stamp_is_never_the_legacy_marker() {
    let stamp = current_count_method_version();
    assert!(
        !stamp.is_legacy_unstamped(),
        "a live stamp must never be the legacy marker"
    );
    assert!(!stamp.tokenizer_family.is_empty());
    assert!(!stamp.method.is_empty());
    assert!(!stamp.version.is_empty());
    assert!(
        stamp.tokenizer_family == "none"
            || matches!(
                stamp.tokenizer_family.as_str(),
                "cl100k" | "o200k" | "sentencepiece"
            ),
        "stamp family must name a real counting identity, got {}",
        stamp.tokenizer_family
    );
}

#[test]
fn stamped_token_mass_round_trips() {
    let stamped = TokenMass {
        visible_tokens: 7,
        raw_tokens: 9,
        prevented_tokens: 0,
        saved_bytes: 0,
        count_method_version: CountMethodVersion {
            tokenizer_family: "cl100k".to_string(),
            method: "average-char-width-estimate".to_string(),
            version: "tokenzero.approximate-count.v1".to_string(),
        },
    };
    let text = serde_json::to_string(&stamped).unwrap();
    let back: TokenMass = serde_json::from_str(&text).unwrap();
    assert_eq!(back, stamped);
    assert!(!back.count_method_version.is_legacy_unstamped());
}

#[test]
fn recorded_ledger_records_carry_a_non_legacy_stamp() {
    let directory = tempdir().unwrap();
    let cache_path = directory.path().join("cache.json");
    let writer = LedgerWriter::with_max_bytes(
        &cache_path,
        "session-stamp".to_owned(),
        "/workspace/repo".to_owned(),
        vec![],
        DEFAULT_MAX_LEDGER_BYTES,
    );
    let mut response = ToolResponse::default();
    response.accounting = Some(Accounting {
        raw_tokens: 200,
        visible_tokens: 50,
        recovery_tokens: 0,
        billed_tokens: 50,
        ..Accounting::default()
    });
    writer.record_response("read", &response);
    writer.flush();
    let records = read_records(&ledger_path_for_cache(&cache_path)).unwrap();
    assert_eq!(records.len(), 1);
    let stamp = &records[0].token_mass.count_method_version;
    assert!(
        !stamp.is_legacy_unstamped(),
        "every recorded count must carry a real method stamp, got {stamp:?}"
    );
    // No model env is set by engine tests, but the test host may set
    // TOKENZERO_MODEL/OMP_MODEL/OPENAI_MODEL, so the family is asserted
    // against the method actually stamped rather than hard-coded.
    match stamp.method.as_str() {
        "lexical-split" => assert_eq!(stamp.tokenizer_family, "none"),
        "average-char-width-estimate" => assert!(
            matches!(
                stamp.tokenizer_family.as_str(),
                "cl100k" | "o200k" | "sentencepiece"
            ),
            "approximate stamp must name the real family, got {stamp:?}"
        ),
        other => panic!("unexpected stamped method {other:?}"),
    }
}
