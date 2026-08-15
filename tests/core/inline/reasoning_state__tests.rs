use super::*;
use crate::decision_view::{DecisionViewIdentity, DecisionViewSection};
use crate::model_artifacts::{ExactTokenMap, ExactTokenizerAdapter, ExactTokenizerIdentity};
use std::collections::BTreeMap;
use zero_abi::NativeStatePolicyV1;
use zero_gauge::ProviderLock;

fn d(byte: u8) -> DigestV1 {
    DigestV1::from_bytes([byte; 32])
}

fn contract(tokenizer_identity: DigestV1, tool_schema: DigestV1) -> ReasoningContractV1 {
    contract_with_policy(
        tokenizer_identity,
        tool_schema,
        NativeStatePolicyV1::ExactRequired,
    )
}

fn contract_with_policy(
    tokenizer_identity: DigestV1,
    tool_schema: DigestV1,
    native_state_policy: NativeStatePolicyV1,
) -> ReasoningContractV1 {
    ReasoningContractV1::new(
        d(1),
        d(2),
        tokenizer_identity,
        d(4),
        tool_schema,
        "enabled",
        "high",
        128,
        100,
        50,
        25,
        native_state_policy,
        false,
        BTreeMap::new(),
    )
    .unwrap()
}

fn state_binding(contract: &ReasoningContractV1) -> ReasoningStateBindingV1 {
    ReasoningStateBindingV1::new(d(7), contract, d(8), d(9), true, Some(d(10))).unwrap()
}

#[test]
fn exact_opaque_state_round_trips_without_logging_or_serializing_bytes() {
    let contract = contract(d(3), d(5));
    let binding = state_binding(&contract);
    let order = ReasoningStateOrderV1::new(0, None).unwrap();
    let secret = b"provider-native-secret-state".to_vec();
    let envelope = OpaqueReasoningStateEnvelopeV1::capture(
        OpaqueReasoningStateKindV1::EncryptedReasoningContent,
        ReasoningContinuationStatusV1::Exact,
        binding.clone(),
        order,
        None,
        Some(2_000),
        secret.clone(),
    )
    .unwrap();

    assert_eq!(envelope.opaque_bytes(), secret);
    assert_eq!(
        envelope.exact_replay_bytes(&binding, order, 1_999).unwrap(),
        secret
    );
    assert_eq!(envelope.reference().content_digest(), digest(&secret));
    assert!(!format!("{envelope:?}").contains("provider-native-secret"));
    assert!(
        !serde_json::to_string(envelope.reference())
            .unwrap()
            .contains("provider-native-secret")
    );
}

#[test]
fn exact_replay_refuses_identity_order_status_and_expiry_drift() {
    let contract = contract(d(3), d(5));
    let binding = state_binding(&contract);
    let initial = ReasoningStateOrderV1::new(0, None).unwrap();
    let envelope = OpaqueReasoningStateEnvelopeV1::capture(
        OpaqueReasoningStateKindV1::ProviderReasoningItems,
        ReasoningContinuationStatusV1::Exact,
        binding.clone(),
        initial,
        None,
        Some(100),
        vec![1, 2, 3],
    )
    .unwrap();
    let other_binding =
        ReasoningStateBindingV1::new(d(7), &contract, d(8), d(11), true, Some(d(10))).unwrap();
    let next = ReasoningStateOrderV1::new(1, Some(envelope.reference().content_digest())).unwrap();

    assert!(matches!(
        envelope.exact_replay_bytes(&other_binding, initial, 99),
        Err(ReasoningStateError::BindingMismatch)
    ));
    assert!(matches!(
        envelope.exact_replay_bytes(&binding, next, 99),
        Err(ReasoningStateError::OrderMismatch)
    ));
    assert!(matches!(
        envelope.exact_replay_bytes(&binding, initial, 100),
        Err(ReasoningStateError::Expired)
    ));

    let approximate_contract =
        contract_with_policy(d(3), d(5), NativeStatePolicyV1::ExactIfAvailable);
    let approximate_binding = state_binding(&approximate_contract);
    let approximate = OpaqueReasoningStateEnvelopeV1::capture(
        OpaqueReasoningStateKindV1::ProviderContinuationId,
        ReasoningContinuationStatusV1::Approximate,
        approximate_binding.clone(),
        initial,
        None,
        None,
        b"opaque-id".to_vec(),
    )
    .unwrap();
    assert!(matches!(
        approximate.exact_replay_bytes(&approximate_binding, initial, 0),
        Err(ReasoningStateError::NotExact(
            ReasoningContinuationStatusV1::Approximate
        ))
    ));
}

