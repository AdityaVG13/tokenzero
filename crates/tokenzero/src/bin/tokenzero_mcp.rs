//! Release artifact `tokenzero-mcp`: FastMCP per-op surface only (tokenzero-irx9.3).
//!
//! Packaging subcommands (install/uninstall/sbom/doctor/help) always exit without
//! opening a stdio server. Shared core with `tokenzero-codemode`.

use std::env;
use std::path::PathBuf;
use std::process;
use tokenzero_install::packaging::{
    PackageSurface, assert_packaged_surface_features, assert_surface_compiled,
    default_install_prefix, install_surface, package_identity, sbom_document,
    semantic_contract_digest, uninstall_report, uninstall_surface,
};
use tokenzero_mcp::{EngineConfig, run_fastmcp_stdio};
use tokenzero_core::McpToolSurface;

const SURFACE: PackageSurface = PackageSurface::Mcp;

fn main() {
    // SAFETY: single-threaded before any other TOKENZERO_PACKAGE_SURFACE readers.
    unsafe {
        env::set_var("TOKENZERO_PACKAGE_SURFACE", "mcp");
    }

    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "help" || a == "--help" || a == "-h") {
        let id = package_identity(SURFACE);
        println!(
            "tokenzero-mcp — FastMCP per-operation surface (mutually exclusive with tokenzero-codemode)\n\
             semantic_contract_digest: {}\n\
             selection: native CodeMode clients install this package\n\
             usage: tokenzero-mcp | tokenzero-mcp doctor | tokenzero-mcp sbom | tokenzero-mcp install|uninstall\n\
             identity: {}",
            semantic_contract_digest(),
            id
        );
        process::exit(0);
    }

    if args.iter().any(|a| a == "sbom") {
        println!(
            "{}",
            serde_json::to_string_pretty(&sbom_document(SURFACE)).unwrap()
        );
        process::exit(0);
    }

    if args.iter().any(|a| a == "install") {
        run_install(&args);
        process::exit(0);
    }
    if args.iter().any(|a| a == "uninstall") {
        run_uninstall(&args);
        process::exit(0);
    }

    if args.iter().any(|a| a == "doctor" || a == "--doctor") {
        if let Err(e) = assert_surface_compiled(SURFACE) {
            eprintln!("{e}");
            process::exit(2);
        }
        let id = package_identity(SURFACE);
        println!(
            "package: artifact={} surface={} semantic_contract_digest={}",
            id["artifact"], id["surface"], id["semantic_contract_digest"]
        );
        process::exit(0);
    }

    if args.iter().any(|a| a == "--mode=codemode")
        || args.windows(2).any(|w| w[0] == "--mode" && w[1] == "codemode")
    {
        eprintln!(
            "tokenzero-mcp: artifact is locked to surface 'mcp'; refused --mode=codemode. \
Install tokenzero-codemode for the CodeMode catalog (mutually exclusive)."
        );
        process::exit(2);
    }

    if let Err(e) = assert_surface_compiled(SURFACE) {
        eprintln!("{e}");
        process::exit(2);
    }
    // Release packages must not ship dual surface features.
    unsafe {
        env::set_var("TOKENZERO_FAIL_CLOSED_DUAL_FEATURES", "1");
    }
    if let Err(e) = assert_packaged_surface_features(SURFACE) {
        eprintln!("{e}");
        process::exit(2);
    }

    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut config = EngineConfig::for_root(&cwd);
    config.tool_surface = McpToolSurface::Classic;
    // FastMCP run never returns (`!`).
    run_fastmcp_stdio(config);
}

fn parse_flag(args: &[String], name: &str) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == name {
            return args.get(i + 1).cloned();
        }
        if let Some(rest) = args[i].strip_prefix(&format!("{name}=")) {
            return Some(rest.to_string());
        }
        i += 1;
    }
    None
}

fn run_install(args: &[String]) {
    if let Some(requested) = parse_flag(args, "--surface") {
        if PackageSurface::parse(&requested).ok() != Some(SURFACE) {
            eprintln!(
                "tokenzero-mcp: install --surface must be 'mcp' for this artifact (got {requested:?})"
            );
            process::exit(2);
        }
    }
    let prefix = parse_flag(args, "--prefix")
        .map(PathBuf::from)
        .unwrap_or_else(default_install_prefix);
    let binary = parse_flag(args, "--binary")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_exe().unwrap_or_else(|_| PathBuf::from("tokenzero-mcp")));
    match install_surface(SURFACE, &prefix, &binary) {
        Ok(state) => {
            println!(
                "install: ok surface={} artifact={} prefix={} semantic_contract_digest={} client_config={}",
                state.surface.as_str(),
                state.artifact,
                state.prefix,
                state.semantic_contract_digest,
                state.client_config
            );
        }
        Err(e) => {
            eprintln!("install: FAIL {e}");
            process::exit(1);
        }
    }
}

fn run_uninstall(args: &[String]) {
    let prefix = parse_flag(args, "--prefix")
        .map(PathBuf::from)
        .unwrap_or_else(default_install_prefix);
    match uninstall_surface(&prefix) {
        Ok(prev) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&uninstall_report(prev)).unwrap()
            );
        }
        Err(e) => {
            eprintln!("uninstall: FAIL {e}");
            process::exit(1);
        }
    }
}
