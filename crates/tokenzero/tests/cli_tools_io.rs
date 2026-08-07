mod common;
use assert_cmd::Command as AssertCmd;
use common::*;
macro_rules! argv { ($($arg:expr);+ $(;)?) => { &[$($arg),+] }; }
macro_rules! fields {
    ($actual:expr; $($pointer:literal => $value:tt);+ $(;)?) => {
        $(assert_eq!($actual.pointer($pointer), Some(&serde_json::json!($value)), "{}", $pointer);)+
    };
}
fn assert_json_error(output: &std::process::Output, code: &str) {
    assert!(!output.status.success());
    let json = parse_json_stdout(output);
    fields!(json;"/status" =>"error");
    if json["error"]["code"].is_string() {
        assert_eq!(json["error"]["code"], code);
    } else {
        assert_eq!(json["error"]["kind"], code);
    }
}
fn assert_json_contains(value: &serde_json::Value, needle: &str) {
    assert!(value.as_str().unwrap().contains(needle));
}
use std::fs;
use tempfile::tempdir;

#[test]
fn cli_read_expand_json_roundtrip() {
    let (dir, cache) = setup_temp_with_cache();
    let file = dir.path().join("sample.txt");
    std::fs::write(&file, "alpha\nbeta\n").unwrap();
    let json = run_tool_json("read", &[file.to_str().unwrap()], dir.path(), &cache);
    fields!(json;"/schema_version" =>"tokenzero.cli.v1");
    assert_eq!(
        expand_raw_text(&first_ref_with_kind(&json, "blob"), Some(&cache), None, &[]),
        "alpha\nbeta\n"
    );
}

#[test]
fn cli_expand_honors_all_refs_and_refs_from_without_unwired_force() {
    let (dir, cache) = setup_temp_with_cache();
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "alpha\n").unwrap();
    fs::write(&second, "beta\n").unwrap();
    let first_ref = first_ref_with_kind(
        &run_tool_json("read", &[first.to_str().unwrap()], dir.path(), &cache),
        "blob",
    );
    let second_ref = first_ref_with_kind(
        &run_tool_json("read", &[second.to_str().unwrap()], dir.path(), &cache),
        "blob",
    );
    let cache_arg = cache.to_str().unwrap();

    let direct = assert_success(
        tokenzero_cmd()
            .current_dir(dir.path())
            .args([
                "expand",
                &first_ref,
                &second_ref,
                "--cache-path",
                cache_arg,
                "--raw",
            ])
            .output()
            .unwrap(),
        "direct multi-ref expand",
    );
    assert_eq!(String::from_utf8(direct.stdout).unwrap(), "alpha\nbeta\n");

    let refs_file = dir.path().join("refs.txt");
    fs::write(&refs_file, format!("{first_ref}\n{second_ref}\n")).unwrap();
    let from_file = assert_success(
        tokenzero_cmd()
            .current_dir(dir.path())
            .args([
                "expand",
                "--refs-from",
                refs_file.to_str().unwrap(),
                "--cache-path",
                cache_arg,
                "--raw",
                "--json",
            ])
            .output()
            .unwrap(),
        "refs-from multi-ref expand",
    );
    let batch = parse_json_stdout(&from_file);
    assert_eq!(batch.as_array().unwrap().len(), 2, "{batch}");
    assert_eq!(batch[0]["visible"]["text"], "alpha\n");
    assert_eq!(batch[1]["visible"]["text"], "beta\n");

    let invalid_ref = "tz://blob/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let loud_error = tokenzero_cmd()
        .current_dir(dir.path())
        .args([
            "expand",
            &first_ref,
            invalid_ref,
            "--cache-path",
            cache_arg,
            "--raw",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!loud_error.status.success());
    let batch = parse_json_stdout(&loud_error);
    assert_eq!(batch[0]["status"], "ok");
    assert_eq!(batch[1]["status"], "error");
    assert_eq!(batch[1]["error"]["code"], "ref_not_found");

    let help = assert_success(
        tokenzero_cmd().args(["expand", "--help"]).output().unwrap(),
        "expand help",
    );
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("--refs-from"), "{help}");
    assert!(!help.contains("--force"), "{help}");
}

