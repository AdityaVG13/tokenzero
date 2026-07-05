use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "tokenzero",
    version,
    about = "Rust TokenZero RACC runtime",
    after_help = "Agent surfaces:\n  tokenzero capabilities --json   Print the machine-readable CLI contract\n  tokenzero robot-docs guide      Print a paste-ready guide for agents\n  tokenzero run --json -- <cmd>   Run commands with status-truth telemetry"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    #[command(about = "Read bounded file content with exact recovery refs")]
    Read(ReadArgs),
    #[command(
        about = "Search local text and return compact matches",
        alias = "search"
    )]
    Find(FindArgs),
    #[command(about = "Alias for find")]
    Grep(FindArgs),
    #[command(about = "List matching paths without dumping file contents")]
    Glob(GlobArgs),
    #[command(about = "Inspect a bounded directory tree")]
    Tree(TreeArgs),
    #[command(about = "Apply multi-hunk find/replace edits to one file with undo refs")]
    Edit(EditArgs),
    #[command(about = "Search payloads already stored in the recovery cache")]
    Recall(RecallArgs),
    #[command(about = "Fetch an http(s) URL via curl with a TTL cache and exact refs")]
    Fetch(FetchArgs),
    #[command(
        alias = "shell",
        alias = "rn",
        about = "Run a command with status-truth telemetry"
    )]
    Run(RunArgs),
    #[command(about = "Ingest text or a file into a compact TokenZero capsule")]
    Ingest(IngestArgs),
    #[command(about = "Recover exact bytes from a prior TokenZero ref")]
    Expand(ExpandArgs),
    #[command(about = "Inspect recovery-cache state")]
    Mem(CommonArgs),
    #[command(about = "Rewrite a shell command with TokenZero-safe routing")]
    #[command(alias = "rewrite-command")]
    Rewrite(RewriteArgs),
    #[command(about = "Agent-harness hook adapters: stdin JSON in, decision JSON out")]
    Hook(HookArgs),
    #[command(about = "List local TokenZero tool-discovery metadata")]
    Discover(CommonArgs),
    #[command(about = "Check local TokenZero health and next steps")]
    Doctor(DoctorArgs),
    #[command(about = "Print local TokenZero usage statistics")]
    Stats(CommonArgs),
    #[command(about = "Inspect or sync local Pulse telemetry")]
    Pulse(PulseArgs),
    #[command(about = "Inspect or prune TokenZero recovery-cache state")]
    Cache(CacheArgs),
    #[command(about = "Plan or apply local integration writes with rollback data")]
    Install(InstallArgs),
    #[command(about = "Compatibility alias for install --mcp --agent <name>")]
    Init(InitArgs),
    #[command(
        about = "Inspect AI client TokenZero integration state",
        alias = "client"
    )]
    Clients(ClientsArgs),
    #[command(name = "client-status", about = "Alias for clients detect")]
    ClientStatus(ClientStatusArgs),
    #[command(
        about = "Print the machine-readable CLI contract for agents",
        alias = "capability",
        alias = "capabilites"
    )]
    Capabilities(CapabilitiesArgs),
    #[command(
        name = "robot-docs",
        about = "Print in-tool documentation for agents",
        alias = "robot-doc",
        alias = "robotdocs"
    )]
    RobotDocs(RobotDocsArgs),
    #[command(name = "cache-pack")]
    CachePack(CachePackArgs),
    Bench(BenchArgs),
    #[command(name = "mcp-server")]
    McpServer(McpServerArgs),
    #[command(name = "mcp-smoke")]
    McpSmoke(ArtifactArgs),
    #[command(name = "mcp-soak")]
    McpSoak(ArtifactArgs),
    #[command(name = "exact-recovery-shell")]
    ExactRecoveryShell(ExactRecoveryShellArgs),
    #[command(name = "exact-recovery-audit")]
    ExactRecoveryAudit(ExactRecoveryAuditArgs),
    #[command(
        name = "codemode",
        about = "Compose multi-step plans on the same base tools as MCP (fewer round-trips, Cloudflare-style)",
        long_about = "Execute JS-like plans that compose the same TokenZero operations as MCP (zero.read, zero.find, zero.shell, ...) in one call for faster multi-step workflows.\n\n\
            Discovery: tokenzero codemode 'search:read' | tokenzero codemode 'describe:zero.read'\n\n\
            Cache: defaults to codemode-recovery.json (separate from MCP/CLI recovery-cache.json). \
            Pass the same --cache-path to codemode and expand when refs must cross surfaces."
    )]
    CodeMode(CodeModeArgs),
    #[command(name = "harm-eval")]
    HarmEval(ArtifactArgs),
    #[command(name = "protected-anchor-audit")]
    ProtectedAnchorAudit(ProtectedAnchorAuditArgs),
    #[command(name = "false-success-shell")]
    FalseSuccessShell(FalseSuccessShellArgs),
    #[command(name = "repo-inventory")]
    RepoInventory(ArtifactArgs),
    #[command(name = "prompt-cache-pack")]
    PromptCachePack(ArtifactArgs),
    #[command(name = "install-smoke")]
    InstallSmoke(InstallSmokeArgs),
    #[command(name = "package-audit")]
    PackageAudit(PackageAuditArgs),
    #[command(name = "shell-matrix")]
    ShellMatrix(ShellMatrixArgs),
    #[command(name = "os-reach-audit")]
    OsReachAudit(OsReachAuditArgs),
    #[command(name = "os-release-artifact")]
    OsReleaseArtifact(OsReleaseArtifactArgs),
    #[command(name = "one-shot-eval")]
    OneShotEval(OneShotEvalArgs),
    #[command(name = "source-currency-audit")]
    SourceCurrencyAudit(SourceCurrencyAuditArgs),
    #[command(name = "adapter-approval-audit")]
    AdapterApprovalAudit(AdapterApprovalAuditArgs),
    #[command(name = "adapter-approval-template")]
    AdapterApprovalTemplate(AdapterApprovalTemplateArgs),
    #[command(name = "claim-audit")]
    ClaimAudit(ClaimAuditArgs),
    #[command(name = "completion-audit")]
    CompletionAudit(CompletionAuditArgs),
    #[command(name = "security-privacy-audit")]
    SecurityPrivacyAudit(SecurityPrivacyAuditArgs),
    #[command(name = "artifact-handoff")]
    ArtifactHandoff(ArtifactHandoffArgs),
    Reach(ReachArgs),
    #[command(name = "ws-skeleton")]
    WsSkeleton(WsSkeletonArgs),
    Quote(QuoteArgs),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct CommonArgs {
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) cache_path: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct ToolArgs {
    #[arg(long, default_value = "auto")]
    pub(crate) mode: String,
    #[arg(long)]
    pub(crate) budget: Option<usize>,
    #[arg(long)]
    pub(crate) allowed_root: Vec<PathBuf>,
    #[arg(long)]
    pub(crate) cache_path: Option<PathBuf>,
    #[arg(long, alias = "timeout", alias = "timout", value_name = "SECONDS")]
    pub(crate) timeout_seconds: Option<u64>,
    #[arg(long, alias = "jsno", alias = "jason")]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ReadArgs {
    #[arg(value_name = "PATH")]
    pub(crate) path: Vec<PathBuf>,
    #[arg(long)]
    pub(crate) paths_from: Option<PathBuf>,
    #[arg(long, default_value_t = 20)]
    pub(crate) max_files: usize,
    #[arg(long, default_value_t = 4000)]
    pub(crate) max_visible_tokens: usize,
    #[arg(long)]
    pub(crate) start_line: Option<usize>,
    #[arg(long)]
    pub(crate) end_line: Option<usize>,
    #[arg(long)]
    pub(crate) raw: bool,
    #[command(flatten)]
    pub(crate) tool: ToolArgs,
}

