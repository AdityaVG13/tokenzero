#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use serde_json::json;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use tempfile::tempdir;
use tokenzero_core::McpToolSurface;
use tokenzero_core::{
    ContentType, Mode, ToolResponse, detect_content_type,
    shell_display_command_from_argv_for_platform,
};
use tokenzero_install as install;
use tokenzero_mcp_compat::{
    CodeModeOptions, CodeModeResult, CodeModeStatus, EditHunk, EngineConfig, TokenZeroEngine,
    cli_json, default_shell_timeout, execute_codemode_with_options, mcp_idle_timeout_from_secs,
    render_text, shell_timeout_from_secs,
};

mod agent_surfaces;
mod artifact_contracts;
mod claim_actions;
mod cli_args;
mod competitor_adapters;
mod completion_handoff;
mod hook;
mod mcp_artifact;
mod reach;
mod release_claims;
mod source_currency;
mod zerostack_store;
// Process/artifact mutual exclusion (tokenzero-irx9.3): dual surface features
// cannot compile into one binary (see also tokenzero-mcp compile_error).
#[cfg(all(feature = "surface-mcp", feature = "surface-codemode"))]
compile_error!(
    "tokenzero surfaces are mutually exclusive (tokenzero-irx9.3): enable exactly one of \
feature surface-mcp or surface-codemode — never both. The tokenzero CLI is a selected \
shim or single-surface build; install tokenzero-mcp or tokenzero-codemode for servers."
);

use agent_surfaces::{capabilities_json, robot_docs_guide};
use artifact_contracts::{json_artifact_path, release_candidate_id};
use cli_args::*;
use competitor_adapters::{
    competitor_adapter_matrix, competitor_adapter_rows, load_benchmark_adapter_approval,
};
use mcp_artifact::run_mcp_artifact;
use reach::{installed_tokenzero_command_audit, run_reach};
use release_claims::{ClaimEvidenceInputs, run_claim_audit};
use tokenzero_pulse::{
    PulseEvent, SessionLedgerReport, default_ledger_path, doctor_jsonl_sqlite, export_jsonl,
    import_jsonl, record_event, report_for_path, sync_jsonl_to_sqlite,
};
use tokenzero_runtime::{
    ExecutionMode, contains_platform_shell_syntax, env_map, plan_command_for_platform, quote_for,
    split_command_string,
};
use zerostack_store::{
    allowed_roots_for_workspace, default_allowed_roots, resolve_recovery_cache_path,
    tokenzero_work_root,
};

fn emit_json_md<T, F, J, M>(output_json: J, output_md: M, as_json: bool, run: F) -> Result<()>
where
    T: serde::Serialize,
    F: FnOnce(J, M) -> Result<T>,
{
    emit_value(run(output_json, output_md)?, as_json)
}

fn migration_manifest_path(cache_path: &std::path::Path) -> std::path::PathBuf {
    cache_path
        .parent()
        .unwrap_or(cache_path)
        .join("migration-manifest.json")
}

