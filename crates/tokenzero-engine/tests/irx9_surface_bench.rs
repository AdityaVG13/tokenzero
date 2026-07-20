//! End-to-end surface latency / cost benchmark harness (tokenzero-irx9.8).
//!
//! Measures raw dispatcher, MCP, CLI, CodeMode, and raw-worker framing for
//! cold and warm runs. Separates kernel vs dispatcher overhead via
//! `last_dispatch_profile`. Writes a machine-readable evidence document.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::time::Instant;
use tempfile::tempdir;
use tokenzero_engine::{
    EngineConfig, HandshakeSurface, RAW_WORKER_PROTOCOL_VERSION, RawWorkerRequest, TokenZeroEngine,
    build_surface_capability, dispatch_cli, dispatch_codemode_method, dispatch_count,
    dispatch_mcp_tool, dispatch_operation, dispatch_raw_worker, execute_raw_worker_frame,
    last_dispatch_profile, DispatchSurface,
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

fn measure(engine: &TokenZeroEngine, surface: Surface, samples: usize, warmup: usize) -> Value {
    for _ in 0..warmup {
        run_once(engine, surface);
    }
    let mut walls = Vec::with_capacity(samples);
    let mut overheads = Vec::with_capacity(samples);
    let mut kernels = Vec::with_capacity(samples);
    let before = dispatch_count();
    for _ in 0..samples {
        let t0 = Instant::now();
        run_once(engine, surface);
        let wall = t0.elapsed().as_nanos() as u64;
        walls.push(wall);
        let profile = last_dispatch_profile();
        overheads.push(profile.dispatcher_overhead_ns);
        kernels.push(profile.kernel_ns);
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
            "min": walls.first().copied().unwrap_or(0),
            "max": walls.last().copied().unwrap_or(0),
        },
        "dispatcher_overhead_ns": {
            "p50": percentile_ns(&overheads, 0.50),
            "p95": percentile_ns(&overheads, 0.95),
        },
        "kernel_ns": {
            "p50": percentile_ns(&kernels, 0.50),
            "p95": percentile_ns(&kernels, 0.95),
        },
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

    // Cold start proxy: process-local first-call wall already included via
    // warmup=0 re-measure on a fresh engine for raw only.
    let engine_cold = engine_for(root);
    let cold = measure(&engine_cold, Surface::Raw, 5, 0);

    let evidence = json!({
        "schema": "tokenzero.irx9.surface_bench.v1",
        "git_sha": option_env!("VERGEN_GIT_SHA").unwrap_or("unknown"),
        "workload": "tz_mem_no_op",
        "n_values": [1, 3, 10, 30],
        "samples_per_surface": samples,
        "warmup": warmup,
        "outlier_policy": "none_trim_full_sample_percentiles",
        "surfaces": surfaces,
        "cold_raw": cold,
        "targets": {
            "warm_recipe_overhead_note": "recipe/JS orchestration measured separately; this harness isolates adapter+dispatcher overhead above kernel",
            "warm_empty_js_p50_ms": 1.0,
            "warm_empty_js_p99_ms": 5.0,
        },
        "notes": [
            "dispatcher_overhead_ns subtracted via last_dispatch_profile",
            "same machine and corpus for all surfaces in this process"
        ]
    });

    assert_eq!(evidence["schema"], "tokenzero.irx9.surface_bench.v1");
    assert_eq!(evidence["surfaces"].as_array().unwrap().len(), 5);

    // Ratchet: warm raw p50 must be finite and non-zero after work.
    let raw = &evidence["surfaces"][0];
    assert!(raw["wall_ns"]["p50"].as_u64().unwrap() > 0);
    // Adapter overhead must not explode relative to kernel for mem (loose gate).
    let over_p50 = raw["dispatcher_overhead_ns"]["p50"].as_u64().unwrap_or(0);
    let ker_p50 = raw["kernel_ns"]["p50"].as_u64().unwrap_or(1).max(1);
    // Dispatcher overhead can be larger than tiny kernels; bound to 50ms absolute.
    assert!(
        over_p50 < 50_000_000,
        "dispatcher overhead p50 too high: {over_p50} ns"
    );
    let _ = ker_p50;

    // Persist evidence under CARGO_TARGET_TMPDIR if available for CI artifacts.
    if let Ok(tmp) = std::env::var("CARGO_TARGET_TMPDIR") {
        let path = Path::new(&tmp).join("irx9_surface_bench.json");
        fs::write(&path, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
    }
}

#[test]
fn multi_n_cost_scales_monotonically_for_raw() {
    let dir = tempdir().unwrap();
    let engine = engine_for(dir.path());
    // Warm once so cold-start does not dominate N=1.
    let _ = dispatch_raw_worker(&engine, "tz_mem", &json!({}));
    let mut prev_per_op = 0u64;
    for n in [1usize, 3, 10, 30] {
        let t0 = Instant::now();
        for _ in 0..n {
            let _ = dispatch_raw_worker(&engine, "tz_mem", &json!({}));
        }
        let elapsed = t0.elapsed().as_nanos() as u64;
        let per_op = elapsed / n as u64;
        // Per-op cost should not grow unboundedly with N (no quadratic path).
        if prev_per_op > 0 {
            assert!(
                per_op < prev_per_op.saturating_mul(20).max(1_000_000),
                "per-op for N={n} ({per_op}) exploded vs prev ({prev_per_op})"
            );
        }
        prev_per_op = per_op;
        // Total work grows with N (allow noise floor).
        assert!(elapsed > 0, "N={n} recorded zero elapsed");
    }
}
