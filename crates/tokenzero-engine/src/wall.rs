//! Cooperative wall-clock deadlines for long in-process host ops.
//!
//! CodeMode checks `hard_max_wall_ms` between QuickJS microtasks and before
//! scheduling host work, but a single native call (find walk, expand, session
//! resume) can still burn past the budget. Install an active deadline around
//! host dispatch and checkpoint every N steps inside hot loops.

use std::cell::{Cell, RefCell};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
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
    static ACTIVE_HOST_CANCEL: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

fn replace_active(next: Option<WallDeadline>) -> Option<WallDeadline> {
    ACTIVE_HOST_WALL.with(|slot| slot.replace(next))
}

fn replace_active_cancel(next: Option<Arc<AtomicBool>>) -> Option<Arc<AtomicBool>> {
    ACTIVE_HOST_CANCEL.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), next))
}

fn with_host_controls<R>(
    deadline: WallDeadline,
    cancel: Option<Arc<AtomicBool>>,
    f: impl FnOnce() -> R,
) -> R {
    // Restore both controls even when `f` panics so thread-local state never
    // leaks into unrelated host work.
    struct RestoreActive {
        deadline: Option<WallDeadline>,
        cancel: Option<Arc<AtomicBool>>,
    }
    impl Drop for RestoreActive {
        fn drop(&mut self) {
            replace_active(self.deadline.take());
            replace_active_cancel(self.cancel.take());
        }
    }
    let _restore = RestoreActive {
        deadline: replace_active(Some(deadline)),
        cancel: replace_active_cancel(cancel),
    };
    f()
}

/// Run `f` with `deadline` installed for cooperative host-op checkpoints.
pub fn with_host_wall_deadline<R>(deadline: WallDeadline, f: impl FnOnce() -> R) -> R {
    with_host_controls(deadline, None, f)
}

/// Run raw-worker host work with both its wall deadline and session cancel
/// flag installed for the same cooperative checkpoints.
pub(crate) fn with_host_wall_deadline_and_cancel<R>(
    deadline: WallDeadline,
    cancel: Arc<AtomicBool>,
    f: impl FnOnce() -> R,
) -> R {
    with_host_controls(deadline, Some(cancel), f)
}

/// Check the thread-local host-op deadline or cancellation flag, if installed.
pub fn check_active_wall_deadline() -> Option<(String, &'static str)> {
    let cancelled = ACTIVE_HOST_CANCEL.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::SeqCst))
    });
    if cancelled {
        return Some(("runtime: operation cancelled".into(), "operation cancelled"));
    }
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
}
