#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewriteResult {
    pub schema_version: String,
    pub status: String,
    pub command: String,
    pub rewritten_command: String,
    pub applied: bool,
    pub reason: String,
    pub family: String,
    /// `true` only when TokenZero affirmatively vouches the command has no
    /// destructive or mutating semantics. `false` means "not verified", not
    /// "known dangerous": unknown families, compound commands, and anything
    /// matching `unsafe_reason` are never vouched. This field never gates
    /// execution — it routes and informs.
    pub safe: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterInfo {
    pub family: String,
    pub commands: Vec<String>,
    pub supported: bool,
    pub exact_refs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverReport {
    pub schema_version: String,
    pub status: String,
    pub supported_filters: Vec<FilterInfo>,
    pub unsupported_commands: Vec<String>,
    pub install_ready: bool,
    pub mcp_ready: bool,
    pub shell_ready: bool,
    pub os_warnings: Vec<String>,
}

pub fn supported_filters() -> Vec<FilterInfo> {
    vec![
        info("read", ["cat", "head", "tail", "wc"]),
        info("search", ["rg", "grep", "findstr"]),
        info("tree", ["find", "ls", "tree"]),
        info("git", ["git status", "git diff", "git log"]),
        info(
            "test",
            [
                "pytest",
                "cargo test",
                "go test",
                "npm test",
                "pnpm test",
                "yarn test",
                "jest",
                "vitest",
            ],
        ),
        info(
            "build",
            [
                "cargo build",
                "npm run build",
                "pnpm build",
                "yarn build",
                "tsc",
                "eslint",
                "ruff",
                "mypy",
                "clippy",
            ],
        ),
        info("docker", ["docker ps", "docker logs", "docker compose"]),
        info(
            "kubectl",
            ["kubectl get", "kubectl logs", "kubectl describe"],
        ),
        info("package", ["cargo", "npm", "pnpm", "yarn", "uv"]),
        info("config", ["json", "yaml", "toml", "logs"]),
    ]
}

fn info<const N: usize>(family: &str, commands: [&str; N]) -> FilterInfo {
    FilterInfo {
        family: family.to_string(),
        commands: commands.iter().map(|v| v.to_string()).collect(),
        supported: true,
        exact_refs: true,
    }
}

pub fn discover() -> DiscoverReport {
    DiscoverReport {
        schema_version: "tokenzero.discover.v1".to_string(),
        status: "ok".to_string(),
        supported_filters: supported_filters(),
        unsupported_commands: Vec::new(),
        install_ready: true,
        mcp_ready: true,
        shell_ready: true,
        os_warnings: os_warnings(),
    }
}

pub fn os_warnings() -> Vec<String> {
    let mut warnings = Vec::new();
    if cfg!(windows) {
        warnings
            .push("verify PowerShell and cmd quoting with the OS matrix before launch".to_string());
    }
    warnings
}

pub fn classify_command(command: &str) -> String {
    let parts = split_words(command);
    let first = parts.first().map(String::as_str).unwrap_or_default();
    let second = parts.get(1).map(String::as_str).unwrap_or_default();
    match first {
        "cat" | "head" | "tail" | "wc" => "read",
        "rg" | "grep" | "findstr" => "search",
        "find" | "ls" | "tree" => "tree",
        "git" => "git",
        "pytest" | "unittest" | "jest" | "vitest" => "test",
        "cargo" if second == "test" => "test",
        "go" if second == "test" => "test",
        "npm" | "pnpm" | "yarn" if second == "test" => "test",
        "cargo" if second == "build" => "build",
        "npm" | "pnpm" | "yarn" if second == "run" || second == "build" => "build",
        "tsc" | "eslint" | "ruff" | "mypy" | "clippy" => "build",
        "docker" => "docker",
        "kubectl" => "kubectl",
        "cargo" | "npm" | "pnpm" | "yarn" | "uv" => "package",
        _ => "unknown",
    }
    .to_string()
}

pub fn rewrite_command(command: &str, mode: &str, enabled: bool) -> RewriteResult {
    let family = classify_command(command);
    if !enabled || mode == "off" {
        // Disabled still reports an honest safety verdict: run the normal
        // analysis and discard the rewrite.
        let probe = rewrite_command(command, "safe", true);
        return result(command, command, false, "disabled", &family, probe.safe);
    }
    if let Some(reason) = unsafe_reason(command) {
        return result(command, command, false, &reason, &family, false);
    }
    // Family rewrites only understand a single simple command; rewriting one
    // segment of a pipeline/sequence produces a broken command (e.g.
    // `cat a | grep b` must not become `tokenzero read a '|' grep b`).
    // Compounds are never vouched: any segment could mutate.
    if has_shell_operators(command) {
        return result(
            command,
            command,
            false,
            "compound command left unmodified",
            &family,
            false,
        );
    }
    let rewritten = match family.as_str() {
        "read" => rewrite_read(command),
        "search" => rewrite_search(command),
        "tree" => rewrite_tree(command),
        "git" => rewrite_git(command),
        "test" | "build" | "package" => {
            Some(inject_quiet_flag(command).unwrap_or_else(|| command.to_string()))
        }
        "docker" | "kubectl" => Some(command.to_string()),
        _ => None,
    };
    finish_rewrite(command, &family, rewritten)
}

fn finish_rewrite(command: &str, family: &str, rewritten: Option<String>) -> RewriteResult {
    match rewritten {
        Some(value) if value != command => result(
            command,
            &value,
            true,
            "bounded tokenzero-safe rewrite",
            family,
            true,
        ),
        Some(_) => result(
            command,
            command,
            false,
            "already bounded or passthrough",
            family,
            true,
        ),
        None => result(
            command,
            command,
            false,
            "unsupported command family",
            family,
            false,
        ),
    }
}

fn result(
    command: &str,
    rewritten: &str,
    applied: bool,
    reason: &str,
    family: &str,
    safe: bool,
) -> RewriteResult {
    RewriteResult {
        schema_version: "tokenzero.rewrite.v1".to_string(),
        status: "ok".to_string(),
        command: command.to_string(),
        rewritten_command: rewritten.to_string(),
        applied,
        reason: reason.to_string(),
        family: family.to_string(),
        safe,
    }
}

fn rewrite_read(command: &str) -> Option<String> {
    let parts = split_words(command);
    match parts.first().map(String::as_str) {
        Some("cat") if parts.len() >= 2 => {
            Some(format!("tokenzero read {}", shell_join(&parts[1..])))
        }
        Some("head") | Some("tail") => Some(command.to_string()),
        _ => None,
    }
}

fn rewrite_search(command: &str) -> Option<String> {
    let parts = split_words(command);
    match parts.first().map(String::as_str) {
        Some("rg") | Some("grep") if parts.len() >= 2 => Some(command.to_string()),
        _ => None,
    }
}

fn rewrite_tree(command: &str) -> Option<String> {
    let parts = split_words(command);
    match parts.first().map(String::as_str) {
        Some("tree") => {
            if parts.iter().any(|p| is_tree_depth_flag(p)) {
                Some(command.to_string())
            } else {
                Some(format!("{command} -L 2"))
            }
        }
        Some("ls") if !parts.iter().any(|p| p.contains('R')) => Some(command.to_string()),
        Some("find") => Some(command.to_string()),
        _ => None,
    }
}

fn rewrite_git(command: &str) -> Option<String> {
    let parts = split_words(command);
    if parts.first().map(String::as_str) != Some("git") {
        return None;
    }
    match parts.get(1).map(String::as_str) {
        Some("log") => {
            if parts.iter().any(|p| is_git_log_count_flag(p)) {
                Some(command.to_string())
            } else {
                Some(format!("{command} -n 80"))
            }
        }
        Some("clone" | "fetch" | "pull") => {
            Some(inject_quiet_flag(command).unwrap_or_else(|| command.to_string()))
        }
        Some("status" | "diff" | "show") => Some(command.to_string()),
        _ => None,
    }
}

/// Verbosity tokens that mean the caller already chose an output level; the
/// quiet injector never overrides an explicit choice.
fn is_tree_depth_flag(part: &str) -> bool {
    part == "-L"
        || part.starts_with("--depth")
        || part
            .strip_prefix("-L")
            .is_some_and(|value| !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit()))
}

