//! Release artifact `tokenzero-mcp`: FastMCP per-op surface only (tokenzero-irx9.3).
//!
//! Packaging subcommands (install/uninstall/sbom/doctor/help) always exit without
//! opening a stdio server. Domain execution remains shared with the raw worker.

use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use tokenzero_core::McpToolSurface;
use tokenzero_install::packaging::{
    assert_packaged_surface_features, assert_surface_compiled, default_install_prefix,
    install_surface, package_identity, reject_non_stdio_args, sbom_document,
    semantic_contract_digest, uninstall_report, uninstall_surface, PackageSurface,
};
use tokenzero_mcp_compat::{run_fastmcp_stdio, EngineConfig};

const SURFACE: PackageSurface = PackageSurface::Mcp;

fn is_broken_pipe(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::BrokenPipe
}

fn map_stdout_write(result: io::Result<()>) -> io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(err) if is_broken_pipe(&err) => Ok(()),
        Err(err) => Err(err),
    }
}

fn write_stdout(text: &str) -> io::Result<()> {
    let mut stdout = io::stdout();
    map_stdout_write(
        stdout
            .write_all(text.as_bytes())
            .and_then(|_| stdout.flush()),
    )
}

fn writeln_stdout(text: impl AsRef<str>) -> io::Result<()> {
    let text = text.as_ref();
    if text.ends_with('\n') {
        write_stdout(text)
    } else {
        write_stdout(&format!("{text}\n"))
    }
}

/// Packaging verbs print to stdout, not the FastMCP JSON-RPC stream. Broken
/// pipe is a clean CLI exit (same class as `tokenzero` after 6586c19); other
/// write errors fail loud.
fn emit_stdout(text: impl AsRef<str>) {
    if let Err(error) = writeln_stdout(text) {
        eprintln!("tokenzero-mcp: {error}");
        process::exit(2);
    }
}

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
        emit_stdout(format!(
            "tokenzero-mcp — FastMCP per-operation surface (mutually exclusive with tokenzero-codemode)\n\
             semantic_contract_digest: {}\n\
             selection: native CodeMode clients install this package\n\
             usage: tokenzero-mcp | tokenzero-mcp doctor | tokenzero-mcp sbom | tokenzero-mcp install|uninstall\n\
             usage: tokenzero-mcp raw-worker [--handshake|--once JSON|--root DIR]  # OMP private worker\n\
             identity: {}",
            semantic_contract_digest(),
            id
        ));
        process::exit(0);
    }

    if verbs.iter().any(|a| a == "sbom") {
        emit_stdout(serde_json::to_string_pretty(&sbom_document(SURFACE)).unwrap());
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
        emit_stdout(format!(
            "package: artifact={} surface={} semantic_contract_digest={}",
            id["artifact"], id["surface"], id["semantic_contract_digest"]
        ));
        process::exit(0);
    }

    if let Err(e) = require_classic_surface_flags(&args) {
        eprintln!("{e}");
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
    let root = match stdio_root_from_args(&args, cwd) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("tokenzero-mcp: {error}");
            process::exit(2);
        }
    };
    let mut config = EngineConfig::for_root(&root);
    config.tool_surface = McpToolSurface::Classic;
    // FastMCP run never returns (`!`).
    run_fastmcp_stdio(config);
}

/// Flags whose following argv token is a value, not a packaging verb.
/// Includes CLI `mcp-server` value flags so `tokenzero-mcp --allowed-root help`
/// cannot be scanned as the help verb (same class as `--prefix sbom`).
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
    "--allowed-root",
    "--default-mode",
    "--shell-timeout-seconds",
    "--timeout",
    "--idle-timeout-seconds",
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

/// Parse every `--name VALUE` / `--name=VALUE` occurrence. Missing or
/// flag-shaped values fail loud so `install --prefix --binary …` cannot treat
/// `--binary` as a path. Dash-leading paths must use the equals form
/// (`--prefix=-`). Later duplicates are not ignored: `--mode mcp --mode=codemode`
/// must still refuse CodeMode.
fn parse_flag_values(args: &[String], name: &str) -> Result<Vec<String>, String> {
    let mut values = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == name {
            match args.get(i + 1) {
                Some(value) if !value.starts_with('-') => {
                    values.push(value.clone());
                    i += 2;
                    continue;
                }
                Some(value) => return Err(format!("{name} requires a value (got {value:?})")),
                None => return Err(format!("{name} requires a value")),
            }
        }
        if let Some(rest) = args[i].strip_prefix(&format!("{name}=")) {
            if rest.is_empty() {
                return Err(format!("{name} requires a value"));
            }
            values.push(rest.to_string());
        }
        i += 1;
    }
    Ok(values)
}

fn parse_flag(args: &[String], name: &str) -> Result<Option<String>, String> {
    Ok(parse_flag_values(args, name)?.into_iter().next())
}

