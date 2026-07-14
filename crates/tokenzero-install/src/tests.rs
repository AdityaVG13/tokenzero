use super::*;

use super::doctor::*;
use tempfile::tempdir;

fn plan_paths(report: &InstallPlan) -> Vec<String> {
    report.writes.iter().map(|write| write.path.replace('\\', "/")).collect()
}

fn temp_cache(dir: &tempfile::TempDir, rel: &str) -> std::path::PathBuf {
    dir.path().join(rel)
}

fn assert_doctor_ok(report: &Value) {
    assert_eq!(report["ok"], true);
    assert_eq!(report["status"], "ok");
}
fn assert_fix_schema(report: &Value) {
    assert_eq!(report["schema_version"], "tokenzero.doctor.fix.v1");
}
fn write_ends_with(report: &InstallPlan, suffix: &str) -> bool {
    plan_paths(report).iter().any(|path| path.ends_with(suffix))
}
fn write_contains(report: &InstallPlan, needle: &str) -> bool {
    plan_paths(report).iter().any(|path| path.contains(needle))
}

#[test]
fn plan_is_read_only_and_schemaed() {
    let dir = tempdir().unwrap();
    let report = plan(dir.path(), false, &[]);
    assert_eq!(report.schema_version, INSTALL_SCHEMA_VERSION);
    assert!(report.dry_run);
    assert!(!dir.path().join(".tokenzero/mcp-server.json").exists());
}

#[test]
fn doctor_reports_agent_contract_for_healthy_root() {
    let dir = tempdir().unwrap();
    let report = doctor(dir.path(), Some(&temp_cache(&dir, "cache.json")));
    assert_eq!(report["schema_version"], "tokenzero.doctor.v1");
    assert_doctor_ok(&report);
    assert_eq!(report["mutates"], false);
    assert!(report["findings"].as_array().unwrap().is_empty());
    assert_eq!(report["capabilities"]["supports_fix"], true);
    assert_eq!(report["capabilities"]["supports_undo"], true);
    assert_eq!(report["doctor_contract"]["default_read_only"], true);
    assert!(report["exit_codes"].as_array().unwrap().iter().any(|row| row["code"] == 1 && row["label"] == "blocked"));
    assert!(report["next_steps"].as_array().unwrap()[0]["command"].as_str().unwrap().contains("doctor --runtime --json"));
}

#[test]
fn doctor_blocks_missing_root_with_machine_readable_finding() {
    let dir = tempdir().unwrap();
    let report = doctor(&dir.path().join("missing"), None);
    assert_eq!(report["ok"], false);
    assert_eq!(report["status"], "blocked");
    assert!(report["findings"].as_array().unwrap().iter().any(|f| {
        f["id"] == "tz-root-missing" && f["severity"] == "error" && f["fix_supported"] == false
    }));
    assert_eq!(report["next_steps"].as_array().unwrap()[0]["action"], "fix_blocking_findings");
}

#[test]
fn doctor_reports_missing_cache_parent_as_non_blocking_info() {
    let dir = tempdir().unwrap();
    let report = doctor(dir.path(), Some(&temp_cache(&dir, "missing-parent/cache.json")));
    assert_doctor_ok(&report);
    assert!(report["findings"].as_array().unwrap().iter().any(|f| {
        f["id"] == "tz-cache-parent-missing" && f["severity"] == "info" && f["fix_supported"] == true
            && f["recommended_argv"].as_array().unwrap()[0] == "tokenzero"
    }));
}

#[test]
fn doctor_fix_dry_run_plans_cache_parent_without_mutating() {
    let dir = tempdir().unwrap();
    let cache = temp_cache(&dir, ".tokenzero/recovery-cache.json");
    let report = doctor_fix(dir.path(), Some(&cache), true);
    assert_fix_schema(&report);
    assert_eq!(report["ok"], true);
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["mutates"], false);
    assert_eq!(report["actions_taken"], 0);
    assert!(!cache.parent().unwrap().exists());
    assert_eq!(report["actions_planned"].as_array().unwrap()[0]["fixer_id"], DOCTOR_FIXER_CACHE_PARENT);
}

