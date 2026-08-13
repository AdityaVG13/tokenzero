use super::*;

#[test]
fn equals_prefixed_diagnostic_is_a_continuation() {
    assert!(is_critical_continuation_line("= short test summary info ="));
}
