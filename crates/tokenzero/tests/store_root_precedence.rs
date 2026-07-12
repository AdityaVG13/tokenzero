//! Store-root precedence integration tests for ZeroRef v1 rollout (cqr.6).
//!
//! Freezes and tests precedence across CLI args, per-call root,
//! TOKENZERO_CACHE_PATH, project .zerostack, ZEROSTACK_STORE_ROOT,
//! TOKENZERO_SHARED_STORE, ZEROSTACK_SHARED_STORE, cwd, and
//! missing/noncanonical roots.
//!
//! Tested via `tokenzero doctor --json` because `zerostack_store` is a
//! private module. The doctor surface exposes `store_resolution` JSON
//! containing `effective_cache_path`, `effective_store_root`,
//! `shared_store_opt_in`, `global_pin_set`, `isolation_mode`, and
//! `mismatch_summary`.

use assert_cmd::prelude::*;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

/// Env vars that influence store resolution — cleared per-test for isolation.
const STORE_ENVS: &[&str] = &[
    "TOKENZERO_CACHE_PATH",
    "ZEROSTACK_STORE_ROOT",
    "ZERO_STACK_STORE_ROOT",
    "TOKENZERO_SHARED_STORE",
    "ZEROSTACK_SHARED_STORE",
    "TOKENZERO_ROOT",
];

/// Build a `tokenzero doctor --json` command scoped to `root` with all
/// store-related env vars removed so each test starts from a clean slate.
fn doctor_json(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("tokenzero").unwrap();
    for env in STORE_ENVS {
        cmd.env_remove(env);
    }
    cmd.args(["doctor", "--root", root.to_str().unwrap(), "--json"]);
    cmd
}

/// Like `doctor_json` but also passes `--cache-path <explicit>`.
fn doctor_json_with_cache(root: &Path, explicit_cache: &Path) -> Command {
    let mut cmd = doctor_json(root);
    cmd.args(["--cache-path", explicit_cache.to_str().unwrap()]);
    cmd
}

/// Run the command and extract the `store_resolution` JSON object.
fn store_resolution(cmd: &mut Command) -> Value {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "doctor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    json["store_resolution"].clone()
}