fn with_legacy_migration<R>(
    root: Option<PathBuf>,
    cache_path: Option<PathBuf>,
    f: impl FnOnce(&mut tokenzero_recovery::migration::LegacyMigration<'_>) -> R,
) -> R {
    let root = tokenzero_work_root(root);
    let cache = resolve_recovery_cache_path(&root, cache_path);
    let manifest = migration_manifest_path(&cache);
    let mut store = tokenzero_recovery::RecoveryStore::new(Some(cache.clone()));
    let cas = tokenzero_recovery::shared_cas::SharedCas::new(
        tokenzero_recovery::shared_cas::SharedCas::attach_root_for_cache_path(&cache),
    );
    let mut adapter = tokenzero_recovery::migration::RecoveryStoreAdapter::new(&mut store);
    let mut migration =
        tokenzero_recovery::migration::LegacyMigration::new(&mut adapter, &cas, Some(manifest));
    f(&mut migration)
}

fn emit_migration_report(json: String, text: String, failed: bool, as_json: bool) -> Result<()> {
    if as_json {
        println!("{json}");
    } else {
        println!("{text}");
    }
    if failed {
        std::process::exit(1);
    }
    Ok(())
}

macro_rules! cache_migrate {
    ($root:expr, $cache:expr, $json:expr, $body:expr) => {{
        let report = with_legacy_migration($root, $cache, $body);
        emit_migration_report(
            report.to_json(),
            report.to_text(),
            report.is_failure(),
            $json,
        )
    }};
}

macro_rules! dispatch_command {
($command:expr;
@emit { $($ev:ident => $eh:ident),* $(,)? }
@result { $($rv:ident => $rh:ident),* $(,)? }
@json_md { $($jv:ident => $jr:expr),* $(,)? }
@value { $($vv:ident($va:ident) => $value:expr;)* }
@special { $($sv:ident($sa:ident) => $special:block)* }
) => {
match $command {
$(Commands::$ev(args) => emit($eh(args)?)?,)*
$(Commands::$rv(args) => $rh(args)?,)*
$(Commands::$jv(args) => emit_json_md(args.output_json, args.output_md, args.json, $jr)?,)*
$(Commands::$vv($va) => { let as_json = $va.json; emit_value($value, as_json)? },)*
$(Commands::$sv($sa) => $special,)*
}
};
}

fn raw_worker_is_first_command(argv: &[OsString]) -> bool {
    matches!(
        argv.get(1).and_then(|arg| arg.to_str()),
        Some("raw-worker" | "raw_worker")
    )
}

fn raw_worker_argv(argv: Vec<OsString>) -> Result<Vec<String>> {
    argv.into_iter()
        .map(|arg| {
            arg.into_string()
                .map_err(|arg| anyhow::anyhow!("raw-worker argument is not valid UTF-8: {arg:?}"))
        })
        .collect()
}

#[cfg(test)]
mod startup_arg_tests {
    use super::raw_worker_is_first_command;
    use std::ffi::OsString;

    #[test]
    fn raw_worker_dispatch_requires_first_command_argument() {
        let child_command = [
            OsString::from("tokenzero"),
            OsString::from("run"),
            OsString::from("--"),
            OsString::from("raw-worker"),
        ];
        assert!(!raw_worker_is_first_command(&child_command));

        let raw_worker = [OsString::from("tokenzero"), OsString::from("raw-worker")];
        assert!(raw_worker_is_first_command(&raw_worker));
    }
}

fn main() -> Result<()> {
    let argv: Vec<OsString> = std::env::args_os().collect();

    // Private raw worker (tokenzero-irx9.4) on the selected surface artifact / shim.
    // This private command is recognized only in argv[1], before Clap normalization.
    if raw_worker_is_first_command(&argv) {
        let code = tokenzero_mcp_compat::maybe_run_raw_worker_from_args(&raw_worker_argv(argv)?)
            .map_err(anyhow::Error::msg)?
            .context("leading raw-worker command did not parse")?;
        std::process::exit(code);
    }

    // Fast path: avoid building the full clap command tree for --version/-V.
    if argv.len() == 2 && matches!(argv[1].to_str(), Some("--version" | "-V")) {
        println!("tokenzero {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let normalized_argv = normalize_agent_invocation_args(argv);
    let cli = match Cli::try_parse_from(&normalized_argv) {
        Ok(cli) => cli,
        Err(err) => {
            // bara (R-016): an unknown subcommand that is an MCP tz_* tool name
            // must suggest the mapped CLI verb (tz_read -> read), never clap's
            // generic nearest string (which sent tz_read to 'tree').
            if let Some(verb) = mcp_name_to_cli_verb(
                normalized_argv
                    .get(1)
                    .and_then(|arg| arg.to_str())
                    .unwrap_or_default(),
            ) {
                if matches!(err.kind(), clap::error::ErrorKind::InvalidSubcommand) {
                    let corrected: Vec<String> = normalized_argv
                        .iter()
                        .skip(1)
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect();
                    let corrected = std::iter::once(verb.to_string())
                        .chain(corrected.into_iter().skip(1))
                        .collect::<Vec<_>>()
                        .join(" ");
                    eprintln!(
                        "error: unrecognized subcommand '{}'\n\n  tip: '{}' is an MCP tool name; the CLI verb is '{}'\n\n  corrected command: tokenzero {}\n",
                        normalized_argv[1].to_string_lossy(),
                        normalized_argv[1].to_string_lossy(),
                        verb,
                        corrected,
                    );
                    std::process::exit(2);
                }
            }
            err.exit();
        }
    };
    let Some(command) = cli.command else {
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };
    dispatch_command!(command;
    @emit { Read => handle_read, Find => handle_find, Grep => handle_grep, Glob => handle_glob, Tree => handle_tree, Edit => handle_edit, Recall => handle_recall, Fetch => handle_fetch, Run => handle_run, Ingest => handle_ingest, Expand => handle_expand, }
    @result { Rewrite => emit_rewrite, Doctor => handle_doctor, Pulse => handle_pulse, SessionLedger => handle_session_ledger, Cache => handle_cache, Install => handle_install, Init => handle_init, Clients => handle_clients, ClientStatus => handle_client_status, Capabilities => handle_capabilities, CachePack => handle_cache_pack, Bench => handle_bench, Quote => handle_quote, }
    @json_md { McpSmoke => |j, m| run_mcp_artifact(j, m, 1), McpSoak => |j, m| run_mcp_artifact(j, m, 25), ExactRecoveryShell => run_exact_recovery_shell, ExactRecoveryAudit => run_exact_recovery_audit, HarmEval => run_harm_eval, ProtectedAnchorAudit => run_protected_anchor_audit, FalseSuccessShell => run_false_success_shell, RepoInventory => run_repo_inventory, PromptCachePack => run_prompt_cache_pack, ShellMatrix => run_shell_matrix, OneShotEval => run_one_shot_eval, AdapterApprovalTemplate => run_adapter_approval_template, CompletionAudit => run_completion_audit, SecurityPrivacyAudit => run_security_privacy_audit, ArtifactHandoff => run_artifact_handoff, WsSkeleton => run_ws_skeleton, }
    @value { SessionOpen(args) => engine_from_common(&args).session_boot_snapshot(); Stats(args) => handle_stats(args)?; InstallSmoke(args) => run_install_smoke(args.output_json)?; PackageAudit(args) => handle_package_audit(args); OsReachAudit(args) => run_os_reach_audit(args.output_json, args.output_md, args.root, args.os_artifact, args.release_approval,)?; OsReleaseArtifact(args) => run_os_release_artifact(args.output_json, args.output_md, args.root,)?; SourceCurrencyAudit(args) => run_source_currency_audit(args.output_json, args.output_md, args.refresh_ledger, args.refresh_git_heads,)?; AdapterApprovalAudit(args) => run_adapter_approval_audit(args.output_json, args.output_md, args.approval_file, args.execution_approval,)?; ClaimAudit(args) => run_claim_audit(args.output_json, args.output_md, args.release_approval, ClaimEvidenceInputs { source_artifact: args.source_artifact, benchmark_artifact: args.benchmark_artifact, adapter_approval_artifact: args.adapter_approval_artifact, recovery_artifact: args.recovery_artifact, task_success_artifact: args.task_success_artifact, os_artifact: args.os_artifact, },)?; Reach(args) => run_reach(args.root, args.output_json)?; }
    @special {
    Mem(args) => {
        let engine = engine_from_common(&args);
        emit_with_json(dispatch_cli_tool(&engine, "tz_mem", json!({})), args.json)?;
    }
    Hook(args) => { hook::handle_hook(args); }
    Discover(args) => {
        let root = tokenzero_work_root(None);
        let engine = engine_new(
            &root,
            default_allowed_roots(&root),
            None,
            4000,
            Mode::Auto,
            default_shell_timeout(),
            None,
        );
        emit_with_json(dispatch_cli_tool(&engine, "tz_discover", json!({})), args.json)?;
    }
    RobotDocs(args) => { handle_robot_docs(args); }
    McpServer(args) => {
        if args.supervise {
            let program = std::env::current_exe().map(OsString::from).unwrap_or_else(|_| OsString::from("tokenzero"));
            std::process::exit(tokenzero_mcp_compat::run_supervised_stdio(program, supervised_child_args(&args)))
        }
        codemode_host_niceness();
        enforce_surface_exclusivity(&args)?;
        tokenzero_mcp_compat::run_fastmcp_stdio(engine_config_for_mcp(&args)?)
    }
    CodeMode(args) => {
        let plan = match args.plan_text() {
            Ok(plan) => plan,
            Err(err) => {
                let kind = if err.kind() == std::io::ErrorKind::InvalidInput {
                    "validation"
                } else {
                    "io"
                };
                let result = CodeModeResult::error_with_kind(kind, err.to_string(), 0, false);
                if args.json {
                    println!("{}", serde_json::to_string(&result)?);
                } else {
                    println!("{}", result.to_line());
                }
                std::io::stdout().flush()?;
                std::process::exit(1);
            }
        };
        let result = execute_codemode_with_options(&plan, CodeModeOptions {
            root: args.root.clone(), allowed_roots: args.allowed_root.clone(), cache_path: args.cache_path.clone(),
            max_visible_tokens: args.max_visible_tokens, timeout_seconds: args.timeout_seconds, ..Default::default()
        });
        let failed = result.status == CodeModeStatus::Error;
        if args.json { println!("{}", serde_json::to_string(&result)?); } else { println!("{}", result.to_line()); }
        if failed { std::io::stdout().flush()?; std::process::exit(1); }
    }
    });
    Ok(())
}

/// bara (R-016): MCP tz_* tool name -> CLI verb. Consulted when clap rejects
/// an unknown subcommand so the tip names the real CLI verb.
fn mcp_name_to_cli_verb(name: &str) -> Option<&'static str> {
    Some(match name {
        "tz_read" => "read",
        "tz_find" => "find",
        "tz_grep" => "grep",
        "tz_glob" => "glob",
        "tz_tree" => "tree",
        "tz_edit" => "edit",
        "tz_recall" => "recall",
        "tz_fetch" => "fetch",
        "tz_shell" => "run",
        "tz_ingest" => "ingest",
        "tz_expand" => "expand",
        "tz_batch" => "codemode",
        "tz_mem" => "cache",
        "tz_discover" => "capabilities",
        "tz_rewrite" => "rewrite",
        "tz_cache_pack" => "cache-pack",
        _ => return None,
    })
}

fn normalize_agent_invocation_args(mut argv: Vec<OsString>) -> Vec<OsString> {
    if argv.len() <= 1 {
        return argv;
    }
    if argv.len() == 2 && matches!(argv[1].to_str(), Some("--robot-help" | "robot-help")) {
        argv[1] = OsString::from("robot-docs");
        argv.push(OsString::from("guide"));
        return argv;
    }
    if argv[1]
        .to_str()
        .is_some_and(|arg| arg == "--mode" || arg.starts_with("--mode="))
    {
        argv.insert(1, OsString::from("mcp-server"));
        return argv;
    }
    match argv[1].to_str() {
        Some("rn") => {
            let mut normalized = argv;
            normalized[1] = OsString::from("run");
            normalize_run_invocation_args(normalized)
        }
        Some("run" | "shell") => normalize_run_invocation_args(argv),
        Some("install") => normalize_install_invocation_args(argv),
        _ => argv,
    }
}

fn normalize_install_invocation_args(argv: Vec<OsString>) -> Vec<OsString> {
    if argv.len() < 3 {
        return argv;
    }
    match argv[2].to_str() {
        Some("plan") => {
            let mut out = vec![argv[0].clone(), "install".into(), "--plan".into()];
            out.extend(argv[3..].iter().cloned());
            out
        }
        Some("status") => {
            let mut out = vec![argv[0].clone(), "clients".into(), "detect".into()];
            out.extend(
                argv[3..]
                    .iter()
                    .filter(|arg| {
                        !matches!(
                            arg.to_str(),
                            Some(
                                "--global"
                                    | "--mcp"
                                    | "--shell"
                                    | "--instructions"
                                    | "--cli"
                                    | "--plan"
                            )
                        )
                    })
                    .cloned(),
            );
            out
        }
        _ => argv,
    }
}

fn normalize_run_invocation_args(argv: Vec<OsString>) -> Vec<OsString> {
    if argv.iter().skip(2).any(|arg| arg.to_str() == Some("--")) {
        return argv;
    }
    let Some((options, command)) = split_run_args_without_delimiter(&argv[2..]) else {
        return argv;
    };
    let mut normalized = Vec::with_capacity(argv.len() + 1);
    normalized.push(argv[0].clone());
    normalized.push(argv[1].clone());
    normalized.extend(options);
    normalized.push(OsString::from("--"));
    normalized.extend(command);
    normalized
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunOptionKind {
    Flag,
    Value,
    Json,
}

const RUN_OPTIONS: &[(&str, RunOptionKind)] = &[
    ("--json", RunOptionKind::Json),
    ("--jsno", RunOptionKind::Json),
    ("--jason", RunOptionKind::Json),
    ("--no-rewrite", RunOptionKind::Flag),
    ("--stdin", RunOptionKind::Flag),
    ("--explain-runtime", RunOptionKind::Flag),
    ("--cwd", RunOptionKind::Value),
    ("--rewrite", RunOptionKind::Value),
    ("--env", RunOptionKind::Value),
    ("--runtime-platform", RunOptionKind::Value),
    ("--mode", RunOptionKind::Value),
    ("--budget", RunOptionKind::Value),
    ("--allowed-root", RunOptionKind::Value),
    ("--cache-path", RunOptionKind::Value),
    ("--timeout", RunOptionKind::Value),
    ("--timeout-seconds", RunOptionKind::Value),
    ("--timout", RunOptionKind::Value),
];

fn run_option(value: &str) -> Option<(RunOptionKind, bool)> {
    RUN_OPTIONS.iter().find_map(|&(option, kind)| {
        (value == option).then_some((kind, false)).or_else(|| {
            value
                .strip_prefix(option)
                .is_some_and(|suffix| suffix.starts_with('='))
                .then_some((kind, true))
        })
    })
}

fn split_run_args_without_delimiter(args: &[OsString]) -> Option<(Vec<OsString>, Vec<OsString>)> {
    let mut options = Vec::new();
    let mut idx = 0;
    while idx < args.len() {
        let value = args[idx].to_str()?;
        let width = match run_option(value) {
            Some((RunOptionKind::Value, false)) => usize::from(idx + 1 < args.len()) + 1,
            Some(_) => 1,
            None if value.starts_with('-') => return None,
            None => break,
        };
        options.extend_from_slice(&args[idx..idx + width]);
        idx += width;
    }
    if idx >= args.len() {
        return None;
    }
    // Once the first child executable token is seen, every remaining token is
    // child argv. Trailing --json/--jsno/--jason must not be promoted to the
    // parent envelope (CE-P02-01); put parent options before the child or use `--`.
    let command = args[idx..].to_vec();
    (!command.is_empty()).then_some((options, command))
}

fn default_paths(path: Vec<PathBuf>) -> Vec<PathBuf> {
    if path.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        path
    }
}

fn tool_engine_mode(tool: &ToolArgs) -> Result<(TokenZeroEngine, Mode)> {
    Ok((engine_from_tool(tool)?, parse_mode(&tool.mode)?))
}

struct EmitResponse {
    response: ToolResponse,
    json: bool,
}

fn tool_emit(response: ToolResponse, json: bool, tool: &str) -> Result<EmitResponse> {
    record_tool_pulse(&response, tokenzero_work_root(None), tool)?;
    Ok(EmitResponse { response, json })
}

/// Route a CLI domain op through the shared engine dispatcher exactly once.
fn dispatch_cli_tool(engine: &TokenZeroEngine, op: &str, args: serde_json::Value) -> ToolResponse {
    let outcome = tokenzero_mcp_compat::dispatch_cli(engine, op, &args);
    let response = if let Some(response) = outcome.tool_response {
        response
    } else if let Some(err) = outcome.domain_error {
        ToolResponse::error(op, err.kind.as_str(), err.message, None)
    } else {
        ToolResponse::error(
            op,
            "dispatch_empty",
            "domain dispatch returned no tool response",
            None,
        )
    };
    engine.record_ledger_response(op, &response);
    response
}

fn mode_json(mode: Mode) -> String {
    mode.to_string()
}

fn paths_json(paths: &[PathBuf]) -> serde_json::Value {
    json!(
        paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    )
}

fn handle_find(args: FindArgs) -> Result<EmitResponse> {
    let (engine, mode) = tool_engine_mode(&args.tool)?;
    let paths = default_paths(args.path);
    let response = dispatch_cli_tool(
        &engine,
        "tz_find",
        json!({
            "query": args.query,
            "path": paths_json(&paths),
            "mode": mode_json(mode),
            "max_files": args.max_files,
            "max_visible_tokens": args.max_visible_tokens,
        }),
    );
    tool_emit(response, args.tool.json, "find")
}

fn handle_recall(args: RecallArgs) -> Result<EmitResponse> {
    let (engine, mode) = tool_engine_mode(&args.tool)?;
    let response = dispatch_cli_tool(
        &engine,
        "tz_recall",
        json!({
            "query": args.query,
            "max_hits": args.max_hits,
            "mode": mode_json(mode),
            "max_visible_tokens": args.max_visible_tokens,
        }),
    );
    tool_emit(response, args.tool.json, "recall")
}

fn handle_fetch(args: FetchArgs) -> Result<EmitResponse> {
    let (engine, mode) = tool_engine_mode(&args.tool)?;
    let mut payload = json!({
        "url": args.url,
        "fresh": args.fresh,
        "mode": mode_json(mode),
        "max_visible_tokens": args.max_visible_tokens,
    });
    if let Some(ttl) = args.ttl_seconds {
        payload["ttl_seconds"] = json!(ttl);
    }
    let response = dispatch_cli_tool(&engine, "tz_fetch", payload);
    tool_emit(response, args.tool.json, "fetch")
}

fn handle_grep(args: FindArgs) -> Result<EmitResponse> {
    let (engine, mode) = tool_engine_mode(&args.tool)?;
    let paths = default_paths(args.path);
    let response = dispatch_cli_tool(
        &engine,
        "tz_grep",
        json!({
            "query": args.query,
            "path": paths_json(&paths),
            "mode": mode_json(mode),
            "max_files": args.max_files,
            "max_visible_tokens": args.max_visible_tokens,
        }),
    );
    tool_emit(response, args.tool.json, "grep")
}

fn handle_glob(args: GlobArgs) -> Result<EmitResponse> {
    let (engine, mode) = tool_engine_mode(&args.tool)?;
    let paths = default_paths(args.path);
    let response = dispatch_cli_tool(
        &engine,
        "tz_glob",
        json!({
            "pattern": args.pattern,
            "path": paths_json(&paths),
            "include_hidden": args.include_hidden,
            "mode": mode_json(mode),
            "max_files": args.max_files,
            "max_visible_tokens": args.max_visible_tokens,
        }),
    );
    tool_emit(response, args.tool.json, "glob")
}

fn handle_tree(args: TreeArgs) -> Result<EmitResponse> {
    let (engine, mode) = tool_engine_mode(&args.tool)?;
    let paths = default_paths(args.path);
    let response = dispatch_cli_tool(
        &engine,
        "tz_tree",
        json!({
            "path": paths_json(&paths),
            "depth": args.depth,
            "include_hidden": args.include_hidden,
            "mode": mode_json(mode),
            "max_files": args.max_files,
            "max_visible_tokens": args.max_visible_tokens,
        }),
    );
    tool_emit(response, args.tool.json, "tree")
}

fn handle_edit(args: EditArgs) -> Result<EmitResponse> {
    let edits_text = if args.stdin {
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        buffer
    } else {
        args.edits_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("edit requires --edits-json <json> or --stdin"))?
    };
    let hunks: Vec<EditHunk> = serde_json::from_str(&edits_text).map_err(|err| {
        anyhow::anyhow!(
            "invalid edits JSON ({err}); expected [{{\"find\": \"...\", \"replace\": \"...\", \"replace_all\": false}}]"
        )
    })?;
    let (engine, mode) = tool_engine_mode(&args.tool)?;
    let edits_json: Vec<serde_json::Value> = hunks
        .iter()
        .map(|h| {
            json!({
                "find": h.find,
                "replace": h.replace,
                "replace_all": h.replace_all,
            })
        })
        .collect();
    let response = dispatch_cli_tool(
        &engine,
        "tz_edit",
        json!({
            "path": args.path.display().to_string(),
            "edits": edits_json,
            "create": args.create,
            "dry_run": args.dry_run,
            "mode": mode_json(mode),
            "max_visible_tokens": args.max_visible_tokens,
        }),
    );
    tool_emit(response, args.tool.json, "edit")
}

fn handle_ingest(args: IngestArgs) -> Result<EmitResponse> {
    let mut text = String::new();
    if args.stdin || args.input.is_none() || args.input.as_deref() == Some(Path::new("-")) {
        std::io::stdin().read_to_string(&mut text)?;
    } else if let Some(input) = &args.input {
        text = fs::read_to_string(input)?;
    }
    let kind = content_type_from_kind(&args.kind, &text, args.input.as_deref());
    let source = args
        .input
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "stdin".to_string());
    let (engine, mode) = tool_engine_mode(&args.tool)?;
    let response = dispatch_cli_tool(
        &engine,
        "tz_ingest",
        json!({
            "text": text,
            "mode": mode_json(mode),
            "source": source,
            "content_type": kind.to_string(),
        }),
    );
    tool_emit(response, args.tool.json, "ingest")
}

