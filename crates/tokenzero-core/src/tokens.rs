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
