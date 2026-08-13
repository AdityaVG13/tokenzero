use super::*;
use zero_abi::sha256_hex;
use zero_ref::content_hash_hex;

fn identity(provider: &str, model: &str, manifest: &[u8]) -> ExactTokenizerIdentity {
    ExactTokenizerIdentity::new(
        ProviderLock {
            provider: provider.to_string(),
            model: model.to_string(),
            tokenizer_revision_digest: sha256_hex(manifest),
        },
        manifest,
    )
    .unwrap()
}

fn pieces(values: &[(u32, &[u8])]) -> Vec<TokenPiece> {
    values
        .iter()
        .map(|(token_id, bytes)| TokenPiece::new(*token_id, bytes.to_vec()))
        .collect()
}

struct PairTokenizer {
    identity: ExactTokenizerIdentity,
}

impl ExactTokenizerAdapter for PairTokenizer {
    fn identity(&self) -> &ExactTokenizerIdentity {
        &self.identity
    }

    fn encode(&self, source: &[u8]) -> Result<Vec<u32>, String> {
        Ok(source
            .chunks(2)
            .map(|chunk| match chunk {
                [first] => (1u32 << 24) | u32::from(*first),
                [first, second] => (2u32 << 24) | (u32::from(*first) << 8) | u32::from(*second),
                _ => unreachable!("chunks(2) has length one or two"),
            })
            .collect())
    }

    fn token_bytes(&self, token_id: u32) -> Result<Vec<u8>, String> {
        match token_id >> 24 {
            1 => Ok(vec![token_id as u8]),
            2 => Ok(vec![(token_id >> 8) as u8, token_id as u8]),
            width => Err(format!("unsupported fixture token width {width}")),
        }
    }
}

fn exact_map(identity: &ExactTokenizerIdentity, source: &[u8]) -> ExactTokenMap {
    ExactTokenMap::tokenize(
        &PairTokenizer {
            identity: identity.clone(),
        },
        source,
    )
    .unwrap()
}

#[test]
fn exact_identity_is_zero_gauge_locked_and_zero_ledger_compatible() {
    let manifest = br#"{"vocab":"fixture","merges":"v1"}"#;
    let tokenizer = identity("fixture-provider", "fixture-model", manifest);
    assert_eq!(
        tokenizer.provider_lock().tokenizer_revision_digest,
        sha256_hex(manifest)
    );
    let ledger = tokenizer.ledger_identity();
    assert_eq!(
        ledger.tokenizer_id,
        format!("fixture-provider/fixture-model@{}", tokenizer.digest())
    );
    assert_eq!(
        ledger.tokenizer_version_digest.to_hex(),
        sha256_hex(manifest)
    );
    let slash_left = identity("a/b", "c", manifest);
    let slash_right = identity("a", "b/c", manifest);
    assert_ne!(slash_left.ledger_identity(), slash_right.ledger_identity());

    let wrong_manifest =
        ExactTokenizerIdentity::new(tokenizer.provider_lock().clone(), b"different revision");
    assert!(matches!(
        wrong_manifest,
        Err(ModelArtifactError::TokenizerRevisionDigestMismatch { .. })
    ));

    let encoded = serde_json::to_value(&tokenizer).unwrap();
    assert_eq!(
        serde_json::from_value::<ExactTokenizerIdentity>(encoded.clone()).unwrap(),
        tokenizer
    );
    let mut tampered = encoded;
    tampered["identity_digest"] = serde_json::Value::String("00".repeat(32));
    assert!(serde_json::from_value::<ExactTokenizerIdentity>(tampered).is_err());
}

