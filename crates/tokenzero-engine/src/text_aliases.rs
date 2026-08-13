use std::collections::BTreeMap;

use tokenzero_core::{ContentType, count_tokens};
use tokenzero_recovery::RecoveryStore;

#[derive(Clone, Copy)]
enum AliasKind {
    Path,
    Symbol,
}

fn classify(value: &str) -> Option<AliasKind> {
    if value.len() < 16 || value.contains("://") {
        return None;
    }
    if value.contains('/') {
        return Some(AliasKind::Path);
    }
    let mut segments = value.split("::");
    let first = segments.next()?;
    let rest = segments.collect::<Vec<_>>();
    (!first.is_empty()
        && !rest.is_empty()
        && std::iter::once(first).chain(rest).all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        }))
    .then_some(AliasKind::Symbol)
}

/// Borrows every candidate out of `text` rather than allocating a `String` per
/// token. This runs on every response, so an allocation per whitespace-delimited
/// token was pure per-request cost on text that usually has nothing to alias.
fn candidates(text: &str) -> Vec<(&str, Vec<(usize, usize)>)> {
    let mut found = BTreeMap::<&str, Vec<(usize, usize)>>::new();
    let mut start = None;
    for (index, character) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        let allowed =
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '/' | ':');
        if allowed {
            start.get_or_insert(index);
            continue;
        }
        let Some(token_start) = start.take() else {
            continue;
        };
        let mut token_end = index;
        while token_end > token_start && matches!(text.as_bytes()[token_end - 1], b'.' | b':') {
            token_end -= 1;
        }
        if token_end > token_start {
            let raw = &text[token_start..token_end];
            let value_end = raw
                .rsplit_once(':')
                .filter(|(path, suffix)| {
                    path.contains('/')
                        && !suffix.is_empty()
                        && suffix.bytes().all(|byte| byte.is_ascii_digit())
                })
                .map_or(token_end, |(_, suffix)| token_end - suffix.len() - 1);
            let value = &text[token_start..value_end];
            if classify(value).is_some() {
                found
                    .entry(value)
                    .or_default()
                    .push((token_start, value_end));
            }
        }
    }
    let mut repeated = found
        .into_iter()
        .filter(|(_, spans)| spans.len() > 1)
        .collect::<Vec<_>>();
    repeated.sort_by_key(|(_, spans)| spans[0].0);
    repeated
}

/// Replace repeated path and symbol atoms with dense session ordinals when the
/// same token gauge used for refs proves that the visible form is smaller.
/// Each ordinal and its content-addressed short/full forms resolve through the
/// RecoveryStore alias table to byte-identical payloads.
#[cfg(test)]
pub fn alias_repeated_paths_and_symbols(store: &mut RecoveryStore, text: &str) -> String {
    match alias_repeated_paths_and_symbols_if_changed(store, text) {
        Some(rewritten) => rewritten,
        None => text.to_string(),
    }
}

/// True when `text` contains at least one repeated path/symbol atom worth
/// aliasing. This is a pure scan: it opens no store and mints no ordinal, so
/// callers can skip the whole aliasing pipeline on the common no-candidate
/// response instead of paying for a store lease and a full-text token recount.
pub fn has_alias_candidates(text: &str) -> bool {
    if !may_contain_alias_atom(text) {
        return false;
    }
    let floor = ordinal_token_floor();
    candidates(text)
        .into_iter()
        .any(|(value, _)| count_tokens(value) > floor)
}

/// Cheap necessary condition for an alias candidate. `classify` only ever
/// accepts a value containing `/` (path) or `::` (symbol), so text with neither
/// cannot produce a candidate and does not need the char-by-char scan or any
/// tokenizer call. This is a conservative prefilter: it may say "maybe" and let
/// the real scan decide, but it never says "no" to text `classify` would accept.
fn may_contain_alias_atom(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut previous_colon = false;
    for &byte in bytes {
        if byte == b'/' {
            return true;
        }
        if byte == b':' {
            if previous_colon {
                return true;
            }
            previous_colon = true;
        } else {
            previous_colon = false;
        }
    }
    false
}

