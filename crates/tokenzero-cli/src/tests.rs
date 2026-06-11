use super::*;

fn env_lookup(pairs: &[(&str, &str)], name: &str) -> Option<OsString> {
    pairs
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| OsString::from(value))
}

#[test]
fn windows_global_home_prefers_userprofile() {
    let home = home_dir_from_env(
        |name| {
            env_lookup(
                &[("HOME", "C:\\GitHome"), ("USERPROFILE", "C:\\Users\\Ada")],
                name,
            )
        },
        true,
    )
    .unwrap();

    assert_eq!(home, PathBuf::from("C:\\Users\\Ada"));
}

#[test]
fn windows_global_home_falls_back_to_drive_and_path() {
    let home = home_dir_from_env(
        |name| env_lookup(&[("HOMEDRIVE", "C:"), ("HOMEPATH", "\\Users\\Ada")], name),
        true,
    )
    .unwrap();

    assert_eq!(home, PathBuf::from("C:\\Users\\Ada"));
}

#[test]
fn unix_global_home_uses_home() {
    let home = home_dir_from_env(|name| env_lookup(&[("HOME", "/home/ada")], name), false).unwrap();

    assert_eq!(home, PathBuf::from("/home/ada"));
}

#[test]
fn explicit_tool_allowed_roots_include_workspace_root() {
    let roots = tool_allowed_roots(Path::new("C:\\repo"), &[PathBuf::from("C:\\Users\\Ada")]);

    assert!(roots.contains(&PathBuf::from("C:\\Users\\Ada")));
    assert!(roots.contains(&PathBuf::from("C:\\repo")));
}

#[test]
fn default_allowed_roots_are_current_root_only() {
    let roots = default_allowed_roots(Path::new("."));

    assert_eq!(roots, vec![PathBuf::from(".")]);
}

#[test]
fn windows_display_quotes_findstr_argv_metacharacters_for_cmd() {
    let argv = vec![
        "findstr".to_string(),
        "error|warning".to_string(),
        "src\\sample.txt".to_string(),
    ];

    assert_eq!(
        display_command_for_platform(&argv, None, "windows"),
        "findstr \"error|warning\" src\\sample.txt"
    );
    assert_eq!(
        display_command_for_platform(&argv, None, "powershell"),
        "findstr 'error|warning' src\\sample.txt"
    );
}

#[test]
fn windows_display_leaves_shell_builtins_as_shell_script() {
    let argv = vec![
        "echo".to_string(),
        "ok".to_string(),
        "|".to_string(),
        "findstr".to_string(),
        "ok".to_string(),
    ];

    assert_eq!(
        display_command_for_platform(&argv, None, "windows"),
        "echo ok | findstr ok"
    );
}
