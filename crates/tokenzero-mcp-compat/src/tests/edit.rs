use super::*;
use std::sync::Arc;
use tempfile::tempdir;
use tokenzero_core::MCP_SCHEMA_VERSION;

use super::support::*;

#[test]
fn edit_applies_multi_hunk_batches_byte_exact() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    // No trailing newline on purpose: the write must stay byte-exact.
    fs::write(&file, "alpha\nbeta\ngamma").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let edits = vec![
        hunk("alpha", "ALPHA", false),
        hunk("gamma", "gamma\ndelta", false),
    ];
    let response = engine.edit(&file, &edits, false, false, Mode::Auto, 4000);
    assert_eq!(response.status, "ok", "{:?}", response.error);
    assert_eq!(fs::read(&file).unwrap(), b"ALPHA\nbeta\ngamma\ndelta");

    assert_eq!(
        response
            .visible
            .as_ref()
            .map(|visible| visible.text.as_str()),
        Some(""),
        "ACK/2 keeps successful pure mutations payload-silent"
    );
    assert!(
        response.detail_ref.is_some(),
        "silent mutation needs a detail ref"
    );

    let kinds: Vec<&str> = response.refs.iter().map(|r| r.kind.as_str()).collect();
    for kind in ["blob", "file", "undo"] {
        assert!(kinds.contains(&kind), "missing {kind} ref: {kinds:?}");
    }
    let accounting = response.accounting.as_ref().unwrap();
    assert!(
        accounting.visible_tokens <= accounting.raw_tokens,
        "adaptive floor: visible must never cost more than raw"
    );
}

