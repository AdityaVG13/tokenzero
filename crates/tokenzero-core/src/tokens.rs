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

/// The lossy declaration emitted when the visible budget drops bytes and no
/// recovery ref is available.
///
/// Single source of truth. This literal was previously duplicated in
/// `enforce_token_budget_with_ref` and in the capsule emitter, and the budget
/// test modelled a THIRD, shorter string. The test computed how many lines
/// should survive using a 14-token marker while the real marker costs 33, so
/// it demanded more lines than the budget could hold and failed as
/// P01-001. Keep every user pointed at this constant.
pub const VISIBLE_BUDGET_LOSSY_DECLARATION: &str = "[mode=lossy lossy_policy_id=tokenzero.visible-compression.v1 lossy_spans=[{description=omitted-bytes reason=visible-budget recovery_may_be_needed=true}]]";

pub fn enforce_token_budget(text: &str, max_visible_tokens: usize) -> String {
    enforce_token_budget_with_ref(text, max_visible_tokens, None)
}

/// Enforce the visible budget while naming an exact recovery ref when available.
pub fn enforce_token_budget_with_ref(
    text: &str,
    max_visible_tokens: usize,
    recovery_ref: Option<&str>,
) -> String {
    if count_tokens(text) <= max_visible_tokens {
        return text.to_string();
    }

    let marker = recovery_ref.map_or_else(
        || VISIBLE_BUDGET_LOSSY_DECLARATION.to_string(),
        |reference| format!("{VISIBLE_BUDGET_LOSSY_DECLARATION} recovery_ref={reference}"),
    );
    let marker_tokens = count_tokens(&marker);

    // Structured elision can fit below the longer plain-text correctness floor.
    // Try it first so valid objects and arrays remain valid whenever their minimal
    // sentinel representation fits.
    if let Some(json) = elide_top_level_json(text, max_visible_tokens, recovery_ref) {
        return json;
    }
    if matches!(
        serde_json::from_str::<serde_json::Value>(text),
        Ok(serde_json::Value::Object(_) | serde_json::Value::Array(_))
    ) {
        // A structured payload reached here only because the minimal sentinel did
        // not fit or its reserved object key collided. Never emit a JSON prefix.
        return marker;
    }

    if marker_tokens > max_visible_tokens {
        // The canonical lossy declaration is a correctness floor. An impossibly
        // small budget must not turn an omission into unclassified free text.
        return marker;
    }

    retain_plain_lines_after_marker(text, max_visible_tokens, marker)
}

const INLINE_ELISION_SENTINEL_KEY: &str = "__tokenzero_elision__";

fn elide_top_level_json(
    text: &str,
    max_visible_tokens: usize,
    recovery_ref: Option<&str>,
) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(text).ok()?;
    let sentinel = serde_json::json!({
        "lossy": true,
        "reason": "visible-budget",
        "recovery_ref": recovery_ref,
    });
    let sentinel = serde_json::to_string(&sentinel).expect("sentinel is serializable");
    let key =
        serde_json::to_string(INLINE_ELISION_SENTINEL_KEY).expect("sentinel key is serializable");

    match value {
        serde_json::Value::Object(entries) => {
            if entries.contains_key(INLINE_ELISION_SENTINEL_KEY) {
                return None;
            }
            let mut out = format!("{{{key}:{sentinel}");
            if count_tokens(&format!("{out}}}")) > max_visible_tokens {
                return None;
            }
            for (entry_key, value) in entries.iter().take(entries.len().saturating_sub(1)) {
                let entry_key = serde_json::to_string(entry_key).ok()?;
                let value = serde_json::to_string(value).ok()?;
                let candidate = format!("{out},{entry_key}:{value}}}");
                if count_tokens(&candidate) > max_visible_tokens {
                    break;
                }
                out.push(',');
                out.push_str(&entry_key);
                out.push(':');
                out.push_str(&value);
            }
            out.push('}');
            Some(out)
        }
        serde_json::Value::Array(items) => {
            let mut out = format!("[{{{key}:{sentinel}}}");
            if count_tokens(&format!("{out}]")) > max_visible_tokens {
                return None;
            }
            for value in items.iter().take(items.len().saturating_sub(1)) {
                let value = serde_json::to_string(value).ok()?;
                let candidate = format!("{out},{value}]");
                if count_tokens(&candidate) > max_visible_tokens {
                    break;
                }
                out.push(',');
                out.push_str(&value);
            }
            out.push(']');
            Some(out)
        }
        _ => None,
    }
}

