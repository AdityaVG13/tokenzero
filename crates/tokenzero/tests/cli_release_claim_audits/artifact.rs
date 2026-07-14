use serde_json::{json, Value};
use tempfile::tempdir;

use super::common::*;

fn source_row(tool: &str, commit: Option<&str>) -> Value {
    let mut row = json!({
        "tool": tool,
        "url": format!("https://github.com/example/{tool}"),
        "claimed_scope": "fixture",
        "issue_pr_themes": ["fixture"],
        "strengths": ["fixture"],
        "gaps": ["fixture"],
        "source_date": "2026-06-04"
    });
    if let Some(c) = commit {
        row["source_commit"] = json!(c);
    }
    row
}

fn source_rows(commits: impl Fn(usize) -> Option<&'static str>) -> Vec<Value> {
    required_adapter_tools()
        .iter()
        .enumerate()
        .map(|(idx, tool)| source_row(tool, commits(idx)))
        .collect()
}

fn write_source(dir: &std::path::Path, rows: Vec<Value>, extra: Value) -> std::path::PathBuf {
    let path = dir.join("source-currency.json");
    let mut body = json!({
        "schema_version": "tokenzero.source_currency.v1",
        "release_candidate_id": "rc-source",
        "ok": true,
        "fresh_for_public_claim": true,
        "rows": rows
    });
    if let Some(o) = extra.as_object() {
        body.as_object_mut().unwrap().extend(o.clone());
    }
    write_json_fixture(&path, &body);
    path
}

fn claim_source(path: &std::path::Path) -> Value {
    run_tokenzero_json(&[
        "claim-audit",
        "--release-approval",
        "--source-artifact",
        path.to_str().unwrap(),
        "--json",
    ])
}

fn assert_source_row_shape(row: &Value, tool: &str) {
    assert!(row["url"].as_str().unwrap().starts_with("https://github.com/"), "{tool}");
    assert_eq!(row["source_date"], "2026-06-04");
    assert!(row["source_commit"].as_str().unwrap().len() >= 7, "{tool}");
    assert!(!row["claimed_scope"].as_str().unwrap().is_empty(), "{tool}");
    assert!(!row["issue_pr_themes"].as_array().unwrap().is_empty(), "{tool}");
    assert!(!row["strengths"].as_array().unwrap().is_empty(), "{tool}");
    assert!(!row["gaps"].as_array().unwrap().is_empty(), "{tool}");
    assert_eq!(row["fresh_for_private_planning"], true, "{tool}");
    assert_eq!(row["fresh_for_public_claim"], false, "{tool}");
}

#[test]
fn cli_source_currency_audit_records_competitive_ledger_and_blocks_public_claims() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("source-ledger.json");
    let json = run_tokenzero_json(&[
        "source-currency-audit",
        "--output-json",
        output_json.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(json["schema_version"], "tokenzero.source_currency.v1");
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    assert_eq!(json["fresh_for_private_planning"], true);
    assert_eq!(json["fresh_for_public_claim"], false);
    assert!(json["source_commit_pin_status"]["unpinned"].as_u64().unwrap() >= 11);
    assert!(json["unpinned_source_rows"].as_array().unwrap().iter().any(|r| r["source_commit"] == "snapshot-20260604"));
    let rows = json["rows"].as_array().unwrap();
    for tool in required_adapter_tools() {
        let row = rows.iter().find(|r| r["tool"] == *tool).unwrap_or_else(|| panic!("missing source row for {tool}"));
        assert_source_row_shape(row, tool);
    }
    assert!(json["blocked_reasons"].as_array().unwrap().iter().any(|r| r == "source ledger requires same-release-candidate refresh"));
    assert!(output_json.exists());
}

#[test]
fn cli_source_currency_audit_refreshes_release_candidate_pins_without_public_approval() {
    let dir = tempdir().unwrap();
    let refresh_ledger = dir.path().join("source-refresh.json");
    let output_json = dir.path().join("source-ledger.json");
    let claim_json = dir.path().join("claims.json");
    let tools = [
        "rtk", "ztk", "lean-ctx", "tokenpak", "tokenjuice", "context-mode", "caveman",
        "cavekit", "cavemem", "caveman-code", "headroom", "engram", "claw", "contextpilot",
        "wilpel-caveman-compression", "compresh", "compresh-mcp", "context-gateway",
    ];
    let rows: Vec<Value> = tools.iter().enumerate().map(|(idx, tool)| {
        json!({
            "tool": tool,
            "source_commit": format!("{:040x}", idx + 1),
            "source_date": "2026-06-04"
        })
    }).collect();
    write_json_fixture(&refresh_ledger, &json!({ "rows": rows }));
    let env = [("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-source-refresh")];
    let json = run_tokenzero_json_with_env(
        &[
            "source-currency-audit",
            "--refresh-ledger",
            refresh_ledger.to_str().unwrap(),
            "--output-json",
            output_json.to_str().unwrap(),
            "--json",
        ],
        &env,
    );
    assert_eq!(json["schema_version"], "tokenzero.source_currency.v1");
    assert_eq!(json["release_candidate_id"], "rc-source-refresh");
    assert_eq!(json["fresh_for_public_claim"], true);
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    assert_eq!(json["source_commit_pin_status"]["pinned"], tools.len());
    assert_eq!(json["source_commit_pin_status"]["missing"], 0);
    assert_eq!(json["source_commit_pin_status"]["unpinned"], 0);
    assert!(json["unpinned_source_rows"].as_array().unwrap().is_empty());
    assert!(json["rows"].as_array().unwrap().iter().all(|row| {
        row["fresh_for_public_claim"] == true
            && row["source_commit"].as_str().unwrap().chars().all(|ch| ch.is_ascii_hexdigit())
    }));
    assert!(!json["blocked_reasons"].as_array().unwrap().iter().any(|r| r == "source ledger requires same-release-candidate refresh"));

    let claim = run_tokenzero_json_with_env(
        &[
            "claim-audit",
            "--source-artifact",
            output_json.to_str().unwrap(),
            "--output-json",
            claim_json.to_str().unwrap(),
            "--json",
        ],
        &env,
    );
    assert_eq!(find_gate(&claim, "source_currency")["pass"], true);
    assert_eq!(claim["public_claims_approved"], false);
    assert!(claim["claims"].as_array().unwrap().iter().all(|c| c["public_safe_to_publish"] == false));

    let results = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results).unwrap();
    std::fs::copy(&output_json, results.join("tokenzero_source_currency.json")).unwrap();
    let completion = run_tokenzero_json_in_with_env(
        &["completion-audit", "--json"],
        dir.path(),
        &env,
    );
    for (section, id, bad) in [
        ("g_goals", "G-001", "refresh required"),
        ("must_fr", "FR-001", "refresh still required"),
    ] {
        let residual = completion[section].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap()["residual"].as_str().unwrap();
        assert!(residual.contains("source evidence is current"), "{id}");
        assert!(!residual.contains(bad), "{id}");
    }
}

