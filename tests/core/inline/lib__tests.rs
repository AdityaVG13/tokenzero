use super::*;

#[test]
fn failure_anchor_needles_preserve_multiword_not_ok() {
    assert!(looks_failure_anchor_line("not ok 12 - parser"));
    assert!(looks_failure_anchor_line("panic: parser failed"));
    assert!(!looks_failure_anchor_line("ok 12 - parser"));
    assert!(!looks_failure_anchor_line("not ready yet"));
}

#[test]
fn tz7tse_recovery_adjusted_savings_saturates_visible_plus_recovery() {
    let normal = Accounting {
        raw_tokens: 200,
        visible_tokens: 50,
        recovery_tokens: 30,
        ..Accounting::default()
    };
    assert!((normal.recovery_adjusted_savings_ratio() - 0.60).abs() < f64::EPSILON);

    let overflow = Accounting {
        raw_tokens: 200,
        visible_tokens: usize::MAX,
        recovery_tokens: 1,
        ..Accounting::default()
    };
    assert_eq!(overflow.recovery_adjusted_savings_ratio(), 0.0);

    let both_max = Accounting {
        raw_tokens: usize::MAX,
        visible_tokens: usize::MAX,
        recovery_tokens: usize::MAX,
        ..Accounting::default()
    };
    assert_eq!(both_max.recovery_adjusted_savings_ratio(), 0.0);
}

#[test]
fn tz73yc_m_rec_counts_visible_recovery_overlap() {
    // Exact-expand bytes that were also shown count in both masses.
    let overlap = Accounting {
        raw_tokens: 200,
        visible_tokens: 50,
        recovery_tokens: 50,
        ..Accounting::default()
    };
    assert!(
        (overlap.recovery_adjusted_savings_ratio() - 0.50).abs() < f64::EPSILON,
        "used = visible+recovery = 100, savings = 0.50"
    );
    assert!(
        (overlap.visible_savings_ratio() - 0.75).abs() < f64::EPSILON,
        "visible-only savings stay 0.75; M_rec is the more conservative figure"
    );
}
