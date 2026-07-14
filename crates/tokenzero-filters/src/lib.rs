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

#[derive(Clone, Copy)]
enum RewriteKind { Read, Search, Tree, Git, Quiet, Passthrough }

struct RewriteRule { families: Words, kind: RewriteKind }

// Ordered family dispatch keeps rewrite precedence explicit and data-driven.
const REWRITE_RULES: &[RewriteRule] = &[
    RewriteRule { families: &["read"], kind: RewriteKind::Read },
    RewriteRule { families: &["search"], kind: RewriteKind::Search },
    RewriteRule { families: &["tree"], kind: RewriteKind::Tree },
    RewriteRule { families: &["git"], kind: RewriteKind::Git },
    RewriteRule { families: &["test", "build", "package"], kind: RewriteKind::Quiet },
    RewriteRule { families: &["docker", "kubectl"], kind: RewriteKind::Passthrough },
];

type Words = &'static [&'static str];

const FILTER_SPECS: &[(&str, Words)] = &[
    ("read", &["cat", "head", "tail", "wc"]),
    ("search", &["rg", "grep", "findstr"]),
    ("tree", &["find", "ls", "tree"]),
    ("git", &["git status", "git diff", "git log"]),
    ("test", &["pytest", "cargo test", "go test", "npm test", "pnpm test", "yarn test", "jest", "vitest"]),
    ("build", &["cargo build", "npm run build", "pnpm build", "yarn build", "tsc", "eslint", "ruff", "mypy", "clippy"]),
    ("docker", &["docker ps", "docker logs", "docker compose"]),
    ("kubectl", &["kubectl get", "kubectl logs", "kubectl describe"]),
    ("package", &["cargo", "npm", "pnpm", "yarn", "uv"]),
    ("config", &["json", "yaml", "toml", "logs"]),
];

