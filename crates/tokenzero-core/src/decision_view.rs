//! Deterministic Decision View rendering and stable-prefix geometry.
//!
//! ZeroStack selects and orders the semantic components. TokenZero only
//! validates their identities, renders the supplied order deterministically,
//! and records exact byte/token geometry. Prefix byte identity is not a claim
//! of provider eligibility, retention, routing, or cache hit.

use crate::model_artifacts::{
    ExactTokenMap, ExactTokenizerAdapter, ExactTokenizerIdentity, ModelArtifactError, ModelCapsule,
    TokenPage,
};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use zero_abi::{DigestV1, sha256};
use zero_ref::ZeroRefV1;

pub const MAX_DECISION_VIEW_SECTIONS: usize = 1_024;
pub const MAX_DECISION_VIEW_BYTES: usize = 16 * 1_048_576;
pub const MAX_DECISION_VIEW_RECOVERY_REFS: usize = 4_096;
const MAX_MARKER_FIELD_BYTES: usize = 16_384;
const RENDERER_CONTRACT: &[u8] = b"tokenzero.decision-view.renderer.v1; framing=section-kind+decimal-byte-length+lf+payload+lf; order=caller-preserved; stable=system-tool,project-capsule,task-family-capsule,typed-effect-schema; volatile=locus-evidence,working-tree-delta,user-task,uncertainty-coverage,recovery-routes";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecisionViewError {
    ModelArtifact(ModelArtifactError),
    TooManySections { actual: usize, limit: usize },
    TooManyRecoveryRefs { actual: usize, limit: usize },
    ViewByteLimit { actual: usize, limit: usize },
    LengthOverflow,
    StableSectionAfterVolatile { index: usize },
    TokenizerIdentityMismatch { section: DecisionViewSectionKind },
    ToolSchemaDigestMismatch,
    CapsuleSourceRootMismatch,
    CapsuleModelProfileMismatch,
    InvalidRecoveryRef(String),
    NoncanonicalRecoveryRef(String),
    EmptyMarkerCode,
    MarkerFieldTooLong,
    PrefixNotTokenAligned { byte_offset: u64 },
}

impl fmt::Display for DecisionViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid TokenZero Decision View: {self:?}")
    }
}

impl Error for DecisionViewError {}

impl From<ModelArtifactError> for DecisionViewError {
    fn from(error: ModelArtifactError) -> Self {
        Self::ModelArtifact(error)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionViewSectionKind {
    StableSystemToolContract,
    StableProjectCapsule,
    StableTaskFamilyCapsule,
    StableTypedEffectSchema,
    VolatileLocusEvidence,
    VolatileWorkingTreeDelta,
    VolatileUserTask,
    VolatileUncertaintyCoverage,
    VolatileRecoveryRoutes,
}

impl DecisionViewSectionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StableSystemToolContract => "stable_system_tool_contract",
            Self::StableProjectCapsule => "stable_project_capsule",
            Self::StableTaskFamilyCapsule => "stable_task_family_capsule",
            Self::StableTypedEffectSchema => "stable_typed_effect_schema",
            Self::VolatileLocusEvidence => "volatile_locus_evidence",
            Self::VolatileWorkingTreeDelta => "volatile_working_tree_delta",
            Self::VolatileUserTask => "volatile_user_task",
            Self::VolatileUncertaintyCoverage => "volatile_uncertainty_coverage",
            Self::VolatileRecoveryRoutes => "volatile_recovery_routes",
        }
    }

    pub const fn is_stable(self) -> bool {
        matches!(
            self,
            Self::StableSystemToolContract
                | Self::StableProjectCapsule
                | Self::StableTaskFamilyCapsule
                | Self::StableTypedEffectSchema
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionUncertaintyKind {
    Exact,
    SoundOverapproximation,
    PartialCoverage,
    Heuristic,
    Unknown,
}

impl DecisionUncertaintyKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::SoundOverapproximation => "sound_overapproximation",
            Self::PartialCoverage => "partial_coverage",
            Self::Heuristic => "heuristic",
            Self::Unknown => "unknown",
        }
    }
}

