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
        Sha256Digest::from_bytes([1; 32]),
        Sha256Digest::from_bytes([2; 32]),
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
        Sha256Digest::from_bytes([1; 32]),
        Sha256Digest::from_bytes([2; 32]),
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
    changed_identity.model_profile_digest = Sha256Digest::from_bytes([9; 32]);
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
        Sha256Digest::from_bytes([9; 32]),
        Sha256Digest::from_bytes([2; 32]),
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
    wrong_schema_identity.tool_schema_digest = Sha256Digest::from_bytes([7; 32]);
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
    assert_ne!(digest, Sha256Digest::ZERO);
    assert_eq!(digest, decision_view_renderer_contract_digest());
    let _ = TokenPiece::new(1, b"used-by-public-adapter".to_vec());
}

#[test]
fn v6_metadata_round_trips_is_digest_covered_and_unknown_never_upgrades() {
    let tokenizer = tokenizer_identity();
    let adapter = ByteTokenizer {
        identity: tokenizer.clone(),
    };
    let sections = vec![
        DecisionViewSection::stable_system_tool_contract(&byte_map(&tokenizer, b"stable")).unwrap(),
        DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, b"task")).unwrap(),
    ];
    let choice_a = CandidateChoice::new("retry", "retry the last operation").unwrap();
    let choice_b = CandidateChoice::new("abort", "abort the operation").unwrap();
    let metadata_a = DecisionViewMetadata::new(
        vec![choice_a],
        vec!["decision:retry".to_string(), "decision:abort".to_string()],
        CompletenessGrade::BoundedComplete,
        true,
    )
    .unwrap();
    let metadata_b = DecisionViewMetadata::new(
        vec![choice_b],
        vec!["decision:abort".to_string()],
        CompletenessGrade::BoundedComplete,
        true,
    )
    .unwrap();
    let view_a = DecisionView::render_with_metadata(
        &adapter,
        identity(&tokenizer),
        sections.clone(),
        metadata_a.clone(),
    )
    .unwrap();
    let view_b =
        DecisionView::render_with_metadata(&adapter, identity(&tokenizer), sections, metadata_b)
            .unwrap();

    // Metadata is digest-covered: changing candidate_choices changes the view
    // digest while leaving the rendered framing bytes untouched (and the
    // stable-prefix geometry unchanged -- metadata is view-level, not prefix).
    assert_ne!(view_a.digest(), view_b.digest());
    assert_eq!(view_a.rendered(), view_b.rendered());
    assert_eq!(
        view_a.stable_prefix().digest(),
        view_b.stable_prefix().digest()
    );

    // All new fields round-trip through serde, and old-shaped JSON still
    // deserializes with serde defaults.
    let json = serde_json::to_string(&metadata_a).unwrap();
    assert_eq!(
        serde_json::from_str::<DecisionViewMetadata>(&json).unwrap(),
        metadata_a
    );
    assert_eq!(
        serde_json::from_str::<DecisionViewMetadata>("{}").unwrap(),
        DecisionViewMetadata::default()
    );
    assert_eq!(
        serde_json::from_str::<CompletenessGrade>("\"BoundedComplete\"").unwrap(),
        CompletenessGrade::BoundedComplete
    );
    let view_json = serde_json::to_string(&view_a).unwrap();
    assert!(view_json.contains("\"candidate_choices\""));
    assert!(view_json.contains("\"supported_decisions\""));
    assert!(view_json.contains("\"completeness_grade\":\"BoundedComplete\""));
    assert!(view_json.contains("\"baseline_escape\":true"));

    // Unknown is terminal: it can never be constructed as upgraded.
    assert_eq!(
        metadata_a.completeness_grade(),
        CompletenessGrade::BoundedComplete
    );
    assert!(metadata_a.baseline_escape());
    assert_eq!(metadata_a.candidate_choices().len(), 1);
    assert_eq!(metadata_a.supported_decisions().len(), 2);
    assert_eq!(
        CompletenessGrade::Unknown.join(CompletenessGrade::Proved),
        CompletenessGrade::Unknown
    );
    assert_eq!(
        CompletenessGrade::Proved.join(CompletenessGrade::Unknown),
        CompletenessGrade::Unknown
    );
    assert_eq!(
        CompletenessGrade::BoundedComplete.join(CompletenessGrade::Observed),
        CompletenessGrade::Observed
    );
    assert_eq!(CompletenessGrade::default(), CompletenessGrade::Unknown);
    assert_eq!(
        DecisionViewMetadata::default().completeness_grade(),
        CompletenessGrade::Unknown
    );
    assert!(matches!(
        CandidateChoice::new("", "empty id"),
        Err(DecisionViewError::EmptyChoiceId)
    ));
}

/// Tiny deterministic LCG (no external rand crate). Fixed seed makes the
/// permutation sequence reproducible across runs and platforms.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
}

