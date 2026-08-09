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
            .env(
                "TOKENZERO_RELEASE_CANDIDATE_ID",
                GOLDEN_RELEASE_CANDIDATE_ID,
            )
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
#[derive(Clone, Copy)]
enum ArtifactSetup {
    None,
    Reviewed,
    Completion,
    Handoff,
}
struct ArtifactCase {
    id: &'static str,
    command: &'static str,
    file: &'static str,
    golden: &'static str,
    setup: ArtifactSetup,
}
macro_rules! artifact_cases {
    ($($id:literal: $command:literal, $file:literal, $golden:literal, $setup:ident);+ $(;)?) => {
        [$(ArtifactCase { id: $id, command: $command, file: $file, golden: $golden, setup: ArtifactSetup::$setup }),+]
    };
}
fn run_primary_golden(id: &str, kind: &str, dir: &tempfile::TempDir, cache: &Path) {
    let file = dir.path().join("sample.txt");
    match kind {
        "read" => {
            std::fs::write(&file, "alpha\nbeta\n").unwrap();
            let json = run_tool_json("read", &[file.to_str().unwrap()], dir.path(), cache);
            assert_golden(
                "cli/read_json.golden",
                &canonical_json(&serde_json::to_vec(&json).unwrap(), dir.path()),
            );
        }
        "find" => {
            // 5irj: two adjacent matches (alpha@L1, alphabet@L3) whose
            // TARGET_CONTEXT_LINES windows clamp to the same L1-L3 range;
            // hit_search_output dedupes the byte-identical window to one HIT
            // record, and this golden pins that collapsed output.
            std::fs::write(&file, "alpha\nbeta\nalphabet\n").unwrap();
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
                id,
            );
            assert_golden(
                "cli/find_json.golden",
                &canonical_json(&output.stdout, dir.path()),
            );
        }
        _ => unreachable!("{kind}"),
    }
}
fn run_artifact_case(case: &ArtifactCase) {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join(case.file);
    let mut approval = None;
    match case.setup {
        ArtifactSetup::None => {}
        ArtifactSetup::Reviewed => {
            let template = dir.path().join("adapter-approval-template.json");
            golden_tokenzero_with_rc(
                &[
                    "adapter-approval-template",
                    "--output-json",
                    template.to_str().unwrap(),
                    "--json",
                ],
                dir.path(),
            );
            approval = Some(template);
        }
        ArtifactSetup::Completion | ArtifactSetup::Handoff => {
            write_completion_handoff_fixture(dir.path());
            if matches!(case.setup, ArtifactSetup::Handoff) {
                let completion =
                    results_current_dir(dir.path()).join("tokenzero_completion_audit.json");
                golden_tokenzero_with_rc(
                    &[
                        "completion-audit",
                        "--output-json",
                        completion.to_str().unwrap(),
                        "--json",
                    ],
                    dir.path(),
                );
            }
        }
    }
    let mut command = tokenzero_cmd();
    command.current_dir(dir.path()).env(
        "TOKENZERO_RELEASE_CANDIDATE_ID",
        GOLDEN_RELEASE_CANDIDATE_ID,
    );
    if matches!(case.setup, ArtifactSetup::Handoff) {
        command.env("PATH", "");
    }
    command.arg(case.command);
    if let Some(path) = approval.as_ref() {
        command.args(["--approval-file", path.to_str().unwrap()]);
    }
    let output = assert_success(
        command
            .args(["--output-json", output_json.to_str().unwrap(), "--json"])
            .output()
            .unwrap(),
        case.id,
    );
    assert_stdout_and_file_golden(
        &output.stdout,
        &output_json,
        dir.path(),
        &format!("cli/{}", case.golden),
    );
}

#[test]
fn cli_primary_and_artifact_golden_matrix() {
    let (dir, cache) = setup_temp_with_cache();
    for (id, kind) in [("G01", "read"), ("G02", "find")] {
        run_primary_golden(id, kind, &dir, &cache);
    }
    let cases = artifact_cases! {
        "A01": "adapter-approval-template", "adapter-approval-template.json", "adapter_approval_template_json.golden", None;
        "A02": "adapter-approval-audit", "adapter-approval-audit.json", "adapter_approval_audit_missing_review_json.golden", None;
        "A03": "adapter-approval-audit", "adapter-approval-audit.json", "adapter_approval_audit_reviewed_no_execution_json.golden", Reviewed;
        "A04": "completion-audit", "completion-audit.json", "completion_audit_json.golden", Completion;
        "A05": "artifact-handoff", "artifact-handoff.json", "artifact_handoff_json.golden", Handoff;
    };
    for case in &cases {
        run_artifact_case(case);
    }
}