#[derive(Debug, Args)]
pub(crate) struct FindArgs {
    pub(crate) query: String,
    pub(crate) path: Vec<PathBuf>,
    #[arg(long, default_value_t = 20)]
    pub(crate) max_files: usize,
    #[arg(long, default_value_t = 4000)]
    pub(crate) max_visible_tokens: usize,
    #[command(flatten)]
    pub(crate) tool: ToolArgs,
}

#[derive(Debug, Args)]
pub(crate) struct RecallArgs {
    pub(crate) query: String,
    #[arg(long, default_value_t = 50)]
    pub(crate) max_hits: usize,
    #[arg(long, default_value_t = 4000)]
    pub(crate) max_visible_tokens: usize,
    #[command(flatten)]
    pub(crate) tool: ToolArgs,
}

#[derive(Debug, Args)]
pub(crate) struct FetchArgs {
    pub(crate) url: String,
    /// Serve a cached body younger than this without touching the network.
    #[arg(long)]
    pub(crate) ttl_seconds: Option<usize>,
    /// Bypass the TTL cache and re-fetch.
    #[arg(long)]
    pub(crate) fresh: bool,
    #[arg(long, default_value_t = 4000)]
    pub(crate) max_visible_tokens: usize,
    #[command(flatten)]
    pub(crate) tool: ToolArgs,
}

