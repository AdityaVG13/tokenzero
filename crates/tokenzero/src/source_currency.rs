use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

struct CompetitorSource {
    tool: &'static str,
    url: &'static str,
    source_commit: &'static str,
    claimed_scope: &'static str,
    issue_pr_themes: &'static [&'static str],
    strengths: &'static [&'static str],
    gaps: &'static [&'static str],
}

pub(crate) struct CompetitorAdapterSource {
    pub(crate) tool: &'static str,
    pub(crate) url: &'static str,
    pub(crate) source_commit: &'static str,
}

const COMPETITOR_SOURCE_DATE: &str = "2026-06-04";

const COMPETITOR_SOURCES: &[CompetitorSource] = &[
    CompetitorSource {
        tool: "rtk",
        url: "https://github.com/rtk-ai/rtk",
        source_commit: "snapshot-20260604",
        claimed_scope: "Rust command proxy and shell-hook command filters",
        issue_pr_themes: &["parser coverage", "install/platform", "deps/CI", "reach"],
        strengths: &["command-filter mindshare", "large adoption signal"],
        gaps: &[
            "exact recovery beyond command filters",
            "Safe Savings evidence",
        ],
    },
    CompetitorSource {
        tool: "ztk",
        url: "https://github.com/codejunkie99/ztk",
        source_commit: "snapshot-20260604",
        claimed_scope: "Tiny Zig binary with hooks and manual raw mode",
        issue_pr_themes: &["stderr preservation", "failure fidelity"],
        strengths: &["small binary", "manual raw mode"],
        gaps: &["diagnostic invariants", "host reach evidence"],
    },
    CompetitorSource {
        tool: "lean-ctx",
        url: "https://github.com/yvgude/lean-ctx",
        source_commit: "snapshot-20260604",
        claimed_scope: "Context OS with MCP tools, read modes, memory, routing, and dashboard",
        issue_pr_themes: &["Windows Codex compression gap", "signature map"],
        strengths: &["MCP breadth", "context routing"],
        gaps: &["daemonless core proof", "exact runtime contract"],
    },
    CompetitorSource {
        tool: "tokenpak",
        url: "https://github.com/tokenpak/tokenpak",
        source_commit: "snapshot-20260604",
        claimed_scope: "Local HTTP proxy with compression recipes, cost tracking, and routing",
        issue_pr_themes: &[
            "coverage gate mismatch",
            "pricing bootstrap",
            "model leak",
            "stream aborts",
        ],
        strengths: &["cost tracking", "provider routing"],
        gaps: &["proxy default", "provider path fragility"],
    },
    CompetitorSource {
        tool: "tokenjuice",
        url: "https://github.com/vincentkoc/tokenjuice",
        source_commit: "snapshot-20260604",
        claimed_scope: "Deterministic terminal output compaction and broad integrations",
        issue_pr_themes: &["dependency maintenance", "adapter breadth"],
        strengths: &["deterministic output compaction", "host adapter breadth"],
        gaps: &[
            "exact recovery proof",
            "recovery-adjusted benchmark metrics",
        ],
    },
    CompetitorSource {
        tool: "context-mode",
        url: "https://github.com/mksglu/context-mode",
        source_commit: "snapshot-20260604",
        claimed_scope: "MCP sandbox, search/index, FTS knowledge base, and high reduction summaries",
        issue_pr_themes: &[
            "concurrency flood guard",
            "cwd bug",
            "install manifests",
            "platform beta coverage",
        ],
        strengths: &["MCP search/index breadth", "sandbox model"],
        gaps: &["no pre-index default", "shell truth", "one-shot adequacy"],
    },
    CompetitorSource {
        tool: "caveman",
        url: "https://github.com/JuliusBrussee/caveman",
        source_commit: "snapshot-20260604",
        claimed_scope: "Claude skill and terse agent communication style",
        issue_pr_themes: &[
            "install/platform failures",
            "Node removal",
            "OpenCode",
            "PowerShell",
        ],
        strengths: &["social spread", "prompt-style compression"],
        gaps: &[
            "runtime proof system",
            "exact recovery",
            "protected-anchor recall",
        ],
    },
    CompetitorSource {
        tool: "cavekit",
        url: "https://github.com/JuliusBrussee/cavekit",
        source_commit: "snapshot-20260604",
        claimed_scope: "Natural-language blueprint and planning validation",
        issue_pr_themes: &["manual install", "Codex port", "security", "CLI support"],
        strengths: &["planning flow", "blueprint validation"],
        gaps: &["runtime compression", "exact refs"],
    },
    CompetitorSource {
        tool: "cavemem",
        url: "https://github.com/JuliusBrussee/cavemem",
        source_commit: "snapshot-20260604",
        claimed_scope: "Cross-agent compressed local memory",
        issue_pr_themes: &[
            "Windows paths",
            "Codex installer",
            "session leakage",
            "sqlite backend",
        ],
        strengths: &["cross-agent memory", "local compression"],
        gaps: &["daemon default avoidance", "source/context boundaries"],
    },
    CompetitorSource {
        tool: "caveman-code",
        url: "https://github.com/JuliusBrussee/caveman-code",
        source_commit: "snapshot-20260604",
        claimed_scope: "Agent/code integration layer",
        issue_pr_themes: &["install", "login", "provider failures"],
        strengths: &["code-agent integration"],
        gaps: &["host-integration failure evidence", "recovery proof"],
    },
    CompetitorSource {
        tool: "headroom",
        url: "https://github.com/chopratejas/headroom",
        source_commit: "snapshot-20260604",
        claimed_scope: "Library/proxy/MCP compression with retrieval store",
        issue_pr_themes: &[
            "provider-agnostic proxy",
            "timeout",
            "telemetry",
            "Homebrew",
            "RTK hook",
        ],
        strengths: &["library/proxy/MCP surface", "retrieval store"],
        gaps: &["no-proxy default", "exact local runtime"],
    },
    CompetitorSource {
        tool: "engram",
        url: "https://github.com/pythondatascrape/engram",
        source_commit: "snapshot-20260604",
        claimed_scope: "Local daemon for conversation history compression",
        issue_pr_themes: &["identity compression compatibility"],
        strengths: &["conversation history compression"],
        gaps: &["daemon default", "on-demand cache packs"],
    },
    CompetitorSource {
        tool: "claw",
        url: "https://github.com/open-compress/claw-compactor",
        source_commit: "snapshot-20260604",
        claimed_scope: "Reversible multi-stage compression and RewindStore",
        issue_pr_themes: &["credential exposure reports", "RewindStore persistence"],
        strengths: &["reversible pipeline", "multi-stage compression"],
        gaps: &["CLI/MCP exact refs", "security gates"],
    },
    CompetitorSource {
        tool: "contextpilot",
        url: "https://github.com/msousa202/ContextPilot",
        source_commit: "snapshot-20260604",
        claimed_scope: "SDK wrapper, proxy, MCP, and quality fallback",
        issue_pr_themes: &[
            "report capsule",
            "session memory",
            "hybrid scoring",
            "skeletonization",
        ],
        strengths: &["quality gate concept", "SDK integration"],
        gaps: &["service dependency", "local exact recovery"],
    },
    CompetitorSource {
        tool: "wilpel-caveman-compression",
        url: "https://github.com/wilpel/caveman-compression",
        source_commit: "snapshot-20260604",
        claimed_scope: "Semantic grammar stripping for prompt compression",
        issue_pr_themes: &[
            "negation preservation",
            "missing Skill.md",
            "endpoint support",
        ],
        strengths: &["semantic grammar stripping"],
        gaps: &["lossy protected-anchor risk", "exact recovery"],
    },
    CompetitorSource {
        tool: "compresh",
        url: "https://github.com/compresh/compresh",
        source_commit: "snapshot-20260604",
        claimed_scope: "Depth-aware proxy context compression and fetch markers",
        issue_pr_themes: &["no active backlog snapshot"],
        strengths: &["depth-aware compression", "fetch markers"],
        gaps: &["local runtime", "recovery-adjusted accounting"],
    },
    CompetitorSource {
        tool: "compresh-mcp",
        url: "https://github.com/compresh/compresh-mcp",
        source_commit: "snapshot-20260604",
        claimed_scope: "MCP wrapper around Compresh and remote/local fallback",
        issue_pr_themes: &["no active backlog snapshot"],
        strengths: &["MCP wrapper", "remote/local fallback"],
        gaps: &["paid/remote default risk", "exact local refs"],
    },
    CompetitorSource {
        tool: "context-gateway",
        url: "https://github.com/Compresr-ai/Context-Gateway",
        source_commit: "snapshot-20260604",
        claimed_scope: "Agentic API gateway with background history compaction",
        issue_pr_themes: &[
            "thinking-block 400 errors",
            "Copilot integration",
            "SSE/header PRs",
        ],
        strengths: &["agentic API gateway", "session history compaction"],
        gaps: &["background compaction default", "Core local recovery"],
    },
];