#[test]
fn doctor_fix_creates_cache_parent_idempotently_and_undo_restores_absence() {
    let dir = tempdir().unwrap();
    let cache = temp_cache(&dir, ".tokenzero/recovery-cache.json");
    let parent = cache.parent().unwrap().to_path_buf();
    let fixed = doctor_fix(dir.path(), Some(&cache), false);
    assert_eq!(fixed["ok"], true);
    assert_eq!(fixed["actions_taken"], 1);
    assert!(parent.is_dir());
    let run_id = fixed["run_id"].as_str().unwrap().to_string();
    let action_line = fs::read_to_string(dir.path().join(".doctor/runs").join(&run_id).join("actions.jsonl")).unwrap();
    assert!(action_line.contains(DOCTOR_FIXER_CACHE_PARENT));
    assert!(action_line.contains("\"before_exists\":false"));
    let second = doctor_fix(dir.path(), Some(&cache), false);
    assert_eq!(second["ok"], true);
    assert_eq!(second["actions_taken"], 0);
    assert_eq!(doctor_undo(dir.path(), &run_id)["ok"], true);
    assert!(!parent.exists());
    assert!(dir.path().join(".doctor/runs").join(run_id).join("undo.json").exists());
}

#[test]
fn doctor_fix_refuses_with_exit_5_when_lock_is_held() {
    let dir = tempdir().unwrap();
    let cache = temp_cache(&dir, ".tokenzero/recovery-cache.json");
    let _lock = DoctorLock::acquire(dir.path()).unwrap();
    let report = doctor_fix(dir.path(), Some(&cache), false);
    assert_fix_schema(&report);
    assert_eq!(report["status"], "concurrency_lost");
    assert_eq!(report["exit_code"], 5);
    assert_eq!(report["mutates"], false);
    assert!(!cache.parent().unwrap().exists());
}

#[test]
fn doctor_undo_refuses_non_empty_created_cache_parent() {
    let dir = tempdir().unwrap();
    let cache = temp_cache(&dir, ".tokenzero/recovery-cache.json");
    let fixed = doctor_fix(dir.path(), Some(&cache), false);
    let run_id = fixed["run_id"].as_str().unwrap();
    fs::write(&cache, b"later cache contents").unwrap();
    let undone = doctor_undo(dir.path(), run_id);
    assert_eq!(undone["ok"], false);
    assert_eq!(undone["exit_code"], 3);
    assert!(cache.exists());
}

#[test]
fn doctor_capabilities_names_doctor_contract_subcommands() {
    let capabilities = doctor_capabilities();
    assert_eq!(capabilities["schema_version"], "tokenzero.doctor.capabilities.v1");
    assert_eq!(capabilities["supports_fix"], true);
    assert_eq!(capabilities["supports_undo"], true);
    for key in ["commands", "fixers", "detectors"] {
        assert!(capabilities[key].as_array().is_some(), "capabilities JSON missing required key: {key}");
    }
}

#[test]
fn doctor_robot_triage_plans_supported_cache_parent_fix() {
    let dir = tempdir().unwrap();
    let triage = doctor_robot_triage(dir.path(), Some(&temp_cache(&dir, ".tokenzero/recovery-cache.json")));
    assert_eq!(triage["ok"], true);
    assert_eq!(triage["recommended_command"], "tokenzero doctor --dry-run --fix --json");
    let planned = triage["actions_planned"].as_array().unwrap();
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0]["finding_id"], DOCTOR_FIXER_CACHE_PARENT);
}

