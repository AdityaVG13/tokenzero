//! Tests for the CodeMode integrated audit.

use super::audit::run_codemode_audit;
use std::path::PathBuf;

fn audit_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[test]
fn generate_audit_artifact() {
    let root = audit_root();
    let report = run_codemode_audit(&root);
    let json = serde_json::to_string_pretty(&report).unwrap();

    // Write to results/current/ if the directory exists
    let artifact_path = root.join("results/current/tokenzero_codemode_audit.json");
    if artifact_path.parent().unwrap().exists() {
        std::fs::write(&artifact_path, &json).unwrap();
        eprintln!("wrote {}", artifact_path.display());
    }
    println!("{json}");

    // Verify structure
    assert_eq!(report.schema_version, "tokenzero.codemode_audit.v1");
    assert_eq!(
        report.status, "pass",
        "audit must pass: {:?}",
        report.summary
    );
}

#[test]
fn audit_recovery_all_byte_exact() {
    let root = audit_root();
    let report = run_codemode_audit(&root);
    assert!(
        report.recovery_evidence.all_byte_exact,
        "all recovery cases must be byte-exact: {:?}",
        report.recovery_evidence.cases
    );
    assert!(report.recovery_evidence.total_refs_checked >= 4);
}

#[test]
fn audit_cost_plan_never_worse() {
    let root = audit_root();
    let report = run_codemode_audit(&root);
    assert!(
        report.cost_evidence.plan_always_cheaper_or_equal,
        "plan execution must never be worse than direct calls"
    );
}

#[test]
fn audit_cross_surface_parity() {
    let root = audit_root();
    let report = run_codemode_audit(&root);
    assert!(
        report.cross_surface_evidence.all_identical,
        "cross-surface parity must hold: {:?}",
        report.cross_surface_evidence.cases
    );
}