pub(crate) fn competitor_adapter_sources() -> impl Iterator<Item = CompetitorAdapterSource> {
    COMPETITOR_SOURCES
        .iter()
        .map(|source| CompetitorAdapterSource {
            tool: source.tool,
            url: source.url,
            source_commit: source.source_commit,
        })
}

pub(crate) fn competitor_source_url(tool: &str) -> Option<&'static str> {
    COMPETITOR_SOURCES
        .iter()
        .find(|source| source.tool == tool)
        .map(|source| source.url)
}

pub(crate) fn source_currency_report(release_candidate_id: &str) -> serde_json::Value {
    let rows = competitor_source_rows();
    let pin_audit = source_commit_pin_audit(&rows);
    json!({
        "schema_version": "tokenzero.source_currency.v1",
        "status": "ok",
        "ok": true,
        "release_candidate_id": release_candidate_id,
        "source_date": COMPETITOR_SOURCE_DATE,
        "source_snapshot": "PRD matrix generated 2026-06-03 and spot-checked 2026-06-04",
        "fresh_for_private_planning": true,
        "fresh_for_public_claim": false,
        "public_claims_approved": false,
        "release_publication_allowed": false,
        "blocked_reasons": [
            "source ledger requires same-release-candidate refresh",
            "source ledger requires release-candidate commit pins from refreshed primary sources",
            "public claims require benchmark, recovery, task-success, and approval gates to agree"
        ],
        "source_commit_pin_status": pin_audit["source_commit_pin_status"].clone(),
        "unpinned_source_rows": pin_audit["unpinned_source_rows"].clone(),
        "rows": rows
    })
}

