use crate::*;
use tokenzero_core::McpToolSurface;

pub fn inspect_client_surface(write: &InstallWrite, root: &Path) -> ClientSurfaceStatus {
    let path = PathBuf::from(&write.path);
    let exists = path.exists();
    let checks = if exists {
        client_surface_checks(write, &path, root)
    } else {
        Vec::new()
    };
    let installed = exists && !checks.is_empty() && checks.iter().all(|check| check.ok);
    let state = if installed {
        "installed"
    } else if exists {
        "mixed"
    } else {
        "missing"
    };
    ClientSurfaceStatus {
        path: write.path.clone(),
        action: write.action.clone(),
        capability: write.capability.clone(),
        global: write.global,
        exists,
        installed,
        state: state.to_string(),
        checks,
    }
}

pub(crate) fn client_surface_checks(
    write: &InstallWrite,
    path: &Path,
    root: &Path,
) -> Vec<ClientSurfaceCheck> {
    match write.capability.as_str() {
        "mcp" => mcp_surface_checks(path, root, write.global),
        "cli-runtime" => runtime_binary_checks(path),
        "cli" => text_surface_checks(
            path,
            &[
                ("launcher_mentions_tokenzero", "tokenzero"),
                ("launcher_targets_runtime", "tokenzero-runtime-"),
            ],
        ),
        // The Windows POSIX shim (git-bash/WSL) delegates to tokenzero.cmd rather
        // than embedding the runtime hash directly, so it targets the launcher,
        // not the runtime binary.
        "cli-shim" => text_surface_checks(
            path,
            &[
                ("launcher_mentions_tokenzero", "tokenzero"),
                ("shim_delegates_to_launcher", "tokenzero.cmd"),
            ],
        ),
        "runtime" => runtime_manifest_checks(path, root, write.global),
        "shell" => text_surface_checks(path, &[("shell_launcher_mentions_run", " run -- ")]),
        "instructions" => text_surface_checks(path, &[("instructions_pointer", "tokenzero")]),
        "hooks" => hooks_surface_checks(path, root, write.global),
        "shim" => shim_surface_checks(path, root, write.global),
        _ => text_surface_checks(path, &[("mentions_tokenzero", "tokenzero")]),
    }
}

pub(crate) fn hooks_surface_checks(
    path: &Path,
    root: &Path,
    global: bool,
) -> Vec<ClientSurfaceCheck> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            return vec![client_check(
                "hooks_settings_readable",
                false,
                format!("read error: {err}"),
            )];
        }
    };
    let parsed: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(err) => {
            return vec![client_check(
                "hooks_settings_json",
                false,
                format!("invalid JSON: {err}"),
            )];
        }
    };
    let entry = parsed
        .get("hooks")
        .and_then(|hooks| hooks.get("PreToolUse"))
        .and_then(Value::as_array)
        .and_then(|entries| entries.iter().find(|entry| is_tokenzero_hook_entry(entry)));
    let expected_command = hook_command(root, global);
    let command_ok = entry.is_some_and(|entry| {
        entry.get("matcher").and_then(Value::as_str) == Some("Bash")
            && entry
                .get("hooks")
                .and_then(Value::as_array)
                .is_some_and(|hooks| {
                    hooks.iter().any(|hook| {
                        hook.get("type").and_then(Value::as_str) == Some("command")
                            && hook.get("command").and_then(Value::as_str)
                                == Some(expected_command.as_str())
                    })
                })
    });
    let session_entry = parsed
        .get("hooks")
        .and_then(|hooks| hooks.get("SessionStart"))
        .and_then(Value::as_array)
        .and_then(|entries| entries.iter().find(|entry| is_tokenzero_hook_entry(entry)));
    vec![
        client_check(
            "hooks_pretooluse_tokenzero_entry",
            entry.is_some(),
            "hooks.PreToolUse contains the TokenZero entry".to_string(),
        ),
        client_check(
            "hooks_command_targets_installed_runtime",
            command_ok,
            format!("expected Bash matcher running {expected_command}"),
        ),
        client_check(
            "hooks_session_start_tokenzero_entry",
            session_entry.is_some(),
            "hooks.SessionStart restores the TokenZero session pack".to_string(),
        ),
    ]
}

