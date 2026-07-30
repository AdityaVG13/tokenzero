use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

mod common;
use common::*;

#[test]
fn cli_bare_invocation_prints_useful_help() {
    let output = Command::cargo_bin("tokenzero").unwrap().output().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: tokenzero [COMMAND]"));
    assert!(stdout.contains("tokenzero capabilities --json"));
    assert!(stdout.contains("tokenzero robot-docs guide"));
    assert!(stdout.contains("tokenzero run --json -- <cmd>"));
    assert!(
        stdout.lines().count() >= 3,
        "help should have multiple lines"
    );
    assert!(
        stdout.contains("COMMAND") || stdout.contains("command"),
        "help should mention commands"
    );
}

#[test]
fn cli_capabilities_json_exposes_agent_contract() {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["capabilities", "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.capabilities.v1");
    assert_eq!(json["tool"], "tokenzero");
    assert_eq!(json["contract_version"], 1);
    assert_eq!(json["stdout_contract"]["json_flag"], "--json");
    let features = json["features"].as_array().unwrap();
    assert!(features.iter().any(|feature| feature == "json_output"));
    assert!(
        features
            .iter()
            .any(|feature| feature == "non_tty_output_discipline")
    );
    assert_eq!(json["feature_flags"]["capabilities_json"], true);
    // o574 (R-019): the flag must reflect compile-time availability, not a
    // hardcoded true. The test binary and the CLI under test share features.
    assert_eq!(
        json["feature_flags"]["codemode_surface"],
        cfg!(feature = "surface-codemode")
    );
    assert_eq!(
        json["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature == "codemode_surface"),
        cfg!(feature = "surface-codemode")
    );
    assert_eq!(json["feature_flags"]["robot_docs_guide"], true);
    assert_eq!(json["feature_flags"]["intent_inference_aliases"], true);
    assert_eq!(
        json["commands_by_name"]["run"]["primary_invocation"],
        "tokenzero run --json -- <command>"
    );
    assert_eq!(
        json["commands_by_name"]["install"]["description"],
        "Plan or apply local integration writes with rollback data; --hooks wires the Claude Code PreToolUse hook, --shims installs the universal PATH shims, and install status recovers to clients detect."
    );
    assert_eq!(
        json["output_schemas"]["capabilities"]["schema_version"],
        "tokenzero.capabilities.v1"
    );
    assert!(
        json["output_schemas"]["run"]["status_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "telemetry.command_success")
    );
    assert!(json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == "run"
            && row["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|alias| alias == "shell")
            && row["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|alias| alias == "rn")
            && row["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|alias| alias == "--jason")
            && row["primary_invocation"] == "tokenzero run --json -- <command>"
    }));
    assert!(json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == "find"
            && row["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|alias| alias == "search")
    }));
    assert!(json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == "capabilities"
            && row["json"] == true
            && row["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|alias| alias == "--jason")
    }));
    assert!(json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == "robot-docs guide"
            && row["mutates"] == false
            && row["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|alias| alias == "robot-docs commands")
    }));
    assert!(json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == "codemode"
            && row["json"] == true
            && row["primary_invocation"]
                == "tokenzero codemode --json --budget <n> --stdin <<'EOF' … EOF"
    }));
    assert_eq!(json["codemode"]["schema"], "tokenzero.codemode.v1");
    assert_eq!(json["codemode"]["tier"], "B");
    assert_eq!(json["codemode"]["transport"], "shell_trampoline");
    assert_eq!(
        json["codemode"]["budget_flag"],
        "--budget / --max-visible-tokens"
    );
    assert!(
        json["codemode"].get("mcp_tool").is_none(),
        "codemode must not advertise an mcp_tool"
    );
    assert!(json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == "doctor"
            && row["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|alias| alias == "doctor statuz")
    }));
    assert!(json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == "pulse"
            && row["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|alias| alias == "pulse stats")
    }));
    assert!(
        json["exit_codes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["code"] == 2 && row["label"] == "usage")
    );
    assert!(
        json["canonical_invocations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "tokenzero --robot-help")
    );
    assert!(
        json["canonical_invocations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "tokenzero robot-help")
    );
    assert!(
        json["canonical_invocations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "tokenzero robot-docs guide")
    );
    assert!(
        json["canonical_invocations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "tokenzero search <query> <path> --json")
    );
    assert!(
        json["canonical_invocations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "tokenzero install status --json")
    );
    assert!(
        json["commands"].as_array().unwrap().len() >= 10,
        "should list many commands"
    );
}

#[test]
fn cli_robot_docs_guide_is_paste_ready_for_agents() {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["robot-docs", "guide"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("# TokenZero Robot Guide"));
    assert!(stdout.contains("tokenzero capabilities --json"));
    assert!(stdout.contains("tokenzero run --json -- <command>"));
    assert!(stdout.contains("Stdout is data. Stderr is diagnostics."));
    assert!(stdout.contains("telemetry.command_success"));
    assert!(
        stdout.lines().count() >= 10,
        "robot docs guide should be substantial"
    );
    assert!(
        stdout.contains("--json"),
        "robot docs should mention --json flag"
    );
}

#[test]
fn cli_agent_contract_outputs_are_deterministic_and_env_clean() {
    let first = tokenzero_with_agent_env(&["capabilities", "--json"]);
    let second = tokenzero_with_agent_env(&["capabilities", "--json"]);

    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(first.stderr.is_empty());
    assert!(second.stderr.is_empty());
    assert_eq!(first.stdout, second.stdout);
    assert_no_ansi(&first.stdout);
    let json: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.capabilities.v1");
    let features: Vec<&str> = json["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|feature| feature.as_str().unwrap())
        .collect();
    // o574 (R-019): codemode_surface appears only in codemode builds.
    let mut expected = vec![
        "capabilities_json",
        "exact_recovery_refs",
        "intent_inference_aliases",
        "json_output",
        "non_tty_output_discipline",
        "pipeline_rerun_guidance",
        "robot_docs_guide",
        "status_truth_shell",
    ];
    if cfg!(feature = "surface-codemode") {
        expected.push("codemode_surface");
        expected.sort();
    }
    assert_eq!(features, expected);
}

#[test]
fn cli_robot_docs_read_search_and_run_are_env_clean() {
    let dir = tempdir().unwrap();
    let sample = dir.path().join("sample.txt");
    std::fs::write(&sample, "TokenZero\n").unwrap();
    let allowed_root = dir.path().to_str().unwrap();
    let sample = sample.to_str().unwrap();

    for args in [
        &["robot-docs", "guide"][..],
        &["robot-docs", "commands"][..],
        &["robot-docs", "examples"][..],
        &["read", sample, "--allowed-root", allowed_root, "--json"][..],
        &[
            "search",
            "TokenZero",
            sample,
            "--allowed-root",
            allowed_root,
            "--json",
        ][..],
        &["run", "--json", "rustc", "--version"][..],
    ] {
        let output = tokenzero_with_agent_env(args);
        assert!(
            output.status.success(),
            "{args:?}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_no_ansi(&output.stdout);
        assert_no_ansi(&output.stderr);
        if args.contains(&"--json") {
            serde_json::from_slice::<Value>(&output.stdout).unwrap_or_else(|err| {
                panic!(
                    "{args:?}: {err}\n{}",
                    String::from_utf8_lossy(&output.stdout)
                )
            });
        }
    }
}

#[test]
fn cli_agent_contract_aliases_recover_common_wrong_invocations() {
    let capabilities = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["capabilites", "--json"])
        .output()
        .unwrap();

    assert!(
        capabilities.status.success(),
        "{}",
        String::from_utf8_lossy(&capabilities.stderr)
    );
    let json: Value = serde_json::from_slice(&capabilities.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.capabilities.v1");
    assert!(json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == "capabilities"
            && row["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|alias| alias == "capabilites")
    }));

    let robot_docs = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["robot-doc", "manual"])
        .output()
        .unwrap();

    assert!(
        robot_docs.status.success(),
        "{}",
        String::from_utf8_lossy(&robot_docs.stderr)
    );
    let stdout = String::from_utf8_lossy(&robot_docs.stdout);
    assert!(stdout.contains("# TokenZero Robot Guide"));
    assert!(stdout.contains("tokenzero capabilities --json"));

    let robot_help = Command::cargo_bin("tokenzero")
        .unwrap()
        .arg("--robot-help")
        .output()
        .unwrap();

    assert!(
        robot_help.status.success(),
        "{}",
        String::from_utf8_lossy(&robot_help.stderr)
    );
    let stdout = String::from_utf8_lossy(&robot_help.stdout);
    assert!(stdout.contains("# TokenZero Robot Guide"));
    assert!(stdout.contains("tokenzero robot-docs guide"));

    let robot_help_command = Command::cargo_bin("tokenzero")
        .unwrap()
        .arg("robot-help")
        .output()
        .unwrap();

    assert!(
        robot_help_command.status.success(),
        "{}",
        String::from_utf8_lossy(&robot_help_command.stderr)
    );
    let stdout = String::from_utf8_lossy(&robot_help_command.stdout);
    assert!(stdout.contains("# TokenZero Robot Guide"));
    assert!(stdout.contains("tokenzero robot-docs commands"));

    let robot_commands = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["robot-docs", "commands"])
        .output()
        .unwrap();

    assert!(
        robot_commands.status.success(),
        "{}",
        String::from_utf8_lossy(&robot_commands.stderr)
    );
    let stdout = String::from_utf8_lossy(&robot_commands.stdout);
    assert!(stdout.contains("# TokenZero Robot Commands"));
    assert!(stdout.contains("tokenzero search <query> <path> --json"));

    let robot_examples = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["robot-docs", "examples"])
        .output()
        .unwrap();

    assert!(
        robot_examples.status.success(),
        "{}",
        String::from_utf8_lossy(&robot_examples.stderr)
    );
    let stdout = String::from_utf8_lossy(&robot_examples.stdout);
    assert!(stdout.contains("# TokenZero Robot Examples"));
    assert!(stdout.contains("tokenzero rn rustc --version --json"));
}

#[test]
fn cli_safe_subcommand_recoveries_choose_read_or_plan_surfaces() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    let cache = dir.path().join("cache.json");
    let cache = cache.to_str().unwrap();

    let cache_status = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["cache", "statuz", "--root", root, "--json"])
        .output()
        .unwrap();
    assert!(
        cache_status.status.success(),
        "{}",
        String::from_utf8_lossy(&cache_status.stderr)
    );
    let json: Value = serde_json::from_slice(&cache_status.stdout).unwrap();
    assert_eq!(json["tool"], "mem");
    assert_eq!(json["status"], "ok");

    let pulse_stats = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["pulse", "--root", root, "--json", "stats"])
        .output()
        .unwrap();
    assert!(
        pulse_stats.status.success(),
        "{}",
        String::from_utf8_lossy(&pulse_stats.stderr)
    );
    let json: Value = serde_json::from_slice(&pulse_stats.stdout).unwrap();
    assert!(json["event_count"].is_number());

    for subcommand in ["status", "statuz"] {
        let doctor = Command::cargo_bin("tokenzero")
            .unwrap()
            .args([
                "doctor",
                subcommand,
                "--root",
                root,
                "--cache-path",
                cache,
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            doctor.status.success(),
            "{subcommand}: {}",
            String::from_utf8_lossy(&doctor.stderr)
        );
        let json: Value = serde_json::from_slice(&doctor.stdout).unwrap();
        assert_eq!(json["schema_version"], "tokenzero.doctor.health.v1");
        assert_eq!(json["status"], "ok");
    }

    let install_plan = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "install", "plan", "--root", root, "--mcp", "--agent", "codex", "--json",
        ])
        .output()
        .unwrap();
    assert!(
        install_plan.status.success(),
        "{}",
        String::from_utf8_lossy(&install_plan.stderr)
    );
    let json: Value = serde_json::from_slice(&install_plan.stdout).unwrap();
    assert_eq!(json["status"], "planned");
    assert_eq!(json["dry_run"], true);
    assert!(!json["writes"].as_array().unwrap().is_empty());

    let install_status = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "install", "status", "--global", "--mcp", "--root", root, "--agent", "codex", "--json",
        ])
        .output()
        .unwrap();
    assert!(
        install_status.status.success(),
        "{}",
        String::from_utf8_lossy(&install_status.stderr)
    );
    let json: Value = serde_json::from_slice(&install_status.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.clients.v1");
    assert_eq!(json["command"], "clients detect");
    assert_eq!(json["agents"].as_array().unwrap()[0], "codex");
}

