use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use tokenzero_core::McpToolSurface;

use crate::DEFAULT_SHELL_TIMEOUT_SECS;

macro_rules! catalog_json {
    ($($token:tt)*) => { serde_json::json!($($token)*) };
}

pub(crate) const TOOL_ALIASES: &[(&str, &str)] = &[
    ("read", "tz_read"),
    ("find", "tz_find"),
    ("grep", "tz_grep"),
    ("glob", "tz_glob"),
    ("tree", "tz_tree"),
    ("edit", "tz_edit"),
    ("recall", "tz_recall"),
    ("batch", "tz_batch"),
    ("fetch", "tz_fetch"),
    ("shell", "tz_shell"),
    ("ingest", "tz_ingest"),
    ("expand", "tz_expand"),
    ("mem", "tz_mem"),
    ("cache_pack", "tz_cache_pack"),
    ("cache-pack", "tz_cache_pack"),
    ("rewrite", "tz_rewrite"),
    ("discover", "tz_discover"),
    ("report_tool_issue", "tz_report_tool_issue"),
    ("report-tool-issue", "tz_report_tool_issue"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolSpecSeed {
    pub(crate) name: &'static str,
    pub(crate) cluster: &'static str,
    pub(crate) capabilities: &'static [&'static str],
    pub(crate) summary: &'static str,
    pub(crate) doc: String,
    pub(crate) input_schema: Value,
    /// Server-accepted argument aliases kept out of the advertised schema;
    /// published via `resource://tokenzero/tools` so the contract stays discoverable.
    pub(crate) arg_aliases: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSpec {
    pub uri: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

pub fn tool_specs() -> Vec<ToolSpec> {
    tool_specs_for_filter(None, true, McpToolSurface::Classic)
}

pub(crate) fn tool_specs_for_filter(
    cluster: Option<&str>,
    include_aliases: bool,
    surface: McpToolSurface,
) -> Vec<ToolSpec> {
    tool_specs_for_filter_with_health(cluster, include_aliases, surface, false)
}

/// Like [`tool_specs_for_filter`]. CodeMode lists primary tools only for the
/// whole session (`tools.listChanged=false`); expand fallback is internal.
///
/// Membership comes from [`crate::surface_health::tool_listed_on_surface`] —
/// the same policy that gates `tools/call` via `gate_tools_call`.
pub(crate) fn tool_specs_for_filter_with_health(
    cluster: Option<&str>,
    include_aliases: bool,
    surface: McpToolSurface,
    recovery_unlocked: bool,
) -> Vec<ToolSpec> {
    use crate::surface_health::tool_listed_on_surface;
    let canonical = canonical_tool_specs();
    catalog_entries(canonical)
        .filter(|(name, seed)| {
            (include_aliases || name == &seed.name)
                && tool_listed_on_surface(surface, seed.name, recovery_unlocked)
                && cluster.is_none_or(|cluster| seed.cluster == cluster)
        })
        .map(|(name, seed)| ToolSpec {
            name: name.to_string(),
            description: if name == seed.name {
                seed.summary.to_string()
            } else {
                alias_summary(seed.name)
            },
            input_schema: if name == seed.name {
                seed.input_schema.clone()
            } else {
                catalog_json!({"type": "object"})
            },
        })
        .collect()
}

fn catalog_entries(
    canonical: &[ToolSpecSeed],
) -> impl Iterator<Item = (&'static str, &ToolSpecSeed)> {
    canonical
        .iter()
        .map(|seed| (seed.name, seed))
        .chain(TOOL_ALIASES.iter().filter_map(|&(name, target)| {
            canonical
                .iter()
                .find(|seed| seed.name == target)
                .map(|seed| (name, seed))
        }))
}

/// Long-form catalog served by resource://tokenzero/tools. The tools/list
/// wire format stays compact; every detail removed from it lives here.
pub(crate) fn tool_docs() -> Vec<Value> {
    let canonical = canonical_tool_specs();
    catalog_entries(canonical)
        .map(|(name, seed)| {
            let canonical = name == seed.name;
            let mut doc = catalog_json!({ "name": name, "cluster": seed.cluster, "summary": if canonical { seed.summary.to_string() } else { alias_summary(seed.name) }, "description": if canonical { seed.doc.clone() } else { alias_description(name, seed.name) }, "inputSchema": seed.input_schema, "resultType": "complete", "ttlMs": 60000, "cacheScope": "workspace" });
            if canonical
                && seed.arg_aliases.as_object().is_some_and(|map| !map.is_empty())
            {
                doc.as_object_mut()
                    .expect("doc object")
                    .insert("argumentAliases".to_string(), seed.arg_aliases.clone());
            }
            doc
        })
        .collect()
}

macro_rules! resource_rows {
    ($($row:expr),* $(,)?) => { [$($row),*] };
}

pub fn resource_specs() -> Vec<ResourceSpec> {
    resource_rows![
        ( "resource://tokenzero/capabilities", "TokenZero capabilities", "Discover tool clusters, aliases, protocol versions, and next recommended calls.", "application/json", ),
        ( "resource://tokenzero/tools", "TokenZero tool catalog", "Read the complete tool catalog with schemas and agent-oriented descriptions.", "application/json", ),
        ( "resource://tokenzero/roots", "TokenZero allowed roots", "Discover file-system roots that read/find/tree/shell cwd operations may access.", "application/json", ),
        ( "resource://tokenzero/modes", "TokenZero render modes", "Discover accepted mode values for compacting, diagnostics, exact recovery, and pass-through output.", "application/json", ),
        ( "resource://tokenzero/codemode", "TokenZero CodeMode catalog", "Full CodeMode method catalog with signatures and discovery prefixes.", "application/json", ),
        ( "resource://tokenzero/cache", "TokenZero cache state", "Discover local recovery-cache and shell-output retention configuration without exposing payloads.", "application/json", ),
        ( "resource://tokenzero/session-boot", "TokenZero session boot", "Read the bounded manifest+delta boot capsule and component token attribution.", "application/json", ),
        ( "resource://tokenzero/metrics", "TokenZero tool metrics", "Read per-tool call counts, error counts, slow-call counts, and latency (this session plus cross-session cumulative).", "application/json", ),
        ( "resource://tokenzero/shell-contract", "TokenZero shell contract", "Read the shell transport, command-success, exact-ref, timeout, and retry semantics.", "text/markdown", ),
    ]
    .into_iter()
    .map(|(uri, name, description, mime_type)| ResourceSpec {
        uri: uri.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        mime_type: mime_type.to_string(),
    })
    .collect()
}

macro_rules! tool_table {
    ($( $kind:ident => { name: $name:literal, $($body:tt)* } ),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum ToolKind { $( $kind ),* }

        impl ToolKind {
            /// Accept full catalog names (tz_expand) and bare forms (expand)
            /// produced by canonical_tool after stripping the tz_ prefix.
            pub(crate) fn from_canonical(name: &str) -> Option<Self> {
                let bare = name.strip_prefix("tz_").unwrap_or(name);
                $(
                    if name == $name
                        || bare == $name.strip_prefix("tz_").unwrap_or($name)
                    {
                        return Some(Self::$kind);
                    }
                )*
                None
            }
        }

        pub(crate) fn canonical_tool_specs() -> &'static [ToolSpecSeed] {
            static SPECS: OnceLock<Vec<ToolSpecSeed>> = OnceLock::new();
            SPECS.get_or_init(|| vec![$( ToolSpecSeed { name: $name, $($body)* } ),*])
        }
    };
}

tool_table! {
    ExecuteCode => { name: "tz_execute_code", cluster: "codemode", capabilities: &["codemode", "plan-execution", "sandboxed"], summary: "Execute a TokenZero CodeMode recipe, JSON plan, or JavaScript plan.", doc: tool_description( "Execute a CodeMode plan through the native TokenZero executor.", "plan: source text. form: recipe, json, js, or auto. root/allowed_roots: bounded workspace selection. limits: optional integer overrides.", "Use only when the server is launched with --mode=codemode.", "Do keep visible results small; large outputs are stored behind refs.", "Common mistakes: launching the server in default mcp mode; mixing per-op tools with CodeMode tools.", "May mutate only through explicit lowered operations such as zero.edit/zero.shell; free-form QuickJS mutation stays denied. Execution records are persisted as refs.", ), input_schema: catalog_json!({ "type": "object", "required": ["plan"], "additionalProperties": false, "properties": { "plan": {"type": "string", "maxLength": 65536}, "form": {"type": "string", "enum": ["recipe", "json", "js", "auto"]}, "root": {"type": "string", "description": "Execute root; must remain under the server allowlist."}, "cwd": {"type": "string", "description": "Alias for root."}, "workspace": {"type": "string", "description": "Alias for root."}, "allowed_root": { "oneOf": [ {"type": "string"}, {"type": "array", "items": {"type": "string"}} ] }, "allowed_roots": { "oneOf": [ {"type": "string"}, {"type": "array", "items": {"type": "string"}} ] }, "limits": { "type": "object", "properties": { "max_wall_ms": {"type": "integer", "minimum": 0, "maximum": crate::CodeModeLimits::default().hard_max_wall_ms}, "hard_max_wall_ms": {"type": "integer", "minimum": 0, "maximum": crate::CodeModeLimits::default().hard_max_wall_ms} }, "additionalProperties": {"type": "integer", "minimum": 0} } } }), arg_aliases: catalog_json!({}), },
    CodemodeSearch => { name: "tz_codemode_search", cluster: "codemode", capabilities: &["codemode", "catalog-search", "read-only"], summary: "Search the TokenZero CodeMode method catalog.", doc: tool_description( "Search CodeMode methods by keyword.", "query: non-empty search text. limit: maximum hits, 1-50.", "Use for progressive CodeMode discovery.", "Do search before asking for full descriptions.", "Common mistakes: empty query; expecting workspace file search.", "Read-only.", ), input_schema: catalog_json!({ "type": "object", "required": ["query"], "additionalProperties": false, "properties": { "query": {"type": "string", "minLength": 1}, "limit": {"type": "integer", "minimum": 1, "maximum": 50} } }), arg_aliases: catalog_json!({}), },
    CodemodeDescribe => { name: "tz_codemode_describe", cluster: "codemode", capabilities: &["codemode", "catalog-describe", "read-only"], summary: "Describe a TokenZero CodeMode method or capabilities manifest.", doc: tool_description( "Describe CodeMode capabilities or a specific method.", "name: capabilities or a method name such as zero.read.", "Use name=capabilities for the ZeroStack contract manifest.", "Do inspect capabilities before relying on optional limits.", "Common mistakes: using this in mcp mode where CodeMode tools are hidden.", "Read-only.", ), input_schema: catalog_json!({ "type": "object", "required": ["name"], "additionalProperties": false, "properties": {"name": {"type": "string", "minLength": 1}} }), arg_aliases: catalog_json!({}), },
    Read => { name: "tz_read", cluster: "material", capabilities: &["read", "exact-refs", "line-range", "shared-cas"], summary: "Read file(s) under allowed roots: compact visible output plus exact tz:// recovery refs. Bound big files with start_line/end_line.", doc: tool_description( "Read local files into compact visible output while storing exact recovery refs.", "path: use `tree` or `glob` to find workspace paths; paths must stay under allowed roots.\nmode: read `resource://tokenzero/modes` for accepted render modes.", "Use for file reads and bounded line-range inspection. NOT for shell output; use `shell`.", "Do pass `start_line` and `end_line` for large files. Do keep `raw=false` unless exact contiguous text is required. Don't pass paths outside allowed roots. Don't re-read full files when a `tz://` ref can be expanded.", "Common mistakes: missing `path`; path outside allowed roots; using `raw=true` for huge files; assuming visible output is full content without expanding refs.", "Idempotent. Safe to retry. Returns exact refs for full recovery.", ), input_schema: read_schema(), arg_aliases: catalog_json!({}), },
    Find => { name: "tz_find", cluster: "material", capabilities: &["search", "literal", "exact-refs", "shared-cas"], summary: "Search file contents for a literal substring and return compact, recoverable matches. Narrow path before broad queries.", doc: tool_description( "Search local text for a literal substring and return compact, recoverable matches.", "query/pattern: always a literal substring (never a regex; use `grep` for regex). path: use `tree` or `glob` to choose narrow roots.", "Use for semantic repo search before reading files. NOT for shell command output.", "Do search narrow paths first. Do expand refs for hidden matches. Don't use vague one-word queries across the whole repo when a path is known.", "Common mistakes: omitting `query` and `pattern`; passing regex syntax expecting it to be interpreted; using broad roots; treating compact matches as exhaustive context.", "Idempotent. Safe to retry with narrower roots.", ), input_schema: search_schema("Literal substring to search for."), arg_aliases: catalog_json!({}), },
    Grep => { name: "tz_grep", cluster: "material", capabilities: &["search", "regex", "exact-refs", "shared-cas"], summary: "Grep-style exact-first content search: regex when the ripgrep backend is active, literal substring otherwise.", doc: tool_description( "Run grep-compatible exact-first search with recoverable output.", "query/pattern: a regular expression when the ripgrep backend is active (default when `rg` is installed); the internal fallback scanner matches it as a literal substring. An invalid regex under ripgrep returns an `invalid_pattern` error instead of degrading to substring results. path: use `tree` or `glob` to constrain roots.", "Use when an agent expects grep-style search behavior. NOT for path globbing; use `glob`.", "Do provide specific patterns. Do inspect refs for full output. Don't assume shell `grep` flags are accepted here.", "Common mistakes: passing shell flags instead of a pattern; unescaped regex metacharacters like `(` (invalid_pattern under ripgrep); searching generated directories; failing to narrow path.", "Idempotent. Safe to retry.", ), input_schema: search_schema( "Search pattern: regex under the ripgrep backend, literal substring under the internal fallback.", ), arg_aliases: catalog_json!({}), },
    Recall => { name: "tz_recall", cluster: "material", capabilities: &["search", "cache", "exact-refs", "shared-cas"], summary: "Search every payload already stored in the recovery cache; hits carry exact tz:// refs recoverable in one `expand` call.", doc: tool_description( "Full-text search over previously stored tool outputs and file payloads.", "query: literal case-insensitive substring. Each hit line carries its `tz://` ref — recover full bytes with `expand`, never by re-running the original command.", "Use to re-find content TokenZero already served this workspace (earlier reads, search output, shell captures) without re-reading files or re-running commands. NOT a live filesystem search; use `find`/`grep` for that.", "Do recall before re-running expensive commands. Do expand the listed ref for full context. Don't expect regex; don't expect unstored content to be findable.", "Common mistakes: passing regex syntax (literal substring only); searching for content never routed through TokenZero; re-reading a file when the hit's ref already recovers it.", "Idempotent. Read-only over the recovery cache.", ), input_schema: recall_schema(), arg_aliases: catalog_json!({}), },
    Batch => { name: "tz_batch", cluster: "execution", capabilities: &["batch", "exact-refs"], summary: "Run several TokenZero ops in one call: one combined capsule, per-op sections, unioned refs.", doc: tool_description( "Batch several TokenZero operations into one round trip.", "ops: array of {tool, args} — any TokenZero tool except batch itself; args match the sub-tool's schema. Capped at 16 ops.", "Use when several independent reads/searches are needed at once; one call replaces N round trips. NOT for dependent steps where one op's output feeds the next.", "Do batch independent lookups. Don't nest batches; don't batch dependent operations.", "Common mistakes: nesting batch inside batch (rejected per op); expecting op N to see op N-1's results; exceeding the 16-op cap.", "Idempotency follows the batched tools (a batched edit mutates).", ), input_schema: batch_schema(), arg_aliases: catalog_json!({}), },
    Fetch => { name: "tz_fetch", cluster: "web", capabilities: &["fetch", "web", "cache", "exact-refs"], summary: "Fetch an http(s) URL via curl with a TTL cache: compact body capsule plus exact tz:// refs; repeat fetches inside the TTL never touch the network.", doc: tool_description( "Fetch a URL with recoverable output and a TTL'd local cache.", "url: http(s) only. ttl_seconds: cached bodies younger than this serve from the store (default 24h); fresh=true re-fetches.", "Use for web pages, API responses, and raw files. NOT for local files; use `read`.", "Do rely on the TTL cache for repeat lookups. Do expand the blob ref for the full body. Don't fetch secrets-bearing URLs (bodies persist in the local recovery cache).", "Common mistakes: non-http(s) schemes (invalid_url); expecting the visible capsule to be the full body (expand the ref); re-fetching inside the TTL with fresh=true when the cache would do.", "Idempotent within the TTL window. Network access happens via the system curl.", ), input_schema: fetch_schema(), arg_aliases: catalog_json!({}), },
    Glob => { name: "tz_glob", cluster: "material", capabilities: &["discover", "glob", "shared-cas"], summary: "List file paths matching a glob pattern (no contents). Use before read.", doc: tool_description( "Discover paths with glob patterns while keeping results bounded and recoverable.", "pattern: use file globs like `crates/*/src/*.rs`. path: choose the workspace root or a known subdirectory.", "Use to find files before `read`. NOT for content search; use `find` or `grep`.", "Do use narrow patterns. Do set `include_hidden=true` only when needed. Don't glob the entire repo for vague names.", "Common mistakes: using content text as a glob; forgetting hidden files are excluded by default; requesting too many matches.", "Idempotent. Safe to retry with narrower patterns.", ), input_schema: glob_schema(), arg_aliases: catalog_json!({"pattern": ["glob", "query"]}), },
    Tree => { name: "tz_tree", cluster: "material", capabilities: &["discover", "tree", "shared-cas"], summary: "Inspect a bounded directory tree for orientation. Keep depth small.", doc: tool_description( "Inspect a bounded directory tree for repo orientation.", "path: start with the target repo or known subdirectory. depth: use small values first.", "Use at the start of repo inspection or when locating modules. NOT for reading file contents.", "Do keep `depth` low. Do narrow `path` after the first pass. Don't use tree output as proof of file contents.", "Common mistakes: enormous depth; hidden files omitted unless requested; using tree where `glob` is more precise.", "Idempotent. Safe to retry.", ), input_schema: tree_schema(), arg_aliases: catalog_json!({}), },
    Edit => { name: "tz_edit", cluster: "edit", capabilities: &["write", "atomic", "exact-refs"], summary: "Apply multi-hunk find/replace edits to one file in a single call: all-or-nothing, atomic write, undo via tz:// ref, dry_run preview.", doc: tool_description( "Read, verify, and edit one file in a single call with an exact undo ref.", "path: one file under an allowed root; use `tree` or `glob` to locate it. edits: ordered {find, replace, replace_all} hunks applied against the evolving text.", "Use for code or text edits without a separate raw read round-trip. NOT for binary files or bulk multi-file renames; use `shell` for those.", "Do pass the exact current text in `find`; each hunk must match exactly once unless `replace_all=true`. Do use `dry_run=true` to preview the hunk diff. Do expand the `undo` ref to recover the pre-image. Don't send overlapping hunks; the batch is all-or-nothing and any failed hunk aborts the whole edit.", "Common mistakes: `find` copied with stale whitespace (hunk_not_found); multiple matches without `replace_all` (ambiguous_hunk); `find` equal to `replace` (no_op_hunk); create=true with more than one hunk or a non-empty find.", "Not idempotent: a second identical call usually fails with hunk_not_found. Safe to retry only after re-reading the file.", ), input_schema: edit_schema(), arg_aliases: catalog_json!({}), },
    Shell => { name: "tz_shell", cluster: "execution", capabilities: &["shell", "exact-refs", "command-success"], summary: "Run a local command: compact output, exact stream refs, command_success telemetry. Retry only read-only commands.", doc: tool_description( "Run a local command with compact output, exact refs, and command-success telemetry.", "command/cmd/input/script or argv/args: provide the command. cwd: choose an allowed root or subdirectory.", "Use for tests, build tools, git status, and local diagnostics. NOT for reading files when `read` is enough.", "Do prefer argv for simple commands. Do inspect `command_success`, not just transport status. Don't hide failures in pipelines. Don't request network or destructive actions without approval.", "Common mistakes: missing command; using `cd` instead of `cwd`; trusting visible output when refs contain more; assuming nonzero child exit means MCP transport failed.", "Not generally idempotent. Safe to retry only for read-only commands and tests.", ), input_schema: shell_schema(), arg_aliases: catalog_json!({ "command": ["cmd", "input", "script"], "argv": ["args"], "timeout_seconds": ["timeout_secs", "timeout", "shell_timeout_seconds"] }), },
    Ingest => { name: "tz_ingest", cluster: "execution", capabilities: &["ingest", "exact-refs"], summary: "Store external text behind exact tz:// refs and return a compact capsule.", doc: tool_description( "Store external text behind exact refs and return compact visible output.", "text/input: pass the payload produced outside TokenZero.", "Use for preserving external tool output. NOT for reading local files; use `read`.", "Do ingest large external logs before summarizing. Do keep secrets out of payloads. Don't use it as durable public storage.", "Common mistakes: missing `text`; ingesting sensitive values; expecting file-system path access.", "Idempotent for the same payload content from the agent perspective.", ), input_schema: text_schema("External text payload to store behind exact refs."), arg_aliases: catalog_json!({"text": ["input"]}), },
    Expand => { name: "tz_expand", cluster: "material", capabilities: &["expand", "exact-refs", "fragment-selectors", "symbol-anchors", "diff-baseline", "shared-cas"], summary: "Recover exact bytes from a tz://, fz://, or gz:// ref, optionally narrowed by line range, selector, or symbol.", doc: tool_description( "Recover exact content from `tz://`, `fz://`, or `gz://` blob refs (same-store scheme alias, plus evidence-backed cross-engine blob expand under a shared ZeroStack CAS / sibling store; non-blob portable refs unsupported) with optional ranges or anchors.", "ref: copy a ref returned by read/find/tree/shell/ingest or sibling ZeroStack engines. selector/start_line/end_line: use only when narrowing recovery.", "Use whenever compact output omitted needed detail. NOT for arbitrary file paths; use `read`.", "Do expand refs instead of re-running expensive commands. Do use line ranges for large file refs. Don't invent refs.", "Common mistakes: passing `path` instead of `ref`; using stale refs from another workspace; expanding whole huge refs unnecessarily.", "Idempotent. Safe to retry.", ), input_schema: expand_schema(), arg_aliases: catalog_json!({}), },
    Mem => { name: "tz_mem", cluster: "execution", capabilities: &["diagnostic", "cache"], summary: "Inspect local recovery-cache and configuration state.", doc: tool_description( "Inspect local TokenZero recovery, cache, and configuration state.", "No parameters. Use `resource://tokenzero/cache` for the static MCP resource view.", "Use to diagnose cache/ref availability. NOT for project search.", "Do call when refs fail to expand. Don't treat cache paths as public artifacts.", "Common mistakes: expecting project memory; assuming cache state contains raw payload text.", "Idempotent. Safe to retry.", ), input_schema: no_args_schema(), arg_aliases: catalog_json!({}), },
    CachePack => { name: "tz_cache_pack", cluster: "execution", capabilities: &["cache", "prompt-cache"], summary: "Build a daemonless prompt-cache pack with a stable prefix and volatile refs.", doc: tool_description( "Build a daemonless prompt-cache pack with stable prefix and volatile refs.", "scope: use `agent` unless a host integration documents another value.", "Use when preparing cacheable session context. NOT for exact file recovery; use `expand`.", "Do preserve returned cache keys and refs together. Don't assume a daemon or background indexer exists.", "Common mistakes: unsupported scope; treating volatile-tail refs as stable prefix text.", "Idempotent while source content is unchanged.", ), input_schema: cache_pack_schema(), arg_aliases: catalog_json!({}), },
    Rewrite => { name: "tz_rewrite", cluster: "execution", capabilities: &["diagnostic", "rewrite"], summary: "Plan a conservative, TokenZero-safe rewrite of a shell command without executing it.", doc: tool_description( "Plan a conservative command rewrite without executing the command.", "command/cmd/input/script or argv/args: provide the command. mode: use `safe` unless debugging rewrite behavior.", "Use before risky or shell-heavy commands. NOT for execution; use `shell`.", "Do inspect the suggested argv and risk notes. Don't assume rewrite approval means command execution approval.", "Common mistakes: expecting side effects; passing no command; confusing rewrite mode with output render mode.", "Idempotent. Safe to retry.", ), input_schema: rewrite_schema(), arg_aliases: catalog_json!({"command": ["cmd", "input", "script"], "argv": ["args"]}), },
    Discover => { name: "tz_discover", cluster: "execution", capabilities: &["diagnostic", "discovery"], summary: "Report TokenZero filter and runtime readiness metadata.", doc: tool_description( "Discover TokenZero filter and runtime readiness metadata.", "No parameters. Use `server/discover` for protocol metadata.", "Use to decide whether shell rewrite/filtering support is active. NOT for file discovery.", "Do check readiness before relying on rewrite behavior. Don't confuse this with `tools/list`.", "Common mistakes: expecting repo files; assuming unavailable filters are fatal.", "Idempotent. Safe to retry.", ), input_schema: no_args_schema(), arg_aliases: catalog_json!({}), },
    ReportToolIssue => { name: "tz_report_tool_issue", cluster: "codemode", capabilities: &["diagnostic", "report"], summary: "Record a field issue against a CodeMode/TokenZero tool name (accepts zero_execute).", doc: tool_description( "Record a field issue for expand/root/shell/CodeMode failures without leaving the harness.", "tool: reportable surface name (zero_execute, zerostack, tz_execute_code, zero.token.*, tz_* …). summary: short description. detail: optional context.", "Use when CodeMode expand/root/shell fails and you need a durable field report. NOT for filing GitHub issues.", "Do pass tool=zero_execute for unified ZeroStack CodeMode failures. Don't invent unlisted harness tool names.", "Common mistakes: omitting tool or summary; using Browser/native-only names.", "Idempotent per call (writes a new timestamped report file).", ), input_schema: catalog_json!({ "type": "object", "properties": { "tool": {"type": "string", "minLength": 1, "description": "Tool/surface name (zero_execute accepted)."}, "summary": {"type": "string", "minLength": 1, "description": "Short issue summary."}, "detail": {"type": "string", "description": "Optional detail / repro."} }, "required": ["tool", "summary"], "additionalProperties": true }), arg_aliases: catalog_json!({ "tool": ["name", "tool_name", "surface"], "summary": ["message", "title"], "detail": ["body", "repro", "context"] }), },
}

#[allow(dead_code)]
pub(crate) fn canonical_tool_names() -> Vec<&'static str> {
    canonical_tool_specs()
        .iter()
        .map(|seed| seed.name)
        .collect()
}

pub(crate) fn canonical_tool_names_for_surface(surface: McpToolSurface) -> Vec<String> {
    use crate::surface_health::surface_includes;
    canonical_tool_specs()
        .iter()
        .filter(|seed| surface_includes(surface, seed.name))
        .map(|seed| seed.name.to_string())
        .collect()
}

pub(crate) fn tool_cluster_names() -> Vec<&'static str> {
    let mut clusters = canonical_tool_specs()
        .iter()
        .map(|seed| seed.cluster)
        .collect::<Vec<_>>();
    clusters.sort_unstable();
    clusters.dedup();
    clusters
}

