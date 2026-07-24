use std::sync::LazyLock;

use crate::*;

/// Lookup table for hex nibble encoding.
pub(crate) const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

/// Encode one byte as two lowercase hex characters, pushed into `out`.
#[inline]
pub(crate) fn push_hex_byte(out: &mut String, b: u8) {
    out.push(HEX_CHARS[(b >> 4) as usize] as char);
    out.push(HEX_CHARS[(b & 0x0f) as usize] as char);
}

pub fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for &b in digest.iter() {
        push_hex_byte(&mut out, b);
    }
    out
}

pub fn enforce_token_budget(text: &str, max_visible_tokens: usize) -> String {
    enforce_token_budget_with_ref(text, max_visible_tokens, None)
}

/// Enforce the visible budget while naming an exact recovery ref when available.
pub fn enforce_token_budget_with_ref(
    text: &str,
    max_visible_tokens: usize,
    recovery_ref: Option<&str>,
) -> String {
    if max_visible_tokens == 0 || count_tokens(text) <= max_visible_tokens {
        return text.to_string();
    }
    let marker = recovery_ref.map_or_else(
        || "[mode=lossy lossy_policy_id=tokenzero.visible-compression.v1 lossy_spans=[{description=omitted-bytes reason=visible-budget recovery_may_be_needed=true}]]".to_string(),
        |ref_id| format!("... omitted by visible budget; expand {ref_id} for the full output ..."),
    );
    let marker = marker.as_str();
    let marker_tokens = count_tokens(marker);
    if marker_tokens > max_visible_tokens {
        // The omission declaration is a correctness floor. It may exceed an
        // impossibly small visible budget, but must never be replaced by an
        // unclassified free-text omission.
        return marker.to_string();
    }
    const SEPARATOR_TOKENS: usize = 1;
    let mut running: usize = 0;
    let mut keep: usize = 0;
    for line in text.lines() {
        let lt = count_tokens(line);
        let next_total = running
            .saturating_add(lt)
            .saturating_add(SEPARATOR_TOKENS + marker_tokens);
        if next_total > max_visible_tokens {
            break;
        }
        running = running.saturating_add(lt);
        keep += 1;
    }
    if keep == 0 {
        return marker.to_string();
    }
    let end = prefix_end_for_kept_lines(text, keep);
    let prefix = &text[..end];
    let mut out = String::with_capacity(prefix.len() + 1 + marker.len());
    out.push_str(prefix);
    out.push('\n');
    out.push_str(marker);
    out
}

fn prefix_end_for_kept_lines(text: &str, keep: usize) -> usize {
    // `keep` is the number of lines the budget loop retained. The newline
    // after line N is at match index N-1 (`nth(keep - 1)`). Using `keep - 2`
    // dropped one extra fitting line (P01-001 / tokenzero-g3y.10).
    text.match_indices('\n')
        .nth(keep.saturating_sub(1))
        .map_or(text.len(), |(index, _)| index)
}

/// Tokenizer families whose local token-cost characteristics TokenZero knows.
///
/// No tokenizer vocabulary is linked today. The registered families therefore
/// use disclosed average character costs; unknown models retain the legacy
/// lexical counter exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenizerFamily {
    Cl100k,
    O200k,
    SentencePiece,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenizerMetadata {
    pub family: TokenizerFamily,
    /// Average Unicode scalar values per token, scaled by 1,000.
    pub chars_per_token_milli: usize,
    /// Whether counts and boundaries are estimates rather than vocabulary
    /// lookups. This remains true until a real tokenizer is linked.
    pub approximate: bool,
}

const fn tokenizer(family: TokenizerFamily, chars_per_token_milli: usize) -> TokenizerMetadata {
    TokenizerMetadata {
        family,
        chars_per_token_milli,
        approximate: true,
    }
}

const CL100K: TokenizerMetadata = tokenizer(TokenizerFamily::Cl100k, 4_000);
const O200K: TokenizerMetadata = tokenizer(TokenizerFamily::O200k, 4_000);
const SENTENCEPIECE: TokenizerMetadata = tokenizer(TokenizerFamily::SentencePiece, 3_500);

