//! tokenzero-t99g: the visible-budget packer summed ceil(chars(line)/q) per line
//! plus one separator token, but registered-model counting is ceil(total
//! scalars/q) over the WHOLE constructed output including every newline. The
//! per-line ceilings discard each line's fractional residue, so the packer could
//! admit content whose assembled form counts over budget.
//!
//! This lives in its own test binary because the active tokenizer is resolved
//! once per process from TOKENZERO_MODEL through a LazyLock; the in-crate unit
//! tests exercise only the default lexical counter and cannot reach this path.

use tokenzero_core::{count_tokens, enforce_token_budget};

/// Mirrors the private VISIBLE_BUDGET_LOSSY_DECLARATION correctness floor.
const LOSSY_DECLARATION: &str = "[mode=lossy lossy_policy_id=tokenzero.visible-compression.v1 lossy_spans=[{description=omitted-bytes reason=visible-budget recovery_may_be_needed=true}]]";

fn assert_within_budget(text: &str, budget: usize) {
    // Documented exception: the omission declaration is a correctness floor and
    // may exceed an impossibly small budget rather than be replaced by an
    // unclassified free-text omission.
    if budget < count_tokens(LOSSY_DECLARATION) {
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
fn packing_never_exceeds_the_budget_under_a_registered_model() {
    // The defect is specific to registered-model counting. Without a model the
    // lexical counter is active and this binary has nothing to prove, so skip
    // rather than pass vacuously or fail on an unset env var.
    let Some(model) = tokenzero_core::active_model_id() else {
        eprintln!("skipped: set TOKENZERO_MODEL (e.g. gpt-4o) to exercise model counting");
        return;
    };
    eprintln!("exercising registered model {model}");

    // The two falsifiers named in the omega-math finding: eight 4-char lines at
    // budget 48 counted 49; five 7-char lines at budget 55 counted 56.
    assert_within_budget(&"abcd\n".repeat(8), 48);
    assert_within_budget(&"abcdefg\n".repeat(5), 55);

    // Sweep the width/count/budget space rather than trusting two points, since
    // the defect is a residue-accumulation effect that appears at boundaries.
    for line_width in 1..48usize {
        let line = "x".repeat(line_width);
        for lines in 1..24usize {
            let text = format!("{}\n", vec![line.clone(); lines].join("\n"));
            for budget in 1..220usize {
                assert_within_budget(&text, budget);
            }
        }
    }

    for text in [
        "\n\n\n\n",
        "a\n\nb\n\nc\n",
        "\na\n",
        "trailing\n\n\n",
        "x",
        "",
    ] {
        for budget in 1..220usize {
            assert_within_budget(text, budget);
        }
    }
}