fn handle_read(args: ReadArgs) -> Result<EmitResponse> {
    let mut paths = args.path;
    if let Some(paths_from) = args.paths_from {
        let root = tokenzero_work_root(None);
        let allowed_roots = allowed_roots_for_workspace(&root, &args.tool.allowed_root);
        if !existing_path_is_within_allowed_roots(&paths_from, &allowed_roots) {
            return Ok(EmitResponse {
                response: ToolResponse::error(
                    "read",
                    "path_not_allowed",
                    "paths-from file is outside allowed roots",
                    Some(
                        "Move the paths-from file under an allowed root or pass an explicit --allowed-root for that file"
                            .to_string(),
                    ),
                ),
                json: args.tool.json,
            });
        }
        let text = fs::read_to_string(paths_from)?;
        paths.extend(
            text.lines()
                .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                .map(PathBuf::from),
        );
    }
    if paths.is_empty() {
        anyhow::bail!("read requires a path");
    }
    let (engine, mode) = tool_engine_mode(&args.tool)?;
    let mut payload = json!({
        "path": paths_json(&paths),
        "mode": mode_json(mode),
        "raw": args.raw,
        "max_files": args.max_files,
        "max_visible_tokens": args.max_visible_tokens,
    });
    if let Some(s) = args.start_line {
        payload["start_line"] = json!(s);
    }
    if let Some(e) = args.end_line {
        payload["end_line"] = json!(e);
    }
    let response = dispatch_cli_tool(&engine, "tz_read", payload);
    tool_emit(response, args.tool.json, "read")
}

