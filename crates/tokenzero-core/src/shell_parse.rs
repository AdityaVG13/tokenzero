use crate::*;

pub(crate) fn failed_segment(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> Option<String> {
    if is_search_no_match(command, stdout, stderr, exit_code)
        || is_expected_false_exit(command, stdout, stderr, exit_code)
    {
        return None;
    }
    if looks_env_invocation_failure(command, stdout, stderr, exit_code) {
        return Some(command.trim().to_string()).filter(|segment| !segment.is_empty());
    }
    if let Some(segment) = masked_or_failure_segment(command, stdout, stderr, exit_code) {
        return Some(segment);
    }
    if let Some(segment) = masked_pipeline_failure_segment(command, stdout, stderr, exit_code) {
        return Some(segment);
    }

    let segments = split_shell_segments(command);
    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    let stderr_lower = stderr.to_ascii_lowercase();
    let failure_output = if exit_code == Some(0) {
        stderr_lower.as_str()
    } else {
        combined.as_str()
    };

    for segment in &segments {
        if is_explicit_false_segment(segment) {
            return Some(segment.clone());
        }
        if is_cd_failure_segment(segment, exit_code, failure_output) {
            return Some(segment.clone());
        }
        if is_command_not_found_segment(segment, failure_output) {
            return Some(segment.clone());
        }
    }
    if exit_code.is_some_and(|code| code != 0) && looks_diagnostic(&combined) {
        for segment in &segments {
            if is_diagnostic_failure_segment(segment, stdout, stderr) {
                return Some(segment.clone());
            }
        }
    }
    if exit_code.is_some_and(|code| code != 0) {
        segments.last().cloned().filter(|v| !v.is_empty())
    } else {
        None
    }
}

const CD_FAILURE_NEEDLES: &[&str] = &["can't cd", "no such file", "not a directory"];

fn is_cd_failure_segment(segment: &str, exit_code: Option<i32>, failure_output: &str) -> bool {
    segment.to_ascii_lowercase().starts_with("cd ")
        && (exit_code.is_some_and(|code| code != 0)
            || CD_FAILURE_NEEDLES.iter().any(|n| failure_output.contains(n)))
}

fn is_command_not_found_segment(segment: &str, failure_output: &str) -> bool {
    !segment.is_empty()
        && (failure_output.contains("command not found") || failure_output.contains("not found"))
}

const DIAGNOSTIC_SHELL_FAMILIES: &[&str] = &["test", "build", "lint", "python-test", "go-test"];

pub(crate) fn is_diagnostic_failure_segment(segment: &str, stdout: &str, stderr: &str) -> bool {
    DIAGNOSTIC_SHELL_FAMILIES.contains(&shell_family(segment, stdout, stderr).as_str())
        || segment.contains("--check")
}

pub(crate) fn masked_or_failure_segment(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> Option<String> {
    if exit_code != Some(0) || is_masked_expected_false_or(command, stdout, stderr, exit_code) {
        return None;
    }
    let segment = first_or_list_lhs(command)?;
    let segment = segment.trim();
    if segment.is_empty() || is_expected_false_segment(segment, stdout, stderr) {
        return None;
    }
    if looks_masked_failure_evidence(stdout, stderr, Some(segment)) {
        Some(segment.to_string())
    } else {
        None
    }
}

pub(crate) fn masked_pipeline_failure_segment(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> Option<String> {
    if exit_code != Some(0) || is_masked_expected_false_pipeline(command, stdout, stderr, exit_code)
    {
        return None;
    }
    if !shell_operator_features(command).contains(&"pipeline")
        || !looks_masked_failure_evidence(
            stdout,
            stderr,
            first_nonempty_shell_segment(command).as_deref(),
        )
    {
        return None;
    }
    split_shell_segments(command)
        .into_iter()
        .find(|segment| !segment.is_empty())
}

pub(crate) fn masking_warning(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> Option<String> {
    if is_repo_inventory_command(command) && exit_code == Some(0) && stderr.trim().is_empty()
        || looks_env_invocation_failure(command, stdout, stderr, exit_code)
        || is_masked_expected_false_or(command, stdout, stderr, exit_code)
        || is_masked_expected_false_pipeline(command, stdout, stderr, exit_code)
    {
        return None;
    }
    let has_masking_syntax = shell_operator_features(command)
        .iter()
        .any(|feature| matches!(*feature, "pipeline" | "sequence" | "or-list"));
    if !has_masking_syntax {
        return None;
    }
    let should_warn = if exit_code == Some(0) {
        split_shell_segments(command)
            .iter()
            .any(|segment| is_explicit_false_segment(segment))
            || looks_masked_failure_evidence(
                stdout,
                stderr,
                first_nonempty_shell_segment(command).as_deref(),
            )
    } else {
        const NEEDLES: &[&str] = &[
            "not found",
            "no such file",
            "permission denied",
            "unrecognized option",
            "invalid option",
            "usage:",
            "error",
        ];
        let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
        split_shell_segments(command)
            .iter()
            .any(|segment| is_explicit_false_segment(segment))
            || NEEDLES.iter().any(|n| combined.contains(n))
    };
    if should_warn {
        Some("compound or pipeline syntax can mask upstream failure; inspect refs or rerun with pipefail".to_string())
    } else {
        None
    }
}

pub(crate) fn pipeline_rerun_command(command: &str, warning: Option<&String>) -> Option<String> {
    if cfg!(windows) || warning.is_none() {
        return None;
    }
    let features = shell_operator_features(command);
    if !features.contains(&"pipeline") {
        return None;
    }
    let analysis_command = shell_analysis_command(command);
    if analysis_command.trim().is_empty() {
        return None;
    }
    Some(format!(
        "bash -o pipefail -c {}",
        shell_display_arg(analysis_command.trim(), "posix")
    ))
}

pub(crate) fn first_or_list_lhs(command: &str) -> Option<String> {
    let command = shell_analysis_command(command);
    let mut quote: Option<char> = None;
    let mut chars = command.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if Some(ch) == quote {
            quote = None;
        } else if quote.is_none() && (ch == '\'' || ch == '"') {
            quote = Some(ch);
        } else if quote.is_none() && ch == '|' && chars.peek().is_some_and(|(_, n)| *n == '|') {
            return Some(command[..idx].trim().to_string());
        }
    }
    None
}

pub(crate) fn first_nonempty_shell_segment(command: &str) -> Option<String> {
    split_shell_segments(command)
        .into_iter()
        .find(|segment| !segment.is_empty())
}

fn line_has_structured_masked_failure_evidence(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("error:")
        || lower.starts_with("error[")
        || lower.starts_with("warning:")
        || lower.contains("panic")
        || lower.contains("traceback")
        || lower.contains("command not found")
        || lower.contains("no such file or directory")
        || lower.contains("permission denied")
        || lower.contains("assertion failed")
        || lower.starts_with("fatal:")
        || lower.contains("unrecognized option")
        || lower.contains("invalid option")
        || lower.contains("usage:")
}

fn stderr_has_masked_failure_evidence(stderr: &str) -> bool {
    !stderr.trim().is_empty()
        && stderr
            .lines()
            .any(line_has_structured_masked_failure_evidence)
}

fn stdout_has_structured_masked_failure_evidence(stdout: &str) -> bool {
    !stdout.trim().is_empty()
        && stdout
            .lines()
            .any(line_has_structured_masked_failure_evidence)
}

fn search_stdout_has_masked_failure_evidence(stdout: &str) -> bool {
    !stdout.trim().is_empty() && stdout.lines().any(search_stdout_line_is_diagnostic)
}

const SEARCH_DIAG_PREFIXES: &[&str] = &[
    "error:",
    "warning:",
    "fatal:",
    "panic",
    "traceback",
    "rg:",
    "grep:",
    "ripgrep:",
];
const SEARCH_DIAG_NEEDLES: &[&str] = &[
    "regex parse error",
    "unrecognized option",
    "invalid option",
    "permission denied",
    "no such file or directory",
];

fn search_stdout_line_is_diagnostic(line: &str) -> bool {
    let lower = line.trim_start().to_ascii_lowercase();
    SEARCH_DIAG_PREFIXES.iter().any(|p| lower.starts_with(p))
        || SEARCH_DIAG_NEEDLES.iter().any(|n| lower.contains(n))
}

/// Strict masked-failure evidence for exit-code-0 compound/pipeline paths.
/// Bare substrings like `failed` in data lines are not evidence.
pub(crate) fn looks_masked_failure_evidence(
    stdout: &str,
    stderr: &str,
    command_head: Option<&str>,
) -> bool {
    if stderr_has_masked_failure_evidence(stderr) {
        return true;
    }
    if let Some(head) = command_head {
        let analysis = shell_analysis_command(head);
        if split_shell_words(&analysis)
            .first()
            .is_some_and(|word| is_search_command(word))
        {
            return search_stdout_has_masked_failure_evidence(stdout);
        }
    }
    stdout_has_structured_masked_failure_evidence(stdout)
}

#[allow(dead_code)]
pub(crate) fn looks_masked_failure_output(stdout: &str, stderr: &str) -> bool {
    looks_masked_failure_evidence(stdout, stderr, None)
}

pub(crate) fn shell_syntax_summary_for_status(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> String {
    let features = if looks_env_invocation_failure(command, stdout, stderr, exit_code) {
        raw_shell_operator_features(command)
    } else {
        shell_operator_features(command)
    };
    shell_syntax_summary_from_features(&features)
}

pub(crate) fn shell_syntax_summary_from_features(features: &[&'static str]) -> String {
    if features.is_empty() {
        "argv/simple".to_string()
    } else {
        features.join(",")
    }
}

pub(crate) fn shell_operator_features(command: &str) -> Vec<&'static str> {
    let command = shell_analysis_command(command);
    raw_shell_operator_features(&command)
}

struct QuoteCursor<'a> {
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    quote: Option<char>,
    escaped: bool,
}

impl<'a> QuoteCursor<'a> {
    fn new(command: &'a str) -> Self {
        Self {
            chars: command.char_indices().peekable(),
            quote: None,
            escaped: false,
        }
    }

    fn next_unquoted(&mut self) -> Option<(usize, char, Option<char>)> {
        while let Some((idx, ch)) = self.chars.next() {
            if self.escaped {
                self.escaped = false;
                continue;
            }
            if self.quote != Some('\'') && ch == '\\' {
                self.escaped = true;
                continue;
            }
            if Some(ch) == self.quote {
                self.quote = None;
                continue;
            }
            if self.quote.is_some() {
                continue;
            }
            if ch == '\'' || ch == '"' {
                self.quote = Some(ch);
                continue;
            }
            let next = self.chars.peek().map(|(_, n)| *n);
            return Some((idx, ch, next));
        }
        None
    }
}

pub(crate) fn raw_shell_operator_features(command: &str) -> Vec<&'static str> {
    let mut features = Vec::new();
    let mut cursor = QuoteCursor::new(command);
    while let Some((_, ch, next)) = cursor.next_unquoted() {
        match (ch, next) {
            ('&', Some('&')) => {
                push_unique_feature(&mut features, "and-list");
                cursor.chars.next();
            }
            ('|', Some('|')) => {
                push_unique_feature(&mut features, "or-list");
                cursor.chars.next();
            }
            ('|', _) => push_unique_feature(&mut features, "pipeline"),
            (';', _) => push_unique_feature(&mut features, "sequence"),
            ('>' | '<', _) => push_unique_feature(&mut features, "redirect"),
            ('$', Some('(')) => {
                push_unique_feature(&mut features, "subshell");
                cursor.chars.next();
            }
            ('`', _) => push_unique_feature(&mut features, "subshell"),
            _ => {}
        }
    }
    features
}

pub(crate) fn push_unique_feature(features: &mut Vec<&'static str>, feature: &'static str) {
    if !features.contains(&feature) {
        features.push(feature);
    }
}

pub(crate) fn split_shell_segments(command: &str) -> Vec<String> {
    let command = shell_analysis_command(command);
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut cursor = QuoteCursor::new(&command);
    while let Some((idx, ch, next)) = cursor.next_unquoted() {
        let split_len = match (ch, next) {
            ('&', Some('&')) | ('|', Some('|')) => Some(2),
            ('|' | ';', _) => Some(1),
            _ => None,
        };
        if let Some(split_len) = split_len {
            push_shell_segment(&mut segments, &command[start..idx]);
            if split_len == 2 {
                cursor.chars.next();
            }
            start = idx + split_len;
        }
    }
    push_shell_segment(&mut segments, &command[start..]);
    segments
}

pub(crate) fn push_shell_segment(segments: &mut Vec<String>, segment: &str) {
    let segment = segment.trim();
    if !segment.is_empty() {
        segments.push(segment.to_string());
    }
}

pub(crate) fn shell_analysis_command(command: &str) -> String {
    let words = split_shell_words(command);
    if let Some(command) = shell_analysis_command_from_words(&words) {
        return command;
    }
    command.to_string()
}

pub(crate) fn shell_analysis_command_from_words(words: &[String]) -> Option<String> {
    let first = words.first().map(|word| shell_command_basename(word)).unwrap_or_default();
    match first.as_str() {
        "sh" | "bash" | "zsh" => shell_c_command_argument(words),
        "cmd" => cmd_command_argument(words),
        "powershell" | "pwsh" => powershell_command_argument(words),
        "env" => env_split_string_analysis_command(words).or_else(|| {
            let command_index = env_wrapped_command_index(words)?;
            shell_analysis_command_from_words(&words[command_index..])
        }),
        _ => None,
    }
}

pub(crate) fn cmd_command_argument(words: &[String]) -> Option<String> {
    let mut index = 1usize;
    while index < words.len() {
        let word = words[index].as_str();
        let lower = word.to_ascii_lowercase();
        if matches!(lower.as_str(), "/c" | "/k") {
            return shell_command_tail(words, index + 1, "cmd");
        }
        if lower.len() > 2 && (lower.starts_with("/c") || lower.starts_with("/k")) {
            return Some(word[2..].trim().to_string()).filter(|command| !command.is_empty());
        }
        if lower.starts_with('/') {
            index += 1;
            continue;
        }
        return None;
    }
    None
}

pub(crate) fn powershell_command_argument(words: &[String]) -> Option<String> {
    let mut index = 1usize;
    while index < words.len() {
        let word = words[index].as_str();
        let lower = word.to_ascii_lowercase();
        if matches!(lower.as_str(), "-command" | "-c") {
            return shell_command_tail(words, index + 1, "powershell");
        }
        if lower.starts_with("-command:") {
            return Some(word["-command:".len()..].trim().to_string())
                .filter(|command| !command.is_empty());
        }
        if matches!(
            lower.as_str(),
            "-encodedcommand" | "-enc" | "-e" | "-file" | "-f"
        ) {
            return None;
        }
        if is_powershell_inline_option_with_value(&lower) {
            index += 1;
            continue;
        }
        if is_powershell_option_with_value(&lower) {
            index += 2;
            continue;
        }
        if lower.starts_with('-') {
            index += 1;
            continue;
        }
        return None;
    }
    None
}

pub(crate) fn shell_command_tail(
    words: &[String],
    start: usize,
    style: &str,
) -> Option<String> {
    let tail = words.get(start..)?;
    if tail.is_empty() {
        return None;
    }
    if tail.len() == 1 {
        return Some(tail[0].clone());
    }
    if !tail.first().is_some_and(|word| is_search_command(word)) {
        return Some(tail.join(" "));
    }
    Some(
        tail.iter()
            .map(|word| shell_display_arg(word, style))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

const POWERSHELL_VALUE_OPTIONS: &[&str] = &[
    "-configurationname",
    "-executionpolicy",
    "-inputformat",
    "-outputformat",
    "-settingsfile",
    "-version",
    "-windowstyle",
    "-workingdirectory",
];

pub(crate) fn is_powershell_option_with_value(lower: &str) -> bool {
    POWERSHELL_VALUE_OPTIONS.contains(&lower)
}

pub(crate) fn is_powershell_inline_option_with_value(lower: &str) -> bool {
    POWERSHELL_VALUE_OPTIONS.iter().any(|opt| {
        lower.starts_with(opt) && lower.as_bytes().get(opt.len()) == Some(&b':')
    })
}

pub(crate) fn env_split_string_analysis_command(words: &[String]) -> Option<String> {
    let split_words = env_split_string_words(words)?;
    if split_words.is_empty() {
        return None;
    }
    let mut env_words = Vec::with_capacity(split_words.len() + 1);
    env_words.push("env".to_string());
    env_words.extend(split_words);
    let command_index = env_wrapped_command_index(&env_words)?;
    shell_analysis_command_from_words(&env_words[command_index..])
}

fn advance_env_option(words: &[String], index: &mut usize) -> bool {
    let word = words[*index].as_str();
    if is_env_assignment(word) || is_env_no_arg_option(word) || is_env_inline_arg_option(word) {
        *index += 1;
        true
    } else if is_env_arg_option(word) {
        *index += 2;
        true
    } else {
        false
    }
}

pub(crate) fn env_split_string_words(words: &[String]) -> Option<Vec<String>> {
    let mut index = 1usize;
    while index < words.len() {
        let word = words[index].as_str();
        if word == "--" {
            return None;
        }
        if matches!(word, "-S" | "--split-string") {
            return words.get(index + 1).map(|value| split_shell_words(value));
        }
        if let Some(value) = word.strip_prefix("--split-string=") {
            return Some(split_shell_words(value));
        }
        if advance_env_option(words, &mut index) {
            continue;
        }
        return None;
    }
    None
}

pub(crate) fn env_wrapped_command_index(words: &[String]) -> Option<usize> {
    let mut index = 1usize;
    while index < words.len() {
        let word = words[index].as_str();
        if word == "--" {
            return (index + 1 < words.len()).then_some(index + 1);
        }
        if advance_env_option(words, &mut index) {
            continue;
        }
        if word.starts_with('-') {
            return None;
        }
        return Some(index);
    }
    None
}

pub(crate) fn looks_env_invocation_failure(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> bool {
    if exit_code == Some(0) {
        return false;
    }
    let words = split_shell_words(command);
    if words
        .first()
        .is_none_or(|word| shell_command_basename(word) != "env")
    {
        return false;
    }
    if !words
        .iter()
        .any(|word| matches!(word.as_str(), "-C" | "--chdir") || word.starts_with("--chdir="))
    {
        return false;
    }
    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    combined.contains("env:")
        && (combined.contains("cannot change directory")
            || combined.contains("not a directory")
            || combined.contains("chdir"))
}

pub(crate) fn is_env_assignment(word: &str) -> bool {
    !word.starts_with('-') && word.find('=').is_some_and(|index| index > 0)
}

const ENV_NO_ARG_OPTIONS: &[&str] = &["-i", "--ignore-environment", "-0", "--null", "--debug"];
const ENV_ARG_OPTIONS: &[&str] = &["-u", "--unset", "-C", "--chdir", "--argv0", "-S", "--split-string"];
const ENV_INLINE_ARG_PREFIXES: &[&str] = &["--unset=", "--chdir=", "--argv0=", "--split-string="];

pub(crate) fn is_env_no_arg_option(word: &str) -> bool {
    ENV_NO_ARG_OPTIONS.contains(&word)
}

pub(crate) fn is_env_arg_option(word: &str) -> bool {
    ENV_ARG_OPTIONS.contains(&word)
}

pub(crate) fn is_env_inline_arg_option(word: &str) -> bool {
    ENV_INLINE_ARG_PREFIXES.iter().any(|p| word.starts_with(p))
}

pub(crate) fn shell_c_command_argument(words: &[String]) -> Option<String> {
    let mut index = 1usize;
    while index < words.len() {
        let word = words[index].as_str();
        if word == "-c" || word == "--command" {
            return words.get(index + 1).cloned();
        }
        if let Some(command) = word.strip_prefix("--command=") {
            return Some(command.to_string());
        }
        if matches!(word, "-o" | "+o" | "-O" | "+O") {
            index += 2;
            continue;
        }
        if short_shell_flags_contain_command(word) {
            return words.get(index + 1).cloned();
        }
        if word == "--" || !word.starts_with('-') && !word.starts_with('+') {
            return None;
        }
        index += 1;
    }
    None
}

pub(crate) fn short_shell_flags_contain_command(word: &str) -> bool {
    let Some(flags) = word.strip_prefix('-') else {
        return false;
    };
    !flags.is_empty() && !flags.starts_with('-') && flags.chars().any(|ch| ch == 'c')
}

pub(crate) fn split_shell_words(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in command.chars() {
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
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

pub(crate) fn looks_diagnostic(text: &str) -> bool {
    text.lines().any(looks_critical_line)
}

pub(crate) fn looks_critical_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "error",
        "warning",
        "failed",
        "failure",
        "panic",
        "traceback",
        "exception",
        "assertion",
        "expected",
        "actual",
        "not ok",
        "prompt",
        "enter ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

pub(crate) fn repeated_line_count(text: &str) -> usize {
    let mut previous = "";
    let mut repeats = 0usize;
    for line in text.lines() {
        if line == previous && !line.trim().is_empty() {
            repeats += 1;
        }
        previous = line;
    }
    repeats
}

pub(crate) fn looks_status_table(text: &str) -> bool {
    text.lines().any(|line| {
        let upper = line.to_ascii_uppercase();
        upper.contains("STATUS") && (upper.contains("NAME") || upper.contains("READY"))
    })
}

pub(crate) fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

pub(crate) fn compact_json(value: &serde_json::Value) -> String {
    let mut text = serde_json::to_string(value).unwrap_or_default();
    if text.len() > 240 {
        text.truncate(240);
        text.push_str("...");
    }
    text
}

pub(crate) fn is_abnormal_json(value: &serde_json::Value) -> bool {
    let text = value.to_string().to_ascii_lowercase();
    [
        "error",
        "failed",
        "unhealthy",
        "pending",
        "crash",
        "warning",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}
