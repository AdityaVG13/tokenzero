//! Static inventory of every public TokenZero operation (tokenzero-irx9.1).

use serde_json::{Value, json};

use super::schemas::{
    args, batch_schema, cache_pack_schema, codemode_describe_schema, codemode_search_schema,
    default_results, edit_schema, execute_code_schema, expand_schema, fetch_schema, glob_schema,
    no_args_schema, read_schema, recall_schema, ref_first_results, report_tool_issue_schema,
    rewrite_schema, search_schema, shell_schema, text_schema, tree_schema,
};
use super::types::{
    CancellationSemantics, CostClass, DomainErrorKind, MigrationStatus, Mutability, Operation,
    OperationResults, RefOwnership, SurfaceExposure,
};

fn read_errors() -> &'static [DomainErrorKind] {
    &[
        DomainErrorKind::Validation,
        DomainErrorKind::Substrate,
        DomainErrorKind::NotFound,
        DomainErrorKind::DeadlineExceeded,
        DomainErrorKind::Cancelled,
        DomainErrorKind::Busy,
        DomainErrorKind::Unauthorized,
        DomainErrorKind::Policy,
    ]
}

fn search_errors() -> &'static [DomainErrorKind] {
    &[
        DomainErrorKind::Validation,
        DomainErrorKind::InvalidPattern,
        DomainErrorKind::Substrate,
        DomainErrorKind::DeadlineExceeded,
        DomainErrorKind::Cancelled,
        DomainErrorKind::Busy,
        DomainErrorKind::Unauthorized,
    ]
}

fn edit_errors() -> &'static [DomainErrorKind] {
    &[
        DomainErrorKind::Validation,
        DomainErrorKind::Policy,
        DomainErrorKind::HunkNotFound,
        DomainErrorKind::AmbiguousHunk,
        DomainErrorKind::NoOpHunk,
        DomainErrorKind::Substrate,
        DomainErrorKind::DeadlineExceeded,
        DomainErrorKind::Cancelled,
        DomainErrorKind::Unauthorized,
    ]
}

fn shell_errors() -> &'static [DomainErrorKind] {
    &[
        DomainErrorKind::Validation,
        DomainErrorKind::Policy,
        DomainErrorKind::Approval,
        DomainErrorKind::Sandbox,
        DomainErrorKind::Runtime,
        DomainErrorKind::DeadlineExceeded,
        DomainErrorKind::Cancelled,
        DomainErrorKind::Busy,
        DomainErrorKind::Unauthorized,
    ]
}

fn expand_errors() -> &'static [DomainErrorKind] {
    &[
        DomainErrorKind::Validation,
        DomainErrorKind::InvalidRef,
        DomainErrorKind::NotFound,
        DomainErrorKind::Substrate,
        DomainErrorKind::DeadlineExceeded,
        DomainErrorKind::Cancelled,
    ]
}

fn fetch_errors() -> &'static [DomainErrorKind] {
    &[
        DomainErrorKind::Validation,
        DomainErrorKind::InvalidUrl,
        DomainErrorKind::Runtime,
        DomainErrorKind::Policy,
        DomainErrorKind::DeadlineExceeded,
        DomainErrorKind::Cancelled,
    ]
}

/// Shared constructor: every op is Public; surfaces/results/aliases vary by helper.
fn op(
    name: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
    mutability: Mutability,
    cost_class: CostClass,
    ref_ownership: RefOwnership,
    cancellation: CancellationSemantics,
    migration: MigrationStatus,
    exposure: SurfaceExposure,
    capabilities: &'static [&'static str],
    cluster: &'static str,
    schema: Value,
    results: OperationResults,
    error_kinds: &'static [DomainErrorKind],
    arg_aliases: Value,
) -> Operation {
    Operation {
        name,
        description,
        aliases,
        mutability,
        capability: super::types::CapabilityRequirement::Public,
        cost_class,
        ref_ownership,
        cancellation,
        migration,
        exposure,
        capabilities,
        cluster,
        args: args(schema),
        results,
        error_kinds,
        arg_aliases,
    }
}

fn classic_surface(binding: Option<&'static str>, codemode_mcp: bool) -> SurfaceExposure {
    SurfaceExposure {
        fastmcp_tool: true,
        codemode_mcp_tool: codemode_mcp,
        codemode_binding: binding,
        resource_uri: None,
    }
}

