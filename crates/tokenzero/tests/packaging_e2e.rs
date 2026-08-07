//! Installer e2e for tokenzero-irx9.3 (macOS/Linux + platform simulation).
//!
//! Exercises packaging/install.sh with --skip-build against prebuilt single-surface
//! binaries: fresh install, replacement, upgrade, rollback, uninstall, dual fail-closed.

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn install_sh() -> PathBuf {
    repo_root().join("packaging/install.sh")
}

fn cargo_surface_binary(surface: &str) -> PathBuf {
    let (override_name, cargo_binary) = match surface {
        "codemode" => (
            "TOKENZERO_TEST_CODEMODE_BIN",
            env!("CARGO_BIN_EXE_tokenzero-codemode"),
        ),
        _ => (
            "TOKENZERO_TEST_MCP_BIN",
            env!("CARGO_BIN_EXE_tokenzero-mcp"),
        ),
    };
    std::env::var_os(override_name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(cargo_binary))
}

fn ensure_bins() {
    let root = repo_root();
    for (bin, feature) in [
        ("tokenzero-mcp", "surface-mcp"),
        ("tokenzero-codemode", "surface-codemode"),
    ] {
        let out = root.join("target/debug").join(bin);
        if out.is_file() {
            continue;
        }
        let status = Command::new("cargo")
            .args([
                "build",
                "-p",
                "tokenzero",
                "--bin",
                bin,
                "--no-default-features",
                "--features",
                feature,
            ])
            .current_dir(&root)
            .status()
            .expect("cargo build");
        assert!(status.success(), "build {bin} failed");
    }
}

