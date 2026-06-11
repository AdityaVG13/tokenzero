use crate::*;

pub(crate) fn content_for(
    row: &InstallWrite,
    root: &Path,
    previous: Option<&str>,
) -> std::io::Result<PendingContent> {
    match row.capability.as_str() {
        "mcp" => {
            let path = Path::new(&row.path);
            if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
                merge_toml_mcp(previous.unwrap_or_default(), root, path, row.global)
                    .map(PendingContent::Text)
            } else {
                merge_json_mcp(previous.unwrap_or_default(), root, row.global)
                    .map(PendingContent::Text)
            }
        }
        "instructions" => Ok(PendingContent::Text(
            "<!-- tokenzero:rust-core:start -->\nUse `tokenzero read/find/tree/run/expand` or MCP aliases. Rust Core runs as a standalone binary for normal use.\n<!-- tokenzero:rust-core:end -->\n".to_string(),
        )),
        "shell" => Ok(PendingContent::Text(shell_launcher_content(root, row.global))),
        "cli" => Ok(PendingContent::Text(cli_launcher_content(root, row.global))),
        "cli-runtime" => current_exe_bytes().map(PendingContent::Bytes),
        "cli-shim" => Ok(PendingContent::Text(windows_posix_cli_shim_content())),
        "hooks" => merge_json_hooks(
            previous.unwrap_or_default(),
            &hook_command(root, row.global),
        )
        .map(PendingContent::Text),
        "shim" => shim_content(Path::new(&row.path), root, row.global).map(PendingContent::Text),
        _ => Ok(PendingContent::Text(serde_json::json!({
            "schema_version": "tokenzero.runtime_manifest.v1",
            "runtime": "rust",
            "external_runtime_required": false,
            "binary": runtime_manifest_binary(root, row.global),
            "source_binary": current_exe_string(),
            "global_launcher": tokenzero_command(root, row.global)
        })
        .to_string()
            + "\n")),
    }
}

/// Parse `previous` as a JSON object document (empty input -> empty object),
/// preserving the exact error wording each merge fn reported before dedup.
fn parse_json_object(previous: &str, what: &str) -> std::io::Result<Map<String, Value>> {
    if previous.trim().is_empty() {
        return Ok(Map::new());
    }
    let value: Value = serde_json::from_str(previous)
        .map_err(|err| Error::new(ErrorKind::InvalidData, format!("invalid JSON: {err}")))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(Error::new(
            ErrorKind::InvalidData,
            format!("{what} must be an object"),
        )),
    }
}

/// `object[key]` as a mutable object, inserting an empty one if absent.
fn object_entry<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
    what: &str,
) -> std::io::Result<&'a mut Map<String, Value>> {
    object
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, format!("{what} must be an object")))
}

/// Replace the TokenZero-owned entries in the `hooks.<key>` array, leaving
/// every foreign entry untouched.
fn upsert_tokenzero_hooks(
    hooks_object: &mut Map<String, Value>,
    key: &str,
    new_entries: Vec<Value>,
) -> std::io::Result<()> {
    let entries = hooks_object
        .entry(key)
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Claude settings hooks.{key} field must be an array"),
            )
        })?;
    entries.retain(|entry| !is_tokenzero_hook_entry(entry));
    entries.extend(new_entries);
    Ok(())
}

pub(crate) fn merge_json_mcp(previous: &str, root: &Path, global: bool) -> std::io::Result<String> {
    let mut object = parse_json_object(previous, "JSON MCP config")?;
    let servers = object_entry(
        &mut object,
        "mcpServers",
        "JSON MCP config mcpServers field",
    )?;
    servers.insert("tokenzero".to_string(), mcp_server_json(root, global));
    Ok(serde_json::to_string_pretty(&Value::Object(object))? + "\n")
}

/// Upsert only the TokenZero-owned `hooks.PreToolUse` and
/// `hooks.SessionStart` entries into a Claude settings document, mirroring
/// `merge_json_mcp`'s one-key merge: every other hook and setting survives
/// the round-trip (key order may change). The SessionStart entry restores
/// the TokenZero session pack after compaction/resume.
pub(crate) fn merge_json_hooks(previous: &str, hook_command: &str) -> std::io::Result<String> {
    let mut object = parse_json_object(previous, "Claude settings JSON")?;
    let hooks_object = object_entry(&mut object, "hooks", "Claude settings hooks field")?;
    upsert_tokenzero_hooks(
        hooks_object,
        "PreToolUse",
        vec![
            serde_json::json!({
                "matcher": "Bash",
                "hooks": [{
                    "type": "command",
                    "command": hook_command,
                    "timeout": 10,
                }]
            }),
            // Same adapter command: the hook dispatches on tool_name, so the
            // Read matcher reuses it for the unbounded-large-Read guard.
            serde_json::json!({
                "matcher": "Read",
                "hooks": [{
                    "type": "command",
                    "command": hook_command,
                    "timeout": 10,
                }]
            }),
        ],
    )?;
    upsert_tokenzero_hooks(
        hooks_object,
        "SessionStart",
        vec![serde_json::json!({
            "hooks": [{
                "type": "command",
                // `<launcher> hook claude-code` + suffix = the SessionStart
                // adapter; the shared needle keeps both entries TokenZero-owned.
                "command": format!("{hook_command}-session-start"),
                "timeout": 10,
            }]
        })],
    )?;
    Ok(serde_json::to_string_pretty(&Value::Object(object))? + "\n")
}

