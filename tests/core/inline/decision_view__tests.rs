use super::*;
use crate::model_artifacts::{ExactTokenizerIdentity, TokenPiece};
use zero_abi::sha256_hex;
use zero_gauge::ProviderLock;
use zero_ref::content_hash_hex;

#[derive(Clone)]
struct ByteTokenizer {
    identity: ExactTokenizerIdentity,
}

impl ExactTokenizerAdapter for ByteTokenizer {
    fn identity(&self) -> &ExactTokenizerIdentity {
        &self.identity
    }

    fn encode(&self, source: &[u8]) -> Result<Vec<u32>, String> {
        Ok(source.iter().map(|byte| u32::from(*byte)).collect())
    }

    fn token_bytes(&self, token_id: u32) -> Result<Vec<u8>, String> {
        u8::try_from(token_id)
            .map(|byte| vec![byte])
            .map_err(|_| "fixture token id exceeds byte range".to_string())
    }
}

#[derive(Clone)]
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
            _ => Err("invalid fixture token width".to_string()),
        }
    }
}

fn tokenizer_identity() -> ExactTokenizerIdentity {
    let manifest = b"decision-view-fixture-tokenizer";
    ExactTokenizerIdentity::new(
        ProviderLock {
            provider: "fixture-provider".to_string(),
            model: "fixture-model".to_string(),
            tokenizer_revision_digest: sha256_hex(manifest),
        },
        manifest,
    )
    .unwrap()
}

fn identity(tokenizer: &ExactTokenizerIdentity) -> DecisionViewIdentity {
    DecisionViewIdentity::new(
        DigestV1::from_bytes([1; 32]),
        DigestV1::from_bytes([2; 32]),
        tokenizer,
        digest(b"typed effect schema"),
    )
}

fn byte_map(tokenizer: &ExactTokenizerIdentity, bytes: &[u8]) -> ExactTokenMap {
    ExactTokenMap::tokenize(
        &ByteTokenizer {
            identity: tokenizer.clone(),
        },
        bytes,
    )
    .unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn decision_view_is_deterministic_and_preserves_anchors_and_routes() {
    let tokenizer = tokenizer_identity();
    let adapter = ByteTokenizer {
        identity: tokenizer.clone(),
    };
    let source = b"exact locus evidence";
    let source_map = byte_map(&tokenizer, source);
    let anchor = format!("fz://blob/{}", content_hash_hex(source));
    let page = TokenPage::new(&source_map, &anchor, 0..source.len()).unwrap();
    let stable_capsule_map = byte_map(&tokenizer, b"project capsule");
    let empty_tail = byte_map(&tokenizer, b"");
    let capsule = ModelCapsule::new(
        DigestV1::from_bytes([1; 32]),
        DigestV1::from_bytes([2; 32]),
        &tokenizer,
        vec![anchor.clone()],
        std::slice::from_ref(&page),
        &stable_capsule_map,
        &empty_tail,
    )
    .unwrap();
    let marker = DecisionUncertaintyMarker::new(
        DecisionUncertaintyKind::PartialCoverage,
        "coverage_gap",
        "generated source is not covered",
        vec![anchor.clone()],
    )
    .unwrap();
    let sections = vec![
        DecisionViewSection::stable_system_tool_contract(&byte_map(
            &tokenizer,
            b"system and tool contract",
        ))
        .unwrap(),
        DecisionViewSection::stable_project_capsule(&capsule).unwrap(),
        DecisionViewSection::stable_typed_effect_schema(&byte_map(
            &tokenizer,
            b"typed effect schema",
        ))
        .unwrap(),
        DecisionViewSection::volatile_locus_evidence(&page).unwrap(),
        DecisionViewSection::volatile_uncertainty_coverage(&marker).unwrap(),
        DecisionViewSection::volatile_recovery_routes(vec![anchor.clone()]).unwrap(),
        DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, b"fix the task")).unwrap(),
    ];
    let first = DecisionView::render(&adapter, identity(&tokenizer), sections.clone()).unwrap();
    let second = DecisionView::render(&adapter, identity(&tokenizer), sections).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.digest(), second.digest());
    assert_eq!(first.total_tokens(), first.rendered().len() as u64);
    assert_eq!(
        first.stable_prefix().compare(second.stable_prefix()),
        PrefixComparison::PrefixIdentical
    );
    assert!(first.rendered().starts_with(first.stable_prefix().bytes()));
    assert!(contains(first.rendered(), anchor.as_bytes()));
    assert!(contains(first.rendered(), b"partial_coverage"));
    assert!(contains(first.rendered(), source));
    assert!(first.volatile_bytes() > 0);
    assert!(first.volatile_tokens() > 0);
}