struct ClassRule { family: &'static str, commands: Words, subcommands: Words }

// Ordered: specific command/subcommand pairs precede broad package families.
const CLASS_RULES: &[ClassRule] = &[
    ClassRule { family: "read", commands: &["cat", "head", "tail", "wc"], subcommands: &[] },
    ClassRule { family: "search", commands: &["rg", "grep", "findstr"], subcommands: &[] },
    ClassRule { family: "tree", commands: &["find", "ls", "tree"], subcommands: &[] },
    ClassRule { family: "git", commands: &["git"], subcommands: &[] },
    ClassRule { family: "test", commands: &["pytest", "unittest", "jest", "vitest"], subcommands: &[] },
    ClassRule { family: "test", commands: &["cargo", "go", "npm", "pnpm", "yarn"], subcommands: &["test"] },
    ClassRule { family: "build", commands: &["cargo"], subcommands: &["build"] },
    ClassRule { family: "build", commands: &["npm", "pnpm", "yarn"], subcommands: &["run", "build"] },
    ClassRule { family: "build", commands: &["tsc", "eslint", "ruff", "mypy", "clippy"], subcommands: &[] },
    ClassRule { family: "docker", commands: &["docker"], subcommands: &[] },
    ClassRule { family: "kubectl", commands: &["kubectl"], subcommands: &[] },
    ClassRule { family: "package", commands: &["cargo", "npm", "pnpm", "yarn", "uv"], subcommands: &[] },
];

pub fn supported_filters() -> Vec<FilterInfo> {
    FILTER_SPECS.iter().map(|&(family, commands)| FilterInfo {
        family: family.to_string(),
        commands: commands.iter().map(|&command| command.to_string()).collect(),
        supported: true,
        exact_refs: true,
    }).collect()
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
    cfg!(windows).then(|| "verify PowerShell and cmd quoting with the OS matrix before launch".to_string())
        .into_iter().collect()
}

pub fn classify_command(command: &str) -> String {
    let parts = split_words(command);
    let first = parts.first().map(String::as_str).unwrap_or_default();
    let second = parts.get(1).map(String::as_str).unwrap_or_default();
    CLASS_RULES.iter().find(|rule| {
        rule.commands.contains(&first)
            && (rule.subcommands.is_empty() || rule.subcommands.contains(&second))
    }).map_or("unknown", |rule| rule.family).to_string()
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
    let rewritten = REWRITE_RULES.iter()
        .find(|rule| rule.families.contains(&family.as_str()))
        .and_then(|rule| apply_rewrite(rule.kind, command));
    finish_rewrite(command, &family, rewritten)
}

fn finish_rewrite(command: &str, family: &str, rewritten: Option<String>) -> RewriteResult {
    if let Some(value) = rewritten {
        let applied = value != command;
        return result(
            command,
            if applied { &value } else { command },
            applied,
            if applied { "bounded tokenzero-safe rewrite" } else { "already bounded or passthrough" },
            family,
            true,
        );
    }
    result(command, command, false, "unsupported command family", family, false)
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

fn apply_rewrite(kind: RewriteKind, command: &str) -> Option<String> {
    let parts = split_words(command);
    let first = parts.first().map(String::as_str);
    match kind {
        RewriteKind::Read => match first {
            Some("cat") if parts.len() >= 2 => Some(format!("tokenzero read {}", shell_join(&parts[1..]))),
            Some("head" | "tail") => Some(command.to_string()),
            _ => None,
        },
        RewriteKind::Search => matches!(first, Some("rg" | "grep")).then(|| command.to_string()),
        RewriteKind::Tree => match first {
            Some("tree") if !parts.iter().any(|p| is_tree_depth_flag(p)) => Some(format!("{command} -L 2")),
            Some("tree" | "find") => Some(command.to_string()),
            Some("ls") if !parts.iter().any(|p| p.contains('R')) => Some(command.to_string()),
            _ => None,
        },
        RewriteKind::Git => match parts.get(1).map(String::as_str) {
            Some("log") if !parts.iter().any(|p| is_git_log_count_flag(p)) => Some(format!("{command} -n 80")),
            Some("log" | "status" | "diff" | "show") => Some(command.to_string()),
            Some("clone" | "fetch" | "pull") => Some(inject_quiet_flag(command).unwrap_or_else(|| command.to_string())),
            _ => None,
        },
        RewriteKind::Quiet => Some(inject_quiet_flag(command).unwrap_or_else(|| command.to_string())),
        RewriteKind::Passthrough => Some(command.to_string()),
    }
}

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

const VERBOSITY_FLAGS: Words = &["-q", "--quiet", "-v", "-vv", "-vvv", "--verbose", "-s", "--silent", "--progress", "--no-progress"];
struct QuietRule { commands: Words, subcommands: Words, flag: &'static str }
const QUIET_RULES: &[QuietRule] = &[
    QuietRule { commands: &["cargo"], subcommands: &["build", "check", "clippy", "test", "bench", "doc", "fetch", "run"], flag: "-q" },
    QuietRule { commands: &["git"], subcommands: &["clone", "fetch", "pull"], flag: "--quiet" },
    QuietRule { commands: &["npm"], subcommands: &["test", "run", "build", "rebuild"], flag: "--silent" },
];

fn has_explicit_verbosity(parts: &[String]) -> bool {
    parts.iter().any(|part| VERBOSITY_FLAGS.contains(&part.as_str())
        || part.starts_with("--loglevel") || part.starts_with("--verbosity"))
}

fn inject_quiet_flag(command: &str) -> Option<String> {
    let parts = split_words(command);
    if has_explicit_verbosity(&parts) || parts.iter().any(|part| part == "--") { return None; }
    let first = parts.first().map(String::as_str).unwrap_or_default();
    let second = parts.get(1).map(String::as_str).unwrap_or_default();
    QUIET_RULES.iter().find(|rule| rule.commands.contains(&first) && rule.subcommands.contains(&second))
        .map(|rule| format!("{command} {}", rule.flag))
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
    for node in parse_shell_commands(command) {
        if let Some(reason) = unsafe_reason_for_words(&node.words) {
            return Some(reason);
        }
        for nested in node.nested_commands {
            if let Some(reason) = unsafe_reason(&nested) {
                return Some(reason);
            }
        }
        if is_shell_interpreter(&node.words) {
            if let Some(payload) = shell_command_payload(&node.words) {
                if let Some(reason) = unsafe_reason(payload) {
                    return Some(reason);
                }
            }
        }
    }
    None
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ShellCommand {
    words: Vec<String>,
    nested_commands: Vec<String>,
}

/// Parse enough POSIX shell structure to identify every executable position.
/// Words remain opaque data; only operators create new command positions, while
/// command substitutions are returned for recursive classification.
fn parse_shell_commands(command: &str) -> Vec<ShellCommand> {
    let mut commands = vec![ShellCommand::default()];
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let chars: Vec<char> = command.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            word.push(ch);
            escaped = false;
            index += 1;
            continue;
        }
        if ch == '\\' && quote != Some('\'') && !cfg!(windows) {
            escaped = true;
            index += 1;
            continue;
        }
        if quote == Some('\'') {
            if ch == '\'' {
                quote = None;
            } else {
                word.push(ch);
            }
            index += 1;
            continue;
        }
        if ch == '\'' {
            quote = Some('\'');
            index += 1;
            continue;
        }
        if ch == '"' {
            quote = if quote == Some('"') { None } else { Some('"') };
            index += 1;
            continue;
        }
        if ch == '$' && chars.get(index + 1) == Some(&'(') {
            flush_shell_word(&mut commands, &mut word);
            let (nested, next) = take_parenthesized_command(&chars, index + 2);
            commands
                .last_mut()
                .expect("parser always has a command")
                .nested_commands
                .push(nested);
            index = next;
            continue;
        }
        if ch == '`' {
            flush_shell_word(&mut commands, &mut word);
            let (nested, next) = take_backtick_command(&chars, index + 1);
            commands
                .last_mut()
                .expect("parser always has a command")
                .nested_commands
                .push(nested);
            index = next;
            continue;
        }
        if quote.is_none() && ch.is_whitespace() {
            flush_shell_word(&mut commands, &mut word);
            if matches!(ch, '\n' | '\r') {
                start_shell_command(&mut commands);
            }
            index += 1;
            continue;
        }
        if quote.is_none() && matches!(ch, ';' | '|' | '&' | '!' | '(' | ')') {
            flush_shell_word(&mut commands, &mut word);
            start_shell_command(&mut commands);
            index += 1;
            if chars.get(index) == Some(&ch) {
                index += 1;
            }
            continue;
        }
        word.push(ch);
        index += 1;
    }
    if escaped {
        word.push('\\');
    }
    flush_shell_word(&mut commands, &mut word);
    commands.retain(|node| !node.words.is_empty() || !node.nested_commands.is_empty());
    commands
}

fn flush_shell_word(commands: &mut [ShellCommand], word: &mut String) {
    if !word.is_empty() {
        commands
            .last_mut()
            .expect("parser always has a command")
            .words
            .push(std::mem::take(word));
    }
}

fn start_shell_command(commands: &mut Vec<ShellCommand>) {
    if commands
        .last()
        .is_some_and(|node| !node.words.is_empty() || !node.nested_commands.is_empty())
    {
        commands.push(ShellCommand::default());
    }
}

fn take_parenthesized_command(chars: &[char], mut index: usize) -> (String, usize) {
    let start = index;
    let mut depth = 1;
    let mut quote = None;
    let mut escaped = false;
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            escaped = false;
        } else if ch == '\\' && quote != Some('\'') {
            escaped = true;
        } else if quote == Some('\'') {
            if ch == '\'' {
                quote = None;
            }
        } else if ch == '\'' {
            quote = Some('\'');
        } else if ch == '"' {
            quote = if quote == Some('"') { None } else { Some('"') };
        } else if quote.is_none() && ch == '(' {
            depth += 1;
        } else if quote.is_none() && ch == ')' {
            depth -= 1;
            if depth == 0 {
                return (chars[start..index].iter().collect(), index + 1);
            }
        }
        index += 1;
    }
    (chars[start..].iter().collect(), chars.len())
}

