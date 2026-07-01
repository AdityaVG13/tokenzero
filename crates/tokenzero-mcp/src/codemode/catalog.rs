//! Progressive discovery catalog for CodeMode methods.

use serde::Serialize;
use serde_json::{Value, json};

use super::store::CodeModeLimits;

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
        description: "Store arbitrary text/data behind a tz:// recovery ref via ingest",
        signature: "zero.token.compact(data: string): Promise<{ ref: string, raw_tokens: number }>",
    },
    MethodDef {
        path: "zero.token.compactMany",
        connector: "zero.token",
        description: "Batch compact many payloads in one CodeMode step with one visible ack",
        signature: "zero.token.compactMany(items: Array<string | any>): Promise<{ items: Array<{ ref: string }>, refs: string[], count: number }>",
    },
    MethodDef {
        path: "zero.token.expandMany",
        connector: "zero.token",
        description: "Batch expand many tz:// refs in one CodeMode step",
        signature: "zero.token.expandMany(refs: string[]): Promise<{ items: Array<{ text: string }>, count: number }>",
    },
    MethodDef {
        path: "zero.token.dedupe",
        connector: "zero.token",
        description: "Deduplicate JSON/string values while preserving first occurrence order",
        signature: "zero.token.dedupe(items: any[]): Promise<{ items: any[], count: number }>",
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
        description: "Store arbitrary text/data behind a tz:// recovery ref via ingest (compatibility alias for zero.token.compact)",
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
        path: "zero.recall",
        connector: "zero",
        description: "Search payloads already stored in the recovery cache",
        signature: "zero.recall(query: string, opts?: { max_hits?, mode?, max_visible_tokens? }): Promise<{ text: string, ref: string, status: string }>",
    },
    MethodDef {
        path: "zero.fetch",
        connector: "zero",
        description: "Fetch an http(s) URL via curl with TTL cache and exact refs",
        signature: "zero.fetch(url: string, opts?: { ttl_seconds?, fresh?, mode?, max_visible_tokens? }): Promise<{ text: string, ref: string, status: string }>",
    },
    MethodDef {
        path: "zero.cache_pack",
        connector: "zero",
        description: "Build a daemonless prompt-cache pack with stable prefix and volatile refs",
        signature: "zero.cache_pack(opts?: { scope? }): Promise<{ text: string, ref: string }>",
    },
    MethodDef {
        path: "zero.rewrite",
        connector: "zero",
        description: "Plan a conservative shell command rewrite without executing it",
        signature: "zero.rewrite(command: string, opts?: { mode? }): Promise<{ text: string }>",
    },
    MethodDef {
        path: "zero.discover",
        connector: "zero",
        description: "Report TokenZero filter and runtime readiness metadata",
        signature: "zero.discover(): Promise<{ filters: object, runtime: object }>",
    },
    MethodDef {
        path: "zero.batch",
        connector: "zero",
        description: "Run several independent TokenZero ops in one step (max 16)",
        signature: "zero.batch(ops: Array<{ tool: string, args: object }>): Promise<{ text: string, refs: string[] }>",
    },
    MethodDef {
        path: "zero.pipe",
        connector: "zero",
        description: "Execute a sequence of operations with result threading (_prev auto-binding)",
        signature: "zero.pipe(steps: Array<{ method: string, args?: any[] }>): Promise<{ steps: number, results: any[], last: any }>",
    },
    MethodDef {
        path: "zero.pick",
        connector: "zero",
        description: "Extract specific keys from an object value",
        signature: "zero.pick(source: object, keys: string[] | ...string): Promise<object>",
    },
    MethodDef {
        path: "zero.filter_lines",
        connector: "zero",
        description: "Filter lines in a text value by substring match",
        signature: "zero.filter_lines(source: { text: string } | string, pattern: string): Promise<{ text: string, lines: number, pattern: string }>",
    },
    MethodDef {
        path: "zero.compact_max",
        connector: "zero",
        description: "Max compression with guaranteed byte-exact recovery: content-type-aware aggressive compaction with tz:// ref",
        signature: "zero.compact_max(data: string | any): Promise<{ text: string, ref: string, raw_tokens: number, visible_tokens: number, compression_strategy: string, savings_pct: string }>",
    },
    MethodDef {
        path: "zero.count_tokens",
        connector: "zero",
        description: "Count tokens, bytes, and lines in a value without storing it (introspection helper)",
        signature: "zero.count_tokens(data: string | any): Promise<{ tokens: number, bytes: number, lines: number }>",
    },
    MethodDef {
        path: "zero.assert",
        connector: "zero",
        description: "Fail the plan immediately if condition is falsy (plan-level guard)",
        signature: "zero.assert(condition: any, message?: string): Promise<{ ok: true }>",
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
    MethodDef {
        path: "codemode.limits",
        connector: "codemode",
        description: "Return active CodeMode sandbox, output, ref, and operation limits",
        signature: "codemode.limits(): Promise<CodeModeLimits>",
    },
];

pub fn search_catalog(query: &str) -> Value {
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
            "signature": m.signature,
            "example": make_example(m.path),
            "score": score,
        })).collect::<Vec<_>>(),
        "total": results.len(),
        "truncated": false,
        "hint": "Use describe:<path> for full details, or call the method directly in your plan."
    })
}

