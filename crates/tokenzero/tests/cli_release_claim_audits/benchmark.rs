use serde_json::{json, Value};
use tempfile::tempdir;

use super::common::*;

fn tz_row(extra: Value) -> Value {
    let mut row = json!({
        "tool": "tokenzero", "suite": "shell-heavy", "availability_status": "run",
        "raw_tokens": 100, "visible_tokens": 25, "recovery_tokens": 75,
        "safe_savings": 0.75, "harm_rate": 0.0, "task_success": true,
        "fairness_notes": "fixture tokenzero row"
    });
    if let Some(o) = extra.as_object() {
        row.as_object_mut().unwrap().extend(o.clone());
    }
    row
}

fn rtk_run(extra: Value) -> Value {
    let mut row = json!({
        "tool": "rtk", "suite": "shell-heavy", "availability_status": "run",
        "raw_tokens": 10, "visible_tokens": 7, "recovery_tokens": 0,
        "safe_savings": 0.3, "harm_rate": 0.0, "task_success": true,
        "fairness_notes": "fixture runnable competitor row"
    });
    if let Some(o) = extra.as_object() {
        row.as_object_mut().unwrap().extend(o.clone());
    }
    row
}

fn rtk_unavail(reason: Option<&str>) -> Value {
    let mut row = json!({
        "tool": "rtk", "suite": "shell-heavy", "availability_status": "unavailable",
        "raw_tokens": 0, "visible_tokens": 0, "recovery_tokens": 0,
        "safe_savings": 0.0, "harm_rate": 0.0, "task_success": false,
        "fairness_notes": "fixture unavailable competitor row"
    });
    if let Some(r) = reason {
        row["availability_reason"] = json!(r);
    }
    row
}

fn body(rows: Vec<Value>, rc: Option<&str>) -> Value {
    let mut v = json!({
        "schema_version": "tokenzero.bench.v1",
        "ok": true,
        "public_claims_approved": true,
        "adapter_matrix": {
            "all_required_adapters_accounted": true,
            "blind_install_attempted": false
        },
        "rows": rows
    });
    if let Some(rc) = rc {
        v["release_candidate_id"] = json!(rc);
    }
    v
}

fn claim(path: &std::path::Path) -> Value {
    run_tokenzero_json(&[
        "claim-audit",
        "--release-approval",
        "--benchmark-artifact",
        path.to_str().unwrap(),
        "--json",
    ])
}

#[test]
fn cli_claim_audit_benchmark_rejection_matrix() {
    let expand = json!([{"byte_perfect": true, "ref": "tz://blob/tokenzero"}]);
    let unavail_reason = "fixture adapter is not executed without review";
    let cases: &[(&str, &str, Value, &str, &[&str], bool, bool, bool)] = &[
        (
            "missing_public_claim_fields",
            "thin-benchmark.json",
            body(vec![json!({"tool": "tokenzero", "safe_savings": 0.75})], None),
            "benchmark row missing public-claim field: raw_tokens",
            &[], false, false, false,
        ),
        (
            "missing_byte_perfect_recovery",
            "benchmark-missing-recovery.json",
            body(
                vec![
                    tz_row(json!({
                        "raw_tokens": 10, "visible_tokens": 4,
                        "recovery_tokens": 0, "safe_savings": 0.6
                    })),
                    rtk_run(json!({})),
                ],
                Some("rc-benchmark"),
            ),
            "benchmark row missing public-claim field: byte_perfect_recovery",
            &[], false, false, false,
        ),
        (
            "ref_less_expand_checks",
            "benchmark-missing-expand-ref.json",
            body(
                vec![
                    tz_row(json!({
                        "raw_tokens": 10, "visible_tokens": 4, "recovery_tokens": 0,
                        "safe_savings": 0.6, "byte_perfect_recovery": true,
                        "exact_expand_checks": [{"byte_perfect": true}]
                    })),
                    rtk_run(json!({
                        "byte_perfect_recovery": true,
                        "exact_expand_checks": [
                            {"byte_perfect": true, "ref": "tz://blob/fixture"}
                        ]
                    })),
                ],
                Some("rc-benchmark"),
            ),
            "benchmark row exact expand check missing ref",
            &[], false, true, false,
        ),
        (
            "unavailable_competitor_rows",
            "benchmark.json",
            body(vec![tz_row(json!({})), rtk_unavail(Some(unavail_reason))], None),
            "benchmark competitor rows must be runnable for public claims",
            &[], true, false, true,
        ),
        (
            "unavailable_not_recovery_failures",
            "benchmark-unavailable-row.json",
            body(
                vec![
                    tz_row(json!({
                        "byte_perfect_recovery": true,
                        "exact_expand_checks": expand.clone()
                    })),
                    rtk_unavail(Some(unavail_reason)),
                ],
                None,
            ),
            "benchmark competitor rows must be runnable for public claims",
            &[
                "benchmark row failed byte-perfect recovery",
                "benchmark row missing public-claim field: exact_expand_checks",
            ],
            true, false, false,
        ),
        (
            "unavailable_missing_reason",
            "benchmark-unavailable-without-reason.json",
            body(
                vec![
                    tz_row(json!({
                        "byte_perfect_recovery": true,
                        "exact_expand_checks": expand.clone()
                    })),
                    rtk_unavail(None),
                ],
                None,
            ),
            "benchmark unavailable row missing availability_reason",
            &[], false, true, false,
        ),
    ];

    for &(name, file, ref body, reason, absent, unavail_detail, blocked_ne, unavail_reason_detail) in cases {
        let dir = tempdir().unwrap();
        let path = dir.path().join(file);
        write_json_fixture(&path, body);
        let json = claim(&path);
        let gate = find_gate(&json, "benchmark_artifact");
        assert_eq!(gate["pass"], false, "{name}");
        assert_reason(gate, reason);
        for a in absent {
            assert!(
                !gate["reasons"].as_array().unwrap().iter().any(|r| r == a),
                "{name} has unexpected {a}"
            );
        }
        assert_eq!(json["public_claims_approved"], false, "{name}");
        assert!(gate["details"].is_object(), "{name}");
        if unavail_detail {
            assert_eq!(
                gate["details"]["public_claim_status"]["competitor_unavailable_rows"],
                1,
                "{name}"
            );
        }
        if unavail_reason_detail {
            assert_eq!(
                gate["details"]["public_claim_status"]["unavailable_competitors"][0]["availability_reason"],
                unavail_reason,
                "{name}"
            );
        }
        if blocked_ne {
            assert!(!json["blocked_reasons"].as_array().unwrap().is_empty(), "{name}");
        }
    }
}