pub(crate) fn alias_summary(target: &str) -> String {
    format!("Alias of `{target}`; same schema and behavior.")
}

/// Aliases are behaviorally identical to their targets, so the catalog entry
/// defers to the target's long-form doc instead of restating the section
/// boilerplate once per alias.
fn alias_description(alias: &str, target: &str) -> String {
    format!(
        "Alias `{alias}` for `{target}`: same `inputSchema`, arguments, returned MCP content, \
         and retry semantics. Read the `{target}` entry in this catalog for discovery guidance, \
         examples, and common mistakes. Prefer `{target}` in durable instructions; `{alias}` \
         exists for interactive ergonomics.",
    )
}

fn tool_description(
    one_liner: &str,
    discovery: &str,
    when_to_use: &str,
    do_dont: &str,
    mistakes: &str,
    idempotency: &str,
) -> String {
    format!(
        "{one_liner}\n\nDiscovery\n---------\n{discovery}\n\nWhen to use\n-----------\n{when_to_use}\n\nParameters\n----------\nSee `inputSchema` for exact JSON Schema 2020-12 types, defaults, enums, and required fields.\n\nReturns\n-------\nMCP text content with visible output plus a `refs:` recovery footer (shell reports command_success and refs inline). Set TOKENZERO_MCP_ENVELOPE=compact|full for a `structuredContent.cli` envelope.\n\nDo / Don't\n----------\n{do_dont}\n\nExamples\n--------\nUse JSON-RPC `tools/call` with this tool name and an `arguments` object matching `inputSchema`.\n\nCommon mistakes\n---------------\n{mistakes}\n\nIdempotency\n-----------\n{idempotency}",
    )
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    let mut schema = catalog_json!({ "type": "object", "additionalProperties": false, "properties": properties });
    if !required.is_empty() {
        schema
            .as_object_mut()
            .expect("schema object")
            .insert("required".to_string(), catalog_json!(required));
    }
    schema
}

