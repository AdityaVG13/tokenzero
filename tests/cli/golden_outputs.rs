mod common;
use common::*;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
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
    //
    // R1 (CONF-P10-H03): rch may rewrite CARGO_TARGET_DIR to
    // `.rch-target-spark-*` even when the caller set
    // `/tmp/rch_target_tokenzero`. JSON that names either directory (or a
    // profile other than the compile-time CARGO_BIN_EXE spelling) must still
    // collapse. Live read_json/run_failure goldens did not contain these
    // strings; the extra CLI field was `tz://capsule/<digest>` (causal key
    // includes the host path; RefScrubber now collapses that scheme).
    let current_exe = normalize_path(env!("CARGO_BIN_EXE_tokenzero"));
    let current_exe_canonical = std::fs::canonicalize(env!("CARGO_BIN_EXE_tokenzero"))
        .map(|p| normalize_path(&p.to_string_lossy()))
        .unwrap_or_else(|_| current_exe.clone());
    let mut out = normalize_path(text)
        .replace(&current_exe, "[WORKSPACE]/target/debug/tokenzero")
        .replace(&current_exe_canonical, "[WORKSPACE]/target/debug/tokenzero");
    out = collapse_cargo_target_dir_binaries(&out);
    out = collapse_rch_spark_binaries(&out);
    out.replace(&temp, "[TMP]")
        .replace(&workspace, "[WORKSPACE]")
        .replace("/target/debug/tokenzero.exe", "/target/debug/tokenzero")
        .replace("/target/release/tokenzero.exe", "/target/release/tokenzero")
}

fn home_users_alt(path: &str) -> Option<String> {
    path.strip_prefix("/Users/")
        .map(|rest| format!("/home/{rest}"))
        .or_else(|| {
            path.strip_prefix("/home/")
                .map(|rest| format!("/Users/{rest}"))
        })
}

fn collapse_cargo_target_dir_binaries(text: &str) -> String {
    const PLACEHOLDER: &str = "[WORKSPACE]/target/debug/tokenzero";
    let mut dirs = Vec::new();
    if let Ok(td) = std::env::var("CARGO_TARGET_DIR") {
        if !td.is_empty() {
            dirs.push(normalize_path(&td));
            if let Ok(canonical) = std::fs::canonicalize(&td) {
                dirs.push(normalize_path(&canonical.to_string_lossy()));
            }
        }
    }
    dirs.push("/tmp/rch_target_tokenzero".to_string());
    dirs.sort();
    dirs.dedup();
    let mut out = text.to_string();
    for dir in dirs {
        let mut alts = vec![dir.clone()];
        if let Some(alt) = home_users_alt(&dir) {
            alts.push(alt);
        }
        for alt in alts {
            for profile in ["debug", "release", "release-perf"] {
                out = out.replace(&format!("{alt}/{profile}/tokenzero"), PLACEHOLDER);
            }
        }
    }
    out
}

