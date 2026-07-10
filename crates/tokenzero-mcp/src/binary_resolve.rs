//! Portable engine/helper binary discovery (wqw.3).
//!
//! Resolution order for TokenZero-owned tool binaries:
//! 1. **env override** (`TOKENZERO_BIN`, `TOKENZERO_RG_PATH`, `TOKENZERO_CURL_PATH`)
//! 2. **PATH** lookup
//! 3. **well-known layouts** (cargo bin, Homebrew, `/usr/local`, `/usr`,
//!    `~/.tokenzero/bin`)
//! 4. **clear error** when nothing resolves
//!
//! No host-absolute personal paths (e.g. `/Users/<you>/AI/.../target/release`)
//! are consulted. Multi-machine installs put `tokenzero` on PATH or under
//! `$HOME/.tokenzero/bin`.

use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const WINDOWS_EXECUTABLE_EXTENSIONS: [&str; 4] = [".COM", ".EXE", ".BAT", ".CMD"];

/// Env override for the TokenZero CLI / MCP entry binary.
pub const TOKENZERO_BIN_ENV: &str = "TOKENZERO_BIN";
/// Env override for ripgrep (`rg`).
pub const TOKENZERO_RG_PATH_ENV: &str = "TOKENZERO_RG_PATH";
/// Env override for curl (fetch).
pub const TOKENZERO_CURL_PATH_ENV: &str = "TOKENZERO_CURL_PATH";

/// Successful binary discovery: path plus how it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBinary {
    pub name: String,
    pub path: PathBuf,
    pub source: &'static str,
}

/// Failed binary discovery with a clear operator-facing message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveError {
    pub name: String,
    pub message: String,
}

/// Typed resolution outcome (replaces the old path+error Option pair).
pub type BinaryResolution = Result<ResolvedBinary, ResolveError>;

impl ResolvedBinary {
    pub fn new(name: impl Into<String>, path: PathBuf, source: &'static str) -> Self {
        Self {
            name: name.into(),
            path,
            source,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "path": self.path.display().to_string(),
            "source": self.source,
            "error": null,
            "ok": true,
        })
    }
}

impl ResolveError {
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: message.into(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "path": null,
            "source": "unresolved",
            "error": self.message,
            "ok": false,
        })
    }
}

fn resolution_to_json(res: &BinaryResolution) -> serde_json::Value {
    match res {
        Ok(ok) => ok.to_json(),
        Err(err) => err.to_json(),
    }
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn binary_candidate_names(name: &str, windows: bool, pathext: Option<&OsStr>) -> Vec<String> {
    let mut names = vec![name.to_string()];
    if !windows || Path::new(name).extension().is_some() {
        return names;
    }

    let mut extensions = Vec::new();
    if let Some(pathext) = pathext {
        for extension in pathext.to_string_lossy().split(';') {
            let extension = extension.trim();
            if extension.is_empty() {
                continue;
            }
            if extension.starts_with('.') {
                extensions.push(extension.to_string());
            } else {
                extensions.push(format!(".{extension}"));
            }
        }
    }
    extensions.extend(WINDOWS_EXECUTABLE_EXTENSIONS.map(str::to_string));

    for extension in extensions {
        let candidate = format!("{name}{extension}");
        if !names
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&candidate))
        {
            names.push(candidate);
        }
    }
    names
}

fn find_on_paths(
    name: &str,
    dirs: &[PathBuf],
    windows: bool,
    pathext: Option<&OsStr>,
) -> Option<PathBuf> {
    let names = binary_candidate_names(name, windows, pathext);
    for dir in dirs {
        for n in &names {
            let candidate = dir.join(n);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// PATH lookup for `name` (honors `PATHEXT` and standard script extensions on Windows).
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    let dirs: Vec<PathBuf> = env::split_paths(&path_var)
        .filter(|dir| !dir.as_os_str().is_empty())
        .collect();
    let pathext = env::var_os("PATHEXT");
    find_on_paths(name, &dirs, cfg!(windows), pathext.as_deref())
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn well_known_candidates_for(
    name: &str,
    home: Option<&Path>,
    windows: bool,
    pathext: Option<&OsStr>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join(".tokenzero").join("bin"));
        dirs.push(home.join(".cargo").join("bin"));
        dirs.push(home.join(".local").join("bin"));
    }
    // Homebrew / system layouts (never personal AI checkouts).
    for prefix in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        dirs.push(PathBuf::from(prefix));
    }
    let names = binary_candidate_names(name, windows, pathext);
    dirs.into_iter()
        .flat_map(|dir| names.iter().map(move |candidate| dir.join(candidate)))
        .collect()
}