macro_rules! schema {
    ($required:expr; { $($properties:tt)* }) => {
        object_schema(catalog_json!({ $($properties)* }), $required)
    };
}

fn no_args_schema() -> Value {
    object_schema(catalog_json!({}), &[])
}

fn mode_property() -> Value {
    // Legacy aliases (hybrid/critical/fidelity) stay accepted server-side but
    // are kept out of the advertised enum; resource://tokenzero/modes documents them.
    catalog_json!({ "type": "string", "enum": ["auto", "passthrough", "diagnostic", "structured", "dedupe", "diff-aware", "exact"], "default": "auto" })
}

fn path_value(description: &str) -> Value {
    catalog_json!({ "type": ["string", "array"], "items": {"type": "string"}, "description": description })
}

fn positive_usize(default: usize) -> Value {
    catalog_json!({ "type": "integer", "minimum": 1, "default": default })
}

fn line_property() -> Value {
    catalog_json!({"type": "integer", "minimum": 1})
}

fn string_alias_property() -> Value {
    catalog_json!({"type": "string", "minLength": 1})
}

fn fresh_property() -> Value {
    catalog_json!({ "type": "boolean", "default": false, "description": "Bypass session dedup/diff for this call and always return the full render." })
}

fn read_schema() -> Value {
    schema!(&["path"]; { "path": path_value("File path(s) under an allowed root."), "mode": mode_property(), "start_line": line_property(), "end_line": line_property(), "raw": {"type": "boolean", "default": false, "description": "Return contiguous text instead of a compact capsule."}, "fresh": fresh_property(), "max_files": positive_usize(20), "max_visible_tokens": positive_usize(4000) })
}

