//! Opaque reasoning-state transport and protected Decision View headroom.
//!
//! Opaque provider bytes are never parsed, summarized, reordered, or serialized
//! by this module. Metadata binds them to the exact provider/model/backend,
//! reasoning contract, session, position, sampler, and lineage. Exact replay is
//! refused unless every binding matches. Headroom arithmetic delegates to the
//! canonical ZeroStack [`ReasoningContractV1`] contract.

use crate::decision_view::{DecisionView, DecisionViewIdentity};
use serde::Serialize;
use std::{error::Error, fmt};
use zero_abi::{
    DigestV1, NativeStatePolicyV1, ReasoningContractErrorV1, ReasoningContractV1, sha256,
};

pub const MAX_OPAQUE_REASONING_STATE_BYTES: usize = 16 * 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpaqueReasoningStateKindV1 {
    ProviderReasoningItems,
    SignedThinkingBlocks,
    EncryptedReasoningContent,
    ProviderContinuationId,
    LocalExactStateCartridge,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningContinuationStatusV1 {
    Exact,
    ScopedCertificate,
    Approximate,
    Unavailable,
    Expired,
    Rejected,
    IdentityMismatch,
}

impl ReasoningContinuationStatusV1 {
    const fn carries_payload(self) -> bool {
        matches!(
            self,
            Self::Exact | Self::ScopedCertificate | Self::Approximate
        )
    }
}

#[derive(Debug)]
pub enum ReasoningStateError {
    ReasoningContract(ReasoningContractErrorV1),
    ZeroIdentity(&'static str),
    MissingSamplerIdentity,
    EmptyPayload,
    PayloadTooLarge {
        actual: usize,
        limit: usize,
    },
    UnavailableKindHasPayload,
    PayloadStatusRequired,
    ScopedCertificateRequired,
    UnexpectedScopedCertificate,
    InvalidInitialOrder,
    MissingParentDigest,
    InvalidParentDigest,
    InvalidExpiry,
    InputTokenOverflow,
    TokenizerIdentityMismatch,
    ToolSchemaIdentityMismatch,
    NativeStatePolicyMismatch {
        policy: NativeStatePolicyV1,
        status: ReasoningContinuationStatusV1,
    },
    BindingMismatch,
    OrderMismatch,
    NotExact(ReasoningContinuationStatusV1),
    Expired,
    ContentDigestMismatch,
}

impl fmt::Display for ReasoningStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReasoningContract(error) => write!(f, "reasoning contract rejected: {error}"),
            Self::ZeroIdentity(field) => write!(f, "reasoning-state {field} identity is zero"),
            Self::MissingSamplerIdentity => {
                f.write_str("reasoning-state sampler identity is required but absent")
            }
            Self::EmptyPayload => f.write_str("opaque reasoning-state payload is empty"),
            Self::PayloadTooLarge { actual, limit } => write!(
                f,
                "opaque reasoning-state payload is {actual} bytes; limit is {limit}"
            ),
            Self::UnavailableKindHasPayload => {
                f.write_str("unavailable reasoning-state kind cannot carry opaque bytes")
            }
            Self::PayloadStatusRequired => {
                f.write_str("opaque bytes require exact, scoped-certificate, or approximate status")
            }
            Self::ScopedCertificateRequired => {
                f.write_str("scoped continuation status requires a certificate digest")
            }
            Self::UnexpectedScopedCertificate => {
                f.write_str("continuation certificate is present outside scoped status")
            }
            Self::InvalidInitialOrder => {
                f.write_str("initial reasoning state must not name a parent digest")
            }
            Self::MissingParentDigest => {
                f.write_str("noninitial reasoning state requires a parent digest")
            }
            Self::InvalidParentDigest => f.write_str("reasoning-state parent digest is zero"),
            Self::InvalidExpiry => f.write_str("reasoning-state expiry must be nonzero"),
            Self::InputTokenOverflow => {
                f.write_str("Decision View token count exceeds the hub headroom range")
            }
            Self::TokenizerIdentityMismatch => {
                f.write_str("Decision View tokenizer identity differs from reasoning contract")
            }
            Self::ToolSchemaIdentityMismatch => {
                f.write_str("Decision View tool schema differs from reasoning contract")
            }
            Self::NativeStatePolicyMismatch { policy, status } => write!(
                f,
                "reasoning-state status {status:?} is not authorized by native policy {policy:?}"
            ),
            Self::BindingMismatch => {
                f.write_str("reasoning-state replay identity binding does not match")
            }
            Self::OrderMismatch => {
                f.write_str("reasoning-state replay order or parent does not match")
            }
            Self::NotExact(status) => {
                write!(
                    f,
                    "reasoning-state status {status:?} cannot authorize exact replay"
                )
            }
            Self::Expired => f.write_str("reasoning state has expired"),
            Self::ContentDigestMismatch => {
                f.write_str("opaque reasoning-state bytes do not match their digest")
            }
        }
    }
}

impl Error for ReasoningStateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReasoningContract(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ReasoningContractErrorV1> for ReasoningStateError {
    fn from(error: ReasoningContractErrorV1) -> Self {
        Self::ReasoningContract(error)
    }
}

/// Complete execution binding for provider-native reasoning bytes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReasoningStateBindingV1 {
    provider_identity: DigestV1,
    model_identity: DigestV1,
    backend_identity: DigestV1,
    tokenizer_identity: DigestV1,
    decoder_identity: DigestV1,
    tool_schema_digest: DigestV1,
    reasoning_contract_digest: DigestV1,
    native_state_policy: NativeStatePolicyV1,
    position_identity: DigestV1,
    session_identity: DigestV1,
    sampler_identity_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    sampler_identity: Option<DigestV1>,
}