#[test]
fn native_state_policy_cannot_be_upgraded_to_exact_replay() {
    let order = ReasoningStateOrderV1::new(0, None).unwrap();
    for policy in [
        NativeStatePolicyV1::CleanRestart,
        NativeStatePolicyV1::Unavailable,
    ] {
        let contract = contract_with_policy(d(3), d(5), policy);
        let binding = state_binding(&contract);
        assert!(matches!(
            OpaqueReasoningStateEnvelopeV1::capture(
                OpaqueReasoningStateKindV1::ProviderContinuationId,
                ReasoningContinuationStatusV1::Exact,
                binding,
                order,
                None,
                None,
                b"opaque-id".to_vec(),
            ),
            Err(ReasoningStateError::NativeStatePolicyMismatch {
                policy: rejected,
                status: ReasoningContinuationStatusV1::Exact,
            }) if rejected == policy
        ));
    }

    let scoped_contract = contract_with_policy(d(3), d(5), NativeStatePolicyV1::ScopedCertificate);
    let scoped_binding = state_binding(&scoped_contract);
    let scoped = OpaqueReasoningStateEnvelopeV1::capture(
        OpaqueReasoningStateKindV1::SignedThinkingBlocks,
        ReasoningContinuationStatusV1::ScopedCertificate,
        scoped_binding.clone(),
        order,
        Some(d(30)),
        None,
        b"signed-block".to_vec(),
    )
    .unwrap();
    assert!(matches!(
        scoped.exact_replay_bytes(&scoped_binding, order, 0),
        Err(ReasoningStateError::NotExact(
            ReasoningContinuationStatusV1::ScopedCertificate
        ))
    ));
}

#[test]
fn unavailable_and_scoped_states_cannot_be_upgraded_by_shape() {
    let contract = contract(d(3), d(5));
    let binding = state_binding(&contract);
    let unavailable = OpaqueReasoningStateRefV1::unavailable(binding.clone());
    assert_eq!(
        unavailable.status(),
        ReasoningContinuationStatusV1::Unavailable
    );
    assert_eq!(unavailable.content_digest(), DigestV1::ZERO);
    assert_eq!(unavailable.byte_len(), 0);

    assert!(matches!(
        OpaqueReasoningStateEnvelopeV1::capture(
            OpaqueReasoningStateKindV1::Unavailable,
            ReasoningContinuationStatusV1::Exact,
            binding.clone(),
            ReasoningStateOrderV1::new(0, None).unwrap(),
            None,
            None,
            vec![1]
        ),
        Err(ReasoningStateError::UnavailableKindHasPayload)
    ));
    assert!(matches!(
        OpaqueReasoningStateEnvelopeV1::capture(
            OpaqueReasoningStateKindV1::SignedThinkingBlocks,
            ReasoningContinuationStatusV1::ScopedCertificate,
            binding,
            ReasoningStateOrderV1::new(0, None).unwrap(),
            None,
            None,
            vec![1]
        ),
        Err(ReasoningStateError::ScopedCertificateRequired)
    ));
}

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