// Advertised schemas must stay plain objects: top-level combinators
// (anyOf/oneOf/allOf) make some MCP clients — Claude Code included — drop the
// tool from the model's tool list entirely. Either-or argument requirements
// are stated in property descriptions and enforced server-side.
fn search_schema(query_description: &str) -> Value {
    schema!(&[]; { "query": {"type": "string", "minLength": 1, "description": format!("{query_description} Provide this or `pattern`.")}, "pattern": string_alias_property(), "path": path_value("Roots or files to search; defaults to the workspace root."), "mode": mode_property(), "fresh": fresh_property(), "max_files": positive_usize(20), "max_visible_tokens": positive_usize(4000) })
}

fn batch_schema() -> Value {
    schema!(&["ops"]; { "ops": { "type": "array", "minItems": 1, "maxItems": 16, "description": "Sub-operations, each {tool, args}. Any TokenZero tool except batch itself; args match that tool's schema.", "items": { "type": "object", "properties": { "tool": {"type": "string", "minLength": 1}, "args": {"type": "object"} }, "required": ["tool"] } }, "mode": mode_property() })
}

fn fetch_schema() -> Value {
    schema!(&["url"]; { "url": {"type": "string", "minLength": 1, "description": "http(s) URL to fetch."}, "ttl_seconds": {"type": "integer", "minimum": 0, "default": 86400, "description": "Serve a cached body younger than this without touching the network."}, "fresh": {"type": "boolean", "default": false, "description": "Bypass the TTL cache and re-fetch."}, "mode": mode_property(), "max_visible_tokens": positive_usize(4000) })
}