impl ReasoningStateBindingV1 {
    pub fn new(
        provider_identity: DigestV1,
        contract: &ReasoningContractV1,
        position_identity: DigestV1,
        session_identity: DigestV1,
        sampler_identity_required: bool,
        sampler_identity: Option<DigestV1>,
    ) -> Result<Self, ReasoningStateError> {
        contract.validate()?;
        for (field, digest) in [
            ("provider", provider_identity),
            ("position", position_identity),
            ("session", session_identity),
        ] {
            nonzero(field, digest)?;
        }
        if sampler_identity_required && sampler_identity.is_none() {
            return Err(ReasoningStateError::MissingSamplerIdentity);
        }
        if let Some(digest) = sampler_identity {
            nonzero("sampler", digest)?;
        }
        Ok(Self {
            provider_identity,
            model_identity: contract.model_identity(),
            backend_identity: contract.backend_identity(),
            tokenizer_identity: contract.tokenizer_identity(),
            decoder_identity: contract.decoder_identity(),
            tool_schema_digest: contract.tool_schema_digest(),
            reasoning_contract_digest: contract.identity_digest()?,
            native_state_policy: contract.native_state_policy(),
            position_identity,
            session_identity,
            sampler_identity_required,
            sampler_identity,
        })
    }

    pub const fn provider_identity(&self) -> DigestV1 {
        self.provider_identity
    }
    pub const fn model_identity(&self) -> DigestV1 {
        self.model_identity
    }
    pub const fn backend_identity(&self) -> DigestV1 {
        self.backend_identity
    }
    pub const fn tokenizer_identity(&self) -> DigestV1 {
        self.tokenizer_identity
    }
    pub const fn decoder_identity(&self) -> DigestV1 {
        self.decoder_identity
    }
    pub const fn tool_schema_digest(&self) -> DigestV1 {
        self.tool_schema_digest
    }
    pub const fn reasoning_contract_digest(&self) -> DigestV1 {
        self.reasoning_contract_digest
    }
    pub const fn native_state_policy(&self) -> NativeStatePolicyV1 {
        self.native_state_policy
    }
    pub const fn position_identity(&self) -> DigestV1 {
        self.position_identity
    }
    pub const fn session_identity(&self) -> DigestV1 {
        self.session_identity
    }
    pub const fn sampler_identity_required(&self) -> bool {
        self.sampler_identity_required
    }
    pub const fn sampler_identity(&self) -> Option<DigestV1> {
        self.sampler_identity
    }
}

/// Monotonic provider ordering and exact parent lineage.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ReasoningStateOrderV1 {
    sequence_index: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_content_digest: Option<DigestV1>,
}

impl ReasoningStateOrderV1 {
    pub fn new(
        sequence_index: u64,
        parent_content_digest: Option<DigestV1>,
    ) -> Result<Self, ReasoningStateError> {
        match (sequence_index, parent_content_digest) {
            (0, Some(_)) => return Err(ReasoningStateError::InvalidInitialOrder),
            (1.., None) => return Err(ReasoningStateError::MissingParentDigest),
            (_, Some(digest)) if digest == DigestV1::ZERO => {
                return Err(ReasoningStateError::InvalidParentDigest);
            }
            _ => {}
        }
        Ok(Self {
            sequence_index,
            parent_content_digest,
        })
    }

    pub const fn sequence_index(&self) -> u64 {
        self.sequence_index
    }
    pub const fn parent_content_digest(&self) -> Option<DigestV1> {
        self.parent_content_digest
    }
}

/// Serializable metadata only. It never contains provider-native reasoning bytes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OpaqueReasoningStateRefV1 {
    kind: OpaqueReasoningStateKindV1,
    status: ReasoningContinuationStatusV1,
    binding: ReasoningStateBindingV1,
    order: ReasoningStateOrderV1,
    content_digest: DigestV1,
    byte_len: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    continuation_certificate_digest: Option<DigestV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    valid_until_unix_ms: Option<u64>,
}

impl OpaqueReasoningStateRefV1 {
    pub fn unavailable(binding: ReasoningStateBindingV1) -> Self {
        Self {
            kind: OpaqueReasoningStateKindV1::Unavailable,
            status: ReasoningContinuationStatusV1::Unavailable,
            binding,
            order: ReasoningStateOrderV1 {
                sequence_index: 0,
                parent_content_digest: None,
            },
            content_digest: DigestV1::ZERO,
            byte_len: 0,
            continuation_certificate_digest: None,
            valid_until_unix_ms: None,
        }
    }

    pub fn rejected(
        kind: OpaqueReasoningStateKindV1,
        binding: ReasoningStateBindingV1,
        order: ReasoningStateOrderV1,
        content_digest: DigestV1,
    ) -> Result<Self, ReasoningStateError> {
        terminal_ref(
            kind,
            ReasoningContinuationStatusV1::Rejected,
            binding,
            order,
            content_digest,
            None,
        )
    }

    pub fn expired(
        kind: OpaqueReasoningStateKindV1,
        binding: ReasoningStateBindingV1,
        order: ReasoningStateOrderV1,
        content_digest: DigestV1,
        valid_until_unix_ms: u64,
    ) -> Result<Self, ReasoningStateError> {
        terminal_ref(
            kind,
            ReasoningContinuationStatusV1::Expired,
            binding,
            order,
            content_digest,
            Some(valid_until_unix_ms),
        )
    }

