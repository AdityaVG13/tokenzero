use crate::*;

pub(crate) fn should_compact_tiny_shell(
    input: &ShellRenderInput<'_>,
    policy: &PolicyDecision,
    status: &CommandStatus,
) -> bool {
    input.mode.effective_policy() == Mode::Auto
        && policy.policy == "passthrough"
        && status.command_success
        && input.exit_code == Some(0)
        && !input.timed_out
        && input.stderr.trim().is_empty()
        && status.failed_segment.is_none()
        && status.pipeline_masking_warning.is_none()
        && input.stdout.len() <= 512
        && input.stdout.lines().count() <= 8
        && count_tokens(input.stdout) <= 48
}

pub(crate) fn compact_shell_view(stdout: &str) -> String {
    let trimmed = stdout.trim_end();
    if trimmed.is_empty() {
        "ok".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn should_compact_repo_inventory_shell(
    input: &ShellRenderInput<'_>,
    policy: &PolicyDecision,
    status: &CommandStatus,
) -> bool {
    input.mode.effective_policy() == Mode::Auto
        && policy.policy == "structured"
        && is_repo_inventory_command(input.command)
        && status.command_success
        && input.exit_code == Some(0)
        && !input.timed_out
        && input.stderr.trim().is_empty()
        && input.combined_ref.is_some()
        && count_tokens(input.stdout) <= 160
        && input.stdout.lines().count() <= 40
}

pub(crate) fn compact_repo_inventory_view(command: &str, output: &str) -> String {
    let mut file_count = 0usize;
    let mut dir_count = 0usize;
    let mut sample_files = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("===") || trimmed.starts_with("---") {
            continue;
        }
        if trimmed.ends_with('/') {
            dir_count += 1;
        } else if looks_like_inventory_file_path(trimmed) {
            file_count += 1;
            if sample_files.len() < 3 {
                sample_files.push(trimmed.to_string());
            }
        }
    }
    let mut out = String::new();
    out.push_str("repo_inventory\n");
    out.push_str(&format!("files_seen: {file_count}\n"));
    if dir_count > 0 {
        out.push_str(&format!("dirs_seen: {dir_count}\n"));
    }
    if !sample_files.is_empty() {
        out.push_str("sample_paths:\n");
        for file in sample_files {
            out.push_str(&format!("- {}\n", compact_inventory_path(&file)));
        }
    } else if !command.trim().is_empty() {
        out.push_str("sample_paths: none\n");
    }
    out
}

pub(crate) fn compact_repo_inventory_shell_capsule(
    input: &ShellRenderInput<'_>,
    body: &str,
) -> String {
    let mut visible = String::new();
    visible.push_str(body.trim_end());
    if let Some(combined_ref) = input.combined_ref {
        visible.push_str(&format!("\ncombined_ref: {combined_ref}"));
    }
    visible
}

pub(crate) fn looks_like_inventory_file_path(path: &str) -> bool {
    path.contains('/')
        || path.contains('\\')
        || path
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| name.contains('.'))
}

pub(crate) fn compact_inventory_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let normalized = trimmed.replace('\\', "/");
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() <= 2 {
        normalized
    } else {
        parts[parts.len().saturating_sub(2)..].join("/")
    }
}

/// Token budget for the visible body of a verified-success command. Raw bytes
/// always remain recoverable through the exact refs, so success output spends
/// at most this many visible tokens (criticals are exempt and always kept).
pub(crate) const SHELL_SUCCESS_SUMMARY_TOKENS: usize = 200;

pub(crate) fn shell_success_summary_budget(max_visible_tokens: usize) -> usize {
    if max_visible_tokens == 0 {
        SHELL_SUCCESS_SUMMARY_TOKENS
    } else {
        SHELL_SUCCESS_SUMMARY_TOKENS.min(max_visible_tokens)
    }
}

