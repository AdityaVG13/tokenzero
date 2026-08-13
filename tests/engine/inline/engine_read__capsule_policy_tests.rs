use std::fs;

use tempfile::tempdir;

use super::*;

#[test]
fn auto_read_inlines_at_threshold_and_uses_exact_ref_above_it() {
    let dir = tempdir().unwrap();
    let inline_path = dir.path().join("inline.txt");
    let exact_path = dir.path().join("exact.txt");
    fs::write(&inline_path, "abcdefgh").unwrap();
    fs::write(&exact_path, "abcdefghi").unwrap();

    let mut config = EngineConfig::for_root(dir.path());
    config.capsule_exact_ref_threshold_bytes = 8;
    config.session_dedup = false;
    let engine = TokenZeroEngine::new(config);

    let inline = engine.read(&[inline_path], Mode::Auto, None, None, false, 1, 4000);
    assert_eq!(inline.visible.unwrap().text, "abcdefgh");

    let exact = engine.read(&[exact_path], Mode::Auto, None, None, false, 1, 4000);
    let visible = exact.visible.unwrap().text;
    assert!(!visible.contains("abcdefghi"), "{visible}");
    assert!(visible.contains("exact payload stored"), "{visible}");
    assert!(visible.contains("#B0-9"), "{visible}");
}

#[test]
fn read_missing_file_names_no_such_file_hint() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("absent.txt");
    let mut config = EngineConfig::for_root(dir.path());
    config.session_dedup = false;
    let engine = TokenZeroEngine::new(config);
    let response = engine.read(&[missing.clone()], Mode::Auto, None, None, false, 1, 4000);
    let error = response.error.expect("missing path must fail");
    assert_eq!(error.code, "read_failed");
    assert!(error.message.contains("no such file"), "{}", error.message);
    assert!(
        error.message.contains(&missing.display().to_string()),
        "{}",
        error.message
    );
}

#[test]
fn read_directory_names_use_tree_hint() {
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.session_dedup = false;
    let engine = TokenZeroEngine::new(config);
    let response = engine.read(
        &[dir.path().to_path_buf()],
        Mode::Auto,
        None,
        None,
        false,
        1,
        4000,
    );
    let error = response.error.expect("directory path must fail");
    assert_eq!(error.code, "read_failed");
    assert!(
        error.message.contains("path is a directory - use tree"),
        "{}",
        error.message
    );
}