    pub fn identity_mismatch(
        kind: OpaqueReasoningStateKindV1,
        binding: ReasoningStateBindingV1,
        order: ReasoningStateOrderV1,
        content_digest: DigestV1,
    ) -> Result<Self, ReasoningStateError> {
        terminal_ref(
            kind,
            ReasoningContinuationStatusV1::IdentityMismatch,
            binding,
            order,
            content_digest,
            None,
        )
    }

    pub const fn kind(&self) -> OpaqueReasoningStateKindV1 {
        self.kind
    }
    pub const fn status(&self) -> ReasoningContinuationStatusV1 {
        self.status
    }
    pub fn binding(&self) -> &ReasoningStateBindingV1 {
        &self.binding
    }
    pub const fn order(&self) -> ReasoningStateOrderV1 {
        self.order
    }
    pub const fn content_digest(&self) -> DigestV1 {
        self.content_digest
    }
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }
    pub const fn continuation_certificate_digest(&self) -> Option<DigestV1> {
        self.continuation_certificate_digest
    }
    pub const fn valid_until_unix_ms(&self) -> Option<u64> {
        self.valid_until_unix_ms
    }
}

/// In-memory opaque pass-through. `Debug` is redacted and `Serialize` is absent.
pub struct OpaqueReasoningStateEnvelopeV1 {
    reference: OpaqueReasoningStateRefV1,
    opaque_bytes: Vec<u8>,
}

impl OpaqueReasoningStateEnvelopeV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn capture(
        kind: OpaqueReasoningStateKindV1,
        status: ReasoningContinuationStatusV1,
        binding: ReasoningStateBindingV1,
        order: ReasoningStateOrderV1,
        continuation_certificate_digest: Option<DigestV1>,
        valid_until_unix_ms: Option<u64>,
        opaque_bytes: Vec<u8>,
    ) -> Result<Self, ReasoningStateError> {
        if kind == OpaqueReasoningStateKindV1::Unavailable {
            return Err(ReasoningStateError::UnavailableKindHasPayload);
        }
        if !status.carries_payload() {
            return Err(ReasoningStateError::PayloadStatusRequired);
        }
        if opaque_bytes.is_empty() {
            return Err(ReasoningStateError::EmptyPayload);
        }
        if opaque_bytes.len() > MAX_OPAQUE_REASONING_STATE_BYTES {
            return Err(ReasoningStateError::PayloadTooLarge {
                actual: opaque_bytes.len(),
                limit: MAX_OPAQUE_REASONING_STATE_BYTES,
            });
        }
        validate_certificate(status, continuation_certificate_digest)?;
        validate_native_state_policy(binding.native_state_policy(), status)?;
        validate_expiry(valid_until_unix_ms)?;
        let byte_len = u64::try_from(opaque_bytes.len()).map_err(|_| {
            ReasoningStateError::PayloadTooLarge {
                actual: opaque_bytes.len(),
                limit: MAX_OPAQUE_REASONING_STATE_BYTES,
            }
        })?;
        let reference = OpaqueReasoningStateRefV1 {
            kind,
            status,
            binding,
            order,
            content_digest: digest(&opaque_bytes),
            byte_len,
            continuation_certificate_digest,
            valid_until_unix_ms,
        };
        Ok(Self {
            reference,
            opaque_bytes,
        })
    }

    pub fn reference(&self) -> &OpaqueReasoningStateRefV1 {
        &self.reference
    }

    /// Exact original provider bytes, with no parse/rewrite step.
    ///
    /// This accessor does not upgrade continuation status. Strict replay must
    /// use [`Self::exact_replay_bytes`].
    pub fn opaque_bytes(&self) -> &[u8] {
        &self.opaque_bytes
    }

    pub fn exact_replay_bytes(
        &self,
        expected_binding: &ReasoningStateBindingV1,
        expected_order: ReasoningStateOrderV1,
        now_unix_ms: u64,
    ) -> Result<&[u8], ReasoningStateError> {
        if self.reference.status != ReasoningContinuationStatusV1::Exact {
            return Err(ReasoningStateError::NotExact(self.reference.status));
        }
        validate_native_state_policy(
            self.reference.binding.native_state_policy(),
            self.reference.status,
        )?;
        if &self.reference.binding != expected_binding {
            return Err(ReasoningStateError::BindingMismatch);
        }
        if self.reference.order != expected_order {
            return Err(ReasoningStateError::OrderMismatch);
        }
        if self
            .reference
            .valid_until_unix_ms
            .is_some_and(|expiry| now_unix_ms >= expiry)
        {
            return Err(ReasoningStateError::Expired);
        }
        if digest(&self.opaque_bytes) != self.reference.content_digest {
            return Err(ReasoningStateError::ContentDigestMismatch);
        }
        Ok(&self.opaque_bytes)
    }
}

impl fmt::Debug for OpaqueReasoningStateEnvelopeV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpaqueReasoningStateEnvelopeV1")
            .field("reference", &self.reference)
            .field(
                "opaque_bytes",
                &format_args!("<redacted:{} bytes>", self.opaque_bytes.len()),
            )
            .finish()
    }
}

