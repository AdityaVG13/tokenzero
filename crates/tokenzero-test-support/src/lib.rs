//! TokenZero-specific tests plus the shared ZeroStack test contract.

pub mod gauntlet;
pub mod invariant_catalog;
pub mod parity_taxonomy;
pub use gauntlet::{
    CanonicalizationRules, CrashBoundary, CrashWindowDriver, CrashWindowKind, EngineVersions,
    ExecutionEnvelope, FAILURE_BUNDLE_SCHEMA, FAILURE_FIRST_DIVERGENCE_JSONPTR,
    FORBIDDEN_MCP_ENGINE_IDENTITY, FORBIDDEN_MCP_REGISTRY_ENGINE, FailureBody, FailureBundle,
    FailureProvenance, FailureType, FirstDivergence, GauntletEngineIdentity, GauntletIdentityPair,
    GauntletOracle, SPEC_TAG_WIRES, SUBJECT_IDENTITY, ScenarioAgreement, SpecTagClass, SpecTagWire,
    assert_distinct, compare_bytes, fragment_reason_class_matches, is_forbidden_gauntlet_identity,
    scenario,
};
pub use invariant_catalog::{
    ArtifactRef, BaseGate, CATALOG_SCHEMA_VERSION, CatalogViolation, CloseDecision, ContractStatus,
    InvariantCatalog, InvariantId, ParityInvariant, ProofKind, ProofObligation, ProofStatus,
    VERIFICATION_CONTRACT_SCHEMA, close_decision, seal_satisfied_hashes, unique_invariant_ids,
};
pub use parity_taxonomy::{
    Feature, FeatureId, FeatureUniverse, LoaderError, ParityStatus, Stats, truncate_score,
};
