//! Static evidence for tokenzero-irx9.3 process/artifact mutual exclusion.
//!
//! These tests read source/Cargo.toml/install.sh only — they do not invoke
//! cargo, rustc, surface servers, or the package installer.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// Defaults must not enable both surfaces (dual-surface default removed).
#[test]
fn cargo_defaults_are_single_surface() {
    let mcp = read("crates/tokenzero-mcp/Cargo.toml");
    let cli = read("crates/tokenzero/Cargo.toml");

    // Extract the [features] default = ... line (first occurrence after [features]).
    for (label, toml) in [("tokenzero-mcp", mcp.as_str()), ("tokenzero", cli.as_str())] {
        let features = toml
            .split("[features]")
            .nth(1)
            .unwrap_or_else(|| panic!("{label}: missing [features]"));
        let default_line = features
            .lines()
            .find(|l| l.trim_start().starts_with("default"))
            .unwrap_or_else(|| panic!("{label}: missing default ="));
        assert!(
            default_line.contains("surface-mcp"),
            "{label}: default must select surface-mcp: {default_line}"
        );
        assert!(
            !default_line.contains("surface-codemode"),
            "{label}: default must NOT enable surface-codemode (dual default banned): {default_line}"
        );
        // Hard exclusion of the dual-list form from edbc891.
        assert!(
            !default_line.contains("surface-mcp\", \"surface-codemode")
                && !default_line.contains("surface-mcp\", \"surface-codemode"),
            "{label}: dual default list forbidden: {default_line}"
        );
    }
}

/// Dual feature enablement is a compile_error in all three packaging crates.
#[test]
fn compile_error_guards_dual_features_in_source() {
    for path in [
        "crates/tokenzero-mcp/src/lib.rs",
        "crates/tokenzero/src/main.rs",
        "crates/tokenzero-install/src/lib.rs",
    ] {
        let src = read(path);
        assert!(
            src.contains("compile_error!"),
            "{path}: missing compile_error! for dual surface features"
        );
        assert!(
            src.contains("surface-mcp") && src.contains("surface-codemode"),
            "{path}: compile_error must name both surface features"
        );
        assert!(
            src.contains("mutually exclusive") || src.contains("mutual exclusion"),
            "{path}: compile_error must document mutual exclusion"
        );
    }
}

/// Peer surface deps are optional and feature-gated (static Cargo.toml proof).
#[test]
fn peer_surface_dependencies_are_optional() {
    let mcp = read("crates/tokenzero-mcp/Cargo.toml");
    assert!(
        mcp.contains("fastmcp-rust = { workspace = true, optional = true }")
            || mcp.contains("optional = true") && mcp.contains("fastmcp-rust"),
        "fastmcp-rust must be optional"
    );
    assert!(
        mcp.contains("rquickjs = { workspace = true, optional = true }")
            || (mcp.contains("rquickjs") && mcp.contains("optional = true")),
        "rquickjs must be optional"
    );
    assert!(
        mcp.contains("surface-mcp = [\"dep:fastmcp-rust\"]"),
        "surface-mcp must pull only fastmcp-rust"
    );
    assert!(
        mcp.contains("surface-codemode = [\"dep:rquickjs\"]"),
        "surface-codemode must pull only rquickjs"
    );
}

/// Installer lifecycle owner never starts a stdio server to write state.
#[test]
fn install_sh_is_installer_native_no_server_state_writes() {
    let sh = read("packaging/install.sh");
    assert!(
        sh.contains("Never invoke the surface binary")
            || sh.contains("never starts a stdio server")
            || sh.contains("Installer writes state/client-config itself"),
        "install.sh must document installer-native state writes"
    );
    assert!(
        sh.contains("write_install_state") || sh.contains("install-state.json"),
        "install.sh must write install-state itself"
    );
    assert!(
        sh.contains("client-config.json"),
        "install.sh must write client-config itself"
    );
    assert!(
        sh.contains("atomic_write") || sh.contains(".tmp."),
        "install.sh must use atomic writes"
    );
    // State write path must not shell out to surface `install` for lifecycle ownership.
    // (Surface bins may implement install for convenience; shell installer is owner.)
    let install_body = sh
        .split("if [[ \"$ACTION\" == \"uninstall\" ]]")
        .nth(1)
        .unwrap_or(&sh);
    // After option parsing, the install path builds/copies then write_install_state —
    // never: `"$SRC" install` or `"$BIN_DIR/$ARTIFACT" install` for state creation.
    for forbidden in [
        "\"$SRC\" install",
        "\"$BIN_DIR/$ARTIFACT\" install",
        "$ARTIFACT install --",
        "tokenzero-mcp install",
        "tokenzero-codemode install",
    ] {
        assert!(
            !install_body.contains(forbidden),
            "install.sh must not invoke surface install for state: found {forbidden:?}"
        );
    }
    // Uninstall is installer-native (rm state files), not surface server.
    assert!(
        sh.contains("Installer-native uninstall")
            || sh.contains("never invoke surface server")
            || sh.contains("rm -f \"$PREFIX/install-state.json\""),
        "uninstall must be installer-native"
    );
    // Peer replacement on surface switch.
    assert!(
        sh.contains("replacing peer artifact") || sh.contains("PEER="),
        "install.sh must remove peer artifact on replace"
    );
    // Rollback snapshot of prior state before replace.
    assert!(
        sh.contains("ROLLBACK_STATE") || sh.contains("restoring prior"),
        "install.sh must snapshot/restore prior state on failed install"
    );
    // Dual fail closed.
    assert!(
        sh.contains("fail closed") && sh.contains("TOKENZERO_ENABLE_MCP"),
        "install.sh must fail closed on dual env selection"
    );
    // Platform simulation for e2e without real dual-OS.
    assert!(
        sh.contains("TOKENZERO_INSTALL_PLATFORM"),
        "install.sh must support platform simulation"
    );
}

