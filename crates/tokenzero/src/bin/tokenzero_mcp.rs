//! Release artifact `tokenzero-mcp`: FastMCP per-op surface only (tokenzero-irx9.3).
//!
//! Packaging subcommands (install/uninstall/sbom/doctor/help) always exit without
//! opening a stdio server. Domain execution remains shared with the raw worker.

use std::env;
use std::path::PathBuf;
use std::process;
use tokenzero_core::McpToolSurface;
use tokenzero_install::packaging::{
    PackageSurface, assert_packaged_surface_features, assert_surface_compiled,
    default_install_prefix, install_surface, package_identity, reject_non_stdio_args,
    sbom_document, semantic_contract_digest, uninstall_report, uninstall_surface,
};
use tokenzero_mcp_compat::{EngineConfig, run_fastmcp_stdio};

const SURFACE: PackageSurface = PackageSurface::Mcp;

fn main() {
    // SAFETY: single-threaded before any other TOKENZERO_PACKAGE_SURFACE readers.
    unsafe {
        env::set_var("TOKENZERO_PACKAGE_SURFACE", "mcp");
    }

    let args: Vec<String> = env::args().collect();
    // Flag values (`--prefix sbom`, `--mode mcp`) must not be scanned as verbs.
    let verbs = argv_without_option_values(&args);

    // Private raw worker (tokenzero-irx9.4): OMP/router composition path.
    // Framing + handshake + NDJSON exec loop; not a second user catalog.
    match tokenzero_mcp_compat::maybe_run_raw_worker_from_args(&args) {
        Ok(Some(code)) => process::exit(code),
        Ok(None) => {}
        Err(error) => {
            eprintln!("tokenzero-mcp: {error}");
            process::exit(2);
        }
    }

    if verbs
        .iter()
        .any(|a| a == "help" || a == "--help" || a == "-h")
    {
        let id = package_identity(SURFACE);
        println!(
            "tokenzero-mcp — FastMCP per-operation surface (mutually exclusive with tokenzero-codemode)\n\
             semantic_contract_digest: {}\n\
             selection: native CodeMode clients install this package\n\
             usage: tokenzero-mcp | tokenzero-mcp doctor | tokenzero-mcp sbom | tokenzero-mcp install|uninstall\n\
             usage: tokenzero-mcp raw-worker [--handshake|--once JSON|--root DIR]  # OMP private worker\n\
             identity: {}",
            semantic_contract_digest(),
            id
        );
        process::exit(0);
    }

    if verbs.iter().any(|a| a == "sbom") {
        println!(
            "{}",
            serde_json::to_string_pretty(&sbom_document(SURFACE)).unwrap()
        );
        process::exit(0);
    }

    if verbs.iter().any(|a| a == "install") {
        run_install(&args);
        process::exit(0);
    }
    if verbs.iter().any(|a| a == "uninstall") {
        run_uninstall(&args);
        process::exit(0);
    }

    if verbs.iter().any(|a| a == "doctor" || a == "--doctor") {
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
        || args
            .windows(2)
            .any(|w| w[0] == "--mode" && w[1] == "codemode")
    {
        eprintln!(
            "tokenzero-mcp: artifact is locked to classic MCP; refused --mode=codemode. \
Use the ZeroStack aggregate host for plans; it launches tokenzero-codemode only as a raw worker."
        );
        process::exit(2);
    }

    // Every supported subcommand has already exited above, so anything left on
    // argv is a caller mistake. Reject it instead of serving stdio, which would
    // read EOF and exit 0 with no output (tokenzero-j0cn).
    if let Err(e) = reject_non_stdio_args("tokenzero-mcp", &verbs) {
        eprintln!("{e}");
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

/// Flags whose following argv token is a value, not a packaging verb.
const VALUE_FLAGS: &[&str] = &[
    "--prefix",
    "--binary",
    "--surface",
    "--mode",
    "--tool-surface",
    "--root",
    "--repo",
    "--log-level",
    "--cache-path",
    "--once",
];

/// Drop option values so `--prefix sbom` / `--mode mcp` cannot be scanned as
/// subcommands. Flags themselves stay so unknown options still fail loud.
fn argv_without_option_values(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if i > 0 && VALUE_FLAGS.contains(&arg.as_str()) {
            out.push(arg.clone());
            i += 1;
            if i < args.len() && !args[i].starts_with('-') {
                i += 1;
            }
            continue;
        }
        out.push(arg.clone());
        i += 1;
    }
    out
}

/// Parse `--name VALUE` or `--name=VALUE`. Missing or flag-shaped values fail
/// loud so `install --prefix --binary …` cannot treat `--binary` as a path.
/// Dash-leading paths must use the equals form (`--prefix=-`).
fn parse_flag(args: &[String], name: &str) -> Result<Option<String>, String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == name {
            return match args.get(i + 1) {
                Some(value) if !value.starts_with('-') => Ok(Some(value.clone())),
                Some(value) => Err(format!("{name} requires a value (got {value:?})")),
                None => Err(format!("{name} requires a value")),
            };
        }
        if let Some(rest) = args[i].strip_prefix(&format!("{name}=")) {
            if rest.is_empty() {
                return Err(format!("{name} requires a value"));
            }
            return Ok(Some(rest.to_string()));
        }
        i += 1;
    }
    Ok(None)
}

