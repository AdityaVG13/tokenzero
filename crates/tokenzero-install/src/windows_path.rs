use crate::*;

#[cfg(windows)]
const USER_ENVIRONMENT: &str = "HKCU\\Environment";

#[cfg(windows)]
pub(crate) fn is_real_windows_user_root(root: &Path) -> bool {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .is_some_and(|profile| paths_equal(&profile, root))
}

#[cfg(windows)]
pub(crate) fn is_windows_user_path_write(row: &InstallWrite) -> bool {
    row.capability == "path" && row.action == "prepend" && is_windows_user_path_entry(&row.path)
}

#[cfg(not(windows))]
pub(crate) fn is_windows_user_path_write(_: &InstallWrite) -> bool {
    false
}

#[cfg(windows)]
pub(crate) fn is_windows_user_path_entry(path: &str) -> bool {
    path.eq_ignore_ascii_case(WINDOWS_USER_PATH_REGISTRY)
}

#[cfg(not(windows))]
pub(crate) fn is_windows_user_path_entry(_: &str) -> bool {
    false
}

#[cfg(windows)]
pub(crate) fn paths_equal(a: &Path, b: &Path) -> bool {
    let normalize = |path: &Path| {
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string()
    };
    normalize(a).eq_ignore_ascii_case(&normalize(b))
}

#[cfg(windows)]
pub(crate) fn windows_path_with_tokenzero_bin(root: &Path, previous: Option<&str>) -> String {
    let bin = root.join(".tokenzero").join("bin").display().to_string();
    let mut entries = vec![bin.clone()];
    entries.extend(
        previous
            .into_iter()
            .flat_map(|value| value.split(';'))
            .map(str::trim)
            .filter(|entry| !entry.is_empty() && !paths_equal(Path::new(entry), Path::new(&bin)))
            .map(str::to_owned),
    );
    entries.join(";")
}

#[cfg(not(windows))]
pub(crate) fn windows_path_with_tokenzero_bin(_: &Path, previous: Option<&str>) -> String {
    previous.unwrap_or_default().to_string()
}

#[cfg(windows)]
pub(crate) fn windows_user_path() -> std::io::Result<Option<String>> {
    let output = Command::new("reg")
        .args(["query", USER_ENVIRONMENT, "/v", "Path"])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().map(str::trim_start).find_map(|line| {
        let rest = line.strip_prefix("Path")?.trim_start();
        ["REG_EXPAND_SZ", "REG_SZ"]
            .into_iter()
            .find_map(|kind| rest.strip_prefix(kind).map(|value| value.trim_start().to_owned()))
    }))
}

#[cfg(not(windows))]
pub(crate) fn windows_user_path() -> std::io::Result<Option<String>> {
    Ok(None)
}

#[cfg(windows)]
fn update_windows_user_path(args: &[&str], failure: &'static str) -> std::io::Result<()> {
    Command::new("reg")
        .args(args)
        .status()?
        .success()
        .then_some(())
        .ok_or_else(|| Error::other(failure))
}

#[cfg(windows)]
pub(crate) fn write_windows_user_path(value: &str) -> std::io::Result<()> {
    update_windows_user_path(
        &["add", USER_ENVIRONMENT, "/v", "Path", "/t", "REG_EXPAND_SZ", "/d", value, "/f"],
        "failed to update HKCU user Path",
    )
}

#[cfg(not(windows))]
pub(crate) fn write_windows_user_path(_: &str) -> std::io::Result<()> {
    Ok(())
}

#[cfg(windows)]
pub(crate) fn delete_windows_user_path() -> std::io::Result<()> {
    update_windows_user_path(
        &["delete", USER_ENVIRONMENT, "/v", "Path", "/f"],
        "failed to delete HKCU user Path",
    )
}

#[cfg(not(windows))]
pub(crate) fn delete_windows_user_path() -> std::io::Result<()> {
    Ok(())
}
