use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DurableCommitFailPoint {
    BeforePersist,
    BeforeFileSync,
    BeforeDirectorySync,
}

thread_local! {
    pub(crate) static DURABLE_COMMIT_FAIL_POINT: std::cell::Cell<Option<DurableCommitFailPoint>> =
        const { std::cell::Cell::new(None) };
}

pub(crate) fn fail_durable_commit_at(point: DurableCommitFailPoint) -> Result<(), RecoveryError> {
    if DURABLE_COMMIT_FAIL_POINT.with(|configured| configured.get() == Some(point)) {
        return Err(io::Error::other("durable commit fault injected").into());
    }
    Ok(())
}

thread_local! {
    static REF_INDEX_TEST_OVERRIDE: std::cell::RefCell<Option<(bool, PathBuf)>> =
        const { std::cell::RefCell::new(None) };
}

pub(crate) fn set_ref_index_test_override(
    value: Option<(bool, PathBuf)>,
) -> Option<(bool, PathBuf)> {
    REF_INDEX_TEST_OVERRIDE.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), value))
}

pub(crate) fn ref_index_test_override() -> Option<(bool, PathBuf)> {
    REF_INDEX_TEST_OVERRIDE.with(|slot| slot.borrow().clone())
}