/// Well-known layout candidates for a binary base name (no dir).
fn well_known_candidates(name: &str) -> Vec<PathBuf> {
    let home = home_dir();
    let pathext = env::var_os("PATHEXT");
    well_known_candidates_for(name, home.as_deref(), cfg!(windows), pathext.as_deref())
}

fn first_existing(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|p| is_executable_file(p))
}

/// Pure resolution: env path → PATH → well-known → None.
pub fn resolve_binary_with_env(name: &str, env_override: Option<&Path>) -> BinaryResolution {
    if let Some(path) = env_override {
        if path.as_os_str().is_empty() {
            // fall through
        } else if is_executable_file(path) {
            return Ok(ResolvedBinary::new(name, path.to_path_buf(), "env"));
        } else if path.is_file() {
            return Err(ResolveError::new(
                name,
                format!(
                    "env override is not executable: {} (check TOKENZERO_*_PATH / TOKENZERO_BIN)",
                    path.display()
                ),
            ));
        } else {
            return Err(ResolveError::new(
                name,
                format!(
                    "env override points to missing file: {} (check TOKENZERO_*_PATH / TOKENZERO_BIN)",
                    path.display()
                ),
            ));
        }
    }
    if let Some(path) = find_on_path(name) {
        return Ok(ResolvedBinary::new(name, path, "path"));
    }
    if let Some(path) = first_existing(well_known_candidates(name)) {
        return Ok(ResolvedBinary::new(name, path, "well_known"));
    }
    Err(ResolveError::new(
        name,
        format!(
            "{name} not found: set env override, put `{name}` on PATH, or install under \
             ~/.tokenzero/bin, ~/.cargo/bin, /opt/homebrew/bin, or /usr/local/bin"
        ),
    ))
}

/// Resolve the TokenZero entry binary (CLI / MCP host).
///
/// Order: `TOKENZERO_BIN` → PATH `tokenzero` → well-known → `current_exe` (if set).
pub fn resolve_tokenzero_binary() -> BinaryResolution {
    let env_path = env::var_os(TOKENZERO_BIN_ENV).map(PathBuf::from);
    if let Ok(ok) = resolve_binary_with_env("tokenzero", env_path.as_deref()) {
        return Ok(ok);
    }
    // Fallback: current process executable (running binary is a valid discovery).
    if let Ok(exe) = env::current_exe() {
        if exe.is_file() {
            return Ok(ResolvedBinary::new("tokenzero", exe, "current_exe"));
        }
    }
    Err(ResolveError::new(
        "tokenzero",
        "tokenzero binary not found: set TOKENZERO_BIN, put `tokenzero` on PATH, \
         or run `tokenzero install --global` (installs under ~/.tokenzero/bin)",
    ))
}

/// Resolve ripgrep. Order: `TOKENZERO_RG_PATH` → PATH → well-known.
pub fn resolve_rg_binary() -> BinaryResolution {
    let env_path = env::var_os(TOKENZERO_RG_PATH_ENV).map(PathBuf::from);
    resolve_binary_with_env("rg", env_path.as_deref())
}

/// Resolve curl. Order: `TOKENZERO_CURL_PATH` → PATH → well-known.
pub fn resolve_curl_binary() -> BinaryResolution {
    let env_path = env::var_os(TOKENZERO_CURL_PATH_ENV).map(PathBuf::from);
    resolve_binary_with_env("curl", env_path.as_deref())
}

/// Snapshot of all TokenZero-owned helper binaries for doctor / status.
pub fn resolve_all_engine_binaries() -> Vec<BinaryResolution> {
    vec![
        resolve_tokenzero_binary(),
        resolve_rg_binary(),
        resolve_curl_binary(),
    ]
}