#[test]
fn cli_run_recovers_common_wrong_json_and_timeout_invocations() {
    // Parent JSON / timeout typo recovery applies only to options parsed before
    // the child executable. Trailing --json stays in child argv (CE-P02-01).
    let cases: &[&[&str]] = &[
        &["run", "--jsno", "rustc", "--version"],
        &["run", "--jason", "rustc", "--version"],
        &["run", "--json", "rustc", "--version"],
        &["run", "--timout", "30", "--json", "rustc", "--version"],
        &["shell", "--jason", "rustc", "--version"],
        &["rn", "--json", "rustc", "--version"],
    ];

    for args in cases {
        let output = Command::cargo_bin("tokenzero")
            .unwrap()
            .args(*args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let json: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
            panic!(
                "{args:?}: {err}\n{}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        assert_eq!(json["status"], "ok", "{args:?}");
        assert_eq!(json["telemetry"]["command_success"], true, "{args:?}");
        assert!(
            json["telemetry"]["argv"]
                .as_array()
                .unwrap()
                .iter()
                .any(|arg| arg == "rustc"),
            "{args:?}"
        );
    }
}

#[test]
fn cli_run_preserves_trailing_child_json_without_delimiter() {
    // CE-P02-01: after the first child executable token, --json belongs to the
    // child argv and must not promote the parent envelope.
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["run", "printf", "%s\n", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        serde_json::from_slice::<Value>(&output.stdout).is_err(),
        "parent must not steal trailing --json into the JSON envelope; got {stdout}"
    );
    assert!(
        stdout.contains("stdout:\n--json\n"),
        "child must receive and print trailing --json; got {stdout}"
    );
    assert!(
        stdout.contains("exit_code: 0"),
        "text-mode run envelope expected; got {stdout}"
    );
    assert!(
        stdout.contains("combined_ref: tz://blob/"),
        "exact combined recovery ref expected; got {stdout}"
    );
}

#[test]
fn cli_run_inline_shell_envelope_handles_empty_stdout() {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["run", "printf", ""])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("stdout:\n"), "{stdout}");
    assert!(stdout.contains("combined_ref: tz://blob/"), "{stdout}");
    assert!(stdout.contains("exit_code: 0"), "{stdout}");
}