#[test]
fn exact_map_roundtrips_and_requires_real_token_boundaries() {
    let tokenizer = identity("fixture-provider", "fixture-model", b"revision");
    let map = exact_map(&tokenizer, b"abcdef");
    assert_eq!(map.reconstruct(), b"abcdef");
    assert_eq!(map.byte_range_for_tokens(1..3).unwrap(), 2..6);
    assert_eq!(map.token_range_for_bytes(2..6).unwrap(), 1..3);
    assert_eq!(
        map.token_range_for_bytes(1..6),
        Err(ModelArtifactError::TokenBoundaryRequired { byte_offset: 1 })
    );
    assert!(matches!(
        ExactTokenMap::from_token_pieces(
            &tokenizer,
            b"abcdef",
            pieces(&[(10, b"ab"), (11, b"X"), (12, b"def")]),
        ),
        Err(ModelArtifactError::TokenBytesMismatch { token_index: 1, .. })
    ));
}

#[test]
fn token_page_is_bounded_source_anchored_and_exactly_expandable() {
    let tokenizer = identity("fixture-provider", "fixture-model", b"revision");
    let source = b"abcdef";
    let map = exact_map(&tokenizer, source);
    let anchor = format!("tz://blob/{}", content_hash_hex(source));
    let page = TokenPage::new(&map, &anchor, 1..3).unwrap();
    assert_eq!(page.token_range(), 1..3);
    assert_eq!(page.byte_range(), 2..6);
    assert_eq!(page.expand(), b"cdef");
    assert_eq!(page.source_anchor(), anchor);

    let fragmented = format!("{anchor}#B0-2");
    assert_eq!(
        TokenPage::new(&map, &fragmented, 0..1),
        Err(ModelArtifactError::SourceAnchorMustBeWholeBlob)
    );
    let wrong = format!("tz://blob/{}", content_hash_hex(b"different"));
    assert!(matches!(
        TokenPage::new(&map, &wrong, 0..1),
        Err(ModelArtifactError::SourceDigestMismatch { .. })
    ));
}

#[test]
fn capsule_digest_is_canonical_and_provider_locked() {
    let tokenizer = identity("fixture-provider", "fixture-model", b"revision");
    let source = b"abcdef";
    let source_map = exact_map(&tokenizer, source);
    let source_anchor = format!("tz://blob/{}", content_hash_hex(source));
    let page_a = TokenPage::new(&source_map, &source_anchor, 0..1).unwrap();
    let page_b = TokenPage::new(&source_map, &source_anchor, 1..3).unwrap();
    let stable = exact_map(&tokenizer, b"sys:");
    let dynamic = exact_map(&tokenizer, b"ask");
    let other_ref = format!("fz://blob/{}", content_hash_hex(b"other"));

    let one = ModelCapsule::new(
        DigestV1::from_bytes([8; 32]),
        DigestV1::from_bytes([9; 32]),
        &tokenizer,
        vec![source_anchor.clone(), other_ref.clone()],
        &[page_a.clone(), page_b.clone()],
        &stable,
        &dynamic,
    )
    .unwrap();
    let reordered = ModelCapsule::new(
        DigestV1::from_bytes([8; 32]),
        DigestV1::from_bytes([9; 32]),
        &tokenizer,
        vec![other_ref, source_anchor],
        &[page_b, page_a],
        &stable,
        &dynamic,
    )
    .unwrap();
    assert_eq!(one.digest(), reordered.digest());
    assert_eq!(one.render(), b"sys:ask");
    assert_eq!(one.stable_prefix_tokens(), 2);
    assert_eq!(one.dynamic_tail_tokens(), 2);
    assert_eq!(one.total_tokens(), 4);

    let foreign = identity("fixture-provider", "other-model", b"revision");
    let foreign_tail = exact_map(&foreign, b"ask");
    assert_eq!(
        ModelCapsule::new(
            DigestV1::from_bytes([8; 32]),
            DigestV1::from_bytes([9; 32]),
            &tokenizer,
            Vec::new(),
            &[],
            &stable,
            &foreign_tail,
        ),
        Err(ModelArtifactError::TokenizerIdentityMismatch)
    );
}
