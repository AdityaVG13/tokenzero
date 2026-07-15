use crate::shell_parse::split_shell_segments;
use crate::*;

#[derive(Default)]
pub(crate) struct InventoryStats<'a> {
    pub files: usize,
    pub dirs: usize,
    pub line_counts: Vec<&'a str>,
    pub paths: Vec<&'a str>,
}

pub(crate) fn inventory_stats<'a>(
    output: &'a str,
    sample_limit: usize,
    is_file: impl Fn(&str) -> bool,
) -> InventoryStats<'a> {
    let mut stats = InventoryStats::default();
    for line in output.lines().map(str::trim) {
        if line.is_empty() || line.starts_with("===") || line.starts_with("---") {
            continue;
        }
        if line.ends_with('/') {
            stats.dirs += 1;
        } else if is_file(line) {
            stats.files += 1;
            if stats.paths.len() < sample_limit {
                stats.paths.push(line);
            }
        }
        if line
            .split_whitespace()
            .next()
            .is_some_and(|value| value.parse::<usize>().is_ok())
        {
            stats.line_counts.push(line);
        }
    }
    stats
}

pub fn is_repo_inventory_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let inventory_shape =
        (lower.contains("find ") || lower.contains(" tree") || lower.starts_with("tree"))
            && (lower.contains("echo") || lower.contains("wc -l") || lower.contains("sort"))
            || lower.contains("find . -type f")
            || lower.contains("get-childitem")
            || lower.contains("gci ")
            || segment_is_bare_ls(&lower);
    inventory_shape && all_segments_inventory_safe(command)
}

fn segment_is_bare_ls(lower: &str) -> bool {
    split_shell_segments(lower)
        .iter()
        .any(|segment| segment == "ls" || segment.starts_with("ls -") || segment.starts_with("ls "))
}

const INVENTORY_COMMANDS: &[&str] = &[
    "ls",
    "find",
    "tree",
    "dir",
    "gci",
    "get-childitem",
    "sort",
    "wc",
    "head",
    "tail",
    "uniq",
    "cut",
    "echo",
    "cat",
    "sort-object",
    "select-object",
    "where-object",
    "measure-object",
];

fn all_segments_inventory_safe(command: &str) -> bool {
    split_shell_segments(command).iter().all(|segment| {
        split_shell_words(segment)
            .first()
            .map(|word| shell_command_basename(word))
            .is_some_and(|first| INVENTORY_COMMANDS.contains(&first.as_str()))
    })
}

pub fn repo_inventory_view(command: &str, output: &str) -> String {
    let stats = inventory_stats(output, 20, |line| line.contains('/') || line.contains('.'));
    let mut out = String::new();
    out.push_str("repo_inventory:\n");
    out.push_str(&format!("command: {command}\n"));
    out.push_str(&format!(
        "files_seen: {}\ndirs_seen: {}\n",
        stats.files, stats.dirs
    ));
    for (label, values, limit) in [
        ("linecount_summary", &stats.line_counts, 12),
        ("sample_paths", &stats.paths, 20),
    ] {
        if values.is_empty() {
            continue;
        }
        out.push_str(label);
        out.push_str(":\n");
        for value in values.iter().take(limit) {
            out.push_str(&format!("- {value}\n"));
        }
    }
    out
}

pub fn structured_shell_view(command: &str, stdout: &str, stderr: &str) -> String {
    let combined = format!("{stdout}\n{stderr}");
    if is_repo_inventory_command(command) {
        return repo_inventory_view(command, &combined);
    }
    if is_search_shell_command(command) {
        return search_shell_view(stdout, stderr);
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        let mut out = String::from("json_summary:\n");
        match value {
            serde_json::Value::Object(map) => {
                out.push_str(&format!("type: object\nkeys: {}\n", map.len()));
                for (key, value) in map.iter().take(20) {
                    out.push_str(&format!("- {key}: {}\n", json_kind(value)));
                }
            }
            serde_json::Value::Array(items) => {
                out.push_str(&format!("type: array\nitems: {}\n", items.len()));
                for item in items.iter().take(20) {
                    if is_abnormal_json(item) {
                        out.push_str(&format!("- abnormal: {}\n", compact_json(item)));
                    }
                }
            }
            other => out.push_str(&format!("type: {}\n", json_kind(&other))),
        }
        return out;
    }
    if looks_status_table(&combined) {
        let mut out = String::from("status_summary:\n");
        for line in combined.lines().take(80) {
            let lower = line.to_ascii_lowercase();
            if [
                "error",
                "failed",
                "crash",
                "pending",
                "terminating",
                "unhealthy",
            ]
            .iter()
            .any(|needle| lower.contains(needle))
                || line.starts_with("NAME")
            {
                out.push_str(line);
                out.push('\n');
            }
        }
        if out.lines().count() > 1 {
            return out;
        }
    }
    summarize_lines(&combined, 20, 12, "")
}

/// True only when EVERY top-level segment is a search command or a pure
/// line filter. `search_shell_view` labels all stdout lines as matches, so a
/// mixed command like `grep X; ls Y` must never take the search view: the
/// ls output would be presented as grep matches.
pub(crate) fn is_search_shell_command(command: &str) -> bool {
    let segments = split_shell_segments(command);
    let mut any_search = false;
    for segment in &segments {
        let Some(first) = split_shell_words(segment)
            .first()
            .map(|word| shell_command_basename(word))
        else {
            return false;
        };
        if is_search_command(&first) {
            any_search = true;
        } else if !SEARCH_FILTERS.contains(&first.as_str()) {
            return false;
        }
    }
    any_search
}

