//! Cooperative wall-clock deadlines for long in-process host ops.
//!
//! CodeMode checks `hard_max_wall_ms` between QuickJS microtasks and before
//! scheduling host work, but a single native call (find walk, expand, session
//! resume) can still burn past the budget. Install an active deadline around
//! host dispatch and checkpoint every N steps inside hot loops.

use std::cell::Cell;
use std::time::Instant;

/// How often hot loops should sample the active wall deadline.
pub const WALL_CHECK_EVERY_N: usize = 32;

/// Plan-start Instant plus the hard wall budget for host-op checkpoints.
#[derive(Clone, Copy, Debug)]
pub struct WallDeadline {
    pub started: Instant,
    pub hard_max_wall_ms: u64,
}

impl WallDeadline {
    pub(crate) fn new(started: Instant, hard_max_wall_ms: u64) -> Self {
        Self {
            started,
            hard_max_wall_ms,
        }
    }

    /// Reconstruct a deadline from elapsed wall ms (CodeMode `started_ms`).
    pub fn from_elapsed_ms(elapsed_ms: u64, hard_max_wall_ms: u64) -> Self {
        let started = Instant::now()
            .checked_sub(std::time::Duration::from_millis(elapsed_ms))
            .unwrap_or_else(Instant::now);
        Self::new(started, hard_max_wall_ms)
    }
}

/// Shared helper: structured error when `started` has exceeded `hard_max_wall_ms`.
///
/// Message shape matches CodeMode `wall_clock_limit_error` hard branch so host
/// aborts and microtask aborts look the same to agents.
pub fn check_wall_deadline(
    started: Instant,
    hard_max_wall_ms: u64,
) -> Option<(String, &'static str)> {
    let elapsed = started.elapsed().as_millis() as u64;
    if elapsed > hard_max_wall_ms {
        Some((
            format!("runtime: hard_max_wall_ms exceeded {hard_max_wall_ms}"),
            "hard wall clock exceeded",
        ))
    } else {
        None
    }
}

thread_local! {
    static ACTIVE_HOST_WALL: Cell<Option<WallDeadline>> = const { Cell::new(None) };
}

fn replace_active(next: Option<WallDeadline>) -> Option<WallDeadline> {
    ACTIVE_HOST_WALL.with(|slot| slot.replace(next))
}

/// Run `f` with `deadline` installed for cooperative host-op checkpoints.
pub fn with_host_wall_deadline<R>(deadline: WallDeadline, f: impl FnOnce() -> R) -> R {
    let previous = replace_active(Some(deadline));
    let result = f();
    replace_active(previous);
    result
}

/// Check the thread-local host-op deadline, if one is installed.
pub fn check_active_wall_deadline() -> Option<(String, &'static str)> {
    ACTIVE_HOST_WALL.with(|slot| {
        slot.get()
            .and_then(|deadline| check_wall_deadline(deadline.started, deadline.hard_max_wall_ms))
    })
}

/// Sample the active deadline every `every_n` steps (and on step 0).
pub fn check_active_wall_deadline_every(
    step: usize,
    every_n: usize,
) -> Option<(String, &'static str)> {
    if every_n == 0 || step % every_n == 0 {
        check_active_wall_deadline()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
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
}