fn run_install(
    surface: &str,
    prefix: &Path,
    bin_dir: &Path,
    platform: &str,
) -> (i32, String, String) {
    let out = Command::new("bash")
        .arg(install_sh())
        .arg("--surface")
        .arg(surface)
        .arg("--prefix")
        .arg(prefix)
        .arg("--bin-dir")
        .arg(bin_dir)
        .arg("--skip-build")
        .env("TOKENZERO_INSTALL_PLATFORM", platform)
        .env("TOKENZERO_INSTALL_SRC", cargo_surface_binary(surface))
        .env_remove("TOKENZERO_TELEMETRY")
        .env("HOME", prefix)
        .current_dir(repo_root())
        .output()
        .expect("install.sh");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn run_uninstall(prefix: &Path, bin_dir: &Path, platform: &str) -> (i32, String, String) {
    let out = Command::new("bash")
        .arg(install_sh())
        .arg("--uninstall")
        .arg("--prefix")
        .arg(prefix)
        .arg("--bin-dir")
        .arg(bin_dir)
        .env("TOKENZERO_INSTALL_PLATFORM", platform)
        .current_dir(repo_root())
        .output()
        .expect("uninstall");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

#[cfg(unix)]
struct PriorInstall {
    peer_bytes: Vec<u8>,
    state_bytes: Vec<u8>,
    config_bytes: Vec<u8>,
    shim_target_bytes: Vec<u8>,
    peer_mode: u32,
    state_mode: u32,
    config_mode: u32,
    shim_target_mode: u32,
    compat_target: PathBuf,
}

#[cfg(unix)]
fn write_mode(path: &Path, bytes: &[u8], mode: u32) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
fn seed_prior_codemode_install(prefix: &Path, bin_dir: &Path) -> PriorInstall {
    fs::create_dir_all(prefix).unwrap();
    fs::create_dir_all(bin_dir).unwrap();
    let peer = bin_dir.join("tokenzero-codemode");
    let compat = bin_dir.join("tokenzero");
    let state = prefix.join("install-state.json");
    let config = prefix.join("client-config.json");
    let shim_target = prefix.join("shim-target");
    let peer_bytes = b"#!/bin/sh\nprintf 'prior-peer\\n'\n".to_vec();
    let mut state_bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "surface": "codemode",
        "artifact": "tokenzero-codemode",
        "binary_path": peer.display().to_string(),
        "prefix": prefix.display().to_string(),
        "semantic_contract_digest": "0".repeat(64),
        "package_version": "prior",
        "installed_at_unix": 1,
        "platform": "linux",
        "client_config": prefix.join("client-config.json").display().to_string(),
        "sentinel": "prior-state",
    }))
    .unwrap();
    state_bytes.push(b'\n');
    let config_bytes = b"{\"surface\":\"codemode\",\"args\":[\"--mode=codemode\"],\"sentinel\":\"prior-config\"}\n".to_vec();
    let shim_target_bytes = b"codemode\n".to_vec();
    let peer_mode = 0o751;
    let state_mode = 0o640;
    let config_mode = 0o600;
    let shim_target_mode = 0o644;
    write_mode(&peer, &peer_bytes, peer_mode);
    write_mode(&state, &state_bytes, state_mode);
    write_mode(&config, &config_bytes, config_mode);
    write_mode(&shim_target, &shim_target_bytes, shim_target_mode);
    symlink(&peer, &compat).unwrap();
    PriorInstall {
        peer_bytes,
        state_bytes,
        config_bytes,
        shim_target_bytes,
        peer_mode,
        state_mode,
        config_mode,
        shim_target_mode,
        compat_target: peer,
    }
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
fn assert_prior_install_restored(
    prefix: &Path,
    bin_dir: &Path,
    prior: &PriorInstall,
    selected_artifact: Option<(&[u8], u32)>,
) {
    let selected = bin_dir.join("tokenzero-mcp");
    let peer = bin_dir.join("tokenzero-codemode");
    let compat = bin_dir.join("tokenzero");
    let state = prefix.join("install-state.json");
    let config = prefix.join("client-config.json");
    let shim_target = prefix.join("shim-target");
    match selected_artifact {
        Some((bytes, expected_mode)) => {
            assert_eq!(fs::read(&selected).unwrap(), bytes);
            assert_eq!(mode(&selected), expected_mode);
        }
        None => assert!(!selected.exists()),
    }
    assert_eq!(fs::read(&peer).unwrap(), prior.peer_bytes);
    assert_eq!(fs::read(&state).unwrap(), prior.state_bytes);
    assert_eq!(fs::read(&config).unwrap(), prior.config_bytes);
    assert_eq!(fs::read(&shim_target).unwrap(), prior.shim_target_bytes);
    assert_eq!(mode(&peer), prior.peer_mode);
    assert_eq!(mode(&state), prior.state_mode);
    assert_eq!(mode(&config), prior.config_mode);
    assert_eq!(mode(&shim_target), prior.shim_target_mode);
    assert!(
        fs::symlink_metadata(&compat)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(&compat).unwrap(), prior.compat_target);
    assert!(fs::read_dir(prefix).unwrap().flatten().all(|entry| {
        !entry
            .file_name()
            .to_string_lossy()
            .starts_with(".install-rollback.")
    }));
    assert!(
        fs::read_dir(bin_dir)
            .unwrap()
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().contains(".candidate."))
    );
}

#[cfg(unix)]
#[test]
fn unverified_candidate_preserves_installed_peer_and_state() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("prefix");
    let bin_dir = tmp.path().join("bin");
    let prior = seed_prior_codemode_install(&prefix, &bin_dir);
    let bad_candidate = tmp.path().join("bad-tokenzero-mcp");
    write_mode(
        &bad_candidate,
        b"#!/bin/sh\nprintf 'not-an-sbom\\n'\n",
        0o755,
    );

    let output = Command::new("bash")
        .arg(install_sh())
        .args(["--surface", "mcp", "--skip-build"])
        .arg("--prefix")
        .arg(&prefix)
        .arg("--bin-dir")
        .arg(&bin_dir)
        .env("TOKENZERO_INSTALL_SRC", &bad_candidate)
        .env("TOKENZERO_INSTALL_PLATFORM", "linux")
        .env("HOME", &prefix)
        .current_dir(repo_root())
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("candidate SBOM digest"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_prior_install_restored(&prefix, &bin_dir, &prior, None);
}