/// Packaging helpers document process mutual exclusion and reject dual compile.
#[test]
fn packaging_rs_rejects_dual_compiled_surfaces() {
    let src = read("crates/tokenzero-install/src/packaging.rs");
    assert!(
        src.contains("reject_dual_compiled_surfaces"),
        "missing reject_dual_compiled_surfaces"
    );
    assert!(
        src.contains("compile_time_surfaces"),
        "missing compile_time_surfaces"
    );
    // No dual-surface "dev fallback" that injects both surfaces when features empty.
    assert!(
        !src.contains("Dev fallback: both allowed")
            && !src.contains("both surfaces are package options"),
        "must not reintroduce dual-surface compile_time fallback"
    );
    assert!(
        src.contains("install_surface") && src.contains("uninstall_surface"),
        "lifecycle owner API required"
    );
    assert!(
        src.contains("atomic_write"),
        "state writes must be atomic"
    );
}

/// Runtime exclusivity chokepoint on mcp-server path.
#[test]
fn main_enforces_resolve_startup_surface() {
    let main = read("crates/tokenzero/src/main.rs");
    assert!(
        main.contains("enforce_surface_exclusivity"),
        "mcp-server must call exclusivity gate"
    );
    assert!(
        main.contains("resolve_startup_surface")
            || main.contains("reject_dual_compiled_surfaces"),
        "must resolve single surface / reject dual compile at runtime"
    );
    assert!(
        main.contains("TOKENZERO_ALLOW_DUAL"),
        "ALLOW_DUAL mentioned for hub sentinel only"
    );
    assert!(
        main.contains("never dual catalogs") || main.contains("hub sentinel only"),
        "ALLOW_DUAL must not authorize dual catalogs"
    );
}

/// Surface bins handle packaging subcommands before server start.
#[test]
fn surface_bins_handle_install_before_server() {
    for path in [
        "crates/tokenzero/src/bin/tokenzero_mcp.rs",
        "crates/tokenzero/src/bin/tokenzero_codemode.rs",
    ] {
        let src = read(path);
        // Match server *call sites*, not imports (`use ... run_fastmcp_stdio`).
        let install_pos = src.find("if args.iter().any(|a| a == \"install\")");
        let server_pos = src
            .find("run_fastmcp_stdio(config)")
            .or_else(|| src.find("let code = run_stdio(config)"))
            .or_else(|| src.find("run_stdio(config)"));
        assert!(install_pos.is_some(), "{path}: missing install branch");
        assert!(
            server_pos.is_some(),
            "{path}: missing server call site (run_fastmcp_stdio/run_stdio)"
        );
        assert!(
            install_pos.unwrap() < server_pos.unwrap(),
            "{path}: install must be handled before server start"
        );
        let after_install = &src[install_pos.unwrap()..];
        let exit_before_server = after_install
            .find("process::exit(0)")
            .or_else(|| after_install.find("process::exit("));
        let server_rel = server_pos.unwrap() - install_pos.unwrap();
        assert!(
            exit_before_server.is_some() && exit_before_server.unwrap() < server_rel,
            "{path}: install path must process::exit before server call"
        );
        assert!(
            src.contains("install_surface") && src.contains("uninstall_surface"),
            "{path}: must use packaging install_surface (no stdio state write)"
        );
    }
}

/// Selection matrix docs and docs/install.md stay aligned.
#[test]
fn selection_matrix_documented() {
    let docs = read("docs/install.md");
    assert!(docs.contains("native CodeMode client") || docs.contains("Native CodeMode"));
    assert!(docs.contains("tokenzero-mcp"));
    assert!(docs.contains("tokenzero-codemode"));
    assert!(
        docs.contains("compile error") || docs.contains("compile_error"),
        "docs must state dual features are a compile error"
    );
    assert!(
        docs.contains("never") && docs.contains("stdio server"),
        "docs must state installer never starts stdio for state"
    );
}
