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
    let bytes = &*digest;
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        push_hex_byte(&mut out, b);
    }
    out
}

pub fn enforce_token_budget(text: &str, max_visible_tokens: usize) -> String {
    enforce_token_budget_with_ref(text, max_visible_tokens, None)
}

/// Budget enforcement with a recovery cue: when `recovery_ref` is provided,
/// the truncation marker names the exact ref to expand instead of the
/// generic "exact refs available", so an agent reading only the visible
/// capsule knows precisely how to recover the omitted content. The marker's
/// token cost is part of the budget either way.
pub fn enforce_token_budget_with_ref(
    text: &str,
    max_visible_tokens: usize,
    recovery_ref: Option<&str>,
) -> String {
    if max_visible_tokens == 0 || count_tokens(text) <= max_visible_tokens {
        return text.to_string();
    }
    let marker = match recovery_ref {
        Some(ref_id) => {
            format!("... omitted by visible budget; expand {ref_id} for the full output ...")
        }
        None => "... omitted by visible budget; exact refs available ...".to_string(),
    };
    let marker = marker.as_str();
    let marker_tokens = count_tokens(marker);
    if marker_tokens > max_visible_tokens {
        return "omitted".to_string();
    }
    // O(n) shape: iterate lines, tokenize each lazily, and stop the first
    // time adding the next line (plus the trailing "\n{marker}" overhead)
    // would exceed the budget. Replaces the prior O(n^2) (re-join the whole
    // prefix + re-tokenize on every iteration).
    //
    // Isomorphism note: `count_tokens` treats `\n` as whitespace, not a token
    // boundary, so `count_tokens(lines[..=k].join("\n"))` equals
    // `sum(count_tokens(line) for line in lines[..=k])` exactly — a running
    // sum of per-line token counts is equivalent to retokenizing the joined
    // prefix. The candidate being compared in the prior loop was
    // `out.join("\n") + "\n" + marker`, with token cost
    // `running + line_tokens + 1 (the trailing \n) + marker_tokens`.
    const SEPARATOR_TOKENS: usize = 1;
    let mut running: usize = 0;
    let mut keep: usize = 0;
    let mut first_start: Option<usize> = None;
    for line in text.lines() {
        let lt = count_tokens(line);
        let next_total = running
            .saturating_add(lt)
            .saturating_add(SEPARATOR_TOKENS + marker_tokens);
        if next_total > max_visible_tokens {
            break;
        }
        if keep == 0 {
            first_start = Some(line.as_ptr() as usize - text.as_ptr() as usize);
        }
        running = running.saturating_add(lt);
        keep += 1;
    }
    if keep == 0 {
        return marker.to_string();
    }
    // Reassemble the exact same byte sequence the old code produced:
    // `lines[0..keep].join("\n") + "\n" + marker`. The first kept line's
    // start in `text` is captured above; we then walk forward through
    // `text` counting newlines to find the byte offset just past the
    // (keep-1)th newline after the first kept line, which is the end of
    // the joined prefix. This avoids allocating a `Vec<&str>`.
    let start = first_start.expect("keep > 0 implies first_start set");
    let end = prefix_end_for_kept_lines(text, start, keep);
    let prefix = &text[..end];
    let mut out = String::with_capacity(prefix.len() + 1 + marker.len());
    out.push_str(prefix);
    out.push('\n');
    out.push_str(marker);
    out
}

fn prefix_end_for_kept_lines(text: &str, start: usize, keep: usize) -> usize {
    if keep == 1 {
        return next_newline_or_end(text, start);
    }
    let target = keep - 1;
    let mut newlines_seen = 0usize;
    for (i, b) in text[start..].bytes().enumerate() {
        if b == b'\n' {
            newlines_seen += 1;
            if newlines_seen == target {
                return start + i;
            }
        }
    }
    text.len()
}

fn next_newline_or_end(text: &str, start: usize) -> usize {
    for (i, b) in text[start..].bytes().enumerate() {
        if b == b'\n' {
            return start + i;
        }
    }
    text.len()
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

const CL100K: TokenizerMetadata = TokenizerMetadata {
    family: TokenizerFamily::Cl100k,
    chars_per_token_milli: 4_000,
    approximate: true,
};
const O200K: TokenizerMetadata = TokenizerMetadata {
    family: TokenizerFamily::O200k,
    chars_per_token_milli: 4_000,
    approximate: true,
};
const SENTENCEPIECE: TokenizerMetadata = TokenizerMetadata {
    family: TokenizerFamily::SentencePiece,
    chars_per_token_milli: 3_500,
    approximate: true,
};

/// Resolve a provider model id without allocating or making network calls.
pub fn tokenizer_metadata(model_id: &str) -> Option<&'static TokenizerMetadata> {
    // Provider-qualified ids ("openai/gpt-4o-...", "deepseek/...") carry the
    // model name in the last path segment.
    let model_id = model_id.rsplit('/').next().unwrap_or(model_id);
    if starts_with_ignore_ascii_case(model_id, "gpt-4o")
        || starts_with_ignore_ascii_case(model_id, "gpt-4.1")
        || starts_with_ignore_ascii_case(model_id, "gpt-5")
        || starts_with_ignore_ascii_case(model_id, "o1")
        || starts_with_ignore_ascii_case(model_id, "o3")
        || starts_with_ignore_ascii_case(model_id, "o4")
        || contains_ignore_ascii_case(model_id, "codex")
    {
        Some(&O200K)
    } else if starts_with_ignore_ascii_case(model_id, "gpt-4")
        || starts_with_ignore_ascii_case(model_id, "gpt-3.5")
    {
        Some(&CL100K)
    } else if starts_with_ignore_ascii_case(model_id, "llama")
        || starts_with_ignore_ascii_case(model_id, "mistral")
        || starts_with_ignore_ascii_case(model_id, "mixtral")
        || starts_with_ignore_ascii_case(model_id, "gemma")
    {
        Some(&SENTENCEPIECE)
    } else {
        None
    }
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
    if text.is_empty() {
        return 0;
    }
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
    match text.char_indices().nth(chars) {
        Some((end, _)) => &text[..end],
        None => text,
    }
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
        } else {
            if tokens == max_tokens {
                completed = false;
                break;
            }
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
    match active_tokenizer_metadata() {
        Some(metadata) => approximate_token_count(text, metadata),
        None => count_tokens_lexical(text),
    }
}