/// Classic FastMCP + CodeMode domain tool (canonical, ref-first, empty arg aliases).
fn classic(
    name: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
    mutability: Mutability,
    cost_class: CostClass,
    ref_ownership: RefOwnership,
    cancellation: CancellationSemantics,
    binding: &'static str,
    capabilities: &'static [&'static str],
    cluster: &'static str,
    schema: Value,
    error_kinds: &'static [DomainErrorKind],
) -> Operation {
    classic_ex(
        name,
        description,
        aliases,
        mutability,
        cost_class,
        ref_ownership,
        cancellation,
        binding,
        capabilities,
        cluster,
        schema,
        ref_first_results(),
        error_kinds,
        json!({}),
    )
}

fn classic_ex(
    name: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
    mutability: Mutability,
    cost_class: CostClass,
    ref_ownership: RefOwnership,
    cancellation: CancellationSemantics,
    binding: &'static str,
    capabilities: &'static [&'static str],
    cluster: &'static str,
    schema: Value,
    results: OperationResults,
    error_kinds: &'static [DomainErrorKind],
    arg_aliases: Value,
) -> Operation {
    op(
        name,
        description,
        aliases,
        mutability,
        cost_class,
        ref_ownership,
        cancellation,
        MigrationStatus::Canonical,
        classic_surface(Some(binding), false),
        capabilities,
        cluster,
        schema,
        results,
        error_kinds,
        arg_aliases,
    )
}

/// CodeMode binding-only helper (`name` is also the binding path).
fn binding(
    name: &'static str,
    description: &'static str,
    mutability: Mutability,
    cost_class: CostClass,
    ref_ownership: RefOwnership,
    cancellation: CancellationSemantics,
    capabilities: &'static [&'static str],
    schema: Value,
    results: OperationResults,
    error_kinds: &'static [DomainErrorKind],
) -> Operation {
    binding_ex(
        name,
        description,
        &[],
        mutability,
        cost_class,
        ref_ownership,
        cancellation,
        capabilities,
        schema,
        results,
        error_kinds,
    )
}

fn binding_ex(
    name: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
    mutability: Mutability,
    cost_class: CostClass,
    ref_ownership: RefOwnership,
    cancellation: CancellationSemantics,
    capabilities: &'static [&'static str],
    schema: Value,
    results: OperationResults,
    error_kinds: &'static [DomainErrorKind],
) -> Operation {
    op(
        name,
        description,
        aliases,
        mutability,
        cost_class,
        ref_ownership,
        cancellation,
        MigrationStatus::CodemodeControl,
        SurfaceExposure {
            fastmcp_tool: false,
            codemode_mcp_tool: false,
            codemode_binding: Some(name),
            resource_uri: None,
        },
        capabilities,
        "codemode",
        schema,
        results,
        error_kinds,
        json!({}),
    )
}

fn resource(
    name: &'static str,
    description: &'static str,
    uri: &'static str,
    capabilities: &'static [&'static str],
    ref_ownership: RefOwnership,
) -> Operation {
    op(
        name,
        description,
        &[],
        Mutability::ReadOnly,
        CostClass::Cheap,
        ref_ownership,
        CancellationSemantics::None,
        MigrationStatus::Resource,
        SurfaceExposure {
            fastmcp_tool: false,
            codemode_mcp_tool: false,
            codemode_binding: None,
            resource_uri: Some(uri),
        },
        capabilities,
        "resource",
        no_args_schema(),
        default_results(),
        read_errors(),
        json!({}),
    )
}

/// Runtime builder: `json!` schemas are owned Values, so the registry is
/// materialized once via `OnceLock` and sorted by canonical name.
pub fn all_operations() -> &'static [Operation] {
    use std::sync::OnceLock;
    static REG: OnceLock<Vec<Operation>> = OnceLock::new();
    REG.get_or_init(build_registry).as_slice()
}

pub fn operation_by_name(name: &str) -> Option<&'static Operation> {
    all_operations().iter().find(|op| op.name == name)
}

