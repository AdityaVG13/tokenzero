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

fn run_default_envelope_json(args: &[&str], cwd: &std::path::Path) -> serde_json::Value {
    let output = tokenzero_cmd()
        .env_remove("TOKENZERO_SLIM_ENVELOPE")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert_success_ref(&output, "default envelope command");
    parse_json_stdout(&output)
}

use sha2::{Digest, Sha256};
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

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn boundary_payload(size: usize, label: &str) -> String {
    let prefix = format!("BEGIN-{label}\n");
    let suffix = format!("\nEND-{label}\n");
    assert!(prefix.len() + suffix.len() <= size);
    let mut payload = prefix;
    let fill = size - payload.len() - suffix.len();
    payload.extend((0..fill).map(|idx| if idx % 2 == 0 { 'x' } else { ' ' }));
    payload.push_str(&suffix);
    assert_eq!(payload.len(), size);
    payload
}

#[test]
fn cli_auto_read_default_cutoff_preserves_quality_and_exact_restart_expansion() {
    const CUTOFF: usize = 40_960;
    let (dir, cache) = setup_temp_with_cache();
    let cache_arg = cache.to_str().unwrap();
    let root_arg = dir.path().to_str().unwrap();
    let mut responses = Vec::new();

    for (label, size) in [("below", CUTOFF - 1), ("at", CUTOFF), ("above", CUTOFF + 1)] {
        let payload = boundary_payload(size, label);
        let file = dir.path().join(format!("{label}-boundary.txt"));
        fs::write(&file, &payload).unwrap();
        let output = tokenzero_cmd()
            .env_remove("TOKENZERO_CAPSULE_EXACT_REF_THRESHOLD_BYTES")
            .current_dir(dir.path())
            .args([
                "read",
                file.to_str().unwrap(),
                "--max-visible-tokens",
                "128",
                "--cache-path",
                cache_arg,
                "--allowed-root",
                root_arg,
                "--json",
            ])
            .output()
            .unwrap();
        assert_success_ref(&output, label);
        responses.push((label, payload, parse_json_stdout(&output)));
    }

    for (label, payload, response) in &responses[..2] {
        let visible = response["visible"]["text"].as_str().unwrap();
        assert!(
            !visible.contains("[exact payload stored; use expand for raw bytes]"),
            "{label} must stay on the inclusive preview side of the cutoff: {visible}"
        );
        assert!(
            visible.contains(&format!("{label}-boundary.txt")),
            "bounded preview must identify its source: {visible}"
        );
        assert!(
            visible.contains(&format!("BEGIN-{label}")),
            "bounded preview must retain source content: {visible}"
        );
        assert!(
            visible.len() < payload.len(),
            "preview must stay bounded below the full {label} payload"
        );
    }

    let (_, payload, above) = &responses[2];
    let visible = above["visible"]["text"].as_str().unwrap();
    assert!(
        visible.contains("[exact payload stored; use expand for raw bytes]"),
        "only bytes strictly above the cutoff switch to exact-ref Auto mode: {visible}"
    );
    assert!(
        visible.contains("above-boundary.txt"),
        "exact-ref output must identify the source selected for expansion: {visible}"
    );
    let blob_ref = first_ref_with_kind(above, "blob");
    let file_ref = first_ref_with_kind(above, "file");
    let expected_digest = sha256_hex(payload.as_bytes());
    assert_eq!(blob_ref, format!("tz://blob/{expected_digest}"));
    assert!(
        visible.contains(&file_ref),
        "live selector must be visible: {visible}"
    );

    // expand_raw_text launches a fresh CLI process after the read process has
    // exited. The entire source, not the bounded preview, must round-trip.
    let expanded = expand_raw_text(&file_ref, Some(&cache), None, &[]);
    assert_eq!(expanded.as_bytes(), payload.as_bytes());
    assert_eq!(sha256_hex(expanded.as_bytes()), expected_digest);
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
fn cli_slim_envelope_is_default_and_full_is_an_exact_opt_in() {
    let (dir, cache) = setup_temp_with_cache();
    let file = dir.path().join("tiny.txt");
    std::fs::write(&file, "tiny payload\n").unwrap();
    let file_arg = file.to_str().unwrap().to_string();
    let root_arg = dir.path().to_str().unwrap().to_string();
    let cache_arg = cache.to_str().unwrap().to_string();
    let args = [
        "read",
        file_arg.as_str(),
        "--cache-path",
        cache_arg.as_str(),
        "--allowed-root",
        root_arg.as_str(),
        "--json",
    ];

    let slim = run_default_envelope_json(&args, dir.path());
    fields!(slim; "/schema_version" => "tokenzero.cli.v1"; "/status" => "ok"; "/tool" => "read");
    for dropped in ["telemetry", "accounting", "mode", "content_type"] {
        assert!(
            slim.get(dropped).is_none(),
            "slim envelope drops {dropped}: {slim}"
        );
    }
    assert_json_contains(&slim["visible"], "tiny payload");
    assert!(
        slim.get("detail_ref").is_none(),
        "refs[0] carries the detail ref without duplication: {slim}"
    );
    let refs = slim["refs"].as_array().expect("slim refs array");
    assert!(!refs.is_empty(), "{slim}");
    // 1glt: the ordinal rewrite is published only when the complete serialized
    // response is strictly cheaper under the same token gauge. Under the
    // default gauge a full `tz://blob/<64hex>` ref (6 tokens) is cheaper than
    // its ordinal `tz://o/<gen>/<ord>` (8 tokens), so the full blob ref stays
    // and remains the exact expandable identity.
    let first_ref = refs[0].as_str().expect("slim refs[0] string");
    assert!(
        first_ref.starts_with("tz://blob/"),
        "full blob ref stays when the ordinal rewrite is not a whole-response win: {slim}"
    );
    for reference in refs.iter().filter_map(serde_json::Value::as_str) {
        assert_eq!(
            expand_raw_text(reference, Some(&cache), None, &[]),
            "tiny payload\n",
            "every slim ref expands after the producing process exits: {reference}"
        );
    }
    let recovered = run_default_envelope_json(
        &[
            "expand",
            refs[0].as_str().unwrap(),
            "--cache-path",
            cache_arg.as_str(),
            "--json",
        ],
        dir.path(),
    );
    assert_eq!(recovered["recovery"]["terminal"], true, "{recovered}");
    assert_eq!(
        recovered["recovery"]["do_not_recompact"], true,
        "{recovered}"
    );
    assert_eq!(recovered["recovery"]["exact_bytes"], true, "{recovered}");

    let mut full_args = args.to_vec();
    *full_args.last_mut().unwrap() = "--json=full";
    let full = run_default_envelope_json(&full_args, dir.path());
    fields!(full; "/schema_version" => "tokenzero.cli.v1"; "/status" => "ok"; "/tool" => "read");
    assert!(full.get("telemetry").is_some(), "{full}");
    assert!(full.get("accounting").is_some(), "{full}");
    assert!(full["visible"]["text"].as_str().is_some(), "{full}");
    assert!(
        full["refs"][0]["ref"]
            .as_str()
            .is_some_and(|reference| reference.starts_with("tz://blob/")),
        "full envelope keeps canonical forensic refs: {full}"
    );
    assert!(
        serde_json::to_vec(&slim).unwrap().len() * 2 < serde_json::to_vec(&full).unwrap().len(),
        "slim default must remain less than half the full envelope"
    );

    let invalid = tokenzero_cmd()
        .env_remove("TOKENZERO_SLIM_ENVELOPE")
        .current_dir(dir.path())
        .args([
            "read",
            file_arg.as_str(),
            "--allowed-root",
            root_arg.as_str(),
            "--json=unknown",
        ])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));

    let authored = tokenzero_cmd()
        .env_remove("TOKENZERO_SLIM_ENVELOPE")
        .args(["quote", "--platform", "unix", "--", "--json=full", "--json"])
        .output()
        .unwrap();
    assert_success_ref(&authored, "quoted child argv");
    let authored = String::from_utf8(authored.stdout).unwrap();
    assert!(authored.contains("--json=full"), "{authored}");
}

