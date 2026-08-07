//! Opaque reasoning-state transport and protected Decision View headroom.
//!
//! Opaque provider bytes are never parsed, summarized, reordered, or serialized
//! by this module. Metadata binds them to the exact provider/model/backend,
//! reasoning contract, session, position, sampler, and lineage. Exact replay is
//! refused unless every binding matches. Headroom arithmetic delegates to the
//! canonical ZeroStack [`ReasoningContractV1`] contract.

use crate::decision_view::DecisionView;
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
mod tests {
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
        let next =
            ReasoningStateOrderV1::new(1, Some(envelope.reference().content_digest())).unwrap();

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

        let scoped_contract =
            contract_with_policy(d(3), d(5), NativeStatePolicyV1::ScopedCertificate);
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
}
