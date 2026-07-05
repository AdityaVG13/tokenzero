use super::*;

#[test]
fn long_labels_do_not_crowd_out_tiny_payloads() {
    let c = make_capsule(
        "ok\n",
        Mode::Auto,
        20,
        Some("C:\\Users\\Ada\\AppData\\Local\\Temp\\tokenzero-long-label\\tiny.md"),
    );

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
fn line_and_symbol_selectors_work() {
    let text = "fn alpha() {\n  call();\n}\n\nfn beta() {}\n";
    assert_eq!(line_range(text, 1, 2), "fn alpha() {\n  call();");
    assert!(symbol_block(text, "alpha").contains("call"));
}