/// `count_tokens` runs a real tokenizer, so the shortest possible ordinal form
/// is measured once per process instead of once per candidate.
fn ordinal_token_floor() -> usize {
    static FLOOR: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *FLOOR.get_or_init(|| count_tokens("tz://o/1/1"))
}

/// Returns `Some(rewritten)` only when aliasing actually replaced something, so
/// callers can tell "nothing changed" from "changed" without comparing strings.
pub fn alias_repeated_paths_and_symbols_if_changed(
    store: &mut RecoveryStore,
    text: &str,
) -> Option<String> {
    let mut replacements = Vec::<(usize, usize, String)>::new();
    let floor = ordinal_token_floor();
    for (value, spans) in candidates(text) {
        if count_tokens(value) <= floor {
            continue;
        }
        let Ok(full_ref) = store.store_blob(value, ContentType::Unknown) else {
            continue;
        };
        let _short_ref = store.register_session_visible_alias(&full_ref);
        let Ok(range) = store.reserve_ordinal_range(1) else {
            continue;
        };
        let Ok(ordinal_ref) = store.store_ordinal_alias_deferred(range, 0, &full_ref) else {
            continue;
        };
        if count_tokens(value) <= count_tokens(&ordinal_ref) || store.persist_pending().is_err() {
            continue;
        }
        replacements.extend(
            spans
                .into_iter()
                .map(|(start, end)| (start, end, ordinal_ref.clone())),
        );
    }
    if replacements.is_empty() {
        return None;
    }
    replacements.sort_by_key(|(start, _, _)| *start);
    let mut rewritten = text.to_string();
    for (start, end, alias) in replacements.into_iter().rev() {
        rewritten.replace_range(start..end, &alias);
    }
    // Candidate-local token counts are not a sound proof of a win: BPE cost is
    // contextual, so an ordinal that is cheaper standalone can cost more once
    // the surrounding tokens merge. Publish the rewrite only when the final
    // whole string is strictly cheaper than the original.
    if count_tokens(&rewritten) >= count_tokens(text) {
        return None;
    }
    Some(rewritten)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use tokenzero_recovery::RecoveryStore;

    use super::*;

    #[test]
    fn path_heavy_fixture_reduces_ta_and_all_forms_expand_identical_bytes() {
        let corpus = include_str!("../../../tests/engine/fixtures/path_heavy_aliases.txt");
        let expected = [
            "/workspace/crates/tokenzero-engine/src/engine_expand.rs",
            "tokenzero_engine::engine_expand::recovery_orchestration::PathHeavyExpandCoordinator",
        ];
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery.json");
        let mut store = RecoveryStore::new(Some(cache.clone()));
        let rewritten = alias_repeated_paths_and_symbols(&mut store, corpus);

        assert!(count_tokens(&rewritten) < count_tokens(corpus));
        // The emitted ordinal generation is not a stable constant: the first
        // durable range on a fresh persistent store starts at generation 2
        // (generation one is never allocated again after the legacy
        // pre-sidecar era). Derive the actually emitted ordinal refs from the
        // rewritten text instead of hardcoding a stale generation.
        let ordinals = emitted_ordinals(&rewritten);
        assert_eq!(ordinals.len(), 2, "{rewritten}");
        let mut ordinal_by_value = std::collections::BTreeMap::<String, &String>::new();
        for ordinal in &ordinals {
            let expanded = store.expand(ordinal, Some("raw"), None, None, None, None);
            assert!(expanded.found, "{ordinal}: {}", expanded.reason);
            assert!(
                expected.contains(&expanded.content.as_str()),
                "{ordinal} expands to content not in the fixture: {}",
                expanded.content
            );
            assert!(
                ordinal_by_value
                    .insert(expanded.content.clone(), ordinal)
                    .is_none(),
                "two emitted ordinals expand to the same value"
            );
        }
        for value in expected {
            assert!(!rewritten.contains(value));
            let full = store.store_blob(value, ContentType::Unknown).unwrap();
            // The visible short form is the store's keyed (opaque) alias.
            let short = store.register_session_visible_alias(&full);
            let ordinal = ordinal_by_value
                .get(value)
                .expect("one emitted ordinal per expected value");
            let canonical = store.expand(&full, Some("raw"), None, None, None, None);
            for form in [short.as_str(), ordinal.as_str()] {
                let expanded = store.expand(form, Some("raw"), None, None, None, None);
                assert!(expanded.found, "{form}: {}", expanded.reason);
                assert_eq!(expanded.content.as_bytes(), canonical.content.as_bytes());
                assert_eq!(expanded.content.as_bytes(), value.as_bytes());
            }
        }
        let restarted = RecoveryStore::new(Some(cache));
        for ordinal in &ordinals {
            assert!(restarted.has_ref(ordinal));
        }
    }

    /// Extract every distinct `tz://o/<gen>/<ord>` ordinal ref from rewritten
    /// text in first-occurrence order. Ordinals can sit adjacent to punctuation
    /// (the fixture keeps `:line` suffixes), so scan the bytes rather than
    /// splitting on whitespace.
    fn emitted_ordinals(text: &str) -> Vec<String> {
        let mut ordinals = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find("tz://o/") {
            let tail = &rest[start + "tz://o/".len()..];
            let gen_end = tail
                .find(|character: char| !character.is_ascii_digit())
                .unwrap_or(tail.len());
            let Some(after_gen) = tail[gen_end..].strip_prefix('/') else {
                rest = &rest[start + 1..];
                continue;
            };
            let ord_end = after_gen
                .find(|character: char| !character.is_ascii_digit())
                .unwrap_or(after_gen.len());
            let ordinal = format!("tz://o/{}/{}", &tail[..gen_end], &after_gen[..ord_end]);
            if !ordinals.contains(&ordinal) {
                ordinals.push(ordinal);
            }
            rest = &rest[start + 1..];
        }
        ordinals
    }
}

