//! Typed TokenZero implementation consumed directly by ZeroKernel.
//!
//! TokenZero owns measurement, projection, compression, and exact expansion.
//! It does not own shell process creation or expose model-facing commands.

use std::path::PathBuf;
use std::str::FromStr;

use tiktoken_rs::tokenizer::{Tokenizer, get_tokenizer};
use zero_abi::{
    CompressionRequest, CompressionResult, EngineError, EngineErrorKind, EngineInvocation,
    ExpandOptions, ProjectionRequest, ProjectionResult, TokenAccounting, TokenEngine, ZeroHandle,
};
use zero_store::{SelectionIndex, ZeroCas, ZeroObjectMetadata};

/// Pulse-grammar estimator id for the lexical gauge. Never an exact identity.
pub const LEXICAL_ESTIMATOR_ID: &str = "estimator:tokenzero-lexical";
/// Pulse-grammar estimator id for non-UTF-8 byte mass (1 token per byte).
pub const BYTES_ESTIMATOR_ID: &str = "estimator:tokenzero-bytes";

/// Output-contract 4-tuple for [`TokenAccounting`].
///
/// `raw` is billed source mass, `visible`/`spent` are presented mass,
/// `recovered` is cache/recovery mass (`cached`, 0 until a cache hit is
/// charged). This mapping is the account golden, not a second counter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountMass {
    pub raw: u64,
    pub visible: u64,
    pub recovered: u64,
    pub spent: u64,
}

/// Map ZeroKernel accounting onto Pulse/ledger raw/visible/recovered/spent.
pub fn account_mass(accounting: &TokenAccounting) -> AccountMass {
    AccountMass {
        raw: accounting.billed,
        visible: accounting.visible,
        recovered: accounting.cached,
        spent: accounting.visible,
    }
}

/// Pulse `estimator:<slug>` id for an approximate family, or the lexical gauge.
pub fn estimator_tokenizer_id(model_id: Option<&str>) -> String {
    match model_id.and_then(tokenzero_core::tokenizer_metadata) {
        Some(meta) => format!("estimator:tokenzero-{}", meta.family.name()),
        None => LEXICAL_ESTIMATOR_ID.to_string(),
    }
}

/// Bundled tiktoken encoding name. Certified BPE, not Pulse `provider/model@hex`
/// (no revision digest is bound to the crate vocab).
pub fn tiktoken_tokenizer_id(model: &str) -> String {
    let encoding = match get_tokenizer(model) {
        Some(Tokenizer::Cl100kBase) => "cl100k_base",
        Some(Tokenizer::O200kBase) => "o200k_base",
        Some(Tokenizer::O200kHarmony) => "o200k_harmony",
        Some(Tokenizer::P50kBase) => "p50k_base",
        Some(Tokenizer::R50kBase) => "r50k_base",
        Some(Tokenizer::P50kEdit) => "p50k_edit",
        Some(Tokenizer::Gpt2) => "gpt2",
        None => "unknown",
    };
    format!("tiktoken:{encoding}")
}

#[derive(Clone, Debug)]
pub struct ZeroTokenEngine {
    cas: ZeroCas,
    model_id: Option<String>,
}

impl ZeroTokenEngine {
    pub fn open(store_root: impl Into<PathBuf>, model_id: Option<String>) -> Self {
        Self {
            cas: ZeroCas::open(store_root),
            model_id: model_id.or_else(active_model),
        }
    }

    /// Open without reading `TOKENZERO_MODEL` / `OMP_MODEL` / `OPENAI_MODEL`.
    /// `None` is the lexical estimator; goldens and hermetic tests use this.
    pub fn open_unbound(store_root: impl Into<PathBuf>, model_id: Option<String>) -> Self {
        Self {
            cas: ZeroCas::open(store_root),
            model_id,
        }
    }

    fn cancelled(invocation: &EngineInvocation) -> Result<(), EngineError> {
        if invocation.cancellation.is_cancelled() {
            return Err(EngineError::new(
                EngineErrorKind::Cancelled,
                "TokenZero operation cancelled",
                false,
            ));
        }
        Ok(())
    }

    fn count(&self, bytes: &[u8]) -> TokenAccounting {
        let text = std::str::from_utf8(bytes).ok();
        if let (Some(model), Some(text)) = (self.model_id.as_deref(), text)
            && let Ok(bpe) = tiktoken_rs::bpe_for_model(model)
        {
            let count = bpe.encode_with_special_tokens(text).len() as u64;
            return TokenAccounting {
                tokenizer: tiktoken_tokenizer_id(model),
                billed: count,
                visible: count,
                cached: 0,
                certified: true,
            };
        }
        let (count, tokenizer) = match text {
            Some(text) => (
                tokenzero_core::count_tokens_for_model(text, self.model_id.as_deref()) as u64,
                estimator_tokenizer_id(self.model_id.as_deref()),
            ),
            None => (bytes.len() as u64, BYTES_ESTIMATOR_ID.to_string()),
        };
        TokenAccounting {
            tokenizer,
            billed: count,
            visible: count,
            cached: 0,
            certified: false,
        }
    }