#[test]
fn cli_slim_envelope_opt_in_drops_advisory_blocks_and_keeps_durable_refs() {
    // 0ok7: TOKENZERO_SLIM_ENVELOPE=1 slims the CLI JSON envelope; the full
    // envelope is the unchanged default.
    let (dir, cache) = setup_temp_with_cache();
    let file = dir.path().join("tiny.txt");
    std::fs::write(&file, "tiny payload\n").unwrap();
    let file_arg = file.to_str().unwrap().to_string();
    let root_arg = dir.path().to_str().unwrap().to_string();
    let cache_arg = cache.to_str().unwrap().to_string();
    let args = vec![
        "read",
        file_arg.as_str(),
        "--cache-path",
        cache_arg.as_str(),
        "--allowed-root",
        root_arg.as_str(),
        "--json",
    ];

    let full = run_tokenzero_json_in_with_env(&args, dir.path(), &[]);
    fields!(full;"/schema_version" =>"tokenzero.cli.v1");
    assert!(
        full.get("telemetry").is_some(),
        "full envelope carries telemetry"
    );
    assert!(
        full.get("accounting").is_some(),
        "full envelope carries accounting"
    );

    let slim =
        run_tokenzero_json_in_with_env(&args, dir.path(), &[("TOKENZERO_SLIM_ENVELOPE", "1")]);
    fields!(slim;"/status" =>"ok"; "/tool" =>"read");
    for dropped in [
        "schema_version",
        "telemetry",
        "accounting",
        "mode",
        "content_type",
    ] {
        assert!(
            slim.get(dropped).is_none(),
            "slim envelope drops {dropped}: {slim}"
        );
    }
    // Slim flattens the visible capsule to bare text: "capsule" is the only
    // kind the CLI emits, so the {kind,text} wrapper is pure overhead.
    assert_json_contains(&slim["visible"], "tiny payload");
    // detail_ref is refs.first(); slim must not restate it as a second 74B ref.
    assert!(
        slim.get("detail_ref").is_none(),
        "slim drops detail_ref when refs already carry it: {slim}"
    );
    let ref_id = slim["refs"][0]
        .as_str()
        .expect("slim refs are bare ref strings");
    assert!(ref_id.starts_with("tz://blob/"), "{ref_id}");
    assert_eq!(
        expand_raw_text(ref_id, Some(&cache), None, &[]),
        "tiny payload\n",
        "slim refs stay durable and expand to exact bytes"
    );
    assert_eq!(
        slim["detail_ref"].as_str().or(Some(ref_id)),
        Some(ref_id),
        "detail_ref stays recoverable as refs[0]"
    );
    let slim_bytes = serde_json::to_string(&slim).unwrap().len();
    let full_bytes = serde_json::to_string(&full).unwrap().len();
    assert!(
        slim_bytes * 2 < full_bytes,
        "slim {slim_bytes}B must be under half of full {full_bytes}B"
    );
}

#[test]
fn cli_slim_envelope_overhead_is_a_flat_constant_not_a_payload_multiple() {
    // 0ok7 acceptance: for sub-1KB payloads the slim envelope must add a small
    // fixed cost (refs + status truth), never a cost that scales with content.
    let (dir, cache) = setup_temp_with_cache();
    let cache_arg = cache.to_str().unwrap().to_string();
    let root_arg = dir.path().to_str().unwrap().to_string();

    let mut overheads = Vec::new();
    for size in [64_usize, 256, 900] {
        let file = dir.path().join(format!("payload_{size}.txt"));
        let body = "x".repeat(size);
        std::fs::write(&file, &body).unwrap();
        let file_arg = file.to_str().unwrap().to_string();
        let slim = run_tokenzero_json_in_with_env(
            &[
                "read",
                file_arg.as_str(),
                "--cache-path",
                cache_arg.as_str(),
                "--allowed-root",
                root_arg.as_str(),
                "--json",
            ],
            dir.path(),
            &[("TOKENZERO_SLIM_ENVELOPE", "1")],
        );
        let bytes = serde_json::to_string(&slim).unwrap().len();
        let payload = slim["visible"].as_str().unwrap_or_default().len();
        assert!(payload >= size, "payload {payload} carries {size} content");
        overheads.push(bytes - payload);
    }

    // Overhead must be flat across a 14x payload range: no per-byte tax.
    let (min, max) = (
        *overheads.iter().min().unwrap(),
        *overheads.iter().max().unwrap(),
    );
    assert!(
        max - min <= 32,
        "slim overhead must be constant, saw {overheads:?}"
    );
    // Two durable refs (~74B each) plus status/tool/ack is the floor.
    assert!(
        max <= 260,
        "slim overhead must stay within the ref-dominated floor, saw {overheads:?}"
    );
}

