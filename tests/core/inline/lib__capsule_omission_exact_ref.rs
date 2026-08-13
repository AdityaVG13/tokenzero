use super::*;

fn big_original() -> String {
    (0..4000)
        .map(|i| format!("line {i} alpha beta gamma delta epsilon token content sample\n"))
        .collect()
}

fn truncated_capsule() -> Capsule {
    Capsule {
        visible_tokens: 10,
        raw_tokens: 100,
        omitted_lines: 3990,
        text: "line 0 alpha beta gamma delta epsilon token content sample".to_string(),
        mode: Mode::Auto,
        protected_anchors: Vec::new(),
        exact_refs: Vec::new(),
        lossy_spans: Vec::new(),
        lossy_policy_id: None,
    }
}

#[test]
fn omission_validation_failure_is_returned_without_panicking() {
    let error = validated_capsule(truncated_capsule(), &big_original()).unwrap_err();
    assert!(error.contains("capsule omitted bytes without"), "{error}");
}

/// tokenzero-kt7z: `tokenzero read --json` aborted with exit 101 on any file
/// large enough to be budgeted, because this branch recorded the recovery ref
/// in `exact_refs` but never put it in the visible text -- and
/// `validate_omission_rule` requires it in the text. The struct field alone
/// satisfied nobody: an agent cannot expand a ref it cannot see.
#[test]
fn exact_ref_branch_puts_the_ref_where_an_agent_can_see_it() {
    let original = big_original();
    let capsule = finalize_capsule_omission(
        truncated_capsule(),
        &original,
        0,
        Some("tz://blob/abc#B0-100".to_string()),
    )
    .expect("capsule should satisfy the omission rule");
    assert!(
        capsule.text.contains("tz://blob/abc#B0-100"),
        "recovery ref must be visible, not merely recorded: {}",
        capsule.text
    );
    assert!(
        capsule
            .exact_refs
            .iter()
            .any(|r| r == "tz://blob/abc#B0-100")
    );
    capsule
        .validate_omission_rule(&original)
        .expect("omission rule must hold");
}

/// The panic was reached through the ordinary read path, so guard the whole
/// path and not just the helper: a ref without a selector must fall through
/// to the lossy declaration rather than claiming exact recovery.
#[test]
fn ref_without_a_selector_falls_back_to_a_declared_lossy_capsule() {
    let original = big_original();
    let capsule = finalize_capsule_omission(
        truncated_capsule(),
        &original,
        0,
        Some("tz://blob/abc".to_string()),
    )
    .expect("capsule should satisfy the omission rule");
    assert_eq!(capsule.mode, Mode::Lossy);
    assert!(capsule.text.contains("mode=lossy"), "{}", capsule.text);
    assert!(capsule.exact_refs.is_empty());
    capsule
        .validate_omission_rule(&original)
        .expect("omission rule must hold");
}
