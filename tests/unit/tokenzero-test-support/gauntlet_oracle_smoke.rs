//! Phase 0/3 greenfield identity smokes. Subject ≠ Oracle. MCP registry
//! labels are forbidden. Missing drivers must be `None`, not a deleted path.

use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use serde_json::Value;
use tokenzero_test_support::{
    assert_distinct, is_forbidden_gauntlet_identity, scenario, CrashBoundary, ExecutionEnvelope,
    GauntletEngineIdentity, GauntletIdentityPair, GauntletOracle, ScenarioAgreement, SpecTagClass,
    FORBIDDEN_MCP_ENGINE_IDENTITY, FORBIDDEN_MCP_REGISTRY_ENGINE, SPEC_TAG_WIRES, SUBJECT_IDENTITY,
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
    let wired = SPEC_TAG_WIRES.iter().filter(|row| row.is_wired()).count();
    assert_eq!(wired, 19, "Phase 2 live-wired Verifiable count");
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
fn crash_boundary_drivers_are_uncovered_after_d8c0844() {
    let root = repo_root();
    assert_eq!(CrashBoundary::ALL.len(), 8);
    for boundary in CrashBoundary::ALL {
        assert!(
            !boundary.is_subprocess_armed(),
            "{} must not claim Pattern 65 arming",
            boundary.as_str()
        );
        assert!(
            boundary.existing_driver().is_none(),
            "{} existing_driver must be None (Uncovered); do not cite a deleted path as live",
            boundary.as_str()
        );
        let census = boundary.deleted_driver_census();
        assert!(
            !root.join(census.path).exists(),
            "{} census path {} reappeared; wire existing_driver to the live file",
            boundary.as_str(),
            census.path
        );
    }
}

#[test]
fn hub_002_no_fszero_graphzero_crate_deps() {
    let root = repo_root();
    let mut tomls = vec![root.join("Cargo.toml")];
    let crates = root.join("crates");
    for entry in std::fs::read_dir(&crates).expect("crates/") {
        let entry = entry.expect("crate dir");
        let cargo = entry.path().join("Cargo.toml");
        if cargo.is_file() {
            tomls.push(cargo);
        }
    }
    assert!(
        tomls.len() > 1,
        "expected workspace + crate Cargo.toml files"
    );
    for path in &tomls {
        for (idx, line) in std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
            .lines()
            .enumerate()
        {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                continue;
            }
            let lower = trimmed.to_ascii_lowercase();
            assert!(
                !lower.contains("fszero"),
                "{}:{} imports FSZero: {trimmed}",
                path.display(),
                idx + 1
            );
            assert!(
                !lower.contains("graphzero"),
                "{}:{} imports GraphZero: {trimmed}",
                path.display(),
                idx + 1
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

    assert_eq!(
        GauntletOracle::Spec.as_str(),
        "GauntletOracle::Spec::tokenzero-spec@HEAD-fb73416"
    );
    assert!(
        SUBJECT_IDENTITY.contains("862e3e682cb8aee0e150c1cb0b116cb2e23a44e2"),
        "Subject identity stays Self-oracle prior-commit 862e3e6, not retargeted to HEAD"
    );
}

#[test]
fn embedded_surface_matrix_byte_matches_gauntlet_workspace_when_present() {
    let fixture = repo_root()
        .join("crates/tokenzero-test-support/src/fixtures/supported_surface_matrix.toml");
    let fixture_bytes = std::fs::read(&fixture).expect("embedded fixture");
    let workspace = repo_root()
        .parent()
        .expect("sibling")
        .join("TokenZero__gauntlet_workspace/docs/contracts/supported_surface_matrix.toml");
    if workspace.is_file() {
        let workspace_bytes = std::fs::read(&workspace).expect("workspace matrix");
        assert_eq!(
            fixture_bytes, workspace_bytes,
            "workspace supported_surface_matrix.toml must byte-match the TokenZero fixture"
        );
    }
}
