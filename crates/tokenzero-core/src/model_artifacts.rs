//! Provider-locked tokenizer identities and exact model-facing artifacts.
//!
//! TokenZero owns byte/token correspondence, token pages, and model capsules.
//! The hub remains the authority for the provider lock (`zero-gauge`) and the
//! ledger tokenizer gauge (`zero-ledger`). Every derived digest below includes
//! the exact provider/model/tokenizer-revision identity.

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};
use std::error::Error;
use std::fmt;
use std::ops::Range;
use zero_abi::{DigestV1, sha256};
use zero_gauge::ProviderLock;
use zero_ledger::{Digest as LedgerDigest, TokenizerIdentity as LedgerTokenizerIdentity};
use zero_ref::{ZeroFragment, ZeroRefV1};

/// Maximum encoded tokens expanded by one page.
pub const MAX_TOKEN_PAGE_TOKENS: usize = 4_096;
/// Maximum exact bytes expanded by one page.
pub const MAX_TOKEN_PAGE_BYTES: usize = 1_048_576;
/// Maximum evidence references bound into one model capsule.
pub const MAX_CAPSULE_EVIDENCE_REFS: usize = 4_096;
/// Maximum token pages bound into one model capsule.
pub const MAX_CAPSULE_TOKEN_PAGES: usize = 4_096;
/// Maximum stable-prefix plus dynamic-tail bytes carried by one capsule.
pub const MAX_CAPSULE_RENDER_BYTES: usize = 16 * 1_048_576;
const MAX_IDENTITY_FIELD_BYTES: usize = 1_024;

/// Typed refusal for malformed or identity-inconsistent model artifacts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelArtifactError {
    EmptyProvider,
    EmptyModel,
    IdentityFieldTooLong,
    InvalidTokenizerRevisionDigest,
    TokenizerRevisionDigestMismatch {
        expected: String,
        actual: String,
    },
    IdentityDigestMismatch,
    TokenizerAdapter(String),
    EmptyTokenBytes {
        token_index: usize,
    },
    TokenBytesMismatch {
        token_index: usize,
        byte_offset: usize,
    },
    TokenBytesLengthMismatch {
        expected: usize,
        actual: usize,
    },
    LengthOverflow,
    InvalidTokenRange {
        start: usize,
        end: usize,
        token_count: usize,
    },
    TokenBoundaryRequired {
        byte_offset: u64,
    },
    InvalidSourceAnchor(String),
    NoncanonicalSourceAnchor,
    SourceAnchorMustBeWholeBlob,
    SourceDigestMismatch {
        expected: String,
        actual: String,
    },
    EmptyTokenPage,
    TokenPageTokenLimit {
        actual: usize,
        limit: usize,
    },
    TokenPageByteLimit {
        actual: usize,
        limit: usize,
    },
    TokenizerIdentityMismatch,
    CapsuleEvidenceLimit {
        actual: usize,
        limit: usize,
    },
    CapsulePageLimit {
        actual: usize,
        limit: usize,
    },
    CapsuleRenderByteLimit {
        actual: usize,
        limit: usize,
    },
    InvalidEvidenceRef(String),
    NoncanonicalEvidenceRef(String),
}

impl fmt::Display for ModelArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid TokenZero model artifact: {self:?}")
    }
}

impl Error for ModelArtifactError {}

/// Exact provider/model/tokenizer-revision identity.
///
/// [`ExactTokenizerIdentity::new`] checks the supplied revision manifest bytes
/// against the lowercase SHA-256 digest in the canonical zero-gauge lock. The
/// identity digest also binds provider and model, so equal revision files under
/// different model locks do not alias.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactTokenizerIdentity {
    provider_lock: ProviderLock,
    identity_digest: DigestV1,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactTokenizerIdentityWire {
    provider_lock: ProviderLock,
    identity_digest: DigestV1,
}