/// Caller-supplied epistemic marker. TokenZero renders it but never upgrades it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DecisionUncertaintyMarker {
    kind: DecisionUncertaintyKind,
    code: String,
    message: String,
    recovery_refs: Vec<String>,
}

impl DecisionUncertaintyMarker {
    pub fn new(
        kind: DecisionUncertaintyKind,
        code: impl Into<String>,
        message: impl Into<String>,
        recovery_refs: Vec<String>,
    ) -> Result<Self, DecisionViewError> {
        let code = code.into();
        let message = message.into();
        if code.is_empty() {
            return Err(DecisionViewError::EmptyMarkerCode);
        }
        if code.len() > MAX_MARKER_FIELD_BYTES || message.len() > MAX_MARKER_FIELD_BYTES {
            return Err(DecisionViewError::MarkerFieldTooLong);
        }
        validate_refs(&recovery_refs)?;
        Ok(Self {
            kind,
            code,
            message,
            recovery_refs,
        })
    }

    pub const fn kind(&self) -> DecisionUncertaintyKind {
        self.kind
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn recovery_refs(&self) -> &[String] {
        &self.recovery_refs
    }
}

/// One caller-selected section. Constructors preserve anchors and bindings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DecisionViewSection {
    kind: DecisionViewSectionKind,
    payload: Vec<u8>,
    tokenizer_identity_digest: Option<DigestV1>,
    tool_schema_digest: Option<DigestV1>,
    source_root_digest: Option<DigestV1>,
    model_profile_digest: Option<DigestV1>,
}

impl DecisionViewSection {
    pub fn stable_system_tool_contract(map: &ExactTokenMap) -> Result<Self, DecisionViewError> {
        Self::from_map(DecisionViewSectionKind::StableSystemToolContract, map)
    }

    pub fn stable_typed_effect_schema(map: &ExactTokenMap) -> Result<Self, DecisionViewError> {
        Self::from_map(DecisionViewSectionKind::StableTypedEffectSchema, map)
    }

    pub fn stable_project_capsule(capsule: &ModelCapsule) -> Result<Self, DecisionViewError> {
        Self::from_capsule(DecisionViewSectionKind::StableProjectCapsule, capsule)
    }

    pub fn stable_task_family_capsule(capsule: &ModelCapsule) -> Result<Self, DecisionViewError> {
        Self::from_capsule(DecisionViewSectionKind::StableTaskFamilyCapsule, capsule)
    }

    pub fn volatile_locus_evidence(page: &TokenPage) -> Result<Self, DecisionViewError> {
        let mut payload = b"TOKENZERO-LOCUS-EVIDENCE-V1\n".to_vec();
        put_record(
            &mut payload,
            "source_anchor",
            page.source_anchor().as_bytes(),
        )?;
        put_record(
            &mut payload,
            "tokenizer_identity",
            page.tokenizer_identity_digest().to_hex().as_bytes(),
        )?;
        put_record(
            &mut payload,
            "token_map",
            page.map_digest().to_hex().as_bytes(),
        )?;
        let token_range = page.token_range();
        let byte_range = page.byte_range();
        payload.extend_from_slice(
            format!(
                "token_range {} {}\nbyte_range {} {}\n",
                token_range.start, token_range.end, byte_range.start, byte_range.end
            )
            .as_bytes(),
        );
        put_record(&mut payload, "exact_bytes", &page.expand())?;
        Ok(Self {
            kind: DecisionViewSectionKind::VolatileLocusEvidence,
            payload,
            tokenizer_identity_digest: Some(page.tokenizer_identity_digest()),
            tool_schema_digest: None,
            source_root_digest: None,
            model_profile_digest: None,
        })
    }

    pub fn volatile_working_tree_delta(map: &ExactTokenMap) -> Result<Self, DecisionViewError> {
        Self::from_map(DecisionViewSectionKind::VolatileWorkingTreeDelta, map)
    }

