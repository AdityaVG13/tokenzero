//! CodeMode plan composition benchmark harness.
//!
//! Measures end-to-end efficiency of plan-based execution versus equivalent
//! raw subprocess output and equivalent classic MCP tool operations. Produces
//! machine-readable JSON output.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use tokenzero_core::{ToolResponse, count_tokens};

use crate::fastmcp_mode::fastmcp_content_texts_from_tool_result;
use crate::tools::{dispatch_tool, mcp_tool_response};
use crate::{EngineConfig, TokenZeroEngine};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub workload: String,
    pub description: String,
    pub scale_workload: bool,
    pub plan_ops: usize,
    pub equivalent_perop_calls: usize,
    pub raw_command_count: usize,
    pub raw_visible_tokens: usize,
    pub perop_visible_tokens: usize,
    pub perop_args_tokens: usize,
    pub perop_raw_tokens: usize,
    pub plan_visible_tokens: usize,
    pub plan_recovery_tokens: usize,
    pub plan_text_tokens: usize,
    pub payload_tokens: usize,
    pub envelope_tokens: usize,
    pub plan_raw_tokens: usize,
    pub plan_duration_ms: u64,
    pub perop_duration_ms: u64,
    pub raw_duration_ms: u64,
    /// Recovery-adjusted savings: plan visible tokens plus M_rec debits.
    pub codemode_vs_raw_savings_pct: f64,
    pub codemode_vs_perop_savings_pct: f64,
    pub gross_codemode_vs_raw_savings_pct: f64,
    pub gross_codemode_vs_perop_savings_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub version: String,
    pub workloads: Vec<BenchmarkResult>,
    pub totals: BenchmarkTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkTotals {
    pub total_raw_visible: usize,
    pub total_perop_visible: usize,
    pub total_perop_args: usize,
    pub total_perop_raw: usize,
    pub total_plan_visible: usize,
    pub total_plan_recovery: usize,
    pub total_plan_text: usize,
    pub total_payload: usize,
    pub total_envelope: usize,
    pub total_plan_raw: usize,
    /// Recovery-adjusted savings: total visible tokens plus M_rec debits.
    pub codemode_vs_raw_savings_pct: f64,
    pub codemode_vs_perop_savings_pct: f64,
    pub gross_codemode_vs_raw_savings_pct: f64,
    pub gross_codemode_vs_perop_savings_pct: f64,
    pub headline_savings_pct: f64,
}

#[derive(Debug, Clone)]
pub struct Workload {
    pub name: String,
    pub description: String,
    pub scale_workload: bool,
    pub plan: String,
    raw_commands: Vec<RawCommand>,
    perop_calls: Vec<DirectCall>,
}

