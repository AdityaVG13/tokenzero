//! Gate-C proof that TokenZero retains domain truth without an engine-local planner.

use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use tokenzero_core::operation_abi::{Mutability, operation_by_name};
use tokenzero_core::{ProtocolTokenizer, is_verified_one_token_atom, portable_one_token_atoms};

const FUZZ_LOCK_SHA256: &str = "ae9a0a8cab41b0c6e097298465279ccde3bf6c3563c3137a3c572abd6ff550fa";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    fs::read_to_string(workspace_root().join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

#[test]
fn planner_free_worker_has_only_the_raw_worker_entrypoint() {
    let source_dir = workspace_root().join("crates/tokenzero-codemode/src");
    let entries = fs::read_dir(&source_dir)
        .expect("worker source directory")
        .map(|entry| entry.expect("worker source entry").file_name())
        .collect::<BTreeSet<_>>();
    assert_eq!(entries, BTreeSet::from(["main.rs".into()]));

    let manifest = read("crates/tokenzero-codemode/Cargo.toml");
    for forbidden in [
        "[lib]",
        "surface-codemode",
        "rquickjs",
        "fastmcp",
        "machine-permit",
        "tokenzero-mcp-compat",
        "zero-codemode.workspace",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "worker manifest contains forbidden host marker {forbidden}"
        );
    }
    for required in ["tokenzero-engine", "zero-abi.workspace = true"] {
        assert!(manifest.contains(required), "worker missing {required}");
    }

    let main = read("crates/tokenzero-codemode/src/main.rs");
    for forbidden in ["rquickjs", "fastmcp", "machine_permit", "zero_codemode"] {
        assert!(
            !main.contains(forbidden),
            "raw-worker entrypoint contains forbidden host marker {forbidden}"
        );
    }
    assert!(main.contains("maybe_run_raw_worker_from_args"));
}

#[test]
fn aggregate_bindings_effects_and_protocol_atoms_remain_owned() {
    for (tool, binding) in [
        ("tz_read", "zero.read"),
        ("tz_find", "zero.find"),
        ("tz_tree", "zero.tree"),
        ("tz_shell", "zero.shell"),
        ("tz_edit", "zero.edit"),
        ("tz_expand", "zero.token.expand"),
    ] {
        let operation = operation_by_name(tool).unwrap_or_else(|| panic!("missing {tool}"));
        assert_eq!(operation.exposure.codemode_binding, Some(binding));
    }
    assert_eq!(
        operation_by_name("tz_read").expect("read").mutability,
        Mutability::ReadOnly
    );
    assert_eq!(
        operation_by_name("tz_edit").expect("edit").mutability,
        Mutability::WorkspaceMutating
    );

    for atom in portable_one_token_atoms() {
        for tokenizer in ProtocolTokenizer::ALL {
            assert!(
                is_verified_one_token_atom(tokenizer, atom),
                "{atom:?} lost tokenizer verification for {tokenizer:?}"
            );
        }
    }
    let atoms = read("crates/tokenzero-core/src/protocol_atoms.rs");
    assert!(atoms.contains("pub enum AckClass"));
    assert!(atoms.contains("pub fn render_ack"));
}

#[test]
fn local_manifests_have_no_retired_host_or_gate_dependencies() {
    let root = read("Cargo.toml");
    for forbidden in [
        "rquickjs",
        "fastmcp-rust",
        "zerostack-machine-permit",
        "zero-gate =",
    ] {
        assert!(
            !root.contains(forbidden),
            "workspace retains forbidden dependency {forbidden}"
        );
    }

    let engine = read("crates/tokenzero-engine/Cargo.toml");
    assert!(!engine.contains("zero-gate"));
    let wire = read("crates/tokenzero-engine/src/codemode_wire.rs");
    for forbidden in [
        "CodemodeExecuteHook",
        "CODEMODE_EXECUTE_HOOK",
        "register_codemode_execute_hook",
        "pub fn codemode_execute",
    ] {
        assert!(
            !wire.contains(forbidden),
            "retired hook remains: {forbidden}"
        );
    }

    let cli = read("crates/tokenzero/Cargo.toml");
    assert!(cli.contains("surface-mcp"));
    assert!(!cli.contains("surface-codemode"));
    assert!(!cli.contains("dep:tokenzero-codemode"));
}

#[test]
fn process_observation_and_foreign_fuzz_lock_are_preserved() {
    let hooks = read("crates/tokenzero-engine/src/shell_hooks.rs");
    assert!(hooks.contains("pub struct ProcessHooks"));
    assert!(hooks.contains("process_observation_snapshot"));
    let worker = read("crates/tokenzero-engine/src/raw_worker_v2_impl.rs");
    assert!(worker.contains("ProcessHooks::with_note_child"));
    assert!(worker.contains("v2_note_child"));

    let bytes = fs::read(workspace_root().join("fuzz/Cargo.lock")).expect("fuzz lock");
    let digest = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(digest, FUZZ_LOCK_SHA256);
}