fn recall_schema() -> Value {
    schema!(&["query"]; { "query": {"type": "string", "minLength": 1, "description": "Literal case-insensitive substring to search for across stored payloads."}, "max_hits": positive_usize(50), "mode": mode_property(), "max_visible_tokens": positive_usize(4000) })
}

fn glob_schema() -> Value {
    schema!(&["pattern"]; { "pattern": {"type": "string", "minLength": 1, "description": "Glob pattern to match file paths."}, "path": path_value("Roots to inspect; defaults to the workspace root."), "include_hidden": {"type": "boolean", "default": false}, "mode": mode_property(), "max_files": positive_usize(200), "max_visible_tokens": positive_usize(4000) })
}

fn tree_schema() -> Value {
    schema!(&[]; { "path": path_value("Roots to inspect; defaults to the workspace root."), "depth": positive_usize(2), "include_hidden": {"type": "boolean", "default": false}, "mode": mode_property(), "max_files": positive_usize(200), "max_visible_tokens": positive_usize(4000) })
}

fn edit_schema() -> Value {
    schema!(&["path", "edits"]; { "path": {"type": "string", "minLength": 1, "description": "File path under an allowed root."}, "edits": { "type": "array", "minItems": 1, "description": "Hunks applied in order against the evolving text; the batch is all-or-nothing.", "items": { "type": "object", "additionalProperties": false, "properties": { "find": {"type": "string", "description": "Exact text to replace; must match exactly once unless replace_all. With create=true the single hunk's find must be \"\" (empty)."}, "replace": {"type": "string", "description": "Replacement text. With create=true this becomes the full new-file content."}, "replace_all": {"type": "boolean", "default": false, "description": "Replace every occurrence instead of requiring a unique match."} }, "required": ["find", "replace"] } }, "create": {"type": "boolean", "default": false, "description": "Create a new file: requires exactly one hunk whose find is \"\" (empty); its replace becomes the full new-file content; fails if the file already exists."}, "dry_run": {"type": "boolean", "default": false, "description": "Validate and render the hunk diff without writing."}, "mode": mode_property(), "max_visible_tokens": positive_usize(4000) })
}