fn collapse_rch_spark_binaries(text: &str) -> String {
    const NEEDLE: &str = ".rch-target-spark-";
    const PLACEHOLDER: &str = "[WORKSPACE]/target/debug/tokenzero";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = rest.find(NEEDLE) {
        let prefix = &rest[..idx];
        let abs_start = prefix
            .rfind("/Users/")
            .or_else(|| prefix.rfind("/home/"))
            .unwrap_or(prefix.len());
        out.push_str(&rest[..abs_start]);
        let tail = &rest[idx + NEEDLE.len()..];
        let after_hash = tail.find('/').map(|i| &tail[i..]).unwrap_or("");
        let skipped = after_hash
            .strip_prefix("/debug/tokenzero")
            .or_else(|| after_hash.strip_prefix("/release/tokenzero"))
            .or_else(|| after_hash.strip_prefix("/release-perf/tokenzero"));
        out.push_str(PLACEHOLDER);
        rest = skipped.unwrap_or(after_hash);
    }
    out.push_str(rest);
    out
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
    capsule_count: usize,
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
            RefKind::Capsule => {
                self.capsule_count += 1;
                format!("tz://capsule/[CAPSULE_REF_{}]", self.capsule_count)
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
    Capsule,
}
fn ref_end_and_kind(text: &str) -> Option<(usize, RefKind)> {
    for (prefix, kind) in [
        ("tz://blob/", RefKind::Blob),
        ("tz://file/", RefKind::File),
        ("tz://search/h", RefKind::Search),
        ("tz://capsule/", RefKind::Capsule),
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
/// K-9 first-divergence: line-oriented unified hunk of expected vs actual
/// canonical JSON. Panic text must name `-p tokenzero-cli` (crate dir
/// `crates/tokenzero`); `-p tokenzero` matches no package.
fn first_divergence_unified(expected: &str, actual: &str) -> String {
    let exp: Vec<&str> = expected.lines().collect();
    let act: Vec<&str> = actual.lines().collect();
    let n = exp.len().max(act.len());
    let mut first = None;
    for i in 0..n {
        if exp.get(i).copied() != act.get(i).copied() {
            first = Some(i);
            break;
        }
    }
    let Some(idx) = first else {
        return "first_divergence: none (identical lines; trailing-newline or NUL mismatch)\n"
            .to_string();
    };
    let ctx = 3usize;
    let start = idx.saturating_sub(ctx);
    let end = (idx + 16).min(n);
    let mut out = format!(
        "first_divergence: line {idx} (1-based {})\n--- expected (golden)\n+++ actual (canonical_json)\n@@ -{start} +{start} @@\n",
        idx + 1
    );
    for i in start..end {
        let e = exp.get(i).copied();
        let a = act.get(i).copied();
        if e == a {
            if let Some(line) = e {
                out.push_str(&format!(" {line}\n"));
            }
            continue;
        }
        match e {
            Some(line) => out.push_str(&format!("-{line}\n")),
            None => out.push_str("-<missing line>\n"),
        }
        match a {
            Some(line) => out.push_str(&format!("+{line}\n")),
            None => out.push_str("+<missing line>\n"),
        }
    }
    let remaining = n.saturating_sub(end);
    if remaining > 0 {
        out.push_str(&format!(
            "... ({remaining} more lines follow; json_pointer_diffs lists every field)\n"
        ));
    }
    out
}

fn json_pointer_diffs(expected: &str, actual: &str) -> String {
    let exp: Value = match serde_json::from_str(expected) {
        Ok(v) => v,
        Err(err) => return format!("json_pointer_diffs: expected not JSON: {err}"),
    };
    let act: Value = match serde_json::from_str(actual) {
        Ok(v) => v,
        Err(err) => return format!("json_pointer_diffs: actual not JSON: {err}"),
    };
    let mut rows = Vec::new();
    walk_json_diffs(&mut rows, "", &exp, &act);
    if rows.is_empty() {
        return "json_pointer_diffs: none (line mismatch is whitespace/key-order only)\n"
            .to_string();
    }
    let mut out = format!("json_pointer_diffs: {} field(s)\n", rows.len());
    for row in rows {
        out.push_str(&row);
        out.push('\n');
    }
    out
}

fn walk_json_diffs(rows: &mut Vec<String>, pointer: &str, expected: &Value, actual: &Value) {
    if expected == actual {
        return;
    }
    match (expected, actual) {
        (Value::Object(exp), Value::Object(act)) => {
            let mut keys: Vec<&str> = exp.keys().map(String::as_str).collect();
            for key in act.keys() {
                if !exp.contains_key(key) {
                    keys.push(key.as_str());
                }
            }
            keys.sort_unstable();
            keys.dedup();
            for key in keys {
                let child = if pointer.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{pointer}/{key}")
                };
                match (exp.get(key), act.get(key)) {
                    (Some(e), Some(a)) => walk_json_diffs(rows, &child, e, a),
                    (Some(e), None) => rows.push(format!("{child}: expected {e} ; actual <missing>")),
                    (None, Some(a)) => rows.push(format!("{child}: expected <missing> ; actual {a}")),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(exp), Value::Array(act)) => {
            let n = exp.len().max(act.len());
            for i in 0..n {
                let child = format!("{pointer}/{i}");
                match (exp.get(i), act.get(i)) {
                    (Some(e), Some(a)) => walk_json_diffs(rows, &child, e, a),
                    (Some(e), None) => rows.push(format!("{child}: expected {e} ; actual <missing>")),
                    (None, Some(a)) => rows.push(format!("{child}: expected <missing> ; actual {a}")),
                    (None, None) => {}
                }
            }
        }
        _ => rows.push(format!("{pointer}: expected {expected} ; actual {actual}")),
    }
}

fn inherited_tokenizer_env_hint() -> String {
    format!(
        "inherited_tokenizer_env: TOKENZERO_MODEL={:?} OMP_MODEL={:?} OPENAI_MODEL={:?}",
        std::env::var("TOKENZERO_MODEL").ok(),
        std::env::var("OMP_MODEL").ok(),
        std::env::var("OPENAI_MODEL").ok(),
    )
}

fn leftover_host_path_hint(actual: &str) -> String {
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "<unset>".into());
    format!(
        "host_path_leftovers: /tmp/rch_target_tokenzero={} /home/={} /Users/={} .rch-target-spark={} raw_tz_capsule={} CARGO_TARGET_DIR={target_dir}",
        actual.contains("/tmp/rch_target_tokenzero"),
        actual.contains("/home/"),
        actual.contains("/Users/"),
        actual.contains(".rch-target-spark"),
        actual.contains("tz://capsule/") && !actual.contains("tz://capsule/[CAPSULE_REF"),
    )
}

fn dump_golden_actual(relative_path: &str, actual: &str, expected: &str) -> Vec<PathBuf> {
    let stem = relative_path.replace('/', "_");
    let mut dirs = Vec::new();
    if let Some(td) = std::env::var_os("CARGO_TARGET_DIR") {
        dirs.push(PathBuf::from(td).join("golden_mismatch"));
    }
    dirs.push(PathBuf::from("/tmp/rch_target_tokenzero/golden_mismatch"));
    let mut dumped = Vec::new();
    let mut seen = HashSet::new();
    for dir in dirs {
        if !seen.insert(dir.clone()) {
            continue;
        }
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let actual_path = dir.join(format!("{stem}.actual"));
        let expected_path = dir.join(format!("{stem}.expected"));
        let diff_path = dir.join(format!("{stem}.diff"));
        let _ = std::fs::write(&actual_path, actual);
        let _ = std::fs::write(&expected_path, expected);
        let _ = std::fs::write(
            &diff_path,
            format!(
                "{}\n{}",
                first_divergence_unified(expected, actual),
                json_pointer_diffs(expected, actual)
            ),
        );
        dumped.push(actual_path);
    }
    dumped
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
            "Golden file missing: {}\n{err}\nReplay: rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_tokenzero cargo test -p tokenzero-cli --test golden_outputs -- --test-threads=1\nDo not UPDATE_GOLDENS=1 to force green.",
            golden_path.display()
        )
    });
    if actual != expected {
        let actual_path = golden_path.with_extension("actual");
        std::fs::write(&actual_path, actual).unwrap();
        let dumped = dump_golden_actual(relative_path, actual, &expected);
        panic!(
            "GOLDEN MISMATCH: {relative_path}\n\n{}\n{}\n{}\n{}\nCARGO_BIN_EXE={}\nCARGO_BIN_EXE_canonical={:?}\nworkspace_root={}\ndumped_actual={dumped:?}\nbeside_golden={}\n\nReplay: rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_tokenzero cargo test -p tokenzero-cli --test golden_outputs -- --test-threads=1\nDo not UPDATE_GOLDENS=1 to force green.",
            first_divergence_unified(&expected, actual),
            json_pointer_diffs(&expected, actual),
            leftover_host_path_hint(actual),
            inherited_tokenizer_env_hint(),
            env!("CARGO_BIN_EXE_tokenzero"),
            std::fs::canonicalize(env!("CARGO_BIN_EXE_tokenzero")).ok(),
            workspace_root().display(),
            actual_path.display()
        );
    }
}
fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/cli/golden")
}