fn agreed_path_flag(args: &[String], name: &str) -> Result<Option<String>, String> {
    let mut chosen: Option<String> = None;
    for value in parse_flag_values(args, name)? {
        if let Some(existing) = &chosen {
            if Path::new(existing) != Path::new(&value) {
                return Err(format!(
                    "{name} specified more than once ({existing:?} vs {value:?})"
                ));
            }
        } else {
            chosen = Some(value);
        }
    }
    Ok(chosen)
}

fn require_classic_surface_flags(args: &[String]) -> Result<(), String> {
    for name in ["--mode", "--tool-surface", "--surface"] {
        for value in
            parse_flag_values(args, name).map_err(|error| format!("tokenzero-mcp: {error}"))?
        {
            match value.parse::<McpToolSurface>() {
                Ok(McpToolSurface::Classic) => {}
                Ok(McpToolSurface::CodeMode) => {
                    return Err(
                        "tokenzero-mcp: artifact is locked to classic MCP; refused --mode=codemode. \
Use the ZeroStack aggregate host for plans; it launches tokenzero-codemode only as a raw worker."
                            .into(),
                    );
                }
                Err(error) => return Err(format!("tokenzero-mcp: {error}")),
            }
        }
    }
    Ok(())
}

fn stdio_root_from_args(args: &[String], cwd: PathBuf) -> Result<PathBuf, String> {
    let root = agreed_path_flag(args, "--root")?;
    let repo = agreed_path_flag(args, "--repo")?;
    match (root, repo) {
        (Some(root), Some(repo)) if Path::new(&root) != Path::new(&repo) => {
            Err(format!("--root ({root:?}) and --repo ({repo:?}) disagree"))
        }
        (Some(root), _) | (None, Some(root)) => Ok(PathBuf::from(root)),
        (None, None) => Ok(cwd),
    }
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
            emit_stdout(format!(
                "install: ok surface={} artifact={} prefix={} semantic_contract_digest={} client_config={}",
                state.surface.as_str(),
                state.artifact,
                state.prefix,
                state.semantic_contract_digest,
                state.client_config
            ));
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
            emit_stdout(serde_json::to_string_pretty(&uninstall_report(prev)).unwrap());
        }
        Err(e) => {
            eprintln!("uninstall: FAIL {e}");
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        argv_without_option_values, is_broken_pipe, map_stdout_write, parse_flag,
        require_classic_surface_flags, stdio_root_from_args, VALUE_FLAGS,
    };
    use std::io::{self, ErrorKind};
    use std::path::PathBuf;
    use tokenzero_install::packaging::reject_non_stdio_args;

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
        let later_missing = parse_flag(
            &args(&[
                "tokenzero-mcp",
                "install",
                "--prefix",
                "/tmp/tz",
                "--prefix",
            ]),
            "--prefix",
        )
        .expect_err("a later bare --prefix must fail even after an earlier valid value");
        assert!(
            later_missing.contains("requires a value"),
            "{later_missing}"
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

    #[test]
    fn stripped_stdio_flags_are_accepted_by_reject_non_stdio_args() {
        for argv in [
            args(&["tokenzero-mcp", "--mode", "mcp"]),
            args(&["tokenzero-mcp", "--mode=mcp"]),
            args(&["tokenzero-mcp", "--root", "/tmp/ws"]),
            args(&["tokenzero-mcp", "--repo", "/tmp/repo"]),
        ] {
            let verbs = argv_without_option_values(&argv);
            reject_non_stdio_args("tokenzero-mcp", &verbs).unwrap_or_else(|error| {
                panic!("stdio flags must survive verb-stripping: {argv:?} verbs={verbs:?} {error}")
            });
        }
    }

    #[test]
    fn require_classic_surface_flags_accepts_mcp_and_refuses_codemode_aliases() {
        require_classic_surface_flags(&args(&["tokenzero-mcp", "--mode=mcp"])).unwrap();
        require_classic_surface_flags(&args(&["tokenzero-mcp", "--mode", "classic"])).unwrap();
        require_classic_surface_flags(&args(&["tokenzero-mcp", "--tool-surface", "mcp"])).unwrap();

        let refused =
            require_classic_surface_flags(&args(&["tokenzero-mcp", "--tool-surface", "codemode"]))
                .expect_err("--tool-surface codemode must fail as loudly as --mode=codemode");
        assert!(refused.contains("codemode"), "{refused}");

        let invalid = require_classic_surface_flags(&args(&["tokenzero-mcp", "--mode", "foobar"]))
            .expect_err("unknown --mode must not fall through to stdio");
        assert!(
            invalid.contains("foobar") || invalid.contains("unsupported"),
            "{invalid}"
        );

        let stolen =
            require_classic_surface_flags(&args(&["tokenzero-mcp", "--mode", "--root", "/tmp/ws"]))
                .expect_err("bare --mode must not steal the next flag");
        assert!(
            stolen.contains("--root") || stolen.contains("requires a value"),
            "{stolen}"
        );

        let shadowed = require_classic_surface_flags(&args(&[
            "tokenzero-mcp",
            "--mode",
            "mcp",
            "--mode=codemode",
        ]))
        .expect_err("later --mode=codemode must not be shadowed by an earlier --mode mcp");
        assert!(shadowed.contains("codemode"), "{shadowed}");

        let later_space = require_classic_surface_flags(&args(&[
            "tokenzero-mcp",
            "--mode=mcp",
            "--tool-surface",
            "mcp",
            "--mode",
            "codemode",
        ]))
        .expect_err("later space-form --mode codemode must still refuse");
        assert!(later_space.contains("codemode"), "{later_space}");
    }

    #[test]
    fn stdio_root_from_args_honors_root_and_repo() {
        let cwd = PathBuf::from("/cwd");
        assert_eq!(
            stdio_root_from_args(&args(&["tokenzero-mcp"]), cwd.clone()).unwrap(),
            cwd
        );
        assert_eq!(
            stdio_root_from_args(
                &args(&["tokenzero-mcp", "--root", "/tmp/ws"]),
                PathBuf::from("/cwd")
            )
            .unwrap(),
            PathBuf::from("/tmp/ws")
        );
        assert_eq!(
            stdio_root_from_args(
                &args(&["tokenzero-mcp", "--repo=/tmp/repo"]),
                PathBuf::from("/cwd")
            )
            .unwrap(),
            PathBuf::from("/tmp/repo")
        );
        let disagree = stdio_root_from_args(
            &args(&["tokenzero-mcp", "--root", "/tmp/a", "--repo", "/tmp/b"]),
            PathBuf::from("/cwd"),
        )
        .expect_err("disagreeing --root/--repo must fail loud");
        assert!(disagree.contains("/tmp/a"), "{disagree}");
        assert!(disagree.contains("/tmp/b"), "{disagree}");

        assert_eq!(
            stdio_root_from_args(
                &args(&["tokenzero-mcp", "--root", "/tmp/ws", "--repo", "/tmp/ws/"]),
                PathBuf::from("/cwd")
            )
            .unwrap(),
            PathBuf::from("/tmp/ws"),
            "trailing slash is the same path, not a disagreement"
        );

        let duplicate_root = stdio_root_from_args(
            &args(&["tokenzero-mcp", "--root", "/tmp/a", "--root=/tmp/b"]),
            PathBuf::from("/cwd"),
        )
        .expect_err("duplicate disagreeing --root must not keep the first value");
        assert!(duplicate_root.contains("/tmp/a"), "{duplicate_root}");
        assert!(duplicate_root.contains("/tmp/b"), "{duplicate_root}");

        assert_eq!(
            stdio_root_from_args(
                &args(&["tokenzero-mcp", "--root", "/tmp/ws", "--root=/tmp/ws/"]),
                PathBuf::from("/cwd")
            )
            .unwrap(),
            PathBuf::from("/tmp/ws"),
            "duplicate --root with a trailing slash is the same path"
        );
    }

    #[test]
    fn argv_without_option_values_strips_every_stdio_and_install_value_flag() {
        assert!(
            VALUE_FLAGS.contains(&"--allowed-root"),
            "CLI mcp-server --allowed-root is a value flag sibling of --root/--prefix"
        );
        for flag in VALUE_FLAGS {
            let stripped = argv_without_option_values(&args(&["tokenzero-mcp", flag, "help"]));
            assert!(
                !stripped.iter().any(|a| a == "help"),
                "{flag} value must not be scanned as a verb: {stripped:?}"
            );
            assert!(
                stripped.iter().any(|a| a == *flag),
                "{flag} itself must remain so unknown options still fail loud: {stripped:?}"
            );
        }
    }

    #[test]
    fn cli_mcp_server_value_flags_are_not_help_verbs_then_fail_loud() {
        for flag in [
            "--allowed-root",
            "--default-mode",
            "--shell-timeout-seconds",
            "--timeout",
            "--idle-timeout-seconds",
        ] {
            let argv = args(&["tokenzero-mcp", flag, "help"]);
            let verbs = argv_without_option_values(&argv);
            assert!(
                !verbs.iter().any(|a| a == "help"),
                "{flag} help must not open the help verb: {verbs:?}"
            );
            let Err(error) = reject_non_stdio_args("tokenzero-mcp", &verbs) else {
                panic!("{flag} must fail as an unsupported option after stripping its value")
            };
            assert!(
                error.contains(flag),
                "{flag} must fail as an unsupported option after stripping its value: {error}"
            );
        }
    }

    #[test]
    fn broken_pipe_is_a_clean_write_not_a_panic() {
        let err = io::Error::new(ErrorKind::BrokenPipe, "closed pipe");
        assert!(is_broken_pipe(&err));
        map_stdout_write(Err(err)).expect("broken pipe must not fail the MCP packaging CLI");
    }

    #[test]
    fn other_stdout_errors_still_fail_loud() {
        let err = io::Error::new(ErrorKind::PermissionDenied, "stdout");
        assert!(!is_broken_pipe(&err));
        let message = map_stdout_write(Err(err))
            .expect_err("permission errors must stay visible")
            .to_string();
        assert!(
            message.contains("stdout") || message.contains("Permission"),
            "{message}"
        );
    }
}
