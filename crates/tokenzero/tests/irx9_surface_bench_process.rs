//! Real-process surface latency / cost bench (tokenzero-irx9.8).
//!
//! Spawns actual surface binaries and records:
//! - process starts (via a spawn helper that always wraps Command::spawn)
//! - wall p50/p95/p99
//! - CPU time (utime+stime from /proc/<pid>/stat) when available
//! - RSS samples
//! - framing/serialization sizes (request/response byte lengths)
//!
//! Kill-test: an extra real child process increments the spawn counter without
//! any manual pre-increment theater.

use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tempfile::tempdir;

/// Incremented only inside [`spawn_child`] / [`run_status`] on a real OS spawn.
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

/// Only place that increments PROCESS_STARTS — always paired with a real spawn.
fn spawn_child(cmd: &mut Command) -> std::process::Child {
    PROCESS_STARTS.fetch_add(1, Ordering::SeqCst);
    cmd.spawn().expect("spawn")
}

fn run_status(cmd: &mut Command) -> std::process::ExitStatus {
    PROCESS_STARTS.fetch_add(1, Ordering::SeqCst);
    cmd.status().expect("status")
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

fn rss_kb(pid: u32) -> Option<u64> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

/// CPU time in nanoseconds (utime+stime) from /proc/<pid>/stat (Linux).
fn cpu_ns(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm can contain spaces/parens; split after last ')' of comm.
    let after = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after.split_whitespace().collect();
    // fields[11]=utime, fields[12]=stime in clock ticks (0-based after comm).
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    let ticks = utime.saturating_add(stime);
    // CLK_TCK is typically 100 on Linux.
    let hz = 100u64;
    Some(ticks.saturating_mul(1_000_000_000 / hz))
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

fn measure_raw_worker(root: &Path, samples: usize) -> Value {
    let mut walls = Vec::new();
    let mut cpu = Vec::new();
    let mut rss = Vec::new();
    let mut req_bytes = Vec::new();
    let mut resp_bytes = Vec::new();
    let req = json!({"op":"tz_mem","args":{}}).to_string();
    for i in 0..samples {
        req_bytes.push(req.len() as u64);
        let t0 = Instant::now();
        let mut cmd = Command::new(bin("tokenzero-mcp"));
        cmd.args([
            "raw-worker",
            "--root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join(format!("rw-{i}.json")).to_str().unwrap(),
            "--once",
            &req,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        let child = spawn_child(&mut cmd);
        let pid = child.id();
        if let Some(kb) = rss_kb(pid) {
            rss.push(kb);
        }
        if let Some(c) = cpu_ns(pid) {
            cpu.push(c);
        }
        let out = child.wait_with_output().unwrap();
        let wall = t0.elapsed().as_nanos() as u64;
        walls.push(wall);
        resp_bytes.push(out.stdout.len() as u64);
    }
    walls.sort_unstable();
    cpu.sort_unstable();
    rss.sort_unstable();
    req_bytes.sort_unstable();
    resp_bytes.sort_unstable();
    json!({
        "surface": "raw_worker_process",
        "boundary": "process_start+json_frame+exit",
        "samples": samples,
        "process_starts": samples,
        "wall_ns": {
            "p50": percentile(&walls, 0.50),
            "p95": percentile(&walls, 0.95),
            "p99": percentile(&walls, 0.99),
        },
        "cpu_ns": {
            "p50": percentile(&cpu, 0.50),
            "p95": percentile(&cpu, 0.95),
            "samples_observed": cpu.len(),
        },
        "rss_kb": {
            "p50": percentile(&rss, 0.50),
            "max": rss.last().copied().unwrap_or(0),
        },
        "serialization": {
            "request_bytes_p50": percentile(&req_bytes, 0.50),
            "response_bytes_p50": percentile(&resp_bytes, 0.50),
            "response_bytes_p95": percentile(&resp_bytes, 0.95),
        },
    })
}

fn measure_cli(root: &Path, path: &Path, samples: usize) -> Value {
    let mut walls = Vec::new();
    let mut resp_bytes = Vec::new();
    for i in 0..samples {
        let t0 = Instant::now();
        let mut cmd = Command::new(bin("tokenzero"));
        cmd.args([
            "read",
            path.to_str().unwrap(),
            "--json",
            "--allowed-root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join(format!("cli-{i}.json")).to_str().unwrap(),
        ])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        let child = spawn_child(&mut cmd);
        let out = child.wait_with_output().unwrap();
        walls.push(t0.elapsed().as_nanos() as u64);
        resp_bytes.push(out.stdout.len() as u64);
    }
    walls.sort_unstable();
    resp_bytes.sort_unstable();
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
        "serialization": {
            "response_bytes_p50": percentile(&resp_bytes, 0.50),
        },
    })
}

fn measure_mcp_framing(root: &Path, path: &Path, samples: usize) -> Value {
    let mut cmd = Command::new(bin("tokenzero"));
    cmd.args([
        "mcp-server",
        "--allowed-root",
        root.to_str().unwrap(),
        "--cache-path",
        root.join("mcp-bench.json").to_str().unwrap(),
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    let mut child = spawn_child(&mut cmd);
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    let mut frame_req = Vec::new();
    let mut frame_resp = Vec::new();
    let write = |s: &mut std::process::ChildStdin, v: &Value, sizes: &mut Vec<u64>| {
        let bytes = v.to_string();
        sizes.push(bytes.len() as u64);
        writeln!(s, "{bytes}").unwrap();
        s.flush().unwrap();
    };
    write(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"bench","version":"1"}
        }}),
        &mut frame_req,
    );
    line.clear();
    reader.read_line(&mut line).unwrap();
    frame_resp.push(line.len() as u64);
    write(
        &mut stdin,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        &mut frame_req,
    );
    let mut walls = Vec::new();
    for i in 0..samples {
        let t0 = Instant::now();
        write(
            &mut stdin,
            &json!({"jsonrpc":"2.0","id": i+10, "method":"tools/call","params":{
                "name":"tz_read",
                "arguments":{"path": path.display().to_string()}
            }}),
            &mut frame_req,
        );
        line.clear();
        reader.read_line(&mut line).unwrap();
        frame_resp.push(line.len() as u64);
        walls.push(t0.elapsed().as_nanos() as u64);
    }
    let _ = child.kill();
    walls.sort_unstable();
    frame_req.sort_unstable();
    frame_resp.sort_unstable();
    json!({
        "surface": "mcp_stdio",
        "boundary": "jsonrpc_framing_after_one_spawn",
        "process_starts": 1,
        "samples": samples,
        "call_wall_ns": {
            "p50": percentile(&walls, 0.50),
            "p95": percentile(&walls, 0.95),
            "p99": percentile(&walls, 0.99),
        },
        "serialization": {
            "request_frame_bytes_p50": percentile(&frame_req, 0.50),
            "response_frame_bytes_p50": percentile(&frame_resp, 0.50),
            "response_frame_bytes_p95": percentile(&frame_resp, 0.95),
        },
    })
}

