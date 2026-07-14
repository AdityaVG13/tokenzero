//! Tests for the CodeMode integrated audit.

use super::audit::run_codemode_audit;
use std::path::PathBuf;

fn audit_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn codemode_audit_contract() {
    let root = audit_root();
    let report = run_codemode_audit(&root);
    let json = serde_json::to_string_pretty(&report).unwrap();
    let artifact_path = root.join("results/current/tokenzero_codemode_audit.json");
    if artifact_path.parent().unwrap().exists() {
        std::fs::write(&artifact_path, &json).unwrap();
        eprintln!("wrote {}", artifact_path.display());
    }
    println!("{json}");
    assert_eq!(report.schema_version, "tokenzero.codemode_audit.v1");
    assert_eq!(report.status, "pass", "audit must pass: {:?}", report.summary);
    assert!(report.recovery_evidence.all_byte_exact, "all recovery cases must be byte-exact: {:?}", report.recovery_evidence.cases);
    assert!(report.recovery_evidence.total_refs_checked >= 4);
    assert!(report.cost_evidence.plan_always_cheaper_or_equal, "plan execution must never be worse than direct calls");
    assert!(report.cross_surface_evidence.all_identical, "cross-surface parity must hold: {:?}", report.cross_surface_evidence.cases);
}
