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
        Sha256Digest::from_bytes([8; 32]),
        Sha256Digest::from_bytes([9; 32]),
        &tokenizer,
        vec![source_anchor.clone(), other_ref.clone()],
        &[page_a.clone(), page_b.clone()],
        &stable,
        &dynamic,
    )
    .unwrap();
    let reordered = ModelCapsule::new(
        Sha256Digest::from_bytes([8; 32]),
        Sha256Digest::from_bytes([9; 32]),
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
            Sha256Digest::from_bytes([8; 32]),
            Sha256Digest::from_bytes([9; 32]),
            &tokenizer,
            Vec::new(),
            &[],
            &stable,
            &foreign_tail,
        ),
        Err(ModelArtifactError::TokenizerIdentityMismatch)
    );

    // The core constructor forms a content-addressed causal key and a
    // receipt binding the payload: identical inputs, identical receipts.
    let receipt = one.formation_receipt();
    assert_eq!(receipt.constructor_identity, "tokenzero-core.model-capsule.v1");
    assert_eq!(receipt.epoch, 0);
    assert_eq!(
        receipt.payload_root,
        ModelCapsule::payload_digest(b"sys:ask")
    );
    assert!(receipt.verify_payload(ModelCapsule::payload_digest(one.render().as_slice())));
    assert_eq!(one.causal_key(), reordered.causal_key());
    assert_eq!(one.causal_key().as_str(), format!("tz://blob/{}", one.source_root_digest().to_hex()));
}

#[test]
fn formation_receipt_is_canonical_and_rejects_relabeled_payloads() {
    let payload = Sha256Digest::from_bytes([1; 32]);
    let other_payload = Sha256Digest::from_bytes([2; 32]);
    let contract = Sha256Digest::from_bytes([3; 32]);
    let receipt = ModelCapsuleFormationReceipt::new(
        "fixture.constructor.v1",
        contract,
        vec!["tz://blob/dep-a".into(), "tz://blob/dep-b".into()],
        payload,
        7,
    )
    .unwrap();

    // Exact payload verifies; a relabeled payload fails the binding.
    assert!(receipt.verify_payload(payload));
    assert!(!receipt.verify_payload(other_payload));

    // The receipt is digest-canonical: any field change moves the root.
    let root = receipt.receipt_digest().unwrap();
    let mut tampered = receipt.clone();
    tampered.epoch = 99;
    assert_ne!(tampered.receipt_digest().unwrap(), root);
    let mut tampered_root = receipt.clone();
    tampered_root.contract_root = other_payload;
    assert_ne!(tampered_root.receipt_digest().unwrap(), root);

    // Unsupported versions and empty fields fail loud.
    let mut bad_version = receipt.clone();
    bad_version.receipt_version = 99;
    assert!(matches!(
        bad_version.receipt_digest(),
        Err(ModelArtifactError::UnsupportedReceiptVersion { actual: 99 })
    ));
    assert!(matches!(
        ModelCapsuleFormationReceipt::new("", contract, Vec::new(), payload, 0),
        Err(ModelArtifactError::EmptyConstructorIdentity)
    ));
    assert!(matches!(
        ModelCapsuleFormationReceipt::new("c", contract, vec!["".into()], payload, 0),
        Err(ModelArtifactError::EmptyDependencyRoot)
    ));
    assert!(matches!(
        CapsuleCausalKey::new(""),
        Err(ModelArtifactError::EmptyCausalKey)
    ));

    // Serde round-trip preserves the receipt and rejects unknown fields.
    let encoded = serde_json::to_value(&receipt).unwrap();
    assert_eq!(
        serde_json::from_value::<ModelCapsuleFormationReceipt>(encoded.clone()).unwrap(),
        receipt
    );
    let mut tampered_json = encoded;
    tampered_json["epoch"] = serde_json::json!(8);
    assert_ne!(
        serde_json::from_value::<ModelCapsuleFormationReceipt>(tampered_json).unwrap(),
        receipt
    );
}

#[test]
fn formed_capsule_receipt_must_bind_payload_and_slots_reject_rewrites() {
    let key = CapsuleCausalKey::new("fixture:1..2").unwrap();
    let other_key = CapsuleCausalKey::new("fixture:3..4").unwrap();
    let blob_a = format!("tz://blob/{}", content_hash_hex(b"alpha"));
    let blob_b = format!("tz://blob/{}", content_hash_hex(b"beta"));
    let source_root = Sha256Digest::from_bytes([8; 32]);

    let form = |key: &CapsuleCausalKey, payload: &[u8], dep: String| {
        let contract_root = key.contract_root().unwrap();
        let payload_root = ModelCapsule::payload_digest(payload);
        let receipt = ModelCapsuleFormationReceipt::new(
            "fixture.formed.v1",
            contract_root,
            vec![dep.clone()],
            payload_root,
            0,
        )
        .unwrap();
        ModelCapsule::from_formed(
            key.clone(),
            receipt,
            source_root,
            ModelCapsule::absent_model_profile_digest(),
            ModelCapsule::absent_tokenizer_digest(),
            vec![dep],
            Vec::new(),
            payload,
            payload_root,
            u64::try_from(payload.len()).unwrap(),
            &[],
            ModelCapsule::payload_digest(&[]),
            0,
        )
        .unwrap()
    };

    let alpha = form(&key, b"alpha", blob_a.clone());
    assert!(alpha
        .formation_receipt()
        .verify_payload(ModelCapsule::payload_digest(alpha.render().as_slice())));

    // A relabeled payload (receipt binds beta, capsule carries alpha) fails
    // loud at formation: append-never-rewrite starts at construction.
    let contract_root = key.contract_root().unwrap();
    let beta_root = ModelCapsule::payload_digest(b"beta");
    let relabeled_receipt = ModelCapsuleFormationReceipt::new(
        "fixture.formed.v1",
        contract_root,
        vec![blob_a.clone()],
        beta_root,
        0,
    )
    .unwrap();
    assert!(matches!(
        ModelCapsule::from_formed(
            key.clone(),
            relabeled_receipt,
            source_root,
            ModelCapsule::absent_model_profile_digest(),
            ModelCapsule::absent_tokenizer_digest(),
            vec![blob_a.clone()],
            Vec::new(),
            b"alpha",
            ModelCapsule::payload_digest(b"alpha"),
            5,
            &[],
            ModelCapsule::payload_digest(&[]),
            0,
        ),
        Err(ModelArtifactError::CapsuleReceiptPayloadMismatch { .. })
    ));

    // Append-never-rewrite: same key + different bytes fails loud; identical
    // re-record is idempotent; a different key is a fresh slot.
    let mut slots = AppendOnlyCapsuleSlots::new();
    slots.record(&alpha).unwrap();
    slots.record(&alpha).unwrap();
    let beta = form(&key, b"beta", blob_b.clone());
    assert!(matches!(
        slots.record(&beta),
        Err(ModelArtifactError::CapsuleCausalKeyRewrite { key }) if key == "fixture:1..2"
    ));
    let other = form(&other_key, b"gamma", blob_a.clone());
    slots.record(&other).unwrap();
    assert_eq!(slots.len(), 2);
    assert!(!slots.is_empty());
}