#[test]
fn cli_run_nonzero_exit_keeps_existing_failure_envelope() {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["run", "sh", "-c", "printf boom; exit 7"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("exit_code: 7"), "{stdout}");
    assert!(stdout.contains("combined_ref: tz://blob/"), "{stdout}");
}

#[test]
fn cli_mcp_tool_name_suggests_cli_verb_not_nearest_string() {
    // bara (R-016): tz_read must suggest 'read', never clap's generic 'tree'.
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["tz_read", "some/file.rs"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("the CLI verb is 'read'"), "{stderr}");
    assert!(
        stderr.contains("corrected command: tokenzero read some/file.rs"),
        "{stderr}"
    );
    assert!(!stderr.contains("'tree'"), "{stderr}");

    // Non-MCP typos keep clap's generic suggestion path.
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["tre"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("similar subcommand"), "{stderr}");
    assert!(!stderr.contains("MCP tool name"), "{stderr}");
}

#[test]
fn cli_usage_errors_name_exact_corrected_invocation() {
    // dzb2 (R-003): every usage error names the exact corrected command.
    let cases: &[(&[&str], &str)] = &[
        (&["read"], "corrected command: tokenzero read <path> --json"),
        (
            &["find"],
            "corrected command: tokenzero find --json <QUERY>",
        ),
        (
            &["run"],
            "corrected command: tokenzero run --json -- <command>",
        ),
        (
            &["expand"],
            "corrected command: tokenzero expand <tz-ref> --raw",
        ),
    ];
    for (args, needle) in cases {
        let output = Command::cargo_bin("tokenzero")
            .unwrap()
            .args(*args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            combined.contains(needle),
            "{args:?} missing {needle:?}: {combined}"
        );
    }
}

