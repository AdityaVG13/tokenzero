//! Session-scoped short ref aliases for visible capsules.
//!
//! Full-hash `tz://blob/<64hex>` / `fz://blob/<64hex>` refs cost ~18-25 BPE tokens
//! each. Visible text emits `tz://s/<16hex>` (GraphZero-aligned prefix length)
//! while the recovery store keeps `short → full` in the alias table so expand
//! accepts either form. Aliases are content-addressed (first 16 hex of the hash),
//! so concurrent engines sharing a store agree on the short form.

use serde_json::Value;

/// Prefix length for session-visible short aliases (matches GraphZero's 16-hex habit).
pub const SESSION_ALIAS_HEX_LEN: usize = 16;

const SESSION_ALIAS_PREFIX: &str = "tz://s/";

/// Split a ref into bare identity + optional `#B`/`#L` fragment.
pub fn split_ref_fragment(ref_id: &str) -> (&str, Option<&str>) {
    ref_id
        .split_once('#')
        .map_or((ref_id, None), |(bare, frag)| (bare, Some(frag)))
}

fn is_lower_hex(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// True when `bare` is a portable full-hash blob ref (`tz|fz|gz://blob/<64hex>`).
pub fn is_full_hash_blob_bare(bare: &str) -> bool {
    full_hash_blob_parts(bare).is_some()
}

fn full_hash_blob_parts(bare: &str) -> Option<&str> {
    for prefix in ["tz://blob/", "fz://blob/", "gz://blob/"] {
        if let Some(hash) = bare.strip_prefix(prefix) {
            if hash.len() == 64 && is_lower_hex(hash) {
                return Some(hash);
            }
        }
    }
    None
}

/// Canonical full-hash target stored behind a session alias (`tz://blob/<64hex>`).
pub fn canonical_full_blob_ref(bare: &str) -> Option<String> {
    full_hash_blob_parts(bare).map(|hash| format!("tz://blob/{hash}"))
}

/// Session-visible short form for a full-hash blob ref, preserving fragments.
///
/// Returns `None` when `ref_id` is not a portable full-hash blob ref (already
/// short, logical, file/unit, etc.).
pub fn session_visible_blob_alias(ref_id: &str) -> Option<String> {
    let (bare, frag) = split_ref_fragment(ref_id);
    let hash = full_hash_blob_parts(bare)?;
    let short = format!(
        "{SESSION_ALIAS_PREFIX}{}",
        &hash[..SESSION_ALIAS_HEX_LEN]
    );
    Some(match frag {
        Some(f) => format!("{short}#{f}"),
        None => short,
    })
}

/// True when `bare` is a session short alias (`tz://s/<1-64 hex>`).
pub fn is_session_alias_bare(bare: &str) -> bool {
    bare.strip_prefix(SESSION_ALIAS_PREFIX)
        .is_some_and(|id| !id.is_empty() && id.len() <= 64 && is_lower_hex(id))
}

/// Replace every full-hash blob ref in `text` with its session-visible alias.
pub fn rewrite_full_hash_blob_refs_in_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if let Some((consumed, replacement)) = match_full_hash_blob_at(text, i) {
            out.push_str(&replacement);
            i += consumed;
            continue;
        }
        // Advance one full character: refs are pure ASCII, so multibyte
        // characters are copied verbatim (byte-wise `as char` casts would
        // mojibake them, and mid-char slicing panics).
        let next = (i + 1..=text.len())
            .find(|&index| text.is_char_boundary(index))
            .unwrap_or(text.len());
        out.push_str(&text[i..next]);
        i = next;
    }
    out
}

/// If `text[start..]` begins with a full-hash blob ref (optional fragment),
/// return `(end_byte_index, full_ref_string)`.
pub fn take_full_hash_blob_at(text: &str, start: usize) -> Option<(usize, String)> {
    let (consumed, _replacement) = match_full_hash_blob_at(text, start)?;
    let full = text[start..start + consumed].to_string();
    Some((start + consumed, full))
}

