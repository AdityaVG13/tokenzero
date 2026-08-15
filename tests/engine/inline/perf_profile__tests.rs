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

#[test]
fn attribution_pct_fails_closed_when_total_zero() {
    assert_eq!(attribution_pct(0, 0), Err(HotPathEmptyTotal));
    assert_eq!(attribution_pct(1, 0), Err(HotPathEmptyTotal));
    let empty = HotPathProfileSnapshot::default();
    assert_eq!(empty.attribution_pct(HotPathName::Expand), Err(HotPathEmptyTotal));
    assert_eq!(
        HotPathProfileCard::try_from_snapshot(empty),
        Err(HotPathEmptyTotal)
    );
}

#[test]
fn mt8_floor_holds_when_named_path_ran_at_point_one_pct() {
    // 1 / 1000 = 0.1% exactly -- the MT8 showable floor for a path that ran.
    let snap = HotPathProfileSnapshot {
        expand: 1,
        read: 999,
        capsule: 0,
    };
    let pct = snap.attribution_pct(HotPathName::Expand).expect("total > 0");
    assert!((pct - MT8_MIN_ATTRIBUTION_PCT).abs() < 1e-12);
    let card = HotPathProfileCard::try_from_snapshot(snap).expect("total > 0");
    assert!(card.meets_mt8_floor(HotPathName::Expand));
    assert!(card.meets_mt8_floor(HotPathName::Read));
    assert!(!card.meets_mt8_floor(HotPathName::Capsule));
}

#[test]
fn hot_path_profile_card_reads_snapshot_delta_after_n_calls() {
    const N: u64 = 10;
    let before = hot_path_snapshot();
    for _ in 0..N {
        note_hot_path_read();
        note_hot_path_expand();
    }
    note_hot_path_capsule();
    let after = hot_path_snapshot();
    let delta = after.saturating_sub(before);
    assert_eq!(delta.read, N);
    assert_eq!(delta.expand, N);
    assert_eq!(delta.capsule, 1);
    assert_eq!(delta.total(), 2 * N + 1);

    // Profile card consumes HotPathProfileSnapshot counts (not stderr).
    let card = HotPathProfileCard::try_from_snapshot(delta).expect("delta total > 0");
    assert_eq!(card.total, 2 * N + 1);
    let read_pct = card.attribution_pct(HotPathName::Read);
    let expand_pct = card.attribution_pct(HotPathName::Expand);
    let capsule_pct = card.attribution_pct(HotPathName::Capsule);
    let expected_read = 100.0 * (N as f64) / (card.total as f64);
    let expected_capsule = 100.0 * 1.0 / (card.total as f64);
    assert!((read_pct - expected_read).abs() < 1e-12);
    assert!((expand_pct - expected_read).abs() < 1e-12);
    assert!((capsule_pct - expected_capsule).abs() < 1e-12);
    assert!(card.meets_mt8_floor(HotPathName::Read));
    assert!(card.meets_mt8_floor(HotPathName::Expand));
    assert!(card.meets_mt8_floor(HotPathName::Capsule));

    let snap_json = delta.to_export_json();
    assert!(snap_json.contains("\"expand\":10"));
    assert!(snap_json.contains("\"read\":10"));
    assert!(snap_json.contains("\"capsule\":1"));
    assert!(snap_json.contains("\"total\":21"));
    let card_json = card.to_export_json();
    assert!(
        card_json.contains("\"attribution\":\"enter_count\""),
        "MT8 percents are enter counts, not wall-time: {card_json}"
    );
    assert!(card_json.contains("\"expand_pct\":"));
    assert!(card_json.contains("\"mt8_min_pct\":0.1"));
    assert!(
        !card_json.contains("wall") && !card_json.contains("latency"),
        "{card_json}"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&card_json).expect("card JSON must parse");
    assert_eq!(parsed["attribution"], "enter_count");
    let expand_pct = parsed["expand_pct"].as_f64().expect("expand_pct");
    let total = parsed["total"].as_u64().expect("total");
    let expand = parsed["expand"].as_u64().expect("expand");
    assert!((expand_pct - 100.0 * expand as f64 / total as f64).abs() < 1e-5);
}
