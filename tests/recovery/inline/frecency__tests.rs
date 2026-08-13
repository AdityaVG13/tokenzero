use super::*;

#[test]
fn tgzc0_half_life_and_burst_compression() {
    assert_eq!(burst_compress(0), 0.0);
    assert_eq!(burst_compress(1), 0.0);
    assert_eq!(burst_compress(2), 1.0);
    assert_eq!(burst_compress(8), 3.0);
    assert!((decay(0) - 1.0).abs() < 1e-12);
    assert!((decay(HALF_LIFE_SECS) - 0.5).abs() < 1e-12);
    let hot = score(8, 0);
    let cold = score(8, HALF_LIFE_SECS);
    assert!(hot > cold, "same burst must cool over three days");
    assert!(score(8, 0) > score(2, 0), "larger burst ranks hotter");
}

#[test]
fn tgzc0_order_log_keeps_fifo_when_each_ref_appears_once() {
    let order = ["tz://a", "tz://b", "tz://c"].map(str::to_string);
    // Singletons all burst-compress to 0; FIFO is the equal-score tie.
    assert_eq!(score_from_order(&order, "tz://a"), 0.0);
    assert_eq!(score_from_order(&order, "tz://c"), 0.0);
    assert_eq!(coldest(&order, |_| true), Some("tz://a"));
}

#[test]
fn tgzc0_reputs_make_a_ref_hotter_than_an_older_singleton() {
    let order = ["tz://cold", "tz://hot", "tz://hot"].map(str::to_string);
    assert!(score_from_order(&order, "tz://hot") > score_from_order(&order, "tz://cold"));
    assert_eq!(coldest(&order, |_| true), Some("tz://cold"));
}