#[cfg(unix)]
#[test]
fn post_replace_failure_restores_binary_peer_json_modes_and_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("prefix");
    let bin_dir = tmp.path().join("bin");
    let prior = seed_prior_codemode_install(&prefix, &bin_dir);
    let prior_selected = b"#!/bin/sh\nprintf 'prior-selected\\n'\n".to_vec();
    let prior_selected_mode = 0o711;
    write_mode(
        &bin_dir.join("tokenzero-mcp"),
        &prior_selected,
        prior_selected_mode,
    );
    let candidate = cargo_surface_binary("mcp");

    let real_python_output = Command::new("sh")
        .args(["-c", "command -v python3"])
        .output()
        .unwrap();
    assert!(real_python_output.status.success());
    let real_python = String::from_utf8(real_python_output.stdout)
        .unwrap()
        .trim()
        .to_string();
    let fake_bin = tmp.path().join("fake-bin");
    fs::create_dir(&fake_bin).unwrap();
    let python_wrapper = fake_bin.join("python3");
    write_mode(
        &python_wrapper,
        b"#!/bin/sh\ncount=0\nif [ -f \"$PY_COUNT_FILE\" ]; then count=$(cat \"$PY_COUNT_FILE\"); fi\ncount=$((count + 1))\nprintf '%s\\n' \"$count\" >\"$PY_COUNT_FILE\"\nif [ \"$count\" -ge 3 ]; then exit 97; fi\nexec \"$REAL_PYTHON\" \"$@\"\n",
        0o755,
    );
    let search_path = std::env::join_paths(std::iter::once(fake_bin.clone()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let count_file = tmp.path().join("python-count");

    let output = Command::new("bash")
        .arg(install_sh())
        .args(["--surface", "mcp", "--skip-build"])
        .arg("--prefix")
        .arg(&prefix)
        .arg("--bin-dir")
        .arg(&bin_dir)
        .env("TOKENZERO_INSTALL_SRC", &candidate)
        .env("TOKENZERO_INSTALL_PLATFORM", "linux")
        .env("REAL_PYTHON", real_python)
        .env("PY_COUNT_FILE", &count_file)
        .env("PATH", search_path)
        .env("HOME", &prefix)
        .current_dir(repo_root())
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("client/state JSON malformed or inconsistent"),
        "{stderr}"
    );
    assert!(stderr.contains("restoring exact prior"), "{stderr}");
    assert_eq!(fs::read_to_string(&count_file).unwrap().trim(), "3");
    assert_prior_install_restored(
        &prefix,
        &bin_dir,
        &prior,
        Some((&prior_selected, prior_selected_mode)),
    );
}

#[test]
fn invalid_platform_override_fails_before_state_mutation() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("prefix");
    let binary = cargo_surface_binary("mcp");
    let output = Command::new(&binary)
        .args(["install", "--surface", "mcp", "--prefix"])
        .arg(&prefix)
        .arg("--binary")
        .arg(&binary)
        .env("TOKENZERO_INSTALL_PLATFORM", "freebsd")
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("macos, linux, windows"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!prefix.join("client-config.json").exists());
    assert!(!prefix.join("install-state.json").exists());
    assert!(!prefix.join("shim-target").exists());
}