/// Success-noise compaction preconditions: verified success, no timeout, no
/// masked pipeline hazard, and an exact combined ref to recover raw bytes.
pub(crate) fn should_compact_success_noise(
    input: &ShellRenderInput<'_>,
    status: &CommandStatus,
) -> bool {
    input.mode.effective_policy() == Mode::Auto
        && status.command_success
        && !input.timed_out
        && status.failed_segment.is_none()
        && status.pipeline_masking_warning.is_none()
        && input.combined_ref.is_some()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuccessFamily {
    Cargo,
    Pytest,
    NpmInstall,
    GitTransfer,
}

pub(crate) fn success_noise_families(command: &str) -> Vec<SuccessFamily> {
    let mut families = Vec::new();
    for segment in split_shell_segments(command) {
        let words = split_shell_words(&segment);
        let first = words
            .first()
            .map(|word| shell_command_basename(word))
            .unwrap_or_default();
        let family = match first.as_str() {
            "cargo" | "rustc" | "rustup" => Some(SuccessFamily::Cargo),
            "pytest" => Some(SuccessFamily::Pytest),
            "python" | "python3" => segment
                .contains("-m pytest")
                .then_some(SuccessFamily::Pytest),
            "npm" | "pnpm" | "yarn" => {
                let second = words.get(1).map(String::as_str).unwrap_or_default();
                matches!(
                    second,
                    "install" | "ci" | "i" | "add" | "update" | "upgrade" | "audit" | "dedupe"
                )
                .then_some(SuccessFamily::NpmInstall)
            }
            "git" => {
                let sub = git_subcommand_index(&words)
                    .and_then(|index| words.get(index))
                    .map(String::as_str)
                    .unwrap_or_default();
                matches!(
                    sub,
                    "clone" | "fetch" | "pull" | "push" | "gc" | "submodule"
                )
                .then_some(SuccessFamily::GitTransfer)
            }
            _ => None,
        };
        if let Some(family) = family {
            if !families.contains(&family) {
                families.push(family);
            }
        }
    }
    families
}

/// Render a dense success view for known-noisy toolchains: progress and
/// bookkeeping lines collapse into counts while every critical line (and its
/// indented continuation block) is kept verbatim. Returns `None` when the
/// command is not a recognized family or nothing was recognized as noise.
pub(crate) fn success_noise_view(command: &str, stdout: &str, stderr: &str) -> Option<String> {
    let families = success_noise_families(command);
    if families.is_empty() {
        return None;
    }
    let mut compiled = 0usize;
    let mut fresh = 0usize;
    let mut downloaded = 0usize;
    let mut bookkeeping = 0usize;
    let mut tests_ok = 0usize;
    let mut pytest_passed = 0usize;
    let mut git_progress = 0usize;
    let mut finished_in: Option<String> = None;
    let mut summary_lines: Vec<String> = Vec::new();
    let mut kept_lines: Vec<String> = Vec::new();
    let mut other_lines = 0usize;
    let mut kept_other = 0usize;
    let mut in_critical_block = false;
    let mut last_progress: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();

    for raw_line in stdout.lines().chain(stderr.lines()) {
        // Carriage-return progress overwrites itself on a TTY; only the
        // final state carries information.
        let line = raw_line.rsplit('\r').next().unwrap_or(raw_line);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            in_critical_block = false;
            continue;
        }
        // Passing-test markers outrank the critical-keyword scan: test NAMES
        // routinely contain words like "warning" or "failure" while the line
        // itself only certifies a pass. The strict shapes ("test <name> ...
        // ok" / "<id> PASSED") cannot carry failure evidence.
        let pass_marker = families.iter().find_map(|family| match family {
            SuccessFamily::Cargo if is_cargo_test_ok_line(trimmed) => Some(SuccessFamily::Cargo),
            SuccessFamily::Pytest if is_pytest_pass_marker(trimmed) => Some(SuccessFamily::Pytest),
            _ => None,
        });
        if let Some(marker_family) = pass_marker {
            match marker_family {
                SuccessFamily::Cargo => tests_ok += 1,
                _ => pytest_passed += 1,
            }
            in_critical_block = false;
            continue;
        }
        if looks_critical_line(line) {
            in_critical_block = true;
            kept_lines.push(line.to_string());
            continue;
        }
        // Noise classification outranks diagnostic continuation: toolchains
        // indent bookkeeping lines (cargo's "   Compiling ..."), which must
        // end a critical block rather than be swallowed into it.
        let mut classified = false;
        for family in &families {
            match family {
                SuccessFamily::Cargo => {
                    if let Some(rest) = trimmed.strip_prefix("Finished ") {
                        finished_in = rest.rsplit_once(" in ").map(|(_, t)| t.to_string());
                        classified = true;
                    } else if trimmed.starts_with("Compiling ")
                        || trimmed.starts_with("Checking ")
                        || trimmed.starts_with("Documenting ")
                    {
                        compiled += 1;
                        classified = true;
                    } else if trimmed.starts_with("Fresh ") {
                        fresh += 1;
                        classified = true;
                    } else if trimmed.starts_with("Downloaded ")
                        || trimmed.starts_with("Downloading ")
                    {
                        downloaded += 1;
                        classified = true;
                    } else if trimmed.starts_with("Updating ")
                        || trimmed.starts_with("Locking ")
                        || trimmed.starts_with("Adding ")
                        || trimmed.starts_with("Removing ")
                        || trimmed.starts_with("Installing ")
                        || trimmed.starts_with("Blocking ")
                        || trimmed.starts_with("Building ")
                        || trimmed.starts_with("Running ")
                        || trimmed.starts_with("Doc-tests ")
                        || (trimmed.starts_with("running ") && trimmed.ends_with("tests"))
                        || trimmed == "running 1 test"
                    {
                        bookkeeping += 1;
                        classified = true;
                    } else if is_cargo_test_ok_line(trimmed) {
                        tests_ok += 1;
                        classified = true;
                    } else if trimmed.starts_with("test result:") {
                        summary_lines.push(line.to_string());
                        classified = true;
                    }
                }
                SuccessFamily::Pytest => {
                    if is_pytest_summary_line(trimmed) {
                        summary_lines.push(
                            trimmed
                                .trim_matches(|c: char| c == '=' || c == ' ')
                                .to_string(),
                        );
                        classified = true;
                    } else if is_pytest_noise_line(trimmed) {
                        if trimmed.ends_with("PASSED")
                            || trimmed.contains(" PASSED ")
                            || trimmed.contains("::")
                        {
                            pytest_passed += 1;
                        } else {
                            bookkeeping += 1;
                        }
                        classified = true;
                    }
                }
                SuccessFamily::NpmInstall => {
                    if is_npm_summary_line(trimmed) {
                        summary_lines.push(trimmed.to_string());
                        classified = true;
                    } else if is_npm_noise_line(trimmed) {
                        bookkeeping += 1;
                        classified = true;
                    }
                }
                SuccessFamily::GitTransfer => {
                    if let Some(prefix) = git_progress_prefix(trimmed) {
                        git_progress += 1;
                        last_progress.insert(prefix.to_string(), line.to_string());
                        classified = true;
                    }
                }
            }
            if classified {
                break;
            }
        }
        if classified {
            in_critical_block = false;
            continue;
        }
        if in_critical_block && is_critical_continuation_line(line) {
            kept_lines.push(line.to_string());
            continue;
        }
        in_critical_block = false;
        other_lines += 1;
        if kept_other < 24 {
            kept_lines.push(line.to_string());
            kept_other += 1;
        }
    }

    let collapsed =
        compiled + fresh + downloaded + bookkeeping + tests_ok + pytest_passed + git_progress;
    if collapsed == 0 && summary_lines.is_empty() && finished_in.is_none() {
        return None;
    }

    let mut header_parts: Vec<String> = Vec::new();
    if compiled > 0 {
        header_parts.push(format!("{compiled} compiled"));
    }
    if fresh > 0 {
        header_parts.push(format!("{fresh} fresh"));
    }
    if downloaded > 0 {
        header_parts.push(format!("{downloaded} downloaded"));
    }
    if tests_ok > 0 {
        header_parts.push(format!("{tests_ok} tests ok"));
    }
    if pytest_passed > 0 {
        header_parts.push(format!("{pytest_passed} passed"));
    }
    if git_progress > 0 {
        header_parts.push(format!("{git_progress} progress lines"));
    }
    if bookkeeping > 0 {
        header_parts.push(format!("{bookkeeping} bookkeeping"));
    }
    let tool = match families.first() {
        Some(SuccessFamily::Cargo) => "cargo",
        Some(SuccessFamily::Pytest) => "pytest",
        Some(SuccessFamily::NpmInstall) => "npm",
        Some(SuccessFamily::GitTransfer) => "git",
        None => "tool",
    };
    let mut out = String::new();
    out.push_str(tool);
    out.push_str(" ok");
    if let Some(time) = finished_in.as_deref() {
        out.push_str(" in ");
        out.push_str(time);
    }
    if !header_parts.is_empty() {
        out.push_str(": ");
        out.push_str(&header_parts.join(", "));
    }
    out.push_str(" [collapsed]");
    for line in &summary_lines {
        out.push('\n');
        out.push_str(line);
    }
    for line in last_progress.values() {
        out.push('\n');
        out.push_str(line);
    }
    for line in &kept_lines {
        out.push('\n');
        out.push_str(line);
    }
    if other_lines > kept_other {
        out.push_str(&format!(
            "\n... +{} more lines; exact ref available ...",
            other_lines.saturating_sub(kept_other)
        ));
    }
    Some(out)
}
