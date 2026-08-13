use super::*;

#[test]
fn parse_enabled_accepts_truthy() {
    assert!(parse_enabled("1"));
    assert!(parse_enabled("true"));
    assert!(parse_enabled("YES"));
    assert!(parse_enabled(" on "));
    assert!(!parse_enabled("0"));
    assert!(!parse_enabled("false"));
    assert!(!parse_enabled(""));
}

#[test]
fn stage_off_returns_body() {
    // Flag state may already be cached by other tests in the same process;
    // body must still execute either way.
    let v = _profile_read_inner(|| 42u32);
    assert_eq!(v, 42);
}
