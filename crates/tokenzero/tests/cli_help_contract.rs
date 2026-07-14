use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::{Command, Output};
use tempfile::tempdir;

mod common;
use common::*;

fn assert_ok(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run(args: &[&str]) -> Output {
    Command::cargo_bin("tokenzero").unwrap().args(args).output().unwrap()
}

fn run_json(args: &[&str]) -> Value {
    let output = run(args);
    assert_ok(&output);
    serde_json::from_slice(&output.stdout).unwrap()
}

fn command_has_alias(json: &Value, name: &str, alias: &str) -> bool {
    json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == name && row["aliases"].as_array().unwrap().iter().any(|a| a == alias)
    })
}

fn contains_all(text: &str, needles: &[&str]) {
    for needle in needles {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
}

const CANONICAL_INVOCATIONS: &[&str] = &[
    "tokenzero --robot-help",
    "tokenzero robot-help",
    "tokenzero robot-docs guide",
    "tokenzero search <query> <path> --json",
    "tokenzero install status --json",
];

const ROBOT_DOC_CASES: &[(&[&str], &str, &str)] = &[
    (&["robot-doc", "manual"], "# TokenZero Robot Guide", "tokenzero capabilities --json"),
    (&["--robot-help"], "# TokenZero Robot Guide", "tokenzero robot-docs guide"),
    (&["robot-help"], "# TokenZero Robot Guide", "tokenzero robot-docs commands"),
    (&["robot-docs", "commands"], "# TokenZero Robot Commands", "tokenzero search <query> <path> --json"),
    (&["robot-docs", "examples"], "# TokenZero Robot Examples", "tokenzero rn rustc --version --json"),
];

const RUN_RECOVERY_CASES: &[&[&str]] = &[
    &["run", "--jsno", "rustc", "--version"],
    &["run", "--jason", "rustc", "--version"],
    &["run", "--json", "rustc", "--version"],
    &["run", "rustc", "--version", "--json"],
    &["shell", "rustc", "--version", "--jason"],
    &["rn", "rustc", "--version", "--json"],
    &["run", "--timout", "30", "rustc", "--version", "--json"],
];

const COMMAND_ALIAS_CASES: &[(&str, &str)] = &[
    ("find", "search"),
    ("doctor", "doctor statuz"),
    ("pulse", "pulse stats"),
];

#[test]
fn cli_bare_invocation_prints_useful_help() {
    let output = Command::cargo_bin("tokenzero").unwrap().output().unwrap();
    assert_ok(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    contains_all(
        &stdout,
        &[
            "Usage: tokenzero [COMMAND]",
            "tokenzero capabilities --json",
            "tokenzero robot-docs guide",
            "tokenzero run --json -- <cmd>",
        ],
    );
    assert!(stdout.lines().count() >= 3, "help should have multiple lines");
    assert!(
        stdout.contains("COMMAND") || stdout.contains("command"),
        "help should mention commands"
    );
}

#[test]
fn cli_capabilities_json_exposes_agent_contract() {
    let output = run(&["capabilities", "--json"]);
    assert_ok(&output);
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.capabilities.v1");
    assert_eq!(json["tool"], "tokenzero");
    assert_eq!(json["contract_version"], 1);
    assert_eq!(json["stdout_contract"]["json_flag"], "--json");
    let features = json["features"].as_array().unwrap();
    assert!(features.iter().any(|f| f == "json_output"));
    assert!(features.iter().any(|f| f == "non_tty_output_discipline"));
    for flag in [
        "capabilities_json",
        "codemode_surface",
        "robot_docs_guide",
        "intent_inference_aliases",
    ] {
        assert_eq!(json["feature_flags"][flag], true, "{flag}");
    }
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
            && row["aliases"].as_array().unwrap().iter().any(|a| a == "shell")
            && row["aliases"].as_array().unwrap().iter().any(|a| a == "rn")
            && row["aliases"].as_array().unwrap().iter().any(|a| a == "--jason")
            && row["primary_invocation"] == "tokenzero run --json -- <command>"
    }));
    for &(name, alias) in COMMAND_ALIAS_CASES {
        assert!(command_has_alias(&json, name, alias), "{name}/{alias}");
    }
    assert!(json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == "capabilities"
            && row["json"] == true
            && row["aliases"].as_array().unwrap().iter().any(|a| a == "--jason")
    }));
    assert!(json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == "robot-docs guide"
            && row["mutates"] == false
            && row["aliases"].as_array().unwrap().iter().any(|a| a == "robot-docs commands")
    }));
    assert!(json["commands"].as_array().unwrap().iter().any(|row| {
        row["name"] == "codemode"
            && row["json"] == true
            && row["primary_invocation"] == "tokenzero codemode --json --plan '<plan>'"
    }));
    assert_eq!(json["codemode"]["schema"], "tokenzero.codemode.v1");
    assert!(json["codemode"].get("mcp_tool").is_none(), "codemode must not advertise an mcp_tool");
    assert!(
        json["exit_codes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["code"] == 2 && row["label"] == "usage")
    );
    let invocations = json["canonical_invocations"].as_array().unwrap();
    for item in CANONICAL_INVOCATIONS {
        assert!(invocations.iter().any(|row| row == item), "missing canonical invocation {item}");
    }
    assert!(json["commands"].as_array().unwrap().len() >= 10, "should list many commands");
}