    pub fn volatile_user_task(map: &ExactTokenMap) -> Result<Self, DecisionViewError> {
        Self::from_map(DecisionViewSectionKind::VolatileUserTask, map)
    }

    pub fn volatile_uncertainty_coverage(
        marker: &DecisionUncertaintyMarker,
    ) -> Result<Self, DecisionViewError> {
        let mut payload = b"TOKENZERO-UNCERTAINTY-MARKER-V1\n".to_vec();
        payload.extend_from_slice(format!("kind {}\n", marker.kind.as_str()).as_bytes());
        put_record(&mut payload, "code", marker.code.as_bytes())?;
        put_record(&mut payload, "message", marker.message.as_bytes())?;
        put_refs(&mut payload, &marker.recovery_refs)?;
        Ok(Self {
            kind: DecisionViewSectionKind::VolatileUncertaintyCoverage,
            payload,
            tokenizer_identity_digest: None,
            tool_schema_digest: None,
            source_root_digest: None,
            model_profile_digest: None,
        })
    }

    pub fn volatile_recovery_routes(refs: Vec<String>) -> Result<Self, DecisionViewError> {
        validate_refs(&refs)?;
        let mut payload = b"TOKENZERO-RECOVERY-ROUTES-V1\n".to_vec();
        put_refs(&mut payload, &refs)?;
        Ok(Self {
            kind: DecisionViewSectionKind::VolatileRecoveryRoutes,
            payload,
            tokenizer_identity_digest: None,
            tool_schema_digest: None,
            source_root_digest: None,
            model_profile_digest: None,
        })
    }

    pub const fn kind(&self) -> DecisionViewSectionKind {
        self.kind
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    fn from_map(
        kind: DecisionViewSectionKind,
        map: &ExactTokenMap,
    ) -> Result<Self, DecisionViewError> {
        let byte_len = map.byte_len();
        if byte_len > MAX_DECISION_VIEW_BYTES {
            return Err(DecisionViewError::ViewByteLimit {
                actual: byte_len,
                limit: MAX_DECISION_VIEW_BYTES,
            });
        }
        Ok(Self {
            kind,
            payload: map.reconstruct(),
            tokenizer_identity_digest: Some(map.tokenizer_identity_digest()),
            tool_schema_digest: (kind == DecisionViewSectionKind::StableTypedEffectSchema)
                .then_some(map.source_digest()),
            source_root_digest: None,
            model_profile_digest: None,
        })
    }

    fn from_capsule(
        kind: DecisionViewSectionKind,
        capsule: &ModelCapsule,
    ) -> Result<Self, DecisionViewError> {
        let mut payload = b"TOKENZERO-MODEL-CAPSULE-SECTION-V1\n".to_vec();
        put_record(
            &mut payload,
            "source_root",
            capsule.source_root_digest().to_hex().as_bytes(),
        )?;
        put_record(
            &mut payload,
            "model_profile",
            capsule.model_profile_digest().to_hex().as_bytes(),
        )?;
        put_record(
            &mut payload,
            "tokenizer_identity",
            capsule.tokenizer_identity_digest().to_hex().as_bytes(),
        )?;
        put_refs(&mut payload, capsule.evidence_refs())?;
        put_digests(&mut payload, capsule.token_page_digests())?;
        put_record(&mut payload, "rendered_capsule", &capsule.render())?;
        Ok(Self {
            kind,
            payload,
            tokenizer_identity_digest: Some(capsule.tokenizer_identity_digest()),
            tool_schema_digest: None,
            source_root_digest: Some(capsule.source_root_digest()),
            model_profile_digest: Some(capsule.model_profile_digest()),
        })
    }
}

/// Complete identity tuple for canonical stable-prefix bytes P(Z,M,T,R).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DecisionViewIdentity {
    source_root_digest: DigestV1,
    model_profile_digest: DigestV1,
    tokenizer_identity_digest: DigestV1,
    tool_schema_digest: DigestV1,
    renderer_contract_digest: DigestV1,
}