fn decision_view(tool_schema: DigestV1) -> (DecisionView, ReasoningContractV1) {
    let manifest = b"byte-tokenizer-v1";
    let identity = ExactTokenizerIdentity::new(
        ProviderLock {
            provider: "test-provider".to_string(),
            model: "test-model".to_string(),
            tokenizer_revision_digest: digest(manifest).to_hex(),
        },
        manifest,
    )
    .unwrap();
    let adapter = ByteAdapter { identity };
    let stable = ExactTokenMap::tokenize(&adapter, b"stable").unwrap();
    let task = ExactTokenMap::tokenize(&adapter, b"task").unwrap();
    let view = DecisionView::render(
        &adapter,
        DecisionViewIdentity::new(d(20), d(21), adapter.identity(), tool_schema),
        vec![
            DecisionViewSection::stable_system_tool_contract(&stable).unwrap(),
            DecisionViewSection::volatile_user_task(&task).unwrap(),
        ],
    )
    .unwrap();
    let contract = contract(adapter.identity().digest(), tool_schema);
    (view, contract)
}

#[test]
fn raw_decision_view_recovery_is_exact_bound_and_redacted() {
    let (view, _) = decision_view(d(5));
    let mut recovery = RawDecisionViewRecoveryEnvelopeV1::capture(&view, d(40), d(41)).unwrap();

    assert_eq!(
        recovery
            .exact_raw_decision_view_bytes(
                view.identity(),
                view.digest(),
                view.exact_token_map_digest(),
                d(40),
                d(41),
            )
            .unwrap(),
        view.rendered()
    );
    assert_eq!(
        recovery.reference().raw_bytes_digest(),
        digest(view.rendered())
    );
    assert!(!format!("{recovery:?}").contains("TOKENZERO-DECISION-VIEW"));
    assert!(
        !serde_json::to_string(recovery.reference())
            .unwrap()
            .contains("TOKENZERO-DECISION-VIEW")
    );
    assert!(matches!(
        recovery.exact_raw_decision_view_bytes(
            view.identity(),
            view.digest(),
            view.exact_token_map_digest(),
            d(42),
            d(41),
        ),
        Err(ModelStateContinuationErrorV1::RawBaselineIdentityMismatch)
    ));

    recovery.raw_decision_view_bytes[0] ^= 1;
    assert!(matches!(
        recovery.exact_raw_decision_view_bytes(
            view.identity(),
            view.digest(),
            view.exact_token_map_digest(),
            d(40),
            d(41),
        ),
        Err(ModelStateContinuationErrorV1::RawBytesDigestMismatch)
    ));
}

