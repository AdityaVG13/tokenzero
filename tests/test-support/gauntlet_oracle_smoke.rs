//! Phase 3 greenfield oracle smokes. Wrap existing census artifacts; do not
//! reimplement tokenizer matching, never-worse arithmetic, or golden replay.

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use serde_json::Value;
use tokenzero_test_support::{
    CrashBoundary, ExecutionEnvelope, GauntletEngineIdentity, GauntletIdentityPair, GauntletOracle,
    SPEC_TAG_WIRES, SUBJECT_IDENTITY, ScenarioAgreement, SpecTagClass, assert_distinct, scenario,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("TokenZero repo root")
}

fn existing_artifact(relative: &str) -> PathBuf {
    repo_root().join(relative)
}

fn assert_nonempty_file(relative: &str) -> Vec<u8> {
    let path = existing_artifact(relative);
    let bytes = fs::read(&path).unwrap_or_else(|err| panic!("{relative} must exist: {err}"));
    assert!(
        !bytes.is_empty(),
        "{relative} existing driver path must be non-empty"
    );
    bytes
}

fn run_wrap(
    name: &str,
    oracle: GauntletOracle,
    relative: &str,
    spec_check: impl FnOnce(&[u8]) -> Result<(), String>,
) {
    let pair = GauntletIdentityPair::new(oracle);
    let path = existing_artifact(relative);
    let agreed = scenario(
        name,
        pair,
        || {
            let bytes = fs::read(&path).map_err(|err| err.to_string())?;
            if bytes.is_empty() {
                return Err(format!("{relative} is empty"));
            }
            Ok(bytes)
        },
        || Ok(()),
    );
    match agreed {
        ScenarioAgreement::BothOk(bytes) => {
            spec_check(&bytes).unwrap_or_else(|err| panic!("{name}: {err}"));
        }
        ScenarioAgreement::BothErr { subject, oracle } => {
            panic!("{name}: unexpected both-error agreement: {subject:?} / {oracle:?}")
        }
    }
}

#[test]
fn identity_guard_rejects_self_comparison() {
    let subject = GauntletEngineIdentity::Subject.as_str();
    assert_eq!(subject, SUBJECT_IDENTITY);
    let caught = catch_unwind(|| assert_distinct(subject, subject));
    assert!(
        caught.is_err(),
        "subject==oracle must panic (K-9 self-comparison)"
    );

    let oracle = GauntletOracle::Spec.as_str();
    let same_oracle = catch_unwind(|| assert_distinct(oracle, oracle));
    assert!(same_oracle.is_err(), "oracle==oracle must panic");

    GauntletIdentityPair::new(GauntletOracle::Spec).assert_distinct();
}