/// Exactness class for one model-state continuation assessment.
///
/// Only `ExactNeutral` describes identical native continuation state. Scoped
/// and empirical evidence stay non-pointwise and never authorize exact replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "class")]
enum ModelStateContinuationClassV1 {
    ExactNeutral {
        state_content_digest: DigestV1,
    },
    ScopedCertificate {
        state_content_digest: DigestV1,
        certificate_digest: DigestV1,
        declared_scope_digest: DigestV1,
    },
    Empirical {
        state_content_digest: DigestV1,
        frozen_distribution_digest: DigestV1,
        evaluation_receipt_digest: DigestV1,
        declared_scope_digest: DigestV1,
        evidence_valid_until_unix_ms: Option<u64>,
    },
    Unavailable {
        reason: ModelStateUnavailableReasonV1,
    },
}

/// Public discriminant for the validated continuation class. It is descriptive
/// only; the unforgeable receipt is `ModelStateContinuationAssessmentV1`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStateContinuationKindV1 {
    ExactNeutral,
    ScopedCertificate,
    Empirical,
    Unavailable,
}

impl ModelStateContinuationClassV1 {
    const fn kind(&self) -> ModelStateContinuationKindV1 {
        match self {
            Self::ExactNeutral { .. } => ModelStateContinuationKindV1::ExactNeutral,
            Self::ScopedCertificate { .. } => ModelStateContinuationKindV1::ScopedCertificate,
            Self::Empirical { .. } => ModelStateContinuationKindV1::Empirical,
            Self::Unavailable { .. } => ModelStateContinuationKindV1::Unavailable,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStateUnavailableReasonV1 {
    ProviderUnavailable,
    StateExpired,
    StateRejected,
    IdentityMismatch,
    ScopedEvidenceAbsent,
    EmpiricalEvidenceAbsent,
    EmpiricalEvidenceExpired,
}

/// Evidence supplied with an assessment. Evidence can only preserve the
/// class already declared by the validated opaque-state reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelStateContinuationEvidenceV1 {
    None,
    Scoped {
        certificate_digest: DigestV1,
        declared_scope_digest: DigestV1,
    },
    Empirical {
        frozen_distribution_digest: DigestV1,
        evaluation_receipt_digest: DigestV1,
        declared_scope_digest: DigestV1,
        valid_until_unix_ms: Option<u64>,
    },
}

impl ModelStateContinuationEvidenceV1 {
    pub fn scoped(
        certificate_digest: DigestV1,
        declared_scope_digest: DigestV1,
    ) -> Result<Self, ModelStateContinuationErrorV1> {
        require_continuation_digest("certificate", certificate_digest)?;
        require_continuation_digest("declared scope", declared_scope_digest)?;
        Ok(Self::Scoped {
            certificate_digest,
            declared_scope_digest,
        })
    }

    pub fn empirical(
        frozen_distribution_digest: DigestV1,
        evaluation_receipt_digest: DigestV1,
        declared_scope_digest: DigestV1,
        valid_until_unix_ms: Option<u64>,
    ) -> Result<Self, ModelStateContinuationErrorV1> {
        require_continuation_digest("frozen distribution", frozen_distribution_digest)?;
        require_continuation_digest("evaluation receipt", evaluation_receipt_digest)?;
        require_continuation_digest("declared scope", declared_scope_digest)?;
        if valid_until_unix_ms == Some(0) {
            return Err(ModelStateContinuationErrorV1::InvalidEvidenceExpiry);
        }
        Ok(Self::Empirical {
            frozen_distribution_digest,
            evaluation_receipt_digest,
            declared_scope_digest,
            valid_until_unix_ms,
        })
    }
}

/// Serializable raw Decision View recovery metadata. The exact bytes live only
/// in `RawDecisionViewRecoveryEnvelopeV1` and never enter this receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RawDecisionViewRecoveryRefV1 {
    decision_view_identity: DecisionViewIdentity,
    decision_view_digest: DigestV1,
    exact_token_map_digest: DigestV1,
    raw_bytes_digest: DigestV1,
    raw_byte_len: u64,
    total_tokens: u64,
    caller_raw_baseline_identity_digest: DigestV1,
    caller_hub_safepoint_digest: DigestV1,
}

impl RawDecisionViewRecoveryRefV1 {
    pub fn decision_view_identity(&self) -> &DecisionViewIdentity {
        &self.decision_view_identity
    }

    pub const fn decision_view_digest(&self) -> DigestV1 {
        self.decision_view_digest
    }

    pub const fn exact_token_map_digest(&self) -> DigestV1 {
        self.exact_token_map_digest
    }

    pub const fn raw_bytes_digest(&self) -> DigestV1 {
        self.raw_bytes_digest
    }

    pub const fn raw_byte_len(&self) -> u64 {
        self.raw_byte_len
    }

    pub const fn total_tokens(&self) -> u64 {
        self.total_tokens
    }

    pub const fn caller_raw_baseline_identity_digest(&self) -> DigestV1 {
        self.caller_raw_baseline_identity_digest
    }

