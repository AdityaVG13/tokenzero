mod common;
use common::*;

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

const GOLDEN_RELEASE_CANDIDATE_ID: &str = "golden-test";


fn golden_tokenzero_with_rc(args: &[&str], dir: &Path) -> std::process::Output {
    assert_success(
        tokenzero_cmd()
            .current_dir(dir)
            .env("TOKENZERO_RELEASE_CANDIDATE_ID", GOLDEN_RELEASE_CANDIDATE_ID)
            .args(args)
            .output()
            .unwrap(),
        &format!("{args:?}"),
    )
}

fn assert_stdout_and_file_golden(stdout: &[u8], file: &Path, dir: &Path, golden: &str) {
    let actual = canonical_json(stdout, dir);
    let written = canonical_json(&std::fs::read(file).unwrap(), dir);
    assert_eq!(actual, written);
    assert_golden(golden, &actual);
}

#[test]
fn cli_read_json_envelope_matches_golden() {
    let (dir, cache) = setup_temp_with_cache();
    let file = dir.path().join("sample.txt");
    std::fs::write(&file, "alpha\nbeta\n").unwrap();
    let output = assert_success(
        tokenzero_cmd()
            .args([
                "read",
                file.to_str().unwrap(),
                "--cache-path",
                cache.to_str().unwrap(),
                "--allowed-root",
                dir.path().to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap(),
        "read golden",
    );
    assert_golden("cli/read_json.golden", &canonical_json(&output.stdout, dir.path()));
}

#[test]
fn cli_find_json_envelope_matches_golden() {
    let (dir, cache) = setup_temp_with_cache();
    std::fs::write(dir.path().join("sample.txt"), "alpha\nbeta\nalphabet\n").unwrap();
    let output = assert_success(
        tokenzero_cmd()
            .env("TOKENZERO_SEARCH_BACKEND", "internal")
            .current_dir(dir.path())
            .args([
                "find",
                "alpha",
                "sample.txt",
                "--cache-path",
                cache.to_str().unwrap(),
                "--allowed-root",
                dir.path().to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap(),
        "find golden",
    );
    assert_golden("cli/find_json.golden", &canonical_json(&output.stdout, dir.path()));
}

#[test]
fn cli_adapter_approval_template_json_matches_golden() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("adapter-approval-template.json");
    let output = golden_tokenzero_with_rc(
        &[
            "adapter-approval-template",
            "--output-json",
            output_json.to_str().unwrap(),
            "--json",
        ],
        dir.path(),
    );
    assert_stdout_and_file_golden(
        &output.stdout,
        &output_json,
        dir.path(),
        "cli/adapter_approval_template_json.golden",
    );
}

#[test]
fn cli_adapter_approval_audit_missing_review_json_matches_golden() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("adapter-approval-audit.json");
    let output = golden_tokenzero_with_rc(
        &[
            "adapter-approval-audit",
            "--output-json",
            output_json.to_str().unwrap(),
            "--json",
        ],
        dir.path(),
    );
    assert_stdout_and_file_golden(
        &output.stdout,
        &output_json,
        dir.path(),
        "cli/adapter_approval_audit_missing_review_json.golden",
    );
}

#[test]
fn cli_adapter_approval_audit_reviewed_without_execution_json_matches_golden() {
    let dir = tempdir().unwrap();
    let template_json = dir.path().join("adapter-approval-template.json");
    let audit_json = dir.path().join("adapter-approval-audit.json");
    assert_success(
        tokenzero_cmd()
            .env("TOKENZERO_RELEASE_CANDIDATE_ID", GOLDEN_RELEASE_CANDIDATE_ID)
            .args([
                "adapter-approval-template",
                "--output-json",
                template_json.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap(),
        "template",
    );
    let output = assert_success(
        tokenzero_cmd()
            .env("TOKENZERO_RELEASE_CANDIDATE_ID", GOLDEN_RELEASE_CANDIDATE_ID)
            .args([
                "adapter-approval-audit",
                "--approval-file",
                template_json.to_str().unwrap(),
                "--output-json",
                audit_json.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap(),
        "adapter reviewed",
    );
    assert_stdout_and_file_golden(
        &output.stdout,
        &audit_json,
        dir.path(),
        "cli/adapter_approval_audit_reviewed_no_execution_json.golden",
    );
}

#[test]
fn cli_completion_audit_json_matches_golden() {
    let dir = tempdir().unwrap();
    write_completion_handoff_fixture(dir.path());
    let output_json = dir.path().join("completion-audit.json");
    let output = assert_success(
        tokenzero_cmd()
            .current_dir(dir.path())
            .env("TOKENZERO_RELEASE_CANDIDATE_ID", GOLDEN_RELEASE_CANDIDATE_ID)
            .args([
                "completion-audit",
                "--output-json",
                output_json.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap(),
        "completion golden",
    );
    assert_stdout_and_file_golden(
        &output.stdout,
        &output_json,
        dir.path(),
        "cli/completion_audit_json.golden",
    );
}

#[test]
fn cli_artifact_handoff_json_matches_golden() {
    let dir = tempdir().unwrap();
    write_completion_handoff_fixture(dir.path());
    let results_dir = dir.path().join("results").join("current");
    let completion_json = results_dir.join("tokenzero_completion_audit.json");
    let handoff_json = dir.path().join("artifact-handoff.json");
    assert_success(
        tokenzero_cmd()
            .current_dir(dir.path())
            .env("TOKENZERO_RELEASE_CANDIDATE_ID", GOLDEN_RELEASE_CANDIDATE_ID)
            .args([
                "completion-audit",
                "--output-json",
                completion_json.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap(),
        "completion for handoff",
    );
    let output = assert_success(
        tokenzero_cmd()
            .current_dir(dir.path())
            .env("TOKENZERO_RELEASE_CANDIDATE_ID", GOLDEN_RELEASE_CANDIDATE_ID)
            .env("PATH", "")
            .args([
                "artifact-handoff",
                "--output-json",
                handoff_json.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap(),
        "handoff golden",
    );
    assert_stdout_and_file_golden(
        &output.stdout,
        &handoff_json,
        dir.path(),
        "cli/artifact_handoff_json.golden",
    );
}

#[cfg(unix)]
#[test]
fn cli_run_failure_json_envelope_matches_golden() {
    let (dir, cache) = setup_temp_with_cache();
    let output = assert_success(
        tokenzero_cmd()
            .args([
                "run",
                "--json",
                "--cache-path",
                cache.to_str().unwrap(),
                "--allowed-root",
                dir.path().to_str().unwrap(),
                "--cwd",
                dir.path().to_str().unwrap(),
                "--",
                "sh",
                "-c",
                "printf alpha; printf beta >&2; exit 7",
            ])
            .output()
            .unwrap(),
        "run failure golden",
    );
    assert_golden("cli/run_failure_json.golden", &canonical_json(&output.stdout, dir.path()));
}

fn canonical_json(bytes: &[u8], temp_root: &Path) -> String {
    let mut value: Value = serde_json::from_slice(bytes).expect("valid JSON envelope");
    let mut scrubber = RefScrubber::default();
    scrub_value(&mut value, temp_root, &mut scrubber);
    let mut text = serde_json::to_string_pretty(&value).unwrap();
    text.push('\n');
    text
}

fn scrub_value(value: &mut Value, temp_root: &Path, refs: &mut RefScrubber) {
    match value {
        Value::Object(object) => {
            if object.get("kind") == Some(&json!("capture")) {
                if let Some(bytes) = object.get_mut("bytes") {
                    if bytes.is_number() {
                        *bytes = json!("[DYNAMIC_CAPTURE_BYTES]");
                    }
                }
            }
            for (key, value) in object {
                if key == "latency_ms" && value.is_number() {
                    *value = json!("[DYNAMIC_LATENCY_MS]");
                } else if (key == "from_hwm" || key == "to_hwm") && value.is_number() {
                    *value = json!("[DYNAMIC_HWM]");
                } else {
                    scrub_value(value, temp_root, refs);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                scrub_value(value, temp_root, refs);
            }
        }
        Value::String(text) => {
            let text_without_temp_root = scrub_temp_path(text, temp_root);
            *text = refs.scrub(&text_without_temp_root);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn scrub_temp_path(text: &str, temp_root: &Path) -> String {
    let temp = normalize_path(&temp_root.to_string_lossy());
    let workspace = normalize_path(&workspace_root().to_string_lossy());
    let current_exe = normalize_path(env!("CARGO_BIN_EXE_tokenzero"));
    normalize_path(text)
        .replace(&current_exe, "[WORKSPACE]/target/debug/tokenzero")
        .replace(&temp, "[TMP]")
        .replace(&workspace, "[WORKSPACE]")
        .replace("/target/debug/tokenzero.exe", "/target/debug/tokenzero")
        .replace("/target/release/tokenzero.exe", "/target/release/tokenzero")
}

fn normalize_path(text: &str) -> String {
    text.replace('\\', "/")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn write_completion_handoff_fixture(root: &Path) {
    let results_dir = results_current_dir(root);
    write_source_currency_fixture(&results_dir);
    write_claim_audit_fixture(&results_dir);
    write_os_reach_fixture(&results_dir);
    for (file_name, schema_version, release_bound) in [
        ("tokenzero_adapter_approval_audit.json", "tokenzero.adapter_approval_audit.v1", true),
        ("tokenzero_adapter_approval_file.json", "tokenzero.adapter_approval_file.v1", false),
        ("tokenzero_artifact_handoff.json", "tokenzero.artifact_handoff.v1", true),
        ("tokenzero_bench_competitors_shell_heavy.json", "tokenzero.bench.v1", true),
        ("tokenzero_exact_recovery_audit.json", "tokenzero.exact_recovery_audit.v1", true),
        ("tokenzero_exact_recovery_shell.json", "tokenzero.exact_recovery_shell.v1", false),
        ("tokenzero_false_success_shell.json", "tokenzero.false_success_shell.v1", false),
        ("tokenzero_one_shot_eval.json", "tokenzero.one_shot_eval.v1", true),
        ("tokenzero_os_release_artifact.json", "tokenzero.os_release_artifact.v1", true),
        ("tokenzero_protected_anchor_audit.json", "tokenzero.protected_anchor_audit.v1", false),
        ("tokenzero_reach.json", "tokenzero.reach.v1", false),
        ("rust_mcp_smoke.json", "tokenzero.rust_mcp_churn.v1", false),
        ("tokenzero_security_privacy_audit.json", "tokenzero.security_privacy_audit.v1", false),
        ("tokenzero_shell_matrix.json", "tokenzero.shell_matrix.v1", false),
    ] {
        write_schema_pretty(&results_dir, file_name, schema_version, release_bound);
    }
    let docs_dir = root.join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();
    std::fs::write(
        docs_dir.join("advanced-adr-execution-record.md"),
        "## ADR-053 Fixture\nFailure-first evidence:\nResidual gates:\nvalidate_prd_goal.py\ncargo test --workspace\n",
    )
    .unwrap();
    std::fs::write(
        results_dir.join("tokenzero_competitive_superiority_reconciliation.md"),
        "## Snapshot\nno gated action was performed\n",
    )
    .unwrap();
}

fn write_source_currency_fixture(results_dir: &Path) {
    write_json_fixture_pretty(
        &results_dir.join("tokenzero_source_currency.json"),
        &json!({
            "schema_version": "tokenzero.source_currency.v1",
            "release_candidate_id": GOLDEN_RELEASE_CANDIDATE_ID,
            "fresh_for_public_claim": true,
            "blocked_reasons": []
        }),
    );
}

fn write_claim_audit_fixture(results_dir: &Path) {
    write_json_fixture(&results_dir.join("tokenzero_claim_audit.json"), &json!({
        "schema_version": "tokenzero.claim_audit.v1",
        "release_candidate_id": GOLDEN_RELEASE_CANDIDATE_ID,
        "public_claims_approved": false,
        "blocked_reasons": ["release approval not granted"],
        "evidence_gates": [
            {"id": "source_currency", "pass": true, "reasons": []},
            {
                "id": "benchmark_artifact",
                "pass": false,
                "reasons": [
                    "benchmark artifact not approved for publication",
                    "benchmark competitor rows must be runnable for public claims"
                ]
            },
            {
                "id": "adapter_approval",
                "pass": false,
                "reasons": [
                    "adapter approval artifact does not allow execution",
                    "adapter approval artifact not approved for public claims"
                ]
            },
            {
                "id": "os_artifact",
                "pass": false,
                "reasons": ["OS artifact set not approved for public claim"]
            },
            {
                "id": "release_candidate",
                "pass": true,
                "reasons": [],
                "details": {
                    "release_candidate_ids": [GOLDEN_RELEASE_CANDIDATE_ID],
                    "artifacts": [
                        {
                            "artifact_id": "source_artifact",
                            "artifact_path": "results/current/tokenzero_source_currency.json",
                            "release_candidate_id": GOLDEN_RELEASE_CANDIDATE_ID,
                            "schema_version": "tokenzero.source_currency.v1"
                        },
                        {
                            "artifact_id": "adapter_approval_artifact",
                            "artifact_path": "results/current/tokenzero_adapter_approval_audit.json",
                            "release_candidate_id": GOLDEN_RELEASE_CANDIDATE_ID,
                            "schema_version": "tokenzero.adapter_approval_audit.v1"
                        }
                    ]
                }
            },
            {
                "id": "release_approval",
                "pass": false,
                "reasons": ["release approval not granted"]
            }
        ]
    }));
}

fn write_os_reach_fixture(results_dir: &Path) {
    write_json_fixture_pretty(
        &results_dir.join("tokenzero_os_reach_audit.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.os_reach_audit.v1",
            "release_candidate_id": GOLDEN_RELEASE_CANDIDATE_ID,
            "all_release_oses_run": false,
            "public_os_claim_approved": false,
            "blocked_reasons": ["macos not run with shell and install artifacts"],
            "os_rows": [
                {"os": "windows", "claim_ready": true},
                {"os": "linux", "claim_ready": true},
                {"os": "macos", "claim_ready": false}
            ]
        }),
    );
}

fn write_schema_pretty(
    results_dir: &Path,
    file_name: &str,
    schema_version: &str,
    release_bound: bool,
) {
    let mut artifact = json!({
        "schema_version": schema_version,
        "ok": true
    });
    if release_bound {
        artifact["release_candidate_id"] = json!(GOLDEN_RELEASE_CANDIDATE_ID);
    }
    write_json_fixture_pretty(&results_dir.join(file_name), &artifact);
}

#[derive(Default)]
struct RefScrubber {
    replacements: HashMap<String, String>,
    blob_count: usize,
    file_count: usize,
    search_count: usize,
}

impl RefScrubber {
    fn scrub(&mut self, text: &str) -> String {
        let mut output = String::with_capacity(text.len());
        let mut cursor = 0;
        while let Some(relative_start) = text[cursor..].find("tz://") {
            let start = cursor + relative_start;
            output.push_str(&text[cursor..start]);
            if let Some((end, kind)) = ref_end_and_kind(&text[start..]) {
                let end = start + end;
                let original = &text[start..end];
                let replacement = self.replacement(original, kind);
                output.push_str(&replacement);
                cursor = end;
            } else {
                output.push_str("tz://");
                cursor = start + "tz://".len();
            }
        }
        output.push_str(&text[cursor..]);
        output
    }

    fn replacement(&mut self, original: &str, kind: RefKind) -> String {
        if let Some(replacement) = self.replacements.get(original) {
            return replacement.clone();
        }
        let replacement = match kind {
            RefKind::Blob => {
                self.blob_count += 1;
                format!("tz://blob/[BLOB_REF_{}]", self.blob_count)
            }
            RefKind::File => {
                self.file_count += 1;
                format!("tz://file/[FILE_REF_{}]", self.file_count)
            }
            RefKind::Search => {
                self.search_count += 1;
                format!("tz://search/[SEARCH_REF_{}]", self.search_count)
            }
        };
        self.replacements
            .insert(original.to_string(), replacement.clone());
        replacement
    }
}

#[derive(Clone, Copy)]
enum RefKind {
    Blob,
    File,
    Search,
}

fn ref_end_and_kind(text: &str) -> Option<(usize, RefKind)> {
    for (prefix, kind) in [
        ("tz://blob/", RefKind::Blob),
        ("tz://file/", RefKind::File),
        ("tz://search/h", RefKind::Search),
    ] {
        if let Some(rest) = text.strip_prefix(prefix) {
            let digest_len = rest.chars().take_while(|ch| ch.is_ascii_hexdigit()).count();
            if digest_len > 0 {
                return Some((prefix.len() + digest_len, kind));
            }
        }
    }
    None
}

fn assert_golden(relative_path: &str, actual: &str) {
    let golden_path = golden_root().join(relative_path);
    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        std::fs::create_dir_all(golden_path.parent().unwrap()).unwrap();
        std::fs::write(&golden_path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&golden_path).unwrap_or_else(|err| {
        panic!(
            "Golden file missing: {}\n{err}\nRun with UPDATE_GOLDENS=1 cargo test -p tokenzero --test golden_outputs",
            golden_path.display()
        )
    });
    if actual != expected {
        let actual_path = golden_path.with_extension("actual");
        std::fs::write(&actual_path, actual).unwrap();
        panic!(
            "GOLDEN MISMATCH: {relative_path}\n\n{}\n\nTo update: UPDATE_GOLDENS=1 cargo test -p tokenzero --test golden_outputs\nTo review: diff -u {} {}",
            unified_diff(&expected, actual),
            golden_path.display(),
            actual_path.display()
        );
    }
}

fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
}

fn unified_diff(expected: &str, actual: &str) -> String {
    let expected_lines: Vec<_> = expected.lines().collect();
    let actual_lines: Vec<_> = actual.lines().collect();
    let mut diff = String::from("--- expected\n+++ actual\n");
    let max_len = expected_lines.len().max(actual_lines.len());
    for index in 0..max_len {
        match (expected_lines.get(index), actual_lines.get(index)) {
            (Some(expected), Some(actual)) if expected == actual => {}
            (Some(expected), Some(actual)) => {
                diff.push_str(&format!("@@ line {} @@\n", index + 1));
                diff.push_str(&format!("-{expected}\n"));
                diff.push_str(&format!("+{actual}\n"));
            }
            (Some(expected), None) => {
                diff.push_str(&format!("@@ line {} @@\n", index + 1));
                diff.push_str(&format!("-{expected}\n"));
            }
            (None, Some(actual)) => {
                diff.push_str(&format!("@@ line {} @@\n", index + 1));
                diff.push_str(&format!("+{actual}\n"));
            }
            (None, None) => {}
        }
    }
    diff
}