fn competitor_source_rows() -> Vec<serde_json::Value> {
    COMPETITOR_SOURCES
        .iter()
        .map(|source| {
            json!({
                "tool": source.tool,
                "url": source.url,
                "source_date": COMPETITOR_SOURCE_DATE,
                "source_commit": source.source_commit,
                "claimed_scope": source.claimed_scope,
                "issue_pr_themes": source.issue_pr_themes,
                "strengths": source.strengths,
                "gaps": source.gaps,
                "fresh_for_private_planning": true,
                "fresh_for_public_claim": false,
                "freshness_note": "private PRD snapshot only; refresh primary source and pin release-candidate commit before public claim"
            })
        })
        .collect()
}

pub(crate) fn refreshed_source_currency_report(
    refresh_rows: Vec<serde_json::Value>,
    refresh_method: &str,
    refresh_input: Option<&Path>,
    release_candidate_id: &str,
) -> serde_json::Value {
    let mut refresh_by_tool = HashMap::<String, serde_json::Value>::new();
    for row in refresh_rows {
        if let Some(tool) = row["tool"].as_str().filter(|tool| !tool.trim().is_empty()) {
            refresh_by_tool.insert(tool.trim().to_string(), row);
        }
    }

    let mut rows = Vec::<serde_json::Value>::new();
    let mut refresh_errors = Vec::<serde_json::Value>::new();
    for source in COMPETITOR_SOURCES {
        let refresh = refresh_by_tool.get(source.tool);
        let source_commit = refresh
            .and_then(|row| row["source_commit"].as_str())
            .unwrap_or_default()
            .trim()
            .to_string();
        let source_date = refresh
            .and_then(|row| row["source_date"].as_str())
            .unwrap_or(COMPETITOR_SOURCE_DATE)
            .trim()
            .to_string();
        let refresh_error = refresh
            .and_then(|row| row["refresh_error"].as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        if refresh.is_none() {
            refresh_errors.push(json!({
                "tool": source.tool,
                "url": source.url,
                "refresh_error": "missing refreshed source row"
            }));
        } else if let Some(error) = refresh_error.as_deref() {
            refresh_errors.push(json!({
                "tool": source.tool,
                "url": source.url,
                "refresh_error": error
            }));
        }
        let row_fresh = refresh.is_some()
            && refresh_error.is_none()
            && !source_date.is_empty()
            && source_commit_is_release_candidate_pin(&source_commit);
        rows.push(json!({
            "tool": source.tool,
            "url": source.url,
            "source_date": source_date,
            "source_commit": source_commit,
            "claimed_scope": source.claimed_scope,
            "issue_pr_themes": source.issue_pr_themes,
            "strengths": source.strengths,
            "gaps": source.gaps,
            "fresh_for_private_planning": true,
            "fresh_for_public_claim": row_fresh,
            "refresh_method": refresh_method,
            "refresh_error": refresh_error,
            "freshness_note": if row_fresh {
                "release-candidate source pin refreshed from primary source"
            } else {
                "source refresh incomplete; do not use for public claim"
            }
        }));
    }

    let pin_audit = source_commit_pin_audit(&rows);
    let pin_status = &pin_audit["source_commit_pin_status"];
    let all_pinned = pin_status["pinned"].as_u64() == Some(rows.len() as u64)
        && pin_status["missing"].as_u64() == Some(0)
        && pin_status["unpinned"].as_u64() == Some(0);
    let fresh_for_public_claim = all_pinned
        && refresh_errors.is_empty()
        && rows.iter().all(|row| row["fresh_for_public_claim"] == true);
    let blocked_reasons = if fresh_for_public_claim {
        vec![
            "public claims require benchmark, recovery, task-success, OS, adapter, and release approval gates to agree",
        ]
    } else {
        vec![
            "source ledger requires same-release-candidate refresh",
            "source ledger requires release-candidate commit pins from refreshed primary sources",
            "source refresh incomplete",
            "public claims require benchmark, recovery, task-success, and approval gates to agree",
        ]
    };

    json!({
        "schema_version": "tokenzero.source_currency.v1",
        "status": "ok",
        "ok": true,
        "release_candidate_id": release_candidate_id,
        "source_date": source_refresh_date(),
        "source_snapshot": "release-candidate source refresh",
        "refresh_method": refresh_method,
        "refresh_input": refresh_input.map(|path| path.display().to_string()),
        "fresh_for_private_planning": true,
        "fresh_for_public_claim": fresh_for_public_claim,
        "public_claims_approved": false,
        "release_publication_allowed": false,
        "blocked_reasons": blocked_reasons,
        "source_commit_pin_status": pin_audit["source_commit_pin_status"].clone(),
        "unpinned_source_rows": pin_audit["unpinned_source_rows"].clone(),
        "refresh_errors": refresh_errors,
        "rows": rows
    })
}

pub(crate) fn read_source_refresh_rows(path: &Path) -> Result<Vec<serde_json::Value>> {
    let bytes = fs::read(path)
        .with_context(|| format!("reading source refresh ledger {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing source refresh ledger {}", path.display()))?;
    if let Some(rows) = value["rows"].as_array() {
        return Ok(rows.clone());
    }
    if let Some(rows) = value.as_array() {
        return Ok(rows.clone());
    }
    anyhow::bail!("source refresh ledger must be a JSON array or an object with a rows array");
}

pub(crate) fn git_head_source_refresh_rows() -> Vec<serde_json::Value> {
    COMPETITOR_SOURCES
        .iter()
        .map(|source| match git_ls_remote_head(source.url) {
            Ok(source_commit) => json!({
                "tool": source.tool,
                "source_date": source_refresh_date(),
                "source_commit": source_commit
            }),
            Err(error) => json!({
                "tool": source.tool,
                "source_date": source_refresh_date(),
                "source_commit": "",
                "refresh_error": error.to_string()
            }),
        })
        .collect()
}

/// Upper bound for one remote pin lookup; a hung remote must not stall the
/// whole source-currency refresh.
const LS_REMOTE_TIMEOUT: Duration = Duration::from_secs(30);

fn git_ls_remote_head(url: &str) -> Result<String> {
    let argv: Vec<String> = ["git", "ls-remote", url, "HEAD"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let output = tokenzero_runtime::run_command(&argv, None, None, None, LS_REMOTE_TIMEOUT, false)
        .with_context(|| format!("running git ls-remote HEAD for {url}"))?;
    if output.timed_out {
        anyhow::bail!(
            "git ls-remote HEAD timed out after {}s for {url}",
            LS_REMOTE_TIMEOUT.as_secs()
        );
    }
    if !output.ok {
        let stderr = output.stderr.trim();
        anyhow::bail!("git ls-remote HEAD failed for {url}: {stderr}");
    }
    let source_commit = output
        .stdout
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim();
    if !source_commit_is_release_candidate_pin(source_commit) {
        anyhow::bail!("git ls-remote HEAD did not return a commit pin for {url}");
    }
    Ok(source_commit.to_string())
}

fn source_refresh_date() -> String {
    std::env::var("TOKENZERO_SOURCE_REFRESH_DATE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| COMPETITOR_SOURCE_DATE.to_string())
}

fn source_commit_pin_audit(rows: &[serde_json::Value]) -> serde_json::Value {
    let mut pinned = 0usize;
    let mut missing = 0usize;
    let mut unpinned_source_rows = Vec::<serde_json::Value>::new();
    for row in rows {
        let source_commit = row["source_commit"].as_str().unwrap_or_default().trim();
        if source_commit.is_empty() {
            missing += 1;
        } else if source_commit_is_release_candidate_pin(source_commit) {
            pinned += 1;
        } else {
            unpinned_source_rows.push(json!({
                "tool": row["tool"],
                "url": row["url"],
                "source_commit": source_commit
            }));
        }
    }
    json!({
        "source_commit_pin_status": {
            "pinned": pinned,
            "missing": missing,
            "unpinned": unpinned_source_rows.len()
        },
        "unpinned_source_rows": unpinned_source_rows
    })
}

pub(crate) fn source_commit_is_release_candidate_pin(value: &str) -> bool {
    let trimmed = value.trim();
    (7..=64).contains(&trimmed.len()) && trimmed.bytes().all(|byte| byte.is_ascii_hexdigit())
}