    fn clamp_visible_to_budget<'a>(
        &self,
        text: &'a str,
        max_tokens: u64,
    ) -> (&'a str, TokenAccounting, bool) {
        let accounting = self.count(text.as_bytes());
        if accounting.billed <= max_tokens {
            return (text, accounting, false);
        }

        let mut boundaries = Vec::with_capacity(text.chars().count().saturating_add(1));
        boundaries.push(0);
        boundaries.extend(text.char_indices().skip(1).map(|(offset, _)| offset));
        boundaries.push(text.len());
        let mut low = 0usize;
        let mut high = boundaries.len().saturating_sub(1);
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            if self.count(text[..boundaries[middle]].as_bytes()).billed <= max_tokens {
                low = middle;
            } else {
                high = middle.saturating_sub(1);
            }
        }
        let visible = &text[..boundaries[low]];
        let accounting = self.count(visible.as_bytes());
        (visible, accounting, true)
    }

    fn store_exact(&self, bytes: &[u8], media_type: &str) -> Result<ZeroHandle, EngineError> {
        let handle = self.cas.put(bytes).map_err(cas_error)?;
        let selection = std::str::from_utf8(bytes)
            .ok()
            .map(SelectionIndex::from_utf8);
        self.cas
            .publish_metadata(&ZeroObjectMetadata {
                handle: handle.clone(),
                byte_len: bytes.len() as u64,
                media_type: media_type.to_owned(),
                producer: "TokenZero".into(),
                contract_digest: "ZeroKernel.TokenEngine".into(),
                selection,
            })
            .map_err(cas_error)?;
        Ok(handle)
    }
}

impl TokenEngine for ZeroTokenEngine {
    fn measure(
        &self,
        invocation: &EngineInvocation,
        bytes: &[u8],
    ) -> Result<TokenAccounting, EngineError> {
        Self::cancelled(invocation)?;
        Ok(self.count(bytes))
    }

    fn certify(
        &self,
        invocation: &EngineInvocation,
        bytes: &[u8],
        claimed: &TokenAccounting,
    ) -> Result<zero_abi::CertifyResult, EngineError> {
        Self::cancelled(invocation)?;
        let recomputed = self.count(bytes);
        Ok(zero_abi::CertifyResult {
            matches: &recomputed == claimed,
            recomputed,
        })
    }

    fn project(
        &self,
        invocation: &EngineInvocation,
        request: ProjectionRequest,
    ) -> Result<ProjectionResult, EngineError> {
        Self::cancelled(invocation)?;
        let limit = request.visible_byte_limit as usize;
        let raw_accounting = self.count(&request.bytes);
        if request.bytes.len() <= limit
            && let Ok(text) = std::str::from_utf8(&request.bytes)
        {
            return Ok(ProjectionResult {
                visible: text.to_owned(),
                visible_source_bytes: request.bytes.len() as u64,
                exact: None,
                accounting: raw_accounting,
            });
        }
        if limit < 80 {
            return Err(EngineError::new(
                EngineErrorKind::Budget,
                "visible output budget is too small for an exact ZeroHandle",
                false,
            ));
        }
        let handle = self.store_exact(&request.bytes, &request.media_type)?;
        let source = String::from_utf8_lossy(&request.bytes);
        let marker = format!("\nexact: {handle}");
        let visible = bounded_utf8(&source, &marker, limit);
        let visible_source_bytes = visible.strip_suffix(&marker).map_or(0, str::len) as u64;
        let visible_count = self.count(visible.as_bytes());
        // Never-worse vs the raw payload: a handle capsule that costs more
        // tokens than sending the source is not a projection. Pass through
        // even when the byte budget is tight — TokenZero's authority is
        // tokens, not framing bytes.
        if visible_count.visible > raw_accounting.billed
            && let Ok(text) = std::str::from_utf8(&request.bytes)
        {
            return Ok(ProjectionResult {
                visible: text.to_owned(),
                visible_source_bytes: request.bytes.len() as u64,
                exact: None,
                accounting: raw_accounting,
            });
        }
        Ok(ProjectionResult {
            visible,
            visible_source_bytes,
            exact: Some(handle),
            accounting: TokenAccounting {
                tokenizer: raw_accounting.tokenizer,
                billed: raw_accounting.billed,
                visible: visible_count.visible,
                cached: raw_accounting.cached,
                certified: raw_accounting.certified && visible_count.certified,
            },
        })
    }

