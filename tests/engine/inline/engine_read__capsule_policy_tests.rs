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