#[test]
fn edit_rejects_ambiguous_and_missing_hunks_without_writing() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    let original = "dup\ndup\nkeep\n";
    fs::write(&file, original).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let ambiguous = engine.edit(
        &file,
        &[hunk("dup", "other", false)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(ambiguous.status, "error");
    assert_eq!(ambiguous.error.unwrap().code, "ambiguous_hunk");
    assert_eq!(fs::read_to_string(&file).unwrap(), original);

    // A failing hunk later in the batch rolls back the whole batch.
    let missing = engine.edit(
        &file,
        &[
            hunk("keep", "kept", false),
            hunk("absent", "anything", false),
        ],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(missing.status, "error");
    let error = missing.error.unwrap();
    assert_eq!(error.code, "hunk_not_found");
    assert!(error.message.contains("edits[1]"), "{}", error.message);
    assert_eq!(fs::read_to_string(&file).unwrap(), original);

    let no_op = engine.edit(
        &file,
        &[hunk("keep", "keep", false)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(no_op.status, "error");
    assert_eq!(no_op.error.unwrap().code, "no_op_hunk");
    assert_eq!(fs::read_to_string(&file).unwrap(), original);
}

#[test]
fn edit_hunk_not_found_hints_at_closest_line() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, "fn alpha() {}\nfn beta() {}\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.edit(
        &file,
        &[hunk("fn alpha() {}\nfn gamma() {}", "x", false)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(response.status, "error");
    let error = response.error.unwrap();
    assert_eq!(error.code, "hunk_not_found");
    assert!(
        error.message.contains("closest line 1: fn alpha() {}"),
        "{}",
        error.message
    );
}

#[test]
fn edit_replace_all_replaces_every_occurrence() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    fs::write(&file, "x = 1\nx = 2\nx = 3\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.edit(
        &file,
        &[hunk("x = ", "y = ", true)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(response.status, "ok", "{:?}", response.error);
    assert_eq!(fs::read_to_string(&file).unwrap(), "y = 1\ny = 2\ny = 3\n");
    let telemetry = response.telemetry.unwrap();
    assert_eq!(telemetry["lines_added"], 3);
    assert_eq!(telemetry["lines_removed"], 3);
}

#[test]
fn edit_create_writes_new_file_and_rejects_existing() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("new.txt");
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let create_hunk = [hunk("", "one\ntwo\n", false)];
    let response = engine.edit(&file, &create_hunk, true, false, Mode::Auto, 4000);
    assert_eq!(response.status, "ok", "{:?}", response.error);
    assert_eq!(fs::read_to_string(&file).unwrap(), "one\ntwo\n");

    let existing = engine.edit(&file, &create_hunk, true, false, Mode::Auto, 4000);
    assert_eq!(existing.status, "error");
    assert_eq!(existing.error.unwrap().code, "edit_failed");
    assert_eq!(fs::read_to_string(&file).unwrap(), "one\ntwo\n");

    let bad_shape = engine.edit(
        &dir.path().join("other.txt"),
        &[hunk("not-empty", "content", false)],
        true,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(bad_shape.status, "error");
    assert_eq!(bad_shape.error.unwrap().code, "edit_failed");
    assert!(!dir.path().join("other.txt").exists());
}

#[test]
fn edit_dry_run_previews_without_writing() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    fs::write(&file, "alpha\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.edit(
        &file,
        &[hunk("alpha", "beta", false)],
        false,
        true,
        Mode::Auto,
        4000,
    );
    assert_eq!(response.status, "ok", "{:?}", response.error);
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha\n");
    let text = &response.visible.as_ref().unwrap().text;
    assert!(
        text.starts_with(&format!(
            "# edit {} — dry-run: 1 hunks would apply",
            file.display()
        )),
        "{text}"
    );
    assert_eq!(response.telemetry.as_ref().unwrap()["dry_run"], true);

    // The post-image blob still recovers the would-be content.
    let post_ref = response
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let expanded = engine.expand(&post_ref, Some("raw"), None, None, None, None);
    assert_eq!(expanded.visible.unwrap().text, "beta\n");
}

#[test]
fn edit_undo_ref_recovers_exact_pre_image() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.txt");
    let original = "alpha\nbeta";
    fs::write(&file, original).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.edit(
        &file,
        &[hunk("beta", "gamma", false)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(response.status, "ok", "{:?}", response.error);
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha\ngamma");

    let undo_ref = response
        .refs
        .iter()
        .find(|r| r.kind == "undo")
        .unwrap()
        .ref_id
        .clone();
    let expanded = engine.expand(&undo_ref, Some("raw"), None, None, None, None);
    assert_eq!(expanded.visible.unwrap().text, original);
}

#[test]
fn edit_outside_allowed_roots_is_rejected() {
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let file = outside.path().join("sample.txt");
    fs::write(&file, "alpha\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.edit(
        &file,
        &[hunk("alpha", "beta", false)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(response.status, "error");
    assert_eq!(response.error.unwrap().code, "path_not_allowed");
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha\n");
}

#[test]
fn path_allowed_rejects_prefix_sibling_of_allowed_root() {
    // /base/ws must not admit /base/wsbackup even though the latter is a
    // byte-prefix match; Path::starts_with compares whole components.
    let base = tempdir().unwrap();
    let root = base.path().join("ws");
    fs::create_dir(&root).unwrap();
    let sibling = base.path().join("wsbackup");
    fs::create_dir(&sibling).unwrap();
    let file = sibling.join("sample.txt");
    fs::write(&file, "alpha\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(&root));

    assert!(!engine.path_allowed(&file));
    assert!(engine.path_allowed(&root.join("inside.txt")));
}

#[test]
fn path_allowed_rejects_unresolved_parent_components() {
    // `..` behind a nonexistent component survives
    // canonicalize_existing_prefix; it must fail closed instead of
    // passing the component-wise root check.
    let base = tempdir().unwrap();
    let root = base.path().join("ws");
    fs::create_dir(&root).unwrap();
    let escape = root.join("missing").join("..").join("..").join("out.txt");
    let engine = TokenZeroEngine::new(EngineConfig::for_root(&root));

    assert!(!engine.path_allowed(&escape));
    // `..` behind an existing component still resolves and stays allowed.
    let sub = root.join("sub");
    fs::create_dir(&sub).unwrap();
    assert!(engine.path_allowed(&sub.join("..").join("inside.txt")));
}

#[test]
fn path_allowed_resolves_relative_paths_against_call_root() {
    // A bare relative path must be resolved against the engine's call root,
    // not the process cwd, before the allowlist check. This matters when the
    // workspace root is routed or otherwise differs from the current directory.
    let base = tempdir().unwrap();
    let root = base.path().join("ws");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("inside.txt"),
        "alpha
",
    )
    .unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(&root));

    // Relative path under the call root is allowed even when the process cwd
    // is somewhere else (here, the workspace root itself, not a parent).
    assert!(engine.path_allowed(Path::new("inside.txt")));
    // Relative paths that escape the call root via `..` must still be rejected.
    assert!(!engine.path_allowed(Path::new("..").join("outside.txt").as_path()));
}

#[test]
fn edit_rejects_non_utf8_files() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("blob.bin");
    fs::write(&file, [0xff, 0xfe, 0x00, 0x41]).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.edit(
        &file,
        &[hunk("a", "b", false)],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(response.status, "error");
    assert_eq!(response.error.unwrap().code, "not_utf8");
    assert_eq!(fs::read(&file).unwrap(), vec![0xff, 0xfe, 0x00, 0x41]);
}

#[test]
fn read_after_edit_serves_unchanged_note_not_diff() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.rs");
    fs::write(&file, dedup_fixture_content()).unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    read_ok(&engine, &file);
    let edit = engine.edit(
        &file,
        &[EditHunk {
            find: "line 01".to_string(),
            replace: "line 01 edited".to_string(),
            replace_all: false,
        }],
        false,
        false,
        Mode::Auto,
        4000,
    );
    assert_eq!(edit.status, "ok");

    // The edit seeded the seen-set with the post-image: the re-read is
    // an unchanged note, not a diff against the pre-edit serve.
    let reread = read_ok(&engine, &file);
    let text = visible_text(&reread);
    assert!(text.starts_with("unchanged:"), "{text}");
}