fn is_git_log_count_flag(part: &str) -> bool {
    part == "--max-count"
        || part.starts_with("--max-count=")
        || part
            .strip_prefix("-n")
            .is_some_and(|value| value.is_empty() || value.chars().all(|ch| ch.is_ascii_digit()))
}

fn has_explicit_verbosity(parts: &[String]) -> bool {
    parts.iter().any(|p| {
        matches!(
            p.as_str(),
            "-q" | "--quiet"
                | "-v"
                | "-vv"
                | "-vvv"
                | "--verbose"
                | "-s"
                | "--silent"
                | "--progress"
                | "--no-progress"
        ) || p.starts_with("--loglevel")
            || p.starts_with("--verbosity")
    })
}

/// Append a success-safe quiet flag for known-noisy toolchains. Quiet flags
/// only suppress bookkeeping chrome (Compiling/progress/lifecycle banners);
/// errors and warnings still print, and exact refs capture whatever remains.
/// Commands carrying a `--` passthrough separator are left alone because a
/// trailing flag would bind to the inner tool instead.
fn inject_quiet_flag(command: &str) -> Option<String> {
    let parts = split_words(command);
    if has_explicit_verbosity(&parts) || parts.iter().any(|p| p == "--") {
        return None;
    }
    let first = parts.first().map(String::as_str).unwrap_or_default();
    let second = parts.get(1).map(String::as_str).unwrap_or_default();
    match first {
        "cargo"
            if matches!(
                second,
                "build" | "check" | "clippy" | "test" | "bench" | "doc" | "fetch" | "run"
            ) =>
        {
            Some(format!("{command} -q"))
        }
        "git" if matches!(second, "clone" | "fetch" | "pull") => Some(format!("{command} --quiet")),
        "npm" if matches!(second, "test" | "run" | "build" | "rebuild") => {
            Some(format!("{command} --silent"))
        }
        _ => None,
    }
}