const SEARCH_FILTERS: &[&str] = &[
    "head", "tail", "sort", "uniq", "wc", "cut", "tr", "cat", "tee",
];

pub(crate) fn is_search_command(command: &str) -> bool {
    const COMMANDS: &[&str] = &["rg", "grep", "egrep", "fgrep", "ag", "ack", "findstr"];
    COMMANDS.contains(&shell_command_basename(command).as_str())
}

pub(crate) fn shell_command_basename(command: &str) -> String {
    let leaf = command.rsplit(['/', '\\']).next().unwrap_or(command);
    let stem = leaf
        .rsplit_once('.')
        .and_then(|(stem, extension)| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "exe" | "cmd" | "bat" | "com"
            )
            .then_some(stem)
        })
        .unwrap_or(leaf);
    stem.to_ascii_lowercase()
}

pub(crate) fn is_search_no_match(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> bool {
    exit_code == Some(1)
        && stdout.trim().is_empty()
        && stderr.trim().is_empty()
        && is_search_shell_command(command)
}

pub(crate) fn is_expected_false_exit(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> bool {
    if exit_code != Some(1) {
        return false;
    }
    if shell_operator_features(command).contains(&"pipeline") {
        return is_expected_false_pipeline_exit(command, stdout, stderr);
    }
    is_expected_false_segment(command, stdout, stderr)
}

fn is_expected_false_pipeline_edge(command: &str, stdout: &str, stderr: &str, first: bool) -> bool {
    let segments = split_shell_segments(command);
    let edge = if first {
        segments.split_first()
    } else {
        segments.split_last()
    };
    edge.is_some_and(|(candidate, others)| {
        is_expected_false_segment(candidate, stdout, stderr)
            && !others
                .iter()
                .any(|segment| is_explicit_false_segment(segment))
    })
}

pub(crate) fn is_expected_false_pipeline_exit(command: &str, stdout: &str, stderr: &str) -> bool {
    is_expected_false_pipeline_edge(command, stdout, stderr, false)
}

pub(crate) fn is_expected_false_segment(command: &str, stdout: &str, stderr: &str) -> bool {
    if !stderr.trim().is_empty() {
        return false;
    }
    let command = shell_analysis_command(command);
    let words = split_shell_words(&command);
    let first = words
        .first()
        .map(|word| shell_command_basename(word))
        .unwrap_or_default();
    match first.as_str() {
        "test" | "[" | "[[" => stdout.trim().is_empty(),
        command if is_search_command(command) => stdout.trim().is_empty(),
        "cmp" | "diff" => true,
        "git" => {
            let Some(subcommand_index) = git_subcommand_index(&words) else {
                return false;
            };
            let is_diff = words
                .get(subcommand_index)
                .is_some_and(|word| word == "diff");
            let diff_args = &words[subcommand_index + 1..];
            let asks_for_status = diff_args
                .iter()
                .any(|word| word == "--quiet" || word == "--exit-code");
            let check_mode = diff_args.iter().any(|word| word == "--check");
            is_diff && asks_for_status && !check_mode
        }
        _ => false,
    }
}

pub(crate) fn is_masked_expected_false_or(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> bool {
    exit_code == Some(0)
        && first_or_list_lhs(command)
            .filter(|segment| !segment.is_empty())
            .is_some_and(|segment| is_expected_false_segment(&segment, stdout, stderr))
}

pub(crate) fn is_masked_expected_false_pipeline(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> bool {
    exit_code == Some(0)
        && stderr.trim().is_empty()
        && shell_operator_features(command).contains(&"pipeline")
        && is_expected_false_pipeline_edge(command, stdout, stderr, true)
}

pub(crate) fn is_explicit_false_segment(segment: &str) -> bool {
    let lower = segment.to_ascii_lowercase();
    lower == "false" || lower.starts_with("false ")
}

pub(crate) fn git_subcommand_index(words: &[String]) -> Option<usize> {
    let mut index = 1;
    while index < words.len() {
        let word = words[index].as_str();
        if matches!(
            word,
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace"
        ) {
            index += 2;
        } else if word.starts_with('-') {
            index += 1;
        } else {
            return Some(index);
        }
    }
    None
}

pub(crate) fn search_shell_view(stdout: &str, stderr: &str) -> String {
    let matches: Vec<_> = stdout
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect();
    let diagnostics: Vec<_> = stderr
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && looks_critical_line(line))
        .collect();

    let mut out = String::from("search_summary:\n");
    out.push_str(&format!("matches_seen: {}\n", matches.len()));
    if !diagnostics.is_empty() {
        out.push_str("diagnostics:\n");
        for line in diagnostics.iter().take(6) {
            out.push_str(&format!("- {line}\n"));
        }
    }
    if !matches.is_empty() {
        out.push_str("sample_matches:\n");
        for line in matches.iter().take(20) {
            out.push_str(&format!("- {line}\n"));
        }
        if matches.len() > 20 {
            out.push_str(&format!(
                "... omitted {} matches; exact ref available ...\n",
                matches.len() - 20
            ));
        }
    }
    out
}

pub fn diagnostic_shell_view(stdout: &str, stderr: &str, max_visible_tokens: usize) -> String {
    let combined = format!("{stdout}\n{stderr}");
    let critical = critical_lines(&combined, 3);
    let view = if critical.trim().is_empty() {
        summarize_lines(&combined, 16, 12, "")
    } else {
        critical
    };
    enforce_token_budget(&view, max_visible_tokens)
}