fn handle_run(args: RunArgs) -> Result<EmitResponse> {
    if args.command.is_empty() && !args.stdin {
        anyhow::bail!("run requires a command after --");
    }
    if args.explain_runtime {
        let argv = normalize_command(&args.command);
        let platform = args
            .runtime_platform
            .clone()
            .unwrap_or_else(|| tokenzero_runtime::current_platform().to_string());
        let plan = plan_command_for_platform(&argv, args.cwd.as_deref(), false, &platform)?;
        println!("{}", serde_json::to_string_pretty(&plan)?);
        std::process::exit(0);
    }
    let mut stdin_payload = None;
    if args.stdin {
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        stdin_payload = Some(buffer);
    }
    let env = env_map(&args.env_overrides)?;
    let normalized_command = normalize_command(&args.command);
    let command = display_command_for_platform(
        &normalized_command,
        args.cwd.as_deref(),
        tokenzero_runtime::current_platform(),
    );
    let (engine, mode) = tool_engine_mode(&args.tool)?;
    let mut payload = json!({
        "command": command,
        "argv": normalized_command,
        "mode": mode_json(mode),
        "no_rewrite": args.no_rewrite,
    });
    if let Some(cwd) = &args.cwd {
        payload["cwd"] = json!(cwd.display().to_string());
    }
    if let Some(rewrite) = &args.rewrite {
        payload["rewrite"] = json!(rewrite);
    }
    if let Some(stdin) = &stdin_payload {
        payload["stdin"] = json!(stdin);
    }
    if !env.is_empty() {
        payload["env"] = json!(env);
    }
    let response = dispatch_cli_tool(&engine, "tz_shell", payload);
    tool_emit(response, args.tool.json, "shell")
}

fn display_command_for_platform(argv: &[String], cwd: Option<&Path>, platform: &str) -> String {
    match plan_command_for_platform(argv, cwd, false, platform) {
        Ok(plan) if plan.execution_mode == ExecutionMode::Shell => argv.join(" "),
        _ => shell_display_command_from_argv_for_platform(argv, platform),
    }
}

fn handle_expand(args: ExpandArgs) -> Result<EmitResponse> {
    let mut refs = args.refs.clone();
    if let Some(refs_from) = &args.refs_from {
        refs.extend(
            fs::read_to_string(refs_from)?
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string),
        );
    }
    let Some(ref_id) = refs.first() else {
        anyhow::bail!("expand requires a ref");
    };
    let root = tokenzero_work_root(None);
    let engine = engine_new(
        &root,
        default_allowed_roots(&root),
        args.cache_path.clone(),
        4000,
        Mode::Exact,
        default_shell_timeout(),
        None,
    );
    let (selector, start, end) = expand_selector(&args);
    let mut payload = json!({ "ref": ref_id });
    if let Some(sel) = selector {
        payload["selector"] = json!(sel);
    }
    if let Some(s) = start {
        payload["start_line"] = json!(s);
    }
    if let Some(e) = end {
        payload["end_line"] = json!(e);
    }
    if let Some(k) = &args.anchor_kind {
        payload["anchor_kind"] = json!(k);
    }
    if let Some(sym) = &args.symbol {
        payload["symbol"] = json!(sym);
    }
    tool_emit(
        dispatch_cli_tool(&engine, "tz_expand", payload),
        args.json,
        "expand",
    )
}

fn emit_rewrite(args: RewriteArgs) -> Result<()> {
    let command = match (&args.command, args.argv.is_empty()) {
        (Some(command), _) => command.clone(),
        (None, false) => display_command_for_platform(
            &normalize_command(&args.argv),
            None,
            tokenzero_runtime::current_platform(),
        ),
        (None, true) => anyhow::bail!("rewrite requires a command string or `-- <command...>`"),
    };
    let root = tokenzero_work_root(None);
    let engine = engine_new(
        &root,
        default_allowed_roots(&root),
        None,
        4000,
        Mode::Auto,
        default_shell_timeout(),
        None,
    );
    let response = dispatch_cli_tool(
        &engine,
        "tz_rewrite",
        json!({ "command": command, "mode": args.mode }),
    );
    emit_with_json(response, args.json)
}

fn path_display(p: &Path) -> String {
    p.display().to_string()
}

fn doctor_report(args: &DoctorArgs) -> serde_json::Value {
    let root = tokenzero_work_root(args.root.clone());
    let mut report = install::doctor(&root, args.cache_path.as_deref());
    let effective = allowed_roots_for_workspace(&root, &[]);
    report["effective_allowed_roots"] = json!(
        effective
            .iter()
            .map(|p| path_display(p))
            .collect::<Vec<_>>()
    );
    report["allowlist_algorithm"] = json!(
        "effective roots = doctor/call root union configured --allowed-root entries, deduped by canonical path. Relative CodeMode paths join to execute root."
    );
    let store = zerostack_store::store_resolution_report(&root, args.cache_path.clone());
    report["store_resolution"] =
        zerostack_store::store_resolution_json(&root, args.cache_path.clone());
    report["effective_store_root"] =
        json!(store.effective_store_root.as_ref().map(|p| path_display(p)));
    report["effective_cache_path"] = json!(path_display(&store.effective_cache_path));
    report["migration"] =
        tokenzero_recovery::RecoveryStore::new(Some(store.effective_cache_path.clone()))
            .migration_state();
    report["recovery_blobs"] =
        tokenzero_recovery::recovery_blob_status(&store.effective_cache_path);
    report["engine_binaries"] = tokenzero_mcp_compat::engine_binaries_json();
    if let Some(summary) = &store.mismatch_summary {
        let mismatch = store.store_project_mismatch;
        let finding = json!({"id": if mismatch {"tz-store-project-mismatch"} else {"tz-store-global-pin-ignored"}, "severity": if mismatch {"warning"} else {"info"}, "status": "detected", "check": "store_resolution", "summary": summary, "evidence": {"project_root": path_display(&root), "effective_cache_path": path_display(&store.effective_cache_path), "effective_store_root": store.effective_store_root.as_ref().map(|p| path_display(p)), "shared_store_opt_in": store.shared_store_opt_in, "global_pin_set": store.global_pin_set, "isolation_mode": store.isolation_mode}, "auto_fix": false, "fix_supported": false, "next_step": if mismatch {"Use a per-project store (unset TOKENZERO_SHARED_STORE / ZEROSTACK_SHARED_STORE) or pass --cache-path under the project root."} else {"Default is per-project isolation (wqw.2). Set TOKENZERO_SHARED_STORE=1 only for intentional meta-workspace sharing."}});
        if let Some(findings) = report.get_mut("findings").and_then(|v| v.as_array_mut()) {
            findings.push(finding);
        }
    }
    if args.runtime {
        let plan =
            tokenzero_runtime::plan_command(&["echo".into(), "ok".into()], Some(&root), false).ok();
        report["runtime"] = serde_json::to_value(plan).unwrap_or(json!(null));
    }
    report
}

fn handle_doctor(args: DoctorArgs) -> Result<()> {
    let root = || tokenzero_work_root(args.root.clone());
    let cache = args.cache_path.as_deref();
    match args.command.clone() {
        Some(DoctorCommand::Capabilities) => emit_exit_json(install::doctor_capabilities()),
        Some(DoctorCommand::Health) => emit_doctor_health(&args),
        Some(DoctorCommand::Fix) => {
            emit_exit_json(install::doctor_fix(&root(), cache, args.dry_run))
        }
        Some(DoctorCommand::Undo { run_id }) => {
            emit_exit_json(install::doctor_undo(&root(), &run_id))
        }
        Some(DoctorCommand::Ls) => emit_exit_json(install::doctor_ls(&root())),
        Some(DoctorCommand::RobotDocs) => {
            print!("{}", install::doctor_robot_docs());
            Ok(())
        }
        Some(DoctorCommand::Explain { finding_id }) => {
            emit_exit_json(install::doctor_explain(&root(), cache, &finding_id))
        }
        Some(DoctorCommand::Diagnose) | None => {
            if args.fix {
                return emit_exit_json(install::doctor_fix(&root(), cache, args.dry_run));
            }
            if let Some(finding_id) = args.explain.as_deref() {
                return emit_exit_json(install::doctor_explain(&root(), cache, finding_id));
            }
            if args.robot_triage {
                return emit_exit_json(install::doctor_robot_triage(&root(), cache));
            }
            emit_exit_json(doctor_report(&args))
        }
    }
}

