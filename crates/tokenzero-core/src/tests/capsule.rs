use super::*;

#[test]
fn capsule_exact_hides_payload() {
    let c = make_capsule("secret payload", Mode::Exact, 10, Some("file"));
    assert!(!c.text.contains("secret payload"));
    assert!(c.raw_tokens > 0);
}

#[test]
fn capsule_never_costs_more_than_raw_text() {
    let text = "short documented answer that already fits the budget\n";
    let capsule = make_capsule(
        text,
        Mode::Auto,
        4000,
        Some("docs/some/deeply/nested/path/readme.md"),
    );

    assert!(
        capsule.visible_tokens <= capsule.raw_tokens,
        "framing must never exceed raw cost: visible={} raw={}",
        capsule.visible_tokens,
        capsule.raw_tokens
    );
    assert_eq!(capsule.text, text.trim_end());
}

#[test]
fn capsule_can_reuse_known_raw_token_count() {
    let text = "alpha beta gamma\n";
    let counted = make_capsule(text, Mode::Auto, 4000, Some("file"));
    let reused =
        make_capsule_with_raw_tokens(text, counted.raw_tokens, Mode::Auto, 4000, Some("file"));

    assert_eq!(reused, counted);
}

#[test]
fn auto_capsule_honors_visible_budget_for_short_token_heavy_files() {
    let text = (0..40)
        .map(|line| {
            format!(
                "line {line}: {}",
                (0..30)
                    .map(|word| format!("token{line}_{word}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let c = make_capsule(&text, Mode::Auto, 120, Some("state.md"));

    assert!(c.raw_tokens > 120);
    assert!(
        c.visible_tokens <= 120,
        "visible_tokens={} text={}",
        c.visible_tokens,
        c.text
    );
    assert!(c.text.contains("omitted"));
}

#[test]
fn budget_truncation_marker_names_recovery_ref_when_known() {
    let text = (0..500)
        .map(|line| format!("line {line} with some content to count"))
        .collect::<Vec<_>>()
        .join("\n");

    let with_ref = enforce_token_budget_with_ref(&text, 100, Some("tz://file/fabc123"));
    assert!(
        with_ref.contains("expand tz://file/fabc123 for the full output"),
        "{with_ref}"
    );
    assert!(
        count_tokens(&with_ref) <= 100,
        "{}",
        count_tokens(&with_ref)
    );

    let without_ref = enforce_token_budget(&text, 100);
    assert!(
        without_ref.contains("exact refs available"),
        "{without_ref}"
    );

    let capsule = make_capsule_with_recovery_ref(
        &text,
        count_tokens(&text),
        Mode::Structured,
        60,
        Some("big.txt"),
        Some("tz://file/fabc123"),
    );
    assert!(
        capsule.text.contains("tz://file/fabc123"),
        "{}",
        capsule.text
    );
}