#[test]
fn cli_expand_recovers_blob_across_roots_via_ref_index() {
    let (root_a, root_b, index_dir) = (tempdir().unwrap(), tempdir().unwrap(), tempdir().unwrap());
    let file = root_a.path().join("sample.txt");
    fs::write(&file, "cross\nroot\nbytes\n").unwrap();
    let cache_a = root_a.path().join("cache.json");
    let json = parse_json_stdout(&assert_success(
        tokenzero_cmd()
            .env("TOKENZERO_REF_INDEX_PATH", index_dir.path())
            .args(argv!["read";file.to_str().unwrap();"--cache-path";cache_a.to_str().unwrap();"--allowed-root";root_a.path().to_str().unwrap();"--json"])
            .output().unwrap(),
        "read a",
    ));
    let expanded = expand_raw_text(
        &first_ref_with_kind(&json, "blob"),
        None,
        Some(root_b.path()),
        argv![(
            "TOKENZERO_REF_INDEX_PATH",
            index_dir.path().to_str().unwrap()
        )],
    );
    assert_eq!(expanded, "cross\nroot\nbytes\n");
}
fn large_run_stdout_ref(
    root: &std::path::Path,
    index_dir: &std::path::Path,
    cache_env: Option<&std::path::Path>,
    payload_char: char,
) -> String {
    let expected = payload_char.to_string().repeat(28 * 1024);
    let mut cmd = tokenzero_cmd();
    cmd.current_dir(root).env("TOKENZERO_REF_INDEX_PATH", index_dir)
        .args(argv!["run";"--json";"--";"python3";"-c";&format!("import sys; sys.stdout.write({expected:?})")]);
    if let Some(cache) = cache_env {
        cmd.env("TOKENZERO_CACHE_PATH", cache);
    }
    let output = assert_success(cmd.output().unwrap(), "large run");
    if let Some(cache) = cache_env {
        assert!(
            cache.exists(),
            "TOKENZERO_CACHE_PATH should choose the store"
        );
    }
    first_ref_with_kind(&parse_json_stdout(&output), "stdout")
}
fn assert_large_run_ref_roundtrip(payload_char: char, use_env_cache: bool) {
    let (root_a, root_b, index_dir) = (tempdir().unwrap(), tempdir().unwrap(), tempdir().unwrap());
    let cache = root_a.path().join("scoped-cache.json");
    let cache = use_env_cache.then_some(cache.as_path());
    let stdout_ref = large_run_stdout_ref(root_a.path(), index_dir.path(), cache, payload_char);
    let expanded = expand_raw_text(
        &stdout_ref,
        None,
        Some(root_b.path()),
        argv![(
            "TOKENZERO_REF_INDEX_PATH",
            index_dir.path().to_str().unwrap()
        )],
    );
    assert_eq!(expanded, payload_char.to_string().repeat(28 * 1024));
}

#[test]
fn cli_run_ref_expands_across_roots_via_ref_index() {
    assert_large_run_ref_roundtrip('R', false);
}

#[test]
fn cli_run_ref_with_env_cache_path_expands_across_roots_via_ref_index() {
    assert_large_run_ref_roundtrip('E', true);
}

