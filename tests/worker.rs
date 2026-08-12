//! Bounded release proof for the planner-free raw worker.

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

/// True when the runner exported the worker binary path. See tests/ship.rs
/// for why ship tests skip gracefully under a plain workspace test run.
fn ship_env_set() -> bool {
    std::env::var_os("TOKENZERO_SHIP_WORKER_BIN").is_some()
}

#[test]
fn ship_worker_probe_is_planner_free_and_contract_bound() {
    if !ship_env_set() {
        eprintln!("skipped: TOKENZERO_SHIP_WORKER_BIN not set");
        return;
    }
    let binary = std::env::var_os("TOKENZERO_SHIP_WORKER_BIN")
        .map(PathBuf::from)
        .expect("TOKENZERO_SHIP_WORKER_BIN is set by scripts/run_ship_suite.py");
    let output = Command::new(binary)
        .args(["raw-worker", "--handshake"])
        .env("NO_COLOR", "1")
        .env("CI", "true")
        .output()
        .expect("worker probe");
    let value: Value = serde_json::from_slice(&output.stdout).expect("probe JSON");
    let oracle = |value: &Value| {
        value["surface"] == "raw_worker"
            && value["planner_owner"] == "client"
            && value["compression_owner"] == "engine"
            && value["raw_worker_version"] == "zerostack.raw_worker.v2"
            && value["semantic_contract_digest"]
                .as_str()
                .is_some_and(|digest| digest.len() == 64)
    };
    assert!(
        output.status.success() && output.stderr.is_empty() && oracle(&value),
        "worker handshake must be planner-free and contract-bound"
    );
}
