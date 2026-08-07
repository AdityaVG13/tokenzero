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

type Words = &'static [&'static str];

const FILTER_SPECS: &[(&str, &str)] = &[
    ("read", "cat|head|tail|wc"),
    ("search", "rg|grep|findstr"),
    ("tree", "find|ls|tree"),
    ("git", "git status|git diff|git log"),
    (
        "test",
        "pytest|cargo test|go test|npm test|pnpm test|yarn test|jest|vitest",
    ),
    (
        "build",
        "cargo build|npm run build|pnpm build|yarn build|tsc|eslint|ruff|mypy|clippy",
    ),
    ("docker", "docker ps|docker logs|docker compose"),
    ("kubectl", "kubectl get|kubectl logs|kubectl describe"),
    ("package", "cargo|npm|pnpm|yarn|uv"),
    ("config", "json|yaml|toml|logs"),
];

// Ordered: specific command/subcommand pairs precede broad package families.
const CLASS_RULES: &[(&str, Words, Words)] = &[
    ("read", &["cat", "head", "tail", "wc"], &[]),
    ("search", &["rg", "grep", "findstr"], &[]),
    ("tree", &["find", "ls", "tree"], &[]),
    ("git", &["git"], &[]),
    ("test", &["pytest", "unittest", "jest", "vitest"], &[]),
    ("test", &["cargo", "go", "npm", "pnpm", "yarn"], &["test"]),
    ("build", &["cargo"], &["build"]),
    ("build", &["npm", "pnpm", "yarn"], &["run", "build"]),
    ("build", &["tsc", "eslint", "ruff", "mypy", "clippy"], &[]),
    ("docker", &["docker"], &[]),
    ("kubectl", &["kubectl"], &[]),
    ("package", &["cargo", "npm", "pnpm", "yarn", "uv"], &[]),
];

pub fn supported_filters() -> Vec<FilterInfo> {
    FILTER_SPECS
        .iter()
        .map(|&(family, commands)| FilterInfo {
            family: family.to_string(),
            commands: commands.split('|').map(str::to_string).collect(),
            supported: true,
            exact_refs: true,
        })
        .collect()
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
    cfg!(windows)
        .then(|| "verify PowerShell and cmd quoting with the OS matrix before launch".to_string())
        .into_iter()
        .collect()
}

fn classify_words(parts: &[String]) -> &'static str {
    let first = parts
        .first()
        .map(|word| executable_name(word))
        .unwrap_or_default();
    let second = parts.get(1).map(String::as_str).unwrap_or_default();
    CLASS_RULES
        .iter()
        .find(|(_, commands, subcommands)| {
            commands.contains(&first) && (subcommands.is_empty() || subcommands.contains(&second))
        })
        .map_or("unknown", |rule| rule.0)
}

pub fn classify_command(command: &str) -> String {
    classify_words(&split_words(command)).to_string()
}

pub fn rewrite_command(command: &str, mode: &str, enabled: bool) -> RewriteResult {
    let parts = split_words(command);
    let family = classify_words(&parts);
    if !enabled || mode == "off" {
        let safe = rewrite_enabled(command, family, &parts).safe;
        return result(command, command.into(), false, "disabled", family, safe);
    }
    rewrite_enabled(command, family, &parts)
}

fn rewrite_enabled(command: &str, family: &str, parts: &[String]) -> RewriteResult {
    let (unsafe_reason, compound) = analyze_shell(command);
    if let Some(reason) = unsafe_reason {
        return result(command, command.into(), false, &reason, family, false);
    }
    if compound {
        return result(
            command,
            command.into(),
            false,
            "compound command left unmodified",
            family,
            false,
        );
    }
    match apply_rewrite(family, command, parts) {
        Some(rewritten) => {
            let applied = rewritten != command;
            result(
                command,
                rewritten,
                applied,
                if applied {
                    "bounded tokenzero-safe rewrite"
                } else {
                    "already bounded or passthrough"
                },
                family,
                true,
            )
        }
        None => result(
            command,
            command.into(),
            false,
            "unsupported command family",
            family,
            false,
        ),
    }
}