#[test]
fn continuation_classes_preserve_exact_scoped_empirical_and_unavailable() {
    let (view, exact_contract) = decision_view(d(5));
    let tokenizer = view.identity().tokenizer_identity_digest();
    let recovery = RawDecisionViewRecoveryEnvelopeV1::capture(&view, d(40), d(41)).unwrap();
    let order = ReasoningStateOrderV1::new(0, None).unwrap();
    let exact_binding = state_binding(&exact_contract);
    let exact = OpaqueReasoningStateEnvelopeV1::capture(
        OpaqueReasoningStateKindV1::ProviderReasoningItems,
        ReasoningContinuationStatusV1::Exact,
        exact_binding,
        order,
        None,
        None,
        b"exact-native-state".to_vec(),
    )
    .unwrap();
    let exact_assessment = ModelStateContinuationAssessmentV1::assess(
        exact.reference(),
        ModelStateContinuationEvidenceV1::None,
        recovery.reference(),
        10,
    )
    .unwrap();
    assert_eq!(
        exact_assessment.class(),
        ModelStateContinuationKindV1::ExactNeutral
    );

    let scoped_contract =
        contract_with_policy(tokenizer, d(5), NativeStatePolicyV1::ScopedCertificate);
    let scoped = OpaqueReasoningStateEnvelopeV1::capture(
        OpaqueReasoningStateKindV1::SignedThinkingBlocks,
        ReasoningContinuationStatusV1::ScopedCertificate,
        state_binding(&scoped_contract),
        order,
        Some(d(50)),
        None,
        b"scoped-state".to_vec(),
    )
    .unwrap();
    let scoped_assessment = ModelStateContinuationAssessmentV1::assess(
        scoped.reference(),
        ModelStateContinuationEvidenceV1::scoped(d(50), d(51)).unwrap(),
        recovery.reference(),
        10,
    )
    .unwrap();
    assert_eq!(
        scoped_assessment.class(),
        ModelStateContinuationKindV1::ScopedCertificate
    );
    assert_eq!(scoped_assessment.scoped_evidence(), Some((d(50), d(51))));
    assert!(matches!(
        scoped.exact_replay_bytes(scoped.reference().binding(), order, 10),
        Err(ReasoningStateError::NotExact(
            ReasoningContinuationStatusV1::ScopedCertificate
        ))
    ));

    let approximate_contract =
        contract_with_policy(tokenizer, d(5), NativeStatePolicyV1::ExactIfAvailable);
    let approximate = OpaqueReasoningStateEnvelopeV1::capture(
        OpaqueReasoningStateKindV1::ProviderContinuationId,
        ReasoningContinuationStatusV1::Approximate,
        state_binding(&approximate_contract),
        order,
        None,
        None,
        b"approximate-state".to_vec(),
    )
    .unwrap();
    let empirical = ModelStateContinuationAssessmentV1::assess(
        approximate.reference(),
        ModelStateContinuationEvidenceV1::empirical(d(60), d(61), d(62), Some(100)).unwrap(),
        recovery.reference(),
        10,
    )
    .unwrap();
    assert_eq!(empirical.class(), ModelStateContinuationKindV1::Empirical);
    assert_eq!(
        empirical.empirical_evidence(),
        Some((d(60), d(61), d(62), Some(100)))
    );
    assert!(matches!(
        approximate.exact_replay_bytes(approximate.reference().binding(), order, 10),
        Err(ReasoningStateError::NotExact(
            ReasoningContinuationStatusV1::Approximate
        ))
    ));
    let no_empirical_evidence = ModelStateContinuationAssessmentV1::assess(
        approximate.reference(),
        ModelStateContinuationEvidenceV1::None,
        recovery.reference(),
        10,
    )
    .unwrap();
    assert_eq!(
        no_empirical_evidence.class(),
        ModelStateContinuationKindV1::Unavailable
    );
    assert_eq!(
        no_empirical_evidence.unavailable_reason(),
        Some(ModelStateUnavailableReasonV1::EmpiricalEvidenceAbsent)
    );

    let unavailable = OpaqueReasoningStateRefV1::unavailable(state_binding(&exact_contract));
    let unavailable_assessment = ModelStateContinuationAssessmentV1::assess(
        &unavailable,
        ModelStateContinuationEvidenceV1::None,
        recovery.reference(),
        10,
    )
    .unwrap();
    assert_eq!(
        unavailable_assessment.class(),
        ModelStateContinuationKindV1::Unavailable
    );
    assert_eq!(
        unavailable_assessment.unavailable_reason(),
        Some(ModelStateUnavailableReasonV1::ProviderUnavailable)
    );
}

