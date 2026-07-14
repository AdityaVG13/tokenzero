use super::*;
use super::support::*;

#[test]
fn edit_applies_multi_hunk_batches_byte_exact() {
    let (_dir, file, engine) = setup_file("sample.txt", "alpha\nbeta\ngamma");
    let response = engine.edit(&file, &[hunk("alpha", "ALPHA", false), hunk("gamma", "gamma\ndelta", false)], false, false, Mode::Auto, 4000);
    assert_status_ok(&response);
    assert_eq!(fs::read(&file).unwrap(), b"ALPHA\nbeta\ngamma\ndelta");
    let text = &response.visible.as_ref().unwrap().text;
    assert!(
        text.starts_with(&format!("# edit {} — 2 hunks applied (+2 -1 lines)", file.display())),
        "{text}"
    );
    assert!(text.contains("-alpha") && text.contains("+ALPHA"), "{text}");
    let kinds: Vec<&str> = response.refs.iter().map(|r| r.kind.as_str()).collect();
    for kind in ["blob", "file", "undo"] {
        assert!(kinds.contains(&kind), "missing {kind} ref: {kinds:?}");
    }
    let accounting = response.accounting.as_ref().unwrap();
    assert!(accounting.visible_tokens <= accounting.raw_tokens);
}

#[test]
fn edit_rejects_ambiguous_and_missing_hunks_without_writing() {
    let original = "dup\ndup\nkeep\n";
    let (_dir, file, engine) = setup_file("sample.txt", original);
    let ambiguous = engine.edit(&file, &[hunk("dup", "other", false)], false, false, Mode::Auto, 4000);
    assert_error_code(&ambiguous, "ambiguous_hunk");
    assert_eq!(fs::read_to_string(&file).unwrap(), original);

    let missing = engine.edit(&file, &[hunk("keep", "kept", false), hunk("absent", "anything", false)], false, false, Mode::Auto, 4000);
    assert_error_code(&missing, "hunk_not_found");
    assert!(missing.error.as_ref().unwrap().message.contains("edits[1]"));
    assert_eq!(fs::read_to_string(&file).unwrap(), original);

    let no_op = engine.edit(&file, &[hunk("keep", "keep", false)], false, false, Mode::Auto, 4000);
    assert_error_code(&no_op, "no_op_hunk");
    assert_eq!(fs::read_to_string(&file).unwrap(), original);
}

#[test]
fn edit_hunk_not_found_hints_at_closest_line() {
    let (_dir, file, engine) = setup_file("sample.rs", "fn alpha() {}\nfn beta() {}\n");
    let response = engine.edit(&file, &[hunk("fn alpha() {}\nfn gamma() {}", "x", false)], false, false, Mode::Auto, 4000);
    assert_error_code(&response, "hunk_not_found");
    assert!(
        response.error.as_ref().unwrap().message.contains("closest line 1: fn alpha() {}"),
        "{:?}", response.error
    );
}

#[test]
fn edit_replace_all_replaces_every_occurrence() {
    let (_dir, file, engine) = setup_file("sample.txt", "x = 1\nx = 2\nx = 3\n");
    let response = engine.edit(&file, &[hunk("x = ", "y = ", true)], false, false, Mode::Auto, 4000);
    assert_status_ok(&response);
    assert_eq!(fs::read_to_string(&file).unwrap(), "y = 1\ny = 2\ny = 3\n");
    let telemetry = response.telemetry.unwrap();
    assert_eq!(telemetry["lines_added"], 3);
    assert_eq!(telemetry["lines_removed"], 3);
}

#[test]
fn edit_create_writes_new_file_and_rejects_existing() {
    let (dir, engine) = setup_default();
    let file = dir.path().join("new.txt");
    let create_hunk = [hunk("", "one\ntwo\n", false)];
    let response = engine.edit(&file, &create_hunk, true, false, Mode::Auto, 4000);
    assert_status_ok(&response);
    assert_eq!(fs::read_to_string(&file).unwrap(), "one\ntwo\n");

    let existing = engine.edit(&file, &create_hunk, true, false, Mode::Auto, 4000);
    assert_error_code(&existing, "edit_failed");
    assert_eq!(fs::read_to_string(&file).unwrap(), "one\ntwo\n");

    let bad_shape = engine.edit(
        &dir.path().join("other.txt"),
        &[hunk("not-empty", "content", false)],
        true, false, Mode::Auto, 4000,
    );
    assert_error_code(&bad_shape, "edit_failed");
    assert!(!dir.path().join("other.txt").exists());
}

