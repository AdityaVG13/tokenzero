use super::*;

#[derive(Debug)]
pub(crate) struct PollInterleave {
    pub(crate) length_observed: std::sync::Barrier,
    pub(crate) publication_done: std::sync::Barrier,
}

pub(crate) fn reset_background_job_termination_for_tests() {
    if let Some(registry) = BACKGROUND_JOBS.get() {
        registry.terminating.store(false, Ordering::SeqCst);
    }
}
