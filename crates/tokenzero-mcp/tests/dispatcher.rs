//! Typed domain dispatcher identity and dependency tests (tokenzero-irx9.2).

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use tokenzero_core::operation_abi::{DomainErrorKind, all_operations};
use tokenzero_mcp::{
    DispatchSurface, EngineConfig, TokenZeroEngine, dispatch_cli, dispatch_codemode_method,
    dispatch_count, dispatch_mcp_tool, dispatch_operation, dispatch_raw_worker, domain_fastmcp_ops,
    is_domain_operation, last_dispatch_profile,
};

fn engine_for(root: &Path) -> TokenZeroEngine {
    let mut config = EngineConfig::for_root(root);
    config.session_dedup = false;
    config.diff_reads = false;
    config.fetch_enabled = false;
    TokenZeroEngine::new(config)
}

fn minimal_args(op: &str) -> Value {
    match op {
        "tz_read" | "read" => json!({"path": "note.txt"}),
        "tz_find" | "tz_grep" => json!({"query": "dispatcher", "path": "."}),
        "tz_recall" => json!({"query": "dispatcher"}),
        "tz_glob" => json!({"pattern": "*.txt", "path": "."}),
        "tz_tree" => json!({"path": ".", "depth": 1}),
        "tz_edit" => json!({
            "path": "note.txt",
            "edits": [{"find": "dispatcher-identity", "replace": "dispatcher-identity"}],
            "dry_run": true
        }),
        "tz_shell" => json!({"command": "true"}),
        "tz_ingest" => json!({"text": "hello-from-dispatcher"}),
        "tz_expand" => json!({"ref": "tz://deadbeef"}),
        "tz_mem" => json!({}),
        "tz_cache_pack" => json!({"scope": "agent"}),
        "tz_rewrite" => json!({"command": "echo hi"}),
        "tz_discover" => json!({}),
        "tz_report_tool_issue" => json!({
            "tool": "zero_execute",
            "summary": "dispatcher identity probe"
        }),
        "tz_batch" => json!({
            "ops": [{"tool": "tz_mem", "args": {}}]
        }),
        "tz_fetch" => json!({"url": "https://example.invalid/"}),
        _ => json!({}),
    }
}

fn domain_sources() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let names = [
        "dispatcher.rs",
        "shell_hooks.rs",
        "engine_common.rs",
        "engine_edit.rs",
        "engine_expand.rs",
        "engine_fetch.rs",
        "engine_find.rs",
        "engine_ingest.rs",
        "engine_misc.rs",
        "engine_read.rs",
        "engine_search.rs",
        "engine_session.rs",
        "engine_shell.rs",
    ];
    names.into_iter().map(|n| root.join(n)).collect()
}

