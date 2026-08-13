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
