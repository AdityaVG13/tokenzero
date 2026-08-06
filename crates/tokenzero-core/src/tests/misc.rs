use super::*;

#[test]
fn long_labels_do_not_crowd_out_tiny_payloads() {
    let c = make_capsule(
        "ok\n",
        Mode::Auto,
        20,
        Some("C:\\Users\\Ada\\AppData\\Local\\Temp\\tokenzero-long-label\\tiny.md"),
    )
    .expect("capsule should satisfy the omission rule");

    assert!(c.text.contains("ok"), "{}", c.text);
    assert!(!c.text.contains("omitted"), "{}", c.text);
    assert!(
        c.visible_tokens <= 20,
        "{} tokens in {}",
        c.visible_tokens,
        c.text
    );
}

#[test]
fn line_range_returns_requested_slice() {
    let text = "fn alpha() {\n  call();\n}\n\nfn beta() {}\n";
    assert_eq!(line_range(text, 1, 2), "fn alpha() {\n  call();");
    assert_eq!(line_range(text, 4, 5), "\nfn beta() {}");
    // Out-of-range returns empty.
    assert_eq!(line_range(text, 100, 200), "");
}

#[test]
fn symbol_block_captures_enclosing_scope_body() {
    let text = "fn alpha() {\n  call();\n}\n\nfn beta() {}\n";
    let block = symbol_block(text, "alpha");
    assert!(block.contains("fn alpha()"), "{}", block);
    assert!(block.contains("call()"), "{}", block);
    // beta is a separate scope — does not include alpha.
    let beta = symbol_block(text, "beta");
    assert!(beta.contains("fn beta()"), "{}", beta);
    assert!(
        !beta.contains("alpha"),
        "should not include alpha: {}",
        beta
    );
    // Non-existent symbol returns empty.
    assert_eq!(symbol_block(text, "gamma"), "");
}