fn make_example(path: &str) -> &'static str {
    match path {
        "zero.read" => r#"const f = await zero.read("src/main.rs"); return f"#,
        "zero.find" => r#"await zero.find("TODO", "src/")"#,
        "zero.grep" => r#"await zero.grep("fn main", "crates/")"#,
        "zero.glob" => r#"await zero.glob("**/*.rs", "crates/")"#,
        "zero.tree" => r#"await zero.tree("src", { depth: 2 })"#,
        "zero.shell" => r#"await zero.shell("cargo test --quiet")"#,
        "zero.edit" => r#"await zero.edit("src/lib.rs", [{ find: "old", replace: "new" }])"#,
        "zero.expand" | "zero.token.expand" => r#"await zero.expand("tz://blob/abc123")"#,
        "zero.compact" | "zero.token.compact" => r#"await zero.compact(large_output)"#,
        "zero.token.compactMany" => r#"await zero.token.compactMany([payloadA, payloadB])"#,
        "zero.token.expandMany" => r#"await zero.token.expandMany([refA, refB])"#,
        "zero.token.dedupe" => r#"await zero.token.dedupe([refA, refA, refB])"#,
        "zero.compact_max" => r#"await zero.compact_max(large_output)"#,
        "zero.ingest" => r#"await zero.ingest("large text payload")"#,
        "zero.pipe" => {
            r#"await zero.pipe([{ method: "zero.read", args: ["f.rs"] }, { method: "zero.compact", args: ["_prev.text"] }])"#
        }
        "zero.pick" => r#"const r = await zero.read("f.rs"); await zero.pick(r, ["text", "ref"])"#,
        "zero.filter_lines" => {
            r#"const r = await zero.grep("fn", "src/"); await zero.filter_lines(r.text, "pub")"#
        }
        "zero.batch" => {
            r#"await zero.batch([{ tool: "read", args: { path: "a.rs" } }, { tool: "read", args: { path: "b.rs" } }])"#
        }
        "zero.recall" => r#"await zero.recall("fn main")"#,
        "zero.fetch" => r#"await zero.fetch("https://example.com/api")"#,
        "zero.mem" => r#"await zero.mem()"#,
        "zero.rewrite" => r#"await zero.rewrite("find . -name '*.rs'")"#,
        "zero.discover" => r#"await zero.discover()"#,
        "zero.cache_pack" => r#"await zero.cache_pack()"#,
        _ => "(no example available)",
    }
}

fn related_methods(path: &str) -> Vec<&'static str> {
    match path {
        "zero.read" => vec!["zero.expand", "zero.find", "zero.compact"],
        "zero.find" | "zero.grep" => vec!["zero.filter_lines", "zero.recall", "zero.read"],
        "zero.glob" => vec!["zero.tree", "zero.read"],
        "zero.tree" => vec!["zero.glob", "zero.read"],
        "zero.shell" => vec!["zero.compact", "zero.filter_lines"],
        "zero.edit" => vec!["zero.read", "zero.find"],
        "zero.expand" | "zero.token.expand" => vec!["zero.compact", "zero.read"],
        "zero.compact" | "zero.token.compact" | "zero.compact_max" => {
            vec!["zero.expand", "zero.ingest", "zero.token.compactMany"]
        }
        "zero.token.compactMany" => vec!["zero.token.expandMany", "zero.token.dedupe"],
        "zero.token.expandMany" => vec!["zero.token.compactMany", "zero.expand"],
        "zero.token.dedupe" => vec!["zero.token.compactMany"],
        "zero.pipe" => vec!["zero.batch", "zero.pick", "zero.filter_lines"],
        "zero.pick" => vec!["zero.pipe", "zero.filter_lines"],
        "zero.filter_lines" => vec!["zero.find", "zero.pick", "zero.pipe"],
        "zero.batch" => vec!["zero.pipe"],
        "zero.recall" => vec!["zero.find", "zero.expand"],
        _ => vec![],
    }
}

pub fn describe_method(path: &str) -> Value {
    let path_lower = path.to_lowercase();
    if let Some(m) = METHOD_CATALOG
        .iter()
        .find(|m| m.path.to_lowercase() == path_lower)
    {
        json!({
            "path": m.path,
            "description": m.description,
            "signature": m.signature,
            "example": make_example(m.path),
            "related": related_methods(m.path),
            "kind": "method",
            "mutability": if m.path == "zero.edit" { "mutating" } else { "read_only" },
            "limits": CodeModeLimits::default().as_json(),
            "safety": {
                "sandbox": "fresh isolated context per execution; no network/env/process/raw-fs/module/timer capabilities"
            }
        })
    } else {
        json!({
            "path": path,
            "error": format!("no method found for path: {path}"),
            "available": METHOD_CATALOG.iter().map(|m| m.path).collect::<Vec<_>>(),
        })
    }
}

pub(crate) fn codemode_method_catalog() -> Value {
    json!({
        "schema_version": "tokenzero.codemode.catalog.v1",
        "methods": METHOD_CATALOG.iter().map(|m| json!({
            "path": m.path,
            "connector": m.connector,
            "description": m.description,
            "signature": m.signature,
        })).collect::<Vec<_>>(),
        "discovery": {
            "search_prefix": "search:<query>",
            "describe_prefix": "describe:<method>",
            "in_plan": ["codemode.search(query)", "codemode.describe(path)", "codemode.limits()"]
        },
        "limits": CodeModeLimits::default().as_json(),
        "execution_forms": ["recipe", "json", "sandboxed_javascript"],
        "execution_refs": [
            "codemode/execution/{id}",
            "codemode/execution/{id}/code",
            "codemode/execution/{id}/steps",
            "codemode/execution/{id}/telemetry",
            "codemode/execution/{id}/result",
            "codemode/execution/{id}/error"
        ],
        "next_actions": [
            "Run `tokenzero codemode 'search:read'` to rank methods by keyword.",
            "Run `tokenzero codemode 'describe:zero.read'` for full signatures.",
            "Compose multi-step workflows with const bindings and return.",
            "Same tools/engine as MCP tz_* surface; CodeMode composes them in one plan for fewer round-trips."
        ]
    })
}