/// Resolve a provider model id without allocating or making network calls.
pub fn tokenizer_metadata(model_id: &str) -> Option<&'static TokenizerMetadata> {
    let model = model_id.rsplit('/').next().unwrap_or(model_id);
    const RULES: &[(&TokenizerMetadata, &[&str])] = &[
        (&O200K, &["gpt-4o", "gpt-4.1", "gpt-5", "o1", "o3", "o4"]),
        (&CL100K, &["gpt-4", "gpt-3.5"]),
        (&SENTENCEPIECE, &["llama", "mistral", "mixtral", "gemma"]),
    ];
    if contains_ignore_ascii_case(model, "codex") {
        return Some(&O200K);
    }
    RULES.iter().find_map(|(metadata, prefixes)| {
        prefixes
            .iter()
            .any(|prefix| starts_with_ignore_ascii_case(model, prefix))
            .then_some(*metadata)
    })
}

fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

fn contains_ignore_ascii_case(value: &str, needle: &str) -> bool {
    value
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

#[derive(Debug)]
struct ActiveTokenizer {
    model_id: Option<String>,
    metadata: Option<&'static TokenizerMetadata>,
}

static ACTIVE_TOKENIZER: LazyLock<ActiveTokenizer> = LazyLock::new(|| {
    let model_id = ["TOKENZERO_MODEL", "OMP_MODEL", "OPENAI_MODEL"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()));
    let metadata = model_id.as_deref().and_then(tokenizer_metadata);
    ActiveTokenizer { model_id, metadata }
});

/// Model selected once from `TOKENZERO_MODEL`, `OMP_MODEL`, then
/// `OPENAI_MODEL`, in precedence order.
pub fn active_model_id() -> Option<&'static str> {
    ACTIVE_TOKENIZER.model_id.as_deref()
}

pub fn active_tokenizer_metadata() -> Option<&'static TokenizerMetadata> {
    ACTIVE_TOKENIZER.metadata
}

/// Count for an explicit model. Unknown or absent ids deliberately preserve
/// the pre-registry lexical heuristic.
pub fn count_tokens_for_model(text: &str, model_id: Option<&str>) -> usize {
    match model_id.and_then(tokenizer_metadata) {
        Some(metadata) => approximate_token_count(text, metadata),
        None => count_tokens_lexical(text),
    }
}

fn approximate_token_count(text: &str, metadata: &TokenizerMetadata) -> usize {
    text.chars()
        .count()
        .saturating_mul(1_000)
        .div_ceil(metadata.chars_per_token_milli)
}

/// Return the largest prefix that fits `max_tokens` and ends at a token
/// boundary for the selected model. Registered tokenizers use their disclosed
/// average-width boundary; the fallback never cuts a lexical word.
pub fn pack_to_token_boundary(text: &str, max_tokens: usize) -> &str {
    pack_to_token_boundary_with_char_limit(text, max_tokens, usize::MAX)
}

/// Pack a preview while treating refs separately: callers can retain a full,
/// atomic ref and apply both the remaining token budget and a display-width
/// cap to only the preview.
pub fn pack_to_token_boundary_with_char_limit(
    text: &str,
    max_tokens: usize,
    max_chars: usize,
) -> &str {
    pack_to_token_boundary_for_model_with_char_limit(text, max_tokens, max_chars, active_model_id())
}

pub fn pack_to_token_boundary_for_model<'a>(
    text: &'a str,
    max_tokens: usize,
    model_id: Option<&str>,
) -> &'a str {
    pack_to_token_boundary_for_model_with_char_limit(text, max_tokens, usize::MAX, model_id)
}

fn pack_to_token_boundary_for_model_with_char_limit<'a>(
    text: &'a str,
    max_tokens: usize,
    max_chars: usize,
    model_id: Option<&str>,
) -> &'a str {
    if text.is_empty() || max_tokens == 0 || max_chars == 0 {
        return "";
    }
    let Some(metadata) = model_id.and_then(tokenizer_metadata) else {
        return lexical_boundary_prefix(text, max_tokens, max_chars);
    };
    let text_chars = text.chars().count();
    let budget_chars = max_tokens.saturating_mul(metadata.chars_per_token_milli) / 1_000;
    if text_chars <= budget_chars && text_chars <= max_chars {
        return text;
    }
    let capped_tokens = max_chars.saturating_mul(1_000) / metadata.chars_per_token_milli;
    let boundary_tokens = max_tokens.min(capped_tokens);
    let boundary_chars = boundary_tokens.saturating_mul(metadata.chars_per_token_milli) / 1_000;
    char_prefix(text, boundary_chars)
}