#[test]
fn edit_dry_run_previews_without_writing() {
    let (_dir, file, engine) = setup_file("sample.txt", "alpha\n");
    let response = engine.edit(&file, &[hunk("alpha", "beta", false)], false, true, Mode::Auto, 4000);
    assert_status_ok(&response);
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha\n");
    let text = &response.visible.as_ref().unwrap().text;
    assert!(
        text.starts_with(&format!("# edit {} — dry-run: 1 hunks would apply", file.display())),
        "{text}"
    );
    assert_eq!(response.telemetry.as_ref().unwrap()["dry_run"], true);
    assert_eq!(expand_ok(&engine, &blob_ref(&response)), "beta\n");
}

#[test]
fn edit_undo_ref_recovers_exact_pre_image() {
    let original = "alpha\nbeta";
    let (_dir, file, engine) = setup_file("sample.txt", original);
    let response = engine.edit(&file, &[hunk("beta", "gamma", false)], false, false, Mode::Auto, 4000);
    assert_status_ok(&response);
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha\ngamma");
    assert_eq!(expand_ok(&engine, &ref_of(&response, "undo")), original);
}

#[test]
fn edit_outside_allowed_roots_is_rejected() {
    let (dir, engine) = setup_default();
    let outside = tempdir().unwrap();
    let file = outside.path().join("sample.txt");
    fs::write(&file, "alpha\n").unwrap();
    let response = engine.edit(&file, &[hunk("alpha", "beta", false)], false, false, Mode::Auto, 4000);
    assert_error_code(&response, "path_not_allowed");
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha\n");
    let _ = dir;
}

#[test]
fn path_allowed_matrix() {
    // Labeled cases preserve prefix-sibling, unresolved-parent, and relative-root contracts.
    let base = tempdir().unwrap();
    let root = base.path().join("ws");
    fs::create_dir(&root).unwrap();
    let sibling = base.path().join("wsbackup");
    fs::create_dir(&sibling).unwrap();
    let sibling_file = sibling.join("sample.txt");
    fs::write(&sibling_file, "alpha\n").unwrap();
    fs::write(root.join("inside.txt"), "alpha\n").unwrap();
    let sub = root.join("sub");
    fs::create_dir(&sub).unwrap();
    let engine = default_engine(&root);

    let cases: &[(&str, PathBuf, bool)] = &[
        ("prefix_sibling", sibling_file, false),
        ("inside_root", root.join("inside.txt"), true),
        ("unresolved_parent_escape", root.join("missing").join("..").join("..").join("out.txt"), false),
        ("existing_parent_resolve", sub.join("..").join("inside.txt"), true),
        ("relative_inside", PathBuf::from("inside.txt"), true),
        ("relative_escape", Path::new("..").join("outside.txt"), false),
    ];
    for (label, path, allowed) in cases {
        assert_eq!(
            engine.path_allowed(path),
            *allowed,
            "{label}: path={path:?}"
        );
    }
}

#[test]
fn edit_rejects_non_utf8_files() {
    let (_dir, file, engine) = setup_file("blob.bin", [0xff, 0xfe, 0x00, 0x41]);
    let response = engine.edit(&file, &[hunk("a", "b", false)], false, false, Mode::Auto, 4000);
    assert_error_code(&response, "not_utf8");
    assert_eq!(fs::read(&file).unwrap(), vec![0xff, 0xfe, 0x00, 0x41]);
}

#[test]
fn read_after_edit_serves_unchanged_note_not_diff() {
    let (_dir, file, engine, _) = setup_dedup("sample.rs");
    read_ok(&engine, &file);
    let edit = engine.edit(&file, &[hunk("line 01", "line 01 edited", false)], false, false, Mode::Auto, 4000);
    assert_status_ok(&edit);
    let text = visible_text(&read_ok(&engine, &file));
    assert!(text.starts_with("unchanged:"), "{text}");
}