#[test]
fn global_mcp_plan_covers_ai_clients_and_platform_launcher() {
    let dir = tempdir().unwrap();
    let report = plan(dir.path(), true, &["mcp".to_string()]);
    let launcher_suffix = if cfg!(windows) {
        ".tokenzero/bin/tokenzero.cmd"
    } else {
        ".tokenzero/bin/tokenzero"
    };
    for suffix in [
        launcher_suffix,
        ".claude.json",
        ".codex/config.toml",
        ".cursor/mcp.json",
        ".gemini/settings.json",
        ".grok/config.toml",
        ".factory/mcp.json",
        ".config/tokenzero/agents/droid.mcp.json",
    ] {
        assert!(write_ends_with(&report, suffix), "missing {suffix}");
    }
    assert!(write_contains(&report, ".tokenzero/bin/tokenzero-runtime-"));
    if cfg!(windows) {
        assert!(write_ends_with(&report, ".tokenzero/bin/tokenzero"));
        assert!(write_ends_with(&report, "AppData/Roaming/Claude/claude_desktop_config.json"));
        assert!(!write_contains(&report, "Library/Application Support"));
    }
}

#[test]
fn global_mcp_plan_can_target_grok_only() {
    let dir = tempdir().unwrap();
    let report = plan_for_agents(
        dir.path(),
        true,
        &["mcp".to_string()],
        &["grok".to_string()],
        McpToolSurface::Classic,
    );
    assert!(write_ends_with(&report, ".grok/config.toml"));
    assert!(write_ends_with(&report, ".config/tokenzero/agents/grok.mcp.json"));
    assert!(!write_ends_with(&report, ".claude.json"));
    assert!(!write_ends_with(&report, ".cursor/mcp.json"));
}

#[test]
fn global_json_mcp_merge_preserves_existing_servers() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("home with spaces");
    fs::create_dir_all(&root).unwrap();
    let cursor = root.join(".cursor/mcp.json");
    fs::create_dir_all(cursor.parent().unwrap()).unwrap();
    fs::write(&cursor, r#"{"mcpServers":{"superconductor":{"url":"http://localhost:31418/mcp"}}}"#).unwrap();
    apply(&root, true, &["mcp".to_string()]).unwrap();
    let merged: Value = serde_json::from_str(&fs::read_to_string(&cursor).unwrap()).unwrap();
    assert_eq!(merged["mcpServers"]["superconductor"]["url"], "http://localhost:31418/mcp");
    let command = merged["mcpServers"]["tokenzero"]["command"].as_str().unwrap().replace('\\', "/");
    assert!(command.contains(".tokenzero/bin/tokenzero-runtime-"));
    if cfg!(windows) {
        assert!(command.ends_with(".exe"));
    }
    let args = &merged["mcpServers"]["tokenzero"]["args"];
    assert_eq!(args[0], "mcp-server");
    assert_eq!(args[1], "--allowed-root");
    assert_eq!(args[2], root.display().to_string());
    assert_eq!(args[3], "--cache-path");
    assert_eq!(args[4], cache_path(&root).display().to_string());
}

#[cfg(windows)]
#[test]
fn global_toml_mcp_merge_replaces_old_tokenzero_table_once() {
    let dir = tempdir().unwrap();
    let codex = dir.path().join(".codex/config.toml");
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    fs::write(
        &codex,
        r#"[mcp_servers.other]
command = "other"

[mcp_servers.tokenzero]
command = "/old/script/shim"
args = ["mcp-server"]

[mcp_servers.tokenzero.tools.read]
approval_mode = "approve"

[profiles.default]
model = "gpt"
"#,
    )
    .unwrap();
    apply(dir.path(), true, &["mcp".to_string()]).unwrap();
    let merged = fs::read_to_string(&codex).unwrap();
    toml::from_str::<toml::Value>(&merged).unwrap();
    assert!(merged.contains("[mcp_servers.other]"));
    assert!(merged.contains("[profiles.default]"));
    assert!(!merged.contains("/old/script/shim"));
    assert_eq!(merged.matches("[mcp_servers.tokenzero]").count(), 1);
    assert!(merged.contains("# tokenzero:mcp:start"));
    let normalized = merged.replace('\\', "/");
    assert!(normalized.contains("tokenzero-runtime-"));
    if cfg!(windows) {
        assert!(normalized.contains(".exe"));
        assert!(!normalized.contains("cmd.exe"));
        assert!(!normalized.contains("tokenzero.cmd"));
        assert!(!normalized.contains("\"/C\""));
    }
}

