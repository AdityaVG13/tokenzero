//! Phase 0/3 greenfield identity smokes. Subject ≠ Oracle. MCP registry
//! labels are forbidden. Missing drivers must be `None`, not a deleted path.

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use serde_json::Value;
use tokenzero_test_support::{
    ExecutionEnvelope, FORBIDDEN_MCP_ENGINE_IDENTITY, FORBIDDEN_MCP_REGISTRY_ENGINE,
    GauntletEngineIdentity, GauntletIdentityPair, GauntletOracle, SPEC_TAG_WIRES, SUBJECT_IDENTITY,
    ScenarioAgreement, SpecTagClass, assert_distinct, is_forbidden_gauntlet_identity, scenario,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("TokenZero repo root")
}

#[test]
fn identity_guard_rejects_self_comparison() {
    let subject = GauntletEngineIdentity::Subject.as_str();
    assert_eq!(subject, SUBJECT_IDENTITY);
    assert!(
        catch_unwind(|| assert_distinct(subject, subject)).is_err(),
        "subject==oracle must panic (K-9 self-comparison)"
    );
    let oracle = GauntletOracle::Spec.as_str();
    assert!(
        catch_unwind(|| assert_distinct(oracle, oracle)).is_err(),
        "oracle==oracle must panic"
    );
    GauntletIdentityPair::new(GauntletOracle::Spec).assert_distinct();
}

#[test]
fn identity_guard_rejects_forbidden_mcp_identity() {
    let oracle = GauntletOracle::Spec.as_str();
    for forbidden in [FORBIDDEN_MCP_ENGINE_IDENTITY, FORBIDDEN_MCP_REGISTRY_ENGINE] {
        assert!(is_forbidden_gauntlet_identity(forbidden));
        assert!(
            catch_unwind(|| assert_distinct(forbidden, oracle)).is_err(),
            "{forbidden} must not be usable as gauntlet Subject"
        );
        assert_ne!(SUBJECT_IDENTITY, forbidden);
        assert_ne!(oracle, forbidden);
    }
}

#[test]
fn mixed_oracles_are_distinct_from_subject_and_each_other() {
    let subject = SUBJECT_IDENTITY;
    let mut seen = HashSet::new();
    assert!(seen.insert(subject));
    assert_eq!(GauntletOracle::ALL.len(), 6);
    for mode in GauntletOracle::ALL {
        let oracle = mode.as_str();
        assert!(!oracle.is_empty(), "{mode} identity empty");
        assert_ne!(oracle, subject, "{mode} collided with Subject");
        assert!(
            !is_forbidden_gauntlet_identity(oracle),
            "{mode} used a forbidden MCP identity"
        );
        assert!(
            seen.insert(oracle),
            "duplicate oracle identity string: {oracle}"
        );
        GauntletIdentityPair::new(*mode).assert_distinct();
    }
}

#[test]
fn artifact_id_ignores_run_id() {
    let pair = GauntletIdentityPair::new(GauntletOracle::Spec);
    let mut left = ExecutionEnvelope::from_pair("spec-smoke", 7, pair, vec!["a".into()]);
    let mut right = left.clone();
    left.run_id = Some("run-1".into());
    right.run_id = Some("run-2".into());
    assert_eq!(left.artifact_id(), right.artifact_id());
    let mut other = right.clone();
    other.scenario_id = "other-scenario".into();
    assert_ne!(left.artifact_id(), other.artifact_id());
}

#[test]
fn scenario_both_error_is_agreement() {
    let pair = GauntletIdentityPair::new(GauntletOracle::Spec);
    match scenario(
        "both-err",
        pair,
        || Err::<u8, _>("subject-err"),
        || Err("oracle-err"),
    ) {
        ScenarioAgreement::BothErr { subject, oracle } => {
            assert_eq!(subject, "subject-err");
            assert_eq!(oracle, "oracle-err");
        }
        ScenarioAgreement::BothOk(_) => panic!("both-error must be agreement, not Ok"),
    }
}

#[test]
fn scenario_one_error_one_ok_is_hard_fail() {
    let pair = GauntletIdentityPair::new(GauntletOracle::Spec);
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            scenario("divergent-ok", pair, || Ok(1u8), || Err("oracle-err"));
        }))
        .is_err(),
        "subject Ok / oracle Err must panic"
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            scenario(
                "divergent-err",
                pair,
                || Err::<u8, _>("subject-err"),
                || Ok(()),
            );
        }))
        .is_err(),
        "subject Err / oracle Ok must panic"
    );
}

#[test]
fn spec_tag_catalog_does_not_mark_ambiguous_as_wired() {
    let verifiable = SPEC_TAG_WIRES
        .iter()
        .filter(|row| row.class == SpecTagClass::Verifiable)
        .count();
    let ambiguous = SPEC_TAG_WIRES
        .iter()
        .filter(|row| row.class == SpecTagClass::Ambiguous)
        .count();
    assert_eq!(verifiable, 33, "Phase 2 Verifiable count");
    assert_eq!(ambiguous, 7, "Phase 2 Ambiguous count");
    let root = repo_root();
    for row in SPEC_TAG_WIRES {
        if row.class == SpecTagClass::Ambiguous {
            assert!(
                !row.is_wired(),
                "{} is Ambiguous and must stay uncovered",
                row.tag
            );
            assert!(row.existing_driver.is_none());
        }
        if let Some(driver) = row.existing_driver {
            assert!(
                Path::new(&root.join(driver)).exists(),
                "{} driver {} missing on disk (use None, do not cite a deleted path)",
                row.tag,
                driver
            );
        }
    }
}

#[test]
fn provider_tokenizer_fixture_and_cli_golden_still_exist() {
    let root = repo_root();
    let goldens = root.join("tests/engine/fixtures/provider-tokenizer-goldens.json");
    let bytes = std::fs::read(&goldens).expect("provider-tokenizer goldens");
    let v: Value = serde_json::from_slice(&bytes).expect("goldens json");
    assert_eq!(v["schema"], "tokenzero.tokenizer-goldens.v1");
    assert!(
        v["entries"]
            .as_array()
            .map(|e| !e.is_empty())
            .unwrap_or(false),
        "tokenizer goldens entries empty"
    );

    let golden = root.join("tests/cli/golden/cli/read_json.golden");
    assert!(
        golden.is_file() && golden.metadata().map(|m| m.len() > 0).unwrap_or(false),
        "Self-Oracle CLI golden missing"
    );

    let toolchain = std::fs::read_to_string(root.join("rust-toolchain.toml")).unwrap();
    assert!(toolchain.contains("clippy"));
    assert!(toolchain.contains("nightly-2026-05-31"));
}
