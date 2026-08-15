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

#[test]
fn hot_path_snapshot_counts_expand_read_capsule() {
    let before = hot_path_snapshot();
    note_hot_path_expand();
    note_hot_path_read();
    note_hot_path_capsule();
    note_dispatch_hot_path("tz_expand");
    note_dispatch_hot_path("tz_read");
    note_dispatch_hot_path("tz_find");
    let after = hot_path_snapshot();
    assert_eq!(after.expand, before.expand + 2);
    assert_eq!(after.read, before.read + 2);
    assert_eq!(after.capsule, before.capsule + 1);
}
