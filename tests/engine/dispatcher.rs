//! Typed domain dispatcher identity and dependency tests (tokenzero-irx9.2).

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use tokenzero_core::operation_abi::{DomainErrorKind, all_operations};
use tokenzero_engine::{
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
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    walk(&root, &mut out);
    out
}

#[test]
fn engine_crate_does_not_depend_on_surface_layers() {
    // Cargo-level dependency direction: tokenzero-engine must not link FastMCP,
    // rquickjs/CodeMode sandbox, or MCP transport crates.
    let manifest =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    for forbidden in ["fastmcp-rust", "fastmcp_rust", "rquickjs", "tokenzero-mcp"] {
        assert!(
            !manifest.contains(forbidden),
            "tokenzero-engine Cargo.toml must not depend on {forbidden}"
        );
    }

    // Source-level: no imports of surface modules.
    let forbidden_substrings = [
        "crate::codemode::",
        "crate::fastmcp_mode",
        "crate::jsonrpc",
        "tokenzero_mcp::",
        "fastmcp_rust::",
        "use fastmcp",
        "use rquickjs",
        "rquickjs::",
    ];
    let mut violations = Vec::new();
    for path in domain_sources() {
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
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
        "engine sources import forbidden surface layers:
{}",
        violations.join(
            "
"
        )
    );
}

#[test]
fn compatibility_carrier_and_raw_worker_do_not_embed_a_planner() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fastmcp =
        fs::read_to_string(root.join("../tokenzero-mcp-compat/src/fastmcp_mode.rs")).unwrap();
    assert!(
        !fastmcp.contains("execute_codemode") && !fastmcp.contains("rquickjs"),
        "classic FastMCP carrier must not execute plans"
    );
    assert!(
        fastmcp.contains("call_tool_fastmcp") || fastmcp.contains("dispatch_mcp_tool"),
        "classic FastMCP should use the shared tool/dispatch path"
    );

    let worker = fs::read_to_string(root.join("../tokenzero-codemode/src/main.rs")).unwrap();
    for forbidden in ["rquickjs", "fastmcp", "tokenzero_mcp", "zero_codemode"] {
        assert!(
            !worker.contains(forbidden),
            "planner-free raw worker contains forbidden host marker {forbidden}"
        );
    }
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

    let normalize = |out: &tokenzero_engine::DispatchOutcome| {
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
            resp.error
                .as_ref()
                .map(|e| (e.code.clone(), e.message.clone())),
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

        let norm = |o: &tokenzero_engine::DispatchOutcome| {
            (
                o.op.clone(),
                o.is_ok(),
                o.tool_response.as_ref().map(|r| r.status.clone()),
                o.tool_response
                    .as_ref()
                    .and_then(|r| r.error.as_ref())
                    .map(|e| e.code.clone()),
                o.domain_error.as_ref().map(|e| e.kind.as_str().to_string()),
            )
        };
        assert_eq!(norm(&raw), norm(&mcp), "raw vs mcp for {op}");
        assert_eq!(norm(&raw), norm(&cli), "raw vs cli for {op}");
    }
}

#[test]
fn batch_error_taxonomy_matches_cli_and_mcp() {
    let root = tempfile::tempdir().unwrap();
    let cli_engine = engine_for(root.path());
    let mcp_engine = engine_for(root.path());

    let assert_parity =
        |args: &Value, expected_kind: DomainErrorKind, expected_code: Option<&str>| {
            let cli = dispatch_cli(&cli_engine, "tz_batch", args);
            let mcp = dispatch_mcp_tool(&mcp_engine, "tz_batch", args).expect("mcp dispatch");
            assert_eq!(
                cli.tool_domain_error().map(|error| error.kind),
                Some(expected_kind),
                "cli taxonomy for {args}"
            );
            assert_eq!(
                mcp.tool_domain_error().map(|error| error.kind),
                Some(expected_kind),
                "mcp taxonomy for {args}"
            );
            assert_eq!(
                cli.tool_response
                    .as_ref()
                    .and_then(|response| response.error.as_ref())
                    .map(|error| error.code.as_str()),
                expected_code,
                "cli code for {args}"
            );
            assert_eq!(
                mcp.tool_response
                    .as_ref()
                    .and_then(|response| response.error.as_ref())
                    .map(|error| error.code.as_str()),
                expected_code,
                "mcp code for {args}"
            );
        };

    assert_parity(&json!({"ops": []}), DomainErrorKind::Validation, None);
    assert_parity(
        &json!({"ops": [{"tool": "tz_batch", "args": {"ops": []}}]}),
        DomainErrorKind::Runtime,
        Some("batch_operation_failed"),
    );
}