fn has_shell_operators(command: &str) -> bool {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        // POSIX shells escape the next character with a backslash outside
        // single quotes; Windows shells treat backslash as a path separator,
        // so the escape rule must match the shell that will execute.
        if ch == '\\' && quote != Some('\'') && !cfg!(windows) {
            escaped = true;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            continue;
        }
        match quote {
            None => {
                if ch == '\'' || ch == '"' {
                    quote = Some(ch);
                    continue;
                }
                if matches!(ch, '\n' | '\r' | '|' | ';' | '&' | '>' | '<' | '`') {
                    return true;
                }
                if ch == '$' && chars.peek() == Some(&'(') {
                    return true;
                }
            }
            // Command substitution still runs inside double quotes.
            Some('"') if ch == '`' || (ch == '$' && chars.peek() == Some(&'(')) => {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn unsafe_reason(command: &str) -> Option<String> {
    let parts = split_words(command);
    let first = parts.first().map(String::as_str).unwrap_or_default();
    let second = parts.get(1).map(String::as_str).unwrap_or_default();
    if is_destructive_first(first) {
        return Some("unsafe destructive mutation left unmodified".to_string());
    }
    if is_command_dispatcher(first) {
        return Some(
            "command dispatcher left unmodified; safety depends on the dispatched command"
                .to_string(),
        );
    }
    if ["ssh", "scp", "sftp"].contains(&first) {
        return Some("remote execution left unmodified".to_string());
    }
    if matches!(first, "sed" | "awk" | "gawk")
        && parts
            .iter()
            .skip(1)
            .any(|p| p.starts_with("-i") || p == "--in-place" || p == "inplace")
    {
        return Some("in-place file edit left unmodified".to_string());
    }
    if first == "perl"
        && parts
            .iter()
            .skip(1)
            .any(|p| p.starts_with('-') && !p.starts_with("--") && p.contains('i'))
    {
        return Some("in-place file edit left unmodified".to_string());
    }
    if first == "find"
        && parts.iter().any(|p| {
            matches!(
                p.as_str(),
                "-delete" | "-exec" | "-execdir" | "-ok" | "-okdir"
            )
        })
    {
        return Some("find with side effects left unmodified".to_string());
    }
    if is_git_mutation(first, second) {
        return Some("git mutation left unmodified".to_string());
    }
    if is_docker_mutation(first, second, &parts) {
        return Some("docker mutation left unmodified".to_string());
    }
    if is_kubectl_mutation(first, second) {
        return Some("kubectl mutation left unmodified".to_string());
    }
    if is_package_mutation(first, second) {
        return Some("package/network mutation left unmodified".to_string());
    }
    if ["curl", "wget"].contains(&first) {
        return Some("network command left unmodified".to_string());
    }
    None
}

fn is_destructive_first(first: &str) -> bool {
    matches!(
        first,
        "rm" | "rmdir"
            | "unlink"
            | "mv"
            | "cp"
            | "chmod"
            | "chown"
            | "dd"
            | "shutdown"
            | "reboot"
            | "shred"
            | "truncate"
            | "wipefs"
            | "parted"
            | "fdisk"
            | "mount"
            | "umount"
            | "ln"
            | "rsync"
            | "systemctl"
            | "service"
            | "launchctl"
            | "iptables"
            | "nft"
            | "ufw"
            | "crontab"
    ) || first.starts_with("mkfs")
}

fn is_command_dispatcher(first: &str) -> bool {
    matches!(
        first,
        "xargs"
            | "eval"
            | "exec"
            | "source"
            | "env"
            | "sudo"
            | "doas"
            | "nohup"
            | "timeout"
            | "watch"
            | "npx"
    )
}

fn is_git_mutation(first: &str, second: &str) -> bool {
    first == "git"
        && matches!(
            second,
            "push"
                | "reset"
                | "clean"
                | "checkout"
                | "switch"
                | "rebase"
                | "merge"
                | "commit"
                | "restore"
                | "rm"
                | "mv"
                | "apply"
                | "am"
                | "cherry-pick"
                | "revert"
                | "stash"
                | "tag"
                | "branch"
                | "remote"
        )
}

fn is_docker_mutation(first: &str, second: &str, parts: &[String]) -> bool {
    if first != "docker" {
        return false;
    }
    if matches!(
        second,
        "rm" | "rmi"
            | "cp"
            | "import"
            | "stop"
            | "kill"
            | "push"
            | "login"
            | "run"
            | "exec"
            | "build"
            | "prune"
            | "system"
            | "restart"
            | "update"
    ) {
        return true;
    }
    second == "compose"
        && parts
            .iter()
            .skip(2)
            .any(|part| is_docker_compose_mutation(part))
}

fn is_docker_compose_mutation(part: &str) -> bool {
    matches!(
        part,
        "up" | "down"
            | "rm"
            | "run"
            | "exec"
            | "build"
            | "pull"
            | "push"
            | "restart"
            | "start"
            | "stop"
            | "kill"
            | "create"
    )
}

fn is_kubectl_mutation(first: &str, second: &str) -> bool {
    first == "kubectl"
        && matches!(
            second,
            "delete"
                | "apply"
                | "replace"
                | "scale"
                | "patch"
                | "create"
                | "exec"
                | "edit"
                | "drain"
                | "cordon"
                | "uncordon"
                | "rollout"
                | "annotate"
                | "label"
                | "taint"
                | "cp"
        )
}

fn is_package_mutation(first: &str, second: &str) -> bool {
    matches!(
        (first, second),
        (
            "npm" | "pnpm" | "yarn",
            "install"
                | "add"
                | "publish"
                | "login"
                | "uninstall"
                | "remove"
                | "update"
                | "upgrade"
                | "link"
                | "unlink"
                | "exec"
                | "dlx"
                | "create"
                | "ci"
        ) | (
            "cargo",
            "publish" | "install" | "login" | "add" | "remove" | "update" | "yank" | "owner"
        ) | (
            "uv",
            "pip" | "add" | "remove" | "sync" | "tool" | "publish" | "venv"
        )
    )
}

fn split_words(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in command.chars() {
        if escaped {
            escaped = false;
            cur.push(ch);
            continue;
        }
        // Mirror has_shell_operators: POSIX backslash escapes outside single
        // quotes; on Windows a backslash is an ordinary path character.
        if ch == '\\' && quote != Some('\'') && !cfg!(windows) {
            escaped = true;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            continue;
        }
        if quote.is_none() && (ch == '\'' || ch == '"') {
            quote = Some(ch);
            continue;
        }
        if quote.is_none() && ch.is_whitespace() {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            continue;
        }
        cur.push(ch);
    }
    if escaped {
        // A trailing backslash escapes nothing; keep it literal.
        cur.push('\\');
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

fn shell_join(parts: &[String]) -> String {
    parts
        .iter()
        .map(|p| {
            if p.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./:@".contains(c))
            {
                p.clone()
            } else {
                format!("'{}'", p.replace('\'', "'\"'\"'"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