fn emit_doctor_health(args: &DoctorArgs) -> Result<()> {
    let report = doctor_report(args);
    let u64f = |v: &serde_json::Value| v.as_u64().unwrap_or(0);
    let (ok, status) = (
        report["ok"].as_bool().unwrap_or(false),
        report["status"].as_str().unwrap_or("blocked"),
    );
    let (finding_count, blocking, info) = (
        u64f(&report["finding_count"]),
        u64f(&report["summary"]["blocking_findings"]),
        u64f(&report["summary"]["informational_findings"]),
    );
    let exit_code = doctor_exit_code(&report);
    let doctor_ver = report["doctor_version"]
        .as_str()
        .unwrap_or(env!("CARGO_PKG_VERSION"));
    let line = format!(
        "{status} tokenzero={} doctor={doctor_ver} findings={finding_count} blocking={blocking} info={info}",
        env!("CARGO_PKG_VERSION")
    );
    if args.json {
        emit_exit_json(
            json!({"schema_version": "tokenzero.doctor.health.v1", "status": status, "ok": ok, "line": line, "finding_count": finding_count, "blocking_findings": blocking, "informational_findings": info, "exit_code": exit_code}),
        )
    } else {
        println!("{line}");
        exit_if_nonzero(exit_code);
        Ok(())
    }
}

fn print_pretty<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
fn exit_if_nonzero(code: i32) {
    if code != 0 {
        std::process::exit(code);
    }
}
fn emit_exit_json(value: serde_json::Value) -> Result<()> {
    print_pretty(&value)?;
    exit_if_nonzero(doctor_exit_code(&value));
    Ok(())
}
fn doctor_exit_code(value: &serde_json::Value) -> i32 {
    if let Some(code) = value.get("exit_code").and_then(serde_json::Value::as_i64) {
        return code.clamp(0, 255) as i32;
    }
    if value.get("ok") == Some(&json!(false)) || value.get("status") == Some(&json!("blocked")) {
        1
    } else {
        0
    }
}

fn handle_stats(args: CommonArgs) -> Result<serde_json::Value> {
    let root = tokenzero_work_root(args.root);
    let mut report = serde_json::to_value(report_for_path(&default_ledger_path(&root))?)?;
    let cache = resolve_recovery_cache_path(&root, None);
    report["recovery_blobs"] = tokenzero_recovery::recovery_blob_status(&cache);
    Ok(report)
}

fn handle_pulse(args: PulseArgs) -> Result<()> {
    let ledger_path = default_ledger_path(&tokenzero_work_root(args.root));
    match args.command {
        Some(PulseCommand::Sync) => {
            emit_pulse_result("pulse sync", sync_jsonl_to_sqlite(&ledger_path), args.json)
        }
        Some(PulseCommand::Doctor) => {
            emit_pulse_result("pulse doctor", doctor_jsonl_sqlite(&ledger_path), args.json)
        }
        Some(PulseCommand::ExportJsonl(a)) => emit_pulse_result(
            "pulse export-jsonl",
            export_jsonl(&ledger_path, &a.output),
            args.json,
        ),
        Some(PulseCommand::ImportJsonl(a)) => emit_pulse_result(
            "pulse import-jsonl",
            import_jsonl(&a.input, &ledger_path),
            args.json,
        ),
        Some(PulseCommand::Stats) | None => {
            let _ = sync_jsonl_to_sqlite(&ledger_path);
            let report = report_for_path(&ledger_path)?;
            if args.json {
                print_pretty(&report)
            } else {
                print!("{}", tokenzero_pulse::render_text(&report));
                Ok(())
            }
        }
    }
}

fn handle_session_ledger(args: SessionLedgerArgs) -> Result<()> {
    let root = tokenzero_work_root(args.root);
    let pulse_ledger_path = default_ledger_path(&root);
    let response_ledger_path = tokenzero_mcp_compat::ledger::ledger_path_for_cache(
        &resolve_recovery_cache_path(&root, None),
    );
    match args.command {
        Some(SessionLedgerCommand::Schema) => print_pretty(&SessionLedgerReport::schema_json())?,
        Some(SessionLedgerCommand::Inspect(flags)) => {
            let env_value = std::env::var(tokenzero_mcp_compat::ledger::TELEMETRY_ENV).ok();
            let enabled = tokenzero_mcp_compat::ledger::resolve_telemetry(
                flags.telemetry,
                flags.no_telemetry,
                None,
                env_value.as_deref(),
            );
            let usage_path = tokenzero_mcp_compat::ledger::usage_telemetry_path_for_cache(
                &resolve_recovery_cache_path(&root, None),
            );
            emit_value(
                tokenzero_mcp_compat::ledger::inspect_telemetry(&usage_path, enabled)?,
                args.json,
            )?;
        }
        Some(SessionLedgerCommand::Export) => {
            print_pretty(&SessionLedgerReport::from_ledger(&pulse_ledger_path)?)?
        }
        Some(SessionLedgerCommand::Stats) | None => {
            let report = SessionLedgerReport::from_ledger(&pulse_ledger_path)?;
            if args.json {
                print_pretty(&report)?;
            } else {
                print!("{}", report.render_text());
            }
        }
        Some(SessionLedgerCommand::Query { query }) => {
            let since_ms = |days: u64| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| {
                        u64::try_from(d.as_millis())
                            .unwrap_or(u64::MAX)
                            .saturating_sub(days.saturating_mul(86_400_000))
                    })
                    .unwrap_or(0)
            };
            let query = match query {
                LedgerQueryCommand::Repo { repo, days } => {
                    tokenzero_mcp_compat::ledger::LedgerQuery::RepoCost {
                        repo: repo.to_string_lossy().into_owned(),
                        since_ms: since_ms(days),
                    }
                }
                LedgerQueryCommand::VersionDelta {
                    baseline,
                    candidate,
                    days,
                } => tokenzero_mcp_compat::ledger::LedgerQuery::VersionDelta {
                    baseline,
                    candidate,
                    since_ms: since_ms(days),
                },
                LedgerQueryCommand::AgentSpend { days } => {
                    tokenzero_mcp_compat::ledger::LedgerQuery::AgentSpend {
                        since_ms: since_ms(days),
                    }
                }
            };
            emit_value(
                tokenzero_mcp_compat::ledger::query_ledger(&response_ledger_path, &query)?,
                args.json,
            )?;
        }
    }
    Ok(())
}

fn emit_pulse_result<T: serde::Serialize>(
    operation: &str,
    result: std::io::Result<T>,
    as_json: bool,
) -> Result<()> {
    match result {
        Ok(value) => emit_value(value, as_json),
        Err(err) if as_json => {
            let kind = err.kind();
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"schema_version": "tokenzero.pulse.error.v1", "ok": false, "status": "error", "operation": operation, "error_kind": io_error_kind_name(kind), "retryable": kind == std::io::ErrorKind::WouldBlock, "error": err.to_string(), "exit_code": 1})
                )?
            );
            std::process::exit(1);
        }
        Err(err) => Err(err.into()),
    }
}

fn io_error_kind_name(kind: std::io::ErrorKind) -> &'static str {
    use std::io::ErrorKind as K;
    match kind {
        K::NotFound => "not_found",
        K::PermissionDenied => "permission_denied",
        K::ConnectionRefused => "connection_refused",
        K::ConnectionReset => "connection_reset",
        K::ConnectionAborted => "connection_aborted",
        K::NotConnected => "not_connected",
        K::AddrInUse => "addr_in_use",
        K::AddrNotAvailable => "addr_not_available",
        K::BrokenPipe => "broken_pipe",
        K::AlreadyExists => "already_exists",
        K::WouldBlock => "would_block",
        K::InvalidInput => "invalid_input",
        K::InvalidData => "invalid_data",
        K::TimedOut => "timed_out",
        K::WriteZero => "write_zero",
        K::Interrupted => "interrupted",
        K::Unsupported => "unsupported",
        K::UnexpectedEof => "unexpected_eof",
        K::OutOfMemory => "out_of_memory",
        _ => "other",
    }
}

fn handle_cache(args: CacheArgs) -> Result<()> {
    match args.command {
        CacheCommand::Status(args) => {
            let engine = engine_from_common(&args);
            emit_with_json(dispatch_cli_tool(&engine, "tz_mem", json!({})), args.json)?
        }
        CacheCommand::Prune(args) => {
            let root = tokenzero_work_root(args.root);
            let cache = resolve_recovery_cache_path(&root, args.cache_path);
            let dry_run = !args.apply;
            let mut store = tokenzero_recovery::RecoveryStore::new(Some(cache.clone()));
            let mut report = store.prune_stale(dry_run)?;
            report["maintenance"] = tokenzero_mcp_compat::cache_maintenance(&cache, dry_run);
            emit_value(report, args.json)?;
        }
        CacheCommand::MigrateRefs(args) => {
            cache_migrate!(args.root, args.cache_path, args.json, |m| m
                .run(!args.apply))?
        }
        CacheCommand::MigrateVerify(args) => {
            cache_migrate!(args.root, args.cache_path, args.json, |m| m.verify())?
        }
        CacheCommand::MigrateRollback(args) => {
            cache_migrate!(args.root, args.cache_path, args.json, |m| m
                .rollback(args.apply))?
        }
        CacheCommand::MigrateCleanup(args) => {
            cache_migrate!(args.root, args.cache_path, args.json, |m| {
                m.cleanup(args.apply, args.confirm_cleanup)
            })?
        }
    }
    Ok(())
}