#[test]
fn client_surface_inspection_rejects_tokenzero_substring_without_valid_toml_command() {
    let dir = tempdir().unwrap();
    let codex = dir.path().join(".codex/config.toml");
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    fs::write(&codex, "[mcp_servers.tokenzero]\ncommand = \"/bin/false\"\nargs = [\"mcp-server\"]\n").unwrap();
    let report = plan_for_agents(dir.path(), true, &["mcp".to_string()], &["codex".to_string()], McpToolSurface::Classic);
    let row = report.writes.iter().find(|row| row.path.ends_with(".codex/config.toml")).unwrap();
    let status = inspect_client_surface(row, dir.path());
    assert!(status.exists);
    assert_eq!(status.state, "mixed");
    assert!(!status.installed);
    assert!(status.checks.iter().any(|c| c.name == "mcp_command_targets_installed_runtime" && !c.ok));
}

#[test]
fn client_surface_inspection_accepts_applied_grok_json_and_toml_configs() {
    let dir = tempdir().unwrap();
    apply_for_agents(dir.path(), true, &["mcp".to_string()], &["grok".to_string()], McpToolSurface::Classic).unwrap();
    let report = plan_for_agents(dir.path(), true, &["mcp".to_string()], &["grok".to_string()], McpToolSurface::Classic);
    for suffix in [".config/tokenzero/agents/grok.mcp.json", ".grok/config.toml"] {
        let row = report.writes.iter().find(|row| row.path.ends_with(suffix)).unwrap();
        let status = inspect_client_surface(row, dir.path());
        assert_eq!(status.state, "installed", "{suffix}: {status:#?}");
        assert!(status.installed, "{suffix}: {status:#?}");
    }
}

#[cfg(windows)]
#[test]
fn global_cli_runtime_copy_is_rollback_capable() {
    let dir = tempdir().unwrap();
    let applied = apply(dir.path(), true, &["cli".to_string()]).unwrap();
    let runtime = applied
        .written
        .iter()
        .find(|path| path.contains("tokenzero-runtime-"))
        .cloned()
        .expect("global CLI install should copy an installed runtime binary");
    assert!(PathBuf::from(&runtime).exists());
    rollback(dir.path(), "latest").unwrap();
    assert!(!PathBuf::from(&runtime).exists());
}

#[cfg(windows)]
#[test]
fn global_cli_and_shell_wrappers_are_executable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    apply(dir.path(), true, &["cli".to_string(), "shell".to_string()]).unwrap();
    for path in [
        dir.path().join(".tokenzero/bin/tokenzero"),
        dir.path().join(".tokenzero/bin/tokenzero-shell"),
    ] {
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0);
        assert!(fs::read_to_string(&path).unwrap().starts_with("#!/bin/sh"));
    }
}

#[test]
fn atomic_write_replaces_cleanly_without_tmp_debris() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("nested/config.json");
    atomic_write(&target, b"{\"a\":1}\n").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "{\"a\":1}\n");
    atomic_write(&target, b"{\"a\":22}\n").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "{\"a\":22}\n");
    let debris = fs::read_dir(dir.path().join("nested")).unwrap().filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count();
    assert_eq!(debris, 0, "atomic_write must not leave temp debris");
}