#[test]
fn cli_small_complete_read_is_inline_and_successful_edit_is_silent() {
    let (dir, cache) = setup_temp_with_cache();
    let file = dir.path().join("edit.txt");
    fs::write(&file, b"alpha\nbeta\ngamma").unwrap();
    let file_arg = file.to_str().unwrap();
    let cache_arg = cache.to_str().unwrap();

    let edit = tokenzero_cmd()
        .current_dir(dir.path())
        .args([
            "edit",
            "--edits-json",
            r#"[{"find":"beta","replace":"BETA"}]"#,
            file_arg,
            "--cache-path",
            cache_arg,
        ])
        .output()
        .unwrap();
    assert_success_ref(&edit, "small exact edit");
    assert!(edit.stdout.is_empty(), "{:?}", edit.stdout);
    assert_eq!(fs::read(&file).unwrap(), b"alpha\nBETA\ngamma");

    let forensic = tokenzero_cmd()
        .current_dir(dir.path())
        .args([
            "edit",
            "--edits-json",
            r#"[{"find":"BETA","replace":"beta"}]"#,
            file_arg,
            "--cache-path",
            cache_arg,
            "--json=full",
        ])
        .output()
        .unwrap();
    assert_success_ref(&forensic, "full edit envelope");
    let forensic = parse_json_stdout(&forensic);
    assert_eq!(forensic["tool"], "edit");
    assert!(
        forensic["refs"]
            .as_array()
            .is_some_and(|refs| refs.iter().any(|record| record["kind"] == "undo")),
        "{forensic}"
    );

    let edit = tokenzero_cmd()
        .current_dir(dir.path())
        .args([
            "edit",
            "--edits-json",
            r#"[{"find":"beta","replace":"BETA"}]"#,
            file_arg,
            "--cache-path",
            cache_arg,
        ])
        .output()
        .unwrap();
    assert_success_ref(&edit, "second small exact edit");
    assert!(edit.stdout.is_empty(), "{:?}", edit.stdout);
    assert_eq!(fs::read(&file).unwrap(), b"alpha\nBETA\ngamma");

    let read = tokenzero_cmd()
        .current_dir(dir.path())
        .args(["read", file_arg, "--cache-path", cache_arg])
        .output()
        .unwrap();
    assert_success_ref(&read, "small complete read");
    assert_eq!(read.stdout, b"alpha\nBETA\ngamma");

    let partial = tokenzero_cmd()
        .current_dir(dir.path())
        .args([
            "read",
            file_arg,
            "--start-line",
            "2",
            "--end-line",
            "2",
            "--cache-path",
            cache_arg,
        ])
        .output()
        .unwrap();
    assert_success_ref(&partial, "partial read");
    let partial = String::from_utf8(partial.stdout).unwrap();
    assert!(partial.contains("blob_ref: tz://blob/"), "{partial}");

    for (name, bytes) in [
        ("trailing-space.txt", &b"abc "[..]),
        ("trailing-tab.txt", &b"abc\t"[..]),
        ("trailing-newline.txt", &b"abc\n"[..]),
    ] {
        let mutant = dir.path().join(name);
        fs::write(&mutant, bytes).unwrap();
        let output = tokenzero_cmd()
            .current_dir(dir.path())
            .args(["read", mutant.to_str().unwrap(), "--cache-path", cache_arg])
            .output()
            .unwrap();
        assert_success_ref(&output, name);
        let output = String::from_utf8(output.stdout).unwrap();
        assert!(output.contains("blob_ref: tz://blob/"), "{name}: {output}");
    }

    fs::write(&file, format!("{}\n", "x".repeat(257))).unwrap();
    let large = tokenzero_cmd()
        .current_dir(dir.path())
        .args(["read", file_arg, "--cache-path", cache_arg])
        .output()
        .unwrap();
    assert_success_ref(&large, "large read");
    let large = String::from_utf8(large.stdout).unwrap();
    assert!(large.contains("blob_ref: tz://blob/"), "{large}");

    let failed = tokenzero_cmd()
        .current_dir(dir.path())
        .args([
            "edit",
            "--edits-json",
            r#"[{"find":"missing","replace":"nope"}]"#,
            file_arg,
            "--cache-path",
            cache_arg,
        ])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(
        String::from_utf8_lossy(&failed.stdout).contains("error:"),
        "failure must stay loud: {:?}",
        failed.stdout
    );
}

