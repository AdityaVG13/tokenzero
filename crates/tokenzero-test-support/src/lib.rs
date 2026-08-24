//! TokenZero-specific tests plus the shared ZeroStack test contract.

pub mod conformal;
pub mod gauntlet;
pub mod invariant_catalog;
pub mod parity_taxonomy;
pub mod ratchet;
pub use conformal::{
    apply_conformal_residuals, release_pass_on_point_estimate, residual_quantile, score_categories,
    score_passes_trials, BetaParams, CategoryEvidence, CategoryScore, ConformalInterval,
    ConformalStatus, ParityScorecard, ReleaseBlock, ReleaseVerdict, DEFAULT_CONFIDENCE,
    MIN_CALIBRATION_RESIDUALS, SCORECARD_SCHEMA, UNIFORM_ALPHA_PRIOR, UNIFORM_BETA_PRIOR,
};
pub use gauntlet::{
    assert_distinct, compare_bytes, fragment_reason_class_matches, is_forbidden_gauntlet_identity,
    scenario, CanonicalizationRules, CrashBoundary, CrashWindowDriver, CrashWindowKind,
    EngineVersions, ExecutionEnvelope, FailureBody, FailureBundle, FailureProvenance, FailureType,
    FirstDivergence, GauntletEngineIdentity, GauntletIdentityPair, GauntletOracle,
    ScenarioAgreement, SpecTagClass, SpecTagWire, FAILURE_BUNDLE_SCHEMA,
    FAILURE_FIRST_DIVERGENCE_JSONPTR, FORBIDDEN_MCP_ENGINE_IDENTITY, FORBIDDEN_MCP_REGISTRY_ENGINE,
    SPEC_TAG_WIRES, SUBJECT_IDENTITY,
};
pub use invariant_catalog::{
    close_decision, seal_satisfied_hashes, unique_invariant_ids, ArtifactRef, BaseGate,
    CatalogViolation, CloseDecision, ContractStatus, InvariantCatalog, InvariantId,
    ParityInvariant, ProofKind, ProofObligation, ProofStatus, CATALOG_SCHEMA_VERSION,
    VERIFICATION_CONTRACT_SCHEMA,
};
pub use parity_taxonomy::{
    truncate_score, Feature, FeatureId, FeatureUniverse, LoaderError, ParityStatus, Stats,
};
pub use ratchet::{
    apply_ratchet, apply_ratchet_with_waiver, RatchetState, RatchetVerdict, RatchetWaiver,
    CATEGORY_QUARANTINE_THRESHOLD, RATCHET_STATE_SCHEMA,
};