#[test]
fn domain_modules_do_not_import_surface_layers() {
    // Code-level dependency rule: domain engine modules must not import
    // FastMCP, MCP JSON-RPC, or CodeMode sandbox modules.
    // Exact module/crate imports only (avoid false positives on names like
    // domain_fastmcp_ops / fastmcp_tool exposure flags).
    let forbidden_substrings = [
        "crate::codemode",
        "crate::fastmcp_mode",
        "crate::jsonrpc",
        "fastmcp_rust::",
        "use fastmcp",
        "use rquickjs",
        "rquickjs::",
    ];
    let mut violations = Vec::new();
    for path in domain_sources() {
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (lineno, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            for pat in forbidden_substrings {
                if trimmed.contains(pat) {
                    violations.push(format!("{}:{}: {line}", path.display(), lineno + 1));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "domain modules import forbidden surface layers:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_fastmcp_codemode_cross_adapter_calls() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let fastmcp = fs::read_to_string(root.join("fastmcp_mode.rs")).unwrap();
    assert!(
        !fastmcp.contains("crate::codemode") && !fastmcp.contains("execute_codemode"),
        "FastMCP must not call CodeMode modules"
    );
    assert!(
        fastmcp.contains("call_tool_fastmcp") || fastmcp.contains("dispatch_mcp_tool"),
        "FastMCP should use shared tool/dispatch path"
    );

    let connector = fs::read_to_string(root.join("codemode/exec.rs")).unwrap();
    assert!(
        !connector.contains("crate::fastmcp_mode") && !connector.contains("run_fastmcp"),
        "CodeMode must not call FastMCP"
    );
    assert!(
        connector.contains("dispatch_codemode_method")
            || connector.contains("crate::dispatcher::dispatch"),
        "CodeMode domain path should use the typed dispatcher"
    );
}

#[test]
fn one_operation_same_dispatcher_from_all_adapters() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("note.txt"), b"dispatcher-identity").unwrap();

    let mcp = engine_for(root.path());
    let raw = engine_for(root.path());
    let cli = engine_for(root.path());
    let cm = engine_for(root.path());

    let before = dispatch_count();
    let args = json!({"path": root.path().join("note.txt").display().to_string()});

    let mcp_out = dispatch_mcp_tool(&mcp, "tz_read", &args).expect("mcp");
    let raw_out = dispatch_raw_worker(&raw, "tz_read", &args);
    let cli_out = dispatch_cli(&cli, "tz_read", &args);
    let cm_out = dispatch_codemode_method(&cm, "zero.read", &args).expect("cm");

    assert!(mcp_out.is_ok(), "mcp: {:?}", mcp_out.tool_domain_error());
    assert!(raw_out.is_ok(), "raw: {:?}", raw_out.tool_domain_error());
    assert!(cli_out.is_ok(), "cli: {:?}", cli_out.tool_domain_error());
    assert!(cm_out.is_ok(), "cm: {:?}", cm_out.tool_domain_error());

    let normalize = |out: &tokenzero_mcp::DispatchOutcome| {
        let resp = out.tool_response.as_ref().expect("tool response");
        (
            resp.status.clone(),
            resp.tool.clone(),
            resp.refs
                .iter()
                .map(|r| {
                    // Content-addressed refs must agree; strip only transport noise.
                    r.ref_id.clone()
                })
                .collect::<Vec<_>>(),
            resp.visible.as_ref().map(|v| v.text.clone()),
            resp.error.as_ref().map(|e| (e.code.clone(), e.message.clone())),
        )
    };

    let m = normalize(&mcp_out);
    assert_eq!(normalize(&raw_out), m, "raw vs mcp");
    assert_eq!(normalize(&cli_out), m, "cli vs mcp");
    assert_eq!(normalize(&cm_out), m, "codemode vs mcp");

    assert!(dispatch_count() >= before + 4);
    let profile = last_dispatch_profile();
    assert!(profile.wall_ns > 0);
    // Dispatcher overhead is recorded separately from kernel work for benchmarks.
    assert!(profile.dispatcher_overhead_ns < profile.wall_ns || profile.kernel_ns == 0);
}

#[test]
fn differential_registry_domain_ops_raw_mcp_cli() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("note.txt"), b"dispatcher-identity").unwrap();

    let ops = domain_fastmcp_ops();
    assert!(
        ops.contains(&"tz_read") && ops.contains(&"tz_mem"),
        "expected core domain ops in list: {ops:?}"
    );

    for op in ops {
        // Skip ops that need network, pre-existing refs, or mutating dry-run edge cases.
        if matches!(op, "tz_fetch" | "tz_expand" | "tz_report_tool_issue") {
            continue;
        }
        let args = minimal_args(op);
        // Rebase path-bearing ops onto temp root.
        let args = rebase_paths(args, root.path());

        let raw_e = engine_for(root.path());
        let mcp_e = engine_for(root.path());
        let cli_e = engine_for(root.path());

        let raw = dispatch_operation(&raw_e, DispatchSurface::RawWorker, op, &args);
        let mcp = dispatch_mcp_tool(&mcp_e, op, &args).expect("mcp dispatch");
        let cli = dispatch_cli(&cli_e, op, &args);

        let norm = |o: &tokenzero_mcp::DispatchOutcome| {
            (
                o.op.clone(),
                o.is_ok(),
                o.tool_response.as_ref().map(|r| r.status.clone()),
                o.tool_response
                    .as_ref()
                    .and_then(|r| r.error.as_ref())
                    .map(|e| e.code.clone()),
                o.domain_error
                    .as_ref()
                    .map(|e| e.kind.as_str().to_string()),
            )
        };
        assert_eq!(norm(&raw), norm(&mcp), "raw vs mcp for {op}");
        assert_eq!(norm(&raw), norm(&cli), "raw vs cli for {op}");
    }
}

