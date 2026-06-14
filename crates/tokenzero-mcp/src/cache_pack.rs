use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn cache_pack_sources(root: &Path, scope: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let common = [
        "AGENTS.md",
        "CLAUDE.md",
        "GEMINI.md",
        "README.md",
        "Cargo.toml",
        "docs/core.md",
        "docs/mcp.md",
        "docs/command-coverage.md",
    ];
    for rel in common {
        let path = root.join(rel);
        if path.exists() {
            paths.push(path);
        }
    }
    if scope == "agent" || scope == "goal" {
        let goals = root.join("docs/goals");
        if let Ok(entries) = fs::read_dir(goals) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|v| v.to_str()) == Some("md") {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

pub(crate) fn cache_pack_manifest_path(cache_path: &Path, scope: &str) -> PathBuf {
    cache_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("cache-packs")
        .join(format!("{scope}.json"))
}

pub(crate) fn previous_cache_digest(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value
        .get("content_digest")
        .and_then(Value::as_str)
        .map(str::to_string)
}