#[test]
fn cli_expand_not_found_names_all_lookup_tiers() {
    let (root, index_dir) = (tempdir().unwrap(), tempdir().unwrap());
    let output = tokenzero_cmd()
        .current_dir(root.path())
        .env("TOKENZERO_REF_INDEX_PATH", index_dir.path())
        .args([
            "expand",
            "tz://blob/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let json = parse_json_stdout(&output);
    fields!(json;"/error/code" =>"ref_not_found");
    let message = json["error"]["message"].as_str().unwrap();
    for needle in [
        "explicit/env cache",
        "current-root store",
        "per-user ref-index",
    ] {
        assert!(message.contains(needle), "{message}");
    }
}

#[test]
fn cli_expand_decode_failure_keeps_expand_failed_taxonomy() {
    let root = tempdir().unwrap();
    let cache = root.path().join("cache.json");
    let payload = root.path().join("payload.txt");
    fs::write(&payload, "D".repeat(70 * 1024)).unwrap();
    let output = assert_success(
        tokenzero_cmd().env("TOKENZERO_REF_INDEX", "0").current_dir(root.path())
            .args(argv!["run";"--cache-path";cache.to_str().unwrap();"--json";"--";"python3";"-c";"from pathlib import Path; import sys; sys.stdout.buffer.write(Path(sys.argv[1]).read_bytes())";payload.to_str().unwrap()])
            .output().unwrap(),
        "large run for decode",
    );
    let stdout_ref = first_ref_with_kind(&parse_json_stdout(&output), "stdout");
    let sidecar_dir = root.path().join("cache.json.blobs");
    assert!(sidecar_dir.is_dir(), "large blob should be externalized");
    for entry in fs::read_dir(&sidecar_dir).unwrap() {
        fs::remove_file(entry.unwrap().path()).unwrap();
    }
    let expanded = tokenzero_cmd()
        .env("TOKENZERO_REF_INDEX", "0")
        .current_dir(root.path())
        .args(argv!["expand";&stdout_ref;"--cache-path";cache.to_str().unwrap();"--json"])
        .output()
        .unwrap();
    assert!(!expanded.status.success());
    let json = parse_json_stdout(&expanded);
    fields!(json;"/error/code" =>"expand_failed");
    assert_json_contains(&json["error"]["message"], "could not be decoded");
}
fn path_reject_absolute() -> (tempfile::TempDir, std::process::Output, &'static str) {
    let root = tempdir().unwrap();
    let outside_dir = tempdir().unwrap();
    let outside = outside_dir.path().join("canary.txt");
    std::fs::write(&outside, "do-not-leak\n").unwrap();
    let output = tokenzero_cmd()
        .current_dir(root.path())
        .args(["read", outside.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    std::mem::forget(outside_dir);
    (root, output, "do-not-leak")
}
fn path_reject_paths_from() -> (tempfile::TempDir, std::process::Output, &'static str) {
    let root = tempdir().unwrap();
    let allowed = root.path().join("allowed");
    fs::create_dir_all(&allowed).unwrap();
    let outside_dir = tempdir().unwrap();
    let list = outside_dir.path().join("paths.txt");
    fs::write(&list, "SECRET_LEAK_SENTINEL_DO_NOT_READ\n").unwrap();
    let output = tokenzero_cmd().current_dir(root.path())
        .args(argv!["read";"--paths-from";list.to_str().unwrap();"--allowed-root";allowed.to_str().unwrap();"--json"])
        .output().unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::mem::forget(outside_dir);
    (root, output, "SECRET_LEAK_SENTINEL_DO_NOT_READ")
}
fn run_path_reject(build: fn() -> (tempfile::TempDir, std::process::Output, &'static str)) {
    let (_root, output, sentinel) = build();
    assert_json_error(&output, "path_not_allowed");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(sentinel));
}

#[test]
fn cli_read_default_root_rejects_absolute_path_outside_cwd() {
    let (_root, output, sentinel) = path_reject_absolute();
    assert_json_error(&output, "path_not_allowed");
    assert!(
        parse_json_stdout(&output)["error"]["message"]
            .as_str()
            .unwrap()
            .len()
            >= 10
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(sentinel));
}

#[test]
fn cli_read_paths_from_rejects_list_file_outside_allowed_root_without_reflecting_contents() {
    run_path_reject(path_reject_paths_from);
}

#[test]
fn cli_grep_and_glob_are_exact_first_surfaces() {
    let (dir, cache) = setup_temp_with_cache();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
    std::fs::write(dir.path().join("README.md"), "alpha docs\n").unwrap();
    let grep_json = run_tool_json(
        "grep",
        &["alpha", dir.path().to_str().unwrap()],
        dir.path(),
        &cache,
    );
    fields!(grep_json;"/tool" =>"grep");
    assert_json_contains(&grep_json["visible"]["text"], "alpha");
    let _ = first_ref_with_kind(&grep_json, "blob");
    let glob_json = run_tool_json(
        "glob",
        &["**/*.rs", dir.path().to_str().unwrap()],
        dir.path(),
        &cache,
    );
    fields!(glob_json;"/tool" =>"glob");
    let text = glob_json["visible"]["text"].as_str().unwrap();
    assert!(text.contains("src/lib.rs"));
    assert!(!text.contains("README.md"));
}

#[test]
#[cfg(feature = "surface-codemode")]
fn cli_codemode_token_namespace_roundtrip_uses_explicit_cache() {
    let (dir, cache) = setup_temp_with_cache();
    let plan = r#"const c = await zero.token.compact("cli codemode payload"); const e = await zero.token.expand(c.ref); return { ref: c.ref, text: e.text }"#;
    let json = parse_json_stdout(&assert_success(
        tokenzero_cmd()
            .args(argv!["codemode";"--root";dir.path().to_str().unwrap();"--cache-path";cache.to_str().unwrap();"--json";"--plan";plan])
            .output().unwrap(),
        "codemode",
    ));
    fields!(json;"/status" =>"completed");
    assert!(json["value"]["ref"].as_str().unwrap().starts_with("tz://"));
    assert_json_contains(&json["value"]["text"], "cli codemode payload");
    assert!(cache.exists(), "explicit CodeMode cache should be written");
}

#[test]
#[cfg(feature = "surface-codemode")]
fn cli_codemode_rejects_outside_root_without_leaking_contents() {
    let root = tempdir().unwrap();
    let outside_dir = tempdir().unwrap();
    let outside = outside_dir.path().join("secret.txt");
    fs::write(&outside, "CODEMODE_OUTSIDE_ROOT_SENTINEL\n").unwrap();
    let quoted_path = serde_json::to_string(outside.to_str().unwrap()).unwrap();
    let plan = format!("await zero.read({quoted_path})");
    let output = tokenzero_cmd()
        .current_dir(root.path())
        .args(["codemode", "--json", "--plan", &plan])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let json = parse_json_stdout(&output);
    fields!(json;"/status" =>"error";"/error/kind" =>"path_not_allowed");
    assert_eq!(
        json["error"]["message"],
        "path is outside allowed roots: ".to_owned() + outside.to_str().unwrap()
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("CODEMODE_OUTSIDE_ROOT_SENTINEL"));
}

#[test]
#[cfg(feature = "surface-codemode")]
fn cli_codemode_plan_error_exits_nonzero() {
    let output = tokenzero_cmd()
        .args(["codemode", "--json", "--plan", "await zero.nope()"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let json = parse_json_stdout(&output);
    assert_eq!(json["status"], "error");
    assert_json_contains(&json["error"]["message"], "unknown method");
}

#[test]
#[cfg(feature = "surface-codemode")]
fn cli_codemode_stdin_budget_tier_b_trampoline() {
    let (dir, cache) = setup_temp_with_cache();
    let plan = r#"const c = await zero.token.compact("tier-b-stdin"); return { ref: c.ref, text: c.text }"#;
    let mut cmd = AssertCmd::cargo_bin("tokenzero").unwrap();
    let output = assert_success(
        cmd.args([
            "codemode",
            "--json",
            "--budget",
            "512",
            "--root",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--stdin",
        ])
        .write_stdin(plan)
        .output()
        .unwrap(),
        "codemode-stdin-budget",
    );
    let json = parse_json_stdout(&output);
    fields!(json;"/schema" =>"tokenzero.codemode.v1";"/status" =>"completed");
    assert!(json["value"]["ref"].as_str().unwrap().starts_with("tz://"));
    assert_json_contains(&json["value"]["text"], "tier-b-stdin");
    assert!(
        json["refs"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "Tier B envelope must surface refs like MCP"
    );
}

#[test]
#[cfg(feature = "surface-codemode")]
fn cli_codemode_dash_plan_reads_stdin() {
    let (dir, cache) = setup_temp_with_cache();
    let mut cmd = AssertCmd::cargo_bin("tokenzero").unwrap();
    let output = assert_success(
        cmd.args([
            "codemode",
            "--json",
            "--max-visible-tokens",
            "400",
            "--root",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "-",
        ])
        .write_stdin("return { ok: true, via: \"dash\" }")
        .output()
        .unwrap(),
        "codemode-dash-stdin",
    );
    let json = parse_json_stdout(&output);
    fields!(json;"/status" =>"completed");
    assert_eq!(json["value"]["via"], "dash");
}

#[test]
fn cli_codemode_conflicting_plan_sources_typed_error() {
    let mut cmd = AssertCmd::cargo_bin("tokenzero").unwrap();
    let output = cmd
        .args(["codemode", "--json", "--stdin", "--plan", "return 1"])
        .write_stdin("return 2")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let json = parse_json_stdout(&output);
    fields!(json;"/schema" =>"tokenzero.codemode.v1";"/status" =>"error";"/error/kind" =>"validation");
    assert_json_contains(&json["error"]["message"], "conflicting plan sources");
}

#[test]
#[cfg(feature = "surface-codemode")]
fn cli_codemode_empty_plan_typed_error_no_silent_partial() {
    let mut cmd = AssertCmd::cargo_bin("tokenzero").unwrap();
    let output = cmd
        .args(["codemode", "--json", "--stdin"])
        .write_stdin("")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let json = parse_json_stdout(&output);
    fields!(json;"/schema" =>"tokenzero.codemode.v1";"/status" =>"error");
    assert!(json["error"]["kind"].is_string());
    assert_json_contains(&json["error"]["message"], "empty plan");
    assert!(json["value"].is_null());
}

#[test]
fn cli_cache_pack_is_schemaed_and_deterministic() {
    let (dir, cache) = setup_temp_with_cache();
    std::fs::write(dir.path().join("AGENTS.md"), "stable\n").unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let run = || {
        run_tokenzero_json(
            argv!["cache-pack";"--scope";"agent";"--root";dir.path().to_str().unwrap();"--cache-path";cache.to_str().unwrap();"--json"],
        )
    };
    let (first_json, second_json) = (run(), run());
    fields!(first_json;"/tool" =>"cache-pack";"/telemetry/daemon_required" =>false);
    assert!(
        first_json["telemetry"]["content_digest"]
            .as_str()
            .unwrap()
            .len()
            >= 16
    );
    assert_eq!(
        first_json["telemetry"]["content_digest"],
        second_json["telemetry"]["content_digest"]
    );
    fields!(second_json;"/telemetry/invalidation_reason" =>"unchanged");
}
#[derive(Clone, Copy)]
enum AdapterRows<'a> {
    Available(&'a str),
    ApprovedNotExecuted,
}
macro_rules! assert_adapter_rows {
    ($report:expr, $expected:expr) => {{
        let expected = $expected;
        for tool in required_adapter_tools() {
            let row = find_row_by($report["rows"].as_array().unwrap(), "tool", tool);
            fields!(row; "/blind_install_attempted" => false);
            match expected {
                AdapterRows::Available(suite) => {
                    fields!(row; "/suite" => suite; "/adapter_allowlisted" => true);
                    assert!(matches!(row["availability_status"].as_str().unwrap(), "run" | "unavailable"), "{tool}");
                    assert!(!row["availability_reason"].as_str().unwrap().is_empty(), "{tool}");
                    for key in ["raw_tokens", "visible_tokens", "recovery_tokens", "safe_savings", "harm_rate"] {
                        assert!(row[key].is_number(), "{tool} {key}");
                    }
                    assert!(row["fairness_notes"].as_str().unwrap().contains("adapter"), "{tool}");
                }
                AdapterRows::ApprovedNotExecuted => {
                    fields!(row; "/availability_status" => "approved_not_executed"; "/adapter_command_reviewed" => true; "/task_success" => false);
                    assert_eq!(row["adapter_command"], format!("{tool} --version"), "{tool}");
                }
            }
        }
    }};
}
macro_rules! bench_report {
    ($dir:expr, $suite:expr, $approval:expr, $label:expr) => {{
        let output = $dir.join("bench.json");
        let mut args = vec!["bench", "competitors", "--suite", $suite];
        let approval: Option<&std::path::Path> = $approval;
        if let Some(path) = approval {
            args.extend(["--adapter-approval-artifact", path.to_str().unwrap()]);
        }
        args.extend(["--output-json", output.to_str().unwrap(), "--json"]);
        let report = parse_json_stdout(&assert_success(
            tokenzero_cmd().args(args).output().unwrap(),
            $label,
        ));
        (report, output)
    }};
}

#[test]
fn cli_bench_competitors_emits_private_safe_savings_rows() {
    let dir = tempdir().unwrap();
    let (json, output_json) =
        bench_report!(dir.path(), "hostile-output", None, "bench competitors");
    fields!(json;"/schema_version" =>"tokenzero.bench.v1";"/private_artifact" =>true);
    let report_text = serde_json::to_string(&json).unwrap();
    let forbidden_marker = ["place", "holder"].concat();
    assert!(
        !report_text.to_ascii_lowercase().contains(&forbidden_marker),
        "{report_text}"
    );
    assert!(json["rows"].as_array().unwrap().iter().any(|row| {
        row["tool"] == "tokenzero"
            && row["byte_perfect_recovery"] == true
            && row["task_success"] == true
            && row["safe_savings"].is_number()
            && row["exact_expand_checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|check| check["kind"] == "combined" && check["byte_perfect"] == true)
    }));
    assert!(json["rows"].as_array().unwrap().iter().any(|row| {
        row["tool"] == "competitors" && row["availability_status"] == "unavailable"
    }));
    assert_adapter_rows!(&json, AdapterRows::Available("hostile-output"));
    fields!(json;"/adapter_matrix/blind_install_attempted" =>false;"/adapter_matrix/all_required_adapters_accounted" =>true);
    assert_eq!(
        json["adapter_matrix"]["required_adapter_count"],
        required_adapter_tools().len()
    );
    assert!(output_json.exists());
}

#[test]
fn cli_bench_competitors_links_approved_adapters_without_executing_them() {
    let dir = tempdir().unwrap();
    let adapter_artifact = dir.path().join("adapter-approval.json");
    write_json_fixture(
        &adapter_artifact,
        &serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_audit.v1", "ok": true,
            "execution_allowed": true, "public_claims_approved": true, "blind_install_attempted": false,
            "required_adapter_count": 11, "reviewed_command_count": 11, "missing_reviewed_command_count": 0,
            "unsafe_command_count": 0, "duplicate_command_count": 0, "adapters": reviewed_adapter_rows()
        }),
    );
    let (json, _) = bench_report!(
        dir.path(),
        "shell-heavy",
        Some(&adapter_artifact),
        "bench approved adapters"
    );
    fields!(json;"/adapter_matrix/approved_adapter_count" =>11;"/adapter_matrix/runnable_adapter_count" =>0;
        "/adapter_matrix/blind_install_attempted" =>false;"/public_claims_approved" =>false;"/release_publication_allowed" =>false);
    assert_adapter_rows!(&json, AdapterRows::ApprovedNotExecuted);
}
