use super::*;

#[derive(Debug)]
pub(crate) struct PollInterleave {
    pub(crate) length_observed: std::sync::Barrier,
    pub(crate) publication_done: std::sync::Barrier,
}

thread_local! {
    static POLL_INTERLEAVE: std::cell::RefCell<Option<Arc<PollInterleave>>> =
        const { std::cell::RefCell::new(None) };
}

pub(crate) fn set_poll_interleave(
    hook: Option<Arc<PollInterleave>>,
) -> Option<Arc<PollInterleave>> {
    POLL_INTERLEAVE.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), hook))
}

pub(crate) fn wait_poll_interleave() {
    POLL_INTERLEAVE.with(|slot| {
        if let Some(interleave) = slot.borrow().clone() {
            interleave.length_observed.wait();
            interleave.publication_done.wait();
        }
    });
}

pub(crate) fn reset_background_job_termination_for_tests() {
    if let Some(registry) = BACKGROUND_JOBS.get() {
        registry.terminating.store(false, Ordering::SeqCst);
    }
}