#[cfg(unix)]
#[test]
fn installer_serializes_adversarial_paths_as_structured_json() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("prefix-\"quote\\slash\nline");
    let bin_dir = tmp.path().join("bin-\"quote\\slash\nline");
    let candidate = cargo_surface_binary("mcp");

    let output = Command::new("bash")
        .arg(install_sh())
        .args(["--surface", "mcp", "--skip-build"])
        .arg("--prefix")
        .arg(&prefix)
        .arg("--bin-dir")
        .arg(&bin_dir)
        .env("TOKENZERO_INSTALL_SRC", &candidate)
        .env("TOKENZERO_INSTALL_PLATFORM", "linux")
        .env("HOME", &prefix)
        .current_dir(repo_root())
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let client_raw = fs::read_to_string(prefix.join("client-config.json")).unwrap();
    let state_raw = fs::read_to_string(prefix.join("install-state.json")).unwrap();
    let client: serde_json::Value = serde_json::from_str(&client_raw).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state_raw).unwrap();
    let installed = bin_dir.join("tokenzero-mcp").display().to_string();
    assert_eq!(client["command"], installed);
    let digest = client["semantic_contract_digest"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    assert!(digest.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert_eq!(state["semantic_contract_digest"], digest);
    assert_eq!(state["binary_path"], installed);
    assert_eq!(state["prefix"], prefix.display().to_string());
    assert_eq!(
        state["client_config"],
        prefix.join("client-config.json").display().to_string()
    );
}

#[test]
fn installer_e2e_fresh_replace_upgrade_rollback_uninstall() {
    ensure_bins();
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("prefix");
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    for platform in ["macos", "linux"] {
        let (code, stdout, stderr) = run_install("mcp", &prefix, &bin_dir, platform);
        assert_eq!(code, 0, "fresh mcp platform={platform}: {stdout}\n{stderr}");
        assert!(stdout.contains("install: ok"), "{stdout}");
        assert!(stdout.contains(&format!("platform={platform}")), "{stdout}");
        let state = read(&prefix.join("install-state.json"));
        assert!(state.contains("\"mcp\""), "{state}");
        assert!(state.contains(&format!("\"{platform}\"")), "{state}");
        let cfg = read(&prefix.join("client-config.json"));
        assert!(cfg.contains("--mode=mcp"), "{cfg}");
        assert!(!cfg.contains("--mode=codemode"), "{cfg}");
        assert!(bin_dir.join("tokenzero-mcp").exists());
        assert!(bin_dir.join("tokenzero").exists());
        let first_run = Command::new(bin_dir.join("tokenzero-mcp"))
            .arg("--help")
            .env_remove("TOKENZERO_TELEMETRY")
            .env("HOME", &prefix)
            .output()
            .expect("fresh-installed tokenzero-mcp --help");
        assert!(
            first_run.status.success(),
            "first run failed: {first_run:?}"
        );
        for telemetry in [
            prefix.join("usage-telemetry.jsonl"),
            prefix.join("token-amplification.jsonl"),
            prefix.join(".tokenzero/usage-telemetry.jsonl"),
            prefix.join(".tokenzero/pulse/events.jsonl"),
        ] {
            assert!(
                !telemetry.exists(),
                "fresh default-off install wrote {}",
                telemetry.display()
            );
        }

        // Replace with codemode (upgrade / peer replace).
        let (code, stdout, stderr) = run_install("codemode", &prefix, &bin_dir, platform);
        assert_eq!(code, 0, "replace codemode: {stdout}\n{stderr}");
        let cfg = read(&prefix.join("client-config.json"));
        assert!(cfg.contains("codemode"), "{cfg}");
        assert!(!cfg.contains("--mode=mcp"), "{cfg}");
        assert!(
            !bin_dir.join("tokenzero-mcp").exists(),
            "peer mcp binary must be removed"
        );
        assert!(bin_dir.join("tokenzero-codemode").exists());

        // Rollback to mcp.
        let (code, stdout, stderr) = run_install("mcp", &prefix, &bin_dir, platform);
        assert_eq!(code, 0, "rollback mcp: {stdout}\n{stderr}");
        assert!(bin_dir.join("tokenzero-mcp").exists());
        assert!(!bin_dir.join("tokenzero-codemode").exists());

        let (code, stdout, stderr) = run_uninstall(&prefix, &bin_dir, platform);
        assert_eq!(code, 0, "uninstall: {stdout}\n{stderr}");
        assert!(
            stdout.contains("uninstalled=true") || stdout.contains("uninstall: ok"),
            "{stdout}"
        );
        assert!(!prefix.join("install-state.json").exists());
        assert!(!prefix.join("client-config.json").exists());
        assert!(!bin_dir.join("tokenzero").exists());
        assert!(!bin_dir.join("tokenzero-mcp").exists());
    }
}

#[test]
fn installer_fail_closed_dual_surface() {
    ensure_bins();
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("p");
    let bin_dir = tmp.path().join("b");
    fs::create_dir_all(&bin_dir).unwrap();

    let (code, _stdout, stderr) = run_install("both", &prefix, &bin_dir, "macos");
    assert_ne!(code, 0);
    assert!(
        stderr.contains("fail closed") || stderr.contains("not both") || stderr.contains("dual"),
        "{stderr}"
    );

    let out = Command::new("bash")
        .arg(install_sh())
        .args([
            "--surface",
            "mcp",
            "--prefix",
            prefix.to_str().unwrap(),
            "--bin-dir",
            bin_dir.to_str().unwrap(),
            "--skip-build",
        ])
        .env("TOKENZERO_ENABLE_MCP", "1")
        .env("TOKENZERO_ENABLE_CODEMODE", "1")
        .env("TOKENZERO_INSTALL_PLATFORM", "linux")
        .current_dir(repo_root())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("fail closed"), "{err}");
}

