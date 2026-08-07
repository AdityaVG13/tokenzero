use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn read(path: &str) -> String {
    fs::read_to_string(repo_root().join(path)).unwrap()
}

#[test]
fn workspace_has_the_three_canonical_packages() {
    let root = read("Cargo.toml");
    assert!(root.contains("\"crates/tokenzero\""));
    assert!(root.contains("\"crates/tokenzero-codemode\""));
    assert!(root.contains("\"crates/tokenzero-test-support\""));

    let cli = read("crates/tokenzero/Cargo.toml");
    assert!(cli.contains("name = \"tokenzero-cli\""));
    assert!(cli.contains("autobins = false"));
    assert!(cli.contains("default = []"));
    assert!(cli.contains("name = \"tokenzero\""));

    let worker = read("crates/tokenzero-codemode/Cargo.toml");
    assert!(worker.contains("name = \"tokenzero-worker\""));
    assert!(worker.contains("autobins = false"));
    assert!(worker.contains("default = []"));
    assert!(worker.contains("name = \"tokenzero-codemode\""));
    assert!(worker.contains("zero-abi.workspace = true"));
    assert!(!worker.contains("tokenzero-install"));

    let support = read("crates/tokenzero-test-support/Cargo.toml");
    assert!(support.contains("name = \"tokenzero-test-support\""));
    assert!(support.contains("zero-abi.workspace = true"));
}

#[test]
fn canonical_cli_has_no_raw_worker_entrypoint_or_default_adapter() {
    let cargo = read("crates/tokenzero/Cargo.toml");
    assert!(cargo.contains("tokenzero-mcp-compat = {"));
    assert!(cargo.contains("optional = true"));
    assert_eq!(cargo.matches("[[bin]]").count(), 1);

    let main = read("crates/tokenzero/src/main.rs");
    assert!(!main.contains("run_raw_worker_cli"));
    assert!(!main.contains("raw-worker"));
}

#[test]
fn raw_v2_wire_authority_is_only_the_pinned_zero_abi() {
    let root = read("Cargo.toml");
    assert!(root.contains("rev = \"3eca1c6299ec5a683d283ddad0aae62ece2a3abc\""));
    let protocol = read("crates/tokenzero-engine/src/raw_worker_v2_protocol.rs");
    assert!(protocol.contains("pub use zero_abi::raw_worker::*;"));
    for forbidden in ["struct ", "enum ", "serde_json", "MAX_FRAME_BYTES:"] {
        assert!(
            !protocol.contains(forbidden),
            "local wire authority: {forbidden}"
        );
    }
}

#[test]
fn central_build_consumers_select_cli_and_worker_separately() {
    let release = read(".github/workflows/release.yml");
    let release = release.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(release.contains("-p tokenzero-cli"));
    assert!(release.contains("--bin tokenzero --no-default-features"));
    assert!(release.contains("-p tokenzero-worker"));
    assert!(release.contains("--bin tokenzero-codemode --no-default-features"));
    let ci = read(".github/workflows/ci.yml");
    assert!(ci.contains("-p tokenzero-worker --bin tokenzero-codemode --no-default-features"));
    let makefile = read("Makefile");
    assert!(
        makefile.contains("-p tokenzero-worker --bin tokenzero-codemode --no-default-features")
    );
}

#[test]
fn installer_is_selector_probe_uninstall_only_until_central_discovery() {
    let installer = read("packaging/install.sh");
    assert!(installer.contains("zerostack-uf1u"));
    assert!(
        installer
            .contains("cargo build --release -p $PACKAGE --bin $ARTIFACT --no-default-features")
    );
    assert!(installer.contains("legacy MCP artifact retired"));
    assert!(!installer.contains("write_install_state"));
    assert!(!installer.contains("client-config args"));
    assert!(!installer.contains("ln -sf"));
}