#[derive(Debug, Args)]
pub(crate) struct GlobArgs {
    pub(crate) pattern: String,
    pub(crate) path: Vec<PathBuf>,
    #[arg(long, default_value_t = 200)]
    pub(crate) max_files: usize,
    #[arg(long, default_value_t = 4000)]
    pub(crate) max_visible_tokens: usize,
    #[arg(long)]
    pub(crate) include_hidden: bool,
    #[command(flatten)]
    pub(crate) tool: ToolArgs,
}

#[derive(Debug, Args)]
pub(crate) struct TreeArgs {
    pub(crate) path: Vec<PathBuf>,
    #[arg(long, default_value_t = 2)]
    pub(crate) depth: usize,
    #[arg(long, default_value_t = 200)]
    pub(crate) max_files: usize,
    #[arg(long, default_value_t = 4000)]
    pub(crate) max_visible_tokens: usize,
    #[arg(long)]
    pub(crate) include_hidden: bool,
    #[command(flatten)]
    pub(crate) tool: ToolArgs,
}

#[derive(Debug, Args)]
pub(crate) struct EditArgs {
    #[arg(value_name = "PATH")]
    pub(crate) path: PathBuf,
    /// JSON array of {find, replace, replace_all?} hunks.
    #[arg(long = "edits-json", value_name = "JSON")]
    pub(crate) edits_json: Option<String>,
    /// Read the edits JSON from stdin instead of --edits-json.
    #[arg(long)]
    pub(crate) stdin: bool,
    /// Create a new file: one hunk with empty find; replace is the content.
    #[arg(long)]
    pub(crate) create: bool,
    /// Validate and render the hunk diff without writing.
    #[arg(long)]
    pub(crate) dry_run: bool,
    #[arg(long, default_value_t = 4000)]
    pub(crate) max_visible_tokens: usize,
    #[command(flatten)]
    pub(crate) tool: ToolArgs,
}

#[derive(Debug, Args)]
pub(crate) struct RunArgs {
    #[arg(last = true)]
    pub(crate) command: Vec<String>,
    #[arg(long)]
    pub(crate) cwd: Option<PathBuf>,
    #[arg(long)]
    pub(crate) rewrite: Option<String>,
    #[arg(long)]
    pub(crate) no_rewrite: bool,
    #[arg(long)]
    pub(crate) stdin: bool,
    #[arg(long = "env")]
    pub(crate) env_overrides: Vec<String>,
    #[arg(long)]
    pub(crate) explain_runtime: bool,
    #[arg(long)]
    pub(crate) runtime_platform: Option<String>,
    #[command(flatten)]
    pub(crate) tool: ToolArgs,
}

#[derive(Debug, Args)]
pub(crate) struct IngestArgs {
    pub(crate) input: Option<PathBuf>,
    #[arg(long)]
    pub(crate) stdin: bool,
    #[arg(long, default_value = "auto")]
    pub(crate) kind: String,
    #[command(flatten)]
    pub(crate) tool: ToolArgs,
}