impl DecisionViewIdentity {
    pub fn new(
        source_root_digest: DigestV1,
        model_profile_digest: DigestV1,
        tokenizer: &ExactTokenizerIdentity,
        tool_schema_digest: DigestV1,
    ) -> Self {
        Self {
            source_root_digest,
            model_profile_digest,
            tokenizer_identity_digest: tokenizer.digest(),
            tool_schema_digest,
            renderer_contract_digest: decision_view_renderer_contract_digest(),
        }
    }

    pub const fn source_root_digest(&self) -> DigestV1 {
        self.source_root_digest
    }

    pub const fn model_profile_digest(&self) -> DigestV1 {
        self.model_profile_digest
    }

    pub const fn tokenizer_identity_digest(&self) -> DigestV1 {
        self.tokenizer_identity_digest
    }

    pub const fn tool_schema_digest(&self) -> DigestV1 {
        self.tool_schema_digest
    }

    pub const fn renderer_contract_digest(&self) -> DigestV1 {
        self.renderer_contract_digest
    }
}

/// Exact comparison of logical prefix identity only.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefixComparison {
    PrefixIdentical,
    IdentityChanged,
    PrefixBytesChanged,
}

/// Provider-neutral stable-prefix geometry. It contains no hit/eligibility flag.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StablePrefixGeometry {
    identity: DecisionViewIdentity,
    bytes: Vec<u8>,
    bytes_digest: DigestV1,
    breakpoint_after_bytes: u64,
    breakpoint_after_tokens: u64,
    geometry_digest: DigestV1,
}

impl StablePrefixGeometry {
    fn new(
        identity: DecisionViewIdentity,
        bytes: Vec<u8>,
        breakpoint_after_tokens: u64,
    ) -> Result<Self, DecisionViewError> {
        let bytes_digest = digest(&bytes);
        let breakpoint_after_bytes =
            u64::try_from(bytes.len()).map_err(|_| DecisionViewError::LengthOverflow)?;
        let geometry_digest = stable_prefix_geometry_digest(
            &identity,
            bytes_digest,
            breakpoint_after_bytes,
            breakpoint_after_tokens,
            &bytes,
        )?;
        Ok(Self {
            identity,
            bytes,
            bytes_digest,
            breakpoint_after_bytes,
            breakpoint_after_tokens,
            geometry_digest,
        })
    }

    pub fn identity(&self) -> &DecisionViewIdentity {
        &self.identity
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub const fn bytes_digest(&self) -> DigestV1 {
        self.bytes_digest
    }

    pub const fn breakpoint_after_bytes(&self) -> u64 {
        self.breakpoint_after_bytes
    }

    pub const fn breakpoint_after_tokens(&self) -> u64 {
        self.breakpoint_after_tokens
    }

    pub const fn digest(&self) -> DigestV1 {
        self.geometry_digest
    }

    pub fn compare(&self, other: &Self) -> PrefixComparison {
        if self.identity != other.identity {
            PrefixComparison::IdentityChanged
        } else if self.bytes == other.bytes {
            PrefixComparison::PrefixIdentical
        } else {
            PrefixComparison::PrefixBytesChanged
        }
    }
}

/// Deterministic rendering of one ordered, caller-selected Decision View.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DecisionView {
    identity: DecisionViewIdentity,
    section_kinds: Vec<DecisionViewSectionKind>,
    rendered: Vec<u8>,
    exact_token_map_digest: DigestV1,
    total_tokens: u64,
    volatile_bytes: u64,
    volatile_tokens: u64,
    stable_prefix: StablePrefixGeometry,
    digest: DigestV1,
}

