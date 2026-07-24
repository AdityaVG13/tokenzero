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
    assert_eq!(c.mode, Mode::Lossy);
    assert_eq!(
        c.lossy_policy_id.as_deref(),
        Some("tokenzero.visible-compression.v1")
    );
    c.validate_omission_rule(&text).unwrap();
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
        without_ref.contains("mode=lossy") && without_ref.contains("recovery_may_be_needed=true"),
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
    assert!(
        capsule
            .exact_refs
            .iter()
            .any(|reference| reference.starts_with("tz://file/fabc123#B0-"))
    );
    capsule.validate_omission_rule(&text).unwrap();
}

fn omission_fixture(text: &str) -> Capsule {
    Capsule {
        text: text.to_string(),
        raw_tokens: 100,
        visible_tokens: 5,
        omitted_lines: 10,
        mode: Mode::Structured,
        protected_anchors: Vec::new(),
        exact_refs: Vec::new(),
        lossy_spans: Vec::new(),
        lossy_policy_id: None,
    }
}

#[test]
fn omission_rule_accepts_protected_anchor_and_exact_selector() {
    let original = "license header\nfunction body\n";
    let mut anchored = omission_fixture("[[anchor:license-spdx]]\nfunction body");
    anchored.protected_anchors.push("license-spdx".to_string());
    anchored.validate_omission_rule(original).unwrap();

    let mut referenced = omission_fixture("function body ... [tz://blob/abc#L1-L2]");
    referenced
        .exact_refs
        .push("tz://blob/abc#L1-L2".to_string());
    referenced.validate_omission_rule(original).unwrap();
}

#[test]
fn omission_rule_rejects_free_text_summary_and_selectorless_ref() {
    let original = "full stack trace with decisive auth frame";
    let free_text = omission_fixture("an error occurred in auth");
    assert!(free_text.validate_omission_rule(original).is_err());

    let mut selectorless = free_text;
    selectorless.exact_refs.push("tz://blob/abc".to_string());
    assert!(selectorless.validate_omission_rule(original).is_err());
}

#[test]
fn omission_rule_requires_complete_lossy_declaration() {
    let original = "INFO one\nINFO two\nINFO three";
    let mut lossy = omission_fixture(
        "INFO lines collapsed\n[mode=lossy lossy_policy_id=logs.v1 lossy_spans=[{recovery_may_be_needed=true}]]",
    );
    lossy.mode = Mode::Lossy;
    lossy.lossy_policy_id = Some("logs.v1".to_string());
    lossy.lossy_spans.push(LossySpan {
        description: "repeated INFO lines".to_string(),
        reason: "noise policy".to_string(),
        recovery_may_be_needed: true,
    });
    lossy.validate_omission_rule(original).unwrap();

    lossy.lossy_policy_id = None;
    assert!(lossy.validate_omission_rule(original).is_err());
}