#[cfg(test)]
mod prefilter_soundness {
    use super::*;

    /// The prefilter is only safe if it never rejects text the real scan would
    /// have accepted. Assert the implication directly rather than trusting the
    /// reasoning about `classify`.
    #[test]
    fn prefilter_never_rejects_text_the_scan_would_accept() {
        let corpus = include_str!("../../../tests/engine/fixtures/path_heavy_aliases.txt");
        let mut cases: Vec<String> = vec![
            String::new(),
            "plain prose with no atoms at all".into(),
            "a:b c:d single colons only".into(),
            "repeated_word repeated_word repeated_word".into(),
            "aaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbb".into(),
            corpus.to_string(),
            "crates/tokenzero-engine/src/render.rs crates/tokenzero-engine/src/render.rs".into(),
            "tokenzero_engine::render::alias tokenzero_engine::render::alias".into(),
        ];
        // Long homogeneous text of the shape the warm-read workload actually sees.
        cases.push(
            (0..200)
                .map(|i| format!("line {i} alpha beta gamma delta epsilon token content sample\n"))
                .collect(),
        );
        for text in &cases {
            let scan_found = !candidates(text).is_empty();
            if scan_found {
                assert!(
                    may_contain_alias_atom(text),
                    "prefilter rejected text the scan accepts: {text:?}"
                );
            }
        }
    }

    /// Whole-function equivalence: gating on the prefilter must not change the
    /// answer of has_alias_candidates for any of these inputs.
    #[test]
    fn prefilter_does_not_change_the_answer() {
        let corpus = include_str!("../../../tests/engine/fixtures/path_heavy_aliases.txt");
        let floor = ordinal_token_floor();
        for text in [
            "",
            "no atoms here",
            "a::b::c a::b::c short",
            corpus,
            "/very/long/path/to/a/file.rs /very/long/path/to/a/file.rs",
        ] {
            let unfiltered = candidates(text)
                .into_iter()
                .any(|(value, _)| count_tokens(value) > floor);
            assert_eq!(
                has_alias_candidates(text),
                unfiltered,
                "prefilter changed the answer for {text:?}"
            );
        }
    }
}
