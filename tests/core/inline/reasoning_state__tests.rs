use super::*;
use crate::decision_view::{DecisionViewIdentity, DecisionViewSection};
use crate::model_artifacts::{ExactTokenMap, ExactTokenizerAdapter, ExactTokenizerIdentity};
use std::collections::BTreeMap;
use zero_abi::NativeStatePolicy;
use zero_gauge::ProviderLock;

fn d(byte: u8) -> Sha256Digest {
    Sha256Digest::from_bytes([byte; 32])
}

fn contract(tokenizer_identity: Sha256Digest, tool_schema: Sha256Digest) -> ReasoningContract {
    contract_with_policy(
        tokenizer_identity,
        tool_schema,
        NativeStatePolicy::ExactRequired,
    )
}

fn contract_with_policy(
    tokenizer_identity: Sha256Digest,
    tool_schema: Sha256Digest,
    native_state_policy: NativeStatePolicy,
) -> ReasoningContract {
    ReasoningContract::new(
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

fn state_binding(contract: &ReasoningContract) -> ReasoningStateBinding {
    ReasoningStateBinding::new(d(7), contract, d(8), d(9), true, Some(d(10))).unwrap()
}

#[test]
fn exact_opaque_state_round_trips_without_logging_or_serializing_bytes() {
    let contract = contract(d(3), d(5));
    let binding = state_binding(&contract);
    let order = ReasoningStateOrder::new(0, None).unwrap();
    let secret = b"provider-native-secret-state".to_vec();
    let envelope = OpaqueReasoningStateEnvelope::capture(
        OpaqueReasoningStateKind::EncryptedReasoningContent,
        ReasoningContinuationStatus::Exact,
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
    let initial = ReasoningStateOrder::new(0, None).unwrap();
    let envelope = OpaqueReasoningStateEnvelope::capture(
        OpaqueReasoningStateKind::ProviderReasoningItems,
        ReasoningContinuationStatus::Exact,
        binding.clone(),
        initial,
        None,
        Some(100),
        vec![1, 2, 3],
    )
    .unwrap();
    let other_binding =
        ReasoningStateBinding::new(d(7), &contract, d(8), d(11), true, Some(d(10))).unwrap();
    let next = ReasoningStateOrder::new(1, Some(envelope.reference().content_digest())).unwrap();

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
        contract_with_policy(d(3), d(5), NativeStatePolicy::ExactIfAvailable);
    let approximate_binding = state_binding(&approximate_contract);
    let approximate = OpaqueReasoningStateEnvelope::capture(
        OpaqueReasoningStateKind::ProviderContinuationId,
        ReasoningContinuationStatus::Approximate,
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
            ReasoningContinuationStatus::Approximate
        ))
    ));
}

#[test]
fn native_state_policy_cannot_be_upgraded_to_exact_replay() {
    let order = ReasoningStateOrder::new(0, None).unwrap();
    for policy in [
        NativeStatePolicy::CleanRestart,
        NativeStatePolicy::Unavailable,
    ] {
        let contract = contract_with_policy(d(3), d(5), policy);
        let binding = state_binding(&contract);
        assert!(matches!(
            OpaqueReasoningStateEnvelope::capture(
                OpaqueReasoningStateKind::ProviderContinuationId,
                ReasoningContinuationStatus::Exact,
                binding,
                order,
                None,
                None,
                b"opaque-id".to_vec(),
            ),
            Err(ReasoningStateError::NativeStatePolicyMismatch {
                policy: rejected,
                status: ReasoningContinuationStatus::Exact,
            }) if rejected == policy
        ));
    }

    let scoped_contract = contract_with_policy(d(3), d(5), NativeStatePolicy::ScopedCertificate);
    let scoped_binding = state_binding(&scoped_contract);
    let scoped = OpaqueReasoningStateEnvelope::capture(
        OpaqueReasoningStateKind::SignedThinkingBlocks,
        ReasoningContinuationStatus::ScopedCertificate,
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
            ReasoningContinuationStatus::ScopedCertificate
        ))
    ));
}