fn build_registry() -> Vec<Operation> {
    use CancellationSemantics as C;
    use CostClass as K;
    use Mutability as M;
    use RefOwnership as R;

    let mut ops = vec![
        // --- Classic domain tools (FastMCP + CodeMode bindings) ---
        classic(
            "tz_read",
            "Read file(s) under allowed roots: compact visible output plus exact tz:// recovery refs.",
            &["read"],
            M::ReadOnly,
            K::Medium,
            R::Blob,
            C::Deadline,
            "zero.read",
            &["read", "exact-refs", "line-range", "shared-cas"],
            "material",
            read_schema(),
            read_errors(),
        ),
        classic(
            "tz_find",
            "Search file contents for a literal substring and return compact, recoverable matches.",
            &["find"],
            M::ReadOnly,
            K::Medium,
            R::Blob,
            C::Deadline,
            "zero.find",
            &["search", "literal", "exact-refs", "shared-cas"],
            "material",
            search_schema("Literal substring to search for."),
            search_errors(),
        ),
        classic(
            "tz_grep",
            "Grep-style exact-first content search: regex when ripgrep is active, literal otherwise.",
            &["grep"],
            M::ReadOnly,
            K::Medium,
            R::Blob,
            C::Deadline,
            "zero.grep",
            &["search", "regex", "exact-refs", "shared-cas"],
            "material",
            search_schema(
                "Search pattern: regex under the ripgrep backend, literal substring under the internal fallback.",
            ),
            search_errors(),
        ),
        classic(
            "tz_recall",
            "Search every payload already stored in the recovery cache.",
            &["recall"],
            M::ReadOnly,
            K::Cheap,
            R::Blob,
            C::Cooperative,
            "zero.recall",
            &["search", "cache", "exact-refs", "shared-cas"],
            "material",
            recall_schema(),
            read_errors(),
        ),
        classic(
            "tz_batch",
            "Run several TokenZero ops in one call: one combined capsule, per-op sections, unioned refs.",
            &["batch"],
            M::WorkspaceMutating,
            K::Heavy,
            R::Multi,
            C::Deadline,
            "zero.batch",
            &["batch", "exact-refs"],
            "execution",
            batch_schema(),
            read_errors(),
        ),
        classic(
            "tz_fetch",
            "Fetch an http(s) URL via curl with a TTL cache and exact tz:// refs.",
            &["fetch"],
            M::StoreOnly,
            K::Heavy,
            R::Blob,
            C::Deadline,
            "zero.fetch",
            &["fetch", "web", "cache", "exact-refs"],
            "web",
            fetch_schema(),
            fetch_errors(),
        ),
        classic_ex(
            "tz_glob",
            "List file paths matching a glob pattern (no contents).",
            &["glob"],
            M::ReadOnly,
            K::Cheap,
            R::Blob,
            C::Cooperative,
            "zero.glob",
            &["discover", "glob", "shared-cas"],
            "material",
            glob_schema(),
            ref_first_results(),
            read_errors(),
            json!({ "pattern": ["glob", "query"] }),
        ),
        classic(
            "tz_tree",
            "Inspect a bounded directory tree for orientation.",
            &["tree"],
            M::ReadOnly,
            K::Cheap,
            R::Blob,
            C::Cooperative,
            "zero.tree",
            &["discover", "tree", "shared-cas"],
            "material",
            tree_schema(),
            read_errors(),
        ),
        classic(
            "tz_edit",
            "Apply multi-hunk find/replace edits to one file atomically with undo via tz:// ref.",
            &["edit"],
            M::WorkspaceMutating,
            K::Medium,
            R::Blob,
            C::None,
            "zero.edit",
            &["write", "atomic", "exact-refs"],
            "edit",
            edit_schema(),
            edit_errors(),
        ),
        classic_ex(
            "tz_shell",
            "Run a local command: compact output, exact stream refs, command_success telemetry.",
            &["shell"],
            M::WorkspaceMutating,
            K::Heavy,
            R::Blob,
            C::Deadline,
            "zero.shell",
            &["shell", "exact-refs", "command-success"],
            "execution",
            shell_schema(),
            ref_first_results(),
            shell_errors(),
            json!({
                "command": ["cmd", "input", "script"],
                "argv": ["args"],
                "timeout_seconds": ["timeout_secs", "timeout", "shell_timeout_seconds"]
            }),
        ),
        classic_ex(
            "tz_ingest",
            "Store external text behind exact tz:// refs and return a compact capsule.",
            &["ingest"],
            M::StoreOnly,
            K::Cheap,
            R::Blob,
            C::None,
            "zero.ingest",
            &["ingest", "exact-refs"],
            "execution",
            text_schema("External text payload to store behind exact refs."),
            ref_first_results(),
            read_errors(),
            json!({ "text": ["input"] }),
        ),
        classic(
            "tz_expand",
            "Recover exact bytes from a tz://, fz://, or gz:// ref.",
            &["expand"],
            M::ReadOnly,
            K::Cheap,
            R::Blob,
            C::Deadline,
            "zero.token.expand",
            &[
                "expand",
                "exact-refs",
                "fragment-selectors",
                "symbol-anchors",
                "diff-baseline",
                "shared-cas",
            ],
            "material",
            expand_schema(),
            expand_errors(),
        ),
        classic_ex(
            "tz_mem",
            "Inspect local recovery-cache and configuration state.",
            &["mem"],
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            "zero.mem",
            &["diagnostic", "cache"],
            "execution",
            no_args_schema(),
            default_results(),
            read_errors(),
            json!({}),
        ),
        classic(
            "tz_cache_pack",
            "Build a daemonless prompt-cache pack with a stable prefix and volatile refs.",
            &["cache_pack", "cache-pack"],
            M::StoreOnly,
            K::Medium,
            R::Multi,
            C::Cooperative,
            "zero.cache_pack",
            &["cache", "prompt-cache"],
            "execution",
            cache_pack_schema(),
            read_errors(),
        ),
        classic_ex(
            "tz_rewrite",
            "Plan a conservative TokenZero-safe rewrite of a shell command without executing it.",
            &["rewrite"],
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            "zero.rewrite",
            &["diagnostic", "rewrite"],
            "execution",
            rewrite_schema(),
            default_results(),
            shell_errors(),
            json!({ "command": ["cmd", "input", "script"], "argv": ["args"] }),
        ),
        classic_ex(
            "tz_discover",
            "Report TokenZero filter and runtime readiness metadata.",
            &["discover"],
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            "zero.discover",
            &["diagnostic", "discovery"],
            "execution",
            no_args_schema(),
            default_results(),
            read_errors(),
            json!({}),
        ),
        // --- CodeMode MCP control tools ---
        op(
            "tz_execute_code",
            "Execute a TokenZero CodeMode recipe, JSON plan, or JavaScript plan.",
            &[],
            M::WorkspaceMutating,
            K::Heavy,
            R::Execution,
            C::Deadline,
            MigrationStatus::CodemodeControl,
            SurfaceExposure {
                fastmcp_tool: false,
                codemode_mcp_tool: true,
                codemode_binding: None,
                resource_uri: None,
            },
            &["codemode", "plan-execution", "sandboxed"],
            "codemode",
            execute_code_schema(),
            default_results(),
            shell_errors(),
            json!({}),
        ),
        op(
            "tz_codemode_search",
            "Search the TokenZero CodeMode method catalog.",
            &[],
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            MigrationStatus::CodemodeControl,
            SurfaceExposure {
                fastmcp_tool: false,
                codemode_mcp_tool: true,
                codemode_binding: Some("codemode.search"),
                resource_uri: None,
            },
            &["codemode", "catalog-search", "read-only"],
            "codemode",
            codemode_search_schema(),
            default_results(),
            read_errors(),
            json!({}),
        ),
        op(
            "tz_codemode_describe",
            "Describe a TokenZero CodeMode method or capabilities manifest.",
            &[],
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            MigrationStatus::CodemodeControl,
            SurfaceExposure {
                fastmcp_tool: false,
                codemode_mcp_tool: true,
                codemode_binding: Some("codemode.describe"),
                resource_uri: None,
            },
            &["codemode", "catalog-describe", "read-only"],
            "codemode",
            codemode_describe_schema(),
            default_results(),
            read_errors(),
            json!({}),
        ),
        op(
            "tz_report_tool_issue",
            "Record a field issue against a CodeMode/TokenZero tool name.",
            &["report_tool_issue", "report-tool-issue"],
            M::StoreOnly,
            K::Cheap,
            R::None,
            C::None,
            MigrationStatus::Canonical,
            classic_surface(None, true),
            &["diagnostic", "report"],
            "codemode",
            report_tool_issue_schema(),
            default_results(),
            read_errors(),
            json!({
                "tool": ["name", "tool_name", "surface"],
                "summary": ["message", "title"],
                "detail": ["body", "repro", "context"]
            }),
        ),
        // --- CodeMode-only domain helpers (bindings without separate FastMCP tools) ---
        binding_ex(
            "zero.token.compact",
            "Store arbitrary text/data behind a tz:// recovery ref via ingest.",
            &["zero.compact"],
            M::StoreOnly,
            K::Cheap,
            R::Blob,
            C::None,
            &["ingest", "exact-refs", "codemode"],
            json!({
                "type": "object",
                "properties": { "data": {} },
                "required": ["data"]
            }),
            ref_first_results(),
            read_errors(),
        ),
        binding(
            "zero.token.compactMany",
            "Batch compact many payloads in one CodeMode step.",
            M::StoreOnly,
            K::Medium,
            R::Multi,
            C::Cooperative,
            &["ingest", "batch", "codemode"],
            json!({
                "type": "object",
                "properties": { "items": { "type": "array" } },
                "required": ["items"]
            }),
            ref_first_results(),
            read_errors(),
        ),
        binding(
            "zero.token.expandMany",
            "Batch expand many tz:// refs in one CodeMode step.",
            M::ReadOnly,
            K::Medium,
            R::Multi,
            C::Deadline,
            &["expand", "batch", "codemode"],
            json!({
                "type": "object",
                "properties": { "items": { "type": "array" } },
                "required": ["items"]
            }),
            ref_first_results(),
            expand_errors(),
        ),
        binding(
            "zero.token.dedupe",
            "Deduplicate JSON/string values while preserving first occurrence order.",
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            &["codemode"],
            json!({
                "type": "object",
                "properties": { "items": { "type": "array" } },
                "required": ["items"]
            }),
            default_results(),
            read_errors(),
        ),
        binding(
            "zero.pipe",
            "Execute a sequence of operations with result threading (_prev auto-binding).",
            M::WorkspaceMutating,
            K::Heavy,
            R::Multi,
            C::Deadline,
            &["codemode", "pipeline"],
            json!({
                "type": "object",
                "properties": {
                    "steps": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "method": {"type": "string"},
                                "args": {"type": "array"}
                            },
                            "required": ["method"]
                        }
                    }
                },
                "required": ["steps"]
            }),
            default_results(),
            shell_errors(),
        ),
        binding(
            "zero.pick",
            "Extract specific keys from an object value.",
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            &["codemode"],
            json!({
                "type": "object",
                "properties": {
                    "source": {"type": "object"},
                    "keys": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["source", "keys"]
            }),
            default_results(),
            read_errors(),
        ),
        binding(
            "zero.filter_lines",
            "Filter lines in a text value by substring match.",
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            &["codemode"],
            json!({
                "type": "object",
                "properties": {
                    "source": {},
                    "pattern": {"type": "string"}
                },
                "required": ["source", "pattern"]
            }),
            default_results(),
            read_errors(),
        ),
        binding(
            "zero.compact_max",
            "Max compression with guaranteed byte-exact recovery.",
            M::StoreOnly,
            K::Medium,
            R::Blob,
            C::None,
            &["codemode", "exact-refs"],
            json!({
                "type": "object",
                "properties": { "data": {} },
                "required": ["data"]
            }),
            ref_first_results(),
            read_errors(),
        ),
        binding(
            "zero.count",
            "Count lines in a text value or items in an array.",
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            &["codemode"],
            json!({
                "type": "object",
                "properties": { "x": {} },
                "required": ["x"]
            }),
            default_results(),
            read_errors(),
        ),
        binding(
            "zero.first",
            "Return the first line or array item, or the first n lines/items.",
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            &["codemode"],
            json!({
                "type": "object",
                "properties": {
                    "x": {},
                    "n": {"type": "integer", "minimum": 1}
                },
                "required": ["x"]
            }),
            default_results(),
            read_errors(),
        ),
        binding(
            "zero.verdict",
            "Return a compact one-line verdict object.",
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            &["codemode"],
            json!({
                "type": "object",
                "properties": {
                    "ok": {},
                    "detail": {"type": "string"}
                },
                "required": ["ok"]
            }),
            default_results(),
            read_errors(),
        ),
        binding(
            "zero.raw",
            "Opt one final-return value out of automatic ref-first compaction.",
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            &["codemode"],
            json!({
                "type": "object",
                "properties": { "value": {} },
                "required": ["value"]
            }),
            default_results(),
            read_errors(),
        ),
        binding(
            "zero.count_tokens",
            "Count tokens, bytes, and lines in a value without storing it.",
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            &["codemode", "introspection"],
            json!({
                "type": "object",
                "properties": { "data": {} },
                "required": ["data"]
            }),
            default_results(),
            read_errors(),
        ),
        binding(
            "zero.assert",
            "Fail the plan immediately if condition is falsy.",
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            &["codemode", "guard"],
            json!({
                "type": "object",
                "properties": {
                    "condition": {},
                    "message": {"type": "string"}
                },
                "required": ["condition"]
            }),
            default_results(),
            &[DomainErrorKind::Validation, DomainErrorKind::Policy],
        ),
        binding(
            "codemode.journalDoctor",
            "List unresolved plan journals and safe recovery advice.",
            M::ReadOnly,
            K::Cheap,
            R::Session,
            C::None,
            &["codemode", "journal"],
            no_args_schema(),
            default_results(),
            read_errors(),
        ),
        binding(
            "codemode.journalInspect",
            "Inspect a redacted durable plan journal by execution id.",
            M::ReadOnly,
            K::Cheap,
            R::Session,
            C::None,
            &["codemode", "journal"],
            json!({
                "type": "object",
                "properties": { "execution_id": {"type": "string", "minLength": 1} },
                "required": ["execution_id"]
            }),
            default_results(),
            read_errors(),
        ),
        binding(
            "codemode.journalResume",
            "Validate that an unresolved journal can be safely resumed.",
            M::ReadOnly,
            K::Cheap,
            R::Session,
            C::None,
            &["codemode", "journal"],
            json!({
                "type": "object",
                "properties": { "execution_id": {"type": "string", "minLength": 1} },
                "required": ["execution_id"]
            }),
            default_results(),
            read_errors(),
        ),
        binding(
            "codemode.journalRollback",
            "CAS-verified reverse-order rollback of an unresolved plan journal.",
            M::WorkspaceMutating,
            K::Medium,
            R::Session,
            C::Cooperative,
            &["codemode", "journal", "rollback"],
            json!({
                "type": "object",
                "properties": { "execution_id": {"type": "string", "minLength": 1} },
                "required": ["execution_id"]
            }),
            default_results(),
            edit_errors(),
        ),
        binding(
            "codemode.limits",
            "Return active CodeMode sandbox, output, ref, and operation limits.",
            M::ReadOnly,
            K::Cheap,
            R::None,
            C::None,
            &["codemode", "limits"],
            no_args_schema(),
            default_results(),
            read_errors(),
        ),
        // --- Resources ---
        resource(
            "resource.capabilities",
            "Discover tool clusters, aliases, protocol versions, and next recommended calls.",
            "resource://tokenzero/capabilities",
            &["resource"],
            R::None,
        ),
        resource(
            "resource.tools",
            "Complete tool catalog with schemas and agent-oriented descriptions.",
            "resource://tokenzero/tools",
            &["resource"],
            R::None,
        ),
        resource(
            "resource.roots",
            "File-system roots that read/find/tree/shell cwd operations may access.",
            "resource://tokenzero/roots",
            &["resource", "policy"],
            R::None,
        ),
        resource(
            "resource.modes",
            "Accepted mode values for compacting, diagnostics, exact recovery, and pass-through.",
            "resource://tokenzero/modes",
            &["resource"],
            R::None,
        ),
        resource(
            "resource.codemode",
            "Full CodeMode method catalog with signatures and discovery prefixes.",
            "resource://tokenzero/codemode",
            &["resource", "codemode"],
            R::None,
        ),
        resource(
            "resource.cache",
            "Local recovery-cache and shell-output retention configuration.",
            "resource://tokenzero/cache",
            &["resource", "cache"],
            R::None,
        ),
        resource(
            "resource.session_boot",
            "Bounded manifest+delta boot capsule and component token attribution.",
            "resource://tokenzero/session-boot",
            &["resource", "session"],
            R::Session,
        ),
        resource(
            "resource.metrics",
            "Per-tool call counts, error counts, slow-call counts, and latency.",
            "resource://tokenzero/metrics",
            &["resource", "telemetry"],
            R::None,
        ),
        resource(
            "resource.shell_contract",
            "Shell transport, command-success, exact-ref, timeout, and retry semantics.",
            "resource://tokenzero/shell-contract",
            &["resource", "shell", "policy"],
            R::None,
        ),
    ];

    ops.sort_by(|a, b| a.name.cmp(b.name));
    ops
}