    pub const fn caller_hub_safepoint_digest(&self) -> DigestV1 {
        self.caller_hub_safepoint_digest
    }
}

/// In-memory exact raw Decision View carrier for guarded fallback.
///
/// This type does not verify or create hub safepoints, persist CAS objects, or
/// trigger deoptimization. It only binds caller-supplied hub identities to the
/// exact canonical Decision View bytes and checks them before recovery.
pub struct RawDecisionViewRecoveryEnvelopeV1 {
    reference: RawDecisionViewRecoveryRefV1,
    raw_decision_view_bytes: Vec<u8>,
}

impl RawDecisionViewRecoveryEnvelopeV1 {
    pub fn capture(
        decision_view: &DecisionView,
        caller_raw_baseline_identity_digest: DigestV1,
        caller_hub_safepoint_digest: DigestV1,
    ) -> Result<Self, ModelStateContinuationErrorV1> {
        require_continuation_digest(
            "caller raw-baseline identity",
            caller_raw_baseline_identity_digest,
        )?;
        require_continuation_digest("caller hub safepoint", caller_hub_safepoint_digest)?;
        let raw_decision_view_bytes = decision_view.rendered().to_vec();
        let raw_byte_len = u64::try_from(raw_decision_view_bytes.len())
            .map_err(|_| ModelStateContinuationErrorV1::RawByteLengthOverflow)?;
        let reference = RawDecisionViewRecoveryRefV1 {
            decision_view_identity: decision_view.identity().clone(),
            decision_view_digest: decision_view.digest(),
            exact_token_map_digest: decision_view.exact_token_map_digest(),
            raw_bytes_digest: digest(&raw_decision_view_bytes),
            raw_byte_len,
            total_tokens: decision_view.total_tokens(),
            caller_raw_baseline_identity_digest,
            caller_hub_safepoint_digest,
        };
        Ok(Self {
            reference,
            raw_decision_view_bytes,
        })
    }

    pub fn reference(&self) -> &RawDecisionViewRecoveryRefV1 {
        &self.reference
    }

    #[allow(clippy::too_many_arguments)]
    pub fn exact_raw_decision_view_bytes(
        &self,
        expected_decision_view_identity: &DecisionViewIdentity,
        expected_decision_view_digest: DigestV1,
        expected_exact_token_map_digest: DigestV1,
        expected_raw_baseline_identity_digest: DigestV1,
        expected_hub_safepoint_digest: DigestV1,
    ) -> Result<&[u8], ModelStateContinuationErrorV1> {
        if &self.reference.decision_view_identity != expected_decision_view_identity {
            return Err(ModelStateContinuationErrorV1::DecisionViewIdentityMismatch);
        }
        if self.reference.decision_view_digest != expected_decision_view_digest {
            return Err(ModelStateContinuationErrorV1::DecisionViewDigestMismatch);
        }
        if self.reference.exact_token_map_digest != expected_exact_token_map_digest {
            return Err(ModelStateContinuationErrorV1::ExactTokenMapDigestMismatch);
        }
        if self.reference.caller_raw_baseline_identity_digest
            != expected_raw_baseline_identity_digest
        {
            return Err(ModelStateContinuationErrorV1::RawBaselineIdentityMismatch);
        }
        if self.reference.caller_hub_safepoint_digest != expected_hub_safepoint_digest {
            return Err(ModelStateContinuationErrorV1::HubSafepointDigestMismatch);
        }
        let actual_len = u64::try_from(self.raw_decision_view_bytes.len())
            .map_err(|_| ModelStateContinuationErrorV1::RawByteLengthOverflow)?;
        if actual_len != self.reference.raw_byte_len {
            return Err(ModelStateContinuationErrorV1::RawByteLengthMismatch);
        }
        if digest(&self.raw_decision_view_bytes) != self.reference.raw_bytes_digest {
            return Err(ModelStateContinuationErrorV1::RawBytesDigestMismatch);
        }
        Ok(&self.raw_decision_view_bytes)
    }
}

impl fmt::Debug for RawDecisionViewRecoveryEnvelopeV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawDecisionViewRecoveryEnvelopeV1")
            .field("reference", &self.reference)
            .field(
                "raw_decision_view_bytes",
                &format_args!("<redacted:{} bytes>", self.raw_decision_view_bytes.len()),
            )
            .finish()
    }
}

/// Receipt-visible continuation classification plus an exact raw fallback.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelStateContinuationAssessmentV1 {
    state_reference: OpaqueReasoningStateRefV1,
    class: ModelStateContinuationClassV1,
    raw_recovery: RawDecisionViewRecoveryRefV1,
}