fn result(
    command: &str,
    rewritten: std::borrow::Cow<'_, str>,
    applied: bool,
    reason: &str,
    family: &str,
    safe: bool,
) -> RewriteResult {
    RewriteResult {
        schema_version: "tokenzero.rewrite.v1".to_string(),
        status: "ok".to_string(),
        command: command.to_string(),
        rewritten_command: rewritten.into_owned(),
        applied,
        reason: reason.to_string(),
        family: family.to_string(),
        safe,
    }
}

fn apply_rewrite<'a>(
    family: &str,
    command: &'a str,
    parts: &[String],
) -> Option<std::borrow::Cow<'a, str>> {
    use std::borrow::Cow::{Borrowed, Owned};
    // Classification and safety normalize Windows/path-qualified executables.
    // Rewrites intentionally require the canonical command spelling so a
    // filter never changes the executable identity the caller selected.
    let first = parts.first().map(String::as_str);
    match family {
        "read" => match first {
            Some("cat") if parts.len() >= 2 => {
                // Preserve the original argument span so expansions, globs,
                // tilde, and comments keep their shell-denoted meaning.
                let args = raw_args_after_first_word(command)?;
                Some(Owned(format!("tokenzero read {args}")))
            }
            Some("head" | "tail") => Some(Borrowed(command)),
            _ => None,
        },
        "search" => matches!(first, Some("rg" | "grep")).then_some(Borrowed(command)),
        "tree" => match first {
            Some("tree") if !parts.iter().any(|p| is_tree_depth_flag(p)) => {
                Some(Owned(format!("{command} -L 2")))
            }
            Some("tree" | "find") => Some(Borrowed(command)),
            Some("ls") if !parts.iter().any(|p| p.contains('R')) => Some(Borrowed(command)),
            _ => None,
        },
        "git" => match parts.get(1).map(String::as_str) {
            Some("log") if !parts.iter().any(|p| is_git_log_count_flag(p)) => {
                Some(Owned(format!("{command} -n 80")))
            }
            Some("log" | "status" | "diff" | "show") => Some(Borrowed(command)),
            Some("clone" | "fetch" | "pull") => {
                Some(inject_quiet_flag(command, parts).map_or(Borrowed(command), Owned))
            }
            _ => None,
        },
        "test" | "build" | "package" => {
            Some(inject_quiet_flag(command, parts).map_or(Borrowed(command), Owned))
        }
        "docker" | "kubectl" => Some(Borrowed(command)),
        _ => None,
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

const VERBOSITY_FLAGS: &str =
    "-q --quiet -v -vv -vvv --verbose -s --silent --progress --no-progress";
const QUIET_RULES: &[(&str, &str, &str)] = &[
    ("cargo", "build check clippy test bench doc fetch run", "-q"),
    ("git", "clone fetch pull", "--quiet"),
    ("npm", "test run build rebuild", "--silent"),
];

fn has_explicit_verbosity(parts: &[String]) -> bool {
    parts.iter().any(|part| {
        listed(VERBOSITY_FLAGS, part)
            || part.starts_with("--loglevel")
            || part.starts_with("--verbosity")
    })
}

fn inject_quiet_flag(command: &str, parts: &[String]) -> Option<String> {
    if has_explicit_verbosity(parts) || parts.iter().any(|part| part == "--") {
        return None;
    }
    let first = parts.first().map(String::as_str).unwrap_or_default();
    let second = parts.get(1).map(String::as_str).unwrap_or_default();
    QUIET_RULES
        .iter()
        .find(|(commands, subcommands, _)| listed(commands, first) && listed(subcommands, second))
        .map(|(_, _, flag)| format!("{command} {flag}"))
}

fn analyze_shell(command: &str) -> (Option<String>, bool) {
    let (commands, compound) = parse_shell_commands(command);
    (unsafe_reason_for_commands(&commands), compound)
}

fn unsafe_reason_for_commands(commands: &[ShellCommand]) -> Option<String> {
    for node in commands {
        if let Some(reason) = unsafe_reason_for_words(&node.words) {
            return Some(reason);
        }
        for nested in &node.nested_commands {
            if let (Some(reason), _) = analyze_shell(nested) {
                return Some(reason);
            }
        }
        if is_shell_interpreter(&node.words) {
            if let Some(payload) = shell_command_payload(&node.words) {
                if let (Some(reason), _) = analyze_shell(payload) {
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

/// Parse executable positions and report operators that make a command compound.
fn parse_shell_commands(command: &str) -> (Vec<ShellCommand>, bool) {
    let mut commands = vec![ShellCommand::default()];
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let chars: Vec<char> = command.chars().collect();
    let mut index = 0;
    let mut compound = false;
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            word.push(ch);
            escaped = false;
        } else if ch == '\\' && quote != Some('\'') && !cfg!(windows) {
            escaped = true;
        } else if quote == Some('\'') {
            if ch == '\'' {
                quote = None;
            } else {
                word.push(ch);
            }
        } else if ch == '\'' {
            quote = Some('\'');
        } else if ch == '"' {
            quote = if quote == Some('"') { None } else { Some('"') };
        } else if ch == '$' && chars.get(index + 1) == Some(&'(') {
            flush_shell_word(&mut commands, &mut word);
            index = push_nested(&mut commands, &chars, index + 2, ')');
            compound = true;
            continue;
        } else if ch == '`' {
            flush_shell_word(&mut commands, &mut word);
            index = push_nested(&mut commands, &chars, index + 1, '`');
            compound = true;
            continue;
        } else if quote.is_none() && ch.is_whitespace() {
            flush_shell_word(&mut commands, &mut word);
            if matches!(ch, '\n' | '\r') {
                start_shell_command(&mut commands);
                compound = true;
            }
        } else if quote.is_none() && matches!(ch, ';' | '|' | '&' | '!' | '(' | ')') {
            flush_shell_word(&mut commands, &mut word);
            start_shell_command(&mut commands);
            compound |= matches!(ch, ';' | '|' | '&');
            if chars.get(index + 1) == Some(&ch) {
                index += 1;
            }
        } else {
            compound |= quote.is_none() && matches!(ch, '>' | '<');
            word.push(ch);
        }
        index += 1;
    }
    if escaped {
        word.push('\\');
    }
    flush_shell_word(&mut commands, &mut word);
    commands.retain(|node| !node.words.is_empty() || !node.nested_commands.is_empty());
    (commands, compound)
}

fn push_nested(
    commands: &mut Vec<ShellCommand>,
    chars: &[char],
    start: usize,
    delimiter: char,
) -> usize {
    let (nested, next) = take_nested_command(chars, start, delimiter);
    if let Some(command) = commands.last_mut() {
        command.nested_commands.push(nested);
    } else {
        commands.push(ShellCommand {
            nested_commands: vec![nested],
            ..ShellCommand::default()
        });
    }
    next
}

fn flush_shell_word(commands: &mut Vec<ShellCommand>, word: &mut String) {
    if !word.is_empty() {
        let word = std::mem::take(word);
        if let Some(command) = commands.last_mut() {
            command.words.push(word);
        } else {
            commands.push(ShellCommand {
                words: vec![word],
                ..ShellCommand::default()
            });
        }
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

fn take_nested_command(chars: &[char], start: usize, delimiter: char) -> (String, usize) {
    let mut index = start;
    let mut depth = 1;
    let mut quote = None;
    let mut escaped = false;
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            escaped = false;
        } else if ch == '\\' && (delimiter == '`' || quote != Some('\'')) {
            escaped = true;
        } else if delimiter == '`' {
            if ch == '`' {
                return (chars[start..index].iter().collect(), index + 1);
            }
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

fn is_shell_interpreter(words: &[String]) -> bool {
    command_words(words).first().is_some_and(|word| {
        matches!(
            executable_name(word),
            "sh" | "bash" | "dash" | "zsh" | "ksh"
        )
    })
}

fn shell_command_payload(words: &[String]) -> Option<&str> {
    command_words(words).windows(2).find_map(|pair| {
        let flag = pair[0].as_str();
        (flag == "-c"
            || (flag.starts_with('-')
                && !flag.starts_with("--")
                && flag[1..].chars().any(|ch| ch == 'c')))
        .then_some(pair[1].as_str())
    })
}

fn command_words(words: &[String]) -> &[String] {
    let mut index = 0;
    while let Some(word) = words.get(index) {
        if matches!(
            word.as_str(),
            "{" | "}" | "if" | "then" | "elif" | "else" | "while" | "until" | "do"
        ) || is_shell_assignment(word)
        {
            index += 1;
        } else {
            break;
        }
    }
    &words[index..]
}

fn is_shell_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let name = name.strip_suffix('+').unwrap_or(name);
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn executable_name(word: &str) -> &str {
    let basename = word.rsplit(['/', '\\']).next().unwrap_or(word);
    for suffix in [".exe", ".cmd", ".bat", ".com"] {
        if basename.len() > suffix.len()
            && basename[basename.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
        {
            return &basename[..basename.len() - suffix.len()];
        }
    }
    basename
}

/// Embedded language payloads are intentionally opaque. Without a language
/// parser TokenZero cannot prove that `python -c`, `node -e`, or equivalent
/// code avoids mutations, so the safety surface must never vouch for it.
fn has_embedded_interpreter_payload(words: &[String]) -> bool {
    let words = command_words(words);
    let Some(first) = words.first() else {
        return false;
    };
    let executable = executable_name(first).to_ascii_lowercase();
    let executable = executable.as_str();
    let flags = words.iter().skip(1).map(String::as_str);
    let python = executable == "python"
        || executable
            .strip_prefix("python")
            .is_some_and(|suffix| suffix.chars().next().is_some_and(|ch| ch.is_ascii_digit()));
    if python {
        return flags.into_iter().any(|flag| flag == "-c");
    }
    match executable {
        "node" | "deno" | "bun" => flags
            .into_iter()
            .any(|flag| matches!(flag, "-e" | "--eval")),
        "perl" | "ruby" => flags.into_iter().any(|flag| flag == "-e"),
        "cmd" => flags
            .into_iter()
            .any(|flag| flag.eq_ignore_ascii_case("/c") || flag.eq_ignore_ascii_case("/k")),
        "powershell" | "pwsh" => flags.into_iter().any(|flag| {
            matches!(
                flag.to_ascii_lowercase().as_str(),
                "-c" | "-command" | "-encodedcommand"
            )
        }),
        _ => false,
    }
}

const DESTRUCTIVE: &str = "rm rmdir unlink mv cp chmod chown dd shutdown reboot shred truncate wipefs parted fdisk mount umount ln rsync systemctl service launchctl iptables nft ufw crontab";
const DISPATCHERS: &str =
    "xargs eval exec source env sudo doas nohup timeout watch npx command time builtin nice ionice";
const GIT_MUTATIONS: &str = "push reset clean checkout switch rebase merge commit restore rm mv apply am cherry-pick revert stash tag branch remote";
const DOCKER_MUTATIONS: &str =
    "rm rmi cp import stop kill push login run exec build prune system restart update";
const COMPOSE_MUTATIONS: &str =
    "up down rm run exec build pull push restart start stop kill create";
const KUBECTL_MUTATIONS: &str = "delete apply replace scale patch create exec edit drain cordon uncordon rollout annotate label taint cp";
const JS_PACKAGE_MUTATIONS: &str =
    "install add publish login uninstall remove update upgrade link unlink exec dlx create ci";
const CARGO_MUTATIONS: &str = "publish install login add remove update yank owner";
const UV_MUTATIONS: &str = "pip add remove sync tool publish venv";

fn listed(list: &str, word: &str) -> bool {
    list.split_ascii_whitespace()
        .any(|candidate| candidate == word)
}

fn unsafe_reason_for_words(parts: &[String]) -> Option<String> {
    let parts = command_words(parts);
    let first = parts
        .first()
        .map(|word| executable_name(word).to_ascii_lowercase())
        .unwrap_or_default();
    let first = first.as_str();
    let second = parts.get(1).map(String::as_str).unwrap_or_default();
    let flags = parts.get(1..).unwrap_or_default();
    let in_place_edit = matches!(first, "sed" | "awk" | "gawk")
        && flags
            .iter()
            .any(|p| p.starts_with("-i") || p == "--in-place" || p == "inplace")
        || first == "perl"
            && flags
                .iter()
                .any(|p| p.starts_with('-') && !p.starts_with("--") && p.contains('i'));
    let reason = if listed(DESTRUCTIVE, first) || first.starts_with("mkfs") {
        "unsafe destructive mutation left unmodified"
    } else if listed(DISPATCHERS, first) {
        "command dispatcher left unmodified; safety depends on the dispatched command"
    } else if matches!(first, "ssh" | "scp" | "sftp") {
        "remote execution left unmodified"
    } else if in_place_edit {
        "in-place file edit left unmodified"
    } else if has_embedded_interpreter_payload(parts) {
        "embedded interpreter payload left unmodified"
    } else if first == "find"
        && parts.iter().any(|p| {
            matches!(
                p.as_str(),
                "-delete" | "-exec" | "-execdir" | "-ok" | "-okdir"
            )
        })
    {
        "find with side effects left unmodified"
    } else if first == "git" && listed(GIT_MUTATIONS, second) {
        "git mutation left unmodified"
    } else if first == "docker"
        && (listed(DOCKER_MUTATIONS, second)
            || second == "compose" && parts.iter().skip(2).any(|p| listed(COMPOSE_MUTATIONS, p)))
    {
        "docker mutation left unmodified"
    } else if first == "kubectl" && listed(KUBECTL_MUTATIONS, second) {
        "kubectl mutation left unmodified"
    } else if match first {
        "npm" | "pnpm" | "yarn" => listed(JS_PACKAGE_MUTATIONS, second),
        "cargo" => listed(CARGO_MUTATIONS, second),
        "uv" => listed(UV_MUTATIONS, second),
        _ => false,
    } {
        "package/network mutation left unmodified"
    } else if matches!(first, "curl" | "wget") {
        "network command left unmodified"
    } else {
        return None;
    };
    Some(reason.to_string())
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

/// Return the raw argument text after the first shell word, preserving quotes,
/// expansions, globs, tilde, and comment syntax exactly as the user wrote them.
fn raw_args_after_first_word(command: &str) -> Option<&str> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut in_first = false;
    let mut past_first = false;
    for (idx, ch) in command.char_indices() {
        if past_first {
            let rest = command[idx..].trim_start();
            return (!rest.is_empty()).then_some(rest);
        }
        if escaped {
            escaped = false;
            in_first = true;
            continue;
        }
        if ch == '\\' && quote != Some('\'') && !cfg!(windows) {
            escaped = true;
            in_first = true;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            in_first = true;
            continue;
        }
        if quote.is_none() && (ch == '\'' || ch == '"') {
            quote = Some(ch);
            in_first = true;
            continue;
        }
        if quote.is_none() && ch.is_whitespace() {
            if in_first {
                past_first = true;
            }
            continue;
        }
        in_first = true;
    }
    None
}

#[cfg(test)]
mod bypass_regression_tests {
    use super::*;

    #[test]
    fn safety_finds_mutations_behind_shell_prefixes() {
        for command in [
            "{ rm -rf /tmp/tokenzero-bypass; }",
            "TOKENZERO_TEST=1 rm -rf /tmp/tokenzero-bypass",
            "time rm -rf /tmp/tokenzero-bypass",
            "command rm -rf /tmp/tokenzero-bypass",
        ] {
            let result = rewrite_command(command, "on", true);
            assert!(!result.safe, "unexpectedly vouched: {command}");
            assert!(
                result.reason.contains("mutation") || result.reason.contains("dispatcher"),
                "mutation prefix was not classified for {command}: {}",
                result.reason
            );
        }
    }

    #[test]
    fn embedded_interpreter_payloads_are_never_vouched() {
        for command in [
            "python3 -c 'from pathlib import Path; Path(\"x\").unlink()'",
            "node -e 'require(\"fs\").rmSync(\"x\")'",
            "cmd.exe /C del x",
            "pwsh -Command 'Remove-Item x'",
        ] {
            let result = rewrite_command(command, "on", true);
            assert!(!result.safe, "unexpectedly vouched: {command}");
            assert_eq!(
                result.reason, "embedded interpreter payload left unmodified",
                "embedded payload was not classified for {command}"
            );
        }
    }

    #[test]
    fn windows_executable_suffixes_and_backslashes_reach_safety_rules() {
        let words = vec![
            r"C:\Tools\RM.EXE".to_string(),
            "-rf".to_string(),
            "target".to_string(),
        ];
        assert_eq!(executable_name(&words[0]), "RM");
        assert_eq!(
            unsafe_reason_for_words(&words).as_deref(),
            Some("unsafe destructive mutation left unmodified")
        );
    }
}

#[cfg(test)]
mod tests;
