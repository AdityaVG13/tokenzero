use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;



#[test]
fn cli_source_currency_audit_records_competitive_ledger_and_blocks_public_claims() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("source-ledger.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "source-currency-audit",
            "--output-json",
            output_json.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.source_currency.v1");
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    assert_eq!(json["fresh_for_private_planning"], true);
    assert_eq!(json["fresh_for_public_claim"], false);
    assert!(
        json["source_commit_pin_status"]["unpinned"]
            .as_u64()
            .unwrap()
            >= 11
    );
    assert!(
        json["unpinned_source_rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["source_commit"] == "snapshot-20260604")
    );

    let required_tools = [
        "rtk",
        "ztk",
        "lean-ctx",
        "tokenpak",
        "tokenjuice",
        "context-mode",
        "caveman",
        "headroom",
        "claw",
        "compresh",
        "context-gateway",
    ];
    let rows = json["rows"].as_array().unwrap();
    for tool in required_tools {
        let row = rows
            .iter()
            .find(|row| row["tool"] == tool)
            .unwrap_or_else(|| panic!("missing source row for {tool}"));
        assert!(
            row["url"]
                .as_str()
                .unwrap()
                .starts_with("https://github.com/"),
            "{tool}"
        );
        assert_eq!(row["source_date"], "2026-06-04");
        assert!(row["source_commit"].as_str().unwrap().len() >= 7, "{tool}");
        assert!(!row["claimed_scope"].as_str().unwrap().is_empty(), "{tool}");
        assert!(
            !row["issue_pr_themes"].as_array().unwrap().is_empty(),
            "{tool}"
        );
        assert!(!row["strengths"].as_array().unwrap().is_empty(), "{tool}");
        assert!(!row["gaps"].as_array().unwrap().is_empty(), "{tool}");
        assert_eq!(row["fresh_for_private_planning"], true, "{tool}");
        assert_eq!(row["fresh_for_public_claim"], false, "{tool}");
    }

    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "source ledger requires same-release-candidate refresh")
    );
    assert!(output_json.exists());
}

#[test]
fn cli_source_currency_audit_refreshes_release_candidate_pins_without_public_approval() {
    let dir = tempdir().unwrap();
    let refresh_ledger = dir.path().join("source-refresh.json");
    let output_json = dir.path().join("source-ledger.json");
    let claim_json = dir.path().join("claims.json");
    let tools = [
        "rtk",
        "ztk",
        "lean-ctx",
        "tokenpak",
        "tokenjuice",
        "context-mode",
        "caveman",
        "cavekit",
        "cavemem",
        "caveman-code",
        "headroom",
        "engram",
        "claw",
        "contextpilot",
        "wilpel-caveman-compression",
        "compresh",
        "compresh-mcp",
        "context-gateway",
    ];
    let rows: Vec<Value> = tools
        .iter()
        .enumerate()
        .map(|(idx, tool)| {
            serde_json::json!({
                "tool": tool,
                "source_commit": format!("{:040x}", idx + 1),
                "source_date": "2026-06-04"
            })
        })
        .collect();
    std::fs::write(
        &refresh_ledger,
        serde_json::to_vec_pretty(&serde_json::json!({ "rows": rows })).unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-source-refresh")
        .args([
            "source-currency-audit",
            "--refresh-ledger",
            refresh_ledger.to_str().unwrap(),
            "--output-json",
            output_json.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
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
            && row["source_commit"]
                .as_str()
                .unwrap()
                .chars()
                .all(|ch| ch.is_ascii_hexdigit())
    }));
    assert!(
        !json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "source ledger requires same-release-candidate refresh")
    );

    let claim_output = Command::cargo_bin("tokenzero")
        .unwrap()
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-source-refresh")
        .args([
            "claim-audit",
            "--source-artifact",
            output_json.to_str().unwrap(),
            "--output-json",
            claim_json.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        claim_output.status.success(),
        "{}",
        String::from_utf8_lossy(&claim_output.stderr)
    );
    let claim: Value = serde_json::from_slice(&claim_output.stdout).unwrap();
    let source_gate = claim["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "source_currency")
        .unwrap();
    assert_eq!(source_gate["pass"], true);
    assert_eq!(claim["public_claims_approved"], false);
    assert!(
        claim["claims"]
            .as_array()
            .unwrap()
            .iter()
            .all(|claim| { claim["public_safe_to_publish"] == false })
    );

    let results_dir = dir.path().join("results").join("current");
    std::fs::create_dir_all(&results_dir).unwrap();
    std::fs::copy(
        &output_json,
        results_dir.join("tokenzero_source_currency.json"),
    )
    .unwrap();
    let completion_output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-source-refresh")
        .args(["completion-audit", "--json"])
        .output()
        .unwrap();
    assert!(
        completion_output.status.success(),
        "{}",
        String::from_utf8_lossy(&completion_output.stderr)
    );
    let completion: Value = serde_json::from_slice(&completion_output.stdout).unwrap();
    let g001 = completion["g_goals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "G-001")
        .expect("G-001 row");
    let g001_residual = g001["residual"].as_str().unwrap();
    assert!(g001_residual.contains("source evidence is current"));
    assert!(
        !g001_residual.contains("refresh required"),
        "fresh source evidence should not be reported as a remaining source refresh"
    );
    let fr001 = completion["must_fr"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "FR-001")
        .expect("FR-001 row");
    let fr001_residual = fr001["residual"].as_str().unwrap();
    assert!(fr001_residual.contains("source evidence is current"));
    assert!(
        !fr001_residual.contains("refresh still required"),
        "FR-001 should not ask for a source refresh once the source artifact is fresh"
    );
}

#[test]
fn cli_claim_audit_includes_source_currency_gate_even_with_release_approval() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("claims.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--output-json",
            output_json.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.claim_audit.v1");
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(
        json["source_currency"]["schema_version"],
        "tokenzero.source_currency.v1"
    );
    assert_eq!(json["source_currency"]["fresh_for_public_claim"], false);
    assert!(json["source_currency"]["rows"].as_array().unwrap().len() >= 11);
    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "source ledger requires same-release-candidate refresh")
    );
    assert!(json["claims"].as_array().unwrap().iter().all(|claim| {
        claim["source_current"] == false && claim["public_safe_to_publish"] == false
    }));
    assert!(output_json.exists());
}