impl Serialize for ExactTokenizerIdentity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ExactTokenizerIdentityWire {
            provider_lock: self.provider_lock.clone(),
            identity_digest: self.identity_digest,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExactTokenizerIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ExactTokenizerIdentityWire::deserialize(deserializer)?;
        validate_provider_lock(&wire.provider_lock).map_err(de::Error::custom)?;
        let expected = tokenizer_identity_digest(&wire.provider_lock).map_err(de::Error::custom)?;
        if wire.identity_digest != expected {
            return Err(de::Error::custom(
                ModelArtifactError::IdentityDigestMismatch,
            ));
        }
        Ok(Self {
            provider_lock: wire.provider_lock,
            identity_digest: wire.identity_digest,
        })
    }
}

impl ExactTokenizerIdentity {
    /// Build an identity only after verifying the exact revision manifest.
    pub fn new(
        provider_lock: ProviderLock,
        tokenizer_revision_manifest: &[u8],
    ) -> Result<Self, ModelArtifactError> {
        validate_provider_lock(&provider_lock)?;
        let actual = digest(tokenizer_revision_manifest).to_hex();
        if actual != provider_lock.tokenizer_revision_digest {
            return Err(ModelArtifactError::TokenizerRevisionDigestMismatch {
                expected: provider_lock.tokenizer_revision_digest,
                actual,
            });
        }
        let identity_digest = tokenizer_identity_digest(&provider_lock)?;
        Ok(Self {
            provider_lock,
            identity_digest,
        })
    }

    /// Canonical zero-gauge provider lock used for this identity.
    pub fn provider_lock(&self) -> &ProviderLock {
        &self.provider_lock
    }

    /// Digest of provider, model, and exact tokenizer revision.
    pub const fn digest(&self) -> DigestV1 {
        self.identity_digest
    }

    /// Canonical zero-ledger gauge identity for receipt accounting.
    pub fn ledger_identity(&self) -> LedgerTokenizerIdentity {
        let revision = LedgerDigest::from_hex(&self.provider_lock.tokenizer_revision_digest)
            .expect("validated tokenizer revision digest");
        LedgerTokenizerIdentity::new(
            format!(
                "{}/{}@{}",
                self.provider_lock.provider, self.provider_lock.model, self.identity_digest
            ),
            revision,
        )
    }
}

/// One tokenizer output token and its exact decoded bytes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TokenPiece {
    pub token_id: u32,
    pub bytes: Vec<u8>,
}

impl TokenPiece {
    pub fn new(token_id: u32, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            token_id,
            bytes: bytes.into(),
        }
    }
}

/// Runtime authority for one exact provider-locked tokenizer revision.
///
/// `encode` supplies the provider token ids. `token_bytes` independently maps
/// each id back to its canonical bytes. [`ExactTokenMap::tokenize`] refuses the
/// result unless those bytes reconstruct the complete input exactly.
pub trait ExactTokenizerAdapter {
    fn identity(&self) -> &ExactTokenizerIdentity;
    fn encode(&self, source: &[u8]) -> Result<Vec<u32>, String>;
    fn token_bytes(&self, token_id: u32) -> Result<Vec<u8>, String>;
}

/// Complete exact byte-to-token and token-to-byte correspondence.
///
/// Construction compares every decoded token byte against the original byte
/// stream. No estimate or token-count-only adapter can construct this type.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExactTokenMap {
    tokenizer_identity_digest: DigestV1,
    source_digest: DigestV1,
    tokens: Vec<TokenPiece>,
    digest: DigestV1,
}

impl ExactTokenMap {
    /// Tokenize bytes through the exact provider adapter and verify round-trip bytes.
    pub fn tokenize<T: ExactTokenizerAdapter + ?Sized>(
        tokenizer: &T,
        source: &[u8],
    ) -> Result<Self, ModelArtifactError> {
        let token_ids = tokenizer
            .encode(source)
            .map_err(ModelArtifactError::TokenizerAdapter)?;
        let mut tokens = Vec::with_capacity(token_ids.len());
        for token_id in token_ids {
            let bytes = tokenizer
                .token_bytes(token_id)
                .map_err(ModelArtifactError::TokenizerAdapter)?;
            tokens.push(TokenPiece::new(token_id, bytes));
        }
        Self::from_token_pieces(tokenizer.identity(), source, tokens)
    }