#[derive(Debug, Clone)]
struct DirectCall {
    name: &'static str,
    canonical: &'static str,
    args: Value,
    text_from_previous: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct RawCommand {
    program: &'static str,
    args: Vec<String>,
}

#[derive(Debug)]
struct PlanMeasurement {
    visible_tokens: usize,
    recovery_tokens: usize,
    payload_tokens: usize,
    envelope_tokens: usize,
    raw_tokens: usize,
    ops: usize,
    wire_texts: Vec<String>,
}

#[derive(Debug)]
struct PerOpMeasurement {
    visible_tokens: usize,
    args_tokens: usize,
    raw_tokens: usize,
    wire_text: String,
}

#[derive(Debug)]
struct RawMeasurement {
    visible_tokens: usize,
    wire_text: String,
}

pub fn workloads_for_root(root: &std::path::Path) -> Vec<Workload> {
    let root_buf = benchmark_root(root);
    let root = root_buf.as_path();
    let root_str = root.to_string_lossy();
    let cargo_toml = format!("{root_str}/Cargo.toml");
    let crates_dir = format!("{root_str}/crates");

    // Deterministic synthetic corpus for scale workloads (no live git state).
    // Content is a pure function of loop indices and is written to a fixed
    // temp-dir path, so every run on a machine is byte-identical. Sizes are
    // calibrated to the real transcripts these workloads represent: a
    // multi-file diff review (~1,100 lines), a wide symbol exploration
    // (300 hits), and a 100-commit log.
    let corpus_dir = std::env::temp_dir().join("tokenzero-bench-corpus-v1");
    let _ = std::fs::create_dir_all(&corpus_dir);
    let corpus = corpus_dir.to_string_lossy().to_string();
    {
        let mut diff_data = String::new();
        for file_idx in 0..36 {
            diff_data.push_str(&format!(
                "diff --git a/crates/pkg{file_idx}/src/module.rs b/crates/pkg{file_idx}/src/module.rs\n@@ -{},{} +{},{} @@ fn handler_{file_idx}()\n",
                file_idx * 11 + 1, 24, file_idx * 11 + 1, 27,
            ));
            for line_idx in 0..30 {
                let marker = match (file_idx + line_idx) % 9 {
                    0 => "TODO: tighten error handling for the retry path",
                    3 => "fixme: this clone is avoidable once the borrow is restructured",
                    _ => "let outcome = dispatch(engine, canonical, name, args)?;",
                };
                let sign = if line_idx % 5 == 0 {
                    '+'
                } else if line_idx % 7 == 0 {
                    '-'
                } else {
                    ' '
                };
                diff_data.push_str(&format!(
                    "{sign}    {marker} // L{}\n",
                    file_idx * 30 + line_idx
                ));
            }
        }
        let mut explore_src = String::new();
        for line_idx in 0..1500 {
            let line = match line_idx % 5 {
                0 => format!("    // CodeMode checkpoint {line_idx}: execution substrate marker"),
                1 => format!(
                    "    let handler_{line_idx} = dispatch_table.get({});",
                    line_idx % 64
                ),
                2 => format!("    outcome_{line_idx}.record(telemetry.visible_tokens);"),
                3 => format!(
                    "    if budget.remaining() < {} {{ return Err(Budget); }}",
                    line_idx % 900
                ),
                _ => format!("    store.alias(logical_ref_{line_idx}, blob_ref);"),
            };
            explore_src.push_str(&line);
            explore_src.push('\n');
        }
        let log_data = (1..=100u32)
            .map(|i| {
                let hash = format!("{:07x}", i.wrapping_mul(2654435761) % 0x0fff_ffff);
                let prefix = ["feat", "fix", "docs", "chore"][(i % 4) as usize];
                format!(
                    "{hash} {prefix}: synthetic commit {i} adjusting subsystem {}",
                    i % 12
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let _ = std::fs::write(corpus_dir.join("review.patch"), &diff_data);
        let _ = std::fs::write(corpus_dir.join("explore-src.rs"), &explore_src);
        let _ = std::fs::write(corpus_dir.join("git-log.txt"), &log_data);
    }
    let diff_cmd = format!("cat {corpus}/review.patch");
    let diff_cmd_json = serde_json::to_string(&diff_cmd).unwrap();
    let log_cmd = format!("cat {corpus}/git-log.txt");
    let log_cmd_json = serde_json::to_string(&log_cmd).unwrap();
    let grep_cmd = format!("grep -n CodeMode {corpus}/explore-src.rs");
    let grep_cmd_json = serde_json::to_string(&grep_cmd).unwrap();

    vec![
        Workload {
            name: "file-search-transform".to_string(),
            description: "Read a file, grep for pattern, filter results, compact output".to_string(),
            scale_workload: false,
            plan: format!(
                r#"const f = await zero.read("{cargo_toml}"); const hits = await zero.grep("version", "{root_str}"); const filtered = await zero.filter_lines(hits.text, "tokenzero"); return {{ file_tokens: f.visible_tokens, grep_tokens: hits.visible_tokens, filtered: filtered }}"#,
            ),
            raw_commands: vec![raw_sh(format!("cat {cargo_toml}; grep -R version {root_str}/Cargo.toml {root_str}/crates/*/Cargo.toml"))],
            perop_calls: vec![
                direct("tz_read", "read", json!({"path": cargo_toml})),
                direct("tz_find", "find", json!({"query": "version", "path": root_str.to_string()})),
                direct("tz_find", "find", json!({"query": "version", "path": root_str.to_string()})),
            ],
        },
        Workload {
            name: "shell-multi-step".to_string(),
            description: "Run multiple shell commands and aggregate results in one plan".to_string(),
            scale_workload: false,
            plan: r#"const v = await zero.shell("echo 'git version 2.45.0'"); const s = await zero.shell("printf '%s' '1111111 feat: initial\n2222222 fix: patch\n3333333 docs: readme\n4444444 feat: migrate\n5555555 chore: cleanup'"); const d = await zero.shell("printf '%s' '5555555'"); return { git_version: v.text, log: s.text, rev: d.text }"#.to_string(),
            raw_commands: vec![
                raw_sh("echo 'git version 2.45.0'".to_string()),
                raw_sh("printf '%s' '1111111 feat: initial\n2222222 fix: patch\n3333333 docs: readme\n4444444 feat: migrate\n5555555 chore: cleanup'".to_string()),
                raw_sh("printf '%s' '5555555'".to_string()),
            ],
            perop_calls: vec![
                direct("tz_shell", "shell", json!({"command": "echo 'git version 2.45.0'"})),
                direct("tz_shell", "shell", json!({"command": "printf '%s' '1111111 feat: initial\n2222222 fix: patch\n3333333 docs: readme\n4444444 feat: migrate\n5555555 chore: cleanup'"})),
                direct("tz_shell", "shell", json!({"command": "printf '%s' '5555555'"})),
            ],
        },
        Workload {
            name: "pipe-composition".to_string(),
            description: "Sequential read then compact in one plan, demonstrating zero-roundtrip chaining".to_string(),
            scale_workload: false,
            plan: format!(
                r#"return await zero.pipe([{{"method":"zero.read","args":["{cargo_toml}"]}},{{"method":"zero.compact","args":["_prev"]}}])"#,
            ),
            raw_commands: vec![raw_sh(format!("cat {cargo_toml}"))],
            perop_calls: vec![
                direct("tz_read", "read", json!({"path": cargo_toml})),
                DirectCall { name: "tz_compact", canonical: "compact", args: json!({"text": ""}), text_from_previous: Some("text") },
            ],
        },
        Workload {
            name: "mixed-exploration".to_string(),
            description: "Explore project structure: tree + glob + targeted reads in one plan".to_string(),
            scale_workload: false,
            plan: format!(
                r#"const t = await zero.tree("{crates_dir}", {{ depth: 2 }}); const g = await zero.glob("*.toml", "{crates_dir}"); const r = await zero.read("{cargo_toml}"); return {{ tree_lines: t.text, toml_files: g.text, root_manifest: r.visible_tokens }}"#,
            ),
            raw_commands: vec![raw_sh(format!("find {crates_dir} -maxdepth 2 -print; find {crates_dir} -name '*.toml' -print; cat {cargo_toml}"))],
            perop_calls: vec![
                direct("tz_tree", "tree", json!({"path": crates_dir, "depth": 2})),
                direct("tz_glob", "glob", json!({"pattern": "*.toml", "path": format!("{root_str}/crates")})),
                direct("tz_read", "read", json!({"path": cargo_toml})),
            ],
        },
        Workload {
            name: "scale-diff-review".to_string(),
            description: "Review a multi-file diff for TODO/fixme markers and return a ref-backed verdict".to_string(),
            scale_workload: true,
            plan: format!(
                r#"const d = await zero.shell({diff_cmd_json}); const text = d.text || ""; const flags = text.split(/\r?\n/).filter(l => /todo|fixme/i.test(l)); const saved = await zero.ref(text); return {{ verdict: flags.length ? "review" : "clean", todo_fixme: flags.length, ref: saved.ref || saved, preview: zero.first(d, 1) }}"#,
            ),
            raw_commands: vec![raw_sh(diff_cmd.clone())],
            perop_calls: vec![
                direct("tz_shell", "shell", json!({"command": diff_cmd})),
                direct("tz_shell", "shell", json!({"command": format!("grep -c TODO {corpus}/review.patch")})),
                direct("tz_shell", "shell", json!({"command": format!("grep -c fixme {corpus}/review.patch")})),
            ],
        },
        Workload {
            name: "scale-multi-file-explore".to_string(),
            description: "Grep a common symbol across crates, read top hit files, and return one-line summaries plus refs".to_string(),
            scale_workload: true,
            plan: format!(
                r#"const out = await zero.shell({grep_cmd_json}); const text = out.text || ""; const lines = text.split(/\r?\n/).filter(Boolean); return {{ hits: lines.filter(l => l.includes("CodeMode")).length, total_lines: lines.length, preview: zero.first(out, 3) }}"#,
            ),
            raw_commands: vec![raw_sh(grep_cmd.clone())],
            perop_calls: vec![
                direct("tz_shell", "shell", json!({"command": grep_cmd})),
            ],
        },
        Workload {
            name: "scale-log-summarize".to_string(),
            description: "Summarize the last 100 commits by conventional prefix and return a verdict".to_string(),
            scale_workload: true,
            plan: format!(
                r#"const log = await zero.shell({log_cmd_json}); const text = log.text || ""; const lines = text.split(/\r?\n/).filter(Boolean); const counts = {{ f: 0, x: 0, d: 0, o: 0 }}; for (const line of lines) {{ const msg = line.replace(/^[0-9a-f]+\s+/, ""); if (msg.startsWith("feat")) counts.f++; else if (msg.startsWith("fix")) counts.x++; else if (msg.startsWith("docs")) counts.d++; else counts.o++; }} return `ok f${{counts.f}} x${{counts.x}} d${{counts.d}} o${{counts.o}}`"#,
            ),
            raw_commands: vec![raw_sh(log_cmd.clone())],
            perop_calls: vec![direct("tz_shell", "shell", json!({"command": log_cmd}))],
        },
    ]
}

fn benchmark_root(root: &Path) -> PathBuf {
    let mut cursor = root.to_path_buf();
    loop {
        let manifest = cursor.join("Cargo.toml");
        if std::fs::read_to_string(&manifest)
            .map(|text| text.contains("[workspace]") && text.contains("tokenzero-mcp"))
            .unwrap_or(false)
        {
            return cursor;
        }
        if !cursor.pop() {
            return root.to_path_buf();
        }
    }
}

fn direct(name: &'static str, canonical: &'static str, args: Value) -> DirectCall {
    DirectCall {
        name,
        canonical,
        args,
        text_from_previous: None,
    }
}

fn raw_sh(command: String) -> RawCommand {
    RawCommand {
        program: "sh",
        args: vec!["-lc".to_string(), command],
    }
}

const BENCHMARK_REPORT_VERSION: &str = "1.4.0";

fn timed<T>(run: impl FnOnce() -> T) -> (T, u64) {
    let start = Instant::now();
    let value = run();
    (value, start.elapsed().as_millis() as u64)
}

pub fn run_benchmark(root: &std::path::Path) -> BenchmarkReport {
    let root_buf = benchmark_root(root);
    let root = root_buf.as_path();
    let workloads = workloads_for_root(root);
    let mut results = Vec::new();

    for (index, wl) in workloads.iter().enumerate() {
        let plan_cache = hermetic_cache_path(index, &wl.name, "plan");
        let perop_cache = hermetic_cache_path(index, &wl.name, "perop");
        let _ = std::fs::remove_file(&plan_cache);
        let _ = std::fs::remove_file(&perop_cache);
        let plan_engine = engine_for_leg(root, plan_cache);
        let perop_engine = engine_for_leg(root, perop_cache);

        let (plan, plan_ms) = timed(|| measure_plan_leg(&plan_engine, &wl.plan));
        let (perop, perop_ms) = timed(|| measure_perop_leg(&perop_engine, wl));
        let (raw, raw_ms) = timed(|| measure_raw_leg(root, wl));

        results.push(BenchmarkResult {
            workload: wl.name.clone(),
            description: wl.description.clone(),
            scale_workload: wl.scale_workload,
            plan_ops: plan.ops,
            equivalent_perop_calls: wl.perop_calls.len(),
            raw_command_count: wl.raw_commands.len(),
            raw_visible_tokens: raw.visible_tokens,
            perop_visible_tokens: perop.visible_tokens,
            perop_args_tokens: perop.args_tokens,
            perop_raw_tokens: perop.raw_tokens,
            plan_visible_tokens: plan.visible_tokens,
            plan_recovery_tokens: plan.recovery_tokens,
            plan_text_tokens: count_tokens(&wl.plan),
            payload_tokens: plan.payload_tokens,
            envelope_tokens: plan.envelope_tokens,
            plan_raw_tokens: plan.raw_tokens,
            plan_duration_ms: plan_ms,
            perop_duration_ms: perop_ms,
            raw_duration_ms: raw_ms,
            codemode_vs_raw_savings_pct: savings_pct(
                plan.visible_tokens.saturating_add(plan.recovery_tokens),
                raw.visible_tokens,
            ),
            codemode_vs_perop_savings_pct: savings_pct(
                plan.visible_tokens.saturating_add(plan.recovery_tokens),
                perop.visible_tokens,
            ),
            gross_codemode_vs_raw_savings_pct: savings_pct(plan.visible_tokens, raw.visible_tokens),
            gross_codemode_vs_perop_savings_pct: savings_pct(
                plan.visible_tokens,
                perop.visible_tokens,
            ),
        });
    }

    let total_raw_visible: usize = results.iter().map(|r| r.raw_visible_tokens).sum();
    let total_perop_visible: usize = results.iter().map(|r| r.perop_visible_tokens).sum();
    let total_perop_args: usize = results.iter().map(|r| r.perop_args_tokens).sum();
    let total_perop_raw: usize = results.iter().map(|r| r.perop_raw_tokens).sum();
    let total_plan_visible: usize = results.iter().map(|r| r.plan_visible_tokens).sum();
    let total_plan_recovery: usize = results.iter().map(|r| r.plan_recovery_tokens).sum();
    let total_plan_text: usize = results.iter().map(|r| r.plan_text_tokens).sum();
    let total_payload: usize = results.iter().map(|r| r.payload_tokens).sum();
    let total_envelope: usize = results.iter().map(|r| r.envelope_tokens).sum();
    let total_plan_raw: usize = results.iter().map(|r| r.plan_raw_tokens).sum();
    let recovery_adjusted_plan = total_plan_visible.saturating_add(total_plan_recovery);
    let vs_raw = savings_pct(recovery_adjusted_plan, total_raw_visible);
    let vs_perop = savings_pct(recovery_adjusted_plan, total_perop_visible);
    let gross_vs_raw = savings_pct(total_plan_visible, total_raw_visible);
    let gross_vs_perop = savings_pct(total_plan_visible, total_perop_visible);

    BenchmarkReport {
        version: BENCHMARK_REPORT_VERSION.to_string(),
        workloads: results,
        totals: BenchmarkTotals {
            total_raw_visible,
            total_perop_visible,
            total_perop_args,
            total_perop_raw,
            total_plan_visible,
            total_plan_recovery,
            total_plan_text,
            total_payload,
            total_envelope,
            total_plan_raw,
            codemode_vs_raw_savings_pct: vs_raw,
            codemode_vs_perop_savings_pct: vs_perop,
            gross_codemode_vs_raw_savings_pct: gross_vs_raw,
            gross_codemode_vs_perop_savings_pct: gross_vs_perop,
            headline_savings_pct: vs_raw,
        },
    }
}

fn savings_pct(plan_visible: usize, baseline_visible: usize) -> f64 {
    if baseline_visible > 0 {
        let savings = (1.0 - (plan_visible as f64 / baseline_visible as f64)) * 100.0;
        (savings * 10.0).round() / 10.0
    } else {
        0.0
    }
}

fn hermetic_cache_path(index: usize, workload: &str, leg: &str) -> PathBuf {
    static BENCH_RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let run_id = BENCH_RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "tokenzero-bench-cache-{}-{index}-{workload}-{leg}-{run_id}.json",
        std::process::id()
    ))
}

fn engine_for_leg(root: &Path, cache_path: PathBuf) -> TokenZeroEngine {
    // Workloads use absolute paths under the workspace (via benchmark_root).
    // Package-cwd engines (crates/tokenzero-mcp) would deny those paths after
    // wqw.5 hard path_not_allowed — always allowlist the workspace root.
    let workspace = benchmark_root(root);
    let mut config = EngineConfig::for_root(&workspace);
    config.cache_path = cache_path;
    config.session_dedup = false;
    TokenZeroEngine::new(config)
}

fn wire_tokens(texts: &[String]) -> usize {
    texts.iter().map(|text| count_tokens(text)).sum()
}

fn recovery_tokens_from_response(response: &ToolResponse) -> usize {
    response
        .telemetry
        .as_ref()
        .and_then(|telemetry| telemetry.pointer("/structuredContent/telemetry/recovery_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize
}

fn accounted_raw_tokens(response: &ToolResponse) -> usize {
    response
        .accounting
        .as_ref()
        .map(|a| a.raw_tokens)
        .unwrap_or(0)
}

fn measure_plan_leg(engine: &TokenZeroEngine, plan: &str) -> PlanMeasurement {
    // Bench plans (esp. scale-* with shell + compact) can exceed the product
    // 5s hard wall under parallel package-suite load, zeroing payload tokens
    // and flaking matrix_integrity. Raise wall for measurement only.
    let response = dispatch_tool(
        engine,
        "execute_code",
        "tz_execute_code",
        &json!({
            "plan": plan,
            "envelope": "v2",
            "ref_first": true,
            "limits": {
                "max_wall_ms": 60_000,
                "hard_max_wall_ms": 60_000
            }
        }),
    )
    .expect("plan leg dispatch");
    let raw_tokens = accounted_raw_tokens(&response);
    // Read M_rec from the response telemetry before FastMCP scalar folding can
    // elide structuredContent from the visible wire representation.
    let recovery_tokens = recovery_tokens_from_response(&response);
    let mcp = mcp_tool_response(response);
    let wire_texts = fastmcp_content_texts_from_tool_result(&mcp).expect("fastmcp render");
    let primary = wire_texts.first().cloned().unwrap_or_default();
    let structured_text = wire_texts.get(1).cloned().unwrap_or_default();
    let structured = serde_json::from_str::<Value>(&structured_text).unwrap_or(Value::Null);
    let payload_tokens = structured
        .get("value")
        .and_then(|value| serde_json::to_string(value).ok())
        .map(|text| count_tokens(&text))
        .or_else(|| folded_scalar_payload(&primary).map(|text| count_tokens(&text)))
        .unwrap_or(0);
    let visible_tokens = wire_tokens(&wire_texts);
    let ops = parse_ops_from_ack(&primary);
    PlanMeasurement {
        visible_tokens,
        recovery_tokens,
        payload_tokens,
        envelope_tokens: visible_tokens.saturating_sub(payload_tokens),
        raw_tokens,
        ops,
        wire_texts,
    }
}

fn folded_scalar_payload(ack: &str) -> Option<String> {
    let (_, after_equals) = ack.split_once(" =")?;
    let (value, _) = after_equals.rsplit_once(" t:")?;
    Some(value.to_string())
}

fn parse_ops_from_ack(ack: &str) -> usize {
    ack.split_whitespace()
        .find_map(|part| part.strip_prefix("tz"))
        .and_then(|digits| digits.parse::<usize>().ok())
        .unwrap_or(0)
}

fn measure_perop_leg(engine: &TokenZeroEngine, workload: &Workload) -> PerOpMeasurement {
    let mut visible_tokens = 0usize;
    let mut args_tokens = 0usize;
    let mut raw_tokens = 0usize;
    let mut wire_chunks = Vec::new();
    let mut previous_text = String::new();

    for call in &workload.perop_calls {
        let mut args = call.args.clone();
        if let Some(key) = call.text_from_previous {
            args[key] = Value::String(previous_text.clone());
        }
        args_tokens += count_tokens(&serde_json::to_string(&args).unwrap_or_default());
        let response =
            dispatch_tool(engine, call.canonical, call.name, &args).expect("per-op call");
        raw_tokens += accounted_raw_tokens(&response);
        let mcp = mcp_tool_response(response);
        let contents = fastmcp_content_texts_from_tool_result(&mcp).expect("per-op fastmcp render");
        visible_tokens += wire_tokens(&contents);
        previous_text = contents.first().cloned().unwrap_or_default();
        wire_chunks.push(contents.join("\n"));
    }

    PerOpMeasurement {
        visible_tokens,
        args_tokens,
        raw_tokens,
        wire_text: wire_chunks.join("\n\n"),
    }
}

fn measure_raw_leg(root: &Path, workload: &Workload) -> RawMeasurement {
    let mut visible_tokens = 0usize;
    let mut chunks = Vec::new();
    for command in &workload.raw_commands {
        let output = Command::new(command.program)
            .args(&command.args)
            .current_dir(root)
            .output()
            .expect("raw command");
        let mut text = format!("$ {} {}\n", command.program, command.args.join(" "));
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        visible_tokens += count_tokens(&text);
        chunks.push(text);
    }
    RawMeasurement {
        visible_tokens,
        wire_text: chunks.join("\n"),
    }
}

#[cfg(test)]
#[path = "bench_tests.rs"]
mod bench_harness;