fn take_backtick_command(chars: &[char], mut index: usize) -> (String, usize) {
    let start = index;
    let mut escaped = false;
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '`' {
            return (chars[start..index].iter().collect(), index + 1);
        }
        index += 1;
    }
    (chars[start..].iter().collect(), chars.len())
}

fn is_shell_interpreter(words: &[String]) -> bool {
    words.first().is_some_and(|word| {
        let executable = word.rsplit('/').next().unwrap_or(word);
        matches!(executable, "sh" | "bash" | "dash" | "zsh" | "ksh")
    })
}

fn shell_command_payload(words: &[String]) -> Option<&str> {
    words.windows(2).find_map(|pair| {
        let flag = pair[0].as_str();
        (flag == "-c"
            || (flag.starts_with('-')
                && !flag.starts_with("--")
                && flag[1..].chars().any(|ch| ch == 'c')))
        .then_some(pair[1].as_str())
    })
}

#[derive(Clone, Copy)]
enum SafetyKind { Destructive, Dispatcher, Remote, InPlace, Find, Git, Docker, Kubectl, Package, Network }

struct SafetyRule { kind: SafetyKind, reason: &'static str }

// Ordered from broad process/filesystem hazards through family-specific network hazards.
const SAFETY_RULES: &[SafetyRule] = &[
    SafetyRule { kind: SafetyKind::Destructive, reason: "unsafe destructive mutation left unmodified" },
    SafetyRule { kind: SafetyKind::Dispatcher, reason: "command dispatcher left unmodified; safety depends on the dispatched command" },
    SafetyRule { kind: SafetyKind::Remote, reason: "remote execution left unmodified" },
    SafetyRule { kind: SafetyKind::InPlace, reason: "in-place file edit left unmodified" },
    SafetyRule { kind: SafetyKind::Find, reason: "find with side effects left unmodified" },
    SafetyRule { kind: SafetyKind::Git, reason: "git mutation left unmodified" },
    SafetyRule { kind: SafetyKind::Docker, reason: "docker mutation left unmodified" },
    SafetyRule { kind: SafetyKind::Kubectl, reason: "kubectl mutation left unmodified" },
    SafetyRule { kind: SafetyKind::Package, reason: "package/network mutation left unmodified" },
    SafetyRule { kind: SafetyKind::Network, reason: "network command left unmodified" },
];

const DESTRUCTIVE: Words = &["rm", "rmdir", "unlink", "mv", "cp", "chmod", "chown", "dd", "shutdown", "reboot", "shred", "truncate", "wipefs", "parted", "fdisk", "mount", "umount", "ln", "rsync", "systemctl", "service", "launchctl", "iptables", "nft", "ufw", "crontab"];
const DISPATCHERS: Words = &["xargs", "eval", "exec", "source", "env", "sudo", "doas", "nohup", "timeout", "watch", "npx"];
const GIT_MUTATIONS: Words = &["push", "reset", "clean", "checkout", "switch", "rebase", "merge", "commit", "restore", "rm", "mv", "apply", "am", "cherry-pick", "revert", "stash", "tag", "branch", "remote"];
const DOCKER_MUTATIONS: Words = &["rm", "rmi", "cp", "import", "stop", "kill", "push", "login", "run", "exec", "build", "prune", "system", "restart", "update"];
const COMPOSE_MUTATIONS: Words = &["up", "down", "rm", "run", "exec", "build", "pull", "push", "restart", "start", "stop", "kill", "create"];
const KUBECTL_MUTATIONS: Words = &["delete", "apply", "replace", "scale", "patch", "create", "exec", "edit", "drain", "cordon", "uncordon", "rollout", "annotate", "label", "taint", "cp"];
const JS_PACKAGE_MUTATIONS: Words = &["install", "add", "publish", "login", "uninstall", "remove", "update", "upgrade", "link", "unlink", "exec", "dlx", "create", "ci"];
const CARGO_MUTATIONS: Words = &["publish", "install", "login", "add", "remove", "update", "yank", "owner"];
const UV_MUTATIONS: Words = &["pip", "add", "remove", "sync", "tool", "publish", "venv"];

fn unsafe_reason_for_words(parts: &[String]) -> Option<String> {
    let first = parts.first().map(String::as_str).unwrap_or_default().rsplit('/').next().unwrap_or_default();
    let second = parts.get(1).map(String::as_str).unwrap_or_default();
    SAFETY_RULES.iter().find(|rule| safety_rule_matches(rule.kind, first, second, parts))
        .map(|rule| rule.reason.to_string())
}

fn safety_rule_matches(kind: SafetyKind, first: &str, second: &str, parts: &[String]) -> bool {
    match kind {
        SafetyKind::Destructive => DESTRUCTIVE.contains(&first) || first.starts_with("mkfs"),
        SafetyKind::Dispatcher => DISPATCHERS.contains(&first),
        SafetyKind::Remote => ["ssh", "scp", "sftp"].contains(&first),
        SafetyKind::InPlace => {
            let flags = parts.get(1..).unwrap_or_default();
            matches!(first, "sed" | "awk" | "gawk") && flags.iter().any(|p| p.starts_with("-i") || p == "--in-place" || p == "inplace")
                || first == "perl" && flags.iter().any(|p| p.starts_with('-') && !p.starts_with("--") && p.contains('i'))
        }
        SafetyKind::Find => first == "find" && parts.iter().any(|p| ["-delete", "-exec", "-execdir", "-ok", "-okdir"].contains(&p.as_str())),
        SafetyKind::Git => first == "git" && GIT_MUTATIONS.contains(&second),
        SafetyKind::Docker => first == "docker" && (DOCKER_MUTATIONS.contains(&second)
            || second == "compose" && parts.iter().skip(2).any(|p| COMPOSE_MUTATIONS.contains(&p.as_str()))),
        SafetyKind::Kubectl => first == "kubectl" && KUBECTL_MUTATIONS.contains(&second),
        SafetyKind::Package => match first {
            "npm" | "pnpm" | "yarn" => JS_PACKAGE_MUTATIONS.contains(&second),
            "cargo" => CARGO_MUTATIONS.contains(&second),
            "uv" => UV_MUTATIONS.contains(&second),
            _ => false,
        },
        SafetyKind::Network => ["curl", "wget"].contains(&first),
    }
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
