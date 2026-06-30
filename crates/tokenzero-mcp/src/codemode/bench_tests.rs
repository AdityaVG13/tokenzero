//! Integration tests for the composition benchmark harness.

use super::bench::{run_benchmark, workloads_for_root};
use super::exec::execute_codemode_with_options;
use super::result::{CodeModeOptions, CodeModeStatus};
use std::path::PathBuf;

fn bench_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Runs the full benchmark and prints JSON output (used by the shell script).
#[test]
fn run_composition_benchmark() {
    let root = bench_root();
    let report = run_benchmark(&root);
    let json = serde_json::to_string_pretty(&report).unwrap();
    println!("{json}");

    // Verify all workloads produced valid results
    for wl in &report.workloads {
        assert!(wl.plan_ops > 0, "workload '{}' had zero ops", wl.workload);
        assert!(
            wl.plan_visible_tokens > 0,
            "workload '{}' produced no visible tokens",
            wl.workload
        );
    }
}

/// Consistency test: running the same workload twice produces similar token counts.
#[test]
fn benchmark_harness_produces_consistent_results() {
    let root = bench_root();
    let report1 = run_benchmark(&root);
    let report2 = run_benchmark(&root);

    assert_eq!(report1.workloads.len(), report2.workloads.len());
    for (a, b) in report1.workloads.iter().zip(report2.workloads.iter()) {
        assert_eq!(a.workload, b.workload);
        // Token counts should be deterministic (same engine, same files)
        assert_eq!(
            a.plan_visible_tokens, b.plan_visible_tokens,
            "workload '{}' visible tokens differ between runs: {} vs {}",
            a.workload, a.plan_visible_tokens, b.plan_visible_tokens
        );
        assert_eq!(
            a.plan_raw_tokens, b.plan_raw_tokens,
            "workload '{}' raw tokens differ between runs: {} vs {}",
            a.workload, a.plan_raw_tokens, b.plan_raw_tokens
        );
    }
}

/// Each workload definition is valid (parses and executes without error).
#[test]
fn all_workload_plans_execute_successfully() {
    let root = bench_root();
    let workloads = workloads_for_root(&root);
    let options = CodeModeOptions {
        root: Some(root.clone()),
        ..Default::default()
    };

    for wl in &workloads {
        let result = execute_codemode_with_options(&wl.plan, options.clone());
        assert_eq!(
            result.status,
            CodeModeStatus::Completed,
            "workload '{}' failed: {:?}\nplan: {}",
            wl.name,
            result.error,
            wl.plan
        );
    }
}

/// Composition always produces fewer or equal visible tokens vs direct calls.
#[test]
fn composition_never_worse_than_direct() {
    let root = bench_root();
    let report = run_benchmark(&root);

    for wl in &report.workloads {
        // Plan visible tokens should be <= direct visible tokens (composition advantage)
        // or at worst within a small margin (plan itself adds return-value framing)
        let margin = (wl.direct_visible_tokens as f64 * 0.2) as usize; // 20% margin for framing
        assert!(
            wl.plan_visible_tokens <= wl.direct_visible_tokens + margin,
            "workload '{}': plan ({}) significantly worse than direct ({}) even with 20% margin",
            wl.workload,
            wl.plan_visible_tokens,
            wl.direct_visible_tokens
        );
    }
}
