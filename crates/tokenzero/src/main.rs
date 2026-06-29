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
use tokenzero_core::{
    Accounting, ContentType, Mode, ToolResponse, count_tokens, detect_content_type,
    shell_display_command_from_argv_for_platform,
};
use tokenzero_filters::{discover, rewrite_command};
use tokenzero_install as install;
use tokenzero_mcp::{CodeModeOptions, CodeModeStatus, execute_codemode_with_options};

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
use agent_surfaces::{capabilities_json, robot_docs_guide};
use artifact_contracts::{json_artifact_path, release_candidate_id};
use cli_args::*;
use competitor_adapters::{
    competitor_adapter_matrix, competitor_adapter_rows, load_benchmark_adapter_approval,
};
use mcp_artifact::run_mcp_artifact;
use reach::{installed_tokenzero_command_audit, run_reach};
use release_claims::{ClaimEvidenceInputs, run_claim_audit};
use tokenzero_mcp::{
    EditHunk, EngineConfig, TokenZeroEngine, cli_json, default_shell_timeout,
    mcp_idle_timeout_from_secs, render_text, shell_timeout_from_secs,
};
use tokenzero_pulse::{
    PulseEvent, default_ledger_path, doctor_jsonl_sqlite, export_jsonl, import_jsonl, record_event,
    report_for_path, sync_jsonl_to_sqlite,
};
use tokenzero_runtime::{
    ExecutionMode, contains_platform_shell_syntax, env_map, plan_command_for_platform, quote_for,
    split_command_string,
};
use zerostack_store::{
    allowed_roots_for_workspace, default_allowed_roots, resolve_recovery_cache_path,
    tokenzero_work_root,
};