impl DecisionView {
    /// Render the supplied sections without selecting, sorting, or dropping any.
    pub fn render<T: ExactTokenizerAdapter + ?Sized>(
        tokenizer: &T,
        identity: DecisionViewIdentity,
        sections: Vec<DecisionViewSection>,
    ) -> Result<Self, DecisionViewError> {
        if sections.len() > MAX_DECISION_VIEW_SECTIONS {
            return Err(DecisionViewError::TooManySections {
                actual: sections.len(),
                limit: MAX_DECISION_VIEW_SECTIONS,
            });
        }
        if tokenizer.identity().digest() != identity.tokenizer_identity_digest {
            return Err(DecisionViewError::TokenizerIdentityMismatch {
                section: DecisionViewSectionKind::StableSystemToolContract,
            });
        }

        let mut saw_volatile = false;
        let mut rendered = b"TOKENZERO-DECISION-VIEW-V1\n".to_vec();
        let mut stable_boundary = rendered.len();
        let mut section_kinds = Vec::with_capacity(sections.len());
        for (index, section) in sections.iter().enumerate() {
            if section.kind.is_stable() {
                if saw_volatile {
                    return Err(DecisionViewError::StableSectionAfterVolatile { index });
                }
            } else {
                saw_volatile = true;
            }
            if section
                .tokenizer_identity_digest
                .is_some_and(|digest| digest != identity.tokenizer_identity_digest)
            {
                return Err(DecisionViewError::TokenizerIdentityMismatch {
                    section: section.kind,
                });
            }
            if section
                .tool_schema_digest
                .is_some_and(|digest| digest != identity.tool_schema_digest)
            {
                return Err(DecisionViewError::ToolSchemaDigestMismatch);
            }
            if section
                .source_root_digest
                .is_some_and(|digest| digest != identity.source_root_digest)
            {
                return Err(DecisionViewError::CapsuleSourceRootMismatch);
            }
            if section
                .model_profile_digest
                .is_some_and(|digest| digest != identity.model_profile_digest)
            {
                return Err(DecisionViewError::CapsuleModelProfileMismatch);
            }
            append_section(&mut rendered, section)?;
            if section.kind.is_stable() {
                stable_boundary = rendered.len();
            }
            if rendered.len() > MAX_DECISION_VIEW_BYTES {
                return Err(DecisionViewError::ViewByteLimit {
                    actual: rendered.len(),
                    limit: MAX_DECISION_VIEW_BYTES,
                });
            }
            section_kinds.push(section.kind);
        }

        let token_map = ExactTokenMap::tokenize(tokenizer, &rendered)?;
        let stable_boundary_u64 =
            u64::try_from(stable_boundary).map_err(|_| DecisionViewError::LengthOverflow)?;
        let stable_token_range = token_map
            .token_range_for_bytes(0..stable_boundary_u64)
            .map_err(|error| match error {
                ModelArtifactError::TokenBoundaryRequired { byte_offset } => {
                    DecisionViewError::PrefixNotTokenAligned { byte_offset }
                }
                other => DecisionViewError::ModelArtifact(other),
            })?;
        let breakpoint_after_tokens =
            u64::try_from(stable_token_range.end).map_err(|_| DecisionViewError::LengthOverflow)?;
        let total_tokens = u64::try_from(token_map.token_count())
            .map_err(|_| DecisionViewError::LengthOverflow)?;
        let volatile_tokens = total_tokens
            .checked_sub(breakpoint_after_tokens)
            .ok_or(DecisionViewError::LengthOverflow)?;
        let volatile_bytes = u64::try_from(rendered.len() - stable_boundary)
            .map_err(|_| DecisionViewError::LengthOverflow)?;
        let stable_prefix = StablePrefixGeometry::new(
            identity.clone(),
            rendered[..stable_boundary].to_vec(),
            breakpoint_after_tokens,
        )?;
        let exact_token_map_digest = token_map.digest();
        let view_digest = decision_view_digest(
            stable_prefix.digest(),
            exact_token_map_digest,
            &section_kinds,
            total_tokens,
            volatile_tokens,
            &rendered,
        )?;
        Ok(Self {
            identity,
            section_kinds,
            rendered,
            exact_token_map_digest,
            total_tokens,
            volatile_bytes,
            volatile_tokens,
            stable_prefix,
            digest: view_digest,
        })
    }

