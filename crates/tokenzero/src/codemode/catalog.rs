//! Progressive discovery catalog for CodeMode methods.

use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize)]
struct MethodDef {
    path: &'static str,
    connector: &'static str,
    description: &'static str,
    signature: &'static str,
}

const METHOD_CATALOG: &[MethodDef] = &[
    MethodDef {
        path: "zero.read",
        connector: "zero",
        description: "Read file(s) with token-budget capsule compression and exact recovery refs",
        signature: "zero.read(path: string | string[], opts?: { mode?, start_line?, end_line?, max_visible_tokens? }): Promise<{ text: string, ref: string, visible_tokens: number, raw_tokens: number }>",
    },
    MethodDef {
        path: "zero.find",
        connector: "zero",
        description: "Search file contents for a pattern (regex or literal) with compact results",
        signature: "zero.find(pattern: string, path?: string | string[], opts?: { mode?, max_files?, max_visible_tokens? }): Promise<{ text: string, ref: string, status: string, visible_tokens?: number, raw_tokens?: number }>",
    },
    MethodDef {
        path: "zero.grep",
        connector: "zero",
        description: "Exact literal substring search (no regex interpretation)",
        signature: "zero.grep(pattern: string, path?: string | string[], opts?: { mode?, max_files?, max_visible_tokens? }): Promise<{ text: string, ref: string, status: string, visible_tokens?: number, raw_tokens?: number }>",
    },
    MethodDef {
        path: "zero.glob",
        connector: "zero",
        description: "List file paths matching a glob pattern (no file contents)",
        signature: "zero.glob(pattern: string, path?: string | string[], opts?: { max_files? }): Promise<{ text: string, ref: string, status: string, visible_tokens?: number, raw_tokens?: number }>",
    },
    MethodDef {
        path: "zero.tree",
        connector: "zero",
        description: "Inspect a bounded directory tree for orientation",
        signature: "zero.tree(path?: string, opts?: { depth?, include_hidden?, max_files? }): Promise<{ text: string, ref: string }>",
    },
    MethodDef {
        path: "zero.shell",
        connector: "zero",
        description: "Run a shell command with status-truth telemetry and compact output",
        signature: "zero.shell(command: string, opts?: { cwd?, mode?, timeout_seconds? }): Promise<{ text: string, ref: string, exit_code: number, success: boolean }>",
    },
    MethodDef {
        path: "zero.edit",
        connector: "zero",
        description: "Apply multi-hunk find/replace edits to one file atomically",
        signature: "zero.edit(path: string, edits: Array<{ find: string, replace: string, replace_all?: boolean }>, opts?: { dry_run?, create? }): Promise<{ text: string, ref: string, hunks_applied: number }>",
    },
    MethodDef {
        path: "zero.token.expand",
        connector: "zero.token",
        description: "Recover exact bytes from a tz:// ref",
        signature: "zero.token.expand(ref: string, opts?: { start_line?, end_line?, selector? }): Promise<{ text: string, status: string, ref?: string, visible_tokens?: number, raw_tokens?: number }>",
    },
    MethodDef {
        path: "zero.token.compact",
        connector: "zero.token",
        description: "Store arbitrary text/data behind a content-addressed tz://blob ref",
        signature: "zero.token.compact(data: string): Promise<{ ref: string, raw_tokens: number }>",
    },
    MethodDef {
        path: "zero.expand",
        connector: "zero",
        description: "Recover exact bytes from a tz:// ref (compatibility alias for zero.token.expand)",
        signature: "zero.expand(ref: string, opts?: { start_line?, end_line?, selector? }): Promise<{ text: string, status: string, ref?: string, visible_tokens?: number, raw_tokens?: number }>",
    },
    MethodDef {
        path: "zero.compact",
        connector: "zero",
        description: "Store arbitrary text/data behind a content-addressed tz://blob ref (compatibility alias for zero.token.compact)",
        signature: "zero.compact(data: string): Promise<{ ref: string, raw_tokens: number }>",
    },
    MethodDef {
        path: "zero.ingest",
        connector: "zero",
        description: "Ingest text into a compact TokenZero capsule with recovery ref",
        signature: "zero.ingest(text: string, opts?: { mode?, source? }): Promise<{ text: string, ref: string, visible_tokens: number, raw_tokens: number }>",
    },
    MethodDef {
        path: "zero.mem",
        connector: "zero",
        description: "Inspect recovery-cache state and statistics",
        signature: "zero.mem(): Promise<{ text: string }>",
    },
    MethodDef {
        path: "codemode.search",
        connector: "codemode",
        description: "Search available methods by keyword",
        signature: "codemode.search(query: string): Promise<{ results: Array<{ path, description, score }> }>",
    },
    MethodDef {
        path: "codemode.describe",
        connector: "codemode",
        description: "Get full TypeScript signature for a method",
        signature: "codemode.describe(path: string): Promise<{ path, description, types: string }>",
    },
];

pub(crate) fn search_catalog(query: &str) -> Value {
    let query_lower = query.to_lowercase();
    let mut results: Vec<(f64, &MethodDef)> = METHOD_CATALOG
        .iter()
        .filter_map(|m| {
            let haystack = format!("{} {} {}", m.path, m.description, m.signature).to_lowercase();
            let score = if m.path.to_lowercase().contains(&query_lower) {
                1.0
            } else if m.description.to_lowercase().contains(&query_lower) {
                0.7
            } else if haystack.contains(&query_lower) {
                0.4
            } else {
                return None;
            };
            Some((score, m))
        })
        .collect();
    results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    json!({
        "results": results.iter().map(|(score, m)| json!({
            "path": m.path,
            "connector": m.connector,
            "description": m.description,
            "score": score,
        })).collect::<Vec<_>>(),
        "total": results.len(),
        "truncated": false,
    })
}

pub(crate) fn describe_method(path: &str) -> Value {
    let path_lower = path.to_lowercase();
    if let Some(m) = METHOD_CATALOG
        .iter()
        .find(|m| m.path.to_lowercase() == path_lower)
    {
        json!({
            "path": m.path,
            "description": m.description,
            "types": m.signature,
            "kind": "method",
        })
    } else {
        json!({
            "path": path,
            "error": format!("no method found for path: {path}"),
            "available": METHOD_CATALOG.iter().map(|m| m.path).collect::<Vec<_>>(),
        })
    }
}
