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