    fn from_token_pieces(
        tokenizer: &ExactTokenizerIdentity,
        source: &[u8],
        tokens: Vec<TokenPiece>,
    ) -> Result<Self, ModelArtifactError> {
        let mut cursor = 0usize;
        for (token_index, token) in tokens.iter().enumerate() {
            if token.bytes.is_empty() {
                return Err(ModelArtifactError::EmptyTokenBytes { token_index });
            }
            let end = cursor
                .checked_add(token.bytes.len())
                .ok_or(ModelArtifactError::LengthOverflow)?;
            if source.get(cursor..end) != Some(token.bytes.as_slice()) {
                return Err(ModelArtifactError::TokenBytesMismatch {
                    token_index,
                    byte_offset: cursor,
                });
            }
            cursor = end;
        }
        if cursor != source.len() {
            return Err(ModelArtifactError::TokenBytesLengthMismatch {
                expected: source.len(),
                actual: cursor,
            });
        }

        let source_digest = digest(source);
        let map_digest = token_map_digest(tokenizer.digest(), source_digest, &tokens)?;
        Ok(Self {
            tokenizer_identity_digest: tokenizer.digest(),
            source_digest,
            tokens,
            digest: map_digest,
        })
    }

    pub const fn tokenizer_identity_digest(&self) -> DigestV1 {
        self.tokenizer_identity_digest
    }

    pub const fn source_digest(&self) -> DigestV1 {
        self.source_digest
    }

    pub const fn digest(&self) -> DigestV1 {
        self.digest
    }

    pub fn tokens(&self) -> &[TokenPiece] {
        &self.tokens
    }

    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }

    pub fn byte_len(&self) -> usize {
        self.tokens.iter().map(|token| token.bytes.len()).sum()
    }

    /// Reconstruct the exact original byte stream.
    pub fn reconstruct(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.byte_len());
        for token in &self.tokens {
            bytes.extend_from_slice(&token.bytes);
        }
        bytes
    }

    /// Exact byte range for a half-open token range.
    pub fn byte_range_for_tokens(
        &self,
        range: Range<usize>,
    ) -> Result<Range<u64>, ModelArtifactError> {
        if range.start > range.end || range.end > self.tokens.len() {
            return Err(ModelArtifactError::InvalidTokenRange {
                start: range.start,
                end: range.end,
                token_count: self.tokens.len(),
            });
        }
        let start = self.tokens[..range.start]
            .iter()
            .try_fold(0usize, |total, token| total.checked_add(token.bytes.len()))
            .ok_or(ModelArtifactError::LengthOverflow)?;
        let len = self.tokens[range.clone()]
            .iter()
            .try_fold(0usize, |total, token| total.checked_add(token.bytes.len()))
            .ok_or(ModelArtifactError::LengthOverflow)?;
        let end = start
            .checked_add(len)
            .ok_or(ModelArtifactError::LengthOverflow)?;
        Ok(
            u64::try_from(start).map_err(|_| ModelArtifactError::LengthOverflow)?
                ..u64::try_from(end).map_err(|_| ModelArtifactError::LengthOverflow)?,
        )
    }

    /// Exact half-open token range for byte offsets that lie on token boundaries.
    pub fn token_range_for_bytes(
        &self,
        range: Range<u64>,
    ) -> Result<Range<usize>, ModelArtifactError> {
        if range.start > range.end {
            return Err(ModelArtifactError::TokenBoundaryRequired {
                byte_offset: range.start,
            });
        }
        let start = self.token_boundary(range.start)?;
        let end = self.token_boundary(range.end)?;
        Ok(start..end)
    }

    fn token_boundary(&self, byte_offset: u64) -> Result<usize, ModelArtifactError> {
        let mut cursor = 0u64;
        if byte_offset == 0 {
            return Ok(0);
        }
        for (index, token) in self.tokens.iter().enumerate() {
            cursor = cursor
                .checked_add(
                    u64::try_from(token.bytes.len())
                        .map_err(|_| ModelArtifactError::LengthOverflow)?,
                )
                .ok_or(ModelArtifactError::LengthOverflow)?;
            if cursor == byte_offset {
                return Ok(index + 1);
            }
            if cursor > byte_offset {
                break;
            }
        }
        Err(ModelArtifactError::TokenBoundaryRequired { byte_offset })
    }
}

