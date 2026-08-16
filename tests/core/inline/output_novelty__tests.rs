use super::*;
use crate::model_artifacts::ExactTokenizerIdentity;
use zero_gauge::ProviderLock;

struct ByteAdapter {
    identity: ExactTokenizerIdentity,
}

impl ExactTokenizerAdapter for ByteAdapter {
    fn identity(&self) -> &ExactTokenizerIdentity {
        &self.identity
    }

    fn encode(&self, source: &[u8]) -> Result<Vec<u32>, String> {
        Ok(source.iter().map(|byte| u32::from(*byte)).collect())
    }

    fn token_bytes(&self, token_id: u32) -> Result<Vec<u8>, String> {
        u8::try_from(token_id)
            .map(|byte| vec![byte])
            .map_err(|_| "token id is not one byte".to_string())
    }
}

fn d(byte: u8) -> DigestV1 {
    DigestV1::from_bytes([byte; 32])
}

fn adapter() -> ByteAdapter {
    let manifest = b"output-novelty-byte-tokenizer";
    ByteAdapter {
        identity: ExactTokenizerIdentity::new(
            ProviderLock {
                provider: "test-provider".to_string(),
                model: "test-model".to_string(),
                tokenizer_revision_digest: digest(manifest).to_hex(),
            },
            manifest,
        )
        .unwrap(),
    }
}

fn fields() -> Vec<OutputNoveltyFieldV1> {
    vec![
        OutputNoveltyFieldV1::new(
            "operation",
            OutputNoveltyFieldRoleV1::Deterministic,
            b"replace_exact_file".to_vec(),
        )
        .unwrap(),
        OutputNoveltyFieldV1::new(
            "target_ref",
            OutputNoveltyFieldRoleV1::Referenced,
            b"tz://blob/0123456789abcdef".to_vec(),
        )
        .unwrap(),
        OutputNoveltyFieldV1::new(
            "existing_prefix",
            OutputNoveltyFieldRoleV1::Reused,
            b"existing".to_vec(),
        )
        .unwrap(),
        OutputNoveltyFieldV1::new(
            "novel_body",
            OutputNoveltyFieldRoleV1::Novel,
            b"new body".to_vec(),
        )
        .unwrap(),
    ]
}

#[test]
fn coding_preserves_caller_order_and_accounts_exact_field_payloads() {
    let adapter = adapter();
    let first = OutputNoveltyCodingV1::encode(&adapter, d(1), d(2), d(3), fields()).unwrap();
    let second = OutputNoveltyCodingV1::encode(&adapter, d(1), d(2), d(3), fields()).unwrap();

    assert_eq!(first.encoded(), second.encoded());
    assert_eq!(first.receipt(), second.receipt());
    assert_eq!(first.receipt().encoding_digest(), digest(first.encoded()));
    assert_eq!(
        first.receipt().total_encoded_tokens(),
        first.encoded().len() as u64
    );
    assert_eq!(
        first
            .fields()
            .iter()
            .map(|field| field.name())
            .collect::<Vec<_>>(),
        ["operation", "target_ref", "existing_prefix", "novel_body"]
    );
    assert_eq!(first.fields()[3].bytes(), b"new body");
    assert_eq!(first.reconstruct_fields().unwrap(), fields());
    let totals = first.receipt().totals();
    assert_eq!(totals.novel_fields(), 1);
    assert_eq!(totals.novel_payload_bytes(), 8);
    assert_eq!(totals.novel_standalone_payload_tokens(), 8);
    assert_eq!(totals.reused_payload_bytes(), 8);
    assert_eq!(totals.deterministic_payload_bytes(), 18);
    assert_eq!(
        first.receipt().selection_origin(),
        OutputSelectionOriginV1::CallerSupplied
    );
    assert!(
        !serde_json::to_string(first.receipt())
            .unwrap()
            .contains("new body")
    );
}