#[test]
fn cli_claim_audit_includes_source_currency_gate_even_with_release_approval() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("claims.json");
    let json = run_tokenzero_json(&[
        "claim-audit",
        "--release-approval",
        "--output-json",
        output_json.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(json["schema_version"], "tokenzero.claim_audit.v1");
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["source_currency"]["schema_version"], "tokenzero.source_currency.v1");
    assert_eq!(json["source_currency"]["fresh_for_public_claim"], false);
    assert!(json["source_currency"]["rows"].as_array().unwrap().len() >= 11);
    assert!(json["blocked_reasons"].as_array().unwrap().iter().any(|r| r == "source ledger requires same-release-candidate refresh"));
    assert!(json["claims"].as_array().unwrap().iter().all(|c| {
        c["source_current"] == false && c["public_safe_to_publish"] == false
    }));
    assert!(output_json.exists());
}

#[test]
fn cli_claim_audit_source_artifact_rejection_matrix() {
    let cases: &[(&str, fn(usize) -> Option<&'static str>, Value, &str, bool)] = &[
        (
            "missing_currency_fields",
            |idx| if idx == 0 { None } else { Some("release-candidate") },
            json!({}),
            "source ledger row missing source commit",
            false,
        ),
        (
            "snapshot_source_commits",
            |idx| Some(if idx == 0 { "snapshot-20260604" } else { "abcdef1" }),
            json!({ "public_claims_approved": true }),
            "source ledger row source commit is not a release-candidate pin",
            true,
        ),
    ];

    for &(name, commits, ref extra, reason, check_unpinned) in cases {
        let dir = tempdir().unwrap();
        let path = write_source(dir.path(), source_rows(commits), extra.clone());
        let json = claim_source(&path);
        let gate = find_gate(&json, "source_currency");
        let path_s = path.to_str().unwrap().replace('\\', "/");
        assert_eq!(gate["pass"], false, "{name}");
        assert_eq!(gate["artifact_path"], path_s, "{name}");
        assert_eq!(gate["details"]["release_candidate_id"], "rc-source", "{name}");
        assert_eq!(json["gate_artifact_paths"]["source_currency"], path_s, "{name}");
        assert_reason(gate, reason);
        assert_blocked_reason(&json, reason);
        assert_eq!(json["public_claims_approved"], false, "{name}");
        if check_unpinned {
            assert!(
                gate["details"]["unpinned_source_rows"].as_array().unwrap().iter()
                    .any(|r| r["tool"] == "rtk" && r["source_commit"] == "snapshot-20260604"),
                "{name}"
            );
            assert_eq!(gate["details"]["source_commit_pin_status"]["unpinned"], 1, "{name}");
        }
    }
}