/// Bounded, source-anchored slice of an exact token map.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TokenPage {
    tokenizer_identity_digest: DigestV1,
    map_digest: DigestV1,
    source_anchor: String,
    token_start: u64,
    token_end: u64,
    byte_start: u64,
    byte_end: u64,
    tokens: Vec<TokenPiece>,
    digest: DigestV1,
}

impl TokenPage {
    pub fn new(
        map: &ExactTokenMap,
        source_anchor: &str,
        token_range: Range<usize>,
    ) -> Result<Self, ModelArtifactError> {
        let parsed = ZeroRefV1::parse(source_anchor)
            .map_err(|error| ModelArtifactError::InvalidSourceAnchor(error.to_string()))?;
        if parsed.to_string() != source_anchor {
            return Err(ModelArtifactError::NoncanonicalSourceAnchor);
        }
        if parsed.fragment != ZeroFragment::None {
            return Err(ModelArtifactError::SourceAnchorMustBeWholeBlob);
        }
        let expected = map.source_digest.to_hex();
        if parsed.hash != expected {
            return Err(ModelArtifactError::SourceDigestMismatch {
                expected,
                actual: parsed.hash,
            });
        }
        if token_range.start == token_range.end {
            return Err(ModelArtifactError::EmptyTokenPage);
        }
        let byte_range = map.byte_range_for_tokens(token_range.clone())?;
        let token_count = token_range.end.saturating_sub(token_range.start);
        if token_count > MAX_TOKEN_PAGE_TOKENS {
            return Err(ModelArtifactError::TokenPageTokenLimit {
                actual: token_count,
                limit: MAX_TOKEN_PAGE_TOKENS,
            });
        }
        let byte_count = usize::try_from(byte_range.end - byte_range.start)
            .map_err(|_| ModelArtifactError::LengthOverflow)?;
        if byte_count > MAX_TOKEN_PAGE_BYTES {
            return Err(ModelArtifactError::TokenPageByteLimit {
                actual: byte_count,
                limit: MAX_TOKEN_PAGE_BYTES,
            });
        }
        let tokens = map.tokens[token_range.clone()].to_vec();
        let token_start =
            u64::try_from(token_range.start).map_err(|_| ModelArtifactError::LengthOverflow)?;
        let token_end =
            u64::try_from(token_range.end).map_err(|_| ModelArtifactError::LengthOverflow)?;
        let page_digest = token_page_digest(
            map.tokenizer_identity_digest,
            map.digest,
            source_anchor,
            token_start,
            token_end,
            byte_range.start,
            byte_range.end,
            &tokens,
        )?;
        Ok(Self {
            tokenizer_identity_digest: map.tokenizer_identity_digest,
            map_digest: map.digest,
            source_anchor: source_anchor.to_string(),
            token_start,
            token_end,
            byte_start: byte_range.start,
            byte_end: byte_range.end,
            tokens,
            digest: page_digest,
        })
    }

    pub const fn tokenizer_identity_digest(&self) -> DigestV1 {
        self.tokenizer_identity_digest
    }

    pub const fn map_digest(&self) -> DigestV1 {
        self.map_digest
    }

    pub const fn digest(&self) -> DigestV1 {
        self.digest
    }

    pub fn source_anchor(&self) -> &str {
        &self.source_anchor
    }