#[test]
fn surface_bin_install_never_hangs() {
    ensure_bins();
    let bin = repo_root().join("target/debug/tokenzero-mcp");
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("pfx");
    let started = Instant::now();
    let out = Command::new(&bin)
        .args([
            "install",
            "--surface",
            "mcp",
            "--prefix",
            prefix.to_str().unwrap(),
            "--binary",
            bin.to_str().unwrap(),
        ])
        .output()
        .expect("tokenzero-mcp install");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "install hung (stdio server?)"
    );
    assert!(out.status.success(), "{:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("install: ok"), "{stdout}");
    assert!(prefix.join("install-state.json").exists());
    assert!(prefix.join("client-config.json").exists());

    let started = Instant::now();
    let out = Command::new(&bin)
        .args(["uninstall", "--prefix", prefix.to_str().unwrap()])
        .output()
        .expect("uninstall");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(out.status.success());
    assert!(!prefix.join("install-state.json").exists());
}

#[test]
fn help_doctor_sbom_identify_surface() {
    ensure_bins();
    let root = repo_root();
    for (bin, surface) in [("tokenzero-mcp", "mcp"), ("tokenzero-codemode", "codemode")] {
        let path = root.join("target/debug").join(bin);
        let help = Command::new(&path).arg("--help").output().expect("help");
        let h = String::from_utf8_lossy(&help.stdout);
        assert!(h.contains(bin) || h.contains(surface), "{h}");
        assert!(h.contains("semantic_contract_digest"), "{h}");

        let sbom = Command::new(&path).arg("sbom").output().expect("sbom");
        assert!(sbom.status.success());
        let s = String::from_utf8_lossy(&sbom.stdout);
        let doc: serde_json::Value = serde_json::from_str(s.trim()).expect("sbom json");
        assert_eq!(doc["surface"], surface);
        assert_eq!(doc["semantic_contract_digest"].as_str().unwrap().len(), 64);
        assert!(doc["sbom"]["mutually_exclusive_with"].is_string());
    }
}

