use super::*;
use crate::zerostack_store::allowed_roots_for_workspace;

fn env_lookup(pairs: &[(&str, &str)], name: &str) -> Option<OsString> {
    pairs
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| OsString::from(value))
}

#[test]
fn unix_global_home_uses_home() {
    let home = home_dir_from_env(|name| env_lookup(&[("HOME", "/home/ada")], name), false).unwrap();

    assert_eq!(home, PathBuf::from("/home/ada"));
}

#[test]
fn explicit_tool_allowed_roots_include_workspace_root() {
    let roots =
        allowed_roots_for_workspace(Path::new("C:\\repo"), &[PathBuf::from("C:\\Users\\Ada")]);

    assert!(roots.contains(&PathBuf::from("C:\\Users\\Ada")));
    assert!(roots.contains(&PathBuf::from("C:\\repo")));
}

#[test]
fn default_allowed_roots_are_current_root_only() {
    let roots = default_allowed_roots(Path::new("."));

    assert_eq!(roots, vec![PathBuf::from(".")]);
}