impl ModelStateContinuationAssessmentV1 {
    pub fn assess(
        state_reference: &OpaqueReasoningStateRefV1,
        evidence: ModelStateContinuationEvidenceV1,
        raw_recovery: &RawDecisionViewRecoveryRefV1,
        now_unix_ms: u64,
    ) -> Result<Self, ModelStateContinuationErrorV1> {
        validate_continuation_evidence_for_state(state_reference, &evidence)?;
        let expired = state_reference
            .valid_until_unix_ms()
            .is_some_and(|expiry| now_unix_ms >= expiry);
        let class = if expired {
            ModelStateContinuationClassV1::Unavailable {
                reason: ModelStateUnavailableReasonV1::StateExpired,
            }
        } else {
            match (state_reference.status(), evidence) {
                (ReasoningContinuationStatusV1::Exact, ModelStateContinuationEvidenceV1::None) => {
                    ModelStateContinuationClassV1::ExactNeutral {
                        state_content_digest: state_reference.content_digest(),
                    }
                }
                (
                    ReasoningContinuationStatusV1::ScopedCertificate,
                    ModelStateContinuationEvidenceV1::Scoped {
                        certificate_digest,
                        declared_scope_digest,
                    },
                ) => {
                    if state_reference.continuation_certificate_digest() != Some(certificate_digest)
                    {
                        return Err(ModelStateContinuationErrorV1::ScopedCertificateMismatch);
                    }
                    ModelStateContinuationClassV1::ScopedCertificate {
                        state_content_digest: state_reference.content_digest(),
                        certificate_digest,
                        declared_scope_digest,
                    }
                }
                (
                    ReasoningContinuationStatusV1::ScopedCertificate,
                    ModelStateContinuationEvidenceV1::None,
                ) => ModelStateContinuationClassV1::Unavailable {
                    reason: ModelStateUnavailableReasonV1::ScopedEvidenceAbsent,
                },
                (
                    ReasoningContinuationStatusV1::Approximate,
                    ModelStateContinuationEvidenceV1::Empirical {
                        frozen_distribution_digest: _,
                        evaluation_receipt_digest: _,
                        declared_scope_digest: _,
                        valid_until_unix_ms,
                    },
                ) if valid_until_unix_ms.is_some_and(|expiry| now_unix_ms >= expiry) => {
                    ModelStateContinuationClassV1::Unavailable {
                        reason: ModelStateUnavailableReasonV1::EmpiricalEvidenceExpired,
                    }
                }
                (
                    ReasoningContinuationStatusV1::Approximate,
                    ModelStateContinuationEvidenceV1::Empirical {
                        frozen_distribution_digest,
                        evaluation_receipt_digest,
                        declared_scope_digest,
                        valid_until_unix_ms,
                    },
                ) => ModelStateContinuationClassV1::Empirical {
                    state_content_digest: state_reference.content_digest(),
                    frozen_distribution_digest,
                    evaluation_receipt_digest,
                    declared_scope_digest,
                    evidence_valid_until_unix_ms: valid_until_unix_ms,
                },
                (
                    ReasoningContinuationStatusV1::Approximate,
                    ModelStateContinuationEvidenceV1::None,
                ) => ModelStateContinuationClassV1::Unavailable {
                    reason: ModelStateUnavailableReasonV1::EmpiricalEvidenceAbsent,
                },
                (
                    ReasoningContinuationStatusV1::Unavailable,
                    ModelStateContinuationEvidenceV1::None,
                ) => ModelStateContinuationClassV1::Unavailable {
                    reason: ModelStateUnavailableReasonV1::ProviderUnavailable,
                },
                (
                    ReasoningContinuationStatusV1::Expired,
                    ModelStateContinuationEvidenceV1::None,
                ) => ModelStateContinuationClassV1::Unavailable {
                    reason: ModelStateUnavailableReasonV1::StateExpired,
                },
                (
                    ReasoningContinuationStatusV1::Rejected,
                    ModelStateContinuationEvidenceV1::None,
                ) => ModelStateContinuationClassV1::Unavailable {
                    reason: ModelStateUnavailableReasonV1::StateRejected,
                },
                (
                    ReasoningContinuationStatusV1::IdentityMismatch,
                    ModelStateContinuationEvidenceV1::None,
                ) => ModelStateContinuationClassV1::Unavailable {
                    reason: ModelStateUnavailableReasonV1::IdentityMismatch,
                },
                _ => return Err(ModelStateContinuationErrorV1::EvidenceStatusMismatch),
            }
        };
        Ok(Self {
            state_reference: state_reference.clone(),
            class,
            raw_recovery: raw_recovery.clone(),
        })
    }

    pub fn state_reference(&self) -> &OpaqueReasoningStateRefV1 {
        &self.state_reference
    }

    pub const fn class(&self) -> ModelStateContinuationKindV1 {
        self.class.kind()
    }

    pub const fn unavailable_reason(&self) -> Option<ModelStateUnavailableReasonV1> {
        match self.class {
            ModelStateContinuationClassV1::Unavailable { reason } => Some(reason),
            _ => None,
        }
    }

    pub const fn scoped_evidence(&self) -> Option<(DigestV1, DigestV1)> {
        match self.class {
            ModelStateContinuationClassV1::ScopedCertificate {
                certificate_digest,
                declared_scope_digest,
                ..
            } => Some((certificate_digest, declared_scope_digest)),
            _ => None,
        }
    }

    pub const fn empirical_evidence(&self) -> Option<(DigestV1, DigestV1, DigestV1, Option<u64>)> {
        match self.class {
            ModelStateContinuationClassV1::Empirical {
                frozen_distribution_digest,
                evaluation_receipt_digest,
                declared_scope_digest,
                evidence_valid_until_unix_ms,
                ..
            } => Some((
                frozen_distribution_digest,
                evaluation_receipt_digest,
                declared_scope_digest,
                evidence_valid_until_unix_ms,
            )),
            _ => None,
        }
    }

