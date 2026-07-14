use serde_json::{json, Value};
use tempfile::tempdir;

use super::common::*;

fn adapter_body(execution_allowed: bool) -> Value {
    json!({
        "schema_version": "tokenzero.adapter_approval_audit.v1",
        "ok": true,
        "execution_allowed": execution_allowed,
        "public_claims_approved": execution_allowed,
        "blind_install_attempted": false,
        "required_adapter_count": 11,
        "reviewed_command_count": if execution_allowed { 11 } else { 0 },
        "missing_reviewed_command_count": if execution_allowed { 0 } else { 11 },
        "duplicate_command_count": 0,
        "unsafe_command_count": 0,
        "adapters": []
    })
}

fn claim_adapter(path: &std::path::Path) -> Value {
    run_tokenzero_json(&[
        "claim-audit",
        "--release-approval",
        "--adapter-approval-artifact",
        path.to_str().unwrap(),
        "--json",
    ])
}

#[test]
fn cli_claim_audit_adapter_approval_rejection_matrix() {
    let cases: &[(bool, &str, bool, bool)] = &[
        (false, "adapter approval artifact does not allow execution", true, true),
        (true, "adapter approval artifact rows do not cover required adapters", false, false),
    ];
    for &(execution_allowed, reason, is_substr, check_blocked) in cases {
        let dir = tempdir().unwrap();
        let path = dir.path().join("adapter-approval.json");
        write_json_fixture(&path, &adapter_body(execution_allowed));
        let json = claim_adapter(&path);
        assert_eq!(json["public_claims_approved"], false);
        let gate = find_gate(&json, "adapter_approval");
        assert_eq!(gate["pass"], false);
        let hit = gate["reasons"].as_array().unwrap().iter().any(|r| {
            if is_substr { r.as_str().unwrap().contains(reason) } else { r == reason }
        });
        assert!(hit, "missing reason {reason} in {}", gate["reasons"]);
        if check_blocked {
            assert!(json["blocked_reasons"].as_array().unwrap().iter().any(|r| {
                r.as_str().unwrap().contains(reason)
            }));
        }
    }
}

#[test]
fn cli_claim_audit_separates_reviewed_adapter_coverage_from_execution_approval() {
    let dir = tempdir().unwrap();
    let template = dir.path().join("adapter-approval-template.json");
    let audit = dir.path().join("adapter-approval.json");
    let claim_path = dir.path().join("claim.json");
    let _ = run_tokenzero_json(&[
        "adapter-approval-template",
        "--output-json",
        template.to_str().unwrap(),
        "--json",
    ]);
    let _ = run_tokenzero_json(&[
        "adapter-approval-audit",
        "--approval-file",
        template.to_str().unwrap(),
        "--output-json",
        audit.to_str().unwrap(),
        "--json",
    ]);
    let claim = run_tokenzero_json(&[
        "claim-audit",
        "--adapter-approval-artifact",
        audit.to_str().unwrap(),
        "--output-json",
        claim_path.to_str().unwrap(),
        "--json",
    ]);
    let gate = find_gate(&claim, "adapter_approval");
    assert_eq!(gate["pass"], false);
    assert_reason(gate, "adapter approval artifact does not allow execution");
    assert!(!gate["reasons"].as_array().unwrap().iter().any(
        |r| r == "adapter approval artifact rows do not cover required adapters"
    ));
    assert_eq!(gate["details"]["reviewed_command_count"], 11);
    assert_eq!(gate["details"]["missing_reviewed_command_count"], 0);
}
