//! TokenZero-specific tests plus the shared ZeroStack test contract.

pub use zero_testkit;
pub use zero_testkit::decode_worker_transcript;

pub mod gauntlet;
pub use gauntlet::{
    CanonicalizationRules, EngineVersions, ExecutionEnvelope, GauntletEngineIdentity,
    GauntletIdentityPair, GauntletOracle, SPEC_TAG_WIRES, SUBJECT_IDENTITY, ScenarioAgreement,
    SpecTagClass, SpecTagWire, assert_distinct, scenario,
};

#[cfg(test)]
#[path = "../../../tests/test-support/inline/lib__tests.rs"]
mod tests;