#[test]
fn prefix_comparison_separates_identity_changes_from_byte_changes() {
    let tokenizer = tokenizer_identity();
    let adapter = ByteTokenizer {
        identity: tokenizer.clone(),
    };
    let sections = |stable: &[u8]| {
        vec![
            DecisionViewSection::stable_system_tool_contract(&byte_map(&tokenizer, stable))
                .unwrap(),
            DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, b"task")).unwrap(),
        ]
    };
    let original =
        DecisionView::render(&adapter, identity(&tokenizer), sections(b"stable")).unwrap();
    let mut changed_identity = identity(&tokenizer);
    changed_identity.model_profile_digest = DigestV1::from_bytes([9; 32]);
    let identity_changed =
        DecisionView::render(&adapter, changed_identity, sections(b"stable")).unwrap();
    let bytes_changed =
        DecisionView::render(&adapter, identity(&tokenizer), sections(b"changed")).unwrap();

    assert_eq!(
        original.stable_prefix().bytes_digest(),
        identity_changed.stable_prefix().bytes_digest()
    );
    assert_eq!(
        original
            .stable_prefix()
            .compare(identity_changed.stable_prefix()),
        PrefixComparison::IdentityChanged
    );
    assert_eq!(
        original
            .stable_prefix()
            .compare(bytes_changed.stable_prefix()),
        PrefixComparison::PrefixBytesChanged
    );
}

#[test]
fn renderer_preserves_caller_order_and_rejects_stable_after_volatile() {
    let tokenizer = tokenizer_identity();
    let adapter = ByteTokenizer {
        identity: tokenizer.clone(),
    };
    let sections = vec![
        DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, b"task")).unwrap(),
        DecisionViewSection::stable_system_tool_contract(&byte_map(&tokenizer, b"stable")).unwrap(),
    ];
    assert_eq!(
        DecisionView::render(&adapter, identity(&tokenizer), sections),
        Err(DecisionViewError::StableSectionAfterVolatile { index: 1 })
    );
}

#[test]
fn prefix_boundary_must_be_a_real_token_boundary() {
    let tokenizer = tokenizer_identity();
    let byte_adapter = ByteTokenizer {
        identity: tokenizer.clone(),
    };
    let pair_adapter = PairTokenizer {
        identity: tokenizer.clone(),
    };
    let mut stable = b"x".to_vec();
    loop {
        let trial = DecisionView::render(
            &byte_adapter,
            identity(&tokenizer),
            vec![
                DecisionViewSection::stable_system_tool_contract(&byte_map(&tokenizer, &stable))
                    .unwrap(),
                DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, b"tail")).unwrap(),
            ],
        )
        .unwrap();
        if trial.stable_prefix().breakpoint_after_bytes() % 2 == 1 {
            break;
        }
        stable.push(b'x');
    }
    let result = DecisionView::render(
        &pair_adapter,
        identity(&tokenizer),
        vec![
            DecisionViewSection::stable_system_tool_contract(&byte_map(&tokenizer, &stable))
                .unwrap(),
            DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, b"tail")).unwrap(),
        ],
    );

    assert!(matches!(
        result,
        Err(DecisionViewError::PrefixNotTokenAligned { .. })
    ));
}

#[test]
fn capsule_identity_and_recovery_ref_mismatches_fail_loudly() {
    let tokenizer = tokenizer_identity();
    let adapter = ByteTokenizer {
        identity: tokenizer.clone(),
    };
    let capsule = ModelCapsule::new(
        DigestV1::from_bytes([9; 32]),
        DigestV1::from_bytes([2; 32]),
        &tokenizer,
        Vec::new(),
        &[],
        &byte_map(&tokenizer, b"capsule"),
        &byte_map(&tokenizer, b""),
    )
    .unwrap();
    let section = DecisionViewSection::stable_project_capsule(&capsule).unwrap();
    assert_eq!(
        DecisionView::render(&adapter, identity(&tokenizer), vec![section]),
        Err(DecisionViewError::CapsuleSourceRootMismatch)
    );

    let schema = byte_map(&tokenizer, b"typed effect schema");
    let schema_section = DecisionViewSection::stable_typed_effect_schema(&schema).unwrap();
    let mut wrong_schema_identity = identity(&tokenizer);
    wrong_schema_identity.tool_schema_digest = DigestV1::from_bytes([7; 32]);
    assert_eq!(
        DecisionView::render(&adapter, wrong_schema_identity, vec![schema_section]),
        Err(DecisionViewError::ToolSchemaDigestMismatch)
    );

    assert!(matches!(
        DecisionViewSection::volatile_recovery_routes(vec!["tz://not-portable".to_string()]),
        Err(DecisionViewError::InvalidRecoveryRef(_))
    ));
    let valid_ref = format!("tz://blob/{}", content_hash_hex(b"bounded"));
    assert_eq!(
        DecisionViewSection::volatile_recovery_routes(vec![
            valid_ref;
            MAX_DECISION_VIEW_RECOVERY_REFS + 1
        ]),
        Err(DecisionViewError::TooManyRecoveryRefs {
            actual: MAX_DECISION_VIEW_RECOVERY_REFS + 1,
            limit: MAX_DECISION_VIEW_RECOVERY_REFS,
        })
    );
}

#[test]
fn renderer_contract_digest_is_stable_and_nonzero() {
    let digest = decision_view_renderer_contract_digest();
    assert_ne!(digest, DigestV1::ZERO);
    assert_eq!(digest, decision_view_renderer_contract_digest());
    let _ = TokenPiece::new(1, b"used-by-public-adapter".to_vec());
}
