
use super::*;

#[test]
fn summarize_lines_usize_max_keeps_whole_text() {
    let text = "a\nb\nc\nd\ne\nf\ng\nh";
    assert_eq!(
        summarize_lines(text, usize::MAX, 8, ""),
        text,
        "head=usize::MAX must not wrap and panic on lines[..head]"
    );
    assert_eq!(summarize_lines(text, 2, usize::MAX, ""), text);
    assert_eq!(
        summarize_lines(text, usize::MAX, usize::MAX, "p:"),
        format!("p:{text}")
    );
}

#[test]
fn critical_lines_max_radius_keeps_the_hit() {
    let text = "ok\nerror boom\nok";
    let kept = critical_lines(text, usize::MAX);
    assert!(
        kept.contains("error boom"),
        "radius=usize::MAX must saturate to the whole file, not wrap the window empty: {kept:?}"
    );
}

#[test]
fn error_block_zero_radius_keeps_only_the_hit() {
    let text = "ok\nerror boom\nok";
    let kept = error_block(text, 0);
    assert!(
        kept.contains("error boom"),
        "radius=0 must keep the named hit: {kept:?}"
    );
    assert!(
        !kept.lines().any(|line| line == "ok"),
        "radius=0 must omit neighbors, not wrap them in: {kept:?}"
    );
    assert!(
        kept.contains("omitted 1 lines"),
        "omitted neighbors must be marked, not silently dropped: {kept:?}"
    );
}

#[test]
fn dedupe_max_context_keeps_whole_buffer() {
    let text = "a\nb\nc";
    assert_eq!(
        dedupe_lines_impl(text, usize::MAX, false),
        text,
        "context=usize::MAX must not wrap context*2 and panic"
    );
}
