//! TransformFamily metamorphic suite (Predicate / Projection / Structural / Literal).
//!
//! Each family applies a TokenZero-owned render/search transform, then an inverse
//! (or documented no-op), and asserts a named invariant. A planted mutation that
//! violates the invariant is asserted to fail so the property stays regression-
//! sensitive.
use super::*;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Predicate — shell_family classification
// Bug class: predicate shells (test/[ / [[ / cmp) mislabeled as "generic",
// so decision/policy paths treat boolean probes as unstructured chatter.
// ---------------------------------------------------------------------------

const PREDICATE_BASICS: &[&str] = &["test", "[", "[[", "cmp"];

fn predicate_command_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("test -f /tmp/x".to_string()),
        Just("test -z ''".to_string()),
        Just("[ -d . ]".to_string()),
        Just("[[ -n foo ]]".to_string()),
        Just("cmp a.bin b.bin".to_string()),
        Just("test".to_string()),
    ]
}

fn wrap_ws(command: &str) -> String {
    format!("  {command}  \t")
}

/// Planted mutation: collapse every predicate basename to "generic".
fn mutant_predicate_as_generic(command: &str, stdout: &str, stderr: &str) -> String {
    let family = shell_family(command, stdout, stderr);
    if family == "predicate" {
        "generic".to_string()
    } else {
        family
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Metamorphic: wrapping whitespace / trailing no-op padding does not change
    /// the family label. Inverse: identity on the family string.
    #[test]
    fn transform_family_predicate_whitespace_noop_preserves_label(
        command in predicate_command_strategy(),
        stdout in prop::option::of("[ -~]{0,40}"),
        stderr in prop::option::of("[ -~]{0,20}"),
    ) {
        let stdout = stdout.unwrap_or_default();
        let stderr = stderr.unwrap_or_default();
        let base = shell_family(&command, &stdout, &stderr);
        prop_assert_eq!(&base, "predicate");
        // Documented no-op inverse: family label is already the invariant.
        prop_assert_eq!(shell_family(&wrap_ws(&command), &stdout, &stderr), base);
        prop_assert_eq!(
            shell_family(&format!("{command} "), &stdout, &stderr),
            "predicate"
        );
        let first = command.split_whitespace().next().unwrap_or("");
        prop_assert!(PREDICATE_BASICS.contains(&first) || first == "test");
    }
}

#[test]
fn transform_family_predicate_planted_generic_mutation_fails() {
    let command = "test -f /tmp/x";
    assert_eq!(shell_family(command, "", ""), "predicate");
    assert_eq!(
        mutant_predicate_as_generic(command, "", ""),
        "generic",
        "planted mutation must misclassify so the property can catch it"
    );
    assert_ne!(
        mutant_predicate_as_generic(command, "", ""),
        shell_family(command, "", ""),
        "bug class: predicate shells collapsed to generic"
    );
}

// ---------------------------------------------------------------------------
// Projection — search_shell_view stdout projection
// Bug class: search view drops sample matches (empty projection) while stdout
// still carries hit lines, hiding recoverable search evidence.
// ---------------------------------------------------------------------------

fn search_stdout_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-zA-Z0-9_./:-]{1,24}", 1..12usize).prop_map(|lines| lines.join("\n"))
}

fn project_search(stdout: &str) -> String {
    search_shell_view("rg needle src/", stdout, "")
}

/// Re-apply the same projection to the sample_matches payload (inverse shape:
/// extract projected lines, project again). Idempotent when stderr is empty and
/// the match set fits the sample limit.
fn extract_sample_match_lines(view: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_samples = false;
    for line in view.lines() {
        if line == "sample_matches:" {
            in_samples = true;
            continue;
        }
        if in_samples {
            if let Some(rest) = line.strip_prefix("- ") {
                out.push(rest.to_string());
            } else if line.starts_with("...") {
                break;
            } else if !line.is_empty() && !line.starts_with('-') {
                break;
            }
        }
    }
    out
}

/// Planted mutation: drop all sample matches from an otherwise valid view.
fn mutant_projection_drop_matches(stdout: &str) -> String {
    let mut view = project_search(stdout);
    if let Some(idx) = view.find("sample_matches:") {
        view.truncate(idx);
        if !view.ends_with('\n') {
            view.push('\n');
        }
    }
    view
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Metamorphic: projecting stdout, then projecting the extracted sample
    /// matches again, yields the same sample set (idempotent projection).
    #[test]
    fn transform_family_projection_search_view_idempotent(
        stdout in search_stdout_strategy(),
    ) {
        let first = project_search(&stdout);
        let samples = extract_sample_match_lines(&first);
        prop_assert!(
            !samples.is_empty(),
            "search projection must surface stdout lines as matches: {first}"
        );
        prop_assert!(
            first.contains(&format!("matches_seen: {}", samples.len())),
            "{first}"
        );
        let second = project_search(&samples.join("\n"));
        prop_assert_eq!(
            extract_sample_match_lines(&second),
            samples,
            "bug class: non-idempotent projection / dropped matches"
        );
    }
}

