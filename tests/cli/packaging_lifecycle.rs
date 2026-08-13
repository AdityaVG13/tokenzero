//! Lifecycle unit tests for package surface install state (tokenzero-irx9.3).
//!
//! These exercise the Rust packaging helpers without building surface bins.

use std::fs;
use tokenzero_install::packaging::{
    ARTIFACT_MCP, ARTIFACT_RAW_WORKER, CLIENT_CONFIG_FILE, ClientConfig, PackageSurface,
    compile_time_surfaces, install_surface, load_install_state, modes_from_args, package_identity,
    reject_dual_compiled_surfaces, reject_dual_env_selection, sbom_document,
    semantic_contract_digest, uninstall_report, uninstall_surface,
};

#[test]
fn digest_stable() {
    let a = semantic_contract_digest();
    let b = semantic_contract_digest();
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
}

#[test]
fn selection_matrix_docs() {
    assert_eq!(
        PackageSurface::recommended_for_client(true).artifact_name(),
        ARTIFACT_RAW_WORKER
    );
    assert_eq!(
        PackageSurface::recommended_for_client(false).artifact_name(),
        ARTIFACT_MCP
    );
    let id = package_identity(PackageSurface::Mcp);
    assert_eq!(
        id["selection_matrix"]["aggregate_host"],
        ARTIFACT_RAW_WORKER
    );
    assert_eq!(id["selection_matrix"]["classic_mcp_client"], ARTIFACT_MCP);
}

#[test]
fn dual_mode_argv_fails_closed() {
    let err = modes_from_args(&[
        "tokenzero".into(),
        "--mode=mcp".into(),
        "--mode=codemode".into(),
    ])
    .unwrap_err();
    assert!(err.contains("fail closed"), "{err}");
}

#[test]
fn install_replace_uninstall_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path();
    let bin = prefix.join("fake-bin");
    fs::write(&bin, b"x").unwrap();

    let s1 = install_surface(PackageSurface::Mcp, prefix, &bin).unwrap();
    assert_eq!(s1.surface, PackageSurface::Mcp);
    assert!(prefix.join("install-state.json").exists());
    assert!(prefix.join(CLIENT_CONFIG_FILE).exists());

    let s2 = install_surface(PackageSurface::RawWorker, prefix, &bin).unwrap();
    assert_eq!(s2.surface, PackageSurface::RawWorker);
    let cfg: ClientConfig =
        serde_json::from_str(&fs::read_to_string(prefix.join(CLIENT_CONFIG_FILE)).unwrap())
            .unwrap();
    assert_eq!(cfg.surface, PackageSurface::RawWorker);
    assert_eq!(cfg.args, vec!["raw-worker".to_string()]);

    let report = uninstall_report(uninstall_surface(prefix).unwrap());
    assert_eq!(report["uninstalled"], true);
    assert!(load_install_state(prefix).unwrap().is_none());
}

#[test]
fn sbom_peer_exclusion() {
    let mcp = sbom_document(PackageSurface::Mcp);
    assert_eq!(mcp["sbom"]["mutually_exclusive_with"], ARTIFACT_RAW_WORKER);
    let cm = sbom_document(PackageSurface::RawWorker);
    assert_eq!(cm["sbom"]["mutually_exclusive_with"], ARTIFACT_MCP);
}

#[test]
fn dual_surface_diagnostic_is_precise() {
    let msg = tokenzero_install::packaging::dual_surface_diagnostic("both surfaces requested");
    assert!(msg.contains("fail closed"), "{msg}");
    assert!(msg.contains(ARTIFACT_MCP), "{msg}");
    assert!(msg.contains(ARTIFACT_RAW_WORKER), "{msg}");
    // reject_dual_env_selection is covered by packaging_e2e via subprocess env.
    let _ = reject_dual_env_selection();
}

#[test]
fn process_never_reports_dual_compiled_surfaces() {
    // Default tokenzero package features enable exactly one surface.
    let surfaces = compile_time_surfaces();
    assert!(
        surfaces.len() <= 1,
        "process must not compile both catalogs: {surfaces:?}"
    );
    assert!(
        reject_dual_compiled_surfaces().is_ok(),
        "reject_dual_compiled_surfaces must pass for single-surface builds"
    );
}
