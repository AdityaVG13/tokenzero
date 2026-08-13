//! TokenZero-specific tests plus the shared ZeroStack test contract.

pub use zero_testkit;
pub use zero_testkit::decode_worker_transcript;

#[cfg(test)]
#[path = "../../../tests/test-support/inline/lib__tests.rs"]
mod tests;