/// One AI harness detected on this machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedAgent {
    pub agent: String,
    pub evidence: String,
    /// Whether `tokenzero install --agent <agent>` can wire it today;
    /// unsupported harnesses are adapted manually per docs/routing.md.
    pub supported: bool,
}

/// Probe the machine for known AI coding harnesses: config directories under
/// the home root plus launcher binaries on PATH. Pure over its inputs so
/// tests pass a fixture home and PATH string; detection never writes.
pub fn detect_present_agents(home: &Path, path_env: Option<&str>) -> Vec<DetectedAgent> {
    type Probe = (
        &'static str,
        &'static [&'static str],
        &'static [&'static str],
        bool,
    );
    const PROBES: &[Probe] = &[
        ("claude", &[".claude", ".claude.json"], &["claude"], true),
        ("codex", &[".codex"], &["codex"], true),
        ("cursor", &[".cursor"], &["cursor-agent", "cursor"], true),
        ("gemini", &[".gemini"], &["gemini"], true),
        ("factory", &[".factory"], &["droid"], true),
        ("opencode", &[".config/opencode"], &["opencode"], true),
        ("grok", &[".grok"], &["grok"], true),
        ("windsurf", &[".codeium/windsurf"], &["windsurf"], false),
        ("cline", &[".cline"], &[], false),
        ("aider", &[".aider.conf.yml"], &["aider"], false),
        ("zed", &[".config/zed"], &["zed"], false),
        ("crush", &[".config/crush"], &["crush"], false),
    ];
    let mut detected = Vec::new();
    for (agent, home_paths, binaries, supported) in PROBES {
        let mut evidence = home_paths.iter().find_map(|rel| {
            let candidate = home.join(rel);
            candidate
                .exists()
                .then(|| format!("{} exists", candidate.display()))
        });
        if evidence.is_none() {
            if let Some(path_env) = path_env {
                'dirs: for dir in std::env::split_paths(path_env) {
                    for binary in *binaries {
                        if is_executable_file(&dir.join(binary)) {
                            evidence = Some(format!("{binary} on PATH"));
                            break 'dirs;
                        }
                    }
                }
            }
        }
        if let Some(evidence) = evidence {
            detected.push(DetectedAgent {
                agent: (*agent).to_string(),
                evidence,
                supported: *supported,
            });
        }
    }
    detected
}

/// The TokenZero-owned PreToolUse entry is the one whose hook command invokes
/// `... hook claude-code` from a tokenzero binary; both fragments are matched
/// separately because the launcher/runtime file name varies per install
/// (`tokenzero`, `tokenzero.cmd`, `tokenzero-runtime-<hash>`).
pub(crate) fn is_tokenzero_hook_entry(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| {
                        command.contains("tokenzero") && command.contains("hook claude-code")
                    })
            })
        })
}

/// Claude Code runs hook commands through a shell, so the launcher path is
/// quoted (homes with spaces are a tested install target).
pub(crate) fn hook_command(root: &Path, global: bool) -> String {
    let launcher = tokenzero_command(root, global);
    if cfg!(windows) {
        format!("\"{launcher}\" hook claude-code")
    } else {
        format!("{} hook claude-code", shell_quote(&launcher))
    }
}

pub(crate) fn merge_toml_mcp(
    previous: &str,
    root: &Path,
    path: &Path,
    global: bool,
) -> std::io::Result<String> {
    let without_managed = strip_managed_toml_block(previous);
    let without_old_tokenzero = strip_tokenzero_toml_tables(&without_managed);
    let mut merged = without_old_tokenzero.trim_end().to_string();
    if !merged.is_empty() {
        merged.push_str("\n\n");
    }
    merged.push_str(&toml_mcp_block(root, path, global));
    toml::from_str::<toml::Value>(&merged)
        .map_err(|err| Error::new(ErrorKind::InvalidData, format!("invalid TOML: {err}")))?;
    Ok(merged)
}

