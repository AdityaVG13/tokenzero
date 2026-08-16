//! TokenZero-specific tests plus the shared ZeroStack test contract.

pub use zero_testkit;
pub use zero_testkit::decode_worker_transcript;

pub mod gauntlet;
pub mod parity_taxonomy;
pub use gauntlet::{
    assert_distinct, fragment_reason_class_matches, scenario, CanonicalizationRules, CrashBoundary,
    CrashWindowDriver, CrashWindowKind, EngineVersions, ExecutionEnvelope, GauntletEngineIdentity,
    GauntletIdentityPair, GauntletOracle, ScenarioAgreement, SpecTagClass, SpecTagWire,
    SPEC_TAG_WIRES, SUBJECT_IDENTITY,
};
pub use parity_taxonomy::{
    truncate_score, Feature, FeatureId, FeatureUniverse, LoaderError, ParityStatus, Stats,
};

#[cfg(test)]
#[path = "../../../tests/test-support/inline/lib__tests.rs"]
mod tests;