#[test]
fn cli_help_has_no_empty_subcommand_blurbs() {
    // 45lv (R-004): every top-level subcommand carries a one-line about.
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let commands_section = stdout.split("Commands:").nth(1).expect("Commands section");
    let commands_section = commands_section.split("Options:").next().unwrap();
    for line in commands_section.lines() {
        let line = line.trim_end();
        if line.len() <= 2 {
            continue;
        }
        assert!(
            line.split_whitespace().count() > 1,
            "empty help blurb: {line:?}"
        );
    }

    // capabilities lists the thin verbs and quarantines eval commands.
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["capabilities", "--json"])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let names: Vec<&str> = json["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["name"].as_str())
        .collect();
    for verb in [
        "grep",
        "ingest",
        "rewrite",
        "discover",
        "stats",
        "session-ledger",
        "cache",
        "clients",
        "cache-pack",
        "quote",
        "mcp-server",
    ] {
        assert!(
            names.contains(&verb),
            "capabilities.commands missing {verb}"
        );
    }
    let experimental: Vec<&str> = json["experimental_commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for eval in ["bench", "harm-eval", "claim-audit", "reach"] {
        assert!(
            experimental.contains(&eval),
            "experimental_commands missing {eval}"
        );
        assert!(!names.contains(&eval), "{eval} must stay out of commands");
    }
}