#[test]
fn cli_robot_docs_guide_is_paste_ready_for_agents() {
    let output = run(&["robot-docs", "guide"]);
    assert_ok(&output);
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    contains_all(
        &stdout,
        &[
            "# TokenZero Robot Guide",
            "tokenzero capabilities --json",
            "tokenzero run --json -- <command>",
            "Stdout is data. Stderr is diagnostics.",
            "telemetry.command_success",
            "--json",
        ],
    );
    assert!(stdout.lines().count() >= 10, "robot docs guide should be substantial");
}

#[test]
fn cli_agent_contract_outputs_are_deterministic_and_env_clean() {
    let first = tokenzero_with_agent_env(&["capabilities", "--json"]);
    let second = tokenzero_with_agent_env(&["capabilities", "--json"]);
    assert_ok(&first);
    assert_ok(&second);
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
    assert_eq!(
        features,
        vec![
            "capabilities_json",
            "codemode_surface",
            "exact_recovery_refs",
            "intent_inference_aliases",
            "json_output",
            "non_tty_output_discipline",
            "pipeline_rerun_guidance",
            "robot_docs_guide",
            "status_truth_shell"
        ]
    );
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
        &["search", "TokenZero", sample, "--allowed-root", allowed_root, "--json"][..],
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
                panic!("{args:?}: {err}\n{}", String::from_utf8_lossy(&output.stdout))
            });
        }
    }
}

#[test]
fn cli_agent_contract_aliases_recover_common_wrong_invocations() {
    let json = run_json(&["capabilites", "--json"]);
    assert_eq!(json["schema_version"], "tokenzero.capabilities.v1");
    assert!(command_has_alias(&json, "capabilities", "capabilites"));
    for &(args, title, needle) in ROBOT_DOC_CASES {
        let output = run(args);
        assert_ok(&output);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains(title), "{args:?}");
        assert!(stdout.contains(needle), "{args:?}");
    }
}

#[test]
fn cli_safe_subcommand_recoveries_choose_read_or_plan_surfaces() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    let cache = dir.path().join("cache.json");
    let cache = cache.to_str().unwrap();

    let json = run_json(&["cache", "statuz", "--root", root, "--json"]);
    assert_eq!(json["tool"], "mem");
    assert_eq!(json["status"], "ok");

    let json = run_json(&["pulse", "--root", root, "--json", "stats"]);
    assert!(json["event_count"].is_number());

    for subcommand in ["status", "statuz"] {
        let json = run_json(&["doctor", subcommand, "--root", root, "--cache-path", cache, "--json"]);
        assert_eq!(json["schema_version"], "tokenzero.doctor.health.v1");
        assert_eq!(json["status"], "ok");
    }

    let json = run_json(&["install", "plan", "--root", root, "--mcp", "--agent", "codex", "--json"]);
    assert_eq!(json["status"], "planned");
    assert_eq!(json["dry_run"], true);
    assert!(!json["writes"].as_array().unwrap().is_empty());

    let json = run_json(&[
        "install", "status", "--global", "--mcp", "--root", root, "--agent", "codex", "--json",
    ]);
    assert_eq!(json["schema_version"], "tokenzero.clients.v1");
    assert_eq!(json["command"], "clients detect");
    assert_eq!(json["agents"].as_array().unwrap()[0], "codex");
}

#[test]
fn cli_run_recovers_common_wrong_json_and_timeout_invocations() {
    for args in RUN_RECOVERY_CASES {
        let output = run(args);
        assert!(
            output.status.success(),
            "{args:?}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let json: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
            panic!("{args:?}: {err}\n{}", String::from_utf8_lossy(&output.stdout))
        });
        assert_eq!(json["status"], "ok", "{args:?}");
        assert_eq!(json["telemetry"]["command_success"], true, "{args:?}");
        assert!(
            json["telemetry"]["argv"].as_array().unwrap().iter().any(|arg| arg == "rustc"),
            "{args:?}"
        );
    }
}

#[test]
fn cli_search_and_capabilities_json_typo_aliases_recover() {
    for args in [&["capabilities", "--jsno"][..], &["capabilities", "--jason"][..]] {
        assert_eq!(run_json(args)["schema_version"], "tokenzero.capabilities.v1");
    }
    let json = run_json(&["search", "TokenZero", "AGENTS.md", "--json"]);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["tool"], "find");
}

#[test]
fn cli_help_discovers_agent_surfaces() {
    let output = run(&["--help"]);
    assert_ok(&output);
    contains_all(
        &String::from_utf8_lossy(&output.stdout),
        &["capabilities", "robot-docs", "Agent surfaces:"],
    );
}