fn flag_or_exit(args: &[String], name: &str) -> Option<String> {
    match parse_flag(args, name) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("tokenzero-mcp: {error}");
            process::exit(2);
        }
    }
}

fn run_install(args: &[String]) {
    if let Some(requested) = flag_or_exit(args, "--surface") {
        if PackageSurface::parse(&requested).ok() != Some(SURFACE) {
            eprintln!(
                "tokenzero-mcp: install --surface must be 'mcp' for this artifact (got {requested:?})"
            );
            process::exit(2);
        }
    }
    let prefix = flag_or_exit(args, "--prefix")
        .map(PathBuf::from)
        .unwrap_or_else(default_install_prefix);
    let binary = flag_or_exit(args, "--binary")
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
    let prefix = flag_or_exit(args, "--prefix")
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

#[cfg(test)]
mod tests {
    use super::{argv_without_option_values, parse_flag};

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn parse_flag_absent_is_none() {
        assert_eq!(
            parse_flag(&args(&["tokenzero-mcp", "install"]), "--prefix").unwrap(),
            None
        );
    }

    #[test]
    fn parse_flag_space_and_equals_forms() {
        assert_eq!(
            parse_flag(
                &args(&["tokenzero-mcp", "install", "--prefix", "/tmp/tz"]),
                "--prefix"
            )
            .unwrap()
            .as_deref(),
            Some("/tmp/tz")
        );
        assert_eq!(
            parse_flag(
                &args(&["tokenzero-mcp", "install", "--prefix=/tmp/tz"]),
                "--prefix"
            )
            .unwrap()
            .as_deref(),
            Some("/tmp/tz")
        );
    }

    #[test]
    fn parse_flag_rejects_missing_or_flag_shaped_values() {
        let missing = parse_flag(&args(&["tokenzero-mcp", "install", "--prefix"]), "--prefix")
            .expect_err("bare --prefix must fail loud");
        assert!(missing.contains("requires a value"), "{missing}");

        let empty = parse_flag(
            &args(&["tokenzero-mcp", "install", "--prefix="]),
            "--prefix",
        )
        .expect_err("empty --prefix= must fail loud");
        assert!(empty.contains("requires a value"), "{empty}");

        let stolen = parse_flag(
            &args(&[
                "tokenzero-mcp",
                "install",
                "--prefix",
                "--binary",
                "/tmp/bin",
            ]),
            "--prefix",
        )
        .expect_err("--prefix must not swallow the next flag");
        assert!(stolen.contains("--binary"), "{stolen}");
        assert_eq!(
            parse_flag(
                &args(&[
                    "tokenzero-mcp",
                    "install",
                    "--prefix",
                    "--binary",
                    "/tmp/bin",
                ]),
                "--binary"
            )
            .unwrap()
            .as_deref(),
            Some("/tmp/bin")
        );
    }

    #[test]
    fn parse_flag_equals_form_keeps_dash_leading_values() {
        assert_eq!(
            parse_flag(
                &args(&["tokenzero-mcp", "install", "--prefix=-"]),
                "--prefix"
            )
            .unwrap()
            .as_deref(),
            Some("-")
        );
        assert_eq!(
            parse_flag(
                &args(&["tokenzero-mcp", "uninstall", "--prefix=-dash-dir"]),
                "--prefix"
            )
            .unwrap()
            .as_deref(),
            Some("-dash-dir")
        );
    }

    #[test]
    fn argv_without_option_values_does_not_treat_flag_values_as_verbs() {
        let stripped = argv_without_option_values(&args(&[
            "tokenzero-mcp",
            "install",
            "--prefix",
            "sbom",
            "--binary",
            "help",
        ]));
        assert!(
            stripped.iter().any(|a| a == "install"),
            "install verb must remain: {stripped:?}"
        );
        assert!(
            !stripped.iter().any(|a| a == "sbom" || a == "help"),
            "option values must not look like verbs: {stripped:?}"
        );

        let mode = argv_without_option_values(&args(&["tokenzero-mcp", "--mode", "mcp"]));
        assert!(
            mode.iter().any(|a| a == "--mode"),
            "flag must remain so unknown options still fail loud: {mode:?}"
        );
        assert!(
            !mode.iter().any(|a| a == "mcp"),
            "--mode mcp must not be an unknown subcommand: {mode:?}"
        );
    }
}