#[test]
fn hooks_plan_is_scoped_to_claude_agents() {
    let dir = tempdir().unwrap();
    let hooks = vec!["hooks".to_string()];
    for agents in [Vec::new(), vec!["claude".to_string()]] {
        let report = plan_for_agents(dir.path(), true, &hooks, &agents, McpToolSurface::Classic);
        assert!(
            report.writes.iter().any(|write| {
                write.path.replace('\\', "/").ends_with(".claude/settings.json")
                    && write.capability == "hooks"
                    && write.action == "merge"
                    && write.global
            }),
            "expected hooks write for agents {agents:?}: {:?}",
            report.writes
        );
    }
    let grok = plan_for_agents(dir.path(), true, &hooks, &["grok".to_string()], McpToolSurface::Classic);
    assert!(
        !grok
            .writes
            .iter()
            .any(|write| write.path.replace('\\', "/").contains("/.claude")),
        "{:?}",
        grok.writes
    );
}

#[test]
fn global_hooks_merge_preserves_foreign_hooks_and_is_idempotent() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("home with spaces");
    fs::create_dir_all(&root).unwrap();
    let settings = root.join(".claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        r#"{"model":"opus","permissions":{"allow":["Bash(ls:*)"]},"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"/usr/local/bin/guard.sh"}]}],"PostToolUse":[{"matcher":"Edit","hooks":[{"type":"command","command":"fmt.sh"}]}]}}"#,
    )
    .unwrap();
    let hooks = vec!["hooks".to_string()];
    let claude = vec!["claude".to_string()];
    apply_for_agents(&root, true, &hooks, &claude, McpToolSurface::Classic).unwrap();
    apply_for_agents(&root, true, &hooks, &claude, McpToolSurface::Classic).unwrap();
    let merged: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(merged["model"], "opus");
    assert_eq!(merged["permissions"]["allow"][0], "Bash(ls:*)");
    assert_eq!(merged["hooks"]["PostToolUse"][0]["hooks"][0]["command"], "fmt.sh");
    let pre_tool_use = merged["hooks"]["PreToolUse"].as_array().unwrap();
    assert!(pre_tool_use.iter().any(|entry| entry["hooks"][0]["command"] == "/usr/local/bin/guard.sh"));
    let tokenzero_entries: Vec<&Value> = pre_tool_use.iter().filter(|entry| is_tokenzero_hook_entry(entry)).collect();
    assert_eq!(tokenzero_entries.len(), 2, "second apply must not duplicate");
    let matchers: Vec<&str> = tokenzero_entries.iter().map(|entry| entry["matcher"].as_str().unwrap()).collect();
    assert_eq!(matchers, ["Bash", "Read"]);
    for entry in tokenzero_entries {
        let hook = &entry["hooks"][0];
        assert_eq!(hook["type"], "command");
        assert_eq!(hook["timeout"], 10);
        assert_eq!(hook["command"].as_str().unwrap(), hook_command(&root, true));
    }
}

