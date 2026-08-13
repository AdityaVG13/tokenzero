use super::*;
#[test]
fn seeded_schedules_are_bitwise_replayable() {
    let a = DeterministicSchedule::generate(0x5eed, 32);
    let bytes = a.replay_bytes();
    let b: DeterministicSchedule = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(bytes, b.replay_bytes());
    assert_eq!(a.replay(|i| i * i).unwrap(), b.replay(|i| i * i).unwrap());
}
#[test]
fn elle_style_mint_and_alias_cas_histories() {
    let h = vec![
        HistoryEvent {
            process: 1,
            invoke: 1,
            complete: 4,
            op: HistoryOp::Mint { ref_id: "r".into() },
            success: true,
        },
        HistoryEvent {
            process: 2,
            invoke: 2,
            complete: 5,
            op: HistoryOp::AliasCas {
                alias: "a".into(),
                expected: None,
                new: "r".into(),
            },
            success: true,
        },
        HistoryEvent {
            process: 3,
            invoke: 6,
            complete: 7,
            op: HistoryOp::AliasCas {
                alias: "a".into(),
                expected: None,
                new: "x".into(),
            },
            success: false,
        },
    ];
    assert!(check_linearizable(&h).is_ok());
    let mut bad = h;
    bad[2].success = true;
    assert!(check_linearizable(&bad).is_err());
}