#[derive(Debug, Args)]
pub(crate) struct ExpandArgs {
    #[arg(value_name = "REF")]
    pub(crate) refs: Vec<String>,
    #[arg(long)]
    pub(crate) refs_from: Option<PathBuf>,
    #[arg(long)]
    pub(crate) selector: Option<String>,
    #[arg(long)]
    pub(crate) raw: bool,
    #[arg(long)]
    pub(crate) summary: bool,
    #[arg(long)]
    pub(crate) force: bool,
    #[arg(long)]
    pub(crate) start_line: Option<usize>,
    #[arg(long)]
    pub(crate) end_line: Option<usize>,
    #[arg(long)]
    pub(crate) line: Option<usize>,
    #[arg(long)]
    pub(crate) lines: Option<String>,
    #[arg(long)]
    pub(crate) around: Option<String>,
    #[arg(long)]
    pub(crate) anchor_kind: Option<String>,
    #[arg(long)]
    pub(crate) symbol: Option<String>,
    #[arg(long)]
    pub(crate) cache_path: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct RewriteArgs {
    /// Command string; alternative to trailing `-- <command...>`.
    pub(crate) command: Option<String>,
    /// Command after `--`, matching `tokenzero run -- <command...>`.
    #[arg(last = true)]
    pub(crate) argv: Vec<String>,
    #[arg(long, default_value = "safe")]
    pub(crate) mode: String,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct HookArgs {
    #[command(subcommand)]
    pub(crate) target: HookTarget,
}

#[derive(Debug, Subcommand)]
pub(crate) enum HookTarget {
    #[command(
        name = "claude-code",
        about = "Claude Code PreToolUse adapter: wraps Bash commands in `tokenzero run` (fail-open, always exits 0)"
    )]
    ClaudeCode(HookClaudeCodeArgs),
    #[command(
        name = "claude-code-session-start",
        about = "Claude Code SessionStart adapter: restores a compact session pack after compaction/resume (fail-open, always exits 0)"
    )]
    ClaudeCodeSessionStart(HookSessionStartArgs),
}

#[derive(Debug, Args)]
pub(crate) struct HookClaudeCodeArgs {
    /// rewrite | guide | off. Unknown values pass through (fail-open).
    #[arg(long, default_value = "rewrite")]
    pub(crate) mode: String,
}

#[derive(Debug, Args)]
pub(crate) struct HookSessionStartArgs {
    /// Token budget for the restored session pack.
    #[arg(long, default_value_t = 600)]
    pub(crate) max_tokens: usize,
}

#[derive(Debug, Args)]
pub(crate) struct DoctorArgs {
    #[arg(long, global = true)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long, global = true)]
    pub(crate) cache_path: Option<PathBuf>,
    #[arg(long, global = true)]
    pub(crate) runtime: bool,
    #[arg(long, global = true)]
    pub(crate) json: bool,
    #[arg(long = "robot-triage", global = true)]
    pub(crate) robot_triage: bool,
    #[arg(long, global = true)]
    pub(crate) fix: bool,
    #[arg(long = "dry-run", global = true)]
    pub(crate) dry_run: bool,
    #[arg(long, global = true)]
    pub(crate) explain: Option<String>,
    #[command(subcommand)]
    pub(crate) command: Option<DoctorCommand>,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum DoctorCommand {
    #[command(about = "Run all doctor checks. Read-only. Default when omitted.")]
    Diagnose,
    #[command(about = "Apply supported doctor fixers with backups and actions.jsonl")]
    Fix,
    #[command(about = "Undo a prior doctor fixer run")]
    Undo {
        #[arg(value_name = "RUN_ID")]
        run_id: String,
    },
    #[command(name = "ls", about = "List local doctor run artifacts")]
    Ls,
    #[command(about = "Expand a current or known doctor finding")]
    Explain {
        #[arg(value_name = "FINDING_ID")]
        finding_id: String,
    },
    #[command(about = "Print machine-readable doctor contract")]
    Capabilities,
    #[command(
        about = "Print cheap liveness summary",
        alias = "status",
        alias = "statuz"
    )]
    Health,
    #[command(
        name = "robot-docs",
        alias = "robotdocs",
        about = "Print paste-ready doctor handbook for agents"
    )]
    RobotDocs,
}

#[derive(Debug, Args)]
pub(crate) struct PulseArgs {
    #[arg(long, global = true)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long, global = true)]
    pub(crate) json: bool,
    #[command(subcommand)]
    pub(crate) command: Option<PulseCommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PulseCommand {
    #[command(
        name = "stats",
        alias = "status",
        about = "Print local Pulse telemetry report"
    )]
    Stats,
    Sync,
    Doctor,
    #[command(name = "export-jsonl")]
    ExportJsonl(PulseExportArgs),
    #[command(name = "import-jsonl")]
    ImportJsonl(PulseImportArgs),
}