pub(crate) fn shim_surface_checks(
    path: &Path,
    root: &Path,
    global: bool,
) -> Vec<ClientSurfaceCheck> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            return vec![client_check(
                "shim_readable",
                false,
                format!("read error: {err}"),
            )];
        }
    };
    let expected_launcher = tokenzero_command(root, global);
    vec![
        client_check(
            "shim_executable",
            is_executable_file(path),
            "executable shim script".to_string(),
        ),
        client_check(
            "shim_guards_on_env",
            text.contains("TOKENZERO_SHIM") && text.contains("TOKENZERO_INNER"),
            "guards on TOKENZERO_SHIM/TOKENZERO_INNER".to_string(),
        ),
        client_check(
            "shim_targets_installed_runtime",
            text.contains(&expected_launcher) && text.contains("-x \"$TZ\""),
            format!("expected launcher {expected_launcher} behind an -x guard"),
        ),
    ]
}

pub(crate) fn runtime_binary_checks(path: &Path) -> Vec<ClientSurfaceCheck> {
    let metadata = fs::metadata(path);
    match metadata {
        Ok(metadata) => vec![client_check(
            "runtime_copy_present",
            metadata.is_file() && metadata.len() > 0,
            format!("{} bytes", metadata.len()),
        )],
        Err(err) => vec![client_check(
            "runtime_copy_present",
            false,
            format!("metadata error: {err}"),
        )],
    }
}

pub(crate) fn runtime_manifest_checks(
    path: &Path,
    root: &Path,
    global: bool,
) -> Vec<ClientSurfaceCheck> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            return vec![client_check(
                "runtime_manifest_readable",
                false,
                format!("read error: {err}"),
            )];
        }
    };
    let parsed: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(err) => {
            return vec![client_check(
                "runtime_manifest_json",
                false,
                format!("invalid JSON: {err}"),
            )];
        }
    };
    let expected_binary = runtime_manifest_binary(root, global);
    let expected_launcher = tokenzero_command(root, global);
    vec![
        client_check(
            "runtime_manifest_binary",
            parsed.get("binary").and_then(Value::as_str) == Some(expected_binary.as_str()),
            format!("expected {expected_binary}"),
        ),
        client_check(
            "runtime_manifest_launcher",
            parsed.get("global_launcher").and_then(Value::as_str)
                == Some(expected_launcher.as_str()),
            format!("expected {expected_launcher}"),
        ),
        client_check(
            "runtime_manifest_no_external_runtime",
            parsed
                .get("external_runtime_required")
                .and_then(Value::as_bool)
                == Some(false),
            "external_runtime_required=false".to_string(),
        ),
    ]
}

pub(crate) fn text_surface_checks(
    path: &Path,
    needles: &[(&str, &str)],
) -> Vec<ClientSurfaceCheck> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            return vec![client_check(
                "text_readable",
                false,
                format!("read error: {err}"),
            )];
        }
    };
    let lower = text.to_ascii_lowercase();
    needles
        .iter()
        .map(|(name, needle)| {
            client_check(
                name,
                lower.contains(&needle.to_ascii_lowercase()),
                format!("contains {needle}"),
            )
        })
        .collect()
}

pub(crate) fn mcp_surface_checks(
    path: &Path,
    root: &Path,
    global: bool,
) -> Vec<ClientSurfaceCheck> {
    if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
        toml_mcp_surface_checks(path, root, global)
    } else {
        json_mcp_surface_checks(path, root, global)
    }
}

pub(crate) fn json_mcp_surface_checks(
    path: &Path,
    root: &Path,
    global: bool,
) -> Vec<ClientSurfaceCheck> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            return vec![client_check(
                "mcp_json_readable",
                false,
                format!("read error: {err}"),
            )];
        }
    };
    let parsed: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(err) => {
            return vec![client_check(
                "mcp_json_parse",
                false,
                format!("invalid JSON: {err}"),
            )];
        }
    };
    let server = parsed
        .get("mcpServers")
        .and_then(Value::as_object)
        .and_then(|servers| servers.get("tokenzero"));
    let command = server
        .and_then(|server| server.get("command"))
        .and_then(Value::as_str);
    let args = server
        .and_then(|server| server.get("args"))
        .and_then(json_string_array);
    let env = server.and_then(|server| server.get("env"));
    mcp_server_checks(
        server.is_some(),
        command,
        args.as_deref(),
        env.and_then(|env| env.get("TOKENZERO_ALLOWED_ROOTS"))
            .and_then(Value::as_str),
        env.and_then(|env| env.get("TOKENZERO_CACHE_PATH"))
            .and_then(Value::as_str),
        env.and_then(|env| env.get(McpToolSurface::ENV))
            .and_then(Value::as_str),
        root,
        global,
    )
}