/// Install each surface into an independent temp prefix and exercise its real
/// runtime (doctor/sbom/raw-worker handshake). Dual catalog must not appear.
#[test]
fn install_each_surface_independent_prefix_exercises_runtime() {
    ensure_bins();
    let tmp = tempfile::tempdir().unwrap();

    for (surface, artifact, peer) in [
        ("mcp", "tokenzero-mcp", "tokenzero-codemode"),
        ("codemode", "tokenzero-codemode", "tokenzero-mcp"),
    ] {
        let prefix = tmp.path().join(format!("pfx-{surface}"));
        let bin_dir = tmp.path().join(format!("bin-{surface}"));
        fs::create_dir_all(&bin_dir).unwrap();

        let (code, stdout, stderr) = run_install(surface, &prefix, &bin_dir, "linux");
        assert_eq!(code, 0, "install {surface}: {stdout}\n{stderr}");
        let state = read(&prefix.join("install-state.json"));
        assert!(
            state.contains(&format!("\"{surface}\"")),
            "state for {surface}: {state}"
        );
        let cfg = read(&prefix.join("client-config.json"));
        assert!(
            cfg.contains(&format!("--mode={surface}")) || cfg.contains(surface),
            "client-config {surface}: {cfg}"
        );
        assert!(
            !cfg.contains(&format!(
                "--mode={}",
                if surface == "mcp" { "codemode" } else { "mcp" }
            )),
            "dual mode in client-config: {cfg}"
        );
        assert!(bin_dir.join(artifact).exists(), "missing {artifact}");
        assert!(
            !bin_dir.join(peer).exists(),
            "peer {peer} must not be installed alongside {artifact}"
        );
        // Shim points at selected surface only.
        let shim = bin_dir.join("tokenzero");
        assert!(shim.exists(), "shim missing for {surface}");

        let installed = bin_dir.join(artifact);
        let doctor = Command::new(&installed)
            .arg("doctor")
            .output()
            .expect("doctor");
        assert!(doctor.status.success(), "doctor {surface}");
        let d = String::from_utf8_lossy(&doctor.stdout);
        assert!(
            d.contains(surface) || d.contains(artifact),
            "doctor must identify surface: {d}"
        );
        let lower = d.to_ascii_lowercase();
        for banned in [
            "dual catalog",
            "dual catalogs",
            "both catalogs",
            "both surfaces",
            "dual surface",
        ] {
            assert!(
                !lower.contains(banned),
                "doctor must not report dual catalog wording ({banned}): {d}"
            );
        }

        let sbom = Command::new(&installed).arg("sbom").output().expect("sbom");
        assert!(sbom.status.success());
        let doc: serde_json::Value =
            serde_json::from_str(String::from_utf8_lossy(&sbom.stdout).trim()).expect("sbom json");
        assert_eq!(doc["surface"], surface);
        assert_eq!(
            doc["sbom"]["mutually_exclusive_with"].as_str().unwrap(),
            peer
        );

        // Real private-worker entry on the installed artifact.
        let hs = Command::new(&installed)
            .args(["raw-worker", "--handshake"])
            .output()
            .expect("raw-worker handshake");
        let hs_out = String::from_utf8_lossy(&hs.stdout);
        let hs_err = String::from_utf8_lossy(&hs.stderr);
        assert!(
            hs.status.success(),
            "raw-worker handshake {surface} bin={}: status={:?} stdout={hs_out:?} stderr={hs_err:?}",
            installed.display(),
            hs.status.code()
        );
        let cap: serde_json::Value = serde_json::from_str(hs_out.trim()).unwrap_or_else(|e| {
            panic!("cap json {surface}: {e}; stdout={hs_out:?} stderr={hs_err:?}")
        });
        assert_eq!(cap["schema"], "zerostack.surface.v1");
        assert_eq!(cap["surface"], "raw_worker");
        // Catalog-free: no dual tool list.
        assert!(cap.get("canonical_tools").is_none());
        assert!(cap.get("tools").is_none());
    }
}