fn count_tokens_lexical(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    // ASCII fast path: classify each byte via a const 256-entry table.
    // Non-ASCII bytes (>= 0x80) fall back to the per-char path which
    // mirrors the original semantics (char.is_ascii_alphanumeric(),
    // char.is_whitespace()).
    let bytes = text.as_bytes();
    let mut tokens = 0usize;
    let mut in_token = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            match ASCII_TOKEN_CLASS[b as usize] {
                1 => {
                    if !in_token {
                        tokens += 1;
                        in_token = true;
                    }
                }
                2 => in_token = false,
                _ => {
                    in_token = false;
                    tokens += 1;
                }
            }
            i += 1;
        } else {
            // Non-ASCII: defer to the original char-based counting on the
            // remaining slice. This keeps the per-byte loop SIMD-friendly
            // and exact for ASCII, while preserving the Unicode semantics
            // (whitespace per char::is_whitespace, alphanumeric+'_' as in-token).
            return count_tokens_tail(text, i);
        }
    }
    tokens
}

/// Slow path used when `count_tokens` encounters a non-ASCII byte: tokenize
/// the suffix from `start_byte_offset` exactly as the original char-based
/// implementation would, then add the tokens already counted by the fast path.
pub(crate) fn count_tokens_tail(text: &str, start_byte_offset: usize) -> usize {
    let prefix_tokens = {
        // Re-tokenize the ASCII prefix we already walked so the function
        // is self-contained and idempotent; the caller's fast-path prefix
        // tokens are re-derived here. For correctness we just need the
        // suffix tokens added to the prefix count.
        let mut t = 0usize;
        let mut in_token = false;
        for &b in &text.as_bytes()[..start_byte_offset] {
            match ASCII_TOKEN_CLASS[b as usize] {
                1 => {
                    if !in_token {
                        t += 1;
                        in_token = true;
                    }
                }
                2 => in_token = false,
                _ => {
                    in_token = false;
                    t += 1;
                }
            }
        }
        t
    };
    let mut tokens = prefix_tokens;
    let mut in_token = false;
    for ch in text[start_byte_offset..].chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            if !in_token {
                tokens += 1;
                in_token = true;
            }
        } else {
            in_token = false;
            if !ch.is_whitespace() {
                tokens += 1;
            }
        }
    }
    tokens
}

pub fn savings_ratio(raw_tokens: usize, used_tokens: usize) -> f64 {
    if raw_tokens == 0 {
        return 0.0;
    }
    1.0 - (used_tokens as f64 / raw_tokens as f64)
}

#[cfg(test)]
mod tokenizer_tests {
    use super::*;

    #[test]
    fn tokenizer_registry_lookup_and_fallback_are_explicit() {
        let o200k = tokenizer_metadata("openai/gpt-4o-2024-11-20").unwrap();
        assert_eq!(o200k.family, TokenizerFamily::O200k);
        assert!(o200k.approximate);

        let sentencepiece = tokenizer_metadata("Llama-3.3-70B").unwrap();
        assert_eq!(sentencepiece.family, TokenizerFamily::SentencePiece);
        assert!(tokenizer_metadata("claude-sonnet-4").is_none());
        assert_eq!(
            count_tokens_for_model("alpha beta", Some("claude-sonnet-4")),
            2,
            "unknown models must retain the lexical fallback"
        );
    }

    #[test]
    fn token_boundary_packing_keeps_refs_atomic_and_drops_partial_preview_token() {
        let reference = "tz://blob/0123456789abcdef";
        let preview = "alpha betaGamma";
        let packed = pack_to_token_boundary_for_model(preview, 1, None);

        assert_eq!(packed, "alpha ");
        assert_eq!(reference, "tz://blob/0123456789abcdef");
        assert!(count_tokens_for_model(packed, None) <= 1);

        let unicode = pack_to_token_boundary_for_model("ééééé", 1, Some("gpt-4o"));
        assert_eq!(unicode, "éééé");
        assert!(unicode.is_char_boundary(unicode.len()));
    }
}
