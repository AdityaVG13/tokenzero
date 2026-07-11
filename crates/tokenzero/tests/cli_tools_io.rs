use assert_cmd::prelude::*;
use serde_json::Value;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

fn first_ref_with_kind(json: &Value, kind: &str) -> String {
    json["refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == kind)
        .unwrap_or_else(|| panic!("missing ref kind {kind}: {json}"))["ref"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn cli_read_expand_json_roundtrip() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    std::fs::write(&file, "alpha\nbeta\n").unwrap();
    let cache = dir.path().join("cache.json");

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
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
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.cli.v1");
    let blob_ref = json["refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "blob")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_string();

    let expanded = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "expand",
            &blob_ref,
            "--cache-path",
            cache.to_str().unwrap(),
            "--raw",
        ])
        .output()
        .unwrap();
    assert!(
        expanded.status.success(),
        "{}",
        String::from_utf8_lossy(&expanded.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&expanded.stdout), "alpha\nbeta\n");
}

#[test]
fn cli_expand_recovers_blob_across_roots_via_ref_index() {
    let root_a = tempdir().unwrap();
    let root_b = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let file = root_a.path().join("sample.txt");
    fs::write(&file, "cross\nroot\nbytes\n").unwrap();
    let cache_a = root_a.path().join("cache.json");

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .env("TOKENZERO_REF_INDEX_PATH", index_dir.path())
        .args([
            "read",
            file.to_str().unwrap(),
            "--cache-path",
            cache_a.to_str().unwrap(),
            "--allowed-root",
            root_a.path().to_str().unwrap(),
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
    let blob_ref = json["refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "blob")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_string();

    let expanded = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(root_b.path())
        .env("TOKENZERO_REF_INDEX_PATH", index_dir.path())
        .args(["expand", &blob_ref, "--raw"])
        .output()
        .unwrap();
    assert!(
        expanded.status.success(),
        "{}",
        String::from_utf8_lossy(&expanded.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&expanded.stdout),
        "cross\nroot\nbytes\n"
    );
}

#[test]
fn cli_run_ref_expands_across_roots_via_ref_index() {
    let root_a = tempdir().unwrap();
    let root_b = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let expected = "R".repeat(28 * 1024);

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(root_a.path())
        .env("TOKENZERO_REF_INDEX_PATH", index_dir.path())
        .args([
            "run",
            "--json",
            "--",
            "python3",
            "-c",
            &format!("import sys; sys.stdout.write({expected:?})"),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let stdout_ref = first_ref_with_kind(&json, "stdout");

    let expanded = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(root_b.path())
        .env("TOKENZERO_REF_INDEX_PATH", index_dir.path())
        .args(["expand", &stdout_ref, "--raw"])
        .output()
        .unwrap();
    assert!(
        expanded.status.success(),
        "{}",
        String::from_utf8_lossy(&expanded.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&expanded.stdout), expected);
}

#[test]
fn cli_run_ref_with_env_cache_path_expands_across_roots_via_ref_index() {
    let root_a = tempdir().unwrap();
    let root_b = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let cache_a = root_a.path().join("scoped-cache.json");
    let expected = "E".repeat(28 * 1024);

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(root_a.path())
        .env("TOKENZERO_REF_INDEX_PATH", index_dir.path())
        .env("TOKENZERO_CACHE_PATH", &cache_a)
        .args([
            "run",
            "--json",
            "--",
            "python3",
            "-c",
            &format!("import sys; sys.stdout.write({expected:?})"),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        cache_a.exists(),
        "TOKENZERO_CACHE_PATH should choose the store"
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let stdout_ref = first_ref_with_kind(&json, "stdout");

    let expanded = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(root_b.path())
        .env("TOKENZERO_REF_INDEX_PATH", index_dir.path())
        .args(["expand", &stdout_ref, "--raw"])
        .output()
        .unwrap();
    assert!(
        expanded.status.success(),
        "{}",
        String::from_utf8_lossy(&expanded.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&expanded.stdout), expected);
}

#[test]
fn cli_expand_not_found_names_all_lookup_tiers() {
    let root = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
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
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error"]["code"], "ref_not_found");
    let message = json["error"]["message"].as_str().unwrap();
    assert!(message.contains("explicit/env cache"), "{message}");
    assert!(message.contains("current-root store"), "{message}");
    assert!(message.contains("per-user ref-index"), "{message}");
}

#[test]
fn cli_expand_decode_failure_keeps_expand_failed_taxonomy() {
    let root = tempdir().unwrap();
    let cache = root.path().join("cache.json");
    let expected = "D".repeat(70 * 1024);
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(root.path())
        .args([
            "run",
            "--cache-path",
            cache.to_str().unwrap(),
            "--json",
            "--",
            "python3",
            "-c",
            &format!("import sys; sys.stdout.write({expected:?})"),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let stdout_ref = first_ref_with_kind(&json, "stdout");
    let sidecar_dir = root.path().join("cache.json.blobs");
    assert!(sidecar_dir.is_dir(), "large blob should be externalized");
    for entry in fs::read_dir(&sidecar_dir).unwrap() {
        fs::remove_file(entry.unwrap().path()).unwrap();
    }

    let expanded = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(root.path())
        .args([
            "expand",
            &stdout_ref,
            "--cache-path",
            cache.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!expanded.status.success());
    let json: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(json["error"]["code"], "expand_failed");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("could not be decoded")
    );
}

#[test]
fn cli_read_default_root_rejects_absolute_path_outside_cwd() {
    let root = tempdir().unwrap();
    let outside_dir = tempdir().unwrap();
    let outside = outside_dir.path().join("canary.txt");
    std::fs::write(&outside, "do-not-leak\n").unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(root.path())
        .args(["read", outside.to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "error");
    assert_eq!(json["error"]["code"], "path_not_allowed");
    assert!(
        json["error"]["message"].as_str().unwrap().len() >= 10,
        "error message should be descriptive"
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("do-not-leak"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn cli_read_paths_from_rejects_list_file_outside_allowed_root_without_reflecting_contents() {
    let root = tempdir().unwrap();
    let allowed = root.path().join("allowed");
    fs::create_dir_all(&allowed).unwrap();
    let outside_dir = tempdir().unwrap();
    let list = outside_dir.path().join("paths.txt");
    fs::write(&list, "SECRET_LEAK_SENTINEL_DO_NOT_READ\n").unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(root.path())
        .args([
            "read",
            "--paths-from",
            list.to_str().unwrap(),
            "--allowed-root",
            allowed.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "error");
    assert_eq!(json["error"]["code"], "path_not_allowed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("SECRET_LEAK_SENTINEL_DO_NOT_READ"),
        "{stdout}"
    );
}

#[test]
fn cli_grep_and_glob_are_exact_first_surfaces() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    let file = dir.path().join("src/lib.rs");
    std::fs::write(&file, "pub fn alpha() {}\n").unwrap();
    std::fs::write(dir.path().join("README.md"), "alpha docs\n").unwrap();
    let cache = dir.path().join("cache.json");

    let grep = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "grep",
            "alpha",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--allowed-root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        grep.status.success(),
        "{}",
        String::from_utf8_lossy(&grep.stderr)
    );
    let grep_json: Value = serde_json::from_slice(&grep.stdout).unwrap();
    assert_eq!(grep_json["tool"], "grep");
    assert!(
        grep_json["visible"]["text"]
            .as_str()
            .unwrap()
            .contains("alpha"),
        "grep result must contain the search term"
    );
    assert!(
        grep_json["refs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["kind"] == "blob")
    );

    let glob = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "glob",
            "**/*.rs",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--allowed-root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        glob.status.success(),
        "{}",
        String::from_utf8_lossy(&glob.stderr)
    );
    let glob_json: Value = serde_json::from_slice(&glob.stdout).unwrap();
    assert_eq!(glob_json["tool"], "glob");
    assert!(
        glob_json["visible"]["text"]
            .as_str()
            .unwrap()
            .contains("src/lib.rs")
    );
    assert!(
        !glob_json["visible"]["text"]
            .as_str()
            .unwrap()
            .contains("README.md")
    );
}

#[test]
fn cli_codemode_token_namespace_roundtrip_uses_explicit_cache() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("codemode-cache.json");
    let plan = r#"const c = await zero.token.compact("cli codemode payload"); const e = await zero.token.expand(c.ref); return { ref: c.ref, text: e.text }"#;

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "codemode",
            "--root",
            dir.path().to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--json",
            "--plan",
            plan,
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "completed");
    assert!(json["value"]["ref"].as_str().unwrap().starts_with("tz://"));
    assert!(
        json["value"]["text"]
            .as_str()
            .unwrap()
            .contains("cli codemode payload")
    );
    assert!(cache.exists(), "explicit CodeMode cache should be written");
}

#[test]
fn cli_codemode_rejects_outside_root_without_leaking_contents() {
    let root = tempdir().unwrap();
    let outside_dir = tempdir().unwrap();
    let outside = outside_dir.path().join("secret.txt");
    fs::write(&outside, "CODEMODE_OUTSIDE_ROOT_SENTINEL\n").unwrap();
    let quoted_path = serde_json::to_string(outside.to_str().unwrap()).unwrap();
    let plan = format!("await zero.read({quoted_path})");

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(root.path())
        .args(["codemode", "--json", "--plan", &plan])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "outside-root reads must fail the plan; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "error");
    assert_eq!(json["error"]["kind"], "path_not_allowed");
    assert_eq!(
        json["error"]["message"],
        "path is outside allowed roots: ".to_owned() + outside.to_str().unwrap()
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("CODEMODE_OUTSIDE_ROOT_SENTINEL"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn cli_codemode_plan_error_exits_nonzero() {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["codemode", "--json", "--plan", "await zero.nope()"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "error");
    // Structured error contract: { kind, message, retryable } — the message
    // stays INLINE (judgment path: an agent must see why the plan failed
    // without an extra expand round-trip).
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown method")
    );
}

#[test]
fn cli_cache_pack_is_schemaed_and_deterministic() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("AGENTS.md"), "stable\n").unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let cache = dir.path().join("cache.json");

    let run = |cache: &std::path::Path| {
        Command::cargo_bin("tokenzero")
            .unwrap()
            .args([
                "cache-pack",
                "--scope",
                "agent",
                "--root",
                dir.path().to_str().unwrap(),
                "--cache-path",
                cache.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap()
    };
    let first = run(&cache);
    let second = run(&cache);
    assert!(first.status.success());
    assert!(second.status.success());
    let first_json: Value = serde_json::from_slice(&first.stdout).unwrap();
    let second_json: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(first_json["tool"], "cache-pack");
    assert_eq!(first_json["telemetry"]["daemon_required"], false);
    assert!(
        first_json["telemetry"]["content_digest"]
            .as_str()
            .unwrap()
            .len()
            >= 16,
        "digest should be a meaningful hash"
    );
    assert_eq!(
        first_json["telemetry"]["content_digest"],
        second_json["telemetry"]["content_digest"]
    );
    assert_eq!(second_json["telemetry"]["invalidation_reason"], "unchanged");
}

#[test]
fn cli_bench_competitors_emits_private_safe_savings_rows() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("bench.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "bench",
            "competitors",
            "--suite",
            "hostile-output",
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
    assert_eq!(json["schema_version"], "tokenzero.bench.v1");
    assert_eq!(json["private_artifact"], true);
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
    let required_adapters = [
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
    for tool in required_adapters {
        let row = rows
            .iter()
            .find(|row| row["tool"] == tool)
            .unwrap_or_else(|| panic!("missing adapter row for {tool}"));
        assert_eq!(row["suite"], "hostile-output", "{tool}");
        assert!(
            matches!(
                row["availability_status"].as_str().unwrap(),
                "run" | "unavailable"
            ),
            "{tool}"
        );
        assert!(
            !row["availability_reason"].as_str().unwrap().is_empty(),
            "{tool}"
        );
        assert_eq!(row["adapter_allowlisted"], true, "{tool}");
        assert_eq!(row["blind_install_attempted"], false, "{tool}");
        assert!(row["raw_tokens"].is_number(), "{tool}");
        assert!(row["visible_tokens"].is_number(), "{tool}");
        assert!(row["recovery_tokens"].is_number(), "{tool}");
        assert!(row["safe_savings"].is_number(), "{tool}");
        assert!(row["harm_rate"].is_number(), "{tool}");
        assert!(
            row["fairness_notes"].as_str().unwrap().contains("adapter"),
            "{tool}"
        );
    }
    assert_eq!(json["adapter_matrix"]["blind_install_attempted"], false);
    assert_eq!(
        json["adapter_matrix"]["all_required_adapters_accounted"],
        true
    );
    assert_eq!(
        json["adapter_matrix"]["required_adapter_count"],
        required_adapters.len()
    );
    assert!(output_json.exists());
}

#[test]
fn cli_bench_competitors_links_approved_adapters_without_executing_them() {
    let dir = tempdir().unwrap();
    let adapter_artifact = dir.path().join("adapter-approval.json");
    let required_adapters = [
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
    let adapters = required_adapters
        .iter()
        .map(|tool| {
            serde_json::json!({
                "tool": tool,
                "approval_status": "reviewed",
                "execution_allowed": true,
                "reviewed_command": format!("{tool} --version"),
                "blind_install_attempted": false
            })
        })
        .collect::<Vec<_>>();
    std::fs::write(
        &adapter_artifact,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "tokenzero.adapter_approval_audit.v1",
            "ok": true,
            "execution_allowed": true,
            "public_claims_approved": true,
            "blind_install_attempted": false,
            "required_adapter_count": 11,
            "reviewed_command_count": 11,
            "missing_reviewed_command_count": 0,
            "unsafe_command_count": 0,
            "duplicate_command_count": 0,
            "adapters": adapters
        }))
        .unwrap(),
    )
    .unwrap();
    let output_json = dir.path().join("bench.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "bench",
            "competitors",
            "--suite",
            "shell-heavy",
            "--adapter-approval-artifact",
            adapter_artifact.to_str().unwrap(),
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
    assert_eq!(json["adapter_matrix"]["approved_adapter_count"], 11);
    assert_eq!(json["adapter_matrix"]["runnable_adapter_count"], 0);
    assert_eq!(json["adapter_matrix"]["blind_install_attempted"], false);
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    for tool in required_adapters {
        let row = json["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["tool"] == tool)
            .unwrap_or_else(|| panic!("missing adapter row for {tool}"));
        assert_eq!(
            row["availability_status"], "approved_not_executed",
            "{tool}"
        );
        assert_eq!(row["adapter_command_reviewed"], true, "{tool}");
        assert_eq!(
            row["adapter_command"],
            format!("{tool} --version"),
            "{tool}"
        );
        assert_eq!(row["blind_install_attempted"], false, "{tool}");
        assert_eq!(row["task_success"], false, "{tool}");
    }
}
