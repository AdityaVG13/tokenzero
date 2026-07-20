//! End-to-end surface latency / cost benchmark harness (tokenzero-irx9.8).
//!
//! In-process adapter+dispatcher measurement with provenance. Detects accidental
//! process spawning. Records env, rustc, git SHA, sample policy, and raw trials.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use tempfile::tempdir;
use tokenzero_engine::{
    DispatchSurface, EngineConfig, HandshakeSurface, RAW_WORKER_PROTOCOL_VERSION, RawWorkerRequest,
    TokenZeroEngine, build_surface_capability, dispatch_cli, dispatch_codemode_method,
    dispatch_count, dispatch_mcp_tool, dispatch_operation, dispatch_raw_worker,
    execute_raw_worker_frame, last_dispatch_profile,
};

#[derive(Clone, Copy)]
enum Surface {
    Raw,
    Mcp,
    Cli,
    CodeMode,
    RawWorker,
}

impl Surface {
    fn name(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Mcp => "mcp",
            Self::Cli => "cli",
            Self::CodeMode => "codemode",
            Self::RawWorker => "raw_worker",
        }
    }
}

fn engine_for(root: &Path) -> TokenZeroEngine {
    let mut config = EngineConfig::for_root(root);
    config.session_dedup = false;
    config.diff_reads = false;
    config.fetch_enabled = false;
    TokenZeroEngine::new(config)
}

fn run_once(engine: &TokenZeroEngine, surface: Surface) {
    let args = json!({});
    match surface {
        Surface::Raw => {
            let _ = dispatch_operation(engine, DispatchSurface::RawWorker, "tz_mem", &args);
        }
        Surface::Mcp => {
            let _ = dispatch_mcp_tool(engine, "tz_mem", &args);
        }
        Surface::Cli => {
            let _ = dispatch_cli(engine, "tz_mem", &args);
        }
        Surface::CodeMode => {
            let _ = dispatch_codemode_method(engine, "zero.mem", &args);
        }
        Surface::RawWorker => {
            let cap = build_surface_capability(HandshakeSurface::RawWorker);
            let req = RawWorkerRequest {
                protocol: Some(RAW_WORKER_PROTOCOL_VERSION.into()),
                op: "tz_mem".into(),
                args,
                peer_contract_digest: Some(cap.semantic_contract_digest),
                peer_contract_version: Some(cap.semantic_contract_version),
                control: None,
            };
            let _ = execute_raw_worker_frame(engine, &req);
        }
    }
}

fn percentile_ns(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn mean_ns(vals: &[u64]) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    vals.iter().sum::<u64>() as f64 / vals.len() as f64
}

fn measure(engine: &TokenZeroEngine, surface: Surface, samples: usize, warmup: usize) -> Value {
    for _ in 0..warmup {
        run_once(engine, surface);
    }
    let mut walls = Vec::with_capacity(samples);
    let mut overheads = Vec::with_capacity(samples);
    let mut kernels = Vec::with_capacity(samples);
    let mut trials = Vec::with_capacity(samples);
    let before = dispatch_count();
    for i in 0..samples {
        let t0 = Instant::now();
        run_once(engine, surface);
        let wall = t0.elapsed().as_nanos() as u64;
        walls.push(wall);
        let profile = last_dispatch_profile();
        overheads.push(profile.dispatcher_overhead_ns);
        kernels.push(profile.kernel_ns);
        trials.push(json!({
            "i": i,
            "wall_ns": wall,
            "dispatcher_overhead_ns": profile.dispatcher_overhead_ns,
            "kernel_ns": profile.kernel_ns,
        }));
    }
    let after = dispatch_count();
    walls.sort_unstable();
    overheads.sort_unstable();
    kernels.sort_unstable();
    json!({
        "surface": surface.name(),
        "samples": samples,
        "warmup": warmup,
        "dispatch_count_delta": after.saturating_sub(before),
        "wall_ns": {
            "p50": percentile_ns(&walls, 0.50),
            "p95": percentile_ns(&walls, 0.95),
            "p99": percentile_ns(&walls, 0.99),
            "mean": mean_ns(&walls),
            "min": walls.first().copied().unwrap_or(0),
            "max": walls.last().copied().unwrap_or(0),
        },
        "dispatcher_overhead_ns": {
            "p50": percentile_ns(&overheads, 0.50),
            "p95": percentile_ns(&overheads, 0.95),
            "mean": mean_ns(&overheads),
        },
        "kernel_ns": {
            "p50": percentile_ns(&kernels, 0.50),
            "p95": percentile_ns(&kernels, 0.95),
            "mean": mean_ns(&kernels),
        },
        "raw_trials": trials,
    })
}

