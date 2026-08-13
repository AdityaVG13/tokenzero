use super::*;
use std::time::Duration;

#[test]
fn check_wall_deadline_past_started_fails_fast() {
    let started = Instant::now()
        .checked_sub(Duration::from_millis(50))
        .expect("instant subtraction");
    let err = check_wall_deadline(started, 0).expect("past deadline must fail");
    assert!(
        err.0.contains("hard_max_wall_ms exceeded 0"),
        "unexpected message: {}",
        err.0
    );
    assert_eq!(err.1, "hard wall clock exceeded");
}

#[test]
fn check_wall_deadline_fresh_started_ok() {
    assert!(check_wall_deadline(Instant::now(), 60_000).is_none());
}

#[test]
fn active_deadline_checkpoints_inside_with_block() {
    let started = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("instant subtraction");
    let deadline = WallDeadline::new(started, 0);
    let hit = with_host_wall_deadline(deadline, || check_active_wall_deadline_every(0, 32));
    assert!(hit.is_some());
    assert!(check_active_wall_deadline().is_none());
}

#[test]
fn active_cancel_interrupts_the_same_host_checkpoints() {
    let cancel = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&cancel);
    let hit = with_host_wall_deadline_and_cancel(
        WallDeadline::new(Instant::now(), 60_000),
        cancel,
        || {
            observed.store(true, Ordering::SeqCst);
            check_active_wall_deadline_every(0, 32)
        },
    )
    .expect("cancelled host work must stop at its next checkpoint");
    assert_eq!(hit.1, "operation cancelled");
    assert!(check_active_wall_deadline().is_none());
}