fn handle_cache_pack(args: CachePackArgs) -> Result<()> {
    let root = tokenzero_work_root(args.root.clone());
    let engine = engine_new(
        &root,
        default_allowed_roots(&root),
        args.cache_path.clone(),
        4000,
        Mode::Structured,
        default_shell_timeout(),
        None,
    );
    let response = dispatch_cli_tool(&engine, "tz_cache_pack", json!({ "scope": args.scope }));
    emit_with_json(response, args.json)
}

fn handle_bench(args: BenchArgs) -> Result<()> {
    let BenchCommand::Competitors(args) = args.command;
    let report = run_bench_competitors(args)?;
    print_pretty(&report)
}

fn install_apply_or_plan(
    root: &Path,
    global: bool,
    capabilities: &[String],
    agents: &[String],
    surface: McpToolSurface,
    apply: bool,
    as_json: bool,
) -> Result<()> {
    if apply {
        emit_value(
            install::apply_for_agents(root, global, capabilities, agents, surface)?,
            as_json,
        )
    } else {
        emit_value(
            install::plan_for_agents(root, global, capabilities, agents, surface),
            as_json,
        )
    }
}

fn handle_install(args: InstallArgs) -> Result<()> {
    let agents = install_agents(&args.agents, args.grok)?;
    let capabilities = install_capabilities(&args);
    let surface = parse_mcp_surface(&args.surface)?;
    let root = install_root(args.root.clone(), args.global);
    if let Some(id) = args.rollback {
        emit_value(install::rollback(&root, &id)?, args.json)
    } else {
        install_apply_or_plan(
            &root,
            args.global,
            &capabilities,
            &agents,
            surface,
            args.apply,
            args.json,
        )
    }
}

fn handle_init(args: InitArgs) -> Result<()> {
    let _plan_requested = args.plan;
    install_apply_or_plan(
        &install_root(args.root.clone(), args.global),
        args.global,
        &init_capabilities(&args),
        &install_agents(&args.agents, false)?,
        parse_mcp_surface(&args.surface)?,
        args.apply,
        args.json,
    )
}

fn handle_clients(args: ClientsArgs) -> Result<()> {
    match args.command {
        ClientsCommand::Detect(args) => handle_client_status(args),
        ClientsCommand::Scan(args) => handle_clients_scan(args),
        ClientsCommand::Plan(args) => handle_clients_plan(args),
        ClientsCommand::Doctor(args) => handle_clients_doctor(args),
        ClientsCommand::Rollback(args) => handle_clients_rollback(args),
    }
}

/// Presence scan: which AI harnesses live on this machine, and the install
/// invocation that wires the supported ones. Detection only — nothing is
/// written.
fn handle_clients_scan(args: ClientStatusArgs) -> Result<()> {
    let home = install_root(args.root.clone(), true);
    let detected = install::detect_present_agents(&home, std::env::var("PATH").ok().as_deref());
    let supported: Vec<&str> = detected
        .iter()
        .filter(|a| a.supported)
        .map(|a| a.agent.as_str())
        .collect();
    let next_step = if supported.is_empty() {
        "no supported harnesses detected; docs/routing.md covers manual adapters".to_string()
    } else {
        format!(
            "tokenzero install --global --apply --hooks{}",
            supported
                .iter()
                .map(|a| format!(" --agent {a}"))
                .collect::<String>()
        )
    };
    emit_value(
        json!({"schema_version": "tokenzero.clients.v1", "command": "clients scan", "status": "ok", "home": path_display(&home), "detected": detected, "unsupported_note": "supported=false entries need the manual adapter snippets in docs/routing.md", "next_step": next_step}),
        args.json,
    )
}

fn handle_client_status(args: ClientStatusArgs) -> Result<()> {
    emit_value(
        client_status_report(
            &install_root(args.root.clone(), true),
            &install_agents(&args.agents, args.grok)?,
            "detect",
        )?,
        args.json,
    )
}

fn handle_clients_plan(args: ClientsPlanArgs) -> Result<()> {
    let profile = clients_profile(&args.profile)?;
    let agents = install_agents(&args.agents, args.grok)?;
    let root = install_root(args.root.clone(), true);
    let mut value = serde_json::to_value(install::plan_for_agents(
        &root,
        true,
        &clients_capabilities(&profile),
        &agents,
        clients_mcp_surface(&profile),
    ))?;
    if let Some(object) = value.as_object_mut() {
        object.extend([
            ("schema_version".into(), json!("tokenzero.clients.plan.v1")),
            ("command".into(), json!("clients plan")),
            ("profile".into(), json!(profile)),
            ("root".into(), json!(path_display(&root))),
            ("agents".into(), json!(clients_agent_labels(&agents))),
        ]);
    }
    emit_value(value, args.json)
}

const CLIENTS_DOCTOR_FINDINGS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "installed",
        "tz-clients-installed",
        "info",
        "TokenZero client integration surfaces are present",
        "Run tokenzero doctor --json for runtime health.",
    ),
    (
        "mixed",
        "tz-clients-mixed",
        "warning",
        "Some TokenZero client integration surfaces are present and some are missing",
        "Run tokenzero clients plan --profile standard --json, review the plan, then use tokenzero install --global --apply --mcp --json if approved.",
    ),
    (
        "",
        "tz-clients-missing",
        "info",
        "No TokenZero client integration surfaces were detected at the planned target paths",
        "Run tokenzero clients plan --profile standard --json to inspect the read-only integration plan.",
    ),
];

fn clients_doctor_findings(status: &str) -> Vec<serde_json::Value> {
    let row = CLIENTS_DOCTOR_FINDINGS
        .iter()
        .find(|(s, ..)| *s == status)
        .unwrap_or(&CLIENTS_DOCTOR_FINDINGS[2]);
    vec![json!({"id": row.1, "severity": row.2, "summary": row.3, "next_step": row.4})]
}

fn handle_clients_doctor(args: ClientStatusArgs) -> Result<()> {
    let mut report = client_status_report(
        &install_root(args.root.clone(), true),
        &install_agents(&args.agents, args.grok)?,
        "doctor",
    )?;
    let status = report
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let findings = clients_doctor_findings(status);
    if let Some(object) = report.as_object_mut() {
        object.insert("findings".to_string(), json!(findings));
    }
    emit_value(report, args.json)
}

fn handle_clients_rollback(args: ClientsRollbackArgs) -> Result<()> {
    emit_value(
        install::rollback(&install_root(args.root.clone(), true), &args.id)?,
        args.json,
    )
}

fn handle_capabilities(args: CapabilitiesArgs) -> Result<()> {
    emit_value(capabilities_json(), args.json)
}

fn handle_robot_docs(args: RobotDocsArgs) {
    print!(
        "{}",
        match args.command {
            RobotDocsCommand::Guide => robot_docs_guide(),
            RobotDocsCommand::Commands => agent_surfaces::robot_docs_commands(),
            RobotDocsCommand::Examples => agent_surfaces::robot_docs_examples(),
        }
    );
}

