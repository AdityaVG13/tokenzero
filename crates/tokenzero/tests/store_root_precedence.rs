//! Store-root precedence integration tests for ZeroRef v1 rollout (cqr.6).

use assert_cmd::prelude::*;
use serde_json::Value;
use std::{fs, path::Path, process::Command};
use tempfile::tempdir;

const STORE_ENVS: &[&str] = &[
    "TOKENZERO_CACHE_PATH", "ZEROSTACK_STORE_ROOT", "ZERO_STACK_STORE_ROOT",
    "TOKENZERO_SHARED_STORE", "ZEROSTACK_SHARED_STORE", "TOKENZERO_ROOT",
];

fn doctor(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("tokenzero").unwrap();
    for env in STORE_ENVS { cmd.env_remove(env); }
    cmd.args(["doctor", "--root", root.to_str().unwrap(), "--json"]);
    cmd
}

fn store_resolution(mut cmd: Command) -> Value {
    let output = cmd.output().unwrap();
    assert!(output.status.success(), "doctor failed: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["store_resolution"].clone()
}

fn resolve(root: &Path, envs: &[(&str, &str)], args: &[&str]) -> Value {
    let mut cmd = doctor(root);
    cmd.envs(envs.iter().copied()).args(args);
    store_resolution(cmd)
}

fn mk_zs(root: &Path) { fs::create_dir_all(root.join(".zerostack/tokenzero")).unwrap(); }

macro_rules! root_test {
    ($name:ident, |$root:ident| $body:block) => {
        #[test]
        fn $name() {
            let dir = tempdir().unwrap();
            let $root = dir.path();
            $body
        }
    };
}

root_test!(cli_cache_path_beats_env_and_project_local, |root| {
    mk_zs(root);
    let env_cache = root.join("env-cache.json");
    let explicit = root.join("explicit-cache.json");
    let sr = resolve(root, &[("TOKENZERO_CACHE_PATH", env_cache.to_str().unwrap())], &["--cache-path", explicit.to_str().unwrap()]);
    assert_eq!(sr["effective_cache_path"], explicit.to_str().unwrap(), "explicit --cache-path must win over TOKENZERO_CACHE_PATH and .zerostack");
});

root_test!(tokenzero_cache_path_overrides_project_local, |root| {
    mk_zs(root);
    let env_cache = root.join("from-env.json");
    let sr = resolve(root, &[("TOKENZERO_CACHE_PATH", env_cache.to_str().unwrap())], &[]);
    assert_eq!(sr["effective_cache_path"], env_cache.to_str().unwrap(), "TOKENZERO_CACHE_PATH must override the .zerostack default");
});

root_test!(dot_zerostack_detected_and_used, |root| {
    mk_zs(root);
    let sr = resolve(root, &[], &[]);
    let store = sr["effective_store_root"].as_str().unwrap();
    assert!(store.ends_with(".zerostack"), "effective_store_root should be <root>/.zerostack, got {store}");
    let cache = sr["effective_cache_path"].as_str().unwrap();
    assert!(cache.ends_with(".zerostack/tokenzero/recovery-cache.json"), "effective_cache_path should be under .zerostack, got {cache}");
});

root_test!(global_pin_without_opt_in_is_ignored, |root| {
    let shared = tempdir().unwrap();
    let sr = resolve(root, &[("ZEROSTACK_STORE_ROOT", shared.path().to_str().unwrap())], &[]);
    assert!(!sr["shared_store_opt_in"].as_bool().unwrap(), "shared_store_opt_in must be false without opt-in env");
    assert!(sr["global_pin_set"].as_bool().unwrap(), "global_pin_set must be true when ZEROSTACK_STORE_ROOT is set");
    assert_eq!(sr["isolation_mode"], "per_root", "isolation_mode must be per_root when pin is ignored");
    assert!(sr["effective_store_root"].is_null(), "effective_store_root must be null when pin is ignored and no .zerostack");
    let summary = sr["mismatch_summary"].as_str().unwrap_or_default();
    assert!(summary.contains("ignored for isolation"), "mismatch_summary should warn about ignored pin, got: {summary}");
});

fn assert_shared_active(sr: &Value, shared: &Path, opt_in_msg: &str) {
    assert!(sr["shared_store_opt_in"].as_bool().unwrap(), "{opt_in_msg}");
    assert_eq!(sr["isolation_mode"], "shared_opt_in", "isolation_mode must be shared_opt_in");
    assert_eq!(sr["effective_store_root"], shared.to_str().unwrap(), "effective_store_root must equal the ZEROSTACK_STORE_ROOT value");
}

root_test!(global_pin_with_tokenzero_shared_store_is_active, |root| {
    let shared = tempdir().unwrap();
    let sr = resolve(root, &[
        ("ZEROSTACK_STORE_ROOT", shared.path().to_str().unwrap()),
        ("TOKENZERO_SHARED_STORE", "1"),
    ], &[]);
    assert!(sr["global_pin_set"].as_bool().unwrap(), "global_pin_set must be true");
    assert_shared_active(&sr, shared.path(), "shared_store_opt_in must be true with TOKENZERO_SHARED_STORE=1");
});

root_test!(global_pin_with_zerostack_shared_store_is_active, |root| {
    let shared = tempdir().unwrap();
    let sr = resolve(root, &[
        ("ZEROSTACK_STORE_ROOT", shared.path().to_str().unwrap()),
        ("ZEROSTACK_SHARED_STORE", "true"),
    ], &[]);
    assert_shared_active(&sr, shared.path(), "shared_store_opt_in must be true with ZEROSTACK_SHARED_STORE=true");
});

root_test!(dot_zerostack_wins_over_shared_opt_in, |root| {
    let shared = tempdir().unwrap();
    mk_zs(root);
    let sr = resolve(root, &[
        ("ZEROSTACK_STORE_ROOT", shared.path().to_str().unwrap()),
        ("TOKENZERO_SHARED_STORE", "1"),
    ], &[]);
    let store = sr["effective_store_root"].as_str().unwrap();
    assert!(store.ends_with(".zerostack"), "project-local .zerostack must win over shared opt-in, got {store}");
    assert_ne!(store, shared.path().to_str().unwrap(), "shared store must not be used when .zerostack exists");
});

root_test!(missing_store_root_with_opt_in_still_resolves, |root| {
    let ghost = root.join("does-not-exist/shared-store");
    let sr = resolve(root, &[
        ("ZEROSTACK_STORE_ROOT", ghost.to_str().unwrap()),
        ("TOKENZERO_SHARED_STORE", "1"),
    ], &[]);
    assert_eq!(sr["effective_store_root"], ghost.to_str().unwrap(), "nonexistent pin path should still be resolved as the store root");
});

root_test!(no_zerostack_no_pin_falls_back_to_legacy_tokenzero, |root| {
    let sr = resolve(root, &[], &[]);
    assert!(sr["effective_store_root"].is_null(), "effective_store_root must be null when no .zerostack and no pin");
    let cache = sr["effective_cache_path"].as_str().unwrap();
    assert!(cache.ends_with(".tokenzero/recovery-cache.json"), "should fall back to legacy .tokenzero path, got {cache}");
});

root_test!(relative_store_root_resolves_against_repo_root, |root| {
    let rel = "custom-shared-store";
    let sr = resolve(root, &[("ZEROSTACK_STORE_ROOT", rel), ("TOKENZERO_SHARED_STORE", "1")], &[]);
    let store = sr["effective_store_root"].as_str().unwrap();
    assert!(store.ends_with(rel), "relative pin should be joined to repo root, got {store}");
});

#[test]
fn two_roots_same_basename_no_collision() {
    let parent_a = tempdir().unwrap();
    let parent_b = tempdir().unwrap();
    let proj_a = parent_a.path().join("myproject");
    let proj_b = parent_b.path().join("myproject");
    mk_zs(&proj_a);
    mk_zs(&proj_b);
    let sr_a = resolve(&proj_a, &[], &[]);
    let sr_b = resolve(&proj_b, &[], &[]);
    let store_a = sr_a["effective_store_root"].as_str().unwrap();
    let store_b = sr_b["effective_store_root"].as_str().unwrap();
    assert_ne!(store_a, store_b, "two projects with same basename must not share a store root");
    assert!(store_a.starts_with(proj_a.to_str().unwrap()));
    assert!(store_b.starts_with(proj_b.to_str().unwrap()));
    assert_ne!(sr_a["effective_cache_path"], sr_b["effective_cache_path"], "cache paths must not collide");
}

root_test!(cwd_fallback_when_no_root_arg, |root| {
    mk_zs(root);
    let mut cmd = Command::cargo_bin("tokenzero").unwrap();
    for env in STORE_ENVS { cmd.env_remove(env); }
    cmd.args(["doctor", "--json"]).current_dir(root);
    let sr = store_resolution(cmd);
    let store = sr["effective_store_root"].as_str().unwrap();
    assert!(store.ends_with(".zerostack"), "cwd-based root should detect .zerostack, got {store}");
});

root_test!(legacy_store_root_env_spelling_with_opt_in, |root| {
    let shared = tempdir().unwrap();
    let sr = resolve(root, &[
        ("ZERO_STACK_STORE_ROOT", shared.path().to_str().unwrap()),
        ("ZEROSTACK_SHARED_STORE", "yes"),
    ], &[]);
    assert!(sr["shared_store_opt_in"].as_bool().unwrap(), "ZEROSTACK_SHARED_STORE=yes should opt in");
    assert_eq!(sr["effective_store_root"], shared.path().to_str().unwrap(), "ZERO_STACK_STORE_ROOT (legacy spelling) should be honored with opt-in");
});
