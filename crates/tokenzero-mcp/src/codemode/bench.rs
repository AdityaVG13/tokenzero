//! CodeMode plan composition benchmark harness.
//!
//! Measures end-to-end efficiency of plan-based execution versus equivalent
//! sequences of individual operations. Produces machine-readable JSON output.


use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

use super::exec::execute_codemode_with_options;
use super::result::{CodeModeOptions, CodeModeStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub workload: String,
    pub description: String,
    pub plan_ops: usize,
    pub equivalent_direct_calls: usize,
    pub plan_visible_tokens: usize,
    pub plan_raw_tokens: usize,
    pub direct_visible_tokens: usize,
    pub direct_raw_tokens: usize,
    pub plan_duration_ms: u64,
    pub direct_duration_ms: u64,
    pub composition_savings_pct: f64,
    pub round_trip_savings_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub version: String,
    pub workloads: Vec<BenchmarkResult>,
    pub totals: BenchmarkTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkTotals {
    pub total_plan_visible: usize,
    pub total_plan_raw: usize,
    pub total_direct_visible: usize,
    pub total_direct_raw: usize,
    pub overall_composition_savings_pct: f64,
}

pub struct Workload {
    pub name: String,
    pub description: String,
    pub plan: String,
    pub direct_calls: Vec<String>,
}

pub fn workloads_for_root(root: &std::path::Path) -> Vec<Workload> {
    let root_str = root.to_string_lossy();

    vec![
        // Workload 1: File + search + transform pattern
        Workload {
            name: "file-search-transform".to_string(),
            description: "Read a file, grep for pattern, filter results, compact output"
                .to_string(),
            plan: format!(
                r#"const f = await zero.read("{root_str}/Cargo.toml"); const hits = await zero.grep("version", "{root_str}"); const filtered = await zero.filter_lines(hits.text, "tokenzero"); return {{ file_tokens: f.visible_tokens, grep_tokens: hits.visible_tokens, filtered: filtered }}"#,
            ),
            direct_calls: vec![
                format!(r#"await zero.read("{root_str}/Cargo.toml")"#),
                format!(r#"await zero.grep("version", "{root_str}")"#),
                format!(r#"await zero.grep("version", "{root_str}")"#), // re-grep for filter
            ],
        },
        // Workload 2: Shell-heavy multi-step
        Workload {
            name: "shell-multi-step".to_string(),
            description: "Run multiple shell commands and aggregate results in one plan"
                .to_string(),
            plan: format!(
                r#"const v = await zero.shell("git --version"); const s = await zero.shell("git -C {root_str} log --oneline -5"); const d = await zero.shell("git -C {root_str} diff --stat HEAD~1 2>/dev/null || echo 'no diff'"); return {{ git_version: v.text, log: s.text, diff: d.text }}"#,
            ),
            direct_calls: vec![
                r#"await zero.shell("git --version")"#.to_string(),
                format!(r#"await zero.shell("git -C {root_str} log --oneline -5")"#),
                format!(
                    r#"await zero.shell("git -C {root_str} diff --stat HEAD~1 2>/dev/null || echo 'no diff'")"#
                ),
            ],
        },
        // Workload 3: Pipe composition (sequential with auto-binding)
        Workload {
            name: "pipe-composition".to_string(),
            description:
                "Pipe: read then compact then expand, demonstrating zero-roundtrip chaining"
                    .to_string(),
            plan: format!(
                r#"await zero.pipe([{{"method": "zero.read", "args": ["{root_str}/Cargo.toml"]}}, {{"method": "zero.compact", "args": ["_prev.text"]}}])"#,
            ),
            direct_calls: vec![
                format!(r#"await zero.read("{root_str}/Cargo.toml")"#),
                r#"await zero.compact("<result of previous call>")"#.to_string(),
            ],
        },
        // Workload 4: Mixed read + tree + search aggregation
        Workload {
            name: "mixed-exploration".to_string(),
            description: "Explore project structure: tree + glob + targeted reads in one plan"
                .to_string(),
            plan: format!(
                r#"const t = await zero.tree("{root_str}/crates", {{ depth: 2 }}); const g = await zero.glob("*.toml", "{root_str}/crates"); const r = await zero.read("{root_str}/Cargo.toml"); return {{ tree_lines: t.text, toml_files: g.text, root_manifest: r.visible_tokens }}"#,
            ),
            direct_calls: vec![
                format!(r#"await zero.tree("{root_str}/crates", {{ depth: 2 }})"#),
                format!(r#"await zero.glob("*.toml", "{root_str}/crates")"#),
                format!(r#"await zero.read("{root_str}/Cargo.toml")"#),
            ],
        },
    ]
}

pub fn run_benchmark(root: &std::path::Path) -> BenchmarkReport {
    let workloads = workloads_for_root(root);
    let options_base = CodeModeOptions {
        root: Some(root.to_path_buf()),
        ..Default::default()
    };

    let mut results = Vec::new();

    for wl in &workloads {
        // Run composed plan
        let start = Instant::now();
        let plan_result = execute_codemode_with_options(&wl.plan, options_base.clone());
        let plan_ms = start.elapsed().as_millis() as u64;

        let plan_visible = plan_result.telemetry.visible_tokens;
        let plan_raw = plan_result.telemetry.raw_tokens;
        let plan_ops = plan_result.telemetry.operations;

        // Run equivalent direct calls separately
        let direct_start = Instant::now();
        let mut direct_visible: usize = 0;
        let mut direct_raw: usize = 0;
        for call in &wl.direct_calls {
            let r = execute_codemode_with_options(call, options_base.clone());
            direct_visible += r.telemetry.visible_tokens;
            direct_raw += r.telemetry.raw_tokens;
        }
        let direct_ms = direct_start.elapsed().as_millis() as u64;

        let composition_savings = if direct_visible > 0 {
            (1.0 - (plan_visible as f64 / direct_visible as f64)) * 100.0
        } else {
            0.0
        };
        let round_trip_savings = if direct_raw > 0 {
            (1.0 - (plan_raw as f64 / direct_raw as f64)) * 100.0
        } else {
            0.0
        };

        results.push(BenchmarkResult {
            workload: wl.name.clone(),
            description: wl.description.clone(),
            plan_ops,
            equivalent_direct_calls: wl.direct_calls.len(),
            plan_visible_tokens: plan_visible,
            plan_raw_tokens: plan_raw,
            direct_visible_tokens: direct_visible,
            direct_raw_tokens: direct_raw,
            plan_duration_ms: plan_ms,
            direct_duration_ms: direct_ms,
            composition_savings_pct: (composition_savings * 10.0).round() / 10.0,
            round_trip_savings_pct: (round_trip_savings * 10.0).round() / 10.0,
        });
    }

    let total_plan_visible: usize = results.iter().map(|r| r.plan_visible_tokens).sum();
    let total_plan_raw: usize = results.iter().map(|r| r.plan_raw_tokens).sum();
    let total_direct_visible: usize = results.iter().map(|r| r.direct_visible_tokens).sum();
    let total_direct_raw: usize = results.iter().map(|r| r.direct_raw_tokens).sum();

    let overall_savings = if total_direct_visible > 0 {
        (1.0 - (total_plan_visible as f64 / total_direct_visible as f64)) * 100.0
    } else {
        0.0
    };

    BenchmarkReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        workloads: results,
        totals: BenchmarkTotals {
            total_plan_visible,
            total_plan_raw,
            total_direct_visible,
            total_direct_raw,
            overall_composition_savings_pct: (overall_savings * 10.0).round() / 10.0,
        },
    }
}