#[test]
fn identity_guard_rejects_forbidden_mcp_identity() {
    let oracle = GauntletOracle::Spec.as_str();
    for forbidden in ["EngineIdentity::TokenZero", "RegistryEngine::TokenZero"] {
        let caught = catch_unwind(|| assert_distinct(forbidden, oracle));
        assert!(
            caught.is_err(),
            "{forbidden} must not be usable as gauntlet Subject"
        );
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
    let subject_ok = catch_unwind(AssertUnwindSafe(|| {
        scenario("divergent-ok", pair, || Ok(1u8), || Err("oracle-err"));
    }));
    assert!(subject_ok.is_err(), "subject Ok / oracle Err must panic");

    let oracle_ok = catch_unwind(AssertUnwindSafe(|| {
        scenario(
            "divergent-err",
            pair,
            || Err::<u8, _>("subject-err"),
            || Ok(()),
        );
    }));
    assert!(oracle_ok.is_err(), "subject Err / oracle Ok must panic");
}

#[test]
fn smoke_spec_oracle_reuses_prefix_probe() {
    run_wrap(
        "spec-prefix-probe",
        GauntletOracle::Spec,
        "tests/engine/fixtures/prefix-probe-replay.json",
        |bytes| {
            let v: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            if v["schema"] != "tokenzero.prefix-probe.v1" {
                return Err("unexpected prefix-probe schema".into());
            }
            let arm = &v["arms"][0];
            if arm.get("eligibility_declared").is_none()
                || arm.get("hit_declared_by_provider").is_none()
            {
                return Err("eligibility vs hit fields missing".into());
            }
            Ok(())
        },
    );
    assert_nonempty_file("tests/engine/prefix_probe.rs");
}

#[test]
fn smoke_property_oracle_reuses_proptest() {
    run_wrap(
        "property-greenfield-proptest",
        GauntletOracle::Property,
        "tests/core/greenfield_proptest.rs",
        |bytes| {
            let src = String::from_utf8_lossy(bytes);
            if !src.contains("prop_visible_le_raw") {
                return Err("greenfield_proptest.rs missing visible-le-raw property".into());
            }
            Ok(())
        },
    );
    let dual_bytes = assert_nonempty_file("tests/recovery/dual_store_fragment_proptest.rs");
    let dual = String::from_utf8_lossy(&dual_bytes);
    assert!(
        dual.contains("GauntletIdentityPair::new(GauntletOracle::RoundTrip)"),
        "live dual-store driver must stamp Subject vs RoundTrip"
    );
    assert!(
        dual.contains("TokenZeroStore") && dual.contains("RecoveryStore"),
        "dual-store proptest must name both stores"
    );
}

#[test]
fn smoke_self_oracle_reuses_cli_golden() {
    run_wrap(
        "self-cli-golden",
        GauntletOracle::SelfOracle,
        "tests/cli/golden/cli/read_json.golden",
        |_| Ok(()),
    );
    assert_nonempty_file("tests/cli/golden_outputs.rs");
}

#[test]
fn smoke_roundtrip_oracle_reuses_fuzz_differential() {
    run_wrap(
        "roundtrip-expand-fragment",
        GauntletOracle::RoundTrip,
        "fuzz/fuzz_targets/expand_fragment_differential.rs",
        |bytes| {
            let src = String::from_utf8_lossy(bytes);
            if !src.contains("TokenZeroStore") || !src.contains("RecoveryStore") {
                return Err("fuzz target must name both stores".into());
            }
            Ok(())
        },
    );
}

#[test]
fn smoke_external_tool_oracle_reuses_clippy_deny_and_never_worse() {
    run_wrap(
        "external-clippy-toolchain",
        GauntletOracle::ExternalTool,
        "rust-toolchain.toml",
        |bytes| {
            let src = String::from_utf8_lossy(bytes);
            if !src.contains("clippy") {
                return Err("rust-toolchain.toml must declare clippy".into());
            }
            Ok(())
        },
    );
    assert_nonempty_file("deny.toml");

    // Never-worse stays Python. Assert the existing gate refuses Q99 in unit_id.
    // Invocation (not ported): python3 benchmarks/test_never_worse_gate.py \
    //   NeverWorseGateTests.test_count_or_unit_mismatch_fails_closed
    let gate = String::from_utf8(assert_nonempty_file("benchmarks/never_worse_gate.py"))
        .expect("never_worse_gate.py utf-8");
    assert!(
        gate.contains("Q99-Input is not a TokenZero product unit"),
        "never-worse gate must refuse Q99 as a product unit"
    );
    let unit_test = String::from_utf8(assert_nonempty_file("benchmarks/test_never_worse_gate.py"))
        .expect("test_never_worse_gate.py utf-8");
    assert!(
        unit_test.contains("unit_id=\"Q99-Input\""),
        "test_never_worse_gate.py must cover Q99 unit_id rejection"
    );
}

#[test]
fn smoke_provider_tokenizer_oracle_reuses_goldens() {
    run_wrap(
        "provider-tokenizer-goldens",
        GauntletOracle::ProviderTokenizer,
        "tests/engine/fixtures/provider-tokenizer-goldens.json",
        |bytes| {
            let v: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            if v["schema"] != "tokenzero.tokenizer-goldens.v1" {
                return Err("unexpected tokenizer-goldens schema".into());
            }
            let entries = v["entries"]
                .as_array()
                .ok_or_else(|| "entries missing".to_string())?;
            if entries.is_empty() {
                return Err("tokenizer goldens entries empty".into());
            }
            Ok(())
        },
    );
    assert_nonempty_file("tests/engine/provider_tokenizer_goldens.rs");
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
                Path::new(&existing_artifact(driver)).exists(),
                "{} driver {} missing",
                row.tag,
                driver
            );
        }
    }
}

#[test]
fn crash_boundaries_are_named_not_subprocess_armed() {
    assert_eq!(CrashBoundary::ALL.len(), 8);
    for boundary in CrashBoundary::ALL {
        assert!(
            !boundary.is_subprocess_armed(),
            "{} must not be claimed subprocess-armed without arm_crash_boundary",
            boundary.as_str()
        );
        let driver = boundary.existing_driver();
        let bytes = assert_nonempty_file(driver.path);
        let src = String::from_utf8_lossy(&bytes);
        assert!(
            src.contains(driver.test_fn),
            "{} driver {} must contain test {}",
            boundary.as_str(),
            driver.path,
            driver.test_fn
        );
    }
}

#[test]
fn expand_fragment_differential_still_names_both_stores() {
    let src_bytes = assert_nonempty_file("fuzz/fuzz_targets/expand_fragment_differential.rs");
    let src = String::from_utf8_lossy(&src_bytes);
    assert!(
        src.contains("TokenZeroStore") && src.contains("RecoveryStore"),
        "fuzz target must still name both stores"
    );
    assert!(
        src.contains("reason_class_matches"),
        "fuzz comparator must stay class-match, not message-string"
    );
}