    pub fn identity(&self) -> &DecisionViewIdentity {
        &self.identity
    }

    pub fn section_kinds(&self) -> &[DecisionViewSectionKind] {
        &self.section_kinds
    }

    pub fn rendered(&self) -> &[u8] {
        &self.rendered
    }

    pub const fn exact_token_map_digest(&self) -> DigestV1 {
        self.exact_token_map_digest
    }

    pub const fn total_tokens(&self) -> u64 {
        self.total_tokens
    }

    pub const fn volatile_bytes(&self) -> u64 {
        self.volatile_bytes
    }

    pub const fn volatile_tokens(&self) -> u64 {
        self.volatile_tokens
    }

    pub fn stable_prefix(&self) -> &StablePrefixGeometry {
        &self.stable_prefix
    }

    pub const fn digest(&self) -> DigestV1 {
        self.digest
    }
}

pub fn decision_view_renderer_contract_digest() -> DigestV1 {
    digest(RENDERER_CONTRACT)
}

fn validate_refs(refs: &[String]) -> Result<(), DecisionViewError> {
    if refs.len() > MAX_DECISION_VIEW_RECOVERY_REFS {
        return Err(DecisionViewError::TooManyRecoveryRefs {
            actual: refs.len(),
            limit: MAX_DECISION_VIEW_RECOVERY_REFS,
        });
    }
    for reference in refs {
        let parsed = ZeroRefV1::parse(reference)
            .map_err(|error| DecisionViewError::InvalidRecoveryRef(error.to_string()))?;
        if parsed.to_string() != *reference {
            return Err(DecisionViewError::NoncanonicalRecoveryRef(
                reference.clone(),
            ));
        }
    }
    Ok(())
}

fn put_record(out: &mut Vec<u8>, label: &str, value: &[u8]) -> Result<(), DecisionViewError> {
    let len = u64::try_from(value.len()).map_err(|_| DecisionViewError::LengthOverflow)?;
    let header = format!(
        "{label} {len}
"
    );
    let projected = out
        .len()
        .checked_add(header.len())
        .and_then(|size| size.checked_add(value.len()))
        .and_then(|size| size.checked_add(1))
        .ok_or(DecisionViewError::LengthOverflow)?;
    ensure_view_bound(projected)?;
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(value);
    out.push(b'\n');
    Ok(())
}

fn put_refs(out: &mut Vec<u8>, refs: &[String]) -> Result<(), DecisionViewError> {
    let count = u64::try_from(refs.len()).map_err(|_| DecisionViewError::LengthOverflow)?;
    append_bounded(
        out,
        format!(
            "recovery_refs {count}
"
        )
        .as_bytes(),
    )?;
    for reference in refs {
        put_record(out, "recovery_ref", reference.as_bytes())?;
    }
    Ok(())
}

fn put_digests(out: &mut Vec<u8>, digests: &[DigestV1]) -> Result<(), DecisionViewError> {
    let count = u64::try_from(digests.len()).map_err(|_| DecisionViewError::LengthOverflow)?;
    append_bounded(
        out,
        format!(
            "token_page_digests {count}
"
        )
        .as_bytes(),
    )?;
    for value in digests {
        put_record(out, "token_page_digest", value.to_hex().as_bytes())?;
    }
    Ok(())
}

fn append_section(
    rendered: &mut Vec<u8>,
    section: &DecisionViewSection,
) -> Result<(), DecisionViewError> {
    let len =
        u64::try_from(section.payload.len()).map_err(|_| DecisionViewError::LengthOverflow)?;
    let header = format!(
        "section {} {len}
",
        section.kind.as_str()
    );
    let projected = rendered
        .len()
        .checked_add(header.len())
        .and_then(|size| size.checked_add(section.payload.len()))
        .and_then(|size| size.checked_add(1))
        .ok_or(DecisionViewError::LengthOverflow)?;
    ensure_view_bound(projected)?;
    rendered.extend_from_slice(header.as_bytes());
    rendered.extend_from_slice(&section.payload);
    rendered.push(b'\n');
    Ok(())
}