#[test]
fn cli_robot_triage_root_alias_matches_doctor_envelope() {
    // pec5 (R-001): root mega-command aliases reach doctor --robot-triage.
    for args in [
        vec!["--robot-triage"],
        vec!["robot-triage"],
        vec!["doctor", "--robot-triage"],
    ] {
        let output = Command::cargo_bin("tokenzero")
            .unwrap()
            .args(&args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let json: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            json["schema_version"], "tokenzero.doctor.robot_triage.v1",
            "{args:?}"
        );
        for key in [
            "health",
            "quick_ref",
            "recommendations",
            "commands",
            "findings",
            "recommended_command",
        ] {
            assert!(json.get(key).is_some(), "{args:?} missing {key}");
        }
    }

    // The help footer advertises the mega-command.
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("tokenzero --robot-triage"), "{stdout}");

    // capabilities pins the triage schema.
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["capabilities", "--json"])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["output_schemas"]["doctor_robot_triage"]["schema_version"],
        "tokenzero.doctor.robot_triage.v1"
    );
}

#[test]
fn cli_flag_typo_distance_one_offers_corrected_command() {
    // bdki (R-002): distance-1 flag typo -> did-you-mean + corrected command.
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["read", "--jsonn", "some/file.rs"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("did you mean: '--json'"), "{stderr}");
    assert!(
        stderr.contains("corrected command: tokenzero read --json some/file.rs"),
        "{stderr}"
    );

    // A real flag placed before the verb is reordered, not renamed.
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["--jsno", "read", "some/file.rs"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("belongs after the subcommand"), "{stderr}");
    assert!(
        stderr.contains("corrected command: tokenzero read --jsno some/file.rs"),
        "{stderr}"
    );

    // Far-off typos get no misleading suggestion (rejects --exlpain->--help).
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["grep", "--exlpain", "needle"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("unexpected argument '--exlpain'"),
        "{stderr}"
    );
    assert!(!stderr.contains("did you mean"), "{stderr}");
    assert!(!stderr.contains("similar argument"), "{stderr}");
}

#[test]
fn cli_run_json_child_exit_default_mirrors_child_failure() {
    // nt0i (1cwf flip): --json run mirrors the child exit code by default so
    // harnesses gating on process exit observe failure; envelope content is
    // unchanged (status/telemetry stay truthful).
    let default = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["run", "--json", "sh", "-c", "printf boom; exit 7"])
        .output()
        .unwrap();
    assert_eq!(
        default.status.code(),
        Some(7),
        "default mirrors child exit"
    );
    let json: Value = serde_json::from_slice(&default.stdout).unwrap();
    assert_eq!(json["status"], "ok", "envelope content unchanged");
    assert_eq!(json["telemetry"]["command_success"], false);
    assert_eq!(json["telemetry"]["exit_code"], 7);

    let legacy = Command::cargo_bin("tokenzero")
        .unwrap()
        .env("TOKENZERO_RUN_CHILD_EXIT", "0")
        .args(["run", "--json", "sh", "-c", "printf boom; exit 7"])
        .output()
        .unwrap();
    assert!(
        legacy.status.success(),
        "explicit opt-out keeps the legacy exit-0 envelope contract"
    );
    let json: Value = serde_json::from_slice(&legacy.stdout).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["telemetry"]["exit_code"], 7);
}

#[test]
fn cli_run_parent_json_keeps_inline_payload_unwrapped() {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["run", "--json", "printf", "%s\n", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["visible"]["text"], "--json");
    assert_eq!(json["telemetry"]["output_strategy"], "inline_shell");
    assert_eq!(json["telemetry"]["exit_code"], 0);
    assert!(
        json["refs"]
            .as_array()
            .is_some_and(|refs| refs.iter().any(|record| record["kind"] == "combined"))
    );
}

#[test]
fn cli_search_and_capabilities_json_typo_aliases_recover() {
    let capabilities_cases: &[&[&str]] =
        &[&["capabilities", "--jsno"], &["capabilities", "--jason"]];

    for args in capabilities_cases {
        let output = Command::cargo_bin("tokenzero")
            .unwrap()
            .args(*args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let json: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["schema_version"], "tokenzero.capabilities.v1");
    }

    let search = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["search", "TokenZero", "AGENTS.md", "--json"])
        .output()
        .unwrap();

    assert!(
        search.status.success(),
        "{}",
        String::from_utf8_lossy(&search.stderr)
    );
    let json: Value = serde_json::from_slice(&search.stdout).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["tool"], "find");
}

#[test]
fn cli_help_discovers_agent_surfaces() {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .arg("--help")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("capabilities"));
    assert!(stdout.contains("robot-docs"));
    assert!(stdout.contains("Agent surfaces:"));
}
