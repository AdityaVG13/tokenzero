//! SPEC-TZ-TOK-001 / CAP-001 / CAP-002: exact identity + TokenPage MAX_* + expand.
//!
//! Minimal public-API driver. Does not treat dual-store fragment expand as
//! TokenPage/ModelCapsule round-trip.

use tokenzero_core::model_artifacts::{
    ExactTokenMap, ExactTokenizerAdapter, ExactTokenizerIdentity, MAX_CAPSULE_EVIDENCE_REFS,
    MAX_CAPSULE_RENDER_BYTES, MAX_CAPSULE_TOKEN_PAGES, MAX_TOKEN_PAGE_BYTES, MAX_TOKEN_PAGE_TOKENS,
    ModelArtifactError, TokenPage,
};
use tokenzero_core::sha256_hex;
use tokenzero_test_support::{
    ExecutionEnvelope, GauntletIdentityPair, GauntletOracle, ScenarioAgreement, scenario,
};
use zero_gauge::ProviderLock;

fn stamp_subject_ne_oracle() {
    GauntletIdentityPair::new(GauntletOracle::Spec).assert_distinct();
}

struct ByteAdapter {
    identity: ExactTokenizerIdentity,
}

impl ExactTokenizerAdapter for ByteAdapter {
    fn identity(&self) -> &ExactTokenizerIdentity {
        &self.identity
    }

    fn encode(&self, source: &[u8]) -> Result<Vec<u32>, String> {
        Ok(source.iter().copied().map(u32::from).collect())
    }

    fn token_bytes(&self, token_id: u32) -> Result<Vec<u8>, String> {
        let byte = u8::try_from(token_id).map_err(|err| err.to_string())?;
        Ok(vec![byte])
    }
}

fn adapter() -> ByteAdapter {
    let manifest = b"gauntlet-phase2-fake-tokenizer-rev";
    let digest = sha256_hex(std::str::from_utf8(manifest).expect("ascii manifest"));
    let identity = ExactTokenizerIdentity::new(
        ProviderLock {
            provider: "gauntlet".to_string(),
            model: "phase2".to_string(),
            tokenizer_revision_digest: digest,
        },
        manifest,
    )
    .expect("identity");
    ByteAdapter { identity }
}

fn blob_anchor(map: &ExactTokenMap) -> String {
    format!("tz://blob/{}", map.source_digest().to_hex())
}

#[test]
fn exact_tokenizer_identity_rejects_revision_digest_mismatch() {
    stamp_subject_ne_oracle();
    let expected = sha256_hex("gauntlet-expected-manifest");
    let err = ExactTokenizerIdentity::new(
        ProviderLock {
            provider: "gauntlet".to_string(),
            model: "phase2".to_string(),
            tokenizer_revision_digest: expected.clone(),
        },
        b"different-manifest-bytes",
    )
    .expect_err("mismatch must fail loud");
    match err {
        ModelArtifactError::TokenizerRevisionDigestMismatch {
            expected: got_expected,
            actual,
        } => {
            assert_eq!(got_expected, expected);
            assert_ne!(actual, expected);
        }
        other => panic!("expected digest mismatch, got {other:?}"),
    }
}

#[test]
fn token_page_and_capsule_max_contracts() {
    stamp_subject_ne_oracle();
    assert_eq!(MAX_TOKEN_PAGE_TOKENS, 4_096);
    assert_eq!(MAX_TOKEN_PAGE_BYTES, 1_048_576);
    assert_eq!(MAX_CAPSULE_EVIDENCE_REFS, 4_096);
    assert_eq!(MAX_CAPSULE_TOKEN_PAGES, 4_096);
    assert_eq!(MAX_CAPSULE_RENDER_BYTES, 16 * 1_048_576);
}

fn spec_empty_range_is_illegal(start: usize, end: usize) -> Result<(), ModelArtifactError> {
    if start >= end {
        Err(ModelArtifactError::EmptyTokenPage)
    } else {
        Ok(())
    }
}

#[test]
fn empty_token_page_both_error_is_spec_agreement() {
    stamp_subject_ne_oracle();
    let pair = GauntletIdentityPair::new(GauntletOracle::Spec);
    let envelope = ExecutionEnvelope::from_pair("empty-token-page", 1, pair, vec!["0..0".into()]);
    envelope.assert_engine_identities(pair);
    let adapter = adapter();
    let map = ExactTokenMap::tokenize(&adapter, b"abc").expect("map");
    let anchor = blob_anchor(&map);
    match scenario(
        "empty-token-page",
        pair,
        || TokenPage::new(&map, &anchor, 0..0).map(|page| page.expand()),
        || spec_empty_range_is_illegal(0, 0),
    ) {
        ScenarioAgreement::BothErr { subject, oracle } => {
            assert_eq!(subject, ModelArtifactError::EmptyTokenPage);
            assert_eq!(oracle, ModelArtifactError::EmptyTokenPage);
        }
        ScenarioAgreement::BothOk(_) => panic!("empty page must be both-error agreement, not Ok"),
    }
}

#[test]
fn token_page_expand_through_spec_scenario() {
    stamp_subject_ne_oracle();
    let pair = GauntletIdentityPair::new(GauntletOracle::Spec);
    let envelope = ExecutionEnvelope::from_pair("token-page-expand", 2, pair, vec!["0..3".into()]);
    envelope.assert_engine_identities(pair);
    let adapter = adapter();
    let source = b"abc";
    let map = ExactTokenMap::tokenize(&adapter, source).expect("map");
    let anchor = blob_anchor(&map);
    match scenario(
        "token-page-expand",
        pair,
        || TokenPage::new(&map, &anchor, 0..3).map(|page| page.expand()),
        || spec_empty_range_is_illegal(0, 3),
    ) {
        ScenarioAgreement::BothOk(bytes) => assert_eq!(bytes, source),
        ScenarioAgreement::BothErr { subject, oracle } => {
            panic!("in-range page must be BothOk, got subject={subject:?} oracle={oracle:?}")
        }
    }
}