fn char_prefix(text: &str, chars: usize) -> &str {
    text.char_indices()
        .nth(chars)
        .map_or(text, |(end, _)| &text[..end])
}

fn lexical_boundary_prefix(text: &str, max_tokens: usize, max_chars: usize) -> &str {
    let mut tokens = 0usize;
    let mut in_word = false;
    let mut boundary = 0usize;
    let mut completed = true;
    for (seen, (start, ch)) in text.char_indices().enumerate() {
        if seen == max_chars {
            completed = false;
            break;
        }
        let end = start + ch.len_utf8();
        let word = ch.is_ascii_alphanumeric() || ch == '_';
        if word {
            if !in_word {
                if tokens == max_tokens {
                    completed = false;
                    break;
                }
                tokens += 1;
                in_word = true;
            }
        } else if ch.is_whitespace() {
            in_word = false;
        } else if tokens == max_tokens {
            completed = false;
            break;
        } else {
            tokens += 1;
            in_word = false;
        }
        if !in_word {
            boundary = end;
        }
    }
    if completed { text } else { &text[..boundary] }
}

/// Per-byte classification for the ASCII fast path of `count_tokens`.
/// 0 = non-whitespace separator (counts as a token if not currently in a token)
/// 1 = in-token (alphanumeric or `_`)
/// 2 = whitespace (breaks tokens, never itself a token)
#[rustfmt::skip]
pub(crate) const ASCII_TOKEN_CLASS: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut b: usize = 0;
    while b < 256 {
        t[b] = match b as u8 {
            b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' | b'_' => 1,
            b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C => 2,
            _ => 0,
        };
        b += 1;
    }
    t
};

pub fn count_tokens(text: &str) -> usize {
    active_tokenizer_metadata().map_or_else(
        || count_tokens_lexical(text),
        |metadata| approximate_token_count(text, metadata),
    )
}

fn count_ascii(bytes: &[u8], stop_at_non_ascii: bool) -> (usize, usize) {
    let (mut tokens, mut in_token) = (0, false);
    for (index, &byte) in bytes.iter().enumerate() {
        if stop_at_non_ascii && !byte.is_ascii() {
            return (tokens, index);
        }
        let class = ASCII_TOKEN_CLASS[byte as usize];
        tokens += usize::from(class == 0 || class == 1 && !in_token);
        in_token = class == 1;
    }
    (tokens, bytes.len())
}

fn count_tokens_lexical(text: &str) -> usize {
    let (tokens, ascii_end) = count_ascii(text.as_bytes(), true);
    if ascii_end == text.len() {
        tokens
    } else {
        count_tokens_tail(text, ascii_end)
    }
}

/// Finish lexical counting after the ASCII fast path reaches Unicode.
pub(crate) fn count_tokens_tail(text: &str, start_byte_offset: usize) -> usize {
    let (mut tokens, _) = count_ascii(&text.as_bytes()[..start_byte_offset], false);
    let mut in_token = false;
    for ch in text[start_byte_offset..].chars() {
        let word = ch.is_ascii_alphanumeric() || ch == '_';
        tokens += usize::from(!ch.is_whitespace() && (!word || !in_token));
        in_token = word;
    }
    tokens
}

/// Fraction of raw tokens avoided, bounded to the meaningful savings range.
///
/// When an envelope is larger than its input, the raw counts still expose that
/// overhead; a "savings" ratio must not report a negative percentage.
pub fn savings_ratio(raw_tokens: usize, used_tokens: usize) -> f64 {
    if raw_tokens == 0 {
        return 0.0;
    }
    (1.0 - (used_tokens as f64 / raw_tokens as f64)).max(0.0)
}

#[cfg(test)]
#[path = "tokens_inline_tests.rs"]
mod tokenizer_tests;