#[test]
fn hooks_merge_rejects_invalid_settings_without_touching_the_file() {
    assert_eq!(merge_json_hooks("{not json", "tokenzero hook claude-code").unwrap_err().kind(), ErrorKind::InvalidData);
    assert_eq!(
        merge_json_hooks(r#"{"hooks": {"PreToolUse": {"matcher": "Bash"}}}"#, "tokenzero hook claude-code")
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidData
    );
    let dir = tempdir().unwrap();
    let settings = dir.path().join(".claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, "{broken").unwrap();
    assert!(apply_for_agents(dir.path(), true, &["hooks".to_string()], &["claude".to_string()], McpToolSurface::Classic).is_err());
    assert_eq!(fs::read_to_string(&settings).unwrap(), "{broken");
}

#[test]
fn hooks_surface_inspection_reports_installed_after_apply() {
    let dir = tempdir().unwrap();
    let hooks = vec!["hooks".to_string()];
    let claude = vec!["claude".to_string()];
    let report = plan_for_agents(dir.path(), true, &hooks, &claude, McpToolSurface::Classic);
    let row = report.writes.iter().find(|row| row.capability == "hooks").unwrap();
    assert_eq!(inspect_client_surface(row, dir.path()).state, "missing");
    apply_for_agents(dir.path(), true, &hooks, &claude, McpToolSurface::Classic).unwrap();
    let after = inspect_client_surface(row, dir.path());
    assert_eq!(after.state, "installed", "{after:#?}");
    assert!(after.checks.iter().any(|c| c.name == "hooks_command_targets_installed_runtime" && c.ok));
    fs::write(
        PathBuf::from(&row.path),
        r#"{"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/usr/local/bin/guard.sh"}]}]}}"#,
    )
    .unwrap();
    assert_eq!(inspect_client_surface(row, dir.path()).state, "mixed");
}

#[test]
fn shim_plan_contains_only_resolvable_tools() {
    let dir = tempdir().unwrap();
    let report = plan(dir.path(), true, &["shim".to_string()]);
    let planned: Vec<String> = report
        .writes
        .iter()
        .filter(|write| write.capability == "shim")
        .map(|write| PathBuf::from(&write.path).file_name().unwrap().to_string_lossy().to_string())
        .collect();
    for write in report.writes.iter().filter(|w| w.capability == "shim") {
        assert!(write.path.replace('\\', "/").contains(".tokenzero/shims/"), "{}", write.path);
        assert_eq!(write.action, "write");
    }
    for tool in SHIM_TOOLS {
        let resolvable = resolve_real_tool(tool, &shims_dir(dir.path())).is_some();
        assert_eq!(planned.iter().any(|name| name == tool), resolvable, "shim plan must include {tool} iff it resolves on PATH");
    }
    #[cfg(unix)]
    for always_present in ["cat", "ls"] {
        assert!(planned.iter().any(|name| name == always_present));
    }
}

#[cfg(unix)]
#[test]
fn shim_resolution_skips_shim_dirs_and_missing_tools() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let bin = dir.path().join("bin");
    let shim_dir = dir.path().join(".tokenzero/shims");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&shim_dir).unwrap();
    let real_cat = bin.join("cat");
    fs::write(&real_cat, "#!/bin/sh\nexec /bin/cat \"$@\"\n").unwrap();
    fs::set_permissions(&real_cat, fs::Permissions::from_mode(0o755)).unwrap();
    let stale_shims = dir.path().join("old-home/.tokenzero/shims");
    fs::create_dir_all(&stale_shims).unwrap();
    let stale_grep = stale_shims.join("grep");
    fs::write(&stale_grep, "#!/bin/sh\n# tokenzero shim for grep — generated.\nexec /usr/bin/grep \"$@\"\n").unwrap();
    fs::set_permissions(&stale_grep, fs::Permissions::from_mode(0o755)).unwrap();
    let decoy = shim_dir.join("grep");
    fs::write(&decoy, "#!/bin/sh\nexec /usr/bin/grep \"$@\"\n").unwrap();
    fs::set_permissions(&decoy, fs::Permissions::from_mode(0o755)).unwrap();
    let path_var = std::env::join_paths([shim_dir.clone(), stale_shims, bin.clone()]).unwrap();
    assert_eq!(resolve_real_tool_in("cat", &shim_dir, &path_var), Some(real_cat));
    assert_eq!(resolve_real_tool_in("grep", &shim_dir, &path_var), None);
    assert_eq!(resolve_real_tool_in("rg", &shim_dir, &path_var), None);
}

#[cfg(unix)]
#[test]
fn shim_apply_writes_guarded_executable_scripts_and_rollback_restores_absence() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let applied = apply(dir.path(), true, &["shim".to_string()]).unwrap();
    let shim = dir.path().join(".tokenzero/shims/cat");
    assert!(applied.written.iter().any(|path| path.ends_with("/cat")));
    assert!(shim.exists());
    assert_ne!(fs::metadata(&shim).unwrap().permissions().mode() & 0o111, 0, "shim must be executable");
    let text = fs::read_to_string(&shim).unwrap();
    for needle in [
        "#!/bin/sh\n",
        "# tokenzero shim for cat",
        "[ \"$TOKENZERO_SHIM\" = \"1\" ]",
        "[ -z \"$TOKENZERO_INNER\" ]",
        "TOKENZERO_INNER=1 exec",
        &tokenzero_command(dir.path(), true),
        "[ -x \"$TZ\" ]",
    ] {
        assert!(text.contains(needle), "missing {needle}: {text}");
    }
    assert!(text.starts_with("#!/bin/sh\n"));
    rollback(dir.path(), "latest").unwrap();
    assert!(!shim.exists(), "rollback must remove the planned shim");
}