    pub const fn token_range(&self) -> Range<u64> {
        self.token_start..self.token_end
    }

    pub const fn byte_range(&self) -> Range<u64> {
        self.byte_start..self.byte_end
    }

    pub fn tokens(&self) -> &[TokenPiece] {
        &self.tokens
    }

    /// Expand the page to its exact source bytes, bounded by the page limits.
    pub fn expand(&self) -> Vec<u8> {
        let capacity = usize::try_from(self.byte_end - self.byte_start).unwrap_or(0);
        let mut bytes = Vec::with_capacity(capacity);
        for token in &self.tokens {
            bytes.extend_from_slice(&token.bytes);
        }
        bytes
    }
}

/// Canonical model-facing capsule: exact prefix/tail bytes plus evidence and pages.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelCapsule {
    source_root_digest: DigestV1,
    model_profile_digest: DigestV1,
    tokenizer_identity_digest: DigestV1,
    evidence_refs: Vec<String>,
    token_page_digests: Vec<DigestV1>,
    stable_prefix: Vec<u8>,
    dynamic_tail: Vec<u8>,
    stable_prefix_tokens: u64,
    dynamic_tail_tokens: u64,
    digest: DigestV1,
}

impl ModelCapsule {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_root_digest: DigestV1,
        model_profile_digest: DigestV1,
        tokenizer: &ExactTokenizerIdentity,
        mut evidence_refs: Vec<String>,
        token_pages: &[TokenPage],
        stable_prefix: &ExactTokenMap,
        dynamic_tail: &ExactTokenMap,
    ) -> Result<Self, ModelArtifactError> {
        if stable_prefix.tokenizer_identity_digest != tokenizer.digest()
            || dynamic_tail.tokenizer_identity_digest != tokenizer.digest()
            || token_pages
                .iter()
                .any(|page| page.tokenizer_identity_digest != tokenizer.digest())
        {
            return Err(ModelArtifactError::TokenizerIdentityMismatch);
        }
        if evidence_refs.len() > MAX_CAPSULE_EVIDENCE_REFS {
            return Err(ModelArtifactError::CapsuleEvidenceLimit {
                actual: evidence_refs.len(),
                limit: MAX_CAPSULE_EVIDENCE_REFS,
            });
        }
        for reference in &evidence_refs {
            let parsed = ZeroRefV1::parse(reference)
                .map_err(|error| ModelArtifactError::InvalidEvidenceRef(error.to_string()))?;
            if parsed.to_string() != *reference {
                return Err(ModelArtifactError::NoncanonicalEvidenceRef(
                    reference.clone(),
                ));
            }
        }
        evidence_refs.sort();
        evidence_refs.dedup();

        if token_pages.len() > MAX_CAPSULE_TOKEN_PAGES {
            return Err(ModelArtifactError::CapsulePageLimit {
                actual: token_pages.len(),
                limit: MAX_CAPSULE_TOKEN_PAGES,
            });
        }
        let mut token_page_digests: Vec<_> = token_pages.iter().map(TokenPage::digest).collect();
        token_page_digests.sort();
        token_page_digests.dedup();

        // Token counts come from the already validated maps, never from bytes.
        let stable_prefix_tokens = u64::try_from(stable_prefix.token_count())
            .map_err(|_| ModelArtifactError::LengthOverflow)?;
        let dynamic_tail_tokens = u64::try_from(dynamic_tail.token_count())
            .map_err(|_| ModelArtifactError::LengthOverflow)?;
        let stable_prefix_map_digest = stable_prefix.digest();
        let dynamic_tail_map_digest = dynamic_tail.digest();
        let stable_prefix = stable_prefix.reconstruct();
        let dynamic_tail = dynamic_tail.reconstruct();
        let render_bytes = stable_prefix
            .len()
            .checked_add(dynamic_tail.len())
            .ok_or(ModelArtifactError::LengthOverflow)?;
        if render_bytes > MAX_CAPSULE_RENDER_BYTES {
            return Err(ModelArtifactError::CapsuleRenderByteLimit {
                actual: render_bytes,
                limit: MAX_CAPSULE_RENDER_BYTES,
            });
        }
        let capsule_digest = model_capsule_digest(
            source_root_digest,
            model_profile_digest,
            tokenizer.digest(),
            &evidence_refs,
            &token_page_digests,
            stable_prefix_map_digest,
            dynamic_tail_map_digest,
            &stable_prefix,
            &dynamic_tail,
            stable_prefix_tokens,
            dynamic_tail_tokens,
        )?;
        Ok(Self {
            source_root_digest,
            model_profile_digest,
            tokenizer_identity_digest: tokenizer.digest(),
            evidence_refs,
            token_page_digests,
            stable_prefix,
            dynamic_tail,
            stable_prefix_tokens,
            dynamic_tail_tokens,
            digest: capsule_digest,
        })
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

    pub fn evidence_refs(&self) -> &[String] {
        &self.evidence_refs
    }

    pub fn token_page_digests(&self) -> &[DigestV1] {
        &self.token_page_digests
    }

    pub fn stable_prefix(&self) -> &[u8] {
        &self.stable_prefix
    }

    pub fn dynamic_tail(&self) -> &[u8] {
        &self.dynamic_tail
    }

    pub const fn stable_prefix_tokens(&self) -> u64 {
        self.stable_prefix_tokens
    }

    pub const fn dynamic_tail_tokens(&self) -> u64 {
        self.dynamic_tail_tokens
    }

    pub const fn total_tokens(&self) -> u64 {
        self.stable_prefix_tokens + self.dynamic_tail_tokens
    }

    pub fn render(&self) -> Vec<u8> {
        let mut rendered = Vec::with_capacity(self.stable_prefix.len() + self.dynamic_tail.len());
        rendered.extend_from_slice(&self.stable_prefix);
        rendered.extend_from_slice(&self.dynamic_tail);
        rendered
    }

    pub const fn digest(&self) -> DigestV1 {
        self.digest
    }
}

