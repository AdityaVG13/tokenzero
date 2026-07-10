//! Field `report_tool_issue` allowlist (wqw.6).
//!
//! Agents report expand/root/shell failures against the primary CodeMode surface
//! name `zero_execute` (and aliases). Rejecting that name forced agents out of
//! the harness. This module owns TokenZero's reportable-name policy and the
//! MCP/CLI entry that records a structured field report.

use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Exact surface / tool names that are not covered by the prefix rules below.
const REPORTABLE_EXACT: &[&str] = &[
    "zero_execute",
    "zero-execute",
    "zerostack",
    "zero_search",
    "zero_describe",
    "execute_code",
    "codemode_search",
    "codemode_describe",
    "read",
    "expand",
    "shell",
    "edit",
    "find",
    "grep",
    "tree",
    "glob",
];

/// Namespaced engine / product prefixes (tightened vs open `zero_` / `tz_` alone).
const REPORTABLE_PREFIXES: &[&str] = &[
    "zero.token.",
    "zero.fs.",
    "zero.graph.",
    "zero.",
    "tz_",
    "tokenzero",
    "fszero",
    "graphzero",
];

/// Returns true if `name` is a reportable tool/surface for field issue reports.
pub fn is_reportable_tool_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if REPORTABLE_EXACT.iter().any(|n| *n == lower) {
        return true;
    }
    // zero_execute / zero_search style (underscore, not dotted namespaces).
    if lower.starts_with("zero_") {
        return true;
    }
    REPORTABLE_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

/// Normalize a reported tool name for storage (trim; keep original casing of body).
pub fn normalize_report_tool_name(name: &str) -> String {
    name.trim().to_string()
}

/// Build the structured field-report payload (no I/O).
pub fn build_tool_issue_report(
    tool: &str,
    summary: &str,
    detail: Option<&str>,
    session_id: Option<&str>,
) -> Result<Value, String> {
    let tool = normalize_report_tool_name(tool);
    if !is_reportable_tool_name(&tool) {
        return Err(format!(
            "tool name not reportable: {tool}. Accepted: zero_execute, zerostack, tz_execute_code, \
             zero.token.*/zero.fs.*/zero.graph.*, and TokenZero tz_* tools. \
             See resource://tokenzero/tools."
        ));
    }
    let summary = summary.trim();
    if summary.is_empty() {
        return Err("summary must be non-empty".into());
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(json!({
        "schema_version": "tokenzero.tool_issue.v1",
        "status": "accepted",
        "tool": tool,
        "summary": summary,
        "detail": detail.unwrap_or("").trim(),
        "session_id": session_id.unwrap_or(""),
        "recorded_at_unix": ts,
        "note": "Field report recorded by TokenZero. Expand/root/shell failures may cite zero_execute.",
    }))
}

fn tool_issue_stem(ts: u64, tool: &str) -> String {
    let safe_tool: String = tool
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("issue-{ts}-{safe_tool}")
}

fn write_unique_report(dir: &Path, stem: &str, text: &str) -> Result<std::path::PathBuf, String> {
    for suffix in 0_u64.. {
        let file_name = if suffix == 0 {
            format!("{stem}.json")
        } else {
            format!("{stem}-{suffix}.json")
        };
        let path = dir.join(file_name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(text.as_bytes())
                    .map_err(|e| format!("write report: {e}"))?;
                return Ok(path);
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(format!("create report: {err}")),
        }
    }
    unreachable!("u64 report suffix space exhausted")
}

/// Persist a field report under the recovery-cache parent `.tokenzero/tool-issues/`.
pub fn record_tool_issue(
    cache_path: &Path,
    tool: &str,
    summary: &str,
    detail: Option<&str>,
    session_id: Option<&str>,
) -> Result<Value, String> {
    let mut report = build_tool_issue_report(tool, summary, detail, session_id)?;
    let dir = cache_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("tool-issues");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create tool-issues dir: {e}"))?;
    let ts = report
        .get("recorded_at_unix")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    let path = write_unique_report(&dir, &tool_issue_stem(ts, tool), &text)?;
    report["report_path"] = json!(path.display().to_string());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn accepts_zero_execute_and_aliases() {
        for name in [
            "zero_execute",
            "Zero_Execute",
            "zerostack",
            "tz_execute_code",
            "execute_code",
            "zero.token.expand",
            "zero.fs.write",
            "zero.graph.blast",
            "tz_expand",
            "tz_shell",
        ] {
            assert!(is_reportable_tool_name(name), "expected reportable: {name}");
        }
    }

    #[test]
    fn rejects_unknown_harness_noise() {
        assert!(!is_reportable_tool_name(""));
        assert!(!is_reportable_tool_name("   "));
        assert!(!is_reportable_tool_name("definitely_not_a_tz_tool_xyz"));
        assert!(!is_reportable_tool_name("Browser"));
        // Open prefixes like bare "fz_" / "gz_" must not accept arbitrary noise.
        assert!(!is_reportable_tool_name("fz_random_noise"));
        assert!(!is_reportable_tool_name("gz_not_a_tool"));
    }

    #[test]
    fn build_report_accepts_zero_execute() {
        let v = build_tool_issue_report(
            "zero_execute",
            "expand returned X0 for fz://blob",
            Some("root=/proj"),
            Some("sess-1"),
        )
        .unwrap();
        assert_eq!(v["status"], "accepted");
        assert_eq!(v["tool"], "zero_execute");
        assert!(v["summary"].as_str().unwrap().contains("expand"));
    }

    #[test]
    fn build_report_rejects_unknown_with_hint() {
        let err = build_tool_issue_report("Browser", "x", None, None).unwrap_err();
        assert!(err.contains("not reportable"), "{err}");
        assert!(err.contains("zero_execute"), "{err}");
    }

    #[test]
    fn record_tool_issue_writes_file() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let report = record_tool_issue(
            &cache,
            "zero_execute",
            "shell timeout orphan",
            Some("wqw.4 field"),
            None,
        )
        .unwrap();
        let path = report["report_path"].as_str().unwrap();
        assert!(std::path::Path::new(path).is_file(), "{path}");
        let body = std::fs::read_to_string(path).unwrap();
        assert!(body.contains("zero_execute"));
        assert!(body.contains("shell timeout"));
    }

    #[test]
    fn colliding_report_names_never_overwrite() {
        let dir = tempdir().unwrap();
        let dotted = tool_issue_stem(123, "zero.token.fetch");
        let underscored = tool_issue_stem(123, "zero_token_fetch");
        assert_eq!(dotted, underscored, "fixture must sanitize to one stem");

        let first = write_unique_report(dir.path(), &dotted, "first").unwrap();
        let second = write_unique_report(dir.path(), &underscored, "second").unwrap();

        assert_ne!(first, second);
        assert_eq!(std::fs::read_to_string(first).unwrap(), "first");
        assert_eq!(std::fs::read_to_string(second).unwrap(), "second");
    }
}