fn retain_plain_lines_after_marker(
    text: &str,
    max_visible_tokens: usize,
    marker: String,
) -> String {
    let mut out = marker;
    for line in text.split_inclusive('\n') {
        let mut candidate = String::with_capacity(out.len() + 1 + line.len());
        candidate.push_str(&out);
        candidate.push('\n');
        candidate.push_str(line);
        if count_tokens(&candidate) > max_visible_tokens {
            break;
        }
        out = candidate;
    }
    out
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
fn prefix_end_for_kept_lines(text: &str, kept_lines: usize) -> usize {
    if kept_lines == 0 {
        return 0;
    }

    text.match_indices('\n')
        .nth(kept_lines - 1)
        .map_or(text.len(), |(index, _)| index)
}

#[cfg(test)]
#[path = "tokens_inline_tests.rs"]
mod tokenizer_tests;

#[cfg(test)]
mod inline_elision_tests {
    use super::*;

    fn plain_marker(recovery_ref: Option<&str>) -> String {
        recovery_ref.map_or_else(
            || VISIBLE_BUDGET_LOSSY_DECLARATION.to_string(),
            |reference| format!("{VISIBLE_BUDGET_LOSSY_DECLARATION} recovery_ref={reference}"),
        )
    }

    #[test]
    fn inline_elision_plain_marker_is_head_visible() {
        let marker = plain_marker(None);
        let budget = count_tokens(&format!("{marker}\nalpha\n"));
        let text = format!("alpha\n{}", "payload ".repeat(100));
        let out = enforce_token_budget(&text, budget);
        assert_eq!(out.lines().next(), Some(marker.as_str()));
    }

    #[test]
    fn inline_elision_respects_budget_when_marker_fits() {
        let marker = plain_marker(None);
        let budget = count_tokens(&format!("{marker}\nfirst\nsecond\n"));
        let text = format!("first\nsecond\n{}", "tail ".repeat(100));
        let out = enforce_token_budget(&text, budget);
        assert!(count_tokens(&out) <= budget, "{out:?}");
    }

    #[test]
    fn inline_elision_keeps_recovery_ref_explicit() {
        let recovery = "tz://blob/0123456789abcdef";
        let marker = plain_marker(Some(recovery));
        let out = enforce_token_budget_with_ref(
            &"payload ".repeat(100),
            count_tokens(&marker),
            Some(recovery),
        );
        assert!(out.starts_with(VISIBLE_BUDGET_LOSSY_DECLARATION));
        assert!(out.contains(recovery));
    }

    #[test]
    fn inline_elision_json_object_is_parseable_and_keeps_whole_values() {
        let text = format!(
            r#"{{"a":{{"nested":[1,2,3]}},"b":"kept","z":"{}"}}"#,
            "tail ".repeat(100)
        );
        let maximal = elide_top_level_json(&text, usize::MAX, Some("tz://object")).unwrap();
        let budget = count_tokens(&maximal);
        let out = enforce_token_budget_with_ref(&text, budget, Some("tz://object"));
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(out.starts_with("{\"__tokenzero_elision__\":{"));
        assert_eq!(parsed["a"]["nested"], serde_json::json!([1, 2, 3]));
        assert_eq!(parsed["b"], "kept");
        assert!(parsed.get("z").is_none());
        assert!(count_tokens(&out) <= budget);
    }

    #[test]
    fn inline_elision_json_array_is_parseable_and_keeps_whole_values() {
        let text = format!(
            r#"[{{"nested":[1,2,3]}},["whole",{{"value":4}}],"{}"]"#,
            "tail ".repeat(100)
        );
        let maximal = elide_top_level_json(&text, usize::MAX, Some("tz://array")).unwrap();
        let budget = count_tokens(&maximal);
        let out = enforce_token_budget_with_ref(&text, budget, Some("tz://array"));
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let items = parsed.as_array().unwrap();
        assert!(out.starts_with("[{\"__tokenzero_elision__\":{"));
        assert_eq!(items[1]["nested"], serde_json::json!([1, 2, 3]));
        assert_eq!(items[2], serde_json::json!(["whole", {"value": 4}]));
        assert_eq!(items.len(), 3);
        assert!(count_tokens(&out) <= budget);
    }

    #[test]
    fn inline_elision_reserved_key_collision_falls_back_safely() {
        let text = format!(
            r#"{{"__tokenzero_elision__":{{"user":true}},"payload":"{}"}}"#,
            "tail ".repeat(100)
        );
        let marker = plain_marker(None);
        let out = enforce_token_budget(&text, count_tokens(&marker));
        assert_eq!(out, marker);
        assert!(serde_json::from_str::<serde_json::Value>(&out).is_err());
    }

    #[test]
    fn inline_elision_tiny_budget_uses_marker_correctness_floor() {
        let marker = plain_marker(None);
        let budget = count_tokens(&marker).saturating_sub(1);
        let out = enforce_token_budget(&"payload ".repeat(100), budget);
        assert_eq!(out, marker);
        assert!(count_tokens(&out) > budget);

        let json_out = enforce_token_budget(r#"{"payload":"long long long"}"#, 0);
        assert_eq!(json_out, VISIBLE_BUDGET_LOSSY_DECLARATION);
    }

    #[test]
    fn inline_elision_nonlossy_output_is_byte_identical() {
        let text = "  exact\nJSON-ish: { \"x\": 1 }\n";
        assert_eq!(enforce_token_budget(text, count_tokens(text)), text);
    }

    #[test]
    fn inline_elision_utf8_retains_only_whole_lines() {
        let marker = plain_marker(None);
        let retained = "αβ🙂\n";
        let budget = count_tokens(&format!("{marker}\n{retained}"));
        let text = format!("{retained}{}", "終わり ".repeat(100));
        let out = enforce_token_budget(&text, budget);
        assert_eq!(out, format!("{marker}\n{retained}"));
        assert!(out.is_char_boundary(out.len()));
        assert!(count_tokens(&out) <= budget);
    }
}

#[cfg(test)]
mod visible_budget_never_exceeds {
    use super::*;

    /// tokenzero-t99g: the packer summed ceil(chars(line)/q) per line plus one
    /// separator token, but the registered-model counter is ceil(total scalars/q)
    /// over the WHOLE constructed output including every newline. Per-line
    /// ceilings hide the newlines' fractional residue, so the packer admitted
    /// output it then over-counted.
    ///
    /// The invariant is the only thing that matters here: whatever comes back
    /// must count at or under the budget it was given.
    fn assert_within_budget(text: &str, budget: usize) {
        // Documented exception: the omission declaration is a correctness floor
        // and may exceed an impossibly small budget rather than be replaced by
        // an unclassified free-text omission. Only budgets that can actually
        // hold the marker are in scope for the packing invariant.
        if budget < count_tokens(VISIBLE_BUDGET_LOSSY_DECLARATION) {
            return;
        }
        let out = enforce_token_budget(text, budget);
        let counted = count_tokens(&out);
        assert!(
            counted <= budget,
            "budget {budget} exceeded: counted {counted} for {out:?}"
        );
    }

    #[test]
    fn documented_falsifiers_stay_within_budget() {
        // From the omega-math finding: eight 4-char lines at budget 48 counted
        // 49; five 7-char lines at budget 55 counted 56.
        assert_within_budget(&"abcd\n".repeat(8), 48);
        assert_within_budget(&"abcdefg\n".repeat(5), 55);
    }

    #[test]
    fn width_boundaries_stay_within_budget() {
        for line_width in 1..24usize {
            let line = "x".repeat(line_width);
            for lines in 1..12usize {
                let text = format!("{}\n", vec![line.clone(); lines].join("\n"));
                for budget in 1..96usize {
                    assert_within_budget(&text, budget);
                }
            }
        }
    }

    #[test]
    fn blank_lines_and_trailing_newlines_stay_within_budget() {
        for text in [
            "\n\n\n\n",
            "a\n\nb\n\nc\n",
            "\na\n",
            "trailing\n\n\n",
            "no-trailing-newline",
            "",
        ] {
            for budget in 1..96usize {
                assert_within_budget(text, budget);
            }
        }
    }
}