#[cfg(unix)]
#[test]
fn cli_run_failure_json_envelope_matches_golden() {
    let (dir, cache) = setup_temp_with_cache();
    let output = tokenzero_cmd()
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
        .unwrap();
    // nt0i (1cwf flip): the process mirrors the child exit code; the JSON
    // envelope itself is unchanged and still matches the golden.
    assert_eq!(
        output.status.code(),
        Some(7),
        "run failure golden must mirror child exit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_golden(
        "cli/run_failure_json.golden",
        &canonical_json(&output.stdout, dir.path()),
    );
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
        Value::Array(values) => values
            .iter_mut()
            .for_each(|v| scrub_value(v, temp_root, refs)),
        Value::String(text) => {
            let scrubbed = scrub_temp_path(text, temp_root);
            *text = refs.scrub(&scrubbed);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
fn scrub_temp_path(text: &str, temp_root: &Path) -> String {
    let temp = normalize_path(&temp_root.to_string_lossy());
    let workspace = normalize_path(&workspace_root().to_string_lossy());
    // hn67: scrub the release binary path regardless of how the executable was
    // spawned. On rch workers the build target dir lives behind a symlink
    // (/Users/... -> /home/...), so the compile-time CARGO_BIN_EXE_tokenzero
    // literal and the runtime current_exe() can disagree by symlink
    // resolution. Replace both the literal and its canonical form with the
    // stable [WORKSPACE]/target placeholder.
    let current_exe = normalize_path(env!("CARGO_BIN_EXE_tokenzero"));
    let current_exe_canonical = std::fs::canonicalize(env!("CARGO_BIN_EXE_tokenzero"))
        .map(|p| normalize_path(&p.to_string_lossy()))
        .unwrap_or_else(|_| current_exe.clone());
    normalize_path(text)
        .replace(&current_exe, "[WORKSPACE]/target/debug/tokenzero")
        .replace(&current_exe_canonical, "[WORKSPACE]/target/debug/tokenzero")
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
    for &(stem, release_bound) in &[
        ("adapter_approval_audit", true),
        ("adapter_approval_file", false),
        ("artifact_handoff", true),
        ("bench_competitors_shell_heavy", true),
        ("exact_recovery_audit", true),
        ("exact_recovery_shell", false),
        ("false_success_shell", false),
        ("one_shot_eval", true),
        ("os_release_artifact", true),
        ("protected_anchor_audit", false),
        ("reach", false),
        ("mcp_smoke", false),
        ("security_privacy_audit", false),
        ("shell_matrix", false),
    ] {
        let (prefix, schema) = match stem {
            "bench_competitors_shell_heavy" => ("tokenzero", "bench"),
            "mcp_smoke" => ("rust", "rust_mcp_churn"),
            _ => ("tokenzero", stem),
        };
        write_schema_pretty(
            &results_dir,
            &format!("{prefix}_{stem}.json"),
            &format!("tokenzero.{schema}.v1"),
            release_bound,
        );
    }
    let docs_dir = root.join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();
    std::fs::write(docs_dir.join("advanced-adr-execution-record.md"),
        "## ADR-053 Fixture\nFailure-first evidence:\nResidual gates:\nvalidate_prd_goal.py\ncargo test --workspace\n").unwrap();
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
            "fresh_for_public_claim": true, "blocked_reasons": []
        }),
    );
}
fn evidence_gate(id: &str, pass: bool, reasons: &[&str]) -> Value {
    json!({"id": id, "pass": pass, "reasons": reasons})
}
fn write_claim_audit_fixture(results_dir: &Path) {
    let mut release_candidate = evidence_gate("release_candidate", true, &[]);
    release_candidate["details"] = json!({
        "release_candidate_ids": [GOLDEN_RELEASE_CANDIDATE_ID],
        "artifacts": [
            {"artifact_id": "source_artifact", "artifact_path": "results/current/tokenzero_source_currency.json",
             "release_candidate_id": GOLDEN_RELEASE_CANDIDATE_ID, "schema_version": "tokenzero.source_currency.v1"},
            {"artifact_id": "adapter_approval_artifact", "artifact_path": "results/current/tokenzero_adapter_approval_audit.json",
             "release_candidate_id": GOLDEN_RELEASE_CANDIDATE_ID, "schema_version": "tokenzero.adapter_approval_audit.v1"}
        ]
    });
    let gates = [
        evidence_gate("source_currency", true, &[]),
        evidence_gate(
            "benchmark_artifact",
            false,
            &[
                "benchmark artifact not approved for publication",
                "benchmark competitor rows must be runnable for public claims",
            ],
        ),
        evidence_gate(
            "adapter_approval",
            false,
            &[
                "adapter approval artifact does not allow execution",
                "adapter approval artifact not approved for public claims",
            ],
        ),
        evidence_gate(
            "os_artifact",
            false,
            &["OS artifact set not approved for public claim"],
        ),
        release_candidate,
        evidence_gate("release_approval", false, &["release approval not granted"]),
    ];
    write_json_fixture(
        &results_dir.join("tokenzero_claim_audit.json"),
        &json!({
            "schema_version": "tokenzero.claim_audit.v1",
            "release_candidate_id": GOLDEN_RELEASE_CANDIDATE_ID,
            "public_claims_approved": false,
            "blocked_reasons": ["release approval not granted"],
            "evidence_gates": gates
        }),
    );
}
fn write_os_reach_fixture(results_dir: &Path) {
    write_json_fixture_pretty(
        &results_dir.join("tokenzero_os_reach_audit.json"),
        &serde_json::json!({
            "schema_version": "tokenzero.os_reach_audit.v1",
            "release_candidate_id": GOLDEN_RELEASE_CANDIDATE_ID,
            "all_release_oses_run": false, "public_os_claim_approved": false,
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
    let mut artifact = json!({"schema_version": schema_version, "ok": true});
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
                output.push_str(&self.replacement(&text[start..end], kind));
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
        panic!("Golden file missing: {}\n{err}\nRun with UPDATE_GOLDENS=1 cargo test -p tokenzero --test golden_outputs",
            golden_path.display())
    });
    if actual != expected {
        let actual_path = golden_path.with_extension("actual");
        std::fs::write(&actual_path, actual).unwrap();
        panic!(
            "GOLDEN MISMATCH: {relative_path}\n\nTo update: UPDATE_GOLDENS=1 cargo test -p tokenzero --test golden_outputs\nTo review: diff -u {} {}",
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