pub fn engine_binaries_json() -> serde_json::Value {
    let bins = resolve_all_engine_binaries();
    serde_json::json!({
        "schema_version": "tokenzero.engine_binaries.v1",
        "resolution_order": ["env", "path", "well_known", "current_exe(tokenzero only)", "error"],
        "env_overrides": {
            "tokenzero": TOKENZERO_BIN_ENV,
            "rg": TOKENZERO_RG_PATH_ENV,
            "curl": TOKENZERO_CURL_PATH_ENV,
        },
        "binaries": bins.iter().map(resolution_to_json).collect::<Vec<_>>(),
        "note": "No host-absolute personal AI checkout paths are used. Multi-machine: install tokenzero on PATH or under $HOME/.tokenzero/bin; use TOKENZERO_BIN / TOKENZERO_RG_PATH when needed.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn env_override_wins_when_file_exists() {
        let dir = tempdir().unwrap();
        let fake = dir.path().join("rg");
        fs::write(&fake, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&fake, perms).unwrap();
        }
        let res = resolve_binary_with_env("rg", Some(&fake));
        let ok = res.expect("env override should resolve");
        assert_eq!(ok.source, "env");
        assert_eq!(ok.path, fake);
    }

    #[test]
    fn missing_env_override_is_clear_error() {
        let missing = PathBuf::from("/no/such/tokenzero-wqw3-rg-binary-xyz");
        let res = resolve_binary_with_env("rg", Some(&missing));
        let err = res.expect_err("missing env override");
        assert!(err.message.contains("missing file"), "{}", err.message);
        assert!(err.message.contains("TOKENZERO"), "{}", err.message);
    }

    #[cfg(unix)]
    #[test]
    fn path_lookup_skips_non_executable_files() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let fake = dir.path().join("rg");
        fs::write(&fake, b"not executable").unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o644)).unwrap();

        assert!(
            find_on_paths("rg", &[dir.path().to_path_buf()], false, None).is_none(),
            "PATH lookup must not select a non-executable file"
        );
    }

    #[test]
    fn windows_candidates_honor_pathext_and_include_script_launchers() {
        let names = binary_candidate_names("tokenzero", true, Some(OsStr::new(".PY;.EXE")));
        for expected in [
            "tokenzero.PY",
            "tokenzero.EXE",
            "tokenzero.COM",
            "tokenzero.BAT",
            "tokenzero.CMD",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "missing {expected}"
            );
        }
        assert_eq!(
            names
                .iter()
                .filter(|name| name.eq_ignore_ascii_case("tokenzero.exe"))
                .count(),
            1,
            "PATHEXT and standard fallbacks must be deduplicated"
        );
    }

    #[test]
    fn windows_well_known_candidates_include_installer_cmd() {
        let home = Path::new("test-home");
        let candidates = well_known_candidates_for("tokenzero", Some(home), true, None);
        assert!(candidates.contains(&home.join(".tokenzero").join("bin").join("tokenzero.CMD")));
    }

    #[test]
    fn unix_candidates_do_not_add_windows_extensions() {
        assert_eq!(
            binary_candidate_names("tokenzero", false, Some(OsStr::new(".CMD"))),
            vec!["tokenzero"]
        );
    }

    #[test]
    fn non_executable_env_override_is_clear_error() {
        #[cfg(unix)]
        {
            let dir = tempdir().unwrap();
            let fake = dir.path().join("rg");
            fs::write(&fake, b"x").unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&fake, perms).unwrap();
            let res = resolve_binary_with_env("rg", Some(&fake));
            let err = res.expect_err("non-executable env override");
            assert!(err.message.contains("not executable"), "{}", err.message);
        }
    }

    #[test]
    fn well_known_finds_file_outside_path() {
        let dir = tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let fake = bin_dir.join("rg");
        fs::write(&fake, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&fake, perms).unwrap();
        }
        // Simulate well-known by calling first_existing directly.
        let found = first_existing([fake.clone()]);
        assert_eq!(found, Some(fake));
    }

    #[test]
    fn well_known_skips_non_executable_on_unix() {
        #[cfg(unix)]
        {
            let dir = tempdir().unwrap();
            let fake = dir.path().join("rg");
            fs::write(&fake, b"x").unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&fake, perms).unwrap();
            assert!(first_existing([fake]).is_none());
        }
    }

    #[test]
    fn resolution_order_is_documented_in_json() {
        let json = engine_binaries_json();
        let order = json["resolution_order"].as_array().unwrap();
        assert_eq!(order[0], "env");
        assert_eq!(order[1], "path");
        assert_eq!(order[2], "well_known");
        assert!(
            json["note"]
                .as_str()
                .unwrap()
                .contains("No host-absolute personal")
        );
    }

    #[test]
    fn well_known_candidates_never_include_personal_ai_checkouts() {
        let cands = well_known_candidates("tokenzero");
        for c in &cands {
            let s = c.to_string_lossy();
            assert!(
                !s.contains("/AI/tokenzero/target")
                    && !s.contains("/AI/FSZero/target")
                    && !s.contains("/AI/graphzero/target"),
                "must not hardcode personal AI target/release paths: {s}"
            );
        }
    }

    #[test]
    fn tokenzero_resolution_has_fallback_or_path() {
        // Running under cargo test: current_exe is available as last resort.
        let res = resolve_tokenzero_binary();
        let ok = res.expect("expected tokenzero resolve via path/well_known/current_exe");
        assert!(
            matches!(ok.source, "env" | "path" | "well_known" | "current_exe"),
            "{ok:?}"
        );
    }
}