#[derive(Debug, Args)]
pub(crate) struct PulseExportArgs {
    #[arg(value_name = "OUTPUT")]
    pub(crate) output: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct PulseImportArgs {
    #[arg(value_name = "INPUT")]
    pub(crate) input: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct CacheArgs {
    #[command(subcommand)]
    pub(crate) command: CacheCommand,
}

#[derive(Debug, Args)]
pub(crate) struct CachePackArgs {
    #[arg(long, default_value = "agent")]
    pub(crate) scope: String,
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) cache_path: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct BenchArgs {
    #[command(subcommand)]
    pub(crate) command: BenchCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum BenchCommand {
    Competitors(BenchCompetitorsArgs),
}

#[derive(Debug, Args)]
pub(crate) struct BenchCompetitorsArgs {
    #[arg(long, default_value = "shell-heavy")]
    pub(crate) suite: String,
    #[arg(long)]
    pub(crate) output_json: Option<PathBuf>,
    #[arg(long)]
    pub(crate) adapter_approval_artifact: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum CacheCommand {
    #[command(alias = "statuz")]
    Status(CommonArgs),
    Prune(CachePruneArgs),
}

#[derive(Debug, Args)]
pub(crate) struct CachePruneArgs {
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) cache_path: Option<PathBuf>,
    #[arg(long)]
    pub(crate) apply: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct InstallArgs {
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) plan: bool,
    #[arg(long)]
    pub(crate) apply: bool,
    #[arg(long)]
    pub(crate) rollback: Option<String>,
    #[arg(long)]
    pub(crate) global: bool,
    #[arg(long)]
    pub(crate) mcp: bool,
    #[arg(long)]
    pub(crate) shell: bool,
    #[arg(long)]
    pub(crate) instructions: bool,
    #[arg(long)]
    pub(crate) cli: bool,
    /// Wire the Claude Code PreToolUse hook into .claude/settings.json.
    #[arg(long)]
    pub(crate) hooks: bool,
    /// Install the universal PATH shims under .tokenzero/shims/.
    #[arg(long)]
    pub(crate) shims: bool,
    #[arg(long = "agent", value_name = "AGENT")]
    pub(crate) agents: Vec<String>,
    #[arg(long)]
    pub(crate) grok: bool,
    /// MCP tool surface profile (always `classic`; CodeMode is a separate execution layer).
    #[arg(long, value_name = "SURFACE", default_value = "classic")]
    pub(crate) surface: String,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) global: bool,
    #[arg(long = "agent", value_name = "AGENT")]
    pub(crate) agents: Vec<String>,
    #[arg(long)]
    pub(crate) mcp: bool,
    #[arg(long)]
    pub(crate) shell: bool,
    #[arg(long)]
    pub(crate) instructions: bool,
    #[arg(long)]
    pub(crate) cli: bool,
    /// Wire the Claude Code PreToolUse hook into .claude/settings.json.
    #[arg(long)]
    pub(crate) hooks: bool,
    /// Install the universal PATH shims under .tokenzero/shims/.
    #[arg(long)]
    pub(crate) shims: bool,
    #[arg(long)]
    pub(crate) apply: bool,
    #[arg(long)]
    pub(crate) plan: bool,
    /// MCP tool surface profile (always `classic`; CodeMode is a separate execution layer).
    #[arg(long, value_name = "SURFACE", default_value = "classic")]
    pub(crate) surface: String,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ClientsArgs {
    #[command(subcommand)]
    pub(crate) command: ClientsCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ClientsCommand {
    #[command(about = "Detect configured TokenZero AI client surfaces")]
    Detect(ClientStatusArgs),
    #[command(about = "Scan this machine for AI harnesses TokenZero can adapt to")]
    Scan(ClientStatusArgs),
    #[command(about = "Plan TokenZero AI client integration writes")]
    Plan(ClientsPlanArgs),
    #[command(about = "Diagnose TokenZero AI client integration state")]
    Doctor(ClientStatusArgs),
    #[command(about = "Rollback a previous TokenZero client integration write")]
    Rollback(ClientsRollbackArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ClientStatusArgs {
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long = "agent", value_name = "AGENT")]
    pub(crate) agents: Vec<String>,
    #[arg(long)]
    pub(crate) grok: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ClientsPlanArgs {
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long, default_value = "standard")]
    pub(crate) profile: String,
    #[arg(long = "agent", value_name = "AGENT")]
    pub(crate) agents: Vec<String>,
    #[arg(long)]
    pub(crate) grok: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ClientsRollbackArgs {
    pub(crate) id: String,
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct CapabilitiesArgs {
    #[arg(long, alias = "jsno", alias = "jason")]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct RobotDocsArgs {
    #[command(subcommand)]
    pub(crate) command: RobotDocsCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum RobotDocsCommand {
    #[command(alias = "manual")]
    Guide,
    #[command(about = "Print canonical command quick reference for agents")]
    Commands,
    #[command(about = "Print copy-paste examples for common agent tasks")]
    Examples,
}

#[derive(Debug, Args)]
pub(crate) struct McpServerArgs {
    /// Launch mode: mcp exposes per-op tools; codemode exposes exactly the three CodeMode tools.
    #[arg(long, default_value = "mcp", value_name = "MODE")]
    pub(crate) mode: String,
    #[arg(long)]
    pub(crate) allowed_root: Vec<PathBuf>,
    #[arg(long)]
    pub(crate) cache_path: Option<PathBuf>,
    #[arg(long, default_value = "auto")]
    pub(crate) default_mode: String,
    #[arg(long, alias = "timeout", value_name = "SECONDS")]
    pub(crate) shell_timeout_seconds: Option<u64>,
    #[arg(long, value_name = "SECONDS")]
    pub(crate) idle_timeout_seconds: Option<u64>,
    /// Run a crash-transparent supervisor that owns the client stdio pipes
    /// and automatically respawns the inner MCP server if it ever dies.
    #[arg(long)]
    pub(crate) supervise: bool,
    /// Backward-compatible alias for --mode.
    #[arg(long, value_name = "SURFACE")]
    pub(crate) tool_surface: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct ArtifactArgs {
    #[arg(long, default_value = "results/current/rust_mcp_smoke.json")]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ShellMatrixArgs {
    #[arg(long, default_value = "results/current/tokenzero_shell_matrix.json")]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct FalseSuccessShellArgs {
    #[arg(
        long,
        default_value = "results/current/tokenzero_false_success_shell.json"
    )]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ExactRecoveryShellArgs {
    #[arg(
        long,
        default_value = "results/current/tokenzero_exact_recovery_shell.json"
    )]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ExactRecoveryAuditArgs {
    #[arg(
        long,
        default_value = "results/current/tokenzero_exact_recovery_audit.json"
    )]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ProtectedAnchorAuditArgs {
    #[arg(
        long,
        default_value = "results/current/tokenzero_protected_anchor_audit.json"
    )]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct OsReachAuditArgs {
    #[arg(long, default_value = "results/current/tokenzero_os_reach_audit.json")]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long, default_value = ".")]
    pub(crate) root: PathBuf,
    #[arg(long = "os-artifact")]
    pub(crate) os_artifact: Vec<PathBuf>,
    #[arg(long)]
    pub(crate) release_approval: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct OneShotEvalArgs {
    #[arg(long, default_value = "results/current/tokenzero_one_shot_eval.json")]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct OsReleaseArtifactArgs {
    #[arg(
        long,
        default_value = "results/current/tokenzero_os_release_artifact.json"
    )]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long, default_value = ".")]
    pub(crate) root: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct SourceCurrencyAuditArgs {
    #[arg(long, default_value = "results/current/tokenzero_source_currency.json")]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) refresh_ledger: Option<PathBuf>,
    #[arg(long)]
    pub(crate) refresh_git_heads: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct AdapterApprovalAuditArgs {
    #[arg(
        long,
        default_value = "results/current/tokenzero_adapter_approval_audit.json"
    )]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) approval_file: Option<PathBuf>,
    #[arg(long)]
    pub(crate) execution_approval: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct AdapterApprovalTemplateArgs {
    #[arg(
        long,
        default_value = "results/current/tokenzero_adapter_approval_file.json"
    )]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ClaimAuditArgs {
    #[arg(long, default_value = "results/current/tokenzero_claim_audit.json")]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) source_artifact: Option<PathBuf>,
    #[arg(long)]
    pub(crate) benchmark_artifact: Option<PathBuf>,
    #[arg(long)]
    pub(crate) adapter_approval_artifact: Option<PathBuf>,
    #[arg(long)]
    pub(crate) recovery_artifact: Option<PathBuf>,
    #[arg(long)]
    pub(crate) task_success_artifact: Option<PathBuf>,
    #[arg(long)]
    pub(crate) os_artifact: Option<PathBuf>,
    #[arg(long)]
    pub(crate) release_approval: bool,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct CompletionAuditArgs {
    #[arg(
        long,
        default_value = "results/current/tokenzero_completion_audit.json"
    )]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct SecurityPrivacyAuditArgs {
    #[arg(
        long,
        default_value = "results/current/tokenzero_security_privacy_audit.json"
    )]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ArtifactHandoffArgs {
    #[arg(
        long,
        default_value = "results/current/tokenzero_artifact_handoff.json"
    )]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ReachArgs {
    #[arg(long, default_value = ".")]
    pub(crate) root: PathBuf,
    #[arg(long)]
    pub(crate) output_json: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct WsSkeletonArgs {
    #[arg(long, default_value = "results/current/tokenzero_ws_001.json")]
    pub(crate) output_json: PathBuf,
    #[arg(long)]
    pub(crate) output_md: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct InstallSmokeArgs {
    #[arg(long)]
    pub(crate) output_json: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct PackageAuditArgs {
    #[arg(long, default_value = ".")]
    pub(crate) dist: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct QuoteArgs {
    #[arg(long)]
    pub(crate) platform: String,
    #[arg(last = true)]
    pub(crate) args: Vec<String>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct CodeModeArgs {
    /// CodeMode plan (JS-style zero.token.compact / expand) as a positional argument.
    #[arg(value_name = "PLAN")]
    pub(crate) plan: Option<String>,
    /// CodeMode plan as an explicit flag; kept for router compatibility.
    #[arg(short = 'p', long = "plan", value_name = "PLAN")]
    pub(crate) plan_flag: Option<String>,
    /// Read plan from a file instead of inline. Supports .txt and .js extensions.
    #[arg(long = "plan-file", value_name = "PATH")]
    pub(crate) plan_file: Option<PathBuf>,
    /// Workspace root used for CodeMode file, shell, and recovery-cache boundaries.
    #[arg(long)]
    pub(crate) root: Option<PathBuf>,
    /// Additional allowed roots for plans that must intentionally cross the workspace boundary.
    #[arg(long)]
    pub(crate) allowed_root: Vec<PathBuf>,
    /// Override the CodeMode recovery cache path.
    #[arg(long)]
    pub(crate) cache_path: Option<PathBuf>,
    /// Maximum visible tokens for each underlying TokenZero operation.
    #[arg(long, default_value_t = 4000)]
    pub(crate) max_visible_tokens: usize,
    #[arg(long, alias = "timeout", alias = "timout", value_name = "SECONDS")]
    pub(crate) timeout_seconds: Option<u64>,
    #[arg(long)]
    pub(crate) json: bool,
}

impl CodeModeArgs {
    pub(crate) fn plan_text(&self) -> Result<String, std::io::Error> {
        if let Some(path) = &self.plan_file {
            return std::fs::read_to_string(path);
        }
        Ok(self
            .plan_flag
            .as_deref()
            .or(self.plan.as_deref())
            .unwrap_or("")
            .to_string())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cli_args_do_not_import_cli_monolith() {
        let source = include_str!("cli_args.rs");
        // The test module itself uses super::* but the non-test code must not.
        let non_test: &str = source.split("#[cfg(test)]").next().unwrap_or(source);
        let forbidden_imports = ["use crate::main", "crate::main::"];
        for forbidden in forbidden_imports {
            assert!(
                !non_test.contains(forbidden),
                "cli_args.rs must not back-import the CLI monolith: {forbidden}"
            );
        }
    }

    use super::CodeModeArgs;

    #[test]
    fn plan_file_reads_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let plan_path = dir.path().join("test.plan.js");
        std::fs::write(&plan_path, r#"await zero.compact("from file")"#).unwrap();
        let args = CodeModeArgs {
            plan: None,
            plan_flag: None,
            plan_file: Some(plan_path),
            root: None,
            allowed_root: Vec::new(),
            cache_path: None,
            max_visible_tokens: 4000,
            timeout_seconds: None,
            json: false,
        };
        let text = args.plan_text().unwrap();
        assert!(text.contains("from file"));
    }

    #[test]
    fn plan_text_prefers_flag_over_positional() {
        let args = CodeModeArgs {
            plan: Some("positional".to_string()),
            plan_flag: Some("flag_wins".to_string()),
            plan_file: None,
            root: None,
            allowed_root: Vec::new(),
            cache_path: None,
            max_visible_tokens: 4000,
            timeout_seconds: None,
            json: false,
        };
        assert_eq!(args.plan_text().unwrap(), "flag_wins");
    }
}
