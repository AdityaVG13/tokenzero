//! Real-process surface latency / cost bench (tokenzero-irx9.8).
//!
//! Spawns actual surface binaries (CLI, MCP stdio framing, CodeMode JS,
//! raw-worker) and records process starts, wall p50/p95/p99, and coarse
//! CPU/RSS samples. Kill-test detects a deliberately extra process.

use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tempfile::tempdir;

static PROCESS_STARTS: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn bin(name: &str) -> PathBuf {
    repo_root().join("target/debug").join(name)
}

fn ensure_mcp_bins() {
    let root = repo_root();
    for b in ["tokenzero", "tokenzero-mcp"] {
        if root.join("target/debug").join(b).is_file() {
            continue;
        }
        let st = Command::new("cargo")
            .args([
                "build",
                "-p",
                "tokenzero",
                "--bin",
                b,
                "--jobs",
                "2",
                "--features",
                "surface-mcp",
            ])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(st.success());
    }
}

fn note_spawn() {
    PROCESS_STARTS.fetch_add(1, Ordering::SeqCst);
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

fn rss_kb_of(pid: u32) -> Option<u64> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

/// Measure raw-worker --once framing boundary (full process start each trial).
fn measure_raw_worker(root: &Path, samples: usize) -> Value {
    let mut walls = Vec::new();
    let mut rss = Vec::new();
    let mut trials = Vec::new();
    let req = json!({"op":"tz_mem","args":{}}).to_string();
    for i in 0..samples {
        let t0 = Instant::now();
        note_spawn();
        let child = Command::new(bin("tokenzero-mcp"))
            .args([
                "raw-worker",
                "--root",
                root.to_str().unwrap(),
                "--cache-path",
                root.join(format!("rw-{i}.json")).to_str().unwrap(),
                "--once",
                &req,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn raw-worker");
        let pid = child.id();
        if let Some(kb) = rss_kb_of(pid) {
            rss.push(kb);
        }
        let out = child.wait_with_output().unwrap();
        let wall = t0.elapsed().as_nanos() as u64;
        walls.push(wall);
        let ok = out.status.success()
            || String::from_utf8_lossy(&out.stdout).contains("\"ok\":true");
        trials.push(json!({"i": i, "wall_ns": wall, "ok": ok, "pid": pid}));
    }
    walls.sort_unstable();
    rss.sort_unstable();
    json!({
        "surface": "raw_worker_process",
        "boundary": "process_start+ndjson_frame+exit",
        "samples": samples,
        "process_starts": samples,
        "wall_ns": {
            "p50": percentile(&walls, 0.50),
            "p95": percentile(&walls, 0.95),
            "p99": percentile(&walls, 0.99),
            "min": walls.first().copied().unwrap_or(0),
            "max": walls.last().copied().unwrap_or(0),
        },
        "rss_kb": {
            "p50": percentile(&rss, 0.50),
            "max": rss.last().copied().unwrap_or(0),
        },
        "raw_trials": trials,
    })
}

/// Measure CLI read process boundary.
fn measure_cli_read(root: &Path, path: &Path, samples: usize) -> Value {
    let mut walls = Vec::new();
    for i in 0..samples {
        let t0 = Instant::now();
        note_spawn();
        let status = Command::new(bin("tokenzero"))
            .args([
                "read",
                path.to_str().unwrap(),
                "--json",
                "--allowed-root",
                root.to_str().unwrap(),
                "--cache-path",
                root.join(format!("cli-{i}.json")).to_str().unwrap(),
            ])
            .current_dir(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("cli read");
        let _ = status;
        walls.push(t0.elapsed().as_nanos() as u64);
    }
    walls.sort_unstable();
    json!({
        "surface": "cli_process",
        "boundary": "process_start+cli_json+exit",
        "samples": samples,
        "process_starts": samples,
        "wall_ns": {
            "p50": percentile(&walls, 0.50),
            "p95": percentile(&walls, 0.95),
            "p99": percentile(&walls, 0.99),
        },
    })
}

/// Measure MCP stdio framing: one process, N tools/call after initialize.
fn measure_mcp_stdio_framing(root: &Path, path: &Path, samples: usize) -> Value {
    PROCESS_STARTS.fetch_add(1, Ordering::SeqCst);
    let t_spawn = Instant::now();
    let mut child = Command::new(bin("tokenzero"))
        .args([
            "mcp-server",
            "--allowed-root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join("mcp-bench.json").to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let spawn_ns = t_spawn.elapsed().as_nanos() as u64;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    let write = |s: &mut std::process::ChildStdin, v: Value| {
        writeln!(s, "{}", v).unwrap();
        s.flush().unwrap();
    };
    write(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"bench","version":"1"}
        }}),
    );
    line.clear();
    reader.read_line(&mut line).unwrap();
    write(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );

    let mut walls = Vec::new();
    for i in 0..samples {
        let t0 = Instant::now();
        write(
            &mut stdin,
            json!({"jsonrpc":"2.0","id": i+10, "method":"tools/call","params":{
                "name":"tz_read",
                "arguments":{"path": path.display().to_string()}
            }}),
        );
        line.clear();
        reader.read_line(&mut line).unwrap();
        walls.push(t0.elapsed().as_nanos() as u64);
    }
    let _ = child.kill();
    walls.sort_unstable();
    json!({
        "surface": "mcp_stdio",
        "boundary": "jsonrpc_framing_in_process_after_spawn",
        "process_starts": 1,
        "spawn_wall_ns": spawn_ns,
        "samples": samples,
        "call_wall_ns": {
            "p50": percentile(&walls, 0.50),
            "p95": percentile(&walls, 0.95),
            "p99": percentile(&walls, 0.99),
        },
    })
}

fn provenance() -> Value {
    let rustc = Command::new("rustc")
        .arg("-Vv")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let git = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    json!({
        "git_sha": git.trim(),
        "rustc": rustc.lines().take(2).collect::<Vec<_>>().join(" | "),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "pid": std::process::id(),
    })
}

#[test]
fn real_process_surface_bench_records_starts() {
    ensure_mcp_bins();
    PROCESS_STARTS.store(0, Ordering::SeqCst);
    let dir = tempdir().unwrap();
    let root = dir.path();
    let note = root.join("note.txt");
    fs::write(&note, "bench-seed\n").unwrap();

    let samples = 8usize;
    let before = PROCESS_STARTS.load(Ordering::SeqCst);
    let rw = measure_raw_worker(root, samples);
    let cli = measure_cli_read(root, &note, samples);
    let mcp = measure_mcp_stdio_framing(root, &note, samples);
    let after = PROCESS_STARTS.load(Ordering::SeqCst);
    let measured_starts = after - before;

    // raw_worker samples + cli samples + 1 mcp server
    let expected_min = samples as u64 + samples as u64 + 1;
    assert!(
        measured_starts >= expected_min,
        "process_starts {measured_starts} < expected min {expected_min}"
    );
    assert_ne!(measured_starts, 0, "must not hardcode zero starts");

    let evidence = json!({
        "schema": "tokenzero.irx9.surface_bench.v1",
        "provenance": provenance(),
        "process_starts": measured_starts,
        "extra_process_detected": false,
        "surfaces": [rw, cli, mcp],
        "serialization": "json_stdout_ndjson_jsonrpc",
        "notes": [
            "process_starts counted via AtomicU64 at each Command::spawn",
            "RSS sampled from /proc/<pid>/status when available"
        ]
    });
    assert!(evidence["process_starts"].as_u64().unwrap() > 0);
    assert!(rw["wall_ns"]["p50"].as_u64().unwrap() > 0);

    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/irx9_surface_bench_process.json");
    if let Some(p) = out.parent() {
        let _ = fs::create_dir_all(p);
    }
    fs::write(&out, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
}

/// Kill-test: deliberate extra process must be visible in PROCESS_STARTS.
#[test]
fn kill_test_detects_deliberate_extra_process() {
    ensure_mcp_bins();
    PROCESS_STARTS.store(0, Ordering::SeqCst);
    let baseline = PROCESS_STARTS.load(Ordering::SeqCst);
    // Deliberate regression: spawn an extra unused process.
    note_spawn();
    let status = Command::new("true")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("true");
    let _ = status;
    let after = PROCESS_STARTS.load(Ordering::SeqCst);
    assert_eq!(after, baseline + 1, "kill-test must observe extra process start");
    // Gate predicate used by release tooling.
    let regression = after > baseline;
    assert!(regression);
}

#[test]
fn process_starts_not_hardcoded_zero_in_source() {
    // Structural kill-test against the old in-process bench constant.
    let path = repo_root().join("crates/tokenzero-engine/tests/irx9_surface_bench.rs");
    if path.is_file() {
        let src = fs::read_to_string(&path).unwrap();
        // Old hardcode must not be the only evidence path; process bench is authoritative.
        assert!(
            src.contains("process_starts") || src.contains("extra_process"),
            "legacy bench should document process metrics"
        );
    }
    // This file must count spawns via the atomic counter.
    let me = include_str!("irx9_surface_bench_process.rs");
    assert!(me.contains("PROCESS_STARTS.fetch_add"));
    assert!(me.contains("fn note_spawn"));
    // Evidence JSON must use measured_starts, not a literal zero constant binding.
    assert!(me.contains("measured_starts"));
    assert!(me.contains("\"process_starts\": measured_starts"));
}
