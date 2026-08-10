//! The explicit classic MCP binary must fail loudly on CLI-only subcommands.

#[cfg(feature = "surface-mcp")]
use assert_cmd::prelude::*;
#[cfg(feature = "surface-mcp")]
use std::process::Command;

#[cfg(feature = "surface-mcp")]
const CLI_ONLY_VERBS: &[&str] = &["expand", "ingest", "capabilities", "run", "robot-docs"];

#[cfg(feature = "surface-mcp")]
fn assert_rejects_unknown_verb(bin: &str, verb: &str) {
    let output = Command::cargo_bin(bin)
        .unwrap()
        .arg(verb)
        .arg("--raw")
        .arg("tz://blob/deadbeef")
        .output()
        .unwrap();

    let code = output.status.code();
    assert_ne!(
        code,
        Some(0),
        "{bin} {verb} exited 0; a surface binary must not silently succeed on a CLI verb"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(verb),
        "{bin} {verb} stderr must name the rejected argument, got: {stderr}"
    );
    assert!(
        stderr.contains("tokenzero"),
        "{bin} {verb} stderr must point at the real CLI, got: {stderr}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "{bin} {verb} must not emit stdout payload on rejection, got: {stdout}"
    );
}

#[cfg(feature = "surface-mcp")]
#[test]
fn mcp_surface_rejects_cli_verbs_instead_of_silently_serving_stdio() {
    for verb in CLI_ONLY_VERBS {
        assert_rejects_unknown_verb("tokenzero-mcp", verb);
    }
}