#[test]
fn cli_claim_audit_rejects_source_artifact_missing_currency_fields() {
    let dir = tempdir().unwrap();
    let source_artifact = dir.path().join("source-currency.json");
    let required_tools = [
        "rtk",
        "ztk",
        "lean-ctx",
        "tokenpak",
        "tokenjuice",
        "context-mode",
        "caveman",
        "headroom",
        "claw",
        "compresh",
        "context-gateway",
    ];
    let rows = required_tools
        .iter()
        .enumerate()
        .map(|(idx, tool)| {
            let mut row = serde_json::json!({
                "tool": tool,
                "url": format!("https://github.com/example/{tool}"),
                "claimed_scope": "fixture",
                "issue_pr_themes": ["fixture"],
                "strengths": ["fixture"],
                "gaps": ["fixture"],
                "source_date": "2026-06-04",
                "source_commit": "release-candidate"
            });
            if idx == 0 {
                row.as_object_mut().unwrap().remove("source_commit");
            }
            row
        })
        .collect::<Vec<_>>();
    std::fs::write(
        &source_artifact,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.source_currency.v1",
            "release_candidate_id": "rc-source",
            "ok": true,
            "fresh_for_public_claim": true,
            "rows": rows
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--source-artifact",
            source_artifact.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let source_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "source_currency")
        .expect("source gate");
    assert_eq!(source_gate["pass"], false);
    assert_eq!(
        source_gate["artifact_path"],
        source_artifact.to_str().unwrap().replace('\\', "/")
    );
    assert_eq!(source_gate["details"]["release_candidate_id"], "rc-source");
    assert_eq!(
        json["gate_artifact_paths"]["source_currency"],
        source_artifact.to_str().unwrap().replace('\\', "/")
    );
    assert!(
        source_gate["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "source ledger row missing source commit")
    );
    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "source ledger row missing source commit")
    );
    assert_eq!(json["public_claims_approved"], false);
}

#[test]
fn cli_claim_audit_rejects_source_artifact_with_snapshot_source_commits() {
    let dir = tempdir().unwrap();
    let source_artifact = dir.path().join("source-currency.json");
    let required_tools = [
        "rtk",
        "ztk",
        "lean-ctx",
        "tokenpak",
        "tokenjuice",
        "context-mode",
        "caveman",
        "headroom",
        "claw",
        "compresh",
        "context-gateway",
    ];
    let rows = required_tools
        .iter()
        .enumerate()
        .map(|(idx, tool)| {
            serde_json::json!({
                "tool": tool,
                "url": format!("https://github.com/example/{tool}"),
                "claimed_scope": "fixture",
                "issue_pr_themes": ["fixture"],
                "strengths": ["fixture"],
                "gaps": ["fixture"],
                "source_date": "2026-06-04",
                "source_commit": if idx == 0 { "snapshot-20260604" } else { "abcdef1" }
            })
        })
        .collect::<Vec<_>>();
    std::fs::write(
        &source_artifact,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.source_currency.v1",
            "release_candidate_id": "rc-source",
            "ok": true,
            "fresh_for_public_claim": true,
            "public_claims_approved": true,
            "rows": rows
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "claim-audit",
            "--release-approval",
            "--source-artifact",
            source_artifact.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let source_gate = json["evidence_gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "source_currency")
        .expect("source gate");

    assert_eq!(source_gate["pass"], false);
    assert!(
        source_gate["reasons"].as_array().unwrap().iter().any(
            |reason| reason == "source ledger row source commit is not a release-candidate pin"
        )
    );
    assert!(
        source_gate["details"]["unpinned_source_rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["tool"] == "rtk" && row["source_commit"] == "snapshot-20260604")
    );
    assert_eq!(
        source_gate["details"]["source_commit_pin_status"]["unpinned"],
        1
    );
    assert_eq!(json["public_claims_approved"], false);
}
