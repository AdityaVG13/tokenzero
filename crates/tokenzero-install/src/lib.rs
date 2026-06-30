#![forbid(unsafe_code)]

mod package_audit;
pub use package_audit::package_audit;

use fs4::{FileExt, TryLockError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Error, ErrorKind, Write};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tokenzero_core::{INSTALL_SCHEMA_VERSION, McpToolSurface};

#[cfg(windows)]
const WINDOWS_USER_PATH_REGISTRY: &str = "HKCU\\Environment\\Path";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallWrite {
    pub path: String,
    pub action: String,
    pub backup_id: String,
    pub capability: String,
    pub global: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientSurfaceCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientSurfaceStatus {
    pub path: String,
    pub action: String,
    pub capability: String,
    pub global: bool,
    pub exists: bool,
    pub installed: bool,
    pub state: String,
    pub checks: Vec<ClientSurfaceCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackInfo {
    pub id: String,
    pub available: bool,
    pub manifest_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    pub schema_version: String,
    pub status: String,
    pub dry_run: bool,
    pub detected_surfaces: Vec<String>,
    pub mcp_surface: McpToolSurface,
    pub writes: Vec<InstallWrite>,
    pub rollback: RollbackInfo,
    pub global_writes_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedInstall {
    pub schema_version: String,
    pub status: String,
    pub dry_run: bool,
    pub written: Vec<String>,
    pub rollback: RollbackInfo,
    pub verification: Vec<VerificationRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationRow {
    pub path: String,
    pub observed_sha256: String,
    pub byte_count: usize,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RollbackManifest {
    schema_version: String,
    id: String,
    created_unix: u64,
    entries: Vec<RollbackEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RollbackEntry {
    path: String,
    existed: bool,
    previous_content: Option<String>,
    #[serde(default)]
    previous_bytes: Option<Vec<u8>>,
    previous_sha256: Option<String>,
}

enum PendingContent {
    Text(String),
    Bytes(Vec<u8>),
}

impl PendingContent {
    fn as_bytes(&self) -> &[u8] {
        match self {
            PendingContent::Text(text) => text.as_bytes(),
            PendingContent::Bytes(bytes) => bytes,
        }
    }

    fn as_text(&self) -> Option<&str> {
        match self {
            PendingContent::Text(text) => Some(text),
            PendingContent::Bytes(_) => None,
        }
    }
}

pub fn plan(root: &Path, global: bool, capabilities: &[String]) -> InstallPlan {
    plan_for_agents(root, global, capabilities, &[], McpToolSurface::Classic)
}

pub fn plan_for_agents(
    root: &Path,
    global: bool,
    capabilities: &[String],
    agents: &[String],
    mcp_surface: McpToolSurface,
) -> InstallPlan {
    let rollback_id = rollback_id();
    let rollback_path = root
        .join(".tokenzero/install")
        .join(format!("{rollback_id}.json"));
    let selected = if capabilities.is_empty() {
        vec![
            "mcp".to_string(),
            "instructions".to_string(),
            "shell".to_string(),
        ]
    } else {
        capabilities.to_vec()
    };
    let mut writes = Vec::new();
    for capability in &selected {
        match capability.as_str() {
            "mcp" => push_mcp_writes(&mut writes, root, global, &rollback_id, agents),
            "instructions" => push_write(
                &mut writes,
                root.join("AGENTS.md"),
                "patch",
                capability,
                &rollback_id,
                global,
            ),
            "shell" => push_write(
                &mut writes,
                shell_launcher_path(root, global),
                "write",
                capability,
                &rollback_id,
                global,
            ),
            "runtime" => push_write(
                &mut writes,
                root.join(".tokenzero/install/runtime.json"),
                "write",
                capability,
                &rollback_id,
                global,
            ),
            "cli" => push_cli_writes(&mut writes, root, global, &rollback_id),
            "hooks" => push_hooks_writes(&mut writes, root, global, &rollback_id, agents),
            "shim" => push_shim_writes(&mut writes, root, global, &rollback_id),
            _ => {}
        }
    }
    InstallPlan {
        schema_version: INSTALL_SCHEMA_VERSION.to_string(),
        status: "planned".to_string(),
        dry_run: true,
        detected_surfaces: selected,
        mcp_surface,
        writes,
        rollback: RollbackInfo {
            id: rollback_id,
            available: true,
            manifest_path: rollback_path.display().to_string(),
        },
        global_writes_allowed: global,
    }
}

fn push_mcp_writes(
    writes: &mut Vec<InstallWrite>,
    root: &Path,
    global: bool,
    rollback_id: &str,
    agents: &[String],
) {
    if !global {
        push_write(
            writes,
            root.join(".tokenzero/mcp-server.json"),
            "merge",
            "mcp",
            rollback_id,
            global,
        );
        return;
    }

    push_cli_writes(writes, root, global, rollback_id);
    for path in global_json_mcp_paths_for_agents(root, agents) {
        push_write(writes, path, "merge", "mcp", rollback_id, global);
    }
    for path in global_toml_mcp_paths_for_agents(root, agents) {
        push_write(writes, path, "merge", "mcp", rollback_id, global);
    }
}

fn push_cli_writes(writes: &mut Vec<InstallWrite>, root: &Path, global: bool, rollback_id: &str) {
    if global {
        push_write(
            writes,
            installed_runtime_binary_path(root),
            "copy",
            "cli-runtime",
            rollback_id,
            global,
        );
    }
    push_write(
        writes,
        tokenzero_launcher_path(root, global),
        "write",
        "cli",
        rollback_id,
        global,
    );
    #[cfg(windows)]
    if global {
        push_write(
            writes,
            root.join(".tokenzero").join("bin").join("tokenzero"),
            "write",
            "cli-shim",
            rollback_id,
            global,
        );
    }
    push_write(
        writes,
        root.join(".tokenzero/install/runtime.json"),
        "write",
        "runtime",
        rollback_id,
        global,
    );
    #[cfg(windows)]
    if global && is_real_windows_user_root(root) {
        push_write(
            writes,
            PathBuf::from(WINDOWS_USER_PATH_REGISTRY),
            "prepend",
            "path",
            rollback_id,
            global,
        );
    }
}

fn global_json_mcp_paths_for_agents(root: &Path, agents: &[String]) -> Vec<PathBuf> {
    if !agents.is_empty() {
        let mut paths = Vec::new();
        for agent in agents {
            paths.push(root.join(format!(".config/tokenzero/agents/{agent}.mcp.json")));
            match agent.as_str() {
                "claude" => {
                    paths.push(root.join(".claude.json"));
                    paths.push(root.join(".claude/mcp.json"));
                    if cfg!(windows) {
                        paths.push(root.join("AppData/Roaming/Claude/claude_desktop_config.json"));
                    } else {
                        paths.push(
                            root.join(
                                "Library/Application Support/Claude/claude_desktop_config.json",
                            ),
                        );
                    }
                }
                "cursor" => {
                    paths.push(root.join(".cursor/mcp.json"));
                    if cfg!(windows) {
                        paths.push(root.join("AppData/Roaming/Cursor/User/mcp.json"));
                    } else {
                        paths.push(root.join("Library/Application Support/Cursor/User/mcp.json"));
                    }
                }
                "factory" => paths.push(root.join(".factory/mcp.json")),
                "gemini" => paths.push(root.join(".gemini/settings.json")),
                "opencode" => paths.push(root.join(".config/opencode/mcp.json")),
                "codex" | "droid" | "grok" => {}
                _ => {}
            }
        }
        return paths;
    }

    let mut paths = vec![
        root.join(".tokenzero/mcp.json"),
        root.join(".tokenzero/mcp-server.json"),
        root.join(".config/tokenzero/mcp-server.json"),
        root.join(".mcp.json"),
        root.join(".claude.json"),
        root.join(".claude/mcp.json"),
        root.join(".cursor/mcp.json"),
        root.join(".gemini/settings.json"),
        root.join(".config/opencode/mcp.json"),
        root.join(".factory/mcp.json"),
    ];
    if cfg!(windows) {
        paths.push(root.join("AppData/Roaming/Claude/claude_desktop_config.json"));
        paths.push(root.join("AppData/Roaming/Cursor/User/mcp.json"));
    } else {
        paths.push(root.join("Library/Application Support/Claude/claude_desktop_config.json"));
        paths.push(root.join("Library/Application Support/Cursor/User/mcp.json"));
    }
    for agent in [
        "claude", "codex", "cursor", "droid", "factory", "gemini", "grok", "opencode",
    ] {
        paths.push(root.join(format!(".config/tokenzero/agents/{agent}.mcp.json")));
    }
    paths
}

fn global_toml_mcp_paths_for_agents(root: &Path, agents: &[String]) -> Vec<PathBuf> {
    if agents.is_empty() {
        return vec![
            root.join(".codex/config.toml"),
            root.join(".grok/config.toml"),
        ];
    }
    let mut paths = Vec::new();
    for agent in agents {
        match agent.as_str() {
            "codex" => paths.push(root.join(".codex/config.toml")),
            "grok" => paths.push(root.join(".grok/config.toml")),
            _ => {}
        }
    }
    paths
}

fn push_hooks_writes(
    writes: &mut Vec<InstallWrite>,
    root: &Path,
    global: bool,
    rollback_id: &str,
    agents: &[String],
) {
    // Agent-scoped exactly like the native claude MCP paths: a plan targeting
    // only other agents (e.g. grok) must never list a .claude path.
    if !agents.is_empty() && !agents.iter().any(|agent| agent == "claude") {
        return;
    }
    push_write(
        writes,
        root.join(".claude/settings.json"),
        "merge",
        "hooks",
        rollback_id,
        global,
    );
}

/// Read-heavy tools agents reach for via bare shell; each gets a PATH shim.
const SHIM_TOOLS: &[&str] = &[
    "cat", "head", "tail", "grep", "rg", "find", "ls", "tree", "wc",
];

fn push_shim_writes(writes: &mut Vec<InstallWrite>, root: &Path, global: bool, rollback_id: &str) {
    let shim_dir = shims_dir(root);
    for tool in SHIM_TOOLS {
        // The plan is per-machine: a tool absent from PATH at install time
        // (e.g. rg not installed) gets no shim rather than a broken wrapper.
        if resolve_real_tool(tool, &shim_dir).is_none() {
            continue;
        }
        push_write(
            writes,
            shim_dir.join(tool),
            "write",
            "shim",
            rollback_id,
            global,
        );
    }
}

fn shims_dir(root: &Path) -> PathBuf {
    root.join(".tokenzero").join("shims")
}

fn resolve_real_tool(tool: &str, shim_dir: &Path) -> Option<PathBuf> {
    resolve_real_tool_in(tool, shim_dir, std::env::var_os("PATH")?.as_os_str())
}

/// First executable named `tool` on `path_var`, skipping the shim directory
/// itself and any other generated tokenzero shim, so the baked-in REAL path
/// can never point back into the wrapper layer.
fn resolve_real_tool_in(
    tool: &str,
    shim_dir: &Path,
    path_var: &std::ffi::OsStr,
) -> Option<PathBuf> {
    for dir in std::env::split_paths(path_var) {
        if dir.as_os_str().is_empty() || same_path(&dir, shim_dir) || is_shim_layer_dir(&dir) {
            continue;
        }
        let candidate = dir.join(tool);
        if is_executable_file(&candidate) && !is_generated_shim(&candidate) {
            return Some(candidate);
        }
        if cfg!(windows) {
            let exe = dir.join(format!("{tool}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

fn same_path(a: &Path, b: &Path) -> bool {
    let a = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let b = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    a == b
}

fn is_shim_layer_dir(dir: &Path) -> bool {
    dir.ends_with(".tokenzero/shims")
}

fn is_generated_shim(path: &Path) -> bool {
    let mut prefix = [0u8; 64];
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let read = std::io::Read::read(&mut file, &mut prefix).unwrap_or(0);
    String::from_utf8_lossy(&prefix[..read]).contains("tokenzero shim")
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn push_write(
    writes: &mut Vec<InstallWrite>,
    path: PathBuf,
    action: &str,
    capability: &str,
    rollback_id: &str,
    global: bool,
) {
    let path_string = path.display().to_string();
    if writes.iter().any(|existing| existing.path == path_string) {
        return;
    }
    writes.push(InstallWrite {
        path: path_string,
        action: action.to_string(),
        backup_id: rollback_id.to_string(),
        capability: capability.to_string(),
        global,
    });
}

pub fn apply(
    root: &Path,
    global: bool,
    capabilities: &[String],
) -> std::io::Result<AppliedInstall> {
    apply_for_agents(root, global, capabilities, &[], McpToolSurface::Classic)
}

pub fn apply_for_agents(
    root: &Path,
    global: bool,
    capabilities: &[String],
    agents: &[String],
    mcp_surface: McpToolSurface,
) -> std::io::Result<AppliedInstall> {
    let mut plan = plan_for_agents(root, global, capabilities, agents, mcp_surface);
    plan.dry_run = false;
    let mut manifest = RollbackManifest {
        schema_version: "tokenzero.rollback.v1".to_string(),
        id: plan.rollback.id.clone(),
        created_unix: now_unix(),
        entries: Vec::new(),
    };
    // Phase 1: snapshot each target's existing content and compute its new
    // content BEFORE mutating anything, recording the rollback manifest from that
    // snapshot. The manifest is then persisted FIRST (below), so a crash partway
    // through Phase 2 still leaves a complete, usable rollback record.
    let mut pending: Vec<(PathBuf, PendingContent, InstallWrite)> = Vec::new();
    for row in &plan.writes {
        let path = PathBuf::from(&row.path);
        if row.global && !global {
            continue;
        }
        let path_write = is_windows_user_path_write(row);
        let binary_write = row.action == "copy" && row.capability == "cli-runtime";
        let previous = if path_write {
            windows_user_path()?
        } else if binary_write {
            None
        } else {
            fs::read_to_string(&path).ok()
        };
        let content = if path_write {
            PendingContent::Text(windows_path_with_tokenzero_bin(root, previous.as_deref()))
        } else {
            content_for(row, root, previous.as_deref(), plan.mcp_surface)?
        };
        let existing_bytes = if !path_write && binary_write {
            fs::read(&path).ok()
        } else {
            None
        };
        let previous_bytes = existing_bytes
            .as_ref()
            .filter(|bytes| bytes.as_slice() != content.as_bytes())
            .cloned();
        let previous_sha256 = previous_bytes
            .as_deref()
            .map(sha256_bytes)
            .or_else(|| existing_bytes.as_deref().map(sha256_bytes))
            .or_else(|| previous.as_deref().map(sha256));
        manifest.entries.push(RollbackEntry {
            path: row.path.clone(),
            existed: if path_write {
                previous.is_some()
            } else {
                path.exists()
            },
            previous_sha256,
            previous_content: previous.clone(),
            previous_bytes,
        });
        pending.push((path, content, row.clone()));
    }

    let manifest_path = PathBuf::from(&plan.rollback.manifest_path);
    atomic_write(
        &manifest_path,
        (serde_json::to_string_pretty(&manifest)? + "\n").as_bytes(),
    )?;

    // Phase 2: apply each write atomically (temp in the same dir -> fsync ->
    // rename). A user's existing editor/agent config is never observed truncated
    // or partial: readers see either the prior content or the fully written new
    // content, even if the process is killed or the disk fills mid-write.
    let mut written = Vec::new();
    let mut verification = Vec::new();
    for (path, content, row) in &pending {
        let observed = if is_windows_user_path_write(row) {
            let content = content.as_text().ok_or_else(|| {
                Error::new(ErrorKind::InvalidData, "Windows Path write must be text")
            })?;
            write_windows_user_path(content)?;
            windows_user_path()?.unwrap_or_default().into_bytes()
        } else {
            let bytes = content.as_bytes();
            if row.action == "copy" {
                let existing = fs::read(path).ok();
                if existing.as_deref() != Some(bytes) {
                    atomic_write(path, bytes)?;
                }
            } else {
                atomic_write(path, bytes)?;
            }
            make_executable_if_needed(row, path)?;
            fs::read(path)?
        };
        verification.push(VerificationRow {
            path: row.path.clone(),
            observed_sha256: sha256_bytes(&observed),
            byte_count: observed.len(),
            verified: true,
        });
        written.push(row.path.clone());
    }
    written.push(manifest_path.display().to_string());
    Ok(AppliedInstall {
        schema_version: INSTALL_SCHEMA_VERSION.to_string(),
        status: "ok".to_string(),
        dry_run: false,
        written,
        rollback: plan.rollback,
        verification,
    })
}

pub fn rollback(root: &Path, rollback_id: &str) -> std::io::Result<serde_json::Value> {
    let manifest_path = if rollback_id == "latest" {
        latest_manifest(root).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no rollback manifest")
        })?
    } else {
        root.join(".tokenzero/install")
            .join(format!("{rollback_id}.json"))
    };
    let manifest: RollbackManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    let mut restored = Vec::new();
    let mut removed = Vec::new();
    for entry in manifest.entries {
        if is_windows_user_path_entry(&entry.path) {
            if entry.existed {
                write_windows_user_path(entry.previous_content.as_deref().unwrap_or_default())?;
                restored.push(entry.path);
            } else {
                delete_windows_user_path()?;
                removed.push(entry.path);
            }
            continue;
        }

        let path = PathBuf::from(&entry.path);
        if entry.existed {
            if let Some(bytes) = entry.previous_bytes {
                atomic_write(&path, &bytes)?;
                set_executable(&path)?;
                restored.push(entry.path);
            } else if let Some(content) = entry.previous_content {
                atomic_write(&path, content.as_bytes())?;
                restored.push(entry.path);
            }
        } else if path.exists() {
            fs::remove_file(&path)?;
            removed.push(entry.path);
        }
    }
    Ok(serde_json::json!({
        "schema_version": "tokenzero.rollback.v1",
        "status": "ok",
        "rollback_id": manifest.id,
        "restored": restored,
        "removed": removed,
        "manifest_path": manifest_path.display().to_string(),
    }))
}

fn tokenzero_command(root: &Path, global: bool) -> String {
    tokenzero_launcher_path(root, global).display().to_string()
}

fn tokenzero_launcher_path(root: &Path, global: bool) -> PathBuf {
    if !global {
        return PathBuf::from(current_exe_string());
    }
    let filename = if cfg!(windows) {
        "tokenzero.cmd"
    } else {
        "tokenzero"
    };
    root.join(".tokenzero").join("bin").join(filename)
}

fn shell_launcher_path(root: &Path, _global: bool) -> PathBuf {
    let filename = if cfg!(windows) {
        "tokenzero-shell.cmd"
    } else {
        "tokenzero-shell"
    };
    root.join(".tokenzero").join("bin").join(filename)
}

fn mcp_command(root: &Path, global: bool) -> String {
    if global {
        runtime_manifest_binary(root, true)
    } else {
        tokenzero_command(root, false)
    }
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(".tokenzero/recovery-cache.json")
}

fn current_exe_string() -> String {
    std::env::current_exe()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "tokenzero".to_string())
}

fn current_exe_bytes() -> std::io::Result<Vec<u8>> {
    fs::read(std::env::current_exe()?)
}

fn installed_runtime_binary_path(root: &Path) -> PathBuf {
    root.join(".tokenzero")
        .join("bin")
        .join(installed_runtime_binary_name())
}

fn installed_runtime_binary_name() -> String {
    let hash = current_exe_hash_prefix().unwrap_or_else(|| "current".to_string());
    if cfg!(windows) {
        format!("tokenzero-runtime-{hash}.exe")
    } else {
        format!("tokenzero-runtime-{hash}")
    }
}

fn current_exe_hash_prefix() -> Option<String> {
    let bytes = current_exe_bytes().ok()?;
    let hash = sha256_bytes(&bytes);
    Some(hash.chars().take(16).collect())
}

fn runtime_manifest_binary(root: &Path, global: bool) -> String {
    if global {
        installed_runtime_binary_path(root).display().to_string()
    } else {
        current_exe_string()
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn cli_launcher_content(root: &Path, global: bool) -> String {
    let target = runtime_manifest_binary(root, global);
    if cfg!(windows) {
        format!("@echo off\r\n\"{}\" %*\r\nexit /b %ERRORLEVEL%\r\n", target)
    } else {
        format!("#!/bin/sh\nexec {} \"$@\"\n", shell_quote(&target))
    }
}

fn windows_posix_cli_shim_content() -> String {
    "#!/bin/sh\nDIR=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nexec \"$DIR/tokenzero.cmd\" \"$@\"\n"
        .to_string()
}

fn shell_launcher_content(root: &Path, global: bool) -> String {
    if cfg!(windows) {
        format!(
            "@echo off\r\ncall \"{}\" run -- %*\r\nexit /b %ERRORLEVEL%\r\n",
            tokenzero_command(root, global)
        )
    } else {
        format!(
            "#!/bin/sh\nexec {} run -- \"$@\"\n",
            shell_quote(&tokenzero_command(root, global))
        )
    }
}

/// POSIX shim script: inert unless `TOKENZERO_SHIM=1`, and never recursive —
/// `TOKENZERO_INNER=1` prefixes the wrapped exec, so PATH lookups made by the
/// child of `tokenzero run` fall straight through every shim to the real
/// binary. The real tool path is resolved at install time with the shim
/// directory excluded from PATH and baked in.
fn shim_content(path: &Path, root: &Path, global: bool) -> std::io::Result<String> {
    let tool = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "shim path has no tool name"))?;
    let real = resolve_real_tool(tool, &shims_dir(root)).ok_or_else(|| {
        Error::new(
            ErrorKind::NotFound,
            format!("{tool} not found on PATH outside the shim directory"),
        )
    })?;
    // The wrap targets the STABLE launcher (which execs the current
    // versioned runtime) and is guarded on its executability: `exec` of a
    // missing path exits the non-interactive shell before any fallback line,
    // so a stale target would otherwise hard-fail every shimmed utility.
    Ok(format!(
        "#!/bin/sh\n\
         # tokenzero shim for {tool} — generated; real binary resolved at install time.\n\
         REAL={real}\n\
         TZ={launcher}\n\
         if [ \"$TOKENZERO_SHIM\" = \"1\" ] && [ -z \"$TOKENZERO_INNER\" ] && [ -x \"$TZ\" ]; then\n\
         \x20 TOKENZERO_INNER=1 exec \"$TZ\" run -- \"$REAL\" \"$@\"\n\
         fi\n\
         exec \"$REAL\" \"$@\"\n",
        real = shell_quote(&real.display().to_string()),
        launcher = shell_quote(&tokenzero_command(root, global)),
    ))
}

fn path_within_root(root: &Path, path: &Path) -> std::io::Result<bool> {
    let root = root.canonicalize()?;
    let path = canonicalize_existing_or_parent(path)?;
    Ok(path.starts_with(root))
}

fn canonicalize_existing_or_parent(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        return path.canonicalize();
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "path has no parent"))?
        .canonicalize()?;
    let name = path
        .file_name()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "path has no file name"))?;
    Ok(parent.join(name))
}

fn make_executable_if_needed(row: &InstallWrite, path: &Path) -> std::io::Result<()> {
    if row.capability != "cli"
        && row.capability != "cli-shim"
        && row.capability != "cli-runtime"
        && row.capability != "shell"
        && row.capability != "shim"
    {
        return Ok(());
    }
    set_executable(path)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Tighten the nearest enclosing `.tokenzero` directory to owner-only on
/// unix. TokenZero-owned state (cache, manifests, shims, installed binaries)
/// is private to the user; agent config directories outside `.tokenzero`
/// keep their platform defaults. Best-effort: a chmod failure never blocks
/// the write itself.
fn restrict_tokenzero_dir(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut cursor = Some(path);
        while let Some(dir) = cursor {
            if dir.file_name().is_some_and(|name| name == ".tokenzero") {
                let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
                break;
            }
            cursor = dir.parent();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Write `content` to `path` atomically: stage it in a sibling temp file in the
/// same directory, fsync, then rename over the target. A crash or disk-full
/// leaves the original file intact (or absent) - never a truncated/partial
/// config. Mirrors the recovery crate's atomic_write_json; the temp is removed
/// if the rename fails so no `.tmp` debris is left behind.
fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)?;
        restrict_tokenzero_dir(parent);
    }
    let dir = parent.unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("tokenzero"),
        std::process::id()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)?;
    file.write_all(content)?;
    file.sync_all()?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            Err(err)
        }
    }
}

fn latest_manifest(root: &Path) -> Option<PathBuf> {
    let dir = root.join(".tokenzero/install");
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|v| v.to_str()) == Some("json")
                && path
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .is_some_and(|stem| stem.starts_with("rollback-"))
        })
        .max()
}

fn rollback_id() -> String {
    format!("rollback-{}", now_unix())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn sha256(text: &str) -> String {
    sha256_bytes(text.as_bytes())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

mod content;
mod doctor;
mod inspect;
mod windows_path;

pub(crate) use content::*;
pub(crate) use windows_path::*;

pub use content::{DetectedAgent, detect_present_agents};
pub use doctor::{
    doctor, doctor_capabilities, doctor_exit_codes, doctor_explain, doctor_fix, doctor_ls,
    doctor_robot_docs, doctor_robot_triage, doctor_undo,
};
pub use inspect::inspect_client_surface;

#[cfg(test)]
mod tests;