fn handle_package_audit(args: PackageAuditArgs) -> serde_json::Value {
    let root = tokenzero_work_root(None);
    let artifacts = if args.dist.as_path() == Path::new(".") {
        Vec::new()
    } else if args.dist.exists() && args.dist.is_file() {
        vec![args.dist]
    } else if args.dist.exists() {
        fs::read_dir(args.dist)
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    install::package_audit(&root, &artifacts)
}

fn handle_quote(args: QuoteArgs) -> Result<()> {
    let argv = normalize_command(&args.args);
    let quoted = quote_for(&args.platform, &argv);
    if args.json {
        print_pretty(&json!({"platform": args.platform, "argv": argv, "command": quoted}))
    } else {
        println!("{quoted}");
        Ok(())
    }
}

fn engine_config(
    root: &Path,
    allowed_roots: Vec<PathBuf>,
    cache_path: PathBuf,
    max_visible_tokens: usize,
    mode: Mode,
    shell_timeout: std::time::Duration,
    mcp_idle_timeout: Option<std::time::Duration>,
) -> EngineConfig {
    EngineConfig {
        allowed_roots,
        cache_path,
        max_visible_tokens,
        mode,
        shell_timeout,
        mcp_idle_timeout,
        ..EngineConfig::for_root(root)
    }
}

fn engine_new(
    root: &Path,
    allowed_roots: Vec<PathBuf>,
    cache_path: Option<PathBuf>,
    budget: usize,
    mode: Mode,
    shell_timeout: std::time::Duration,
    mcp_idle: Option<std::time::Duration>,
) -> TokenZeroEngine {
    TokenZeroEngine::new_cli(engine_config(
        root,
        allowed_roots,
        resolve_recovery_cache_path(root, cache_path),
        budget,
        mode,
        shell_timeout,
        mcp_idle,
    ))
}

fn engine_from_tool(args: &ToolArgs) -> Result<TokenZeroEngine> {
    let root = tokenzero_work_root(None);
    Ok(engine_new(
        &root,
        allowed_roots_for_workspace(&root, &args.allowed_root),
        args.cache_path.clone(),
        args.budget.unwrap_or(4000),
        parse_mode(&args.mode)?,
        shell_timeout_from_secs(args.timeout_seconds),
        None,
    ))
}

fn engine_from_common(args: &CommonArgs) -> TokenZeroEngine {
    let root = tokenzero_work_root(args.root.clone());
    engine_new(
        &root,
        default_allowed_roots(&root),
        args.cache_path.clone(),
        4000,
        Mode::Auto,
        default_shell_timeout(),
        None,
    )
}

fn engine_config_for_mcp(args: &McpServerArgs) -> Result<EngineConfig> {
    let root = mcp_work_root(&args.allowed_root);
    let tool_surface = args
        .tool_surface
        .as_deref()
        .unwrap_or(&args.mode)
        .parse::<McpToolSurface>()
        .map_err(anyhow::Error::msg)?;
    let mut config = engine_config(
        &root,
        allowed_roots_for_workspace(&root, &args.allowed_root),
        resolve_recovery_cache_path(&root, args.cache_path.clone()),
        4000,
        parse_mode(&args.default_mode)?,
        shell_timeout_from_secs(args.shell_timeout_seconds),
        mcp_idle_timeout_from_secs(args.idle_timeout_seconds),
    );
    config.tool_surface = tool_surface;
    Ok(config)
}

fn mcp_work_root(allowed_roots: &[PathBuf]) -> PathBuf {
    tokenzero_work_root(allowed_roots.first().cloned())
}

/// Long-lived MCP/CodeMode servers run at reduced scheduling priority so a
/// busy worker cannot starve interactive sessions (multi-project runaway CPU,
/// 2026-07-16 incident). `TOKENZERO_NO_RENICE=1` opts out.
#[cfg(unix)]
fn codemode_host_niceness() {
    if std::env::var_os("TOKENZERO_NO_RENICE").is_some() {
        return;
    }
    let _ = std::process::Command::new("renice")
        .args(["-n", "5", "-p", &std::process::id().to_string()])
        .output();
}

#[cfg(not(unix))]
fn codemode_host_niceness() {}

/// MCP XOR CodeMode process/artifact mutual exclusion (tokenzero-irx9.3).
///
/// One running process must never expose both catalogs:
/// 1. Dual compiled surfaces fail closed (also a compile_error).
/// 2. Dual argv / env selection fails closed.
/// 3. Startup surface is resolved to exactly one compiled surface.
/// 4. Hub sentinel: when CodeMode hub owns the root, refuse per-op MCP.
///
/// `TOKENZERO_ALLOW_DUAL=1` only skips the hub sentinel (debug); it never
/// permits dual catalog compilation or dual `--mode` selection.
fn enforce_surface_exclusivity(args: &McpServerArgs) -> Result<()> {
    if let Err(err) = install::packaging::reject_dual_compiled_surfaces() {
        anyhow::bail!("{err}");
    }
    let argv: Vec<String> = std::env::args().collect();
    let resolved =
        install::packaging::resolve_startup_surface(&argv).map_err(|e| anyhow::anyhow!("{e}"))?;

    let requested = args.tool_surface.as_deref().unwrap_or(&args.mode);
    let requested_surface =
        install::packaging::PackageSurface::parse(requested).map_err(|e| anyhow::anyhow!("{e}"))?;
    if requested_surface != resolved {
        anyhow::bail!(
            "tokenzero: process surface is locked to '{}'; refused request for '{}'. \
Install {} for that surface (mutually exclusive — one process, one catalog).",
            resolved.as_str(),
            requested_surface.as_str(),
            requested_surface.artifact_name()
        );
    }
    install::packaging::assert_surface_compiled(resolved).map_err(|e| anyhow::anyhow!("{e}"))?;

    #[cfg(not(unix))]
    {
        return Ok(());
    }
    #[cfg(unix)]
    {
        let surface = args.tool_surface.as_deref().unwrap_or(&args.mode);
        if surface != "mcp" || std::env::var_os("TOKENZERO_ALLOW_DUAL").is_some() {
            return Ok(());
        }
        let root = mcp_work_root(&args.allowed_root);
        let sentinel = root.join(".zerostack").join("codemode.active");
        let Ok(raw) = std::fs::read_to_string(&sentinel) else {
            return Ok(());
        };
        let pid = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|value| value.get("pid").and_then(serde_json::Value::as_u64));
        let live = pid.is_some_and(|pid| {
            std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .is_ok_and(|status| status.success())
        });
        if live {
            anyhow::bail!(
                "CodeMode hub is active for {} (pid {} via {}); per-op MCP and CodeMode must not run together for one repo. Stop the hub or set TOKENZERO_ALLOW_DUAL=1 (hub sentinel only — never dual catalogs).",
                root.display(),
                pid.unwrap_or(0),
                sentinel.display()
            );
        }
        Ok(())
    }
}

/// Rebuilds the mcp-server invocation for the supervised inner child:
/// same configuration, no --supervise (one supervisor only), and idle exit
/// pinned off because the supervisor owns the session lifecycle.
fn supervised_child_args(args: &McpServerArgs) -> Vec<OsString> {
    let mut child_args: Vec<OsString> = vec![
        "mcp-server".into(),
        "--mode".into(),
        args.mode.clone().into(),
        "--default-mode".into(),
        args.default_mode.clone().into(),
        "--idle-timeout-seconds".into(),
        "0".into(),
    ];
    let mut push_opt = |flag: &str, value: OsString| {
        child_args.push(flag.into());
        child_args.push(value);
    };
    for root in &args.allowed_root {
        push_opt("--allowed-root", root.clone().into_os_string());
    }
    if let Some(cache_path) = &args.cache_path {
        push_opt("--cache-path", cache_path.clone().into_os_string());
    }
    if let Some(seconds) = args.shell_timeout_seconds {
        push_opt("--shell-timeout-seconds", seconds.to_string().into());
    }
    if let Some(surface) = &args.tool_surface {
        push_opt("--tool-surface", surface.clone().into());
    }
    child_args
}

fn existing_path_is_within_allowed_roots(path: &Path, allowed_roots: &[PathBuf]) -> bool {
    let Ok(candidate) = path.canonicalize() else {
        return true;
    };
    allowed_roots.iter().any(|root| {
        root.canonicalize()
            .is_ok_and(|allowed| candidate == allowed || candidate.starts_with(allowed))
    })
}

fn install_root(root: Option<PathBuf>, global: bool) -> PathBuf {
    root.or_else(|| global.then(platform_home_dir).flatten())
        .unwrap_or_else(|| tokenzero_work_root(None))
}

fn platform_home_dir() -> Option<PathBuf> {
    home_dir_from_env(|name| std::env::var_os(name), cfg!(windows))
}

fn home_dir_from_env<F>(mut var: F, windows: bool) -> Option<PathBuf>
where
    F: FnMut(&str) -> Option<OsString>,
{
    let mut nonempty = |name: &str| var(name).filter(|value| !value.as_os_str().is_empty());
    if windows {
        if let Some(userprofile) = nonempty("USERPROFILE") {
            return Some(PathBuf::from(userprofile));
        }
        if let (Some(mut drive), Some(path)) = (nonempty("HOMEDRIVE"), nonempty("HOMEPATH")) {
            drive.push(path);
            return Some(PathBuf::from(drive));
        }
    }
    nonempty("HOME").map(PathBuf::from)
}

fn parse_mode(value: &str) -> Result<Mode> {
    value.parse::<Mode>().map_err(anyhow::Error::msg)
}

fn normalize_command(command: &[String]) -> Vec<String> {
    let parts = match command {
        [first, rest @ ..] if first == "--" => rest,
        _ => command,
    };
    match parts {
        [part] if !contains_platform_shell_syntax(part, tokenzero_runtime::current_platform()) => {
            split_command_string(part)
        }
        _ => parts.to_vec(),
    }
}

fn content_type_from_kind(kind: &str, text: &str, path: Option<&Path>) -> ContentType {
    match kind {
        "code" => ContentType::Code,
        "shell" | "tool-output" => ContentType::ShellOutput,
        "diff" => ContentType::Diff,
        "json" => ContentType::JsonConfig,
        "markdown" | "pack" => ContentType::Markdown,
        "log" => ContentType::Logs,
        _ => detect_content_type(text, path),
    }
}

fn parse_line_token(value: &str) -> Option<usize> {
    value.trim().trim_start_matches('L').parse().ok()
}

fn expand_selector(args: &ExpandArgs) -> (Option<String>, Option<usize>, Option<usize>) {
    let mut selector = args.selector.clone();
    if args.raw || selector.is_none() {
        selector = Some("raw".into());
    }
    if args.summary {
        selector = Some("summary".into());
    }
    let mut start = args.start_line;
    let mut end = args.end_line;
    if let Some(line) = args.line {
        start = Some(line);
        end = Some(line);
    }
    if let Some(lines) = args.lines.as_deref() {
        let value = lines.trim().trim_start_matches('L');
        if let Some((s, e)) = value.split_once('-') {
            start = s.parse().ok();
            end = e.parse().ok();
        } else {
            start = value.parse().ok();
            end = start;
        }
    }
    if let Some(around) = args.around.as_deref() {
        let (line, radius) = around.split_once(':').unwrap_or((around, "3"));
        let line = parse_line_token(line).unwrap_or(1);
        let radius = radius.parse::<usize>().unwrap_or(3);
        start = Some(line.saturating_sub(radius).max(1));
        end = Some(line + radius);
    }
    (selector, start, end)
}