/// Fisher-Yates shuffle driven by the LCG.
fn shuffle<T>(rng: &mut Lcg, values: &mut [T]) {
    for index in (1..values.len()).rev() {
        let swap = (rng.next() % (index as u64 + 1)) as usize;
        values.swap(index, swap);
    }
}

/// All permutations of a small slice, in a stable generation order.
fn permutations<T: Clone>(values: Vec<T>) -> Vec<Vec<T>> {
    if values.len() <= 1 {
        return vec![values];
    }
    let mut out = Vec::new();
    for index in 0..values.len() {
        let mut rest = values.clone();
        rest.remove(index);
        for mut tail in permutations(rest) {
            tail.insert(0, values[index].clone());
            out.push(tail);
        }
    }
    out
}

fn position_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// VIEW-004: rendering is byte-identical under randomized caller order of
/// commutative sections. Fifty-plus seeded permutations of a commutative
/// run (scores distinct plus one tie pair) render byte-identical bytes and
/// digests; repeated renders of the identical input are also byte-identical;
/// and the run is emitted score-descending with payload tie-break.
#[test]
fn rendering_is_byte_identical_across_seeded_permutations_of_commutative_run() {
    let tokenizer = tokenizer_identity();
    let adapter = ByteTokenizer {
        identity: tokenizer.clone(),
    };
    let commutative = |payload: &[u8], score: u32| {
        DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, payload))
            .unwrap()
            .with_survival_score_bps(score)
            .unwrap()
    };
    let barrier = |payload: &[u8]| {
        DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, payload)).unwrap()
    };

    // Run of six commutative entries: scores (3000, 9000, 5000, 5000,
    // 7000, 1000) -- one tie pair exercises the payload tie-break.
    let base_run = vec![
        commutative(b"comm-0", 3_000),
        commutative(b"comm-1", 9_000),
        commutative(b"comm-2", 5_000),
        commutative(b"comm-3", 5_000),
        commutative(b"comm-4", 7_000),
        commutative(b"comm-5", 1_000),
    ];
    let nc_first = barrier(b"nc-barrier-0");
    let nc_last = barrier(b"nc-barrier-1");
    let build = |run: Vec<DecisionViewSection>| {
        let mut sections = Vec::with_capacity(run.len() + 2);
        sections.push(nc_first.clone());
        sections.extend(run);
        sections.push(nc_last.clone());
        DecisionView::render(&adapter, identity(&tokenizer), sections).unwrap()
    };

    let reference = build(base_run.clone());
    // Repeated renders of the identical input order are byte-identical.
    let again = build(base_run.clone());
    assert_eq!(reference.rendered(), again.rendered());
    assert_eq!(reference.digest(), again.digest());

    // Fifty-plus seeded permutations of the commutative run only: the
    // noncommutative barriers stay pinned, so the run stays one maximal
    // commutative run and the output must be permutation-invariant.
    let mut rng = Lcg(0x5eed_0001);
    for _ in 0..64 {
        let mut run = base_run.clone();
        shuffle(&mut rng, &mut run);
        let view = build(run);
        assert_eq!(view.rendered(), reference.rendered());
        assert_eq!(view.digest(), reference.digest());
        assert_eq!(view.section_kinds(), reference.section_kinds());
    }

    // The canonical order is score-descending with payload tie-break:
    // 9000, 7000, 5000(comm-2), 5000(comm-3), 3000, 1000.
    let rendered = reference.rendered();
    let order = [
        b"comm-1".as_slice(),
        b"comm-4".as_slice(),
        b"comm-2".as_slice(),
        b"comm-3".as_slice(),
        b"comm-0".as_slice(),
        b"comm-5".as_slice(),
    ];
    let mut previous = 0;
    for payload in order {
        let position = position_of(rendered, payload).expect("commutative payload rendered");
        assert!(
            position > previous,
            "{payload:?} not after the previous entry"
        );
        previous = position;
    }
    // Noncommutative barriers keep their caller positions: first and last.
    assert!(position_of(rendered, b"nc-barrier-0") < position_of(rendered, b"comm-1"));
    assert!(position_of(rendered, b"comm-5") < position_of(rendered, b"nc-barrier-1"));
}

