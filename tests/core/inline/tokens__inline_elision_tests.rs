use super::*;

fn plain_marker(recovery_ref: Option<&str>) -> String {
    visible_budget_marker(recovery_ref)
}

#[test]
fn inline_elision_plain_marker_is_head_visible() {
    let marker = plain_marker(None);
    let budget = count_tokens(&format!("{marker}\nalpha\n"));
    let text = format!("alpha\n{}", "payload ".repeat(100));
    let out = enforce_token_budget(&text, budget);
    assert_eq!(out.lines().next(), Some(marker.as_str()));
}

#[test]
fn inline_elision_respects_budget_when_marker_fits() {
    let marker = plain_marker(None);
    let budget = count_tokens(&format!("{marker}\nfirst\nsecond\n"));
    let text = format!("first\nsecond\n{}", "tail ".repeat(100));
    let out = enforce_token_budget(&text, budget);
    assert!(count_tokens(&out) <= budget, "{out:?}");
}

#[test]
fn inline_elision_keeps_recovery_ref_explicit() {
    let recovery = "tz://blob/0123456789abcdef";
    let marker = plain_marker(Some(recovery));
    let out = enforce_token_budget_with_ref(
        &"payload ".repeat(100),
        count_tokens(&marker),
        Some(recovery),
    );
    assert!(out.starts_with(VISIBLE_BUDGET_LOSSY_DECLARATION));
    assert!(out.contains(recovery));
}

#[test]
fn inline_elision_json_object_is_parseable_and_keeps_whole_values() {
    let text = format!(
        r#"{{"a":{{"nested":[1,2,3]}},"b":"kept","z":"{}"}}"#,
        "tail ".repeat(100)
    );
    let maximal = elide_top_level_json(&text, usize::MAX, Some("tz://object")).unwrap();
    let budget = count_tokens(&maximal);
    let out = enforce_token_budget_with_ref(&text, budget, Some("tz://object"));
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(out.starts_with("{\"__tokenzero_elision__\":{"));
    assert_eq!(parsed["a"]["nested"], serde_json::json!([1, 2, 3]));
    assert_eq!(parsed["b"], "kept");
    assert!(parsed.get("z").is_none());
    assert!(count_tokens(&out) <= budget);
}

#[test]
fn inline_elision_json_array_is_parseable_and_keeps_whole_values() {
    let text = format!(
        r#"[{{"nested":[1,2,3]}},["whole",{{"value":4}}],"{}"]"#,
        "tail ".repeat(100)
    );
    let maximal = elide_top_level_json(&text, usize::MAX, Some("tz://array")).unwrap();
    let budget = count_tokens(&maximal);
    let out = enforce_token_budget_with_ref(&text, budget, Some("tz://array"));
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let items = parsed.as_array().unwrap();
    assert!(out.starts_with("[{\"__tokenzero_elision__\":{"));
    assert_eq!(items[1]["nested"], serde_json::json!([1, 2, 3]));
    assert_eq!(items[2], serde_json::json!(["whole", {"value": 4}]));
    assert_eq!(items.len(), 3);
    assert!(count_tokens(&out) <= budget);
}

#[test]
fn inline_elision_reserved_key_collision_falls_back_safely() {
    let text = format!(
        r#"{{"__tokenzero_elision__":{{"user":true}},"payload":"{}"}}"#,
        "tail ".repeat(100)
    );
    let marker = plain_marker(None);
    let out = enforce_token_budget(&text, count_tokens(&marker));
    assert_eq!(out, marker);
    assert!(serde_json::from_str::<serde_json::Value>(&out).is_err());
}

#[test]
fn inline_elision_tiny_budget_uses_marker_correctness_floor() {
    let marker = plain_marker(None);
    let budget = count_tokens(&marker).saturating_sub(1);
    let out = enforce_token_budget(&"payload ".repeat(100), budget);
    assert_eq!(out, marker);
    assert!(count_tokens(&out) > budget);

    let json_out = enforce_token_budget(r#"{"payload":"long long long"}"#, 0);
    assert_eq!(json_out, VISIBLE_BUDGET_LOSSY_DECLARATION);
}

#[test]
fn inline_elision_nonlossy_output_is_byte_identical() {
    let text = "  exact\nJSON-ish: { \"x\": 1 }\n";
    assert_eq!(enforce_token_budget(text, count_tokens(text)), text);
}

#[test]
fn inline_elision_utf8_retains_only_whole_lines() {
    let marker = plain_marker(None);
    let retained = "αβ🙂\n";
    let budget = count_tokens(&format!("{marker}\n{retained}"));
    let text = format!("{retained}{}", "終わり ".repeat(100));
    let out = enforce_token_budget(&text, budget);
    assert_eq!(out, format!("{marker}\n{retained}"));
    assert!(out.is_char_boundary(out.len()));
    assert!(count_tokens(&out) <= budget);
}