fn rebase_paths(mut args: Value, root: &Path) -> Value {
    if let Some(obj) = args.as_object_mut() {
        if let Some(path) = obj.get("path").cloned() {
            match path {
                Value::String(s) if !Path::new(&s).is_absolute() => {
                    obj.insert("path".into(), json!(root.join(s).display().to_string()));
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
        if let Some(Value::String(cwd)) = obj.get("cwd").cloned()
            && !Path::new(&cwd).is_absolute()
        {
            obj.insert("cwd".into(), json!(root.join(cwd).display().to_string()));
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

    let code = |o: &tokenzero_engine::DispatchOutcome| {
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
#[test]
fn registry_domain_ops_are_metadata_driven_not_masked() {
    // Every Canonical/LegacyAlias non-resource op must be classified domain;
    // every CodemodeControl/Resource must not. No hard-coded name denylist.
    use tokenzero_core::operation_abi::{MigrationStatus, all_operations};
    // operation_is_domain is on the engine crate-root re-export (owned by domain)
    use tokenzero_engine::operation_is_domain as eng_is_domain;
    for op in all_operations() {
        let expected = matches!(
            op.migration,
            MigrationStatus::Canonical | MigrationStatus::LegacyAlias
        ) && op.exposure.resource_uri.is_none();
        assert_eq!(
            eng_is_domain(op),
            expected,
            "classification drift for {}",
            op.name
        );
        assert_eq!(
            tokenzero_engine::is_domain_operation(op.name),
            expected,
            "name resolve drift for {}",
            op.name
        );
    }
}

#[test]
fn every_registry_domain_op_is_kernel_dispatchable() {
    use tokenzero_engine::all_domain_operations;
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("note.txt"), b"dispatcher-identity").unwrap();
    let engine = engine_for(root.path());
    for op in all_domain_operations() {
        // Minimal args; kernel may return tool-level errors but must not be TransportOnly.
        let args = minimal_args(op.name);
        let args = rebase_paths(args, root.path());
        let outcome = dispatch_raw_worker(&engine, op.name, &args);
        if let Some(err) = &outcome.domain_error {
            assert!(
                !err.message.contains("transport-control only"),
                "domain op {} rejected as transport-only: {}",
                op.name,
                err.message
            );
        }
    }
}

#[test]
fn cli_domain_handlers_use_dispatch_cli_only() {
    // Static parity: CLI main routes domain ops through dispatch_cli_tool /
    // dispatch_cli and does not call engine domain methods directly.
    let main = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tokenzero/src/main.rs");
    let text = fs::read_to_string(&main).unwrap_or_else(|e| panic!("read {}: {e}", main.display()));
    assert!(
        text.contains("fn dispatch_cli_tool"),
        "CLI must define dispatch_cli_tool thin adapter"
    );
    assert!(
        text.contains("tokenzero_mcp_compat::dispatch_cli")
            || text.contains("tokenzero_engine::dispatch_cli"),
        "CLI must call shared dispatch_cli"
    );
    let forbidden_direct = [
        "engine.find(",
        "engine.grep(",
        "engine.read(",
        "engine.glob(",
        "engine.tree(",
        "engine.edit(",
        "engine.shell(",
        "engine.ingest(",
        "engine.expand(",
        "engine.expand_with_params(",
        "engine.mem(",
        "engine.recall(",
        "engine.fetch(",
        "engine.cache_pack(",
    ];
    let mut hits = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        for pat in forbidden_direct {
            if trimmed.contains(pat) {
                hits.push(format!("{}: {line}", lineno + 1));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "CLI still calls engine domain methods directly:\n{}",
        hits.join("\n")
    );

    // Every FastMCP domain op that has a CLI surface must appear as dispatch target.
    let required_ops = [
        "tz_read",
        "tz_find",
        "tz_grep",
        "tz_recall",
        "tz_fetch",
        "tz_glob",
        "tz_tree",
        "tz_edit",
        "tz_shell",
        "tz_ingest",
        "tz_expand",
        "tz_mem",
        "tz_cache_pack",
        "tz_rewrite",
        "tz_discover",
    ];
    for op in required_ops {
        let needle = format!("\"{op}\"");
        assert!(text.contains(&needle), "CLI missing dispatch target {op}");
    }
}

#[test]
fn non_domain_cli_commands_are_not_registry_domain_ops() {
    // Administration / audit CLI commands must not be classified as domain ops.
    let admin = [
        "doctor",
        "install",
        "clients",
        "pulse",
        "session_ledger",
        "bench",
        "quote",
        "hook",
        "mcp",
        "codemode",
    ];
    for name in admin {
        assert!(
            !is_domain_operation(name),
            "admin CLI name {name} must not resolve as domain op"
        );
    }
    // Domain ops that intentionally have no first-class CLI verb stay domain.
    assert!(is_domain_operation("tz_batch"));
    assert!(is_domain_operation("tz_report_tool_issue"));
}

/// A shell deadline spelled in milliseconds must actually bound the command.
///
/// Regression for tokenzero-gpa0: `timeout_ms` was not among the keys the shell
/// dispatcher consulted, so it was accepted and discarded. The command then ran
/// to completion under the default 60s timeout and reported success. Measured
/// through the live router before the fix, `{ timeout_ms: 1000 }` on an 8s
/// command returned after 8048ms with status `ok`.
///
/// This asserts on elapsed wall time rather than the response shape: the bug
/// was that the command KEPT RUNNING, which only a clock can observe.
#[test]
fn shell_timeout_ms_actually_bounds_the_command() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = engine_for(dir.path());

    let started = std::time::Instant::now();
    let _ = dispatch_codemode_method(
        &engine,
        "zero.shell",
        &json!({"command": "sleep 10", "timeout_ms": 1500}),
    );
    let elapsed = started.elapsed();

    assert!(
        elapsed < std::time::Duration::from_secs(6),
        "timeout_ms was ignored: a 10s command under a 1500ms deadline took {elapsed:?}"
    );
}

/// The two spellings must not disagree. Equivalent requests in different units
/// producing different behavior is how the millisecond path stayed broken while
/// the seconds path looked fine.
#[test]
fn shell_timeout_units_agree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = engine_for(dir.path());

    let ms_started = std::time::Instant::now();
    let _ = dispatch_codemode_method(
        &engine,
        "zero.shell",
        &json!({"command": "sleep 10", "timeout_ms": 2000}),
    );
    let ms_elapsed = ms_started.elapsed();

    let secs_started = std::time::Instant::now();
    let _ = dispatch_codemode_method(
        &engine,
        "zero.shell",
        &json!({"command": "sleep 10", "timeout_seconds": 2}),
    );
    let secs_elapsed = secs_started.elapsed();

    let delta = ms_elapsed.abs_diff(secs_elapsed);
    assert!(
        delta < std::time::Duration::from_secs(2),
        "timeout_ms ({ms_elapsed:?}) and timeout_seconds ({secs_elapsed:?}) disagree"
    );
}
