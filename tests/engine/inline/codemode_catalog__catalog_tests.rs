use super::*;

#[test]
fn find_grep_prose_matches_engine_semantics_not_inverted() {
    // wk0t.1 (F-005): find is literal-only; grep is regex under the
    // ripgrep backend. The catalog previously claimed the inverse.
    let find = &describe_method("zero.find")["description"]
        .as_str()
        .unwrap()
        .to_lowercase();
    let grep = &describe_method("zero.grep")["description"]
        .as_str()
        .unwrap()
        .to_lowercase();
    assert!(find.contains("literal"), "find prose: {find}");
    assert!(
        !find.contains("regex or literal") && find.contains("never regex"),
        "find must not advertise regex: {find}"
    );
    assert!(grep.contains("regex"), "grep prose names regex: {grep}");
    assert!(
        grep.contains("literal substring otherwise"),
        "grep prose names the literal fallback: {grep}"
    );
}

#[test]
fn read_contract_publishes_raw_fresh_and_matches_schema() {
    let method = describe_method("zero.read");
    let signature = method["signature"].as_str().unwrap();
    let example = method["example"].as_str().unwrap();
    let properties = method["inputSchema"]["properties"].as_object().unwrap();
    for field in [
        "mode",
        "start_line",
        "end_line",
        "raw",
        "fresh",
        "max_files",
        "max_visible_tokens",
    ] {
        assert!(
            signature.contains(field),
            "signature missing {field}: {signature}"
        );
        assert!(properties.contains_key(field), "schema missing {field}");
    }
    assert!(example.contains("raw: true"), "example: {example}");
    assert!(example.contains("fresh: true"), "example: {example}");
}

#[test]
fn shell_contract_recommends_exact_mode_without_advertising_raw() {
    let method = describe_method("zero.shell");
    let signature = method["signature"].as_str().unwrap();
    let example = method["example"].as_str().unwrap();
    assert!(!signature.contains("raw?"), "signature: {signature}");
    for field in [
        "mode?",
        "rewrite?",
        "no_rewrite?",
        "stdin?",
        "timeout_ms?",
        "timeout_seconds?",
    ] {
        assert!(
            signature.contains(field),
            "signature missing {field}: {signature}"
        );
    }
    assert!(signature.contains("string[]"), "signature: {signature}");
    assert!(example.contains(r#"mode: "exact""#), "example: {example}");
    let properties = method["inputSchema"]["properties"].as_object().unwrap();
    assert!(properties.contains_key("argv"));
    assert!(!properties.contains_key("raw"));
}

#[test]
fn job_signature_publishes_long_poll_cursor_and_backoff_contract() {
    let method = describe_method("zero.token.job");
    let signature = method["signature"].as_str().unwrap();
    for field in [
        "waitMs",
        "since",
        "tailBytes",
        "cursor",
        "version",
        "nextPollMs",
    ] {
        assert!(signature.contains(field), "missing {field}: {signature}");
    }
}