fn rebase_paths(mut args: Value, root: &Path) -> Value {
    if let Some(obj) = args.as_object_mut() {
        if let Some(path) = obj.get("path").cloned() {
            match path {
                Value::String(s) if !Path::new(&s).is_absolute() => {
                    obj.insert(
                        "path".into(),
                        json!(root.join(s).display().to_string()),
                    );
                }
                Value::Array(items) => {
                    let mapped: Vec<Value> = items
                        .into_iter()
                        .map(|item| match item {
                            Value::String(s) if !Path::new(&s).is_absolute() => {
                                json!(root.join(s).display().to_string())
                            }
                            other => other,
                        })
                        .collect();
                    obj.insert("path".into(), Value::Array(mapped));
                }
                _ => {}
            }
        }
        if let Some(Value::String(cwd)) = obj.get("cwd").cloned() {
            if !Path::new(&cwd).is_absolute() {
                obj.insert("cwd".into(), json!(root.join(cwd).display().to_string()));
            }
        }
    }
    args
}

#[test]
fn differential_policy_failure_agrees_across_surfaces() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("note.txt"), b"ok").unwrap();
    let _outside = root.path().join("..").join("escape-target.txt");
    // Use an absolute path outside allowed roots.
    let outside = fs::canonicalize(root.path())
        .unwrap()
        .parent()
        .unwrap()
        .join("tokenzero-dispatcher-escape.txt");
    let _ = fs::write(&outside, b"secret");

    let args = json!({"path": outside.display().to_string()});
    let raw_e = engine_for(root.path());
    let mcp_e = engine_for(root.path());
    let cm_e = engine_for(root.path());

    let raw = dispatch_raw_worker(&raw_e, "tz_read", &args);
    let mcp = dispatch_mcp_tool(&mcp_e, "tz_read", &args).unwrap();
    let cm = dispatch_codemode_method(&cm_e, "zero.read", &args).unwrap();

    for out in [&raw, &mcp, &cm] {
        assert!(!out.is_ok(), "escape should fail: {:?}", out.result);
        // Prefer typed policy/validation over success.
        let err = out.tool_domain_error().or_else(|| {
            out.tool_response.as_ref().and_then(|r| {
                r.error.as_ref().map(|e| {
                    tokenzero_core::operation_abi::DomainError::new(
                        DomainErrorKind::Policy,
                        e.message.clone(),
                    )
                })
            })
        });
        assert!(err.is_some(), "expected domain/tool error");
    }

    let code = |o: &tokenzero_mcp::DispatchOutcome| {
        o.tool_response
            .as_ref()
            .and_then(|r| r.error.as_ref())
            .map(|e| e.code.clone())
    };
    assert_eq!(code(&raw), code(&mcp));
    assert_eq!(code(&raw), code(&cm));
    let _ = fs::remove_file(&outside);
}

#[test]
fn transport_control_tools_are_not_domain_ops() {
    assert!(!is_domain_operation("tz_execute_code"));
    assert!(!is_domain_operation("tz_codemode_search"));
    assert!(!is_domain_operation("codemode.limits"));
    assert!(is_domain_operation("tz_read"));
    assert!(is_domain_operation("zero.read"));

    let root = tempfile::tempdir().unwrap();
    let engine = engine_for(root.path());
    let err = dispatch_mcp_tool(&engine, "tz_execute_code", &json!({"plan": "1"}))
        .expect_err("control tool");
    assert_eq!(err.kind, DomainErrorKind::Validation);
}

#[test]
fn dispatcher_records_profile_for_benchmark_subtraction() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("note.txt"), b"profile").unwrap();
    let engine = engine_for(root.path());
    let before = dispatch_count();
    let _ = dispatch_raw_worker(
        &engine,
        "tz_read",
        &json!({"path": root.path().join("note.txt").display().to_string()}),
    );
    assert!(dispatch_count() > before);
    let p = last_dispatch_profile();
    assert_eq!(p.surface, DispatchSurface::RawWorker as u8);
    assert!(p.wall_ns >= p.kernel_ns);
}

#[test]
fn every_fastmcp_domain_op_is_dispatchable() {
    for op in all_operations() {
        if op.exposure.fastmcp_tool && is_domain_operation(op.name) {
            assert!(
                domain_fastmcp_ops().contains(&op.name),
                "missing from domain_fastmcp_ops: {}",
                op.name
            );
        }
    }
}