    pub fn raw_recovery(&self) -> &RawDecisionViewRecoveryRefV1 {
        &self.raw_recovery
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelStateContinuationErrorV1 {
    ZeroIdentity(&'static str),
    InvalidEvidenceExpiry,
    EvidenceStatusMismatch,
    ScopedCertificateMismatch,
    RawByteLengthOverflow,
    DecisionViewIdentityMismatch,
    DecisionViewDigestMismatch,
    ExactTokenMapDigestMismatch,
    RawBaselineIdentityMismatch,
    HubSafepointDigestMismatch,
    RawByteLengthMismatch,
    RawBytesDigestMismatch,
}

impl fmt::Display for ModelStateContinuationErrorV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroIdentity(field) => {
                write!(f, "model-state continuation {field} digest is zero")
            }
            Self::InvalidEvidenceExpiry => {
                f.write_str("model-state empirical evidence expiry must be nonzero")
            }
            Self::EvidenceStatusMismatch => {
                f.write_str("model-state evidence does not match the declared continuation status")
            }
            Self::ScopedCertificateMismatch => f.write_str(
                "model-state scoped evidence certificate differs from the state reference",
            ),
            Self::RawByteLengthOverflow => {
                f.write_str("raw Decision View byte length exceeds the receipt range")
            }
            Self::DecisionViewIdentityMismatch => {
                f.write_str("raw Decision View identity differs from the expected identity")
            }
            Self::DecisionViewDigestMismatch => {
                f.write_str("raw Decision View digest differs from the expected digest")
            }
            Self::ExactTokenMapDigestMismatch => {
                f.write_str("raw Decision View token map differs from the expected token map")
            }
            Self::RawBaselineIdentityMismatch => {
                f.write_str("caller raw-baseline identity differs from the recovery binding")
            }
            Self::HubSafepointDigestMismatch => {
                f.write_str("caller hub safepoint digest differs from the recovery binding")
            }
            Self::RawByteLengthMismatch => {
                f.write_str("raw Decision View byte length differs from recovery metadata")
            }
            Self::RawBytesDigestMismatch => {
                f.write_str("raw Decision View bytes differ from recovery metadata")
            }
        }
    }
}

impl Error for ModelStateContinuationErrorV1 {}

fn validate_continuation_evidence_for_state(
    state_reference: &OpaqueReasoningStateRefV1,
    evidence: &ModelStateContinuationEvidenceV1,
) -> Result<(), ModelStateContinuationErrorV1> {
    match evidence {
        ModelStateContinuationEvidenceV1::None => {}
        ModelStateContinuationEvidenceV1::Scoped {
            certificate_digest,
            declared_scope_digest,
        } => {
            require_continuation_digest("certificate", *certificate_digest)?;
            require_continuation_digest("declared scope", *declared_scope_digest)?;
        }
        ModelStateContinuationEvidenceV1::Empirical {
            frozen_distribution_digest,
            evaluation_receipt_digest,
            declared_scope_digest,
            valid_until_unix_ms,
        } => {
            require_continuation_digest("frozen distribution", *frozen_distribution_digest)?;
            require_continuation_digest("evaluation receipt", *evaluation_receipt_digest)?;
            require_continuation_digest("declared scope", *declared_scope_digest)?;
            if *valid_until_unix_ms == Some(0) {
                return Err(ModelStateContinuationErrorV1::InvalidEvidenceExpiry);
            }
        }
    }
    match (state_reference.status(), evidence) {
        (ReasoningContinuationStatusV1::Exact, ModelStateContinuationEvidenceV1::None)
        | (
            ReasoningContinuationStatusV1::ScopedCertificate,
            ModelStateContinuationEvidenceV1::None,
        )
        | (ReasoningContinuationStatusV1::Approximate, ModelStateContinuationEvidenceV1::None)
        | (
            ReasoningContinuationStatusV1::Approximate,
            ModelStateContinuationEvidenceV1::Empirical { .. },
        )
        | (
            ReasoningContinuationStatusV1::Unavailable
            | ReasoningContinuationStatusV1::Expired
            | ReasoningContinuationStatusV1::Rejected
            | ReasoningContinuationStatusV1::IdentityMismatch,
            ModelStateContinuationEvidenceV1::None,
        ) => Ok(()),
        (
            ReasoningContinuationStatusV1::ScopedCertificate,
            ModelStateContinuationEvidenceV1::Scoped {
                certificate_digest, ..
            },
        ) if state_reference.continuation_certificate_digest() == Some(*certificate_digest) => {
            Ok(())
        }
        (
            ReasoningContinuationStatusV1::ScopedCertificate,
            ModelStateContinuationEvidenceV1::Scoped { .. },
        ) => Err(ModelStateContinuationErrorV1::ScopedCertificateMismatch),
        _ => Err(ModelStateContinuationErrorV1::EvidenceStatusMismatch),
    }
}

fn require_continuation_digest(
    field: &'static str,
    digest: DigestV1,
) -> Result<(), ModelStateContinuationErrorV1> {
    if digest == DigestV1::ZERO {
        Err(ModelStateContinuationErrorV1::ZeroIdentity(field))
    } else {
        Ok(())
    }
}

/// Receipt-visible proof that input rendering preserved all protected reserves.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DecisionViewHeadroomPlanV1 {
    reasoning_contract_digest: DigestV1,
    decision_view_digest: DigestV1,
    exact_token_map_digest: DigestV1,
    context_capacity: u32,
    logical_input_tokens: u32,
    max_output_tokens: u32,
    reserved_reasoning_tokens: u32,
    reserved_visible_output_tokens: u32,
    reserved_recovery_tokens: u32,
    reserved_tool_tokens: u32,
    admitted_input_ceiling: u32,
    remaining_input_headroom: u32,
}