// Domain-separated, length-prefixed canonical hashing helpers.
fn digest(bytes: &[u8]) -> DigestV1 {
    DigestV1::from_bytes(sha256(bytes))
}

fn validate_provider_lock(lock: &ProviderLock) -> Result<(), ModelArtifactError> {
    if lock.provider.is_empty() {
        return Err(ModelArtifactError::EmptyProvider);
    }
    if lock.model.is_empty() {
        return Err(ModelArtifactError::EmptyModel);
    }
    if lock.provider.len() > MAX_IDENTITY_FIELD_BYTES || lock.model.len() > MAX_IDENTITY_FIELD_BYTES
    {
        return Err(ModelArtifactError::IdentityFieldTooLong);
    }
    DigestV1::from_hex(&lock.tokenizer_revision_digest)
        .map_err(|_| ModelArtifactError::InvalidTokenizerRevisionDigest)?;
    Ok(())
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ModelArtifactError> {
    let len = u64::try_from(bytes.len()).map_err(|_| ModelArtifactError::LengthOverflow)?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

fn put_string(out: &mut Vec<u8>, value: &str) -> Result<(), ModelArtifactError> {
    put_bytes(out, value.as_bytes())
}

fn put_tokens(out: &mut Vec<u8>, tokens: &[TokenPiece]) -> Result<(), ModelArtifactError> {
    let count = u64::try_from(tokens.len()).map_err(|_| ModelArtifactError::LengthOverflow)?;
    out.extend_from_slice(&count.to_be_bytes());
    for token in tokens {
        out.extend_from_slice(&token.token_id.to_be_bytes());
        put_bytes(out, &token.bytes)?;
    }
    Ok(())
}

fn tokenizer_identity_digest(lock: &ProviderLock) -> Result<DigestV1, ModelArtifactError> {
    let mut bytes = b"TOKENZERO-EXACT-TOKENIZER-IDENTITY-V1".to_vec();
    put_string(&mut bytes, &lock.provider)?;
    put_string(&mut bytes, &lock.model)?;
    bytes.extend_from_slice(
        DigestV1::from_hex(&lock.tokenizer_revision_digest)
            .map_err(|_| ModelArtifactError::InvalidTokenizerRevisionDigest)?
            .as_bytes(),
    );
    Ok(digest(&bytes))
}

fn token_map_digest(
    tokenizer: DigestV1,
    source: DigestV1,
    tokens: &[TokenPiece],
) -> Result<DigestV1, ModelArtifactError> {
    let mut bytes = b"TOKENZERO-EXACT-TOKEN-MAP-V1".to_vec();
    bytes.extend_from_slice(tokenizer.as_bytes());
    bytes.extend_from_slice(source.as_bytes());
    put_tokens(&mut bytes, tokens)?;
    Ok(digest(&bytes))
}

#[allow(clippy::too_many_arguments)]
fn token_page_digest(
    tokenizer: DigestV1,
    map: DigestV1,
    source_anchor: &str,
    token_start: u64,
    token_end: u64,
    byte_start: u64,
    byte_end: u64,
    tokens: &[TokenPiece],
) -> Result<DigestV1, ModelArtifactError> {
    let mut bytes = b"TOKENZERO-TOKEN-PAGE-V1".to_vec();
    bytes.extend_from_slice(tokenizer.as_bytes());
    bytes.extend_from_slice(map.as_bytes());
    put_string(&mut bytes, source_anchor)?;
    bytes.extend_from_slice(&token_start.to_be_bytes());
    bytes.extend_from_slice(&token_end.to_be_bytes());
    bytes.extend_from_slice(&byte_start.to_be_bytes());
    bytes.extend_from_slice(&byte_end.to_be_bytes());
    put_tokens(&mut bytes, tokens)?;
    Ok(digest(&bytes))
}

#[allow(clippy::too_many_arguments)]
fn model_capsule_digest(
    source_root: DigestV1,
    model_profile: DigestV1,
    tokenizer: DigestV1,
    evidence_refs: &[String],
    page_digests: &[DigestV1],
    stable_prefix_map: DigestV1,
    dynamic_tail_map: DigestV1,
    stable_prefix: &[u8],
    dynamic_tail: &[u8],
    stable_prefix_tokens: u64,
    dynamic_tail_tokens: u64,
) -> Result<DigestV1, ModelArtifactError> {
    let mut bytes = b"TOKENZERO-MODEL-CAPSULE-V1".to_vec();
    bytes.extend_from_slice(source_root.as_bytes());
    bytes.extend_from_slice(model_profile.as_bytes());
    bytes.extend_from_slice(tokenizer.as_bytes());
    bytes.extend_from_slice(
        &u64::try_from(evidence_refs.len())
            .map_err(|_| ModelArtifactError::LengthOverflow)?
            .to_be_bytes(),
    );
    for reference in evidence_refs {
        put_string(&mut bytes, reference)?;
    }
    bytes.extend_from_slice(
        &u64::try_from(page_digests.len())
            .map_err(|_| ModelArtifactError::LengthOverflow)?
            .to_be_bytes(),
    );
    for page in page_digests {
        bytes.extend_from_slice(page.as_bytes());
    }
    bytes.extend_from_slice(stable_prefix_map.as_bytes());
    bytes.extend_from_slice(dynamic_tail_map.as_bytes());
    put_bytes(&mut bytes, stable_prefix)?;
    put_bytes(&mut bytes, dynamic_tail)?;
    bytes.extend_from_slice(&stable_prefix_tokens.to_be_bytes());
    bytes.extend_from_slice(&dynamic_tail_tokens.to_be_bytes());
    Ok(digest(&bytes))
}

#[cfg(test)]
#[path = "../../../tests/core/inline/model_artifacts__tests.rs"]
mod tests;
