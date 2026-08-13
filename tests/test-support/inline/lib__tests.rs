#[test]
fn installer_prints_only_the_canonical_backend_selector() {
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/install.sh");
    let output = std::process::Command::new("bash")
        .arg(&script)
        .args(["--surface", "codemode", "--dry-run"])
        .output()
        .expect("installer dry run starts");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 dry run"),
        "cargo build --release -p tokenzero-worker --bin tokenzero-codemode --no-default-features\n"
    );

    let legacy = std::process::Command::new("bash")
        .arg(&script)
        .args(["--surface", "mcp", "--dry-run"])
        .output()
        .expect("legacy selector starts");
    assert_eq!(legacy.status.code(), Some(2));
    let stderr = String::from_utf8(legacy.stderr).expect("UTF-8 diagnostic");
    assert!(stderr.contains("classic MCP compatibility"), "{stderr}");
    assert!(stderr.contains("surface-mcp"), "{stderr}");

    let blocked_root = std::env::temp_dir().join(format!(
        "tokenzero-worker-install-blocked-{}",
        std::process::id()
    ));
    let blocked = std::process::Command::new("bash")
        .arg(&script)
        .args(["--surface", "codemode", "--prefix"])
        .arg(&blocked_root)
        .output()
        .expect("blocked install starts");
    assert_eq!(blocked.status.code(), Some(2));
    let stderr = String::from_utf8(blocked.stderr).expect("UTF-8 diagnostic");
    assert!(stderr.contains("zerostack-uf1u"), "{stderr}");
    assert!(!blocked_root.exists(), "blocked install mutated its prefix");
}

#[test]
fn canonical_worker_manifest_has_no_host_dependencies() {
    let manifest = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tokenzero-codemode/Cargo.toml"),
    )
    .expect("worker manifest readable");
    let default = manifest
        .split_once("[features]")
        .and_then(|(_, features)| features.lines().find(|line| line.starts_with("default")))
        .expect("worker default feature declaration");
    assert_eq!(default.trim(), "default = []");
    for forbidden in [
        "[lib]",
        "surface-codemode",
        "rquickjs",
        "fastmcp",
        "machine-permit",
        "tokenzero-mcp-compat",
        "zero-codemode =",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "canonical worker manifest contains {forbidden}"
        );
    }

    let main = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tokenzero-codemode/src/main.rs"),
    )
    .expect("worker main readable");
    for forbidden in ["rquickjs", "fastmcp", "execute_codemode", "run_stdio"] {
        assert!(!main.contains(forbidden), "raw worker imports {forbidden}");
    }
}