impl DecisionViewHeadroomPlanV1 {
    pub fn plan(
        contract: &ReasoningContractV1,
        context_capacity: u32,
        reserved_tool_tokens: u32,
        view: &DecisionView,
    ) -> Result<Self, ReasoningStateError> {
        contract.validate()?;
        if contract.tokenizer_identity() != view.identity().tokenizer_identity_digest() {
            return Err(ReasoningStateError::TokenizerIdentityMismatch);
        }
        if contract.tool_schema_digest() != view.identity().tool_schema_digest() {
            return Err(ReasoningStateError::ToolSchemaIdentityMismatch);
        }
        let logical_input_tokens = u32::try_from(view.total_tokens())
            .map_err(|_| ReasoningStateError::InputTokenOverflow)?;
        let admitted_input_ceiling =
            contract.admitted_input_ceiling(context_capacity, reserved_tool_tokens)?;
        let remaining_input_headroom =
            contract.admit_input(context_capacity, reserved_tool_tokens, logical_input_tokens)?;
        Ok(Self {
            reasoning_contract_digest: contract.identity_digest()?,
            decision_view_digest: view.digest(),
            exact_token_map_digest: view.exact_token_map_digest(),
            context_capacity,
            logical_input_tokens,
            max_output_tokens: contract.max_output_tokens(),
            reserved_reasoning_tokens: contract.reserved_reasoning_tokens(),
            reserved_visible_output_tokens: contract.reserved_visible_output_tokens(),
            reserved_recovery_tokens: contract.reserved_recovery_tokens(),
            reserved_tool_tokens,
            admitted_input_ceiling,
            remaining_input_headroom,
        })
    }

    pub const fn reasoning_contract_digest(&self) -> DigestV1 {
        self.reasoning_contract_digest
    }
    pub const fn decision_view_digest(&self) -> DigestV1 {
        self.decision_view_digest
    }
    pub const fn exact_token_map_digest(&self) -> DigestV1 {
        self.exact_token_map_digest
    }
    pub const fn context_capacity(&self) -> u32 {
        self.context_capacity
    }
    pub const fn logical_input_tokens(&self) -> u32 {
        self.logical_input_tokens
    }
    pub const fn max_output_tokens(&self) -> u32 {
        self.max_output_tokens
    }
    pub const fn reserved_reasoning_tokens(&self) -> u32 {
        self.reserved_reasoning_tokens
    }
    pub const fn reserved_visible_output_tokens(&self) -> u32 {
        self.reserved_visible_output_tokens
    }
    pub const fn reserved_recovery_tokens(&self) -> u32 {
        self.reserved_recovery_tokens
    }
    pub const fn reserved_tool_tokens(&self) -> u32 {
        self.reserved_tool_tokens
    }
    pub const fn admitted_input_ceiling(&self) -> u32 {
        self.admitted_input_ceiling
    }
    pub const fn remaining_input_headroom(&self) -> u32 {
        self.remaining_input_headroom
    }
}

fn terminal_ref(
    kind: OpaqueReasoningStateKindV1,
    status: ReasoningContinuationStatusV1,
    binding: ReasoningStateBindingV1,
    order: ReasoningStateOrderV1,
    content_digest: DigestV1,
    valid_until_unix_ms: Option<u64>,
) -> Result<OpaqueReasoningStateRefV1, ReasoningStateError> {
    if kind == OpaqueReasoningStateKindV1::Unavailable {
        return Err(ReasoningStateError::UnavailableKindHasPayload);
    }
    nonzero("content", content_digest)?;
    validate_expiry(valid_until_unix_ms)?;
    Ok(OpaqueReasoningStateRefV1 {
        kind,
        status,
        binding,
        order,
        content_digest,
        byte_len: 0,
        continuation_certificate_digest: None,
        valid_until_unix_ms,
    })
}

fn validate_native_state_policy(
    policy: NativeStatePolicyV1,
    status: ReasoningContinuationStatusV1,
) -> Result<(), ReasoningStateError> {
    let authorized = matches!(
        (policy, status),
        (
            NativeStatePolicyV1::ExactRequired | NativeStatePolicyV1::ExactIfAvailable,
            ReasoningContinuationStatusV1::Exact
        ) | (
            NativeStatePolicyV1::ExactIfAvailable,
            ReasoningContinuationStatusV1::Approximate
        ) | (
            NativeStatePolicyV1::ScopedCertificate,
            ReasoningContinuationStatusV1::ScopedCertificate
        )
    );
    if authorized {
        Ok(())
    } else {
        Err(ReasoningStateError::NativeStatePolicyMismatch { policy, status })
    }
}

fn validate_certificate(
    status: ReasoningContinuationStatusV1,
    certificate: Option<DigestV1>,
) -> Result<(), ReasoningStateError> {
    match (status, certificate) {
        (ReasoningContinuationStatusV1::ScopedCertificate, None) => {
            Err(ReasoningStateError::ScopedCertificateRequired)
        }
        (ReasoningContinuationStatusV1::ScopedCertificate, Some(digest)) => {
            nonzero("continuation certificate", digest)
        }
        (_, Some(_)) => Err(ReasoningStateError::UnexpectedScopedCertificate),
        _ => Ok(()),
    }
}

fn validate_expiry(expiry: Option<u64>) -> Result<(), ReasoningStateError> {
    if expiry == Some(0) {
        Err(ReasoningStateError::InvalidExpiry)
    } else {
        Ok(())
    }
}

fn nonzero(field: &'static str, digest: DigestV1) -> Result<(), ReasoningStateError> {
    if digest == DigestV1::ZERO {
        Err(ReasoningStateError::ZeroIdentity(field))
    } else {
        Ok(())
    }
}

fn digest(bytes: &[u8]) -> DigestV1 {
    DigestV1::from_bytes(sha256(bytes))
}

#[cfg(test)]
#[path = "../../../tests/core/inline/reasoning_state__tests.rs"]
mod tests;
