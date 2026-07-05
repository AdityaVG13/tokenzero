use crate::shell_parse::split_shell_segments;
use crate::*;

pub fn is_repo_inventory_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let inventory_shape =
        (lower.contains("find ") || lower.contains(" tree") || lower.starts_with("tree"))
            && (lower.contains("echo") || lower.contains("wc -l") || lower.contains("sort"))
            || lower.contains("find . -type f")
            || lower.contains("get-childitem")
            || lower.contains("gci ")
            || segment_is_bare_ls(&lower);
    // The inventory view renders ALL output lines as file paths, so every
    // segment must be a lister or a pure filter; `ls -d x; other-tool run`
    // must not have other-tool's output swallowed as sample paths (and
    // "tools -v" must not match the "ls -" substring at all).
    inventory_shape && all_segments_inventory_safe(command)
}

fn segment_is_bare_ls(lower: &str) -> bool {
    split_shell_segments(lower)
        .iter()
        .any(|segment| segment == "ls" || segment.starts_with("ls -") || segment.starts_with("ls "))
}

fn all_segments_inventory_safe(command: &str) -> bool {
    split_shell_segments(command).iter().all(|segment| {
        split_shell_words(segment)
            .first()
            .map(|word| shell_command_basename(word))
            .is_some_and(|first| {
                matches!(
                    first.as_str(),
                    "ls" | "find"
                        | "tree"
                        | "dir"
                        | "gci"
                        | "get-childitem"
                        | "sort"
                        | "wc"
                        | "head"
                        | "tail"
                        | "uniq"
                        | "cut"
                        | "echo"
                        | "cat"
                        | "sort-object"
                        | "select-object"
                        | "where-object"
                        | "measure-object"
                )
            })
    })
}

pub fn repo_inventory_view(command: &str, output: &str) -> String {
    let mut file_count = 0usize;
    let mut dir_count = 0usize;
    let mut line_count_rows = Vec::new();
    let mut sample_files = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("===") || trimmed.starts_with("---") {
            continue;
        }
        if trimmed.ends_with('/') {
            dir_count += 1;
        } else if trimmed.contains('/') || trimmed.contains('.') {
            file_count += 1;
            if sample_files.len() < 20 {
                sample_files.push(trimmed.to_string());
            }
        }
        if trimmed
            .split_whitespace()
            .next()
            .is_some_and(|v| v.parse::<usize>().is_ok())
        {
            line_count_rows.push(trimmed.to_string());
        }
    }
    let mut out = String::new();
    out.push_str("repo_inventory:\n");
    out.push_str(&format!("command: {command}\n"));
    out.push_str(&format!("files_seen: {file_count}\n"));
    out.push_str(&format!("dirs_seen: {dir_count}\n"));
    if !line_count_rows.is_empty() {
        out.push_str("linecount_summary:\n");
        for row in line_count_rows.iter().take(12) {
            out.push_str(&format!("- {row}\n"));
        }
    }
    if !sample_files.is_empty() {
        out.push_str("sample_paths:\n");
        for file in sample_files {
            out.push_str(&format!("- {file}\n"));
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
            if lower.contains("error")
                || lower.contains("failed")
                || lower.contains("crash")
                || lower.contains("pending")
                || lower.contains("terminating")
                || lower.contains("unhealthy")
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
        } else if !is_search_pipeline_filter(&first) {
            return false;
        }
    }
    any_search
}

fn is_search_pipeline_filter(basename: &str) -> bool {
    matches!(
        basename,
        "head" | "tail" | "sort" | "uniq" | "wc" | "cut" | "tr" | "cat" | "tee"
    )
}

pub(crate) fn is_search_command(command: &str) -> bool {
    matches!(
        shell_command_basename(command).as_str(),
        "rg" | "grep" | "egrep" | "fgrep" | "ag" | "ack" | "findstr"
    )
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

pub(crate) fn is_expected_false_pipeline_exit(command: &str, stdout: &str, stderr: &str) -> bool {
    let segments = split_shell_segments(command);
    let Some((last, upstream)) = segments.split_last() else {
        return false;
    };
    is_expected_false_segment(last, stdout, stderr)
        && !upstream
            .iter()
            .any(|segment| is_explicit_false_segment(segment))
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
    if exit_code != Some(0) {
        return false;
    }
    first_or_list_lhs(command)
        .filter(|segment| !segment.is_empty())
        .is_some_and(|segment| is_expected_false_segment(&segment, stdout, stderr))
}

pub(crate) fn is_masked_expected_false_pipeline(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
) -> bool {
    if exit_code != Some(0) || !stderr.trim().is_empty() {
        return false;
    }
    let features = shell_operator_features(command);
    if !features.contains(&"pipeline") {
        return false;
    }
    let segments = split_shell_segments(command);
    let Some((first, rest)) = segments.split_first() else {
        return false;
    };
    is_expected_false_segment(first, stdout, stderr)
        && !rest
            .iter()
            .any(|segment| is_explicit_false_segment(segment))
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
    let mut matches = Vec::new();
    let mut diagnostics = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            matches.push(trimmed);
        }
    }
    for line in stderr.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if looks_critical_line(trimmed) {
            diagnostics.push(trimmed);
        }
    }

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