#[test]
fn transform_family_projection_planted_drop_matches_fails() {
    let stdout = "src/a.rs:1:hit\nsrc/b.rs:2:hit";
    let good = project_search(stdout);
    let bad = mutant_projection_drop_matches(stdout);
    assert!(
        !extract_sample_match_lines(&good).is_empty(),
        "current code must keep matches"
    );
    assert!(
        extract_sample_match_lines(&bad).is_empty(),
        "planted mutation must drop matches"
    );
    assert_ne!(good, bad);
}

// ---------------------------------------------------------------------------
// Structural — JSON elision / enforce_token_budget
// Bug class: budget path emits a truncated non-JSON prefix of a structured
// payload (invalid JSON), breaking parse-round-trip recovery consumers.
// ---------------------------------------------------------------------------

fn json_object_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(("[a-z]{1,6}", "[a-z0-9 ]{0,24}"), 1..8usize).prop_map(|entries| {
        let mut map = serde_json::Map::new();
        for (k, v) in entries {
            map.insert(k, serde_json::Value::String(v));
        }
        // Pad so budgets often force elision without leaving the object empty.
        map.insert(
            "pad".to_string(),
            serde_json::Value::String("word ".repeat(40)),
        );
        serde_json::Value::Object(map).to_string()
    })
}

fn json_array_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z0-9 ]{1,16}", 2..10usize).prop_map(|items| {
        let mut values: Vec<serde_json::Value> =
            items.into_iter().map(serde_json::Value::String).collect();
        values.push(serde_json::Value::String("pad ".repeat(40)));
        serde_json::Value::Array(values).to_string()
    })
}

fn structural_budget_strategy() -> impl Strategy<Value = usize> {
    prop_oneof![0usize..16, 16..64usize, 64..256usize, Just(usize::MAX)]
}

fn is_documented_structural_sentinel(out: &str) -> bool {
    out == VISIBLE_BUDGET_LOSSY_DECLARATION || out.starts_with(VISIBLE_BUDGET_LOSSY_DECLARATION)
}

/// Inverse for Structural: parse JSON (or accept the documented plain sentinel).
fn structural_inverse_ok(out: &str) -> bool {
    is_documented_structural_sentinel(out) || serde_json::from_str::<serde_json::Value>(out).is_ok()
}

/// Planted mutation: emit a truncated non-JSON prefix of the input.
fn mutant_structural_truncated_prefix(text: &str, _budget: usize) -> String {
    let keep = (text.len() / 2).max(1).min(text.len().saturating_sub(1));
    text[..keep].to_string()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Metamorphic: valid JSON in stays valid JSON out (elided sentinel object/
    /// array) or the documented plain lossy marker. Inverse: JSON parse.
    #[test]
    fn transform_family_structural_json_elision_stays_parseable(
        text in prop_oneof![json_object_strategy(), json_array_strategy()],
        budget in structural_budget_strategy(),
    ) {
        prop_assert!(serde_json::from_str::<serde_json::Value>(&text).is_ok());
        let out = enforce_token_budget(&text, budget);
        prop_assert!(
            structural_inverse_ok(&out),
            "bug class: truncated non-JSON prefix under budget; out={out:?}"
        );
        if !is_documented_structural_sentinel(&out) {
            let parsed: serde_json::Value =
                serde_json::from_str(&out).expect("inverse parse");
            prop_assert!(parsed.is_object() || parsed.is_array());
        }
    }
}

#[test]
fn transform_family_structural_planted_truncated_prefix_fails() {
    let text = r#"{"a":"alpha","b":"bravo","pad":"word word word word word word word word"}"#;
    let budget = 20;
    let good = enforce_token_budget(text, budget);
    assert!(
        structural_inverse_ok(&good),
        "current code must stay parseable or sentinel: {good:?}"
    );
    let bad = mutant_structural_truncated_prefix(text, budget);
    assert!(
        !structural_inverse_ok(&bad),
        "planted truncation must yield non-JSON: {bad:?}"
    );
}

// Literal / tz_find lives with the in-process matcher `collect_search` uses:
// `tests/engine/inline/collect__tests.rs` (`transform_family_literal_*`).
// Core must not fake that family via stdout projection.
