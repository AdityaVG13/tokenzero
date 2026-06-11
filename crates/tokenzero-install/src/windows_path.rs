use crate::*;

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
pub(crate) fn is_windows_user_path_write(_row: &InstallWrite) -> bool {
    false
}

#[cfg(windows)]
pub(crate) fn is_windows_user_path_entry(path: &str) -> bool {
    path.eq_ignore_ascii_case(WINDOWS_USER_PATH_REGISTRY)
}

#[cfg(not(windows))]
pub(crate) fn is_windows_user_path_entry(_path: &str) -> bool {
    false
}

#[cfg(windows)]
pub(crate) fn paths_equal(a: &Path, b: &Path) -> bool {
    let a = a
        .canonicalize()
        .unwrap_or_else(|_| a.to_path_buf())
        .display()
        .to_string();
    let b = b
        .canonicalize()
        .unwrap_or_else(|_| b.to_path_buf())
        .display()
        .to_string();
    a.eq_ignore_ascii_case(&b)
}

#[cfg(windows)]
pub(crate) fn windows_path_with_tokenzero_bin(root: &Path, previous: Option<&str>) -> String {
    let bin = root.join(".tokenzero").join("bin").display().to_string();
    let mut entries = vec![bin.clone()];
    if let Some(previous) = previous {
        for entry in previous
            .split(';')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if !paths_equal(Path::new(entry), Path::new(&bin)) {
                entries.push(entry.to_string());
            }
        }
    }
    entries.join(";")
}

#[cfg(not(windows))]
pub(crate) fn windows_path_with_tokenzero_bin(_root: &Path, previous: Option<&str>) -> String {
    previous.unwrap_or_default().to_string()
}

#[cfg(windows)]
pub(crate) fn windows_user_path() -> std::io::Result<Option<String>> {
    let output = Command::new("reg")
        .args(["query", "HKCU\\Environment", "/v", "Path"])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().map(str::trim_start) {
        if let Some(rest) = line.strip_prefix("Path") {
            let rest = rest.trim_start();
            for kind in ["REG_EXPAND_SZ", "REG_SZ"] {
                if let Some(value) = rest.strip_prefix(kind) {
                    return Ok(Some(value.trim_start().to_string()));
                }
            }
        }
    }
    Ok(None)
}

#[cfg(not(windows))]
pub(crate) fn windows_user_path() -> std::io::Result<Option<String>> {
    Ok(None)
}

#[cfg(windows)]
pub(crate) fn write_windows_user_path(value: &str) -> std::io::Result<()> {
    let status = Command::new("reg")
        .args([
            "add",
            "HKCU\\Environment",
            "/v",
            "Path",
            "/t",
            "REG_EXPAND_SZ",
            "/d",
            value,
            "/f",
        ])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::other("failed to update HKCU user Path"))
    }
}

#[cfg(not(windows))]
pub(crate) fn write_windows_user_path(_value: &str) -> std::io::Result<()> {
    Ok(())
}

#[cfg(windows)]
pub(crate) fn delete_windows_user_path() -> std::io::Result<()> {
    let status = Command::new("reg")
        .args(["delete", "HKCU\\Environment", "/v", "Path", "/f"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::other("failed to delete HKCU user Path"))
    }
}

#[cfg(not(windows))]
pub(crate) fn delete_windows_user_path() -> std::io::Result<()> {
    Ok(())
}