// ──────────────────────────────────────────────────────────────────────────
// 1. CLI arg precedence: --cache-path > TOKENZERO_CACHE_PATH > .zerostack
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn cli_cache_path_beats_env_and_project_local() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    // Project-local .zerostack exists (would be default if nothing else set).
    fs::create_dir_all(root.join(".zerostack/tokenzero")).unwrap();

    let env_cache = root.join("env-cache.json");
    let explicit_cache = root.join("explicit-cache.json");

    let mut cmd = doctor_json_with_cache(root, &explicit_cache);
    cmd.env("TOKENZERO_CACHE_PATH", env_cache.to_str().unwrap());

    let sr = store_resolution(&mut cmd);
    let effective = sr["effective_cache_path"].as_str().unwrap();
    assert_eq!(
        effective,
        explicit_cache.to_str().unwrap(),
        "explicit --cache-path must win over TOKENZERO_CACHE_PATH and .zerostack"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// 2. TOKENZERO_CACHE_PATH overrides project-local .zerostack default
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn tokenzero_cache_path_overrides_project_local() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".zerostack/tokenzero")).unwrap();

    let env_cache = root.join("from-env.json");

    let mut cmd = doctor_json(root);
    cmd.env("TOKENZERO_CACHE_PATH", env_cache.to_str().unwrap());

    let sr = store_resolution(&mut cmd);
    let effective = sr["effective_cache_path"].as_str().unwrap();
    assert_eq!(
        effective,
        env_cache.to_str().unwrap(),
        "TOKENZERO_CACHE_PATH must override the .zerostack default"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// 3. .zerostack directory is detected and used as the store root
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn dot_zerostack_detected_and_used() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".zerostack/tokenzero")).unwrap();

    let sr = store_resolution(&mut doctor_json(root));

    let store_root = sr["effective_store_root"].as_str().unwrap();
    assert!(
        store_root.ends_with(".zerostack"),
        "effective_store_root should be <root>/.zerostack, got {store_root}"
    );

    let cache = sr["effective_cache_path"].as_str().unwrap();
    assert!(
        cache.ends_with(".zerostack/tokenzero/recovery-cache.json"),
        "effective_cache_path should be under .zerostack, got {cache}"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// 4. ZEROSTACK_STORE_ROOT without shared opt-in is ignored (bare global pin)
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn global_pin_without_opt_in_is_ignored() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let shared = tempdir().unwrap();

    let mut cmd = doctor_json(root);
    cmd.env("ZEROSTACK_STORE_ROOT", shared.path().to_str().unwrap());
    // Deliberately do NOT set TOKENZERO_SHARED_STORE or ZEROSTACK_SHARED_STORE.

    let sr = store_resolution(&mut cmd);

    assert!(
        !sr["shared_store_opt_in"].as_bool().unwrap(),
        "shared_store_opt_in must be false without opt-in env"
    );
    assert!(
        sr["global_pin_set"].as_bool().unwrap(),
        "global_pin_set must be true when ZEROSTACK_STORE_ROOT is set"
    );
    assert_eq!(
        sr["isolation_mode"].as_str().unwrap(),
        "per_root",
        "isolation_mode must be per_root when pin is ignored"
    );
    assert!(
        sr["effective_store_root"].is_null(),
        "effective_store_root must be null when pin is ignored and no .zerostack"
    );
    let summary = sr["mismatch_summary"].as_str().unwrap_or_default();
    assert!(
        summary.contains("ignored for isolation"),
        "mismatch_summary should warn about ignored pin, got: {summary}"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// 5a. ZEROSTACK_STORE_ROOT with TOKENZERO_SHARED_STORE is active
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn global_pin_with_tokenzero_shared_store_is_active() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let shared = tempdir().unwrap();

    let mut cmd = doctor_json(root);
    cmd.env("ZEROSTACK_STORE_ROOT", shared.path().to_str().unwrap());
    cmd.env("TOKENZERO_SHARED_STORE", "1");

    let sr = store_resolution(&mut cmd);

    assert!(
        sr["shared_store_opt_in"].as_bool().unwrap(),
        "shared_store_opt_in must be true with TOKENZERO_SHARED_STORE=1"
    );
    assert!(
        sr["global_pin_set"].as_bool().unwrap(),
        "global_pin_set must be true"
    );
    assert_eq!(
        sr["isolation_mode"].as_str().unwrap(),
        "shared_opt_in",
        "isolation_mode must be shared_opt_in"
    );
    let store_root = sr["effective_store_root"].as_str().unwrap();
    assert_eq!(
        store_root,
        shared.path().to_str().unwrap(),
        "effective_store_root must equal the ZEROSTACK_STORE_ROOT value"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// 5b. ZEROSTACK_STORE_ROOT with ZEROSTACK_SHARED_STORE (alternate env) is active
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn global_pin_with_zerostack_shared_store_is_active() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let shared = tempdir().unwrap();

    let mut cmd = doctor_json(root);
    cmd.env("ZEROSTACK_STORE_ROOT", shared.path().to_str().unwrap());
    cmd.env("ZEROSTACK_SHARED_STORE", "true");

    let sr = store_resolution(&mut cmd);

    assert!(
        sr["shared_store_opt_in"].as_bool().unwrap(),
        "shared_store_opt_in must be true with ZEROSTACK_SHARED_STORE=true"
    );
    assert_eq!(
        sr["isolation_mode"].as_str().unwrap(),
        "shared_opt_in",
        "isolation_mode must be shared_opt_in"
    );
    let store_root = sr["effective_store_root"].as_str().unwrap();
    assert_eq!(
        store_root,
        shared.path().to_str().unwrap(),
        "effective_store_root must equal the ZEROSTACK_STORE_ROOT value"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// 5c. .zerostack still wins over shared opt-in (project-local precedence)
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn dot_zerostack_wins_over_shared_opt_in() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let shared = tempdir().unwrap();
    // Project-local .zerostack exists.
    fs::create_dir_all(root.join(".zerostack/tokenzero")).unwrap();

    let mut cmd = doctor_json(root);
    cmd.env("ZEROSTACK_STORE_ROOT", shared.path().to_str().unwrap());
    cmd.env("TOKENZERO_SHARED_STORE", "1");

    let sr = store_resolution(&mut cmd);

    // .zerostack is checked first in resolve_store_root_with_env, so it wins.
    let store_root = sr["effective_store_root"].as_str().unwrap();
    assert!(
        store_root.ends_with(".zerostack"),
        "project-local .zerostack must win over shared opt-in, got {store_root}"
    );
    assert_ne!(
        store_root,
        shared.path().to_str().unwrap(),
        "shared store must not be used when .zerostack exists"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// 6. Missing/noncanonical roots fall back safely
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn missing_store_root_with_opt_in_still_resolves() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    // Point to a path that does not exist on disk.
    let ghost = root.join("does-not-exist").join("shared-store");

    let mut cmd = doctor_json(root);
    cmd.env("ZEROSTACK_STORE_ROOT", ghost.to_str().unwrap());
    cmd.env("TOKENZERO_SHARED_STORE", "1");

    let sr = store_resolution(&mut cmd);

    // resolve_store_root_with_env does not check existence of the pin path;
    // it returns it as-is. The store root should be the (nonexistent) path.
    let store_root = sr["effective_store_root"].as_str().unwrap();
    assert_eq!(
        store_root,
        ghost.to_str().unwrap(),
        "nonexistent pin path should still be resolved as the store root"
    );
}

#[test]
fn no_zerostack_no_pin_falls_back_to_legacy_tokenzero() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    // No .zerostack, no ZEROSTACK_STORE_ROOT, no shared opt-in.

    let sr = store_resolution(&mut doctor_json(root));

    assert!(
        sr["effective_store_root"].is_null(),
        "effective_store_root must be null when no .zerostack and no pin"
    );
    let cache = sr["effective_cache_path"].as_str().unwrap();
    assert!(
        cache.ends_with(".tokenzero/recovery-cache.json"),
        "should fall back to legacy .tokenzero path, got {cache}"
    );
}

#[test]
fn relative_store_root_resolves_against_repo_root() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    // Relative path — should be joined to repo_root per resolve_store_root_with_env.
    let rel = "custom-shared-store";

    let mut cmd = doctor_json(root);
    cmd.env("ZEROSTACK_STORE_ROOT", rel);
    cmd.env("TOKENZERO_SHARED_STORE", "1");

    let sr = store_resolution(&mut cmd);

    let store_root = sr["effective_store_root"].as_str().unwrap();
    assert!(
        store_root.ends_with(rel),
        "relative pin should be joined to repo root, got {store_root}"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// 7. Two roots with same basename don't collide
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn two_roots_same_basename_no_collision() {
    let parent_a = tempdir().unwrap();
    let parent_b = tempdir().unwrap();

    // Both projects have a subdirectory with the same basename.
    let proj_a = parent_a.path().join("myproject");
    let proj_b = parent_b.path().join("myproject");
    fs::create_dir_all(proj_a.join(".zerostack/tokenzero")).unwrap();
    fs::create_dir_all(proj_b.join(".zerostack/tokenzero")).unwrap();

    let sr_a = store_resolution(&mut doctor_json(&proj_a));
    let sr_b = store_resolution(&mut doctor_json(&proj_b));

    let store_a = sr_a["effective_store_root"].as_str().unwrap();
    let store_b = sr_b["effective_store_root"].as_str().unwrap();

    assert_ne!(
        store_a, store_b,
        "two projects with same basename must not share a store root"
    );
    assert!(store_a.starts_with(proj_a.to_str().unwrap()));
    assert!(store_b.starts_with(proj_b.to_str().unwrap()));

    let cache_a = sr_a["effective_cache_path"].as_str().unwrap();
    let cache_b = sr_b["effective_cache_path"].as_str().unwrap();
    assert_ne!(cache_a, cache_b, "cache paths must not collide");
}

// ──────────────────────────────────────────────────────────────────────────
// 8. Cwd fallback: without --root, doctor uses current working directory
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn cwd_fallback_when_no_root_arg() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".zerostack/tokenzero")).unwrap();

    let mut cmd = Command::cargo_bin("tokenzero").unwrap();
    for env in STORE_ENVS {
        cmd.env_remove(env);
    }
    cmd.args(["doctor", "--json"]);
    cmd.current_dir(root);

    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "doctor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let sr = &json["store_resolution"];

    let store_root = sr["effective_store_root"].as_str().unwrap();
    assert!(
        store_root.ends_with(".zerostack"),
        "cwd-based root should detect .zerostack, got {store_root}"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// 9. Alternate global pin env ZERO_STACK_STORE_ROOT (legacy spelling)
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn legacy_store_root_env_spelling_with_opt_in() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let shared = tempdir().unwrap();

    let mut cmd = doctor_json(root);
    cmd.env("ZERO_STACK_STORE_ROOT", shared.path().to_str().unwrap());
    cmd.env("ZEROSTACK_SHARED_STORE", "yes");

    let sr = store_resolution(&mut cmd);

    assert!(
        sr["shared_store_opt_in"].as_bool().unwrap(),
        "ZEROSTACK_SHARED_STORE=yes should opt in"
    );
    let store_root = sr["effective_store_root"].as_str().unwrap();
    assert_eq!(
        store_root,
        shared.path().to_str().unwrap(),
        "ZERO_STACK_STORE_ROOT (legacy spelling) should be honored with opt-in"
    );
}
