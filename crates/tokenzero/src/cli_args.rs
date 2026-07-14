use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

macro_rules! define_args {
    (
        $(
            $(#[$struct_attr:meta])*
            $name:ident {
                $(
                    $(#[$field_doc:meta])*
                    $field:ident: $ty:ty $(=> $field_attr:meta)*
                ),* $(,)?
            }
        )*
    ) => {
        $(
            #[derive(Debug, Args)]
            $(#[$struct_attr])*
            pub(crate) struct $name {
                $(
                    $(#[$field_doc])*
                    $(#[$field_attr])*
                    pub(crate) $field: $ty,
                )*
            }
        )*
    };
}

macro_rules! define_subcommands {
    ($(
        $name:ident {
            $(
                $(#[$variant_doc:meta])*
                $variant:ident $(($arg:ty))? $(=> $variant_attr:meta)*
            ),* $(,)?
        }
    )*) => {$(
        #[derive(Debug, Subcommand)]
        pub(crate) enum $name {$(
            $(#[$variant_doc])*
            $(#[$variant_attr])*
            $variant $(($arg))?,
        )*}
    )*};
}

macro_rules! artifact_args {
    ($($name:ident => $default:literal),* $(,)?) => {
        $(
            #[derive(Debug, Args)]
            pub(crate) struct $name {
                #[arg(long, default_value = $default)]
                pub(crate) output_json: PathBuf,
                #[arg(long)]
                pub(crate) output_md: Option<PathBuf>,
                #[arg(long)]
                pub(crate) json: bool,
            }
        )*
    };
}

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

#[derive(Debug, Subcommand)]
pub(crate) enum SessionLedgerCommand {
    #[command(about = "Print per-session cost breakdown (mass × turns)")]
    Stats,
    #[command(about = "Export ledger as JSON array")]
    Export,
    #[command(about = "Print the stable schema for the session ledger")]
    Schema,
    #[command(about = "Query the response ledger")]
    Query {
        #[command(subcommand)]
        query: LedgerQueryCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum LedgerQueryCommand {
    #[command(about = "Aggregate visible token cost for one repo over a time window")]
    Repo {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long, default_value_t = 30)]
        days: u64,
    },
    #[command(
        name = "version-delta",
        about = "Compare visible token cost between crate versions"
    )]
    VersionDelta {
        #[arg(long)]
        baseline: String,
        #[arg(long)]
        candidate: String,
        #[arg(long, default_value_t = 30)]
        days: u64,
    },
    #[command(
        name = "agent-spend",
        about = "Aggregate visible token cost by agent identity"
    )]
    AgentSpend {
        #[arg(long, default_value_t = 30)]
        days: u64,
    },
}

define_subcommands! {
    Commands {
        Read(ReadArgs) => command(about = "Read bounded file content with exact recovery refs"),
        Find(FindArgs) => command( about = "Search local text and return compact matches", alias = "search" ),
        Grep(FindArgs) => command(about = "Alias for find"),
        Glob(GlobArgs) => command(about = "List matching paths without dumping file contents"),
        Tree(TreeArgs) => command(about = "Inspect a bounded directory tree"),
        Edit(EditArgs) => command(about = "Apply multi-hunk find/replace edits to one file with undo refs"),
        Recall(RecallArgs) => command(about = "Search payloads already stored in the recovery cache"),
        Fetch(FetchArgs) => command(about = "Fetch an http(s) URL via curl with a TTL cache and exact refs"),
        Run(RunArgs) => command( alias = "shell", alias = "rn", about = "Run a command with status-truth telemetry" ),
        Ingest(IngestArgs) => command(about = "Ingest text or a file into a compact TokenZero capsule"),
        Expand(ExpandArgs) => command(about = "Recover exact bytes from a prior TokenZero ref"),
        SessionOpen(CommonArgs) => command(name = "session-open", about = "Open a bounded manifest+delta session"),
        Mem(CommonArgs) => command(about = "Inspect recovery-cache state"),
        Rewrite(RewriteArgs) => command(about = "Rewrite a shell command with TokenZero-safe routing") => command(alias = "rewrite-command"),
        Hook(HookArgs) => command(about = "Agent-harness hook adapters: stdin JSON in, decision JSON out"),
        Discover(CommonArgs) => command(about = "List local TokenZero tool-discovery metadata"),
        Doctor(DoctorArgs) => command(about = "Check local TokenZero health and next steps"),
        Stats(CommonArgs) => command(about = "Print local TokenZero usage statistics"),
        Pulse(PulseArgs) => command(about = "Inspect or sync local Pulse telemetry"),
        SessionLedger(SessionLedgerArgs) => command( about = "Session cost ledger: per-session, per-repo, per-agent mass × turns accounting", alias = "ledger" ),
        Cache(CacheArgs) => command(about = "Inspect or prune TokenZero recovery-cache state"),
        Install(InstallArgs) => command(about = "Plan or apply local integration writes with rollback data"),
        Init(InitArgs) => command(about = "Compatibility alias for install --mcp --agent <name>"),
        Clients(ClientsArgs) => command( about = "Inspect AI client TokenZero integration state", alias = "client" ),
        ClientStatus(ClientStatusArgs) => command(name = "client-status", about = "Alias for clients detect"),
        Capabilities(CapabilitiesArgs) => command( about = "Print the machine-readable CLI contract for agents", alias = "capability", alias = "capabilites" ),
        RobotDocs(RobotDocsArgs) => command( name = "robot-docs", about = "Print in-tool documentation for agents", alias = "robot-doc", alias = "robotdocs" ),
        CachePack(CachePackArgs) => command(name = "cache-pack"),
        Bench(BenchArgs),
        McpServer(McpServerArgs) => command(name = "mcp-server"),
        McpSmoke(ArtifactArgs) => command(name = "mcp-smoke"),
        McpSoak(ArtifactArgs) => command(name = "mcp-soak"),
        ExactRecoveryShell(ExactRecoveryShellArgs) => command(name = "exact-recovery-shell"),
        ExactRecoveryAudit(ExactRecoveryAuditArgs) => command(name = "exact-recovery-audit"),
        CodeMode(CodeModeArgs) => command( name = "codemode", about = "Compose multi-step plans on the same base tools as MCP (fewer round-trips, Cloudflare-style)", long_about = "Execute JS-like plans that compose the same TokenZero operations as MCP (zero.read, zero.find, zero.shell, ...) in one call for faster multi-step workflows.\n\nDiscovery: tokenzero codemode 'search:read' | tokenzero codemode 'describe:zero.read'\n\nCache: defaults to the same recovery-cache.json as CLI expand/MCP so refs mintable by codemode expand on the next call. Override with --cache-path / TOKENZERO_CACHE_PATH when using an isolated store (wrong root yields store_mismatch naming both paths)." ),
        HarmEval(ArtifactArgs) => command(name = "harm-eval"),
        ProtectedAnchorAudit(ProtectedAnchorAuditArgs) => command(name = "protected-anchor-audit"),
        FalseSuccessShell(FalseSuccessShellArgs) => command(name = "false-success-shell"),
        RepoInventory(ArtifactArgs) => command(name = "repo-inventory"),
        PromptCachePack(ArtifactArgs) => command(name = "prompt-cache-pack"),
        InstallSmoke(InstallSmokeArgs) => command(name = "install-smoke"),
        PackageAudit(PackageAuditArgs) => command(name = "package-audit"),
        ShellMatrix(ShellMatrixArgs) => command(name = "shell-matrix"),
        OsReachAudit(OsReachAuditArgs) => command(name = "os-reach-audit"),
        OsReleaseArtifact(OsReleaseArtifactArgs) => command(name = "os-release-artifact"),
        OneShotEval(OneShotEvalArgs) => command(name = "one-shot-eval"),
        SourceCurrencyAudit(SourceCurrencyAuditArgs) => command(name = "source-currency-audit"),
        AdapterApprovalAudit(AdapterApprovalAuditArgs) => command(name = "adapter-approval-audit"),
        AdapterApprovalTemplate(AdapterApprovalTemplateArgs) => command(name = "adapter-approval-template"),
        ClaimAudit(ClaimAuditArgs) => command(name = "claim-audit"),
        CompletionAudit(CompletionAuditArgs) => command(name = "completion-audit"),
        SecurityPrivacyAudit(SecurityPrivacyAuditArgs) => command(name = "security-privacy-audit"),
        ArtifactHandoff(ArtifactHandoffArgs) => command(name = "artifact-handoff"),
        Reach(ReachArgs),
        WsSkeleton(WsSkeletonArgs) => command(name = "ws-skeleton"),
        Quote(QuoteArgs),
    }
    HookTarget {
        ClaudeCode(HookClaudeCodeArgs) => command( name = "claude-code", about = "Claude Code PreToolUse adapter: wraps Bash commands in `tokenzero run` (fail-open, always exits 0)" ),
        ClaudeCodeSessionStart(HookSessionStartArgs) => command( name = "claude-code-session-start", about = "Claude Code SessionStart adapter: restores a compact session pack after compaction/resume (fail-open, always exits 0)" ),
    }
    PulseCommand {
        Stats => command( name = "stats", alias = "status", about = "Print local Pulse telemetry report" ),
        Sync,
        Doctor,
        ExportJsonl(PulseExportArgs) => command(name = "export-jsonl"),
        ImportJsonl(PulseImportArgs) => command(name = "import-jsonl"),
    }
    BenchCommand {
        Competitors(BenchCompetitorsArgs),
    }
    CacheCommand {
        Status(CommonArgs) => command(alias = "statuz"),
        Prune(CachePruneArgs),
        /// Migrate legacy short refs to full-hash canonical refs (dry-run by default).
        MigrateRefs(CacheMigrateRefsArgs),
        /// Verify migration integrity without mutating.
        MigrateVerify(CacheMigrateVerifyArgs),
        /// Rollback migration aliases and manifest (never CAS/source bytes).
        MigrateRollback(CacheMigrateRollbackArgs),
        /// Clean up legacy source payloads after successful verification.
        MigrateCleanup(CacheMigrateCleanupArgs),
    }
    ClientsCommand {
        Detect(ClientStatusArgs) => command(about = "Detect configured TokenZero AI client surfaces"),
        Scan(ClientStatusArgs) => command(about = "Scan this machine for AI harnesses TokenZero can adapt to"),
        Plan(ClientsPlanArgs) => command(about = "Plan TokenZero AI client integration writes"),
        Doctor(ClientStatusArgs) => command(about = "Diagnose TokenZero AI client integration state"),
        Rollback(ClientsRollbackArgs) => command(about = "Rollback a previous TokenZero client integration write"),
    }
    RobotDocsCommand {
        Guide => command(alias = "manual"),
        Commands => command(about = "Print canonical command quick reference for agents"),
        Examples => command(about = "Print copy-paste examples for common agent tasks"),
    }
}

define_args! {
    #[derive(Clone)]
    CommonArgs {
        root: Option<PathBuf> => arg(long),
        cache_path: Option<PathBuf> => arg(long),
        json: bool => arg(long),
    }
    #[derive(Clone)]
    ToolArgs {
        mode: String => arg(long, default_value = "auto"),
        budget: Option<usize> => arg(long),
        allowed_root: Vec<PathBuf> => arg(long),
        cache_path: Option<PathBuf> => arg(long),
        timeout_seconds: Option<u64> => arg(long, alias = "timeout", alias = "timout", value_name = "SECONDS"),
        json: bool => arg(long, alias = "jsno", alias = "jason"),
    }
    ReadArgs {
        path: Vec<PathBuf> => arg(value_name = "PATH"),
        paths_from: Option<PathBuf> => arg(long),
        max_files: usize => arg(long, default_value_t = 20),
        max_visible_tokens: usize => arg(long, default_value_t = 4000),
        start_line: Option<usize> => arg(long),
        end_line: Option<usize> => arg(long),
        raw: bool => arg(long),
        tool: ToolArgs => command(flatten),
    }
    FindArgs {
        query: String,
        path: Vec<PathBuf>,
        max_files: usize => arg(long, default_value_t = 20),
        max_visible_tokens: usize => arg(long, default_value_t = 4000),
        tool: ToolArgs => command(flatten),
    }
    RecallArgs {
        query: String,
        max_hits: usize => arg(long, default_value_t = 50),
        max_visible_tokens: usize => arg(long, default_value_t = 4000),
        tool: ToolArgs => command(flatten),
    }
    FetchArgs {
        url: String,
        /// Serve a cached body younger than this without touching the network.
        ttl_seconds: Option<usize> => arg(long),
        /// Bypass the TTL cache and re-fetch.
        fresh: bool => arg(long),
        max_visible_tokens: usize => arg(long, default_value_t = 4000),
        tool: ToolArgs => command(flatten),
    }
    GlobArgs {
        pattern: String,
        path: Vec<PathBuf>,
        max_files: usize => arg(long, default_value_t = 200),
        max_visible_tokens: usize => arg(long, default_value_t = 4000),
        include_hidden: bool => arg(long),
        tool: ToolArgs => command(flatten),
    }
    TreeArgs {
        path: Vec<PathBuf>,
        depth: usize => arg(long, default_value_t = 2),
        max_files: usize => arg(long, default_value_t = 200),
        max_visible_tokens: usize => arg(long, default_value_t = 4000),
        include_hidden: bool => arg(long),
        tool: ToolArgs => command(flatten),
    }
    EditArgs {
        path: PathBuf => arg(value_name = "PATH"),
        /// JSON array of {find, replace, replace_all?} hunks.
        edits_json: Option<String> => arg(long = "edits-json", value_name = "JSON"),
        /// Read the edits JSON from stdin instead of --edits-json.
        stdin: bool => arg(long),
        /// Create a new file: one hunk with empty find; replace is the content.
        create: bool => arg(long),
        /// Validate and render the hunk diff without writing.
        dry_run: bool => arg(long),
        max_visible_tokens: usize => arg(long, default_value_t = 4000),
        tool: ToolArgs => command(flatten),
    }
    RunArgs {
        command: Vec<String> => arg(last = true),
        cwd: Option<PathBuf> => arg(long),
        rewrite: Option<String> => arg(long),
        no_rewrite: bool => arg(long),
        stdin: bool => arg(long),
        env_overrides: Vec<String> => arg(long = "env"),
        explain_runtime: bool => arg(long),
        runtime_platform: Option<String> => arg(long),
        tool: ToolArgs => command(flatten),
    }
    IngestArgs {
        input: Option<PathBuf>,
        stdin: bool => arg(long),
        kind: String => arg(long, default_value = "auto"),
        tool: ToolArgs => command(flatten),
    }
    ExpandArgs {
        refs: Vec<String> => arg(value_name = "REF"),
        refs_from: Option<PathBuf> => arg(long),
        selector: Option<String> => arg(long),
        raw: bool => arg(long),
        summary: bool => arg(long),
        force: bool => arg(long),
        start_line: Option<usize> => arg(long),
        end_line: Option<usize> => arg(long),
        line: Option<usize> => arg(long),
        lines: Option<String> => arg(long),
        around: Option<String> => arg(long),
        anchor_kind: Option<String> => arg(long),
        symbol: Option<String> => arg(long),
        cache_path: Option<PathBuf> => arg(long),
        json: bool => arg(long),
    }
    RewriteArgs {
        /// Command string; alternative to trailing `-- <command...>`.
        command: Option<String>,
        /// Command after `--`, matching `tokenzero run -- <command...>`.
        argv: Vec<String> => arg(last = true),
        mode: String => arg(long, default_value = "safe"),
        json: bool => arg(long),
    }
    HookArgs {
        target: HookTarget => command(subcommand),
    }
    HookClaudeCodeArgs {
        /// rewrite | guide | off. Unknown values pass through (fail-open).
        mode: String => arg(long, default_value = "rewrite"),
    }
    HookSessionStartArgs {
        /// Token budget for the restored session pack.
        max_tokens: usize => arg(long, default_value_t = 600),
    }
    DoctorArgs {
        root: Option<PathBuf> => arg(long, global = true),
        cache_path: Option<PathBuf> => arg(long, global = true),
        runtime: bool => arg(long, global = true),
        json: bool => arg(long, global = true),
        robot_triage: bool => arg(long = "robot-triage", global = true),
        fix: bool => arg(long, global = true),
        dry_run: bool => arg(long = "dry-run", global = true),
        explain: Option<String> => arg(long, global = true),
        command: Option<DoctorCommand> => command(subcommand),
    }
    PulseArgs {
        root: Option<PathBuf> => arg(long, global = true),
        json: bool => arg(long, global = true),
        command: Option<PulseCommand> => command(subcommand),
    }
    PulseExportArgs {
        output: PathBuf => arg(value_name = "OUTPUT"),
    }
    PulseImportArgs {
        input: PathBuf => arg(value_name = "INPUT"),
    }
    SessionLedgerArgs {
        root: Option<PathBuf> => arg(long, global = true),
        json: bool => arg(long, global = true),
        command: Option<SessionLedgerCommand> => command(subcommand),
    }
    CacheArgs {
        command: CacheCommand => command(subcommand),
    }
    CachePackArgs {
        scope: String => arg(long, default_value = "agent"),
        root: Option<PathBuf> => arg(long),
        cache_path: Option<PathBuf> => arg(long),
        json: bool => arg(long),
    }
    BenchArgs {
        command: BenchCommand => command(subcommand),
    }
    BenchCompetitorsArgs {
        suite: String => arg(long, default_value = "shell-heavy"),
        output_json: Option<PathBuf> => arg(long),
        adapter_approval_artifact: Option<PathBuf> => arg(long),
        json: bool => arg(long),
    }
    CachePruneArgs {
        root: Option<PathBuf> => arg(long),
        cache_path: Option<PathBuf> => arg(long),
        apply: bool => arg(long),
        json: bool => arg(long),
    }
    CacheMigrateRefsArgs {
        root: Option<PathBuf> => arg(long),
        cache_path: Option<PathBuf> => arg(long),
        /// Actually write to CAS, store, and manifest. Without this flag, migration is dry-run only.
        apply: bool => arg(long),
        json: bool => arg(long),
    }
    CacheMigrateVerifyArgs {
        root: Option<PathBuf> => arg(long),
        cache_path: Option<PathBuf> => arg(long),
        json: bool => arg(long),
    }
    CacheMigrateRollbackArgs {
        root: Option<PathBuf> => arg(long),
        cache_path: Option<PathBuf> => arg(long),
        /// Actually remove aliases and manifest. Without this flag, rollback is dry-run only.
        apply: bool => arg(long),
        json: bool => arg(long),
    }
    CacheMigrateCleanupArgs {
        root: Option<PathBuf> => arg(long),
        cache_path: Option<PathBuf> => arg(long),
        /// Actually remove legacy source payloads. Requires --confirm-cleanup.
        apply: bool => arg(long, requires = "confirm_cleanup"),
        /// Required confirmation flag. Cleanup is irreversible without migration re-run.
        confirm_cleanup: bool => arg(long, requires = "apply"),
        json: bool => arg(long),
    }
    InstallArgs {
        root: Option<PathBuf> => arg(long),
        plan: bool => arg(long),
        apply: bool => arg(long),
        rollback: Option<String> => arg(long),
        global: bool => arg(long),
        mcp: bool => arg(long),
        shell: bool => arg(long),
        instructions: bool => arg(long),
        cli: bool => arg(long),
        /// Wire the Claude Code PreToolUse hook into .claude/settings.json.
        hooks: bool => arg(long),
        /// Install the universal PATH shims under .tokenzero/shims/.
        shims: bool => arg(long),
        agents: Vec<String> => arg(long = "agent", value_name = "AGENT"),
        grok: bool => arg(long),
        /// MCP tool surface profile (always `classic`; CodeMode is a separate execution layer).
        surface: String => arg(long, value_name = "SURFACE", default_value = "classic"),
        json: bool => arg(long),
    }
    InitArgs {
        root: Option<PathBuf> => arg(long),
        global: bool => arg(long),
        agents: Vec<String> => arg(long = "agent", value_name = "AGENT"),
        mcp: bool => arg(long),
        shell: bool => arg(long),
        instructions: bool => arg(long),
        cli: bool => arg(long),
        /// Wire the Claude Code PreToolUse hook into .claude/settings.json.
        hooks: bool => arg(long),
        /// Install the universal PATH shims under .tokenzero/shims/.
        shims: bool => arg(long),
        apply: bool => arg(long),
        plan: bool => arg(long),
        /// MCP tool surface profile (always `classic`; CodeMode is a separate execution layer).
        surface: String => arg(long, value_name = "SURFACE", default_value = "classic"),
        json: bool => arg(long),
    }
    ClientsArgs {
        command: ClientsCommand => command(subcommand),
    }
    ClientStatusArgs {
        root: Option<PathBuf> => arg(long),
        agents: Vec<String> => arg(long = "agent", value_name = "AGENT"),
        grok: bool => arg(long),
        json: bool => arg(long),
    }
    ClientsPlanArgs {
        root: Option<PathBuf> => arg(long),
        profile: String => arg(long, default_value = "standard"),
        agents: Vec<String> => arg(long = "agent", value_name = "AGENT"),
        grok: bool => arg(long),
        json: bool => arg(long),
    }
    ClientsRollbackArgs {
        id: String,
        root: Option<PathBuf> => arg(long),
        json: bool => arg(long),
    }
    CapabilitiesArgs {
        json: bool => arg(long, alias = "jsno", alias = "jason"),
    }
    RobotDocsArgs {
        command: RobotDocsCommand => command(subcommand),
    }
    McpServerArgs {
        /// Launch mode: mcp exposes per-op tools; codemode exposes primary, report, and gated recovery tools.
        mode: String => arg(long, default_value = "mcp", value_name = "MODE"),
        allowed_root: Vec<PathBuf> => arg(long),
        cache_path: Option<PathBuf> => arg(long),
        default_mode: String => arg(long, default_value = "auto"),
        shell_timeout_seconds: Option<u64> => arg(long, alias = "timeout", value_name = "SECONDS"),
        idle_timeout_seconds: Option<u64> => arg(long, value_name = "SECONDS"),
        /// Run a crash-transparent supervisor that owns the client stdio pipes
        /// and automatically respawns the inner MCP server if it ever dies.
        supervise: bool => arg(long),
        /// Backward-compatible alias for --mode.
        tool_surface: Option<String> => arg(long, value_name = "SURFACE"),
    }
    OsReachAuditArgs {
        output_json: PathBuf => arg(long, default_value = "results/current/tokenzero_os_reach_audit.json"),
        output_md: Option<PathBuf> => arg(long),
        root: PathBuf => arg(long, default_value = "."),
        os_artifact: Vec<PathBuf> => arg(long = "os-artifact"),
        release_approval: bool => arg(long),
        json: bool => arg(long),
    }
    OsReleaseArtifactArgs {
        output_json: PathBuf => arg( long, default_value = "results/current/tokenzero_os_release_artifact.json" ),
        output_md: Option<PathBuf> => arg(long),
        root: PathBuf => arg(long, default_value = "."),
        json: bool => arg(long),
    }
    SourceCurrencyAuditArgs {
        output_json: PathBuf => arg(long, default_value = "results/current/tokenzero_source_currency.json"),
        output_md: Option<PathBuf> => arg(long),
        refresh_ledger: Option<PathBuf> => arg(long),
        refresh_git_heads: bool => arg(long),
        json: bool => arg(long),
    }
    AdapterApprovalAuditArgs {
        output_json: PathBuf => arg( long, default_value = "results/current/tokenzero_adapter_approval_audit.json" ),
        output_md: Option<PathBuf> => arg(long),
        approval_file: Option<PathBuf> => arg(long),
        execution_approval: bool => arg(long),
        json: bool => arg(long),
    }
    ClaimAuditArgs {
        output_json: PathBuf => arg(long, default_value = "results/current/tokenzero_claim_audit.json"),
        output_md: Option<PathBuf> => arg(long),
        source_artifact: Option<PathBuf> => arg(long),
        benchmark_artifact: Option<PathBuf> => arg(long),
        adapter_approval_artifact: Option<PathBuf> => arg(long),
        recovery_artifact: Option<PathBuf> => arg(long),
        task_success_artifact: Option<PathBuf> => arg(long),
        os_artifact: Option<PathBuf> => arg(long),
        release_approval: bool => arg(long),
        json: bool => arg(long),
    }
    ReachArgs {
        root: PathBuf => arg(long, default_value = "."),
        output_json: Option<PathBuf> => arg(long),
        json: bool => arg(long),
    }
    InstallSmokeArgs {
        output_json: Option<PathBuf> => arg(long),
        json: bool => arg(long),
    }
    PackageAuditArgs {
        dist: PathBuf => arg(long, default_value = "."),
        json: bool => arg(long),
    }
    QuoteArgs {
        platform: String => arg(long),
        args: Vec<String> => arg(last = true),
        json: bool => arg(long),
    }
    CodeModeArgs {
        /// CodeMode plan (JS-style zero.token.compact / expand) as a positional argument.
        plan: Option<String> => arg(value_name = "PLAN"),
        /// CodeMode plan as an explicit flag; kept for router compatibility.
        plan_flag: Option<String> => arg(short = 'p', long = "plan", value_name = "PLAN"),
        /// Read plan from a file instead of inline. Supports .txt and .js extensions.
        plan_file: Option<PathBuf> => arg(long = "plan-file", value_name = "PATH"),
        /// Workspace root used for CodeMode file, shell, and recovery-cache boundaries.
        root: Option<PathBuf> => arg(long),
        /// Additional allowed roots for plans that must intentionally cross the workspace boundary.
        allowed_root: Vec<PathBuf> => arg(long),
        /// Override the CodeMode recovery cache path.
        cache_path: Option<PathBuf> => arg(long),
        /// Maximum visible tokens for each underlying TokenZero operation.
        max_visible_tokens: usize => arg(long, default_value_t = 4000),
        timeout_seconds: Option<u64> => arg(long, alias = "timeout", alias = "timout", value_name = "SECONDS"),
        json: bool => arg(long),
    }
}

artifact_args! {
    ArtifactArgs => "results/current/rust_mcp_smoke.json",
    ShellMatrixArgs => "results/current/tokenzero_shell_matrix.json",
    FalseSuccessShellArgs => "results/current/tokenzero_false_success_shell.json",
    ExactRecoveryShellArgs => "results/current/tokenzero_exact_recovery_shell.json",
    ExactRecoveryAuditArgs => "results/current/tokenzero_exact_recovery_audit.json",
    ProtectedAnchorAuditArgs => "results/current/tokenzero_protected_anchor_audit.json",
    OneShotEvalArgs => "results/current/tokenzero_one_shot_eval.json",
    AdapterApprovalTemplateArgs => "results/current/tokenzero_adapter_approval_file.json",
    CompletionAuditArgs => "results/current/tokenzero_completion_audit.json",
    SecurityPrivacyAuditArgs => "results/current/tokenzero_security_privacy_audit.json",
    ArtifactHandoffArgs => "results/current/tokenzero_artifact_handoff.json",
    WsSkeletonArgs => "results/current/tokenzero_ws_001.json",
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

    use super::{CacheCommand, Cli, CodeModeArgs, Commands};
    use clap::Parser;

    fn code_mode_args(
        plan: Option<&str>,
        plan_flag: Option<&str>,
        plan_file: Option<std::path::PathBuf>,
    ) -> CodeModeArgs {
        CodeModeArgs {
            plan: plan.map(str::to_owned),
            plan_flag: plan_flag.map(str::to_owned),
            plan_file,
            root: None,
            allowed_root: Vec::new(),
            cache_path: None,
            max_visible_tokens: 4000,
            timeout_seconds: None,
            json: false,
        }
    }

    #[test]
    fn representative_clap_contracts() {
        let cli = Cli::try_parse_from([
            "tokenzero", "search", "needle", "src", "--timeout", "7", "--jsno",
        ])
        .unwrap();
        let Commands::Find(find) = cli.command.unwrap() else {
            panic!("search alias did not select find");
        };
        assert_eq!(find.query, "needle");
        assert_eq!(find.path, [std::path::PathBuf::from("src")]);
        assert_eq!(find.tool.timeout_seconds, Some(7));
        assert!(find.tool.json);

        let read = Cli::try_parse_from(["tokenzero", "read", "Cargo.toml"]).unwrap();
        let Commands::Read(read) = read.command.unwrap() else {
            panic!("read command did not parse");
        };
        assert_eq!((read.max_files, read.max_visible_tokens), (20, 4000));
        assert_eq!(read.tool.mode, "auto");

        let cleanup = ["tokenzero", "cache", "migrate-cleanup", "--apply"];
        assert!(Cli::try_parse_from(cleanup).is_err());
        let confirm = ["tokenzero", "cache", "migrate-cleanup", "--confirm-cleanup"];
        assert!(Cli::try_parse_from(confirm).is_err());
        let both = Cli::try_parse_from([
            "tokenzero", "cache", "migrate-cleanup", "--apply", "--confirm-cleanup",
        ])
        .unwrap();
        let Commands::Cache(cache) = both.command.unwrap() else {
            panic!("cache command did not parse");
        };
        assert!(matches!(
            cache.command,
            CacheCommand::MigrateCleanup(args) if args.apply && args.confirm_cleanup
        ));
        assert!(
            Cli::try_parse_from(["tokenzero", "find", "x", "--timeout", "invalid"]).is_err()
        );
    }

    #[test]
    fn plan_file_reads_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let plan_path = dir.path().join("test.plan.js");
        std::fs::write(&plan_path, r#"zero.compact("from file")"#).unwrap();
        let args = code_mode_args(None, None, Some(plan_path));
        let text = args.plan_text().unwrap();
        assert!(text.contains("from file"));
    }

    #[test]
    fn plan_text_prefers_flag_over_positional() {
        let args = code_mode_args(Some("positional"), Some("flag_wins"), None);
        assert_eq!(args.plan_text().unwrap(), "flag_wins");
    }
}