#[cfg(unix)]
#[test]
fn shim_falls_through_to_real_binary_when_launcher_is_missing() {
    let dir = tempdir().unwrap();
    apply(dir.path(), true, &["shim".to_string()]).unwrap();
    let shim = dir.path().join(".tokenzero/shims/grep");
    let haystack = dir.path().join("haystack.txt");
    fs::write(&haystack, "alpha\nbeta\n").unwrap();
    let run = |needle: &str| {
        std::process::Command::new("sh")
            .arg(shim.as_os_str())
            .arg(needle)
            .arg(haystack.as_os_str())
            .env("TOKENZERO_SHIM", "1")
            .env_remove("TOKENZERO_INNER")
            .status()
            .unwrap()
    };
    assert_eq!(run("alpha").code(), Some(0));
    assert_eq!(run("zz-no-match").code(), Some(1));
}

#[test]
fn shim_surface_inspection_reports_installed_after_apply() {
    let dir = tempdir().unwrap();
    let report = plan(dir.path(), true, &["shim".to_string()]);
    let row = report.writes.iter().find(|row| row.path.ends_with("/cat")).unwrap();
    assert_eq!(inspect_client_surface(row, dir.path()).state, "missing");
    apply(dir.path(), true, &["shim".to_string()]).unwrap();
    let status = inspect_client_surface(row, dir.path());
    assert_eq!(status.state, "installed", "{status:#?}");
    for check in ["shim_executable", "shim_guards_on_env", "shim_targets_installed_runtime"] {
        assert!(status.checks.iter().any(|c| c.name == check && c.ok), "{check}: {status:#?}");
    }
}

#[cfg(unix)]
#[test]
fn shim_passthrough_matches_real_grep_exit_codes() {
    let dir = tempdir().unwrap();
    apply(dir.path(), true, &["shim".to_string()]).unwrap();
    let shim = dir.path().join(".tokenzero/shims/grep");
    assert!(shim.exists(), "grep must resolve on unix test hosts");
    let haystack = dir.path().join("haystack.txt");
    fs::write(&haystack, "alpha\nbeta\n").unwrap();
    for (envs, needle, expected) in [
        (Vec::new(), "alpha", 0),
        (Vec::new(), "missing", 1),
        (
            vec![("TOKENZERO_SHIM", "1"), ("TOKENZERO_INNER", "1")],
            "alpha",
            0,
        ),
        (
            vec![("TOKENZERO_SHIM", "1"), ("TOKENZERO_INNER", "1")],
            "missing",
            1,
        ),
    ] {
        let mut command = std::process::Command::new("sh");
        command
            .arg(&shim)
            .arg(needle)
            .arg(&haystack)
            .env_remove("TOKENZERO_SHIM")
            .env_remove("TOKENZERO_INNER");
        for (key, value) in &envs {
            command.env(key, value);
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected),
            "envs {envs:?} needle {needle}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        if expected == 0 {
            assert_eq!(String::from_utf8_lossy(&output.stdout), "alpha\n");
        }
    }
}
