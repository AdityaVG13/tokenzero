//! Installer e2e for tokenzero-irx9.3 (macOS/Linux + platform simulation).
//!
//! Exercises packaging/install.sh with --skip-build against prebuilt single-surface
//! binaries: fresh install, replacement, upgrade, rollback, uninstall, dual fail-closed.

use std::fs;
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
    for (bin, surface) in [
        ("tokenzero-mcp", "mcp"),
        ("tokenzero-codemode", "codemode"),
    ] {
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
        assert_eq!(
            code, 0,
            "install {surface}: {stdout}\n{stderr}"
        );
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
            !cfg.contains(&format!("--mode={}", if surface == "mcp" { "codemode" } else { "mcp" })),
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
            panic!(
                "cap json {surface}: {e}; stdout={hs_out:?} stderr={hs_err:?}"
            )
        });
        assert_eq!(cap["schema"], "zerostack.surface.v1");
        assert_eq!(cap["surface"], "raw_worker");
        // Catalog-free: no dual tool list.
        assert!(cap.get("canonical_tools").is_none());
        assert!(cap.get("tools").is_none());
    }
}
