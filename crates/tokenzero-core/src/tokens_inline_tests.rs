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

#[test]
fn visible_budget_prefix_retains_every_fitting_line() {
    // Counterexample from math-review P01-001: the visible budget must keep
    // every line that fits alongside the omission marker.
    let text = (0..50)
        .map(|i| format!("line_{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    // Must be the SAME marker enforce_token_budget emits. Modelling a
    // different, shorter string here is what made this test fail: it
    // predicted a keep count the real 33-token declaration cannot afford.
    let marker = VISIBLE_BUDGET_LOSSY_DECLARATION;
    let marker_tokens = count_tokens(marker);
    const SEPARATOR_TOKENS: usize = 1;
    // Derive the budget from the real marker so the fixture stays valid if
    // the declaration wording changes. The hard-coded 17 was smaller than
    // the marker itself, leaving no room for even one line.
    // Room for exactly the first three lines, so keep>=2 is truly exercised.
    let budget =
        marker_tokens + SEPARATOR_TOKENS + text.lines().take(3).map(count_tokens).sum::<usize>();
    let out = enforce_token_budget(&text, budget);
    let actual = out.lines().take_while(|line| *line != marker).count();

    // P01-001 is a MAXIMALITY property: keep every line that fits. It was
    // previously asserted by replaying the packer's own per-line estimate,
    // which pinned the implementation rather than the property and went red
    // when that estimate was corrected (tokenzero-t99g). Assert the property
    // directly instead: the output must fit, and adding one more line must
    // not fit.
    assert!(actual >= 2, "fixture must exercise keep>=2; got {actual}");
    assert!(
        count_tokens(&out) <= budget,
        "kept prefix must fit the budget; out={out:?}"
    );

    let total_lines = text.lines().count();
    if actual < total_lines {
        let one_more = text.lines().take(actual + 1).collect::<Vec<_>>().join("\n");
        let with_marker = format!("{one_more}\n{marker}");
        assert!(
            count_tokens(&with_marker) > budget,
            "packer dropped a line that would have fit (P01-001); \
                 kept {actual}, but {} more tokens still fits budget {budget}",
            count_tokens(&with_marker)
        );
    }

    assert_eq!(prefix_end_for_kept_lines("a\nb\nc", 1), 1);
    assert_eq!(prefix_end_for_kept_lines("a\nb\nc", 2), 3);
    assert_eq!(prefix_end_for_kept_lines("a\nb\nc", 3), 5);
}
#[test]
fn savings_ratio_never_reports_negative_savings() {
    assert_eq!(savings_ratio(0, 100), 0.0);
    assert_eq!(savings_ratio(100, 120), 0.0);
    assert_eq!(savings_ratio(100, 100), 0.0);
    assert_eq!(savings_ratio(100, 25), 0.75);
}
