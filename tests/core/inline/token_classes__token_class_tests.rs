use super::*;

#[test]
fn same_class_arithmetic_and_sum() {
    let mut visible = Tok::<Visible>::new(40);
    visible += Tok::new(2);
    assert_eq!(visible.get(), 42);
    assert_eq!((visible - Tok::new(2)).get(), 40);

    let parts = [Tok::<Raw>::new(1), Tok::new(2), Tok::new(3)];
    let total: Tok<Raw> = parts.iter().sum();
    assert_eq!(total, Tok::new(6));
}

#[test]
fn saturating_and_checked_edges() {
    let max = Tok::<BilledIn>::new(u64::MAX);
    assert_eq!(max.checked_add(Tok::new(1)), None);
    assert_eq!(max.saturating_add(Tok::new(1)), max);
    assert_eq!(Tok::<BilledIn>::ZERO.saturating_sub(Tok::new(9)).get(), 0);
}

#[test]
fn tz7tse_operators_saturate_at_integer_boundaries() {
    let max = Tok::<Visible>::new(u64::MAX);
    let one = Tok::<Visible>::new(1);
    assert_eq!((max + one).get(), u64::MAX);
    let mut acc = max;
    acc += one;
    assert_eq!(acc.get(), u64::MAX);
    assert_eq!((Tok::<Visible>::ZERO - one).get(), 0);
    let mut zero = Tok::<Visible>::ZERO;
    zero -= one;
    assert_eq!(zero.get(), 0);

    let parts = [max, one];
    let total: Tok<Visible> = parts.into_iter().sum();
    assert_eq!(total.get(), u64::MAX);
    let borrowed: Tok<Visible> = parts.iter().sum();
    assert_eq!(borrowed.get(), u64::MAX);

    assert_eq!(max.checked_add(one), None);
}

#[test]
fn cast_is_explicit_and_value_preserving() {
    let raw = Tok::<Raw>::new(641);
    let reclassified: Tok<Visible> = raw.cast();
    assert_eq!(reclassified.get(), 641);
}

#[test]
fn display_and_debug_carry_the_class_name() {
    let cached = Tok::<Cached>::new(7);
    assert_eq!(cached.to_string(), "7 cached");
    assert_eq!(format!("{cached:?}"), "Tok<cached>(7)");
    assert_eq!(Tok::<BilledOut>::new(3).to_string(), "3 billed_out");
}

#[test]
fn typed_savings_ratio_matches_untyped() {
    let raw = Tok::<Raw>::new(641);
    let visible = Tok::<Visible>::new(331);
    let expected = crate::tokens::savings_ratio(641, 331);
    assert!((savings_ratio_typed(raw, visible) - expected).abs() < f64::EPSILON);
}
