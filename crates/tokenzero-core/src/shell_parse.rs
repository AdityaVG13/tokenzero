use crate::*;

pub(crate) fn failed_segment(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> Option<String> {
    if is_search_no_match(command, stdout, stderr, exit_code) {
        return None;
    }
    if is_expected_false_exit(command, stdout, stderr, exit_code) {
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
    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    let stderr_lower = stderr.to_ascii_lowercase();
    let failure_output = if exit_code == Some(0) {
        stderr_lower.as_str()
    } else {
        combined.as_str()
    };
    for segment in split_shell_segments(command) {
        let lower = segment.to_ascii_lowercase();
        if is_explicit_false_segment(&segment) {
            return Some(segment);
        }
        if lower.starts_with("cd ")
            && (exit_code.is_some_and(|code| code != 0)
                || failure_output.contains("can't cd")
                || failure_output.contains("no such file")
                || failure_output.contains("not a directory"))
        {
            return Some(segment);
        }
        if (failure_output.contains("command not found") || failure_output.contains("not found"))
            && !segment.is_empty()
        {
            return Some(segment);
        }
    }
    if exit_code.is_some_and(|code| code != 0) && looks_diagnostic(&combined) {
        for segment in split_shell_segments(command) {
            if is_diagnostic_failure_segment(&segment, stdout, stderr) {
                return Some(segment);
            }
        }
    }
    if exit_code.is_some_and(|code| code != 0) {
        split_shell_segments(command)
            .last()
            .cloned()
            .filter(|v| !v.is_empty())
    } else {
        None
    }
}

pub(crate) fn is_diagnostic_failure_segment(segment: &str, stdout: &str, stderr: &str) -> bool {
    matches!(
        shell_family(segment, stdout, stderr).as_str(),
        "test" | "build" | "lint" | "python-test" | "go-test"
    ) || segment.contains("--check")
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
    if looks_masked_failure_output(stdout, stderr) {
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
        || !looks_masked_failure_output(stdout, stderr)
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
    if is_repo_inventory_command(command) && exit_code == Some(0) && stderr.trim().is_empty() {
        return None;
    }
    if looks_env_invocation_failure(command, stdout, stderr, exit_code) {
        return None;
    }
    if is_masked_expected_false_or(command, stdout, stderr, exit_code) {
        return None;
    }
    if is_masked_expected_false_pipeline(command, stdout, stderr, exit_code) {
        return None;
    }
    let has_masking_syntax = shell_operator_features(command)
        .iter()
        .any(|feature| matches!(*feature, "pipeline" | "sequence" | "or-list"));
    if !has_masking_syntax {
        return None;
    }
    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    let likely_failure = split_shell_segments(command)
        .iter()
        .any(|segment| is_explicit_false_segment(segment))
        || combined.contains("not found")
        || combined.contains("no such file")
        || combined.contains("permission denied")
        || combined.contains("unrecognized option")
        || combined.contains("invalid option")
        || combined.contains("usage:")
        || combined.contains("error");
    if exit_code == Some(0) || likely_failure {
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
        shell_display_arg(analysis_command.trim(), ShellDisplayQuoteStyle::Posix)
    ))
}

pub(crate) fn first_or_list_lhs(command: &str) -> Option<String> {
    let command = shell_analysis_command(command);
    let mut quote: Option<char> = None;
    let mut chars = command.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if Some(ch) == quote {
            quote = None;
            continue;
        }
        if quote.is_none() && (ch == '\'' || ch == '"') {
            quote = Some(ch);
            continue;
        }
        if quote.is_none() && ch == '|' && chars.peek().is_some_and(|(_, next)| *next == '|') {
            return Some(command[..idx].trim().to_string());
        }
    }
    None
}

pub(crate) fn looks_masked_failure_output(stdout: &str, stderr: &str) -> bool {
    let combined = format!("{stdout}\n{stderr}");
    let lower = combined.to_ascii_lowercase();
    !combined.trim().is_empty()
        && (looks_diagnostic(&combined)
            || lower.contains("not found")
            || lower.contains("no such file")
            || lower.contains("permission denied")
            || lower.contains("unrecognized option")
            || lower.contains("invalid option")
            || lower.contains("usage:"))
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

pub(crate) fn raw_shell_operator_features(command: &str) -> Vec<&'static str> {
    let mut features = Vec::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote != Some('\'') && ch == '\\' {
            escaped = true;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            continue;
        }
        if quote.is_some() {
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        match ch {
            '&' if chars.peek() == Some(&'&') => {
                push_unique_feature(&mut features, "and-list");
                chars.next();
            }
            '|' if chars.peek() == Some(&'|') => {
                push_unique_feature(&mut features, "or-list");
                chars.next();
            }
            '|' => push_unique_feature(&mut features, "pipeline"),
            ';' => push_unique_feature(&mut features, "sequence"),
            '>' | '<' => push_unique_feature(&mut features, "redirect"),
            '$' if chars.peek() == Some(&'(') => {
                push_unique_feature(&mut features, "subshell");
                chars.next();
            }
            '`' => push_unique_feature(&mut features, "subshell"),
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
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut start = 0usize;
    let mut chars = command.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote != Some('\'') && ch == '\\' {
            escaped = true;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            continue;
        }
        if quote.is_some() {
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        let split_len = match ch {
            '&' if chars.peek().is_some_and(|(_, next)| *next == '&') => Some(2),
            '|' if chars.peek().is_some_and(|(_, next)| *next == '|') => Some(2),
            '|' | ';' => Some(1),
            _ => None,
        };
        if let Some(split_len) = split_len {
            push_shell_segment(&mut segments, &command[start..idx]);
            if split_len == 2 {
                chars.next();
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
    let first = words
        .first()
        .map(|word| shell_command_basename(word))
        .unwrap_or_default();
    if matches!(first.as_str(), "sh" | "bash" | "zsh") {
        if let Some(command) = shell_c_command_argument(words) {
            return Some(command);
        }
    }
    if first == "cmd" {
        if let Some(command) = cmd_command_argument(words) {
            return Some(command);
        }
    }
    if matches!(first.as_str(), "powershell" | "pwsh") {
        if let Some(command) = powershell_command_argument(words) {
            return Some(command);
        }
    }
    if first == "env" {
        if let Some(command) = env_split_string_analysis_command(words) {
            return Some(command);
        }
        let command_index = env_wrapped_command_index(words)?;
        return shell_analysis_command_from_words(&words[command_index..]);
    }
    None
}

pub(crate) fn cmd_command_argument(words: &[String]) -> Option<String> {
    let mut index = 1usize;
    while index < words.len() {
        let word = words[index].as_str();
        let lower = word.to_ascii_lowercase();
        if matches!(lower.as_str(), "/c" | "/k") {
            return shell_command_tail(words, index + 1, ShellDisplayQuoteStyle::Cmd);
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
            return shell_command_tail(words, index + 1, ShellDisplayQuoteStyle::PowerShell);
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
    style: ShellDisplayQuoteStyle,
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

pub(crate) fn is_powershell_option_with_value(lower: &str) -> bool {
    matches!(
        lower,
        "-configurationname"
            | "-executionpolicy"
            | "-inputformat"
            | "-outputformat"
            | "-settingsfile"
            | "-version"
            | "-windowstyle"
            | "-workingdirectory"
    )
}

pub(crate) fn is_powershell_inline_option_with_value(lower: &str) -> bool {
    lower.starts_with("-configurationname:")
        || lower.starts_with("-executionpolicy:")
        || lower.starts_with("-inputformat:")
        || lower.starts_with("-outputformat:")
        || lower.starts_with("-settingsfile:")
        || lower.starts_with("-version:")
        || lower.starts_with("-windowstyle:")
        || lower.starts_with("-workingdirectory:")
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
        if is_env_assignment(word) || is_env_no_arg_option(word) {
            index += 1;
            continue;
        }
        if is_env_arg_option(word) {
            index += 2;
            continue;
        }
        if is_env_inline_arg_option(word) {
            index += 1;
            continue;
        }
        if word.starts_with('-') {
            return None;
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
        if is_env_assignment(word) || is_env_no_arg_option(word) {
            index += 1;
            continue;
        }
        if is_env_arg_option(word) {
            index += 2;
            continue;
        }
        if is_env_inline_arg_option(word) {
            index += 1;
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

pub(crate) fn is_env_no_arg_option(word: &str) -> bool {
    matches!(
        word,
        "-i" | "--ignore-environment" | "-0" | "--null" | "--debug"
    )
}

pub(crate) fn is_env_arg_option(word: &str) -> bool {
    matches!(
        word,
        "-u" | "--unset" | "-C" | "--chdir" | "--argv0" | "-S" | "--split-string"
    )
}

pub(crate) fn is_env_inline_arg_option(word: &str) -> bool {
    word.starts_with("--unset=")
        || word.starts_with("--chdir=")
        || word.starts_with("--argv0=")
        || word.starts_with("--split-string=")
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