fn shell_schema() -> Value {
    schema!(&[]; { "command": {"type": "string", "minLength": 1, "description": "Command string to execute. Provide this or `argv`."}, "argv": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Argument vector executed without reparsing."}, "cwd": {"type": "string", "description": "Working directory under an allowed root."}, "mode": mode_property(), "rewrite": {"type": "string", "description": "Rewrite mode applied before execution."}, "no_rewrite": {"type": "boolean", "default": false}, "stdin": {"type": "string"}, "timeout_seconds": positive_usize(DEFAULT_SHELL_TIMEOUT_SECS as usize) })
}

fn text_schema(description: &str) -> Value {
    schema!(&["text"]; { "text": {"type": "string", "minLength": 1, "description": description}, "mode": mode_property() })
}

fn expand_schema() -> Value {
    schema!(&["ref"]; { "ref": {"type": "string", "pattern": "^(tz|fz|gz)://", "description": "Exact recovery ref (tz://, fz://, or gz:// blob refs: same-store scheme alias, plus evidence-backed cross-engine blob expand under shared CAS; non-blob portable refs unsupported)."}, "selector": {"type": "string", "description": "Recovery-store-specific selector."}, "start_line": line_property(), "end_line": line_property(), "anchor_kind": {"type": "string", "description": "Anchor kind for symbol-aware recovery."}, "symbol": {"type": "string", "description": "Symbol name for symbol-aware recovery."}, "since": {"type": "string", "pattern": "^(tz|fz|gz)://", "description": "tz/fz/gz ref baseline for unified diff; errors if not recoverable."}, "fresh": fresh_property() })
}

fn cache_pack_schema() -> Value {
    schema!(&[]; { "scope": { "type": "string", "default": "agent", "description": "Cache-pack scope; use `agent`." } })
}

fn rewrite_schema() -> Value {
    schema!(&[]; { "command": {"type": "string", "minLength": 1, "description": "Command string to rewrite. Provide this or `argv`."}, "argv": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Argument vector to rewrite."}, "mode": {"type": "string", "default": "safe", "description": "Rewrite policy mode."} })
}

pub(crate) fn tool_clusters() -> Value {
    let canonical = canonical_tool_specs();
    let mut by_cluster: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for seed in canonical {
        by_cluster.entry(seed.cluster).or_default().push(seed.name);
    }
    catalog_json!(
        by_cluster
            .into_iter()
            .map(|(cluster, tools)| catalog_json!({"cluster": cluster, "tools": tools}))
            .collect::<Vec<_>>()
    )
}