#[test]
fn continuation_evidence_mismatch_and_expiry_fail_closed() {
    let (view, _) = decision_view(d(5));
    let tokenizer = view.identity().tokenizer_identity_digest();
    let recovery = RawDecisionViewRecoveryEnvelopeV1::capture(&view, d(40), d(41)).unwrap();
    let order = ReasoningStateOrderV1::new(0, None).unwrap();
    let scoped_contract =
        contract_with_policy(tokenizer, d(5), NativeStatePolicyV1::ScopedCertificate);
    let scoped = OpaqueReasoningStateEnvelopeV1::capture(
        OpaqueReasoningStateKindV1::SignedThinkingBlocks,
        ReasoningContinuationStatusV1::ScopedCertificate,
        state_binding(&scoped_contract),
        order,
        Some(d(50)),
        Some(100),
        b"scoped-state".to_vec(),
    )
    .unwrap();
    assert!(matches!(
        ModelStateContinuationAssessmentV1::assess(
            scoped.reference(),
            ModelStateContinuationEvidenceV1::scoped(d(52), d(51)).unwrap(),
            recovery.reference(),
            10,
        ),
        Err(ModelStateContinuationErrorV1::ScopedCertificateMismatch)
    ));
    for (evidence, field) in [
        (
            ModelStateContinuationEvidenceV1::Scoped {
                certificate_digest: DigestV1::ZERO,
                declared_scope_digest: d(51),
            },
            "certificate",
        ),
        (
            ModelStateContinuationEvidenceV1::Scoped {
                certificate_digest: d(50),
                declared_scope_digest: DigestV1::ZERO,
            },
            "declared scope",
        ),
    ] {
        assert_eq!(
            ModelStateContinuationAssessmentV1::assess(
                scoped.reference(),
                evidence,
                recovery.reference(),
                10,
            )
            .unwrap_err(),
            ModelStateContinuationErrorV1::ZeroIdentity(field)
        );
    }

    let approximate_contract =
        contract_with_policy(tokenizer, d(5), NativeStatePolicyV1::ExactIfAvailable);
    let approximate = OpaqueReasoningStateEnvelopeV1::capture(
        OpaqueReasoningStateKindV1::ProviderContinuationId,
        ReasoningContinuationStatusV1::Approximate,
        state_binding(&approximate_contract),
        order,
        None,
        None,
        b"approximate-state".to_vec(),
    )
    .unwrap();
    for (evidence, expected_error) in [
        (
            ModelStateContinuationEvidenceV1::Empirical {
                frozen_distribution_digest: DigestV1::ZERO,
                evaluation_receipt_digest: d(61),
                declared_scope_digest: d(62),
                valid_until_unix_ms: None,
            },
            ModelStateContinuationErrorV1::ZeroIdentity("frozen distribution"),
        ),
        (
            ModelStateContinuationEvidenceV1::Empirical {
                frozen_distribution_digest: d(60),
                evaluation_receipt_digest: DigestV1::ZERO,
                declared_scope_digest: d(62),
                valid_until_unix_ms: None,
            },
            ModelStateContinuationErrorV1::ZeroIdentity("evaluation receipt"),
        ),
        (
            ModelStateContinuationEvidenceV1::Empirical {
                frozen_distribution_digest: d(60),
                evaluation_receipt_digest: d(61),
                declared_scope_digest: DigestV1::ZERO,
                valid_until_unix_ms: None,
            },
            ModelStateContinuationErrorV1::ZeroIdentity("declared scope"),
        ),
        (
            ModelStateContinuationEvidenceV1::Empirical {
                frozen_distribution_digest: d(60),
                evaluation_receipt_digest: d(61),
                declared_scope_digest: d(62),
                valid_until_unix_ms: Some(0),
            },
            ModelStateContinuationErrorV1::InvalidEvidenceExpiry,
        ),
    ] {
        assert_eq!(
            ModelStateContinuationAssessmentV1::assess(
                approximate.reference(),
                evidence,
                recovery.reference(),
                10,
            )
            .unwrap_err(),
            expected_error
        );
    }

    let expired = ModelStateContinuationAssessmentV1::assess(
        scoped.reference(),
        ModelStateContinuationEvidenceV1::scoped(d(50), d(51)).unwrap(),
        recovery.reference(),
        100,
    )
    .unwrap();
    assert_eq!(expired.class(), ModelStateContinuationKindV1::Unavailable);
    assert_eq!(
        expired.unavailable_reason(),
        Some(ModelStateUnavailableReasonV1::StateExpired)
    );
}