/// [SPEC-TZ-NOV-002] Output novelty receipts do not carry recovery entity-novelty fields.
#[test]
fn output_novelty_receipt_schema_is_not_entity_novelty() {
    let adapter = adapter();
    let coding = OutputNoveltyCodingV1::encode(&adapter, d(1), d(2), d(3), fields()).unwrap();
    let receipt_json = serde_json::to_value(coding.receipt()).unwrap();
    let receipt_obj = receipt_json
        .as_object()
        .expect("OutputNoveltyReceiptV1 JSON is an object");
    assert_eq!(
        receipt_obj["schema_version"].as_str().unwrap(),
        OUTPUT_NOVELTY_SCHEMA_V1
    );
    assert_ne!(
        receipt_obj["schema_version"].as_str().unwrap(),
        "zerostack.entity-novelty.v1"
    );
    for entity_only in [
        "record_type",
        "scope_key",
        "entity_ids",
        "producing_engine",
        "updated_at",
        "cas_digest",
    ] {
        assert!(
            !receipt_obj.contains_key(entity_only),
            "OutputNoveltyReceiptV1 JSON must not carry entity-novelty field {entity_only}"
        );
    }
    let dumped = serde_json::to_string(&receipt_json).unwrap();
    assert!(
        !dumped.contains("zerostack.entity-novelty"),
        "output novelty JSON must not carry the entity-novelty schema id"
    );
    assert!(
        !dumped.contains("entity-novelty"),
        "output novelty JSON must not carry entity-novelty record_type"
    );
}

#[test]
fn role_is_caller_authority_not_inferred_from_equal_bytes() {
    let adapter = adapter();
    let reused = vec![
        OutputNoveltyFieldV1::new(
            "body",
            OutputNoveltyFieldRoleV1::Reused,
            b"same bytes".to_vec(),
        )
        .unwrap(),
    ];
    let novel = vec![
        OutputNoveltyFieldV1::new(
            "body",
            OutputNoveltyFieldRoleV1::Novel,
            b"same bytes".to_vec(),
        )
        .unwrap(),
    ];
    let reused = OutputNoveltyCodingV1::encode(&adapter, d(1), d(2), d(3), reused).unwrap();
    let novel = OutputNoveltyCodingV1::encode(&adapter, d(1), d(2), d(3), novel).unwrap();

    assert_ne!(
        reused.receipt().encoding_digest(),
        novel.receipt().encoding_digest()
    );
    assert_eq!(reused.receipt().totals().novel_payload_bytes(), 0);
    assert_eq!(novel.receipt().totals().novel_payload_bytes(), 10);
}

#[test]
fn oversized_payload_is_rejected_before_tokenization() {
    let adapter = adapter();
    let oversized = vec![
        OutputNoveltyFieldV1::new(
            "oversized",
            OutputNoveltyFieldRoleV1::Reused,
            vec![b'x'; MAX_OUTPUT_NOVELTY_BYTES],
        )
        .unwrap(),
    ];
    assert!(matches!(
        OutputNoveltyCodingV1::encode(&adapter, d(1), d(2), d(3), oversized),
        Err(OutputNoveltyError::EncodedByteLimit { .. })
    ));
}

#[test]
fn malformed_or_vacuous_codings_fail_loudly() {
    let adapter = adapter();
    assert!(matches!(
        OutputNoveltyCodingV1::encode(&adapter, DigestV1::ZERO, d(2), d(3), fields()),
        Err(OutputNoveltyError::ZeroIdentity("classification authority"))
    ));
    assert!(matches!(
        OutputNoveltyFieldV1::new("body", OutputNoveltyFieldRoleV1::Novel, Vec::new()),
        Err(OutputNoveltyError::EmptyNovelField(_))
    ));
    let duplicate = vec![
        OutputNoveltyFieldV1::new("body", OutputNoveltyFieldRoleV1::Referenced, b"a".to_vec())
            .unwrap(),
        OutputNoveltyFieldV1::new("body", OutputNoveltyFieldRoleV1::Novel, b"b".to_vec()).unwrap(),
    ];
    assert!(matches!(
        OutputNoveltyCodingV1::encode(&adapter, d(1), d(2), d(3), duplicate),
        Err(OutputNoveltyError::DuplicateFieldName(name)) if name == "body"
    ));
}
