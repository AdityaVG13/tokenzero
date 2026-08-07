//! Exact, fieldwise coding receipts for caller-selected output effects.
//!
//! TokenZero does not select an effect or decide which bytes are semantically
//! novel. The caller supplies ordered fields and their classes, plus digests of
//! the selection authority, selected effect, and verification receipt. This
//! module only validates, frames, tokenizes, and accounts those exact bytes.

use crate::model_artifacts::{ExactTokenMap, ExactTokenizerAdapter, ModelArtifactError};
use serde::Serialize;
use std::collections::BTreeSet;
use std::{error::Error, fmt};
use zero_abi::{DigestV1, sha256};

pub const OUTPUT_NOVELTY_SCHEMA_V1: &str = "tokenzero.output-novelty/v1";
pub const MAX_OUTPUT_NOVELTY_FIELDS: usize = 256;
pub const MAX_OUTPUT_NOVELTY_FIELD_NAME_BYTES: usize = 128;
pub const MAX_OUTPUT_NOVELTY_BYTES: usize = 16 * 1_048_576;
const ENCODING_DOMAIN_V1: &[u8] = b"TOKENZERO-OUTPUT-NOVELTY-V1\0";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputNoveltyFieldRoleV1 {
    /// Existing address or selector supplied by the caller.
    Referenced,
    /// Exact output bytes already available under the caller's authority.
    Reused,
    /// Conditional semantic delta classified as novel by the caller.
    Novel,
    /// Mechanically implied operation/framing bytes.
    Deterministic,
}