fn main() -> Result<()> {
    let cli = Cli::parse_from(normalize_agent_invocation_args(std::env::args_os()));
    let Some(command) = cli.command else {
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };
    match command {
        Commands::Read(args) => emit(handle_read(args)?)?,
        Commands::Find(args) => emit(handle_find(args)?)?,
        Commands::Grep(args) => emit(handle_grep(args)?)?,
        Commands::Glob(args) => emit(handle_glob(args)?)?,
        Commands::Tree(args) => emit(handle_tree(args)?)?,
        Commands::Edit(args) => emit(handle_edit(args)?)?,
        Commands::Recall(args) => emit(handle_recall(args)?)?,
        Commands::Fetch(args) => emit(handle_fetch(args)?)?,
        Commands::Run(args) => emit(handle_run(args)?)?,
        Commands::Ingest(args) => emit(handle_ingest(args)?)?,
        Commands::Expand(args) => emit(handle_expand(args)?)?,
        Commands::Mem(args) => emit_with_json(engine_from_common(&args).mem(), args.json)?,
        Commands::Rewrite(args) | Commands::RewriteCommand(args) => emit_rewrite(args)?,
        // Fail-open hook contract: handle_hook never errors and never sets a
        // nonzero exit; a failing hook would degrade the harness's Bash tool.
        Commands::Hook(args) => hook::handle_hook(args),
        Commands::Discover(args) => emit_value(discover(), args.json)?,
        Commands::Doctor(args) => handle_doctor(args)?,
        Commands::Stats(args) => {
            let as_json = args.json;
            emit_value(handle_stats(args)?, as_json)?;
        }
        Commands::Pulse(args) => handle_pulse(args)?,
        Commands::Cache(args) => handle_cache(args)?,
        Commands::Install(args) => handle_install(args)?,
        Commands::Init(args) => handle_init(args)?,
        Commands::Clients(args) => handle_clients(args)?,
        Commands::ClientStatus(args) => handle_client_status(args)?,
        Commands::Capabilities(args) => handle_capabilities(args)?,
        Commands::RobotDocs(args) => handle_robot_docs(args),
        Commands::CachePack(args) => handle_cache_pack(args)?,
        Commands::Bench(args) => handle_bench(args)?,
        Commands::McpServer(args) => {
            if args.supervise {
                let program = std::env::current_exe()
                    .map(std::ffi::OsString::from)
                    .unwrap_or_else(|_| std::ffi::OsString::from("tokenzero"));
                std::process::exit(tokenzero_mcp::run_supervised_stdio(
                    program,
                    supervised_child_args(&args),
                ))
            }
            std::process::exit(tokenzero_mcp::run_stdio(engine_config_for_mcp(&args)?))
        }
        Commands::McpSmoke(args) => emit_value(
            run_mcp_artifact(args.output_json, args.output_md, 1)?,
            args.json,
        )?,
        Commands::McpSoak(args) => emit_value(
            run_mcp_artifact(args.output_json, args.output_md, 25)?,
            args.json,
        )?,
        Commands::ExactRecoveryShell(args) => emit_value(
            run_exact_recovery_shell(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::ExactRecoveryAudit(args) => emit_value(
            run_exact_recovery_audit(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::HarmEval(args) => {
            emit_value(run_harm_eval(args.output_json, args.output_md)?, args.json)?
        }
        Commands::ProtectedAnchorAudit(args) => emit_value(
            run_protected_anchor_audit(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::FalseSuccessShell(args) => emit_value(
            run_false_success_shell(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::RepoInventory(args) => emit_value(
            run_repo_inventory(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::PromptCachePack(args) => emit_value(
            run_prompt_cache_pack(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::InstallSmoke(args) => {
            emit_value(run_install_smoke(args.output_json)?, args.json)?
        }
        Commands::PackageAudit(args) => {
            let as_json = args.json;
            emit_value(handle_package_audit(args), as_json)?;
        }
        Commands::ShellMatrix(args) => emit_value(
            run_shell_matrix(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::OsReachAudit(args) => emit_value(
            run_os_reach_audit(
                args.output_json,
                args.output_md,
                args.root,
                args.os_artifact,
                args.release_approval,
            )?,
            args.json,
        )?,
        Commands::OsReleaseArtifact(args) => emit_value(
            run_os_release_artifact(args.output_json, args.output_md, args.root)?,
            args.json,
        )?,
        Commands::OneShotEval(args) => emit_value(
            run_one_shot_eval(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::SourceCurrencyAudit(args) => emit_value(
            run_source_currency_audit(
                args.output_json,
                args.output_md,
                args.refresh_ledger,
                args.refresh_git_heads,
            )?,
            args.json,
        )?,
        Commands::AdapterApprovalAudit(args) => emit_value(
            run_adapter_approval_audit(
                args.output_json,
                args.output_md,
                args.approval_file,
                args.execution_approval,
            )?,
            args.json,
        )?,
        Commands::AdapterApprovalTemplate(args) => emit_value(
            run_adapter_approval_template(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::ClaimAudit(args) => emit_value(
            run_claim_audit(
                args.output_json,
                args.output_md,
                args.release_approval,
                ClaimEvidenceInputs {
                    source_artifact: args.source_artifact,
                    benchmark_artifact: args.benchmark_artifact,
                    adapter_approval_artifact: args.adapter_approval_artifact,
                    recovery_artifact: args.recovery_artifact,
                    task_success_artifact: args.task_success_artifact,
                    os_artifact: args.os_artifact,
                },
            )?,
            args.json,
        )?,
        Commands::CompletionAudit(args) => emit_value(
            run_completion_audit(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::SecurityPrivacyAudit(args) => emit_value(
            run_security_privacy_audit(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::ArtifactHandoff(args) => emit_value(
            run_artifact_handoff(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::Reach(args) => emit_value(run_reach(args.root, args.output_json)?, args.json)?,
        Commands::WsSkeleton(args) => emit_value(
            run_ws_skeleton(args.output_json, args.output_md)?,
            args.json,
        )?,
        Commands::CodeMode(args) => {
            let result = execute_codemode_with_options(
                args.plan_text(),
                CodeModeOptions {
                    root: args.root.clone(),
                    allowed_roots: args.allowed_root.clone(),
                    cache_path: args.cache_path.clone(),
                    max_visible_tokens: args.max_visible_tokens,
                    timeout_seconds: args.timeout_seconds,
                },
            );
            let failed = result.status == CodeModeStatus::Error;
            if args.json {
                println!("{}", serde_json::to_string(&result)?);
            } else {
                println!("{}", result.to_line());
            }
            if failed {
                std::io::stdout().flush()?;
                std::process::exit(1);
            }
        }
        Commands::Quote(args) => handle_quote(args)?,
    }
    Ok(())
}

fn normalize_agent_invocation_args<I>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    let argv: Vec<OsString> = args.into_iter().collect();
    if argv.len() <= 1 {
        return argv;
    }

    if argv.len() == 2 && matches!(argv[1].to_str(), Some("--robot-help" | "robot-help")) {
        return vec![
            argv[0].clone(),
            OsString::from("robot-docs"),
            OsString::from("guide"),
        ];
    }

    if argv[1].to_str() == Some("rn") {
        let mut normalized = argv;
        normalized[1] = OsString::from("run");
        return normalize_run_invocation_args(normalized);
    }

    if matches!(argv[1].to_str(), Some("run" | "shell")) {
        return normalize_run_invocation_args(argv);
    }

    if argv[1].to_str() == Some("install") {
        return normalize_install_invocation_args(argv);
    }

    argv
}

fn normalize_install_invocation_args(argv: Vec<OsString>) -> Vec<OsString> {
    if argv.len() < 3 {
        return argv;
    }

    match argv[2].to_str() {
        Some("plan") => {
            let mut normalized = Vec::with_capacity(argv.len() + 1);
            normalized.push(argv[0].clone());
            normalized.push(OsString::from("install"));
            normalized.push(OsString::from("--plan"));
            normalized.extend(argv[3..].iter().cloned());
            normalized
        }
        Some("status") => {
            let mut normalized = Vec::with_capacity(argv.len() + 1);
            normalized.push(argv[0].clone());
            normalized.push(OsString::from("clients"));
            normalized.push(OsString::from("detect"));
            for arg in &argv[3..] {
                if matches!(
                    arg.to_str(),
                    Some("--global" | "--mcp" | "--shell" | "--instructions" | "--cli" | "--plan")
                ) {
                    continue;
                }
                normalized.push(arg.clone());
            }
            normalized
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

fn split_run_args_without_delimiter(args: &[OsString]) -> Option<(Vec<OsString>, Vec<OsString>)> {
    let mut options = Vec::new();
    let mut idx = 0;
    while idx < args.len() {
        let value = args[idx].to_str()?;
        if is_run_bool_option(value) || is_run_value_option_with_equals(value) {
            options.push(args[idx].clone());
            idx += 1;
        } else if is_run_value_option(value) {
            options.push(args[idx].clone());
            idx += 1;
            if idx < args.len() {
                options.push(args[idx].clone());
                idx += 1;
            }
        } else if value.starts_with('-') {
            return None;
        } else {
            break;
        }
    }

    if idx >= args.len() {
        return None;
    }

    let mut command = args[idx..].to_vec();
    while command
        .last()
        .and_then(|arg| arg.to_str())
        .is_some_and(is_run_json_alias)
    {
        if let Some(last) = command.pop() {
            options.push(last);
        }
    }

    if command.is_empty() {
        None
    } else {
        Some((options, command))
    }
}

fn is_run_bool_option(value: &str) -> bool {
    matches!(
        value,
        "--json" | "--jsno" | "--jason" | "--no-rewrite" | "--stdin" | "--explain-runtime"
    )
}

fn is_run_value_option(value: &str) -> bool {
    matches!(
        value,
        "--cwd"
            | "--rewrite"
            | "--env"
            | "--runtime-platform"
            | "--mode"
            | "--budget"
            | "--allowed-root"
            | "--cache-path"
            | "--timeout"
            | "--timeout-seconds"
            | "--timout"
    )
}

fn is_run_value_option_with_equals(value: &str) -> bool {
    [
        "--cwd",
        "--rewrite",
        "--env",
        "--runtime-platform",
        "--mode",
        "--budget",
        "--allowed-root",
        "--cache-path",
        "--timeout",
        "--timeout-seconds",
        "--timout",
    ]
    .iter()
    .any(|option| {
        value
            .strip_prefix(option)
            .is_some_and(|suffix| suffix.starts_with('='))
    })
}

fn is_run_json_alias(value: &str) -> bool {
    matches!(value, "--json" | "--jsno" | "--jason")
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
    let engine = engine_from_tool(&args.tool)?;
    let mode = parse_mode(&args.tool.mode)?;
    let response = engine.read(
        &paths,
        mode,
        args.start_line,
        args.end_line,
        args.raw,
        args.max_files,
        args.max_visible_tokens,
    );
    record_tool_pulse(&response, tokenzero_work_root(None), "read")?;
    Ok(EmitResponse {
        response,
        json: args.tool.json,
    })
}

fn handle_find(args: FindArgs) -> Result<EmitResponse> {
    let paths = if args.path.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.path
    };
    let engine = engine_from_tool(&args.tool)?;
    let mode = parse_mode(&args.tool.mode)?;
    let response = engine.find(
        &args.query,
        &paths,
        mode,
        args.max_files,
        args.max_visible_tokens,
    );
    record_tool_pulse(&response, tokenzero_work_root(None), "find")?;
    Ok(EmitResponse {
        response,
        json: args.tool.json,
    })
}

fn handle_recall(args: RecallArgs) -> Result<EmitResponse> {
    let engine = engine_from_tool(&args.tool)?;
    let mode = parse_mode(&args.tool.mode)?;
    let response = engine.recall(&args.query, args.max_hits, mode, args.max_visible_tokens);
    record_tool_pulse(&response, tokenzero_work_root(None), "recall")?;
    Ok(EmitResponse {
        response,
        json: args.tool.json,
    })
}

fn handle_fetch(args: FetchArgs) -> Result<EmitResponse> {
    let engine = engine_from_tool(&args.tool)?;
    let mode = parse_mode(&args.tool.mode)?;
    let response = engine.fetch(
        &args.url,
        args.ttl_seconds,
        args.fresh,
        mode,
        args.max_visible_tokens,
    );
    record_tool_pulse(&response, tokenzero_work_root(None), "fetch")?;
    Ok(EmitResponse {
        response,
        json: args.tool.json,
    })
}

fn handle_grep(args: FindArgs) -> Result<EmitResponse> {
    let paths = if args.path.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.path
    };
    let engine = engine_from_tool(&args.tool)?;
    let mode = parse_mode(&args.tool.mode)?;
    let response = engine.grep(
        &args.query,
        &paths,
        mode,
        args.max_files,
        args.max_visible_tokens,
    );
    record_tool_pulse(&response, tokenzero_work_root(None), "grep")?;
    Ok(EmitResponse {
        response,
        json: args.tool.json,
    })
}

fn handle_glob(args: GlobArgs) -> Result<EmitResponse> {
    let paths = if args.path.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.path
    };
    let engine = engine_from_tool(&args.tool)?;
    let mode = parse_mode(&args.tool.mode)?;
    let response = engine.glob(
        &args.pattern,
        &paths,
        args.include_hidden,
        mode,
        args.max_files,
        args.max_visible_tokens,
    );
    record_tool_pulse(&response, tokenzero_work_root(None), "glob")?;
    Ok(EmitResponse {
        response,
        json: args.tool.json,
    })
}

fn handle_tree(args: TreeArgs) -> Result<EmitResponse> {
    let paths = if args.path.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.path
    };
    let engine = engine_from_tool(&args.tool)?;
    let mode = parse_mode(&args.tool.mode)?;
    let response = engine.tree(
        &paths,
        args.depth,
        args.include_hidden,
        mode,
        args.max_files,
        args.max_visible_tokens,
    );
    record_tool_pulse(&response, tokenzero_work_root(None), "tree")?;
    Ok(EmitResponse {
        response,
        json: args.tool.json,
    })
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
    let engine = engine_from_tool(&args.tool)?;
    let mode = parse_mode(&args.tool.mode)?;
    let response = engine.edit(
        &args.path,
        &hunks,
        args.create,
        args.dry_run,
        mode,
        args.max_visible_tokens,
    );
    record_tool_pulse(&response, tokenzero_work_root(None), "edit")?;
    Ok(EmitResponse {
        response,
        json: args.tool.json,
    })
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
    let engine = engine_from_tool(&args.tool)?;
    let mode = parse_mode(&args.tool.mode)?;
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
    let response = engine.shell(
        &command,
        Some(normalized_command),
        args.cwd.as_deref(),
        mode,
        args.rewrite.as_deref(),
        args.no_rewrite,
        Some(env),
        stdin_payload.as_deref(),
        None,
    );
    record_tool_pulse(&response, tokenzero_work_root(None), "shell")?;
    Ok(EmitResponse {
        response,
        json: args.tool.json,
    })
}

fn display_command_for_platform(argv: &[String], cwd: Option<&Path>, platform: &str) -> String {
    match plan_command_for_platform(argv, cwd, false, platform) {
        Ok(plan) if plan.execution_mode == ExecutionMode::Shell => argv.join(" "),
        _ => shell_display_command_from_argv_for_platform(argv, platform),
    }
}

fn handle_ingest(args: IngestArgs) -> Result<EmitResponse> {
    let mut text = String::new();
    if args.stdin || args.input.is_none() || args.input.as_deref() == Some(Path::new("-")) {
        std::io::stdin().read_to_string(&mut text)?;
    } else if let Some(input) = &args.input {
        text = fs::read_to_string(input)?;
    }
    let engine = engine_from_tool(&args.tool)?;
    let mode = parse_mode(&args.tool.mode)?;
    let kind = content_type_from_kind(&args.kind, &text, args.input.as_deref());
    let source = args
        .input
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "stdin".to_string());
    let response = engine.ingest(&text, kind, mode, &source);
    record_tool_pulse(&response, tokenzero_work_root(None), "ingest")?;
    Ok(EmitResponse {
        response,
        json: args.tool.json,
    })
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
    let config = EngineConfig {
        allowed_roots: default_allowed_roots(&root),
        cache_path: resolve_recovery_cache_path(&root, args.cache_path.clone()),
        max_visible_tokens: 4000,
        mode: Mode::Exact,
        shell_timeout: default_shell_timeout(),
        mcp_idle_timeout: None,
        ..EngineConfig::for_root(&root)
    };
    let engine = TokenZeroEngine::new(config);
    let (selector, start, end) = expand_selector(&args);
    let response = engine.expand(
        ref_id,
        selector.as_deref(),
        start,
        end,
        args.anchor_kind.as_deref(),
        args.symbol.as_deref(),
    );
    record_tool_pulse(&response, root, "expand")?;
    Ok(EmitResponse {
        response,
        json: args.json,
    })
}

fn emit_rewrite(args: RewriteArgs) -> Result<()> {
    let command = match (&args.command, args.argv.is_empty()) {
        (Some(command), _) => command.clone(),
        (None, false) => {
            let argv = normalize_command(&args.argv);
            display_command_for_platform(&argv, None, tokenzero_runtime::current_platform())
        }
        (None, true) => anyhow::bail!("rewrite requires a command string or `-- <command...>`"),
    };
    let result = rewrite_command(&command, &args.mode, args.mode != "off");
    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        let status = if result.applied {
            "rewrite"
        } else {
            "no-rewrite"
        };
        println!("{status}: {} ({})", result.rewritten_command, result.reason);
    }
    Ok(())
}

fn doctor_report(args: &DoctorArgs) -> serde_json::Value {
    let root = tokenzero_work_root(args.root.clone());
    let mut report = install::doctor(&root, args.cache_path.as_deref());
    if args.runtime {
        let argv = vec!["echo".to_string(), "ok".to_string()];
        let plan = tokenzero_runtime::plan_command(&argv, Some(&root), false).ok();
        report["runtime"] = serde_json::to_value(plan).unwrap_or(json!(null));
    }
    report
}

fn handle_doctor(args: DoctorArgs) -> Result<()> {
    match args.command.clone() {
        Some(DoctorCommand::Capabilities) => emit_doctor_json(install::doctor_capabilities()),
        Some(DoctorCommand::Health) => emit_doctor_health(&args),
        Some(DoctorCommand::Fix) => {
            let root = tokenzero_work_root(args.root.clone());
            emit_doctor_json(install::doctor_fix(
                &root,
                args.cache_path.as_deref(),
                args.dry_run,
            ))
        }
        Some(DoctorCommand::Undo { run_id }) => {
            let root = tokenzero_work_root(args.root.clone());
            emit_doctor_json(install::doctor_undo(&root, &run_id))
        }
        Some(DoctorCommand::Ls) => {
            let root = tokenzero_work_root(args.root.clone());
            emit_doctor_json(install::doctor_ls(&root))
        }
        Some(DoctorCommand::RobotDocs) => {
            print!("{}", install::doctor_robot_docs());
            Ok(())
        }
        Some(DoctorCommand::Explain { finding_id }) => {
            let root = tokenzero_work_root(args.root.clone());
            emit_doctor_json(install::doctor_explain(
                &root,
                args.cache_path.as_deref(),
                &finding_id,
            ))
        }
        Some(DoctorCommand::Diagnose) | None => {
            let root = tokenzero_work_root(args.root.clone());
            if args.fix {
                return emit_doctor_json(install::doctor_fix(
                    &root,
                    args.cache_path.as_deref(),
                    args.dry_run,
                ));
            }
            if let Some(finding_id) = args.explain.as_deref() {
                return emit_doctor_json(install::doctor_explain(
                    &root,
                    args.cache_path.as_deref(),
                    finding_id,
                ));
            }
            if args.robot_triage {
                return emit_doctor_json(install::doctor_robot_triage(
                    &root,
                    args.cache_path.as_deref(),
                ));
            }
            emit_doctor_json(doctor_report(&args))
        }
    }
}

fn emit_doctor_health(args: &DoctorArgs) -> Result<()> {
    let report = doctor_report(args);
    let ok = report["ok"].as_bool().unwrap_or(false);
    let status = report["status"].as_str().unwrap_or("blocked");
    let finding_count = report["finding_count"].as_u64().unwrap_or(0);
    let blocking = report["summary"]["blocking_findings"].as_u64().unwrap_or(0);
    let info = report["summary"]["informational_findings"]
        .as_u64()
        .unwrap_or(0);
    let exit_code = doctor_exit_code(&report);
    let line = format!(
        "{status} tokenzero={} doctor={} findings={finding_count} blocking={blocking} info={info}",
        env!("CARGO_PKG_VERSION"),
        report["doctor_version"]
            .as_str()
            .unwrap_or(env!("CARGO_PKG_VERSION"))
    );
    if args.json {
        emit_doctor_json(json!({
            "schema_version": "tokenzero.doctor.health.v1",
            "status": status,
            "ok": ok,
            "line": line,
            "finding_count": finding_count,
            "blocking_findings": blocking,
            "informational_findings": info,
            "exit_code": exit_code
        }))
    } else {
        println!("{line}");
        if exit_code != 0 {
            std::process::exit(exit_code);
        }
        Ok(())
    }
}

fn emit_doctor_json(value: serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    let exit_code = doctor_exit_code(&value);
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
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
    let report = report_for_path(&default_ledger_path(&root))?;
    Ok(serde_json::to_value(report)?)
}

fn handle_pulse(args: PulseArgs) -> Result<()> {
    let root = tokenzero_work_root(args.root);
    let ledger_path = default_ledger_path(&root);
    match args.command {
        Some(PulseCommand::Sync) => {
            emit_pulse_result("pulse sync", sync_jsonl_to_sqlite(&ledger_path), args.json)?;
        }
        Some(PulseCommand::Doctor) => {
            emit_pulse_result("pulse doctor", doctor_jsonl_sqlite(&ledger_path), args.json)?;
        }
        Some(PulseCommand::ExportJsonl(export_args)) => {
            emit_pulse_result(
                "pulse export-jsonl",
                export_jsonl(&ledger_path, &export_args.output),
                args.json,
            )?;
        }
        Some(PulseCommand::ImportJsonl(import_args)) => {
            emit_pulse_result(
                "pulse import-jsonl",
                import_jsonl(&import_args.input, &ledger_path),
                args.json,
            )?;
        }
        Some(PulseCommand::Stats) | None => {
            let _ = sync_jsonl_to_sqlite(&ledger_path);
            let report = report_for_path(&ledger_path)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", tokenzero_pulse::render_text(&report));
            }
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
            let message = err.to_string();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "schema_version": "tokenzero.pulse.error.v1",
                    "ok": false,
                    "status": "error",
                    "operation": operation,
                    "error_kind": io_error_kind_name(kind),
                    "retryable": kind == std::io::ErrorKind::WouldBlock,
                    "error": message,
                    "exit_code": 1
                }))?
            );
            std::process::exit(1);
        }
        Err(err) => Err(err.into()),
    }
}

fn io_error_kind_name(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::NotFound => "not_found",
        std::io::ErrorKind::PermissionDenied => "permission_denied",
        std::io::ErrorKind::ConnectionRefused => "connection_refused",
        std::io::ErrorKind::ConnectionReset => "connection_reset",
        std::io::ErrorKind::ConnectionAborted => "connection_aborted",
        std::io::ErrorKind::NotConnected => "not_connected",
        std::io::ErrorKind::AddrInUse => "addr_in_use",
        std::io::ErrorKind::AddrNotAvailable => "addr_not_available",
        std::io::ErrorKind::BrokenPipe => "broken_pipe",
        std::io::ErrorKind::AlreadyExists => "already_exists",
        std::io::ErrorKind::WouldBlock => "would_block",
        std::io::ErrorKind::InvalidInput => "invalid_input",
        std::io::ErrorKind::InvalidData => "invalid_data",
        std::io::ErrorKind::TimedOut => "timed_out",
        std::io::ErrorKind::WriteZero => "write_zero",
        std::io::ErrorKind::Interrupted => "interrupted",
        std::io::ErrorKind::Unsupported => "unsupported",
        std::io::ErrorKind::UnexpectedEof => "unexpected_eof",
        std::io::ErrorKind::OutOfMemory => "out_of_memory",
        _ => "other",
    }
}

fn handle_cache(args: CacheArgs) -> Result<()> {
    match args.command {
        CacheCommand::Status(args) => {
            let engine = engine_from_common(&args);
            emit_with_json(engine.mem(), args.json)?;
        }
        CacheCommand::Prune(args) => {
            let root = tokenzero_work_root(args.root);
            let cache = resolve_recovery_cache_path(&root, args.cache_path);
            let dry_run = !args.apply;
            let mut store = tokenzero_recovery::RecoveryStore::new(Some(cache.clone()));
            let mut report = store.prune_stale(dry_run)?;
            report["maintenance"] = tokenzero_mcp::cache_maintenance(&cache, dry_run);
            emit_value(report, args.json)?;
        }
    }
    Ok(())
}

fn handle_cache_pack(args: CachePackArgs) -> Result<()> {
    let root = tokenzero_work_root(args.root.clone());
    let engine = TokenZeroEngine::new(EngineConfig {
        allowed_roots: default_allowed_roots(&root),
        cache_path: resolve_recovery_cache_path(&root, args.cache_path.clone()),
        max_visible_tokens: 4000,
        mode: Mode::Structured,
        shell_timeout: default_shell_timeout(),
        mcp_idle_timeout: None,
        ..EngineConfig::for_root(&root)
    });
    emit_with_json(engine.cache_pack(&args.scope), args.json)
}

fn handle_bench(args: BenchArgs) -> Result<()> {
    match args.command {
        BenchCommand::Competitors(args) => {
            let as_json = args.json;
            emit_value(run_bench_competitors(args)?, as_json)?;
        }
    }
    Ok(())
}

fn handle_install(args: InstallArgs) -> Result<()> {
    let agents = install_agents(&args.agents, args.grok)?;
    let capabilities = install_capabilities(&args);
    let root = install_root(args.root.clone(), args.global);
    if let Some(id) = args.rollback {
        emit_value(install::rollback(&root, &id)?, args.json)?;
    } else if args.apply {
        emit_value(
            install::apply_for_agents(&root, args.global, &capabilities, &agents)?,
            args.json,
        )?;
    } else {
        emit_value(
            install::plan_for_agents(&root, args.global, &capabilities, &agents),
            args.json,
        )?;
    }
    Ok(())
}

fn handle_init(args: InitArgs) -> Result<()> {
    let _plan_requested = args.plan;
    let agents = install_agents(&args.agents, false)?;
    let capabilities = init_capabilities(&args);
    let root = install_root(args.root.clone(), args.global);
    if args.apply {
        emit_value(
            install::apply_for_agents(&root, args.global, &capabilities, &agents)?,
            args.json,
        )?;
    } else {
        emit_value(
            install::plan_for_agents(&root, args.global, &capabilities, &agents),
            args.json,
        )?;
    }
    Ok(())
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
    let path_env = std::env::var("PATH").ok();
    let detected = install::detect_present_agents(&home, path_env.as_deref());
    let supported: Vec<&str> = detected
        .iter()
        .filter(|agent| agent.supported)
        .map(|agent| agent.agent.as_str())
        .collect();
    let next_step = if supported.is_empty() {
        "no supported harnesses detected; docs/routing.md covers manual adapters".to_string()
    } else {
        format!(
            "tokenzero install --global --apply --hooks{}",
            supported
                .iter()
                .map(|agent| format!(" --agent {agent}"))
                .collect::<String>()
        )
    };
    emit_value(
        json!({
            "schema_version": "tokenzero.clients.v1",
            "command": "clients scan",
            "status": "ok",
            "home": home.display().to_string(),
            "detected": detected,
            "unsupported_note": "supported=false entries need the manual adapter snippets in docs/routing.md",
            "next_step": next_step,
        }),
        args.json,
    )
}

fn handle_client_status(args: ClientStatusArgs) -> Result<()> {
    let agents = install_agents(&args.agents, args.grok)?;
    let root = install_root(args.root.clone(), true);
    emit_value(client_status_report(&root, &agents, "detect")?, args.json)
}

fn handle_clients_plan(args: ClientsPlanArgs) -> Result<()> {
    let profile = clients_profile(&args.profile)?;
    let agents = install_agents(&args.agents, args.grok)?;
    let root = install_root(args.root.clone(), true);
    let capabilities = clients_capabilities(&profile);
    let plan = install::plan_for_agents(&root, true, &capabilities, &agents);
    let mut value = serde_json::to_value(plan)?;
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "schema_version".to_string(),
            json!("tokenzero.clients.plan.v1"),
        );
        object.insert("command".to_string(), json!("clients plan"));
        object.insert("profile".to_string(), json!(profile));
        object.insert("root".to_string(), json!(root.display().to_string()));
        object.insert("agents".to_string(), json!(clients_agent_labels(&agents)));
    }
    emit_value(value, args.json)
}

fn handle_clients_doctor(args: ClientStatusArgs) -> Result<()> {
    let agents = install_agents(&args.agents, args.grok)?;
    let root = install_root(args.root.clone(), true);
    let mut report = client_status_report(&root, &agents, "doctor")?;
    let status = report
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let findings = match status {
        "installed" => vec![json!({
            "id": "tz-clients-installed",
            "severity": "info",
            "summary": "TokenZero client integration surfaces are present",
            "next_step": "Run tokenzero doctor --json for runtime health."
        })],
        "mixed" => vec![json!({
            "id": "tz-clients-mixed",
            "severity": "warning",
            "summary": "Some TokenZero client integration surfaces are present and some are missing",
            "next_step": "Run tokenzero clients plan --profile standard --json, review the plan, then use tokenzero install --global --apply --mcp --json if approved."
        })],
        _ => vec![json!({
            "id": "tz-clients-missing",
            "severity": "info",
            "summary": "No TokenZero client integration surfaces were detected at the planned target paths",
            "next_step": "Run tokenzero clients plan --profile standard --json to inspect the read-only integration plan."
        })],
    };
    if let Some(object) = report.as_object_mut() {
        object.insert("findings".to_string(), json!(findings));
    }
    emit_value(report, args.json)
}

fn handle_clients_rollback(args: ClientsRollbackArgs) -> Result<()> {
    let root = install_root(args.root.clone(), true);
    emit_value(install::rollback(&root, &args.id)?, args.json)
}

fn handle_capabilities(args: CapabilitiesArgs) -> Result<()> {
    emit_value(capabilities_json(), args.json)
}

fn handle_robot_docs(args: RobotDocsArgs) {
    match args.command {
        RobotDocsCommand::Guide => {
            print!("{}", robot_docs_guide());
        }
        RobotDocsCommand::Commands => {
            print!("{}", agent_surfaces::robot_docs_commands());
        }
        RobotDocsCommand::Examples => {
            print!("{}", agent_surfaces::robot_docs_examples());
        }
    }
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
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"platform": args.platform, "argv": argv, "command": quoted})
            )?
        );
    } else {
        println!("{quoted}");
    }
    Ok(())
}

fn engine_from_tool(args: &ToolArgs) -> Result<TokenZeroEngine> {
    let root = tokenzero_work_root(None);
    Ok(TokenZeroEngine::new(EngineConfig {
        allowed_roots: allowed_roots_for_workspace(&root, &args.allowed_root),
        cache_path: resolve_recovery_cache_path(&root, args.cache_path.clone()),
        max_visible_tokens: args.budget.unwrap_or(4000),
        mode: parse_mode(&args.mode)?,
        shell_timeout: shell_timeout_from_secs(args.timeout_seconds),
        mcp_idle_timeout: None,
        ..EngineConfig::for_root(&root)
    }))
}

fn engine_from_common(args: &CommonArgs) -> TokenZeroEngine {
    let root = tokenzero_work_root(args.root.clone());
    TokenZeroEngine::new(EngineConfig {
        allowed_roots: default_allowed_roots(&root),
        cache_path: resolve_recovery_cache_path(&root, args.cache_path.clone()),
        max_visible_tokens: 4000,
        mode: Mode::Auto,
        shell_timeout: default_shell_timeout(),
        mcp_idle_timeout: None,
        ..EngineConfig::for_root(&root)
    })
}

fn engine_config_for_mcp(args: &McpServerArgs) -> Result<EngineConfig> {
    let root = tokenzero_work_root(None);
    Ok(EngineConfig {
        allowed_roots: if args.allowed_root.is_empty() {
            default_allowed_roots(&root)
        } else {
            args.allowed_root.clone()
        },
        cache_path: resolve_recovery_cache_path(&root, args.cache_path.clone()),
        max_visible_tokens: 4000,
        mode: parse_mode(&args.default_mode)?,
        shell_timeout: shell_timeout_from_secs(args.shell_timeout_seconds),
        mcp_idle_timeout: mcp_idle_timeout_from_secs(args.idle_timeout_seconds),
        ..EngineConfig::for_root(&root)
    })
}

/// Rebuilds the mcp-server invocation for the supervised inner child:
/// same configuration, no --supervise (one supervisor only), and idle exit
/// pinned off because the supervisor owns the session lifecycle.
fn supervised_child_args(args: &McpServerArgs) -> Vec<std::ffi::OsString> {
    let mut child_args: Vec<std::ffi::OsString> = vec!["mcp-server".into()];
    for root in &args.allowed_root {
        child_args.push("--allowed-root".into());
        child_args.push(root.clone().into_os_string());
    }
    if let Some(cache_path) = &args.cache_path {
        child_args.push("--cache-path".into());
        child_args.push(cache_path.clone().into_os_string());
    }
    child_args.push("--default-mode".into());
    child_args.push(args.default_mode.clone().into());
    if let Some(seconds) = args.shell_timeout_seconds {
        child_args.push("--shell-timeout-seconds".into());
        child_args.push(seconds.to_string().into());
    }
    child_args.push("--idle-timeout-seconds".into());
    child_args.push("0".into());
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
    if let Some(root) = root {
        return root;
    }
    if global {
        if let Some(home) = platform_home_dir() {
            return home;
        }
    }
    tokenzero_work_root(None)
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
    let parts = if command.first().map(String::as_str) == Some("--") {
        &command[1..]
    } else {
        command
    };
    if parts.len() == 1
        && contains_platform_shell_syntax(&parts[0], tokenzero_runtime::current_platform())
    {
        parts.to_vec()
    } else if parts.len() == 1 {
        split_command_string(&parts[0])
    } else {
        parts.to_vec()
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

fn expand_selector(args: &ExpandArgs) -> (Option<String>, Option<usize>, Option<usize>) {
    let mut selector = args.selector.clone();
    if args.raw || selector.is_none() {
        selector = Some("raw".to_string());
    }
    if args.summary {
        selector = Some("summary".to_string());
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
        let line = line
            .trim()
            .trim_start_matches('L')
            .parse::<usize>()
            .unwrap_or(1);
        let radius = radius.parse::<usize>().unwrap_or(3);
        start = Some(line.saturating_sub(radius).max(1));
        end = Some(line + radius);
    }
    (selector, start, end)
}

fn install_capabilities(args: &InstallArgs) -> Vec<String> {
    let mut caps = Vec::new();
    if args.mcp || args.grok || !args.agents.is_empty() {
        caps.push("mcp".to_string());
    }
    if args.shell {
        caps.push("shell".to_string());
    }
    if args.instructions {
        caps.push("instructions".to_string());
    }
    if args.cli {
        caps.push("cli".to_string());
    }
    if args.hooks {
        caps.push("hooks".to_string());
    }
    if args.shims {
        caps.push("shim".to_string());
    }
    caps
}

fn init_capabilities(args: &InitArgs) -> Vec<String> {
    let mut caps = Vec::new();
    if args.mcp || !args.agents.is_empty() {
        caps.push("mcp".to_string());
    }
    if args.shell {
        caps.push("shell".to_string());
    }
    if args.instructions {
        caps.push("instructions".to_string());
    }
    if args.cli {
        caps.push("cli".to_string());
    }
    if args.hooks {
        caps.push("hooks".to_string());
    }
    if args.shims {
        caps.push("shim".to_string());
    }
    if caps.is_empty() {
        caps.push("mcp".to_string());
    }
    caps
}

fn clients_profile(raw: &str) -> Result<String> {
    let profile = raw.trim().to_ascii_lowercase();
    match profile.as_str() {
        "standard" | "" => Ok("standard".to_string()),
        other => anyhow::bail!(
            "unsupported clients profile '{other}'; currently only 'standard' is supported"
        ),
    }
}

fn clients_capabilities(_profile: &str) -> Vec<String> {
    // The standard profile wires MCP plus the Claude Code hook. The PATH shim
    // layer stays opt-in (`tokenzero install --shims`): it mutates PATH-visible
    // binaries, which is a bigger footprint than client config merges.
    vec!["mcp".to_string(), "hooks".to_string()]
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
    let capabilities = clients_capabilities("standard");
    let plan = install::plan_for_agents(root, true, &capabilities, agents);
    let surfaces: Vec<serde_json::Value> = plan
        .writes
        .iter()
        .map(|write| client_surface_status(write, root))
        .collect::<Result<Vec<_>>>()?;
    let installed = surfaces
        .iter()
        .filter(|surface| surface["state"] == "installed")
        .count();
    let mixed = surfaces
        .iter()
        .filter(|surface| surface["state"] == "mixed")
        .count();
    let missing = surfaces
        .iter()
        .filter(|surface| surface["state"] == "missing")
        .count();
    let status = if installed > 0 && missing == 0 && mixed == 0 {
        "installed"
    } else if installed > 0 || mixed > 0 {
        "mixed"
    } else {
        "missing"
    };
    Ok(json!({
        "schema_version": "tokenzero.clients.v1",
        "status": status,
        "ok": true,
        "command": format!("clients {command}"),
        "root": root.display().to_string(),
        "global": true,
        "profile": "standard",
        "agents": clients_agent_labels(agents),
        "summary": {
            "installed": installed,
            "mixed": mixed,
            "missing": missing,
            "total": surfaces.len(),
            "raw_bypass_risk": status != "installed"
        },
        "surfaces": surfaces,
        "next_action": if status == "installed" {
            "Run tokenzero doctor --json to verify runtime health."
        } else {
            "Run tokenzero clients plan --profile standard --json to review the read-only integration plan."
        }
    }))
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

fn push_agent(agents: &mut Vec<String>, raw: &str) -> Result<()> {
    let normalized = raw.trim().to_ascii_lowercase().replace(['_', ' '], "-");
    let agent = match normalized.as_str() {
        "all" => return Ok(()),
        "claude" | "claude-code" | "claude-desktop" => "claude",
        "codex" => "codex",
        "cursor" => "cursor",
        "droid" | "factory-droid" => "droid",
        "factory" => "factory",
        "gemini" => "gemini",
        "grok" => "grok",
        "opencode" | "open-code" => "opencode",
        "" => anyhow::bail!("--agent requires a non-empty agent name"),
        other => anyhow::bail!(
            "unsupported agent '{other}'; expected one of claude, codex, cursor, droid, factory, gemini, grok, opencode, or all"
        ),
    };
    if !agents.iter().any(|existing| existing == agent) {
        agents.push(agent.to_string());
    }
    Ok(())
}

struct EmitResponse {
    response: ToolResponse,
    json: bool,
}

fn emit(value: EmitResponse) -> Result<()> {
    emit_with_json(value.response, value.json)
}

fn emit_with_json(response: ToolResponse, as_json: bool) -> Result<()> {
    let exit_error = response.status == "error";
    if as_json {
        println!("{}", cli_json(&response));
    } else if response.tool == "expand" && response.status == "ok" {
        if let Some(visible) = &response.visible {
            // Exact recovery: emit recovered bytes verbatim (no forced
            // trailing newline). Fixes F-004 (non-newline-terminated files).
            print!("{}", visible.text);
        }
    } else {
        print!("{}", render_text(&response));
    }
    if exit_error {
        std::process::exit(1);
    }
    if !as_json {
        if let Some(code) = child_failure_exit_code(&response) {
            std::process::exit(code);
        }
    }
    Ok(())
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
        // Masked pipeline: the shell itself reported 0; mirror sh semantics.
        Some(0) => None,
        Some(code) => Some(code.clamp(1, 255) as i32),
        // Timeout or signal without an exit code.
        None => Some(1),
    }
}

fn emit_value<T: serde::Serialize>(value: T, _as_json: bool) -> Result<()> {
    let json_value = serde_json::to_value(value)?;
    println!("{}", serde_json::to_string_pretty(&json_value)?);
    if json_value.get("ok") == Some(&json!(false))
        || json_value.get("status") == Some(&json!("blocked"))
    {
        std::process::exit(1);
    }
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

#[allow(dead_code)]
fn response_from_text(tool: &str, text: String) -> ToolResponse {
    ToolResponse::ok(
        tool,
        Mode::Hybrid,
        text.clone(),
        Vec::new(),
        Accounting {
            raw_tokens: count_tokens(&text),
            visible_tokens: count_tokens(&text),
            recovery_tokens: 0,
            exact_ref_tokens: Some(0),
        },
    )
}

mod audits;

use audits::bench::*;
use audits::os_reach::*;
use audits::recovery::*;
use audits::release::*;

#[cfg(test)]
mod tests;