/// Every documented or automated way to build a surface must pass
/// `--no-default-features`.
///
/// `default = ["surface-mcp"]`, so naming a surface without it leaves
/// surface-mcp enabled too, and both surfaces at once is a hard compile error.
/// The obvious command for codemode therefore FAILS:
///
///   cargo build --release --bin tokenzero-codemode --features surface-codemode
///     error: tokenzero surfaces are mutually exclusive ... never both.
///
/// and plain `cargo build --release` silently produces only tokenzero-mcp.
/// This asserts the docs and the release workflow keep saying the version that
/// actually works, so the trap is not rediscovered by trial and error.
#[test]
fn documented_surface_builds_disable_default_features() {
    let root = repo_root();
    for rel in ["docs/install.md", ".github/workflows/release.yml"] {
        let path = root.join(rel);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            // Comments are exempt: docs deliberately quote the command that
            // FAILS so readers recognize the trap. Constraining those would
            // forbid explaining the bug.
            if trimmed.starts_with('#') {
                continue;
            }
            if !trimmed.starts_with("cargo build") {
                continue;
            }
            // Only lines that select a surface are constrained.
            if !line.contains("--features surface-") {
                continue;
            }
            assert!(
                line.contains("--no-default-features"),
                "{rel}:{} selects a surface without --no-default-features, \
                 which enables surface-mcp too and fails to compile:\n  {line}",
                index + 1
            );
        }
    }
}

/// The release must ship BOTH surface artifacts.
///
/// The workflow previously ran only `cargo build -p tokenzero`, which takes
/// the default feature set and produces tokenzero-mcp alone. tokenzero-codemode
/// was never built or published, so the surface users are told to install for
/// legacy MCP-only clients did not exist in any release archive.
#[test]
fn release_workflow_builds_and_ships_both_surfaces() {
    let path = repo_root().join(".github/workflows/release.yml");
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    for artifact in ["tokenzero-mcp", "tokenzero-codemode"] {
        assert!(
            text.contains(&format!("--bin {artifact}")),
            "release workflow never builds {artifact}"
        );
        assert!(
            text.contains(&format!("$rel/{artifact}")) || text.contains(&format!("{artifact}.exe")),
            "release workflow builds {artifact} but never packages it"
        );
    }
}

/// zerostack-vpa: automatic CI must stay off in this repo to preserve the
/// GitHub Actions budget. `development-contract.yml` shipped with
/// `push`/`pull_request` triggers and was left untracked, so a single
/// `git add -A` would have started a 3-OS matrix on every push and every PR.
///
/// Committing it with the trigger fixed is only half a fix; this asserts the
/// property directly so no workflow can reintroduce an automatic trigger.
#[test]
fn no_workflow_runs_automatically() {
    let workflows = repo_root().join(".github/workflows");
    let mut checked = 0;
    for entry in std::fs::read_dir(&workflows).expect("read workflows dir") {
        let path = entry.expect("workflow entry").path();
        if path.extension().is_none_or(|e| e != "yml" && e != "yaml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read workflow");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();

        // Triggers are the keys of the top-level `on:` block, so scan exactly
        // that block. Matching anywhere in the file would trip over these words
        // in a comment or a job step; matching at column zero would miss them
        // entirely, since they are indented under `on:`.
        let triggers: Vec<&str> = text
            .lines()
            .skip_while(|l| !l.starts_with("on:"))
            .skip(1)
            .take_while(|l| l.trim().is_empty() || l.starts_with(char::is_whitespace))
            .filter_map(|l| {
                let t = l.trim();
                (!t.is_empty() && !t.starts_with('#') && t.ends_with(':'))
                    .then(|| t.trim_end_matches(':'))
            })
            .collect();

        for forbidden in ["push", "pull_request", "pull_request_target", "schedule"] {
            assert!(
                !triggers.contains(&forbidden),
                "{name} declares the automatic trigger '{forbidden}'; \
                 only workflow_dispatch is allowed (zerostack-vpa). \
                 Triggers found: {triggers:?}"
            );
        }
        assert!(
            triggers.contains(&"workflow_dispatch"),
            "{name} must be manually dispatchable; triggers found: {triggers:?}"
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "no workflows found under {}",
        workflows.display()
    );
}