fn append_bounded(out: &mut Vec<u8>, value: &[u8]) -> Result<(), DecisionViewError> {
    let projected = out
        .len()
        .checked_add(value.len())
        .ok_or(DecisionViewError::LengthOverflow)?;
    ensure_view_bound(projected)?;
    out.extend_from_slice(value);
    Ok(())
}

fn ensure_view_bound(actual: usize) -> Result<(), DecisionViewError> {
    if actual > MAX_DECISION_VIEW_BYTES {
        return Err(DecisionViewError::ViewByteLimit {
            actual,
            limit: MAX_DECISION_VIEW_BYTES,
        });
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> DigestV1 {
    DigestV1::from_bytes(sha256(bytes))
}

fn put_identity(out: &mut Vec<u8>, identity: &DecisionViewIdentity) {
    out.extend_from_slice(identity.source_root_digest.as_bytes());
    out.extend_from_slice(identity.model_profile_digest.as_bytes());
    out.extend_from_slice(identity.tokenizer_identity_digest.as_bytes());
    out.extend_from_slice(identity.tool_schema_digest.as_bytes());
    out.extend_from_slice(identity.renderer_contract_digest.as_bytes());
}

fn stable_prefix_geometry_digest(
    identity: &DecisionViewIdentity,
    bytes_digest: DigestV1,
    byte_count: u64,
    token_count: u64,
    bytes: &[u8],
) -> Result<DigestV1, DecisionViewError> {
    let mut canonical = b"TOKENZERO-STABLE-PREFIX-GEOMETRY-V1".to_vec();
    put_identity(&mut canonical, identity);
    canonical.extend_from_slice(bytes_digest.as_bytes());
    canonical.extend_from_slice(&byte_count.to_be_bytes());
    canonical.extend_from_slice(&token_count.to_be_bytes());
    put_binary(&mut canonical, bytes)?;
    Ok(digest(&canonical))
}

fn decision_view_digest(
    prefix_geometry: DigestV1,
    token_map: DigestV1,
    section_kinds: &[DecisionViewSectionKind],
    total_tokens: u64,
    volatile_tokens: u64,
    rendered: &[u8],
) -> Result<DigestV1, DecisionViewError> {
    let mut canonical = b"TOKENZERO-DECISION-VIEW-IDENTITY-V1".to_vec();
    canonical.extend_from_slice(prefix_geometry.as_bytes());
    canonical.extend_from_slice(token_map.as_bytes());
    canonical.extend_from_slice(
        &u64::try_from(section_kinds.len())
            .map_err(|_| DecisionViewError::LengthOverflow)?
            .to_be_bytes(),
    );
    for kind in section_kinds {
        put_binary(&mut canonical, kind.as_str().as_bytes())?;
    }
    canonical.extend_from_slice(&total_tokens.to_be_bytes());
    canonical.extend_from_slice(&volatile_tokens.to_be_bytes());
    put_binary(&mut canonical, rendered)?;
    Ok(digest(&canonical))
}

fn put_binary(out: &mut Vec<u8>, value: &[u8]) -> Result<(), DecisionViewError> {
    out.extend_from_slice(
        &u64::try_from(value.len())
            .map_err(|_| DecisionViewError::LengthOverflow)?
            .to_be_bytes(),
    );
    out.extend_from_slice(value);
    Ok(())
}

#[cfg(test)]
mod tests {
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
            &[page.clone()],
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
            DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, b"fix the task"))
                .unwrap(),
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
            DecisionViewSection::stable_system_tool_contract(&byte_map(&tokenizer, b"stable"))
                .unwrap(),
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
                    DecisionViewSection::stable_system_tool_contract(&byte_map(
                        &tokenizer, &stable,
                    ))
                    .unwrap(),
                    DecisionViewSection::volatile_user_task(&byte_map(&tokenizer, b"tail"))
                        .unwrap(),
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
}