#[test]
fn continuation_assessment_refuses_tokenizer_and_tool_schema_drift() {
    let (view, matching) = decision_view(d(5));
    let recovery = RawDecisionViewRecoveryEnvelopeV1::capture(&view, d(40), d(41)).unwrap();
    let order = ReasoningStateOrderV1::new(0, None).unwrap();
    let tokenizer = view.identity().tokenizer_identity_digest();
    let drifted_tokenizer = OpaqueReasoningStateEnvelopeV1::capture(
        OpaqueReasoningStateKindV1::ProviderReasoningItems,
        ReasoningContinuationStatusV1::Exact,
        state_binding(&contract(d(3), d(5))),
        order,
        None,
        None,
        b"exact-native-state".to_vec(),
    )
    .unwrap();
    assert_eq!(
        ModelStateContinuationAssessmentV1::assess(
            drifted_tokenizer.reference(),
            ModelStateContinuationEvidenceV1::None,
            recovery.reference(),
            10,
        )
        .unwrap_err(),
        ModelStateContinuationErrorV1::TokenizerIdentityMismatch
    );
    let drifted_tools = OpaqueReasoningStateEnvelopeV1::capture(
        OpaqueReasoningStateKindV1::ProviderReasoningItems,
        ReasoningContinuationStatusV1::Exact,
        state_binding(&contract(tokenizer, d(99))),
        order,
        None,
        None,
        b"exact-native-state".to_vec(),
    )
    .unwrap();
    assert_eq!(
        ModelStateContinuationAssessmentV1::assess(
            drifted_tools.reference(),
            ModelStateContinuationEvidenceV1::None,
            recovery.reference(),
            10,
        )
        .unwrap_err(),
        ModelStateContinuationErrorV1::ToolSchemaIdentityMismatch
    );
    let matched = OpaqueReasoningStateEnvelopeV1::capture(
        OpaqueReasoningStateKindV1::ProviderReasoningItems,
        ReasoningContinuationStatusV1::Exact,
        state_binding(&matching),
        order,
        None,
        None,
        b"exact-native-state".to_vec(),
    )
    .unwrap();
    assert_eq!(
        ModelStateContinuationAssessmentV1::assess(
            matched.reference(),
            ModelStateContinuationEvidenceV1::None,
            recovery.reference(),
            10,
        )
        .unwrap()
        .class(),
        ModelStateContinuationKindV1::ExactNeutral
    );
}

#[test]
fn headroom_plan_uses_exact_view_tokens_and_canonical_hub_arithmetic() {
    let (view, contract) = decision_view(d(5));
    let plan = DecisionViewHeadroomPlanV1::plan(&contract, 1_000, 10, &view).unwrap();

    assert_eq!(plan.logical_input_tokens(), view.total_tokens() as u32);
    assert_eq!(plan.admitted_input_ceiling(), 815);
    assert_eq!(
        plan.remaining_input_headroom(),
        815 - view.total_tokens() as u32
    );
    assert_eq!(plan.reserved_reasoning_tokens(), 100);
    assert_eq!(plan.reserved_visible_output_tokens(), 50);
    assert_eq!(plan.reserved_recovery_tokens(), 25);
    assert_eq!(plan.reserved_tool_tokens(), 10);
    assert_eq!(
        plan.reasoning_contract_digest(),
        contract.identity_digest().unwrap()
    );
}

#[test]
fn headroom_refuses_identity_drift_and_reserve_overflow() {
    let (view, base_contract) = decision_view(d(5));
    let wrong_tool = contract(view.identity().tokenizer_identity_digest(), d(99));
    assert!(matches!(
        DecisionViewHeadroomPlanV1::plan(&wrong_tool, 1_000, 10, &view),
        Err(ReasoningStateError::ToolSchemaIdentityMismatch)
    ));
    assert!(matches!(
        DecisionViewHeadroomPlanV1::plan(&base_contract, 100, 10, &view),
        Err(ReasoningStateError::ReasoningContract(_))
    ));
    let just_too_small = 100 + 50 + 25 + 10 + view.total_tokens() as u32 - 1;
    assert!(matches!(
        DecisionViewHeadroomPlanV1::plan(&base_contract, just_too_small, 10, &view),
        Err(ReasoningStateError::ReasoningContract(_))
    ));
}