#[test]
fn golden_mismatch_first_divergence_names_first_differing_line() {
    let expected = "{\n  \"cwd\": \"[TMP]\"\n}\n";
    let actual = "{\n  \"cwd\": \"/tmp/rch_target_tokenzero/debug/tokenzero\"\n}\n";
    let diff = first_divergence_unified(expected, actual);
    assert!(
        diff.contains("first_divergence: line 1 (1-based 2)"),
        "{diff}"
    );
    assert!(diff.contains("-  \"cwd\": \"[TMP]\""), "{diff}");
    assert!(
        diff.contains("+  \"cwd\": \"/tmp/rch_target_tokenzero/debug/tokenzero\""),
        "{diff}"
    );
    assert!(
        leftover_host_path_hint(actual).contains("/tmp/rch_target_tokenzero=true"),
        "{}",
        leftover_host_path_hint(actual)
    );
    let pointers = json_pointer_diffs(expected, actual);
    assert!(
        pointers.contains("/cwd: expected \"[TMP]\" ; actual \"/tmp/rch_target_tokenzero/debug/tokenzero\""),
        "{pointers}"
    );
}

#[test]
fn ref_scrubber_collapses_path_dependent_capsule_digest_refs() {
    // Live rch read_json first_divergence: extra refs/2 tz://capsule/<64-hex>
    // whose causal key includes the host temp path. Blob/file/search were
    // already collapsed; capsule was not, so leftover_host_path_hint missed it.
    let mut refs = RefScrubber::default();
    let digest = "4e6c25a9d24500c74be779d6bfd0b4e7708211df8444ab76d9e9602fa6e864c4";
    let input = format!("see tz://capsule/{digest} and again tz://capsule/{digest}");
    let got = refs.scrub(&input);
    assert_eq!(
        got,
        "see tz://capsule/[CAPSULE_REF_1] and again tz://capsule/[CAPSULE_REF_1]"
    );
    let second = refs.scrub("tz://capsule/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(second, "tz://capsule/[CAPSULE_REF_2]");
}

#[test]
fn scrub_temp_path_collapses_cargo_target_dir_and_rch_spark_binaries() {
    let temp = Path::new("/tmp/unused-golden-temp-root");
    let law = "/tmp/rch_target_tokenzero/debug/tokenzero";
    let spark = "/Users/aditya/AI/TokenZero/.rch-target-spark-1672-pool-deadbeef/debug/tokenzero";
    let spark_home =
        "/home/aditya/AI/tokenzero/.rch-target-spark-1672-pool-deadbeef/release-perf/tokenzero";
    assert_eq!(
        scrub_temp_path(law, temp),
        "[WORKSPACE]/target/debug/tokenzero"
    );
    assert_eq!(
        scrub_temp_path(spark, temp),
        "[WORKSPACE]/target/debug/tokenzero"
    );
    assert_eq!(
        scrub_temp_path(spark_home, temp),
        "[WORKSPACE]/target/debug/tokenzero"
    );
}
