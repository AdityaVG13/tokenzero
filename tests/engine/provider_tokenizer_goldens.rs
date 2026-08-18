//! ZS-VIEW-008 provider tokenizer golden fixtures (schema
//! `tokenzero.tokenizer-goldens.v1`).
//!
//! No tokenizer vocabulary is linked in this repo; every registered family in
//! `tokenzero-core::tokens` reports `approximate: true`. These tests therefore
//! enforce the honesty contract:
//! - only `unverified: false` entries are asserted against
//!   `count_tokens_for_model`;
//! - `exact` verified entries must be provable without a vocabulary (empty
//!   string, single byte);
//! - `approximate` entries are recomputed from the disclosed
//!   chars-per-token heuristic and are never presented as exact;
//! - `unverified: true` entries carry NO numeric count (no fabricated counts
//!   enter the repo as facts);
//! - when a real tokenizer is ever linked, `metadata.approximate` flips to
//!   false and the honesty gate below fails, forcing every label to be
//!   re-reviewed.

use serde::Deserialize;
use tokenzero_core::{TokenizerFamily, count_tokens_for_model, tokenizer_metadata};
use tokenzero_test_support::{GauntletIdentityPair, GauntletOracle};

/// Live driver stamp: Subject vs ProviderTokenizer oracle. Never MCP
/// `EngineIdentity::TokenZero`.
fn stamp_gauntlet_subject_ne_oracle() {
    GauntletIdentityPair::new(GauntletOracle::ProviderTokenizer).assert_distinct();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CountClass {
    Exact,
    Approximate,
}

#[derive(Debug, Deserialize)]
struct GoldenEntry {
    id: String,
    provider: String,
    tokenizer_identity: String,
    tokenizer_revision: String,
    model_id: String,
    prompt_text: String,
    /// None exactly when `unverified` is true: TODO entries carry no count.
    expected_count: Option<u64>,
    count_class: CountClass,
    unverified: bool,
    source: String,
}

#[derive(Debug, Deserialize)]
struct GoldenFixture {
    schema: String,
    entries: Vec<GoldenEntry>,
}

fn fixture() -> GoldenFixture {
    stamp_gauntlet_subject_ne_oracle();
    serde_json::from_str(include_str!("fixtures/provider-tokenizer-goldens.json"))
        .expect("provider-tokenizer-goldens.json must parse")
}

/// Map a fixture tokenizer identity string to the core family that the
/// model_id actually resolves to. Family identity must never be conflated.
fn expected_family(identity: &str) -> Option<TokenizerFamily> {
    match identity {
        "cl100k_base" => Some(TokenizerFamily::Cl100k),
        "o200k_base" => Some(TokenizerFamily::O200k),
        "llama_sentencepiece_v3" => Some(TokenizerFamily::SentencePiece),
        _ => None,
    }
}

#[test]
fn gauntlet_subject_is_not_provider_tokenizer_oracle() {
    stamp_gauntlet_subject_ne_oracle();
}

#[test]
fn fixture_schema_is_tokenizer_goldens() {
    assert_eq!(fixture().schema, "tokenzero.tokenizer-goldens.v1");
    assert!(!fixture().entries.is_empty());
}

#[test]
fn tokenizer_identity_mismatch_does_not_resolve_to_a_family() {
    stamp_gauntlet_subject_ne_oracle();
    assert_ne!(
        expected_family("cl100k_base"),
        expected_family("o200k_base"),
        "cl100k and o200k identities must stay distinct"
    );
    assert_ne!(
        expected_family("cl100k_base"),
        expected_family("llama_sentencepiece_v3"),
        "cl100k and sentencepiece identities must stay distinct"
    );
    // Forbidden MCP registry labels and unknown strings are not tokenizer
    // families. They must not silently match a registered family.
    for identity in [
        "EngineIdentity::TokenZero",
        "RegistryEngine::TokenZero",
        "unknown_tokenizer_family",
        "",
    ] {
        assert_eq!(
            expected_family(identity),
            None,
            "{identity} must not map to a TokenizerFamily"
        );
    }
}

#[test]
fn every_entry_has_a_source_and_unverified_entries_carry_no_count() {
    for entry in &fixture().entries {
        assert!(
            !entry.source.is_empty(),
            "entry {} needs a source",
            entry.id
        );
        assert!(
            !entry.provider.is_empty(),
            "entry {} needs a provider",
            entry.id
        );
        assert!(
            !entry.tokenizer_identity.is_empty(),
            "entry {} needs a tokenizer identity",
            entry.id
        );
        assert!(
            !entry.tokenizer_revision.is_empty(),
            "entry {} needs a tokenizer revision",
            entry.id
        );
        // TODO entries are marked unverified and carry no numeric count, so
        // no fabricated count can enter the repo as a fact.
        assert_eq!(
            entry.unverified,
            entry.expected_count.is_none(),
            "entry {}: unverified must exactly mean expected_count is absent",
            entry.id
        );
        if entry.unverified {
            assert_eq!(
                entry.count_class,
                CountClass::Exact,
                "entry {}: TODO entries are future exact goldens, never approximations",
                entry.id
            );
        }
    }
}

#[test]
fn verified_entries_match_count_tokens_for_model() {
    for entry in &fixture().entries {
        if entry.unverified {
            continue;
        }
        let got = count_tokens_for_model(&entry.prompt_text, Some(&entry.model_id));
        let expected: u64 = entry
            .expected_count
            .expect("verified entry must carry a count");
        assert_eq!(
            u64::try_from(got).expect("token counts fit u64"),
            expected,
            "entry {} ({}): local count must match the asserted golden count",
            entry.id,
            entry.count_class_label()
        );
    }
}

#[test]
fn verified_exact_entries_are_provable_without_a_vocabulary() {
    for entry in &fixture().entries {
        if entry.unverified || entry.count_class != CountClass::Exact {
            continue;
        }
        let expected = entry
            .expected_count
            .expect("verified entry must carry a count");
        // The only exact counts asserted today are the empty string (0
        // tokens for every deterministic tokenizer) and single-byte inputs
        // (one base-vocabulary token in byte-level BPE). Anything else would
        // require a linked vocabulary and must stay unverified.
        let trivial = (entry.prompt_text.is_empty() && expected == 0)
            || (entry.prompt_text.chars().count() == 1 && expected == 1);
        assert!(
            trivial,
            "entry {}: exact verified count is not trivially provable without a vocabulary",
            entry.id
        );
    }
}

#[test]
fn approximate_entries_are_recomputed_from_the_disclosed_heuristic() {
    for entry in &fixture().entries {
        if entry.unverified || entry.count_class != CountClass::Approximate {
            continue;
        }
        let metadata = tokenizer_metadata(&entry.model_id)
            .unwrap_or_else(|| panic!("entry {}: model {} must resolve", entry.id, entry.model_id));
        let chars = entry.prompt_text.chars().count() as u64;
        let expected = chars
            .saturating_mul(1_000)
            .div_ceil(metadata.chars_per_token_milli as u64);
        assert_eq!(
            entry.expected_count,
            Some(expected),
            "entry {}: fixture count must equal the disclosed heuristic recomputed",
            entry.id
        );
    }
}

#[test]
fn approximate_counts_are_never_presented_as_exact() {
    for entry in &fixture().entries {
        let Some(metadata) = tokenizer_metadata(&entry.model_id) else {
            panic!("entry {}: model {} must resolve", entry.id, entry.model_id);
        };
        // Honesty gate: every family registered today is approximate. When a
        // real tokenizer is linked this fails, forcing label re-review.
        assert!(
            metadata.approximate,
            "entry {}: family {} must report approximate=true while no vocabulary is linked",
            entry.id,
            metadata.family.name()
        );
        // Approximate-derived counts must be labeled approximate in the
        // fixture; an approximate count is never presented as an exact count.
        if entry.count_class == CountClass::Approximate {
            assert!(
                metadata.approximate,
                "entry {}: approximate count must come from an approximate family",
                entry.id
            );
        }
        assert_eq!(
            entry.expected_family(),
            Some(metadata.family),
            "entry {}: fixture tokenizer identity must match the family core resolves",
            entry.id
        );
    }
}

impl GoldenEntry {
    fn expected_family(&self) -> Option<TokenizerFamily> {
        expected_family(&self.tokenizer_identity)
    }

    fn count_class_label(&self) -> &'static str {
        match self.count_class {
            CountClass::Exact => "exact",
            CountClass::Approximate => "approximate",
        }
    }
}