fn capability_list(
    want_mcp: bool,
    shell: bool,
    instructions: bool,
    cli: bool,
    hooks: bool,
    shims: bool,
    default_mcp_if_empty: bool,
) -> Vec<String> {
    let flags = [
        (want_mcp, "mcp"),
        (shell, "shell"),
        (instructions, "instructions"),
        (cli, "cli"),
        (hooks, "hooks"),
        (shims, "shim"),
    ];
    let mut caps: Vec<String> = flags
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| name.to_string())
        .collect();
    if default_mcp_if_empty && caps.is_empty() {
        caps.push("mcp".to_string());
    }
    caps
}

fn install_capabilities(args: &InstallArgs) -> Vec<String> {
    capability_list(
        args.mcp || args.grok || !args.agents.is_empty(),
        args.shell,
        args.instructions,
        args.cli,
        args.hooks,
        args.shims,
        false,
    )
}

fn init_capabilities(args: &InitArgs) -> Vec<String> {
    capability_list(
        args.mcp || !args.agents.is_empty(),
        args.shell,
        args.instructions,
        args.cli,
        args.hooks,
        args.shims,
        true,
    )
}

fn clients_profile(raw: &str) -> Result<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "standard" | "" => Ok("standard".to_string()),
        "codemode" | "code-mode" => Ok("codemode".to_string()),
        other => anyhow::bail!(
            "unsupported clients profile '{other}'; supported profiles: standard, codemode"
        ),
    }
}

fn clients_capabilities(profile: &str) -> Vec<String> {
    let mut caps = vec!["mcp".to_string(), "hooks".to_string()];
    if profile == "codemode" {
        caps.push("instructions".to_string());
    }
    caps
}

fn clients_mcp_surface(_profile: &str) -> McpToolSurface {
    McpToolSurface::Classic
}
fn parse_mcp_surface(raw: &str) -> Result<McpToolSurface> {
    raw.parse()
        .map_err(|message: String| anyhow::anyhow!(message))
}
fn clients_agent_labels(agents: &[String]) -> Vec<String> {
    if agents.is_empty() {
        vec!["all".to_string()]
    } else {
        agents.to_vec()
    }
}

fn client_status_report(
    root: &Path,
    agents: &[String],
    command: &str,
) -> Result<serde_json::Value> {
    let plan = install::plan_for_agents(
        root,
        true,
        &clients_capabilities("standard"),
        agents,
        McpToolSurface::Classic,
    );
    let surfaces: Vec<serde_json::Value> = plan
        .writes
        .iter()
        .map(|write| client_surface_status(write, root))
        .collect::<Result<_>>()?;
    let count = |state: &str| surfaces.iter().filter(|s| s["state"] == state).count();
    let (installed, mixed, missing) = (count("installed"), count("mixed"), count("missing"));
    let status = if installed > 0 && missing == 0 && mixed == 0 {
        "installed"
    } else if installed > 0 || mixed > 0 {
        "mixed"
    } else {
        "missing"
    };
    Ok(
        json!({"schema_version": "tokenzero.clients.v1", "status": status, "ok": true, "command": format!("clients {command}"), "root": path_display(root), "global": true, "profile": "standard", "agents": clients_agent_labels(agents), "summary": {"installed": installed, "mixed": mixed, "missing": missing, "total": surfaces.len(), "raw_bypass_risk": status != "installed"}, "surfaces": surfaces, "next_action": if status == "installed" {"Run tokenzero doctor --json to verify runtime health."} else {"Run tokenzero clients plan --profile standard --json to review the read-only integration plan."}}),
    )
}

fn client_surface_status(write: &install::InstallWrite, root: &Path) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(install::inspect_client_surface(
        write, root,
    ))?)
}

fn install_agents(raw_agents: &[String], grok: bool) -> Result<Vec<String>> {
    let mut agents = Vec::new();
    if grok {
        push_agent(&mut agents, "grok")?;
    }
    for raw in raw_agents {
        push_agent(&mut agents, raw)?;
    }
    Ok(agents)
}

const AGENT_ALIASES: &[(&str, &str)] = &[
    ("claude", "claude"),
    ("claude-code", "claude"),
    ("claude-desktop", "claude"),
    ("codex", "codex"),
    ("cursor", "cursor"),
    ("droid", "droid"),
    ("factory-droid", "droid"),
    ("factory", "factory"),
    ("gemini", "gemini"),
    ("grok", "grok"),
    ("opencode", "opencode"),
    ("open-code", "opencode"),
];

fn push_agent(agents: &mut Vec<String>, raw: &str) -> Result<()> {
    let normalized = raw.trim().to_ascii_lowercase().replace(['_', ' '], "-");
    if normalized == "all" {
        return Ok(());
    }
    if normalized.is_empty() {
        anyhow::bail!("--agent requires a non-empty agent name");
    }
    let Some((_, agent)) = AGENT_ALIASES.iter().find(|(alias, _)| *alias == normalized) else {
        anyhow::bail!(
            "unsupported agent '{normalized}'; expected one of claude, codex, cursor, droid, factory, gemini, grok, opencode, or all"
        );
    };
    if !agents.iter().any(|existing| existing == *agent) {
        agents.push((*agent).to_string());
    }
    Ok(())
}

fn emit(value: EmitResponse) -> Result<()> {
    emit_with_json(value.response, value.json)
}

fn render_cli_text(response: &ToolResponse) -> String {
    let rendered = render_text(response);
    let Some(telemetry) = response
        .telemetry
        .as_ref()
        .filter(|_| response.tool == "shell")
    else {
        return rendered;
    };
    if telemetry
        .get("output_strategy")
        .and_then(serde_json::Value::as_str)
        != Some("inline_shell")
    {
        return rendered;
    }

    let mut out = String::new();
    if telemetry
        .pointer("/stdout_capture/bytes")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|bytes| bytes > 0)
    {
        out.push_str("stdout:\n");
    }
    out.push_str(&rendered);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if let Some(exit_code) = telemetry
        .get("exit_code")
        .and_then(serde_json::Value::as_i64)
    {
        out.push_str(&format!("exit_code: {exit_code}\n"));
    }
    out
}

fn emit_with_json(response: ToolResponse, as_json: bool) -> Result<()> {
    let exit_error = response.status == "error";
    if as_json {
        println!("{}", cli_json(&response));
    } else if response.tool == "expand" && response.status == "ok" {
        if let Some(visible) = &response.visible {
            print!("{}", visible.text);
        }
    } else {
        print!("{}", render_cli_text(&response));
    }
    if exit_error {
        std::process::exit(1);
    }
    // nt0i: text mode always mirrors the child; JSON mode historically keeps
    // exit 0 (machine consumers read telemetry.command_success). The opt-in
    // gate mirrors the child in JSON mode too, for harnesses that gate on the
    // process exit code. Default flip rides the 1cwf envelope contract bump.
    if !as_json || run_child_exit_enabled() {
        if let Some(code) = child_failure_exit_code(&response) {
            std::process::exit(code);
        }
    }
    Ok(())
}

/// nt0i: opt-in (TOKENZERO_RUN_CHILD_EXIT) child exit-code mirroring for
/// --json `run` envelopes.
pub fn run_child_exit_enabled() -> bool {
    std::env::var("TOKENZERO_RUN_CHILD_EXIT")
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "on" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

/// Text-mode `run` mirrors the child's exit status so `&&`/`||` chains and CI
/// wrappers observe failures. `--json` keeps the exit-0 envelope contract:
/// machine consumers read `telemetry.command_success` instead.
fn child_failure_exit_code(response: &ToolResponse) -> Option<i32> {
    if response.tool != "shell" {
        return None;
    }
    let telemetry = response.telemetry.as_ref()?;
    if telemetry
        .get("command_success")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        return None;
    }
    match telemetry
        .get("exit_code")
        .and_then(serde_json::Value::as_i64)
    {
        Some(0) => None,
        Some(code) => Some(code.clamp(1, 255) as i32),
        None => Some(1),
    }
}

fn emit_value<T: serde::Serialize>(value: T, _as_json: bool) -> Result<()> {
    let json_value = serde_json::to_value(value)?;
    print_pretty(&json_value)?;
    exit_if_nonzero(doctor_exit_code(&json_value));
    Ok(())
}

fn record_tool_pulse(response: &ToolResponse, root: PathBuf, tool: &str) -> Result<()> {
    if let Some(accounting) = response.accounting.as_ref() {
        let event = PulseEvent::tool_call(
            tool,
            response.mode.as_deref().unwrap_or("hybrid"),
            accounting.raw_tokens,
            accounting.visible_tokens,
            accounting.recovery_tokens,
            response.refs.len(),
            response
                .telemetry
                .as_ref()
                .and_then(|v| v.get("latency_ms"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u128,
            None,
        );
        let _ = record_event(&default_ledger_path(&root), &event);
    }
    Ok(())
}

mod audits;
use audits::bench::*;
use audits::os_reach::*;
use audits::recovery::*;
use audits::release::*;