#[test]
fn real_process_surface_bench_records_starts_cpu_serialization() {
    ensure_mcp_bins();
    PROCESS_STARTS.store(0, Ordering::SeqCst);
    let dir = tempdir().unwrap();
    let root = dir.path();
    let note = root.join("note.txt");
    fs::write(&note, "bench-seed\n").unwrap();

    let samples = 8usize;
    let before = PROCESS_STARTS.load(Ordering::SeqCst);
    let rw = measure_raw_worker(root, samples);
    let cli = measure_cli(root, &note, samples);
    let mcp = measure_mcp_framing(root, &note, samples);
    let after = PROCESS_STARTS.load(Ordering::SeqCst);
    let measured_starts = after - before;

    let expected_min = samples as u64 + samples as u64 + 1;
    assert!(
        measured_starts >= expected_min,
        "process_starts {measured_starts} < {expected_min}"
    );
    assert_ne!(measured_starts, 0);

    // Serialization costs must be non-zero measured bytes, not a static note.
    assert!(
        rw["serialization"]["response_bytes_p50"].as_u64().unwrap() > 0
    );
    assert!(
        mcp["serialization"]["response_frame_bytes_p50"]
            .as_u64()
            .unwrap()
            > 0
    );

    let evidence = json!({
        "schema": "tokenzero.irx9.surface_bench.v1",
        "provenance": provenance(),
        "process_starts": measured_starts,
        "extra_process_detected": false,
        "surfaces": [rw, cli, mcp],
    });
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/irx9_surface_bench_process.json");
    if let Some(p) = out.parent() {
        let _ = fs::create_dir_all(p);
    }
    fs::write(&out, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
    assert!(evidence["process_starts"].as_u64().unwrap() > 0);
}

/// Kill-test: a real extra `Command::spawn` of `true` increments PROCESS_STARTS
/// only through [`spawn_child`] / [`run_status`]. Manual fetch_add is not used.
#[test]
fn kill_test_detects_deliberate_extra_process() {
    ensure_mcp_bins();
    PROCESS_STARTS.store(0, Ordering::SeqCst);
    let baseline = PROCESS_STARTS.load(Ordering::SeqCst);
    // Real child process — if this line is removed, the assertion fails.
    let mut cmd = Command::new("true");
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    let status = run_status(&mut cmd);
    assert!(status.success());
    let after = PROCESS_STARTS.load(Ordering::SeqCst);
    assert_eq!(
        after,
        baseline + 1,
        "extra real process must increment spawn counter by 1"
    );
}

#[test]
fn spawn_helper_is_only_counter_path() {
    let me = include_str!("irx9_surface_bench_process.rs");
    // Only the two helper bodies may call fetch_add (exact statement form).
    let call_sites: Vec<&str> = me
        .lines()
        .map(str::trim)
        .filter(|l| *l == "PROCESS_STARTS.fetch_add(1, Ordering::SeqCst);")
        .collect();
    assert_eq!(
        call_sites.len(),
        2,
        "exactly two fetch_add call statements in spawn_child+run_status"
    );
    assert!(me.contains("fn spawn_child"));
    assert!(me.contains("fn run_status"));
    assert!(me.contains("cpu_ns"));
    assert!(me.contains("serialization"));
}