#[test]
fn cli_default_envelope_has_a_labeled_sub_kib_overhead_gate() {
    let (dir, cache) = setup_temp_with_cache();
    let cache_arg = cache.to_str().unwrap().to_string();
    let root_arg = dir.path().to_str().unwrap().to_string();
    let mut rows = Vec::new();

    for source_bytes in [128_usize, 512, 900] {
        let file = dir.path().join(format!("payload_{source_bytes}.txt"));
        std::fs::write(&file, "x".repeat(source_bytes)).unwrap();
        let file_arg = file.to_str().unwrap().to_string();
        let output = tokenzero_cmd()
            .env_remove("TOKENZERO_SLIM_ENVELOPE")
            .current_dir(dir.path())
            .args([
                "read",
                file_arg.as_str(),
                "--cache-path",
                cache_arg.as_str(),
                "--allowed-root",
                root_arg.as_str(),
                "--json",
            ])
            .output()
            .unwrap();
        assert_success_ref(&output, "default envelope measurement");
        let emitted_envelope_bytes = output.stdout.len();
        let envelope = parse_json_stdout(&output);
        let exact_visible_payload_bytes =
            envelope["visible"].as_str().expect("visible string").len();
        let exact_envelope_overhead_bytes =
            emitted_envelope_bytes.saturating_sub(exact_visible_payload_bytes);
        let overhead_per_visible_payload_ppm = exact_envelope_overhead_bytes
            .saturating_mul(1_000_000)
            / exact_visible_payload_bytes.max(1);
        rows.push((
            source_bytes,
            emitted_envelope_bytes,
            exact_visible_payload_bytes,
            exact_envelope_overhead_bytes,
            overhead_per_visible_payload_ppm,
        ));
    }

    let gate = rows
        .iter()
        .find(|(source_bytes, ..)| *source_bytes == 900)
        .unwrap();
    assert!(
        gate.4 <= 200_000,
        "exact_envelope_overhead_bytes / exact_visible_payload_bytes must be <=20% for the 900-byte sub-KiB gate: {rows:?}"
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
fn cli_warm_grep_elides_only_redundant_search_refs() {
    let (dir, cache) = setup_temp_with_cache();
    let file = dir.path().join("matches.txt");
    fs::write(
        &file,
        "needle one
middle
needle two
needle three
",
    )
    .unwrap();
    let file_arg = file.to_str().unwrap();
    let cache_arg = cache.to_str().unwrap();
    let root_arg = dir.path().to_str().unwrap();

    let cold = assert_success(
        tokenzero_cmd()
            .current_dir(dir.path())
            .args([
                "grep",
                "needle",
                file_arg,
                "--cache-path",
                cache_arg,
                "--allowed-root",
                root_arg,
                "--json=full",
            ])
            .output()
            .unwrap(),
        "cold full grep",
    );
    let cold = parse_json_stdout(&cold);
    assert_eq!(
        cold["refs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|record| record["kind"] == "search")
            .count(),
        3,
        "cold JSON keeps every per-hit ref: {cold}"
    );
    let full_results_ref = first_ref_with_kind(&cold, "blob");

    let warm = assert_success(
        tokenzero_cmd()
            .current_dir(dir.path())
            .args([
                "grep",
                "needle",
                file_arg,
                "--cache-path",
                cache_arg,
                "--allowed-root",
                root_arg,
            ])
            .output()
            .unwrap(),
        "warm text grep",
    );
    let warm = String::from_utf8(warm.stdout).unwrap();
    assert!(warm.contains(&full_results_ref), "{warm}");
    assert!(!warm.contains("search_ref:"), "{warm}");

    let expanded = expand_raw_text(&full_results_ref, Some(&cache), None, &[]);
    assert_eq!(
        expanded,
        format!(
            "{}:1:needle one
{}:3:needle two
{}:4:needle three",
            file.display(),
            file.display(),
            file.display()
        )
    );
}

#[test]
fn cli_glob_prefix_trie_keeps_a_durable_exact_full_path_set() {
    let (dir, cache) = setup_temp_with_cache();
    let mut paths = Vec::new();
    for group in ["alpha space", "βeta"] {
        let folder = dir.path().join("src").join(group);
        fs::create_dir_all(&folder).unwrap();
        for idx in 0..24 {
            let path = folder.join(format!("item-{idx:02}.rs"));
            fs::write(&path, format!("// {group} {idx}\n")).unwrap();
            paths.push(path);
        }
    }
    paths.sort();
    let glob = run_tool_json(
        "glob",
        &[
            "**/*.rs",
            dir.path().to_str().unwrap(),
            "--max-visible-tokens",
            "128",
        ],
        dir.path(),
        &cache,
    );
    let visible = glob["visible"]["text"].as_str().unwrap();
    assert!(visible.contains("# root: "), "{visible}");
    assert!(visible.contains("\"src\"/"), "{visible}");
    assert!(
        visible.contains("omitted"),
        "fixture must exercise recovery: {visible}"
    );

    let expected = paths
        .iter()
        .map(|path| path.display().to_string().replace('\\', "/"))
        .collect::<Vec<_>>()
        .join("\n");
    let blob_ref = first_ref_with_kind(&glob, "blob");
    let expanded = expand_raw_text(&blob_ref, Some(&cache), None, &[]);
    assert_eq!(expanded, expected);
    assert_eq!(
        sha256_hex(expanded.as_bytes()),
        sha256_hex(expected.as_bytes())
    );
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
