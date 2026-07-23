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

fn candidates(text: &str) -> Vec<(String, Vec<(usize, usize)>)> {
    let mut found = BTreeMap::<String, Vec<(usize, usize)>>::new();
    let mut start = None;
    for (index, character) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        let allowed = character.is_ascii_alphanumeric()
            || matches!(character, '_' | '-' | '.' | '/' | ':');
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
                    .entry(value.to_string())
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
pub fn alias_repeated_paths_and_symbols(store: &mut RecoveryStore, text: &str) -> String {
    let mut replacements = Vec::<(usize, usize, String)>::new();
    for (value, spans) in candidates(text) {
        if count_tokens(&value) <= count_tokens("tz://o/1/1") {
            continue;
        }
        let Ok(full_ref) = store.store_blob(&value, ContentType::Unknown) else {
            continue;
        };
        let _short_ref = store.register_session_visible_alias(&full_ref);
        let Ok(range) = store.reserve_ordinal_range(1) else {
            continue;
        };
        let Ok(ordinal_ref) = store.store_ordinal_alias_deferred(range, 0, &full_ref) else {
            continue;
        };
        if count_tokens(&value) <= count_tokens(&ordinal_ref) || store.persist_pending().is_err() {
            continue;
        }
        replacements.extend(
            spans
                .into_iter()
                .map(|(start, end)| (start, end, ordinal_ref.clone())),
        );
    }
    replacements.sort_by_key(|(start, _, _)| *start);
    let mut rewritten = text.to_string();
    for (start, end, alias) in replacements.into_iter().rev() {
        rewritten.replace_range(start..end, &alias);
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use tokenzero_recovery::{RecoveryStore, session_visible_blob_alias};

    use super::*;

    #[test]
    fn path_heavy_fixture_reduces_ta_and_all_forms_expand_identical_bytes() {
        let corpus = include_str!("../tests/fixtures/path_heavy_aliases.txt");
        let expected = [
            "/workspace/crates/tokenzero-engine/src/engine_expand.rs",
            "tokenzero_engine::engine_expand::recovery_orchestration::PathHeavyExpandCoordinator",
        ];
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery.json");
        let mut store = RecoveryStore::new(Some(cache.clone()));
        let rewritten = alias_repeated_paths_and_symbols(&mut store, corpus);

        assert!(count_tokens(&rewritten) < count_tokens(corpus));
        assert!(rewritten.contains("tz://o/1/1"), "{rewritten}");
        assert!(rewritten.contains("tz://o/1/2"), "{rewritten}");
        for value in expected {
            assert!(!rewritten.contains(value));
            let full = store.store_blob(value, ContentType::Unknown).unwrap();
            let short = session_visible_blob_alias(&full).unwrap();
            let ordinal = if value.starts_with('/') {
                "tz://o/1/1"
            } else {
                "tz://o/1/2"
            };
            let canonical = store.expand(&full, Some("raw"), None, None, None, None);
            for form in [short.as_str(), ordinal] {
                let expanded = store.expand(form, Some("raw"), None, None, None, None);
                assert!(expanded.found, "{form}: {}", expanded.reason);
                assert_eq!(expanded.content.as_bytes(), canonical.content.as_bytes());
                assert_eq!(expanded.content.as_bytes(), value.as_bytes());
            }
        }
        let restarted = RecoveryStore::new(Some(cache));
        for ordinal in ["tz://o/1/1", "tz://o/1/2"] {
            assert!(restarted.has_ref(ordinal));
        }
    }
}