fn match_full_hash_blob_at(text: &str, start: usize) -> Option<(usize, String)> {
    // Callers scan byte offsets; a mid-character offset can never start an
    // ASCII ref and must not panic the slice below.
    if !text.is_char_boundary(start) {
        return None;
    }
    let rest = &text[start..];
    for prefix in ["tz://blob/", "fz://blob/", "gz://blob/"] {
        if !rest.starts_with(prefix) {
            continue;
        }
        let after = &rest[prefix.len()..];
        if after.len() < 64 {
            return None;
        }
        let hash = &after[..64];
        if !is_lower_hex(hash) {
            return None;
        }
        let mut consumed = prefix.len() + 64;
        let mut frag: Option<&str> = None;
        if let Some(tail) = after.get(64..) {
            if let Some(stripped) = tail.strip_prefix('#') {
                if let Some(frag_len) = fragment_len(stripped) {
                    frag = Some(&tail[..=frag_len]);
                    consumed += 1 + frag_len;
                }
            }
        }
        let short = format!("{SESSION_ALIAS_PREFIX}{}", &hash[..SESSION_ALIAS_HEX_LEN]);
        let replacement = match frag {
            Some(f) => format!("{short}{f}"),
            None => short,
        };
        return Some((consumed, replacement));
    }
    None
}

fn fragment_len(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let kind = bytes[0] as char;
    if kind != 'B' && kind != 'L' {
        return None;
    }
    let mut i = 1;
    let mut saw_digit = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        saw_digit = true;
        i += 1;
    }
    if !saw_digit {
        return None;
    }
    if i < bytes.len() && bytes[i] == b'-' {
        i += 1;
        let mut saw_end = false;
        if kind == 'L' && i < bytes.len() && bytes[i] == b'L' {
            i += 1;
        }
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            saw_end = true;
            i += 1;
        }
        if !saw_end {
            return None;
        }
    }
    Some(i)
}

/// Walk a JSON value and rewrite full-hash blob ref strings in place.
pub fn rewrite_full_hash_blob_refs_in_value(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some(short) = session_visible_blob_alias(text) {
                *text = short;
            } else if text.contains("://blob/") {
                let rewritten = rewrite_full_hash_blob_refs_in_text(text);
                if rewritten != *text {
                    *text = rewritten;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                rewrite_full_hash_blob_refs_in_value(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                rewrite_full_hash_blob_refs_in_value(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_full_hash_to_sixteen_hex_session_form() {
        let full = format!("tz://blob/{}", "ab".repeat(32));
        let short = session_visible_blob_alias(&full).unwrap();
        assert_eq!(short, format!("tz://s/{}", "ab".repeat(8)));
    }

    #[test]
    fn preserves_byte_fragment() {
        let full = format!("fz://blob/{}#B0-12", "cd".repeat(32));
        let short = session_visible_blob_alias(&full).unwrap();
        assert_eq!(short, format!("tz://s/{}#B0-12", "cd".repeat(8)));
    }

    #[test]
    fn leaves_non_full_hash_alone() {
        assert!(session_visible_blob_alias("tz://blob/b0123456789abcdef").is_none());
        assert!(session_visible_blob_alias("tz://s/abcdef0123456789").is_none());
        assert!(session_visible_blob_alias("tz://file/abc").is_none());
    }

    #[test]
    fn rewrites_embedded_refs_in_text() {
        let hash = "11".repeat(32);
        let text = format!("stderr_ref: tz://blob/{hash}\nok");
        let out = rewrite_full_hash_blob_refs_in_text(&text);
        assert_eq!(out, format!("stderr_ref: tz://s/{}\nok", &hash[..16]));
    }

    #[test]
    fn take_full_hash_reports_end_index() {
        let hash = "22".repeat(32);
        let text = format!("x tz://blob/{hash} y");
        let (end, full) = take_full_hash_blob_at(&text, 2).unwrap();
        assert_eq!(full, format!("tz://blob/{hash}"));
        assert_eq!(&text[end..], " y");
    }
}