pub(crate) fn toml_mcp_surface_checks(
    path: &Path,
    root: &Path,
    global: bool,
) -> Vec<ClientSurfaceCheck> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            return vec![client_check(
                "mcp_toml_readable",
                false,
                format!("read error: {err}"),
            )];
        }
    };
    let parsed: toml::Value = match toml::from_str(&text) {
        Ok(value) => value,
        Err(err) => {
            return vec![client_check(
                "mcp_toml_parse",
                false,
                format!("invalid TOML: {err}"),
            )];
        }
    };
    let server = parsed
        .get("mcp_servers")
        .and_then(toml::Value::as_table)
        .and_then(|servers| servers.get("tokenzero"));
    let command = server
        .and_then(|server| server.get("command"))
        .and_then(toml::Value::as_str);
    let args = server
        .and_then(|server| server.get("args"))
        .and_then(toml_string_array_value);
    let env = server.and_then(|server| server.get("env"));
    mcp_server_checks(
        server.is_some(),
        command,
        args.as_deref(),
        env.and_then(|env| env.get("TOKENZERO_ALLOWED_ROOTS"))
            .and_then(toml::Value::as_str),
        env.and_then(|env| env.get("TOKENZERO_CACHE_PATH"))
            .and_then(toml::Value::as_str),
        env.and_then(|env| env.get(McpToolSurface::ENV))
            .and_then(toml::Value::as_str),
        root,
        global,
    )
}

/// Normalize path separators so a value round-tripped through an agent config
/// matches the freshly-derived expectation regardless of separator style. The
/// install writes and re-derives every path from the same root, so a `\` vs `/`
/// difference (which only arises on Windows) must still read as a match. This is
/// a no-op on Unix, where paths never contain a backslash separator.
fn normalize_path_sep(value: &str) -> String {
    value.replace('\\', "/")
}

fn path_str_matches(actual: Option<&str>, expected: &str) -> bool {
    actual.map(normalize_path_sep) == Some(normalize_path_sep(expected))
}

fn path_args_match(actual: Option<&[String]>, expected: &[String]) -> bool {
    let normalize = |args: &[String]| {
        args.iter()
            .map(|arg| normalize_path_sep(arg))
            .collect::<Vec<_>>()
    };
    actual.map(normalize) == Some(normalize(expected))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn mcp_server_checks(
    server_present: bool,
    command: Option<&str>,
    args: Option<&[String]>,
    allowed_roots: Option<&str>,
    cache: Option<&str>,
    tool_surface: Option<&str>,
    root: &Path,
    global: bool,
) -> Vec<ClientSurfaceCheck> {
    let expected_command = mcp_command(root, global);
    let expected_args = mcp_args(root);
    let expected_allowed_roots = root.display().to_string();
    let expected_cache = cache_path(root).display().to_string();
    let parsed_surface = tool_surface.and_then(|value| value.parse::<McpToolSurface>().ok());
    vec![
        client_check(
            "mcp_tokenzero_server_present",
            server_present,
            "mcpServers.tokenzero or mcp_servers.tokenzero".to_string(),
        ),
        client_check(
            "mcp_command_targets_installed_runtime",
            path_str_matches(command, &expected_command),
            format!("expected {expected_command}"),
        ),
        client_check(
            "mcp_args_match",
            path_args_match(args, &expected_args),
            format!("expected {:?}", expected_args),
        ),
        client_check(
            "mcp_allowed_roots_match",
            path_str_matches(allowed_roots, &expected_allowed_roots),
            format!("expected {expected_allowed_roots}"),
        ),
        client_check(
            "mcp_cache_path_match",
            path_str_matches(cache, &expected_cache),
            format!("expected {expected_cache}"),
        ),
        client_check(
            "mcp_tool_surface_valid",
            parsed_surface.is_some(),
            format!(
                "expected {} to be classic or codemode",
                McpToolSurface::ENV
            ),
        ),
    ]
}

pub(crate) fn json_string_array(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(ToString::to_string))
        .collect()
}

pub(crate) fn toml_string_array_value(value: &toml::Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(ToString::to_string))
        .collect()
}

pub(crate) fn client_check(name: &str, ok: bool, detail: String) -> ClientSurfaceCheck {
    ClientSurfaceCheck {
        name: name.to_string(),
        ok,
        detail,
    }
}