    fn compress(
        &self,
        invocation: &EngineInvocation,
        request: CompressionRequest,
    ) -> Result<CompressionResult, EngineError> {
        Self::cancelled(invocation)?;
        if request.max_tokens == 0 {
            return Err(EngineError::new(
                EngineErrorKind::InvalidInput,
                "compression max_tokens must be positive",
                false,
            ));
        }
        let text = std::str::from_utf8(&request.bytes).map_err(|_| {
            EngineError::new(
                EngineErrorKind::InvalidInput,
                "compression input must be UTF-8",
                false,
            )
        })?;
        let mode = if request.mode.is_empty() {
            tokenzero_core::Mode::Auto
        } else {
            tokenzero_core::Mode::from_str(&request.mode)
                .map_err(|error| EngineError::new(EngineErrorKind::InvalidInput, error, false))?
        };
        let raw_accounting = self.count(&request.bytes);
        let handle = self.store_exact(&request.bytes, &request.media_type)?;
        let mut capsule = tokenzero_core::make_capsule_with_recovery_ref(
            text,
            raw_accounting.billed as usize,
            mode,
            request.max_tokens as usize,
            request.label.as_deref(),
            Some(&handle.to_string()),
        )
        .map_err(|error| EngineError::new(EngineErrorKind::Budget, error, false))?;
        // ZeroKernel guest passes max_tokens=1024, mode="" (Auto) by default.
        // Auto is passthrough when raw_tokens <= max_tokens, so a 2300-byte
        // 50x-repeated-line input (400 billed tokens) previously returned the
        // entire text verbatim with visible==billed and zero savings. For
        // oversized or highly repetitive inputs compress must still emit a
        // bounded digest while the handle preserves exact recovery.
        let is_passthrough = capsule.text.trim_end() == text.trim_end();
        let oversized = request.bytes.len() > 512 || text.lines().count() > 20;
        if is_passthrough
            && capsule.visible_tokens as u64 >= raw_accounting.billed
            && oversized
            && mode == tokenzero_core::Mode::Auto
        {
            // Prefer run-length collapse for repeated lines; it keeps the
            // visible view honest (visible < billed) and bounded. Fall back
            // to structured head+tail elision for non-repetitive oversized
            // inputs. Both paths already embed the exact handle and enforce
            // the token budget.
            let mut best: Option<tokenzero_core::Capsule> = None;
            for alt_mode in [
                tokenzero_core::Mode::Dedupe,
                tokenzero_core::Mode::Structured,
            ] {
                if let Ok(alt) = tokenzero_core::make_capsule_with_recovery_ref(
                    text,
                    raw_accounting.billed as usize,
                    alt_mode,
                    request.max_tokens as usize,
                    request.label.as_deref(),
                    Some(&handle.to_string()),
                ) {
                    // Honesty requires visible < billed. Prefer the smallest
                    // honest digest; otherwise keep any strictly bounded view.
                    let honest = (alt.visible_tokens as u64) < raw_accounting.billed;
                    let smaller = alt.text.len() < capsule.text.len();
                    // Honesty first: never adopt a smaller-byte view that
                    // costs more tokens than the raw payload.
                    if honest && smaller {
                        best = Some(alt);
                        break;
                    }
                }
            }
            if let Some(alt) = best
                && (alt.visible_tokens as u64) < raw_accounting.billed
            {
                capsule = alt;
            }
        }
        let (visible, visible_accounting, truncated) =
            self.clamp_visible_to_budget(&capsule.text, u64::from(request.max_tokens));
        let visible_tokens = visible_accounting.billed.min(u64::from(request.max_tokens));
        // Never-worse vs raw payload. A wrapper/handle that costs more than
        // the source is not a compression; omit nothing and do not report a
        // save via saturating_sub(visible) looking like break-even.
        if visible_tokens > raw_accounting.billed {
            return Ok(CompressionResult {
                visible: text.to_owned(),
                exact: handle,
                truncated: false,
                omitted_tokens: 0,
                accounting: TokenAccounting {
                    tokenizer: raw_accounting.tokenizer.clone(),
                    billed: raw_accounting.billed,
                    visible: raw_accounting.billed,
                    cached: raw_accounting.cached,
                    certified: raw_accounting.certified,
                },
            });
        }
        Ok(CompressionResult {
            visible: visible.to_owned(),
            exact: handle,
            truncated,
            omitted_tokens: raw_accounting.billed.saturating_sub(visible_tokens),
            accounting: TokenAccounting {
                tokenizer: raw_accounting.tokenizer,
                billed: raw_accounting.billed,
                visible: visible_tokens,
                cached: raw_accounting.cached,
                certified: raw_accounting.certified && visible_accounting.certified,
            },
        })
    }

    fn expand(
        &self,
        invocation: &EngineInvocation,
        handle: &ZeroHandle,
        options: ExpandOptions,
    ) -> Result<Vec<u8>, EngineError> {
        Self::cancelled(invocation)?;
        self.cas.expand(handle, &options).map_err(cas_error)
    }
}

fn active_model() -> Option<String> {
    ["TOKENZERO_MODEL", "OMP_MODEL", "OPENAI_MODEL"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn bounded_utf8(source: &str, marker: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    if marker.len() >= limit {
        let mut end = limit;
        while end > 0 && !marker.is_char_boundary(end) {
            end -= 1;
        }
        return marker[..end].to_owned();
    }
    let head_limit = limit - marker.len();
    let mut end = source.len().min(head_limit);
    while end > 0 && !source.is_char_boundary(end) {
        end -= 1;
    }
    let mut visible = String::with_capacity(end + marker.len());
    visible.push_str(&source[..end]);
    visible.push_str(marker);
    visible
}

fn cas_error(error: impl std::fmt::Display) -> EngineError {
    EngineError::new(
        EngineErrorKind::Corrupt,
        format!("ZeroHandle CAS: {error}"),
        false,
    )
}