#[test]
fn unavailable_and_scoped_states_cannot_be_upgraded_by_shape() {
    let contract = contract(d(3), d(5));
    let binding = state_binding(&contract);
    let unavailable = OpaqueReasoningStateRef::unavailable(binding.clone());
    assert_eq!(
        unavailable.status(),
        ReasoningContinuationStatus::Unavailable
    );
    assert_eq!(unavailable.content_digest(), Sha256Digest::ZERO);
    assert_eq!(unavailable.byte_len(), 0);

    assert!(matches!(
        OpaqueReasoningStateEnvelope::capture(
            OpaqueReasoningStateKind::Unavailable,
            ReasoningContinuationStatus::Exact,
            binding.clone(),
            ReasoningStateOrder::new(0, None).unwrap(),
            None,
            None,
            vec![1]
        ),
        Err(ReasoningStateError::UnavailableKindHasPayload)
    ));
    assert!(matches!(
        OpaqueReasoningStateEnvelope::capture(
            OpaqueReasoningStateKind::SignedThinkingBlocks,
            ReasoningContinuationStatus::ScopedCertificate,
            binding,
            ReasoningStateOrder::new(0, None).unwrap(),
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

fn decision_view(tool_schema: Sha256Digest) -> (DecisionView, ReasoningContract) {
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
    let mut recovery = RawDecisionViewRecoveryEnvelope::capture(&view, d(40), d(41)).unwrap();

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
        Err(ModelStateContinuationError::RawBaselineIdentityMismatch)
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
        Err(ModelStateContinuationError::RawBytesDigestMismatch)
    ));
}

#[test]
fn continuation_classes_preserve_exact_scoped_empirical_and_unavailable() {
    let (view, exact_contract) = decision_view(d(5));
    let tokenizer = view.identity().tokenizer_identity_digest();
    let recovery = RawDecisionViewRecoveryEnvelope::capture(&view, d(40), d(41)).unwrap();
    let order = ReasoningStateOrder::new(0, None).unwrap();
    let exact_binding = state_binding(&exact_contract);
    let exact = OpaqueReasoningStateEnvelope::capture(
        OpaqueReasoningStateKind::ProviderReasoningItems,
        ReasoningContinuationStatus::Exact,
        exact_binding,
        order,
        None,
        None,
        b"exact-native-state".to_vec(),
    )
    .unwrap();
    let exact_assessment = ModelStateContinuationAssessment::assess(
        exact.reference(),
        ModelStateContinuationEvidence::None,
        recovery.reference(),
        10,
    )
    .unwrap();
    assert_eq!(
        exact_assessment.class(),
        ModelStateContinuationKind::ExactNeutral
    );

    let scoped_contract =
        contract_with_policy(tokenizer, d(5), NativeStatePolicy::ScopedCertificate);
    let scoped = OpaqueReasoningStateEnvelope::capture(
        OpaqueReasoningStateKind::SignedThinkingBlocks,
        ReasoningContinuationStatus::ScopedCertificate,
        state_binding(&scoped_contract),
        order,
        Some(d(50)),
        None,
        b"scoped-state".to_vec(),
    )
    .unwrap();
    let scoped_assessment = ModelStateContinuationAssessment::assess(
        scoped.reference(),
        ModelStateContinuationEvidence::scoped(d(50), d(51)).unwrap(),
        recovery.reference(),
        10,
    )
    .unwrap();
    assert_eq!(
        scoped_assessment.class(),
        ModelStateContinuationKind::ScopedCertificate
    );
    assert_eq!(scoped_assessment.scoped_evidence(), Some((d(50), d(51))));
    assert!(matches!(
        scoped.exact_replay_bytes(scoped.reference().binding(), order, 10),
        Err(ReasoningStateError::NotExact(
            ReasoningContinuationStatus::ScopedCertificate
        ))
    ));

    let approximate_contract =
        contract_with_policy(tokenizer, d(5), NativeStatePolicy::ExactIfAvailable);
    let approximate = OpaqueReasoningStateEnvelope::capture(
        OpaqueReasoningStateKind::ProviderContinuationId,
        ReasoningContinuationStatus::Approximate,
        state_binding(&approximate_contract),
        order,
        None,
        None,
        b"approximate-state".to_vec(),
    )
    .unwrap();
    let empirical = ModelStateContinuationAssessment::assess(
        approximate.reference(),
        ModelStateContinuationEvidence::empirical(d(60), d(61), d(62), Some(100)).unwrap(),
        recovery.reference(),
        10,
    )
    .unwrap();
    assert_eq!(empirical.class(), ModelStateContinuationKind::Empirical);
    assert_eq!(
        empirical.empirical_evidence(),
        Some((d(60), d(61), d(62), Some(100)))
    );
    assert!(matches!(
        approximate.exact_replay_bytes(approximate.reference().binding(), order, 10),
        Err(ReasoningStateError::NotExact(
            ReasoningContinuationStatus::Approximate
        ))
    ));
    let no_empirical_evidence = ModelStateContinuationAssessment::assess(
        approximate.reference(),
        ModelStateContinuationEvidence::None,
        recovery.reference(),
        10,
    )
    .unwrap();
    assert_eq!(
        no_empirical_evidence.class(),
        ModelStateContinuationKind::Unavailable
    );
    assert_eq!(
        no_empirical_evidence.unavailable_reason(),
        Some(ModelStateUnavailableReason::EmpiricalEvidenceAbsent)
    );

    let unavailable = OpaqueReasoningStateRef::unavailable(state_binding(&exact_contract));
    let unavailable_assessment = ModelStateContinuationAssessment::assess(
        &unavailable,
        ModelStateContinuationEvidence::None,
        recovery.reference(),
        10,
    )
    .unwrap();
    assert_eq!(
        unavailable_assessment.class(),
        ModelStateContinuationKind::Unavailable
    );
    assert_eq!(
        unavailable_assessment.unavailable_reason(),
        Some(ModelStateUnavailableReason::ProviderUnavailable)
    );
}

#[test]
fn continuation_evidence_mismatch_and_expiry_fail_closed() {
    let (view, _) = decision_view(d(5));
    let tokenizer = view.identity().tokenizer_identity_digest();
    let recovery = RawDecisionViewRecoveryEnvelope::capture(&view, d(40), d(41)).unwrap();
    let order = ReasoningStateOrder::new(0, None).unwrap();
    let scoped_contract =
        contract_with_policy(tokenizer, d(5), NativeStatePolicy::ScopedCertificate);
    let scoped = OpaqueReasoningStateEnvelope::capture(
        OpaqueReasoningStateKind::SignedThinkingBlocks,
        ReasoningContinuationStatus::ScopedCertificate,
        state_binding(&scoped_contract),
        order,
        Some(d(50)),
        Some(100),
        b"scoped-state".to_vec(),
    )
    .unwrap();
    assert!(matches!(
        ModelStateContinuationAssessment::assess(
            scoped.reference(),
            ModelStateContinuationEvidence::scoped(d(52), d(51)).unwrap(),
            recovery.reference(),
            10,
        ),
        Err(ModelStateContinuationError::ScopedCertificateMismatch)
    ));
    for (evidence, field) in [
        (
            ModelStateContinuationEvidence::Scoped {
                certificate_digest: Sha256Digest::ZERO,
                declared_scope_digest: d(51),
            },
            "certificate",
        ),
        (
            ModelStateContinuationEvidence::Scoped {
                certificate_digest: d(50),
                declared_scope_digest: Sha256Digest::ZERO,
            },
            "declared scope",
        ),
    ] {
        assert_eq!(
            ModelStateContinuationAssessment::assess(
                scoped.reference(),
                evidence,
                recovery.reference(),
                10,
            )
            .unwrap_err(),
            ModelStateContinuationError::ZeroIdentity(field)
        );
    }

    let approximate_contract =
        contract_with_policy(tokenizer, d(5), NativeStatePolicy::ExactIfAvailable);
    let approximate = OpaqueReasoningStateEnvelope::capture(
        OpaqueReasoningStateKind::ProviderContinuationId,
        ReasoningContinuationStatus::Approximate,
        state_binding(&approximate_contract),
        order,
        None,
        None,
        b"approximate-state".to_vec(),
    )
    .unwrap();
    for (evidence, expected_error) in [
        (
            ModelStateContinuationEvidence::Empirical {
                frozen_distribution_digest: Sha256Digest::ZERO,
                evaluation_receipt_digest: d(61),
                declared_scope_digest: d(62),
                valid_until_unix_ms: None,
            },
            ModelStateContinuationError::ZeroIdentity("frozen distribution"),
        ),
        (
            ModelStateContinuationEvidence::Empirical {
                frozen_distribution_digest: d(60),
                evaluation_receipt_digest: Sha256Digest::ZERO,
                declared_scope_digest: d(62),
                valid_until_unix_ms: None,
            },
            ModelStateContinuationError::ZeroIdentity("evaluation receipt"),
        ),
        (
            ModelStateContinuationEvidence::Empirical {
                frozen_distribution_digest: d(60),
                evaluation_receipt_digest: d(61),
                declared_scope_digest: Sha256Digest::ZERO,
                valid_until_unix_ms: None,
            },
            ModelStateContinuationError::ZeroIdentity("declared scope"),
        ),
        (
            ModelStateContinuationEvidence::Empirical {
                frozen_distribution_digest: d(60),
                evaluation_receipt_digest: d(61),
                declared_scope_digest: d(62),
                valid_until_unix_ms: Some(0),
            },
            ModelStateContinuationError::InvalidEvidenceExpiry,
        ),
    ] {
        assert_eq!(
            ModelStateContinuationAssessment::assess(
                approximate.reference(),
                evidence,
                recovery.reference(),
                10,
            )
            .unwrap_err(),
            expected_error
        );
    }

    let expired = ModelStateContinuationAssessment::assess(
        scoped.reference(),
        ModelStateContinuationEvidence::scoped(d(50), d(51)).unwrap(),
        recovery.reference(),
        100,
    )
    .unwrap();
    assert_eq!(expired.class(), ModelStateContinuationKind::Unavailable);
    assert_eq!(
        expired.unavailable_reason(),
        Some(ModelStateUnavailableReason::StateExpired)
    );
}

#[test]
fn continuation_assessment_refuses_tokenizer_and_tool_schema_drift() {
    let (view, matching) = decision_view(d(5));
    let recovery = RawDecisionViewRecoveryEnvelope::capture(&view, d(40), d(41)).unwrap();
    let order = ReasoningStateOrder::new(0, None).unwrap();
    let tokenizer = view.identity().tokenizer_identity_digest();
    let drifted_tokenizer = OpaqueReasoningStateEnvelope::capture(
        OpaqueReasoningStateKind::ProviderReasoningItems,
        ReasoningContinuationStatus::Exact,
        state_binding(&contract(d(3), d(5))),
        order,
        None,
        None,
        b"exact-native-state".to_vec(),
    )
    .unwrap();
    assert_eq!(
        ModelStateContinuationAssessment::assess(
            drifted_tokenizer.reference(),
            ModelStateContinuationEvidence::None,
            recovery.reference(),
            10,
        )
        .unwrap_err(),
        ModelStateContinuationError::TokenizerIdentityMismatch
    );
    let drifted_tools = OpaqueReasoningStateEnvelope::capture(
        OpaqueReasoningStateKind::ProviderReasoningItems,
        ReasoningContinuationStatus::Exact,
        state_binding(&contract(tokenizer, d(99))),
        order,
        None,
        None,
        b"exact-native-state".to_vec(),
    )
    .unwrap();
    assert_eq!(
        ModelStateContinuationAssessment::assess(
            drifted_tools.reference(),
            ModelStateContinuationEvidence::None,
            recovery.reference(),
            10,
        )
        .unwrap_err(),
        ModelStateContinuationError::ToolSchemaIdentityMismatch
    );
    let matched = OpaqueReasoningStateEnvelope::capture(
        OpaqueReasoningStateKind::ProviderReasoningItems,
        ReasoningContinuationStatus::Exact,
        state_binding(&matching),
        order,
        None,
        None,
        b"exact-native-state".to_vec(),
    )
    .unwrap();
    assert_eq!(
        ModelStateContinuationAssessment::assess(
            matched.reference(),
            ModelStateContinuationEvidence::None,
            recovery.reference(),
            10,
        )
        .unwrap()
        .class(),
        ModelStateContinuationKind::ExactNeutral
    );
}

#[test]
fn headroom_plan_uses_exact_view_tokens_and_canonical_hub_arithmetic() {
    let (view, contract) = decision_view(d(5));
    let plan = DecisionViewHeadroomPlan::plan(&contract, 1_000, 10, &view).unwrap();

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
        DecisionViewHeadroomPlan::plan(&wrong_tool, 1_000, 10, &view),
        Err(ReasoningStateError::ToolSchemaIdentityMismatch)
    ));
    assert!(matches!(
        DecisionViewHeadroomPlan::plan(&base_contract, 100, 10, &view),
        Err(ReasoningStateError::ReasoningContract(_))
    ));
    let just_too_small = 100 + 50 + 25 + 10 + view.total_tokens() as u32 - 1;
    assert!(matches!(
        DecisionViewHeadroomPlan::plan(&base_contract, just_too_small, 10, &view),
        Err(ReasoningStateError::ReasoningContract(_))
    ));
}