/// VIEW-007: exhaustive small-permutation acceptance. All 24 permutations
/// of four commutative entries render identically post-ordering, and
/// noncommutative content keeps caller order verbatim.
#[test]
fn all_permutations_of_four_commutative_entries_render_identically() {
    let tokenizer = tokenizer_identity();
    let adapter = ByteTokenizer {
        identity: tokenizer.clone(),
    };
    let commutative = |payload: &[u8], score: u32| {
        DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, payload))
            .unwrap()
            .with_survival_score_bps(score)
            .unwrap()
    };
    let semantic = |payload: &[u8]| {
        DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, payload)).unwrap()
    };
    let base_run = vec![
        commutative(b"comm-a", 2_500),
        commutative(b"comm-b", 4_500),
        commutative(b"comm-c", 6_500),
        commutative(b"comm-d", 8_500),
    ];
    let nc_first = semantic(b"semantic-first");
    let nc_second = semantic(b"semantic-second");

    let all = permutations(base_run);
    assert_eq!(all.len(), 24, "four entries have exactly 24 permutations");
    let reference = {
        let mut sections = vec![nc_first.clone()];
        sections.extend(all[0].clone());
        sections.push(nc_second.clone());
        DecisionView::render(&adapter, identity(&tokenizer), sections).unwrap()
    };
    for run in all {
        let mut sections = vec![nc_first.clone()];
        sections.extend(run);
        sections.push(nc_second.clone());
        let view = DecisionView::render(&adapter, identity(&tokenizer), sections).unwrap();
        assert_eq!(view.rendered(), reference.rendered());
        assert_eq!(view.digest(), reference.digest());
    }

    // Score-descending canonical order: comm-d, comm-c, comm-b, comm-a,
    // followed by the semantic-order content in caller order.
    let rendered = reference.rendered();
    let comm_order = [
        b"comm-d".as_slice(),
        b"comm-c".as_slice(),
        b"comm-b".as_slice(),
        b"comm-a".as_slice(),
    ];
    let mut previous = 0;
    for payload in comm_order {
        let position = position_of(rendered, payload).expect("commutative payload rendered");
        assert!(position > previous);
        previous = position;
    }
    let semantic_first = position_of(rendered, b"semantic-first").unwrap();
    let semantic_second = position_of(rendered, b"semantic-second").unwrap();
    assert!(
        semantic_first > previous,
        "semantic content follows the run"
    );
    assert!(
        semantic_first < semantic_second,
        "noncommutative content keeps caller order"
    );
}

/// VIEW-007: the survival score participates in the view digest even when
/// it does not change the rendered order, is validated as basis points, and
/// does not weaken the stable-first invariant.
#[test]
fn survival_score_participates_in_digest_and_is_validated() {
    let tokenizer = tokenizer_identity();
    let adapter = ByteTokenizer {
        identity: tokenizer.clone(),
    };
    let commutative = |payload: &[u8], score: u32| {
        DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, payload))
            .unwrap()
            .with_survival_score_bps(score)
            .unwrap()
    };

    // Same caller order and same relative ranking, different magnitudes:
    // rendered bytes identical, digest must differ (score is digest-covered).
    let sections_high = vec![commutative(b"alpha", 9_000), commutative(b"beta", 8_000)];
    let sections_low = vec![commutative(b"alpha", 7_000), commutative(b"beta", 6_000)];
    let view_high = DecisionView::render(&adapter, identity(&tokenizer), sections_high).unwrap();
    let view_low = DecisionView::render(&adapter, identity(&tokenizer), sections_low).unwrap();
    assert_eq!(view_high.rendered(), view_low.rendered());
    assert_ne!(view_high.digest(), view_low.digest());

    // Accessor and basis-point validation.
    assert_eq!(
        commutative(b"alpha", 9_000).survival_score_bps(),
        Some(9_000)
    );
    let plain = DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, b"plain")).unwrap();
    assert_eq!(plain.survival_score_bps(), None);
    assert!(matches!(
        plain.clone().with_survival_score_bps(10_001),
        Err(DecisionViewError::SurvivalScoreOutOfRange {
            actual: 10_001,
            limit: 10_000
        })
    ));
    assert!(plain.with_survival_score_bps(10_000).is_ok());

    // Score-descending applies within the stable block too, and the
    // stable-first invariant survives scores: stable commutative content
    // after volatile content is still rejected.
    let stable_low =
        DecisionViewSection::stable_system_tool_contract(&byte_map(&tokenizer, b"stable-low"))
            .unwrap()
            .with_survival_score_bps(3_000)
            .unwrap();
    let stable_high =
        DecisionViewSection::stable_system_tool_contract(&byte_map(&tokenizer, b"stable-high"))
            .unwrap()
            .with_survival_score_bps(8_000)
            .unwrap();
    let tail = DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, b"tail")).unwrap();
    let view = DecisionView::render(
        &adapter,
        identity(&tokenizer),
        vec![stable_low.clone(), stable_high, tail],
    )
    .unwrap();
    let rendered = view.rendered();
    assert!(position_of(rendered, b"stable-high") < position_of(rendered, b"stable-low"));
    assert!(position_of(rendered, b"stable-low") < position_of(rendered, b"tail"));
    assert!(view.rendered().starts_with(view.stable_prefix().bytes()));

    let volatile_comm = commutative(b"volatile", 9_000);
    let late_stable = stable_low;
    assert_eq!(
        DecisionView::render(
            &adapter,
            identity(&tokenizer),
            vec![volatile_comm, late_stable]
        ),
        Err(DecisionViewError::StableSectionAfterVolatile { index: 1 })
    );
}
