use super::*;

#[test]
fn digest_is_stable_and_nonempty() {
    let a = semantic_contract_digest();
    let b = semantic_contract_digest();
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn selection_matrix() {
    assert_eq!(
        PackageSurface::recommended_for_client(true),
        PackageSurface::Mcp
    );
    assert_eq!(
        PackageSurface::recommended_for_client(false),
        PackageSurface::Codemode
    );
}

#[test]
fn dual_mode_argv_fails_closed() {
    let args = vec![
        "tokenzero".into(),
        "--mode=mcp".into(),
        "--mode=codemode".into(),
    ];
    let err = modes_from_args(&args).unwrap_err();
    assert!(err.contains("fail closed"), "{err}");
    assert!(err.contains(ARTIFACT_MCP), "{err}");
}

#[test]
fn install_replaces_prior_surface() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path();
    let bin = prefix.join("bin-placeholder");
    fs::write(&bin, b"x").unwrap();

    let s1 = install_surface(PackageSurface::Mcp, prefix, &bin).unwrap();
    assert_eq!(s1.surface, PackageSurface::Mcp);
    let cfg1: ClientConfig =
        serde_json::from_str(&fs::read_to_string(prefix.join(CLIENT_CONFIG_FILE)).unwrap())
            .unwrap();
    assert_eq!(cfg1.surface, PackageSurface::Mcp);

    let s2 = install_surface(PackageSurface::Codemode, prefix, &bin).unwrap();
    assert_eq!(s2.surface, PackageSurface::Codemode);
    let cfg2: ClientConfig =
        serde_json::from_str(&fs::read_to_string(prefix.join(CLIENT_CONFIG_FILE)).unwrap())
            .unwrap();
    assert_eq!(cfg2.surface, PackageSurface::Codemode);
    assert_eq!(cfg2.args, vec!["--mode=codemode".to_string()]);

    let report = uninstall_report(uninstall_surface(prefix).unwrap());
    assert_eq!(report["uninstalled"], true);
    assert!(load_install_state(prefix).unwrap().is_none());
}

#[test]
fn sbom_names_surface_and_peer_exclusion() {
    let doc = sbom_document(PackageSurface::Mcp);
    assert_eq!(doc["artifact"], ARTIFACT_MCP);
    assert_eq!(doc["sbom"]["mutually_exclusive_with"], ARTIFACT_CODEMODE);
    assert!(!doc["semantic_contract_digest"].as_str().unwrap().is_empty());
}

#[test]
fn parse_surface_names() {
    assert_eq!(PackageSurface::parse("mcp").unwrap(), PackageSurface::Mcp);
    assert_eq!(
        PackageSurface::parse("tokenzero-codemode").unwrap(),
        PackageSurface::Codemode
    );
    assert!(PackageSurface::parse("both").is_err());
}

#[test]
fn dual_surface_diagnostic_names_artifacts() {
    let msg = dual_surface_diagnostic("test detail");
    assert!(msg.contains("fail closed"), "{msg}");
    assert!(msg.contains(ARTIFACT_MCP), "{msg}");
    assert!(msg.contains(ARTIFACT_CODEMODE), "{msg}");
}

#[test]
fn compile_time_surfaces_never_reports_both_without_features() {
    // This crate unit-tested without surface markers must not invent a
    // dual-surface process (tokenzero-irx9.3 review correction).
    let surfaces = compile_time_surfaces();
    assert!(
        surfaces.len() <= 1,
        "compile_time_surfaces must not report dual catalogs: {surfaces:?}"
    );
    assert!(reject_dual_compiled_surfaces().is_ok());
}

#[test]
fn install_surface_replaces_peer_config_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path();
    let bin = prefix.join("bin");
    fs::write(&bin, b"x").unwrap();
    install_surface(PackageSurface::Mcp, prefix, &bin).unwrap();
    let cfg_mcp = fs::read_to_string(prefix.join(CLIENT_CONFIG_FILE)).unwrap();
    assert!(cfg_mcp.contains("--mode=mcp"));
    // Replace with codemode — single surface config remains.
    install_surface(PackageSurface::Codemode, prefix, &bin).unwrap();
    let cfg = fs::read_to_string(prefix.join(CLIENT_CONFIG_FILE)).unwrap();
    assert!(cfg.contains("--mode=codemode"));
    assert!(!cfg.contains("--mode=mcp"));
    let state = load_install_state(prefix).unwrap().unwrap();
    assert_eq!(state.surface, PackageSurface::Codemode);
    // Uninstall clears state + config (rollback endpoint).
    uninstall_surface(prefix).unwrap();
    assert!(load_install_state(prefix).unwrap().is_none());
    assert!(!prefix.join(CLIENT_CONFIG_FILE).exists());
}
