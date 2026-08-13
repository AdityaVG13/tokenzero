use tempfile::tempdir;
use tokenzero_recovery::RecoveryStore;

use super::*;

fn alias_repeated_paths_and_symbols(store: &mut RecoveryStore, text: &str) -> String {
    match alias_repeated_paths_and_symbols_if_changed(store, text) {
        Some(rewritten) => rewritten,
        None => text.to_string(),
    }
}

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
