use super::*;

fn choose(
    refs_complete: bool,
    streams_truncated: bool,
    command_success: bool,
    shell_inline_budget: usize,
    raw_tokens: usize,
    output: &str,
    combined_ref: &str,
    render_visible: &str,
) -> ShellVisibleChoice {
    ShellVisibleChoice::choose(
        refs_complete,
        streams_truncated,
        command_success,
        shell_inline_budget,
        raw_tokens,
        output,
        combined_ref,
        render_visible,
    )
}

#[test]
fn visible_choice_inlines_small_successful_output() {
    let choice = choose(
        true,
        false,
        true,
        DEFAULT_SHELL_INLINE_BUDGET,
        8,
        "ok\n",
        "tz://blob/combined",
        "# shell ok\nrender",
    );
    assert!(choice.inline);
    assert!(choice.small);
    assert_eq!(choice.visible_text, "ok");
}

#[test]
fn visible_choice_budget_zero_emits_combined_ref_capsule() {
    let choice = choose(
        true,
        false,
        true,
        0,
        8,
        "ok\n",
        "tz://blob/combined",
        "# shell ok\nrender",
    );
    assert!(!choice.inline);
    assert!(choice.small);
    assert_eq!(
        choice.visible_text,
        "# shell ok\ncombined_ref: tz://blob/combined"
    );
}

#[test]
fn visible_choice_large_success_uses_render_visible() {
    let long = "x".repeat(DEFAULT_SHELL_INLINE_BUDGET.saturating_mul(4) + 1);
    let choice = choose(
        true,
        false,
        true,
        DEFAULT_SHELL_INLINE_BUDGET,
        DEFAULT_SHELL_INLINE_BUDGET + 1,
        &long,
        "tz://blob/combined",
        "# shell ok\nrender",
    );
    assert!(!choice.inline);
    assert!(!choice.small);
    assert_eq!(choice.visible_text, "# shell ok\nrender");
}

#[test]
fn visible_choice_incomplete_refs_falls_back_to_trimmed_output() {
    let choice = choose(
        false,
        false,
        true,
        DEFAULT_SHELL_INLINE_BUDGET,
        8,
        "preview\n",
        "tz://blob/combined",
        "# shell ok\nrender",
    );
    assert!(!choice.inline);
    assert!(!choice.small);
    assert_eq!(choice.visible_text, "preview");
}

#[test]
fn visible_choice_truncation_and_failure_are_not_inline() {
    let truncated = choose(
        true,
        true,
        true,
        DEFAULT_SHELL_INLINE_BUDGET,
        8,
        "ok\n",
        "tz://blob/combined",
        "# shell ok\nrender",
    );
    assert!(!truncated.inline);
    assert!(!truncated.small);
    assert_eq!(truncated.visible_text, "# shell ok\nrender");

    let failed = choose(
        true,
        false,
        false,
        DEFAULT_SHELL_INLINE_BUDGET,
        8,
        "boom\n",
        "tz://blob/combined",
        "# shell failed\nrender",
    );
    assert!(!failed.inline);
    assert!(!failed.small);
    assert_eq!(failed.visible_text, "# shell failed\nrender");
}

#[test]
fn visible_choice_inline_without_small_when_budget_exceeds_default() {
    let tokens = DEFAULT_SHELL_INLINE_BUDGET + 50;
    let output = "y".repeat(tokens);
    let choice = choose(
        true,
        false,
        true,
        DEFAULT_SHELL_INLINE_BUDGET + 200,
        tokens,
        &output,
        "tz://blob/combined",
        "# shell ok\nrender",
    );
    assert!(choice.inline);
    assert!(!choice.small);
    assert_eq!(choice.visible_text, output);
}
