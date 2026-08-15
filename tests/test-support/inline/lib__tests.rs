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
