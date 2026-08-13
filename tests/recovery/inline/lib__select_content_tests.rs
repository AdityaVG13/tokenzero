use super::*;

#[test]
fn selector_line_windows_override_existing_line_args() {
    let content = "one\ntwo\nthree\nfour\nfive\n".to_string();

    assert_eq!(
        select_content(
            content.clone(),
            Some("range:2-3"),
            Some(5),
            Some(5),
            None,
            None
        ),
        "two\nthree\n"
    );
    assert_eq!(
        select_content(
            content.clone(),
            Some("lines:L3-L4"),
            Some(5),
            Some(5),
            None,
            None
        ),
        "three\nfour\n"
    );
    assert_eq!(
        select_content(
            content.clone(),
            Some("line:4"),
            Some(5),
            Some(5),
            None,
            None
        ),
        "four\n"
    );
    assert_eq!(
        select_content(content, Some("around:3:1"), Some(5), Some(5), None, None),
        "two\nthree\nfour\n"
    );
}