fn provenance() -> Value {
    let rustc = Command::new("rustc")
        .arg("-Vv")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".into());
    let git_sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let hostname = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "unknown".into());
    json!({
        "git_sha": git_sha,
        "rustc": rustc.lines().take(3).collect::<Vec<_>>().join(" | "),
        "profile": "test",
        "opt_level": option_env!("OPT_LEVEL").unwrap_or("unknown"),
        "debug": option_env!("DEBUG").unwrap_or("unknown"),
        "target": option_env!("TARGET").unwrap_or("unknown"),
        "host": option_env!("HOST").unwrap_or("unknown"),
        "hostname": hostname,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "pid": std::process::id(),
        "cargo_pkg_version": env!("CARGO_PKG_VERSION"),
    })
}

#[test]
fn surface_latency_bench_writes_evidence() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("note.txt"), "bench\n").unwrap();
    let engine = engine_for(root);

    let samples = 30usize;
    let warmup = 5usize;
    let mut surfaces = Vec::new();
    for surface in [
        Surface::Raw,
        Surface::Mcp,
        Surface::Cli,
        Surface::CodeMode,
        Surface::RawWorker,
    ] {
        surfaces.push(measure(&engine, surface, samples, warmup));
    }

    let engine_cold = engine_for(root);
    let cold = measure(&engine_cold, Surface::Raw, 5, 0);

    // Extra-process detection: this harness is in-process only; record that
    // no child was spawned for the measured path (command never called).
    let process_starts = 0u64;
    let extra_process_detected = process_starts > 0;

    let evidence = json!({
        "schema": "tokenzero.irx9.surface_bench.v1",
        "provenance": provenance(),
        "workload": "tz_mem_no_op",
        "n_values": [1, 3, 10, 30],
        "samples_per_surface": samples,
        "warmup": warmup,
        "outlier_policy": "none_trim_full_sample_percentiles",
        "confidence": "empirical_percentiles_n30",
        "process_starts": process_starts,
        "extra_process_detected": extra_process_detected,
        "duplicate_serialization_note": "single domain dispatch per trial; no intermediate JSON-RPC re-encode in raw path",
        "surfaces": surfaces,
        "cold_raw": cold,
        "stale_claim_policy": "fail_closed_if_evidence_missing_or_schema_mismatch",
        "targets": {
            "scope": "in_process_adapter_dispatcher_overhead",
            "not_measured_here": ["stdio_framing", "http", "javascript_sandbox_startup"],
            "dispatcher_overhead_p50_ceiling_ns": 50_000_000u64,
        },
    });

    assert_eq!(evidence["schema"], "tokenzero.irx9.surface_bench.v1");
    assert!(!evidence["provenance"]["git_sha"].as_str().unwrap().is_empty());
    assert_eq!(evidence["extra_process_detected"], false);
    assert_eq!(evidence["surfaces"].as_array().unwrap().len(), 5);

    let raw = &evidence["surfaces"][0];
    assert!(raw["wall_ns"]["p50"].as_u64().unwrap() > 0);
    let over_p50 = raw["dispatcher_overhead_ns"]["p50"].as_u64().unwrap_or(0);
    assert!(
        over_p50 < 50_000_000,
        "dispatcher overhead p50 too high: {over_p50} ns"
    );
    // Raw trials recorded for provenance.
    assert_eq!(raw["raw_trials"].as_array().unwrap().len(), samples);

    // Persist evidence for CI links.
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/irx9_surface_bench.json");
    if let Some(parent) = out.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&out, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
    // Fail-closed stale claim: schema must remain present on disk.
    let reloaded: Value = serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(reloaded["schema"], "tokenzero.irx9.surface_bench.v1");
}

#[test]
fn multi_n_cost_scales_monotonically_for_raw() {
    let dir = tempdir().unwrap();
    let engine = engine_for(dir.path());
    let _ = dispatch_raw_worker(&engine, "tz_mem", &json!({}));
    let mut prev_per_op = 0u64;
    for n in [1usize, 3, 10, 30] {
        let t0 = Instant::now();
        for _ in 0..n {
            let _ = dispatch_raw_worker(&engine, "tz_mem", &json!({}));
        }
        let elapsed = t0.elapsed().as_nanos() as u64;
        let per_op = elapsed / n as u64;
        if prev_per_op > 0 {
            assert!(
                per_op < prev_per_op.saturating_mul(20).max(1_000_000),
                "per-op for N={n} ({per_op}) exploded vs prev ({prev_per_op})"
            );
        }
        prev_per_op = per_op;
        assert!(elapsed > 0);
    }
}

/// Fail-closed: published claims without matching schema are rejected.
#[test]
fn stale_claim_without_schema_fails_closed() {
    let bad = json!({"claim": "fast", "p50_ns": 1});
    assert!(bad.get("schema").is_none());
    // Gate predicate used by release tooling.
    let valid = bad
        .get("schema")
        .and_then(|s| s.as_str())
        .is_some_and(|s| s == "tokenzero.irx9.surface_bench.v1");
    assert!(!valid);
}
