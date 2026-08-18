use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::tempdir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn installer(args: &[&str], prefix: &Path, bin_dir: &Path) -> Output {
    Command::new("bash")
        .arg(repo_root().join("packaging/install.sh"))
        .args(args)
        .args(["--prefix", prefix.to_str().unwrap()])
        .args(["--bin-dir", bin_dir.to_str().unwrap()])
        .output()
        .expect("run installer")
}

#[test]
fn canonical_dry_run_prints_only_the_worker_selector() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let bin_dir = temp.path().join("bin");
    let out = installer(&["--surface", "codemode", "--dry-run"], &prefix, &bin_dir);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "cargo build --release -p tokenzero-worker --bin tokenzero-codemode --no-default-features\n"
    );
    assert!(!prefix.exists());
    assert!(!bin_dir.exists());
}

#[test]
fn direct_install_and_legacy_mcp_fail_before_mutation() {
    let temp = tempdir().unwrap();
    for (surface, message) in [
        ("codemode", "zerostack-uf1u"),
        (
            "mcp",
            "classic MCP compatibility is built separately with surface-mcp",
        ),
    ] {
        let prefix = temp.path().join(format!("prefix-{surface}"));
        let bin_dir = temp.path().join(format!("bin-{surface}"));
        let out = installer(&["--surface", surface], &prefix, &bin_dir);
        assert_eq!(out.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&out.stderr).contains(message));
        assert!(!prefix.exists());
        assert!(!bin_dir.exists());
    }

    let out = installer(
        &["--surface", "codemode", "--skip-build"],
        &temp.path().join("skip-prefix"),
        &temp.path().join("skip-bin"),
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown arg: --skip-build"));
}

#[test]
fn uninstall_without_state_preserves_a_regular_cli() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let cli = bin_dir.join("tokenzero");
    fs::write(&cli, b"canonical cli").unwrap();

    let out = installer(&["--uninstall"], &prefix, &bin_dir);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("uninstalled=false"));
    assert_eq!(fs::read(cli).unwrap(), b"canonical cli");
    assert!(!prefix.exists());
}

#[test]
fn uninstall_is_bound_to_the_recorded_legacy_binary() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&prefix).unwrap();
    fs::create_dir_all(&bin_dir).unwrap();
    let artifact = bin_dir.join("tokenzero-codemode");
    fs::write(&artifact, b"legacy worker").unwrap();
    let cli = bin_dir.join("tokenzero");
    fs::write(&cli, b"canonical cli").unwrap();
    fs::write(
        prefix.join("install-state.json"),
        serde_json::to_vec(&json!({
            "surface": "codemode",
            "artifact": "tokenzero-codemode",
            "binary_path": artifact,
            "semantic_contract_digest": "old"
        }))
        .unwrap(),
    )
    .unwrap();

    let out = installer(&["--uninstall"], &prefix, &bin_dir);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!artifact.exists());
    assert_eq!(fs::read(cli).unwrap(), b"canonical cli");
    assert!(!prefix.join("install-state.json").exists());
}

#[test]
fn uninstall_preserves_state_when_binary_path_does_not_match() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&prefix).unwrap();
    let state = prefix.join("install-state.json");
    fs::write(
        &state,
        serde_json::to_vec(&json!({
            "surface": "codemode",
            "artifact": "tokenzero-codemode",
            "binary_path": "/unexpected/tokenzero-codemode"
        }))
        .unwrap(),
    )
    .unwrap();

    let out = installer(&["--uninstall"], &prefix, &bin_dir);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("binary_path mismatch"));
    assert!(state.exists());
}

#[test]
fn sbom_uses_the_canonical_worker_binary() {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"))
        .join("packaging-e2e-worker");
    let build = Command::new("cargo")
        .args([
            "build",
            "-p",
            "tokenzero-worker",
            "--bin",
            "tokenzero-codemode",
            "--no-default-features",
        ])
        .env("CARGO_TARGET_DIR", &target)
        .current_dir(repo_root())
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let bin_dir = target.join("debug");
    let worker = bin_dir.join(format!(
        "tokenzero-codemode{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        worker.is_file(),
        "canonical worker build omitted its binary"
    );
    let temp = tempdir().unwrap();
    let out = installer(&["--sbom", "--surface", "codemode"], temp.path(), &bin_dir);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let sbom: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(sbom["artifact"], "tokenzero-codemode");
    assert_eq!(sbom["package"], "tokenzero-worker");
    assert_eq!(sbom["raw_worker_protocol"], "zerostack.raw_worker");
}
