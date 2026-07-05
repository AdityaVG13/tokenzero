use crate::render::domain::{
    git_subcommand_index, is_repo_inventory_command, is_search_shell_command,
    shell_command_basename,
};
use crate::shell_parse::{
    looks_diagnostic, looks_status_table, shell_analysis_command, split_shell_words,
};

pub fn shell_family(command: &str, stdout: &str, stderr: &str) -> String {
    let analysis_command = shell_analysis_command(command);
    let first_words = split_shell_words(&analysis_command);
    let first = first_words
        .first()
        .map(|word| shell_command_basename(word))
        .unwrap_or_default();
    let git_subcommand = (first == "git")
        .then(|| git_subcommand_index(&first_words))
        .flatten();
    let second = git_subcommand
        .and_then(|index| first_words.get(index))
        .or_else(|| first_words.get(1))
        .map(String::as_str)
        .unwrap_or_default();
    let combined = format!("{stdout}\n{stderr}");
    if is_repo_inventory_command(command) || is_repo_inventory_command(&analysis_command) {
        return "repo-inventory".to_string();
    }
    if first == "diff"
        || first == "git" && matches!(second, "diff" | "show")
        || combined.starts_with("diff --git")
        || combined.contains("\n@@ ")
    {
        return "diff".to_string();
    }
    if matches!(first.as_str(), "test" | "[" | "[[" | "cmp") {
        return "predicate".to_string();
    }
    if first == "cargo" && matches!(second, "test" | "build" | "check" | "clippy") {
        return if second == "test" { "test" } else { "build" }.to_string();
    }
    if is_search_shell_command(command) {
        return "search".to_string();
    }
    if first == "pytest"
        || first == "unittest"
        || command.contains("python -m pytest")
        || command.contains("python -m unittest")
    {
        return "python-test".to_string();
    }
    if first == "go" && second == "test" {
        return "go-test".to_string();
    }
    if matches!(first.as_str(), "jest" | "vitest")
        || matches!(first.as_str(), "npm" | "pnpm" | "yarn") && second == "test"
    {
        return "test".to_string();
    }
    if matches!(
        first.as_str(),
        "eslint" | "tsc" | "ruff" | "mypy" | "clippy"
    ) {
        return "lint".to_string();
    }
    if matches!(first.as_str(), "docker" | "kubectl") || looks_status_table(&combined) {
        return "status".to_string();
    }
    if serde_json::from_str::<serde_json::Value>(stdout.trim()).is_ok()
        || combined.contains("<testsuite")
        || combined
            .lines()
            .any(|l| l.starts_with("ok ") || l.starts_with("not ok "))
    {
        return "structured".to_string();
    }
    if looks_diagnostic(&combined) {
        return "diagnostic".to_string();
    }
    "generic".to_string()
}