pub(crate) fn mcp_server_json(root: &Path, global: bool) -> Value {
    serde_json::json!({
        "type": "stdio",
        "command": mcp_command(root, global),
        "args": mcp_args(root),
        "env": mcp_env(root)
    })
}

pub(crate) fn mcp_args(root: &Path) -> Vec<String> {
    vec![
        "mcp-server".to_string(),
        "--allowed-root".to_string(),
        root.display().to_string(),
        "--cache-path".to_string(),
        cache_path(root).display().to_string(),
        "--supervise".to_string(),
    ]
}

pub(crate) fn mcp_env(root: &Path) -> Map<String, Value> {
    let mut env = Map::new();
    env.insert(
        "TOKENZERO_ALLOWED_ROOTS".to_string(),
        Value::String(root.display().to_string()),
    );
    env.insert(
        "TOKENZERO_CACHE_PATH".to_string(),
        Value::String(cache_path(root).display().to_string()),
    );
    env.insert(
        "TOKENZERO_DEFAULT_MODE".to_string(),
        Value::String("auto".to_string()),
    );
    env.insert(
        "TOKENZERO_MCP_TOOL_SURFACE".to_string(),
        Value::String("aliases".to_string()),
    );
    env.insert(
        "TOKENZERO_MAX_OUTPUT_BYTES".to_string(),
        Value::String("2000000".to_string()),
    );
    env.insert(
        "TOKENZERO_SHELL_TIMEOUT".to_string(),
        Value::String("30".to_string()),
    );
    env.insert(
        "TOKENZERO_CACHE_BLOBS".to_string(),
        Value::String("512".to_string()),
    );
    env.insert(
        "TOKENZERO_CACHE_UNITS".to_string(),
        Value::String("8192".to_string()),
    );
    // Long agent sessions can idle for hours between tool calls; an
    // idle-exited server reads as a permanent disconnect to MCP clients.
    env.insert(
        "TOKENZERO_MCP_IDLE_TIMEOUT_SECS".to_string(),
        Value::String("0".to_string()),
    );
    env
}

pub(crate) fn toml_mcp_block(root: &Path, path: &Path, global: bool) -> String {
    let include_codex_tool_approvals = path
        .components()
        .any(|component| component.as_os_str() == ".codex");
    let mut block = format!(
        "# tokenzero:mcp:start\n\
         [mcp_servers.tokenzero]\n\
         command = {}\n\
         args = {}\n\
         enabled = true\n\
         startup_timeout_sec = 15\n\
         tool_timeout_sec = 120\n\n\
         [mcp_servers.tokenzero.env]\n{}\n",
        toml_string(&mcp_command(root, global)),
        toml_string_array(&mcp_args(root)),
        toml_env_lines(root)
    );
    if include_codex_tool_approvals {
        for tool in ["shell", "read", "find", "tree"] {
            block.push_str(&format!(
                "\n[mcp_servers.tokenzero.tools.{tool}]\napproval_mode = \"approve\"\n"
            ));
        }
    }
    block.push_str("# tokenzero:mcp:end\n");
    block
}

pub(crate) fn toml_env_lines(root: &Path) -> String {
    let env = mcp_env(root);
    env.into_iter()
        .map(|(key, value)| {
            let value = value.as_str().unwrap_or_default();
            format!("{key} = {}", toml_string(value))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn strip_managed_toml_block(input: &str) -> String {
    let mut output = Vec::new();
    let mut skipping = false;
    for line in input.lines() {
        match line.trim() {
            "# tokenzero:mcp:start" => {
                skipping = true;
            }
            "# tokenzero:mcp:end" => {
                skipping = false;
            }
            _ if !skipping => output.push(line),
            _ => {}
        }
    }
    let mut text = output.join("\n");
    if input.ends_with('\n') && !text.is_empty() {
        text.push('\n');
    }
    text
}

pub(crate) fn strip_tokenzero_toml_tables(input: &str) -> String {
    let mut output = Vec::new();
    let mut skipping = false;
    for line in input.lines() {
        if let Some(table) = toml_table_name(line) {
            skipping =
                table == "mcp_servers.tokenzero" || table.starts_with("mcp_servers.tokenzero.");
            if skipping {
                continue;
            }
        }
        if !skipping {
            output.push(line);
        }
    }
    let mut text = output.join("\n");
    if input.ends_with('\n') && !text.is_empty() {
        text.push('\n');
    }
    text
}

pub(crate) fn toml_table_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') || trimmed.starts_with("[[") || !trimmed.ends_with(']') {
        return None;
    }
    Some(
        trimmed
            .trim_start_matches('[')
            .trim_end_matches(']')
            .trim()
            .to_string(),
    )
}

pub(crate) fn toml_string_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| toml_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub(crate) fn toml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}