impl OutputNoveltyFieldRoleV1 {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Referenced => "referenced",
            Self::Reused => "reused",
            Self::Novel => "novel",
            Self::Deterministic => "deterministic",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputSelectionOriginV1 {
    CallerSupplied,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputNoveltyError {
    ModelArtifact(ModelArtifactError),
    ZeroIdentity(&'static str),
    EmptyFields,
    TooManyFields { actual: usize, limit: usize },
    InvalidFieldName(String),
    DuplicateFieldName(String),
    EmptyNovelField(String),
    EncodedByteLimit { actual: usize, limit: usize },
    LengthOverflow,
    AccountingOverflow,
    EncodingMismatch,
}

impl fmt::Display for OutputNoveltyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid TokenZero output novelty coding: {self:?}")
    }
}

impl Error for OutputNoveltyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ModelArtifact(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ModelArtifactError> for OutputNoveltyError {
    fn from(error: ModelArtifactError) -> Self {
        Self::ModelArtifact(error)
    }
}

/// One caller-selected field. Raw bytes remain outside the serializable receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputNoveltyFieldV1 {
    name: String,
    role: OutputNoveltyFieldRoleV1,
    bytes: Vec<u8>,
}

impl OutputNoveltyFieldV1 {
    pub fn new(
        name: impl Into<String>,
        role: OutputNoveltyFieldRoleV1,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<Self, OutputNoveltyError> {
        let name = name.into();
        validate_field_name(&name)?;
        let bytes = bytes.into();
        if role == OutputNoveltyFieldRoleV1::Novel && bytes.is_empty() {
            return Err(OutputNoveltyError::EmptyNovelField(name));
        }
        Ok(Self { name, role, bytes })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub const fn role(&self) -> OutputNoveltyFieldRoleV1 {
        self.role
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OutputNoveltyFieldReceiptV1 {
    name: String,
    role: OutputNoveltyFieldRoleV1,
    payload_digest: DigestV1,
    payload_bytes: u64,
    standalone_payload_tokens: u64,
}

impl OutputNoveltyFieldReceiptV1 {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub const fn role(&self) -> OutputNoveltyFieldRoleV1 {
        self.role
    }
    pub const fn payload_digest(&self) -> DigestV1 {
        self.payload_digest
    }
    pub const fn payload_bytes(&self) -> u64 {
        self.payload_bytes
    }
    pub const fn standalone_payload_tokens(&self) -> u64 {
        self.standalone_payload_tokens
    }
}

/// Role totals count exact field payloads only. They intentionally exclude
/// canonical framing, and token totals are not claimed additive across fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct OutputNoveltyTotalsV1 {
    referenced_fields: u32,
    referenced_payload_bytes: u64,
    referenced_standalone_payload_tokens: u64,
    reused_fields: u32,
    reused_payload_bytes: u64,
    reused_standalone_payload_tokens: u64,
    novel_fields: u32,
    novel_payload_bytes: u64,
    novel_standalone_payload_tokens: u64,
    deterministic_fields: u32,
    deterministic_payload_bytes: u64,
    deterministic_standalone_payload_tokens: u64,
}

impl OutputNoveltyTotalsV1 {
    pub const fn novel_fields(&self) -> u32 {
        self.novel_fields
    }
    pub const fn novel_payload_bytes(&self) -> u64 {
        self.novel_payload_bytes
    }
    pub const fn novel_standalone_payload_tokens(&self) -> u64 {
        self.novel_standalone_payload_tokens
    }
    pub const fn referenced_payload_bytes(&self) -> u64 {
        self.referenced_payload_bytes
    }
    pub const fn reused_payload_bytes(&self) -> u64 {
        self.reused_payload_bytes
    }
    pub const fn deterministic_payload_bytes(&self) -> u64 {
        self.deterministic_payload_bytes
    }

    fn add(
        &mut self,
        role: OutputNoveltyFieldRoleV1,
        bytes: u64,
        tokens: u64,
    ) -> Result<(), OutputNoveltyError> {
        let (fields, payload_bytes, payload_tokens) = match role {
            OutputNoveltyFieldRoleV1::Referenced => (
                &mut self.referenced_fields,
                &mut self.referenced_payload_bytes,
                &mut self.referenced_standalone_payload_tokens,
            ),
            OutputNoveltyFieldRoleV1::Reused => (
                &mut self.reused_fields,
                &mut self.reused_payload_bytes,
                &mut self.reused_standalone_payload_tokens,
            ),
            OutputNoveltyFieldRoleV1::Novel => (
                &mut self.novel_fields,
                &mut self.novel_payload_bytes,
                &mut self.novel_standalone_payload_tokens,
            ),
            OutputNoveltyFieldRoleV1::Deterministic => (
                &mut self.deterministic_fields,
                &mut self.deterministic_payload_bytes,
                &mut self.deterministic_standalone_payload_tokens,
            ),
        };
        *fields = fields
            .checked_add(1)
            .ok_or(OutputNoveltyError::AccountingOverflow)?;
        *payload_bytes = payload_bytes
            .checked_add(bytes)
            .ok_or(OutputNoveltyError::AccountingOverflow)?;
        *payload_tokens = payload_tokens
            .checked_add(tokens)
            .ok_or(OutputNoveltyError::AccountingOverflow)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OutputNoveltyReceiptV1 {
    schema_version: String,
    selection_origin: OutputSelectionOriginV1,
    classification_authority_digest: DigestV1,
    selected_effect_digest: DigestV1,
    verification_receipt_digest: DigestV1,
    tokenizer_identity_digest: DigestV1,
    encoding_digest: DigestV1,
    total_encoded_bytes: u64,
    total_encoded_tokens: u64,
    fields: Vec<OutputNoveltyFieldReceiptV1>,
    totals: OutputNoveltyTotalsV1,
}

impl OutputNoveltyReceiptV1 {
    pub const fn selection_origin(&self) -> OutputSelectionOriginV1 {
        self.selection_origin
    }
    pub const fn classification_authority_digest(&self) -> DigestV1 {
        self.classification_authority_digest
    }
    pub const fn selected_effect_digest(&self) -> DigestV1 {
        self.selected_effect_digest
    }
    pub const fn verification_receipt_digest(&self) -> DigestV1 {
        self.verification_receipt_digest
    }
    pub const fn tokenizer_identity_digest(&self) -> DigestV1 {
        self.tokenizer_identity_digest
    }
    pub const fn encoding_digest(&self) -> DigestV1 {
        self.encoding_digest
    }
    pub const fn total_encoded_bytes(&self) -> u64 {
        self.total_encoded_bytes
    }
    pub const fn total_encoded_tokens(&self) -> u64 {
        self.total_encoded_tokens
    }
    pub fn fields(&self) -> &[OutputNoveltyFieldReceiptV1] {
        &self.fields
    }
    pub const fn totals(&self) -> OutputNoveltyTotalsV1 {
        self.totals
    }
}

/// Canonical coding plus a payload-free receipt. Field ordering is caller order.
pub struct OutputNoveltyCodingV1 {
    fields: Vec<OutputNoveltyFieldV1>,
    encoded: Vec<u8>,
    receipt: OutputNoveltyReceiptV1,
}

impl OutputNoveltyCodingV1 {
    pub fn encode<T: ExactTokenizerAdapter + ?Sized>(
        tokenizer: &T,
        classification_authority_digest: DigestV1,
        selected_effect_digest: DigestV1,
        verification_receipt_digest: DigestV1,
        fields: Vec<OutputNoveltyFieldV1>,
    ) -> Result<Self, OutputNoveltyError> {
        for (name, digest) in [
            ("classification authority", classification_authority_digest),
            ("selected effect", selected_effect_digest),
            ("verification receipt", verification_receipt_digest),
        ] {
            if digest == DigestV1::ZERO {
                return Err(OutputNoveltyError::ZeroIdentity(name));
            }
        }
        if fields.is_empty() {
            return Err(OutputNoveltyError::EmptyFields);
        }
        if fields.len() > MAX_OUTPUT_NOVELTY_FIELDS {
            return Err(OutputNoveltyError::TooManyFields {
                actual: fields.len(),
                limit: MAX_OUTPUT_NOVELTY_FIELDS,
            });
        }
        let mut names = BTreeSet::new();
        for field in &fields {
            validate_field_name(&field.name)?;
            if !names.insert(field.name.clone()) {
                return Err(OutputNoveltyError::DuplicateFieldName(field.name.clone()));
            }
            if field.role == OutputNoveltyFieldRoleV1::Novel && field.bytes.is_empty() {
                return Err(OutputNoveltyError::EmptyNovelField(field.name.clone()));
            }
        }

        let expected_encoded_bytes = encoded_len(&fields)?;
        if expected_encoded_bytes > MAX_OUTPUT_NOVELTY_BYTES {
            return Err(OutputNoveltyError::EncodedByteLimit {
                actual: expected_encoded_bytes,
                limit: MAX_OUTPUT_NOVELTY_BYTES,
            });
        }
        let encoded = encode_fields(
            classification_authority_digest,
            selected_effect_digest,
            verification_receipt_digest,
            &fields,
        )?;
        debug_assert_eq!(encoded.len(), expected_encoded_bytes);
        let encoded_map = ExactTokenMap::tokenize(tokenizer, &encoded)?;
        let mut totals = OutputNoveltyTotalsV1::default();
        let mut field_receipts = Vec::with_capacity(fields.len());
        for field in &fields {
            let field_map = ExactTokenMap::tokenize(tokenizer, &field.bytes)?;
            let payload_bytes =
                u64::try_from(field.bytes.len()).map_err(|_| OutputNoveltyError::LengthOverflow)?;
            let standalone_payload_tokens = u64::try_from(field_map.token_count())
                .map_err(|_| OutputNoveltyError::LengthOverflow)?;
            totals.add(field.role, payload_bytes, standalone_payload_tokens)?;
            field_receipts.push(OutputNoveltyFieldReceiptV1 {
                name: field.name.clone(),
                role: field.role,
                payload_digest: digest(&field.bytes),
                payload_bytes,
                standalone_payload_tokens,
            });
        }
        let receipt = OutputNoveltyReceiptV1 {
            schema_version: OUTPUT_NOVELTY_SCHEMA_V1.to_string(),
            selection_origin: OutputSelectionOriginV1::CallerSupplied,
            classification_authority_digest,
            selected_effect_digest,
            verification_receipt_digest,
            tokenizer_identity_digest: tokenizer.identity().digest(),
            encoding_digest: digest(&encoded),
            total_encoded_bytes: u64::try_from(encoded.len())
                .map_err(|_| OutputNoveltyError::LengthOverflow)?,
            total_encoded_tokens: u64::try_from(encoded_map.token_count())
                .map_err(|_| OutputNoveltyError::LengthOverflow)?,
            fields: field_receipts,
            totals,
        };
        Ok(Self {
            fields,
            encoded,
            receipt,
        })
    }

    pub fn fields(&self) -> &[OutputNoveltyFieldV1] {
        &self.fields
    }
    pub fn encoded(&self) -> &[u8] {
        &self.encoded
    }
    pub fn receipt(&self) -> &OutputNoveltyReceiptV1 {
        &self.receipt
    }

    /// Reconstruct the exact caller-selected fields after checking that the
    /// retained canonical coding still matches them byte-for-byte.
    pub fn reconstruct_fields(&self) -> Result<Vec<OutputNoveltyFieldV1>, OutputNoveltyError> {
        let reconstructed = self.fields.clone();
        let encoded = encode_fields(
            self.receipt.classification_authority_digest,
            self.receipt.selected_effect_digest,
            self.receipt.verification_receipt_digest,
            &reconstructed,
        )?;
        if encoded != self.encoded {
            return Err(OutputNoveltyError::EncodingMismatch);
        }
        Ok(reconstructed)
    }
}

fn validate_field_name(name: &str) -> Result<(), OutputNoveltyError> {
    let valid = !name.is_empty()
        && name.len() <= MAX_OUTPUT_NOVELTY_FIELD_NAME_BYTES
        && name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit() && index > 0
                || index > 0 && matches!(byte, b'_' | b'-' | b'.')
        });
    if valid {
        Ok(())
    } else {
        Err(OutputNoveltyError::InvalidFieldName(name.to_string()))
    }
}

fn encoded_len(fields: &[OutputNoveltyFieldV1]) -> Result<usize, OutputNoveltyError> {
    let mut len = ENCODING_DOMAIN_V1
        .len()
        .checked_add(3 * 32)
        .and_then(|value| value.checked_add(8 + fields.len().to_string().len()))
        .ok_or(OutputNoveltyError::LengthOverflow)?;
    for field in fields {
        len = len
            .checked_add(8 + field.name.len())
            .and_then(|value| value.checked_add(8 + field.role.as_str().len()))
            .and_then(|value| value.checked_add(8 + field.bytes.len()))
            .ok_or(OutputNoveltyError::LengthOverflow)?;
    }
    Ok(len)
}

fn encode_fields(
    classification_authority_digest: DigestV1,
    selected_effect_digest: DigestV1,
    verification_receipt_digest: DigestV1,
    fields: &[OutputNoveltyFieldV1],
) -> Result<Vec<u8>, OutputNoveltyError> {
    let mut out = ENCODING_DOMAIN_V1.to_vec();
    out.extend_from_slice(classification_authority_digest.as_bytes());
    out.extend_from_slice(selected_effect_digest.as_bytes());
    out.extend_from_slice(verification_receipt_digest.as_bytes());
    put_bytes(&mut out, fields.len().to_string().as_bytes())?;
    for field in fields {
        put_bytes(&mut out, field.name.as_bytes())?;
        put_bytes(&mut out, field.role.as_str().as_bytes())?;
        put_bytes(&mut out, &field.bytes)?;
        if out.len() > MAX_OUTPUT_NOVELTY_BYTES {
            return Err(OutputNoveltyError::EncodedByteLimit {
                actual: out.len(),
                limit: MAX_OUTPUT_NOVELTY_BYTES,
            });
        }
    }
    Ok(out)
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), OutputNoveltyError> {
    let len = u64::try_from(bytes.len()).map_err(|_| OutputNoveltyError::LengthOverflow)?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

fn digest(bytes: &[u8]) -> DigestV1 {
    DigestV1::from_bytes(sha256(bytes))
}

#[cfg(test)]
mod tests {
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
            OutputNoveltyFieldV1::new("body", OutputNoveltyFieldRoleV1::Novel, b"b".to_vec())
                .unwrap(),
        ];
        assert!(matches!(
            OutputNoveltyCodingV1::encode(&adapter, d(1), d(2), d(3), duplicate),
            Err(OutputNoveltyError::DuplicateFieldName(name)) if name == "body"
        ));
    }
}
