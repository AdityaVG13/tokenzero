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
use std::path::{Path, PathBuf};

/// Env override for the TokenZero CLI / MCP entry binary.
pub const TOKENZERO_BIN_ENV: &str = "TOKENZERO_BIN";
/// Env override for ripgrep (`rg`).
pub const TOKENZERO_RG_PATH_ENV: &str = "TOKENZERO_RG_PATH";
/// Env override for curl (fetch).
pub const TOKENZERO_CURL_PATH_ENV: &str = "TOKENZERO_CURL_PATH";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryResolution {
    pub name: String,
    pub path: Option<PathBuf>,
    pub source: &'static str,
    pub error: Option<String>,
}

impl BinaryResolution {
    pub fn ok(name: impl Into<String>, path: PathBuf, source: &'static str) -> Self {
        Self {
            name: name.into(),
            path: Some(path),
            source,
            error: None,
        }
    }

    pub fn missing(name: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: None,
            source: "unresolved",
            error: Some(error.into()),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "path": self.path.as_ref().map(|p| p.display().to_string()),
            "source": self.source,
            "error": self.error,
            "ok": self.path.is_some(),
        })
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

/// PATH lookup for `name` (also tries `.exe` on Windows).
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    let mut names = vec![name.to_string()];
    if cfg!(windows) && !name.ends_with(".exe") {
        names.push(format!("{name}.exe"));
    }
    for dir in env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for n in &names {
            let candidate = dir.join(n);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Well-known layout candidates for a binary base name (no dir).
fn well_known_candidates(name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = home_dir() {
        out.push(home.join(".tokenzero").join("bin").join(name));
        out.push(home.join(".cargo").join("bin").join(name));
        out.push(home.join(".local").join("bin").join(name));
    }
    // Homebrew / system layouts (never personal AI checkouts).
    for prefix in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        out.push(PathBuf::from(prefix).join(name));
    }
    if cfg!(windows) {
        if let Some(home) = home_dir() {
            out.push(
                home.join(".tokenzero")
                    .join("bin")
                    .join(format!("{name}.exe")),
            );
            out.push(home.join(".cargo").join("bin").join(format!("{name}.exe")));
        }
    }
    out
}

fn first_existing(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|p| is_executable_file(p))
}

/// Pure resolution: env path → PATH → well-known → None.
pub fn resolve_binary_with_env(name: &str, env_override: Option<&Path>) -> BinaryResolution {
    if let Some(path) = env_override {
        if path.as_os_str().is_empty() {
            // fall through
        } else if path.is_file() {
            return BinaryResolution::ok(name, path.to_path_buf(), "env");
        } else {
            return BinaryResolution::missing(
                name,
                format!(
                    "env override points to missing file: {} (check TOKENZERO_*_PATH / TOKENZERO_BIN)",
                    path.display()
                ),
            );
        }
    }
    if let Some(path) = find_on_path(name) {
        return BinaryResolution::ok(name, path, "path");
    }
    if let Some(path) = first_existing(well_known_candidates(name)) {
        return BinaryResolution::ok(name, path, "well_known");
    }
    BinaryResolution::missing(
        name,
        format!(
            "{name} not found: set env override, put `{name}` on PATH, or install under \
             ~/.tokenzero/bin, ~/.cargo/bin, /opt/homebrew/bin, or /usr/local/bin"
        ),
    )
}

/// Resolve the TokenZero entry binary (CLI / MCP host).
///
/// Order: `TOKENZERO_BIN` → PATH `tokenzero` → well-known → `current_exe` (if set).
pub fn resolve_tokenzero_binary() -> BinaryResolution {
    let env_path = env::var_os(TOKENZERO_BIN_ENV).map(PathBuf::from);
    let mut res = resolve_binary_with_env("tokenzero", env_path.as_deref());
    if res.path.is_some() {
        return res;
    }
    // Fallback: current process executable (running binary is a valid discovery).
    if let Ok(exe) = env::current_exe() {
        if exe.is_file() {
            return BinaryResolution::ok("tokenzero", exe, "current_exe");
        }
    }
    res.error = Some(
        "tokenzero binary not found: set TOKENZERO_BIN, put `tokenzero` on PATH, \
         or run `tokenzero install --global` (installs under ~/.tokenzero/bin)"
            .into(),
    );
    res
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
        "binaries": bins.iter().map(BinaryResolution::to_json).collect::<Vec<_>>(),
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
        assert_eq!(res.source, "env");
        assert_eq!(res.path.as_deref(), Some(fake.as_path()));
    }

    #[test]
    fn missing_env_override_is_clear_error() {
        let missing = PathBuf::from("/no/such/tokenzero-wqw3-rg-binary-xyz");
        let res = resolve_binary_with_env("rg", Some(&missing));
        assert!(res.path.is_none());
        assert_eq!(res.source, "unresolved");
        let err = res.error.unwrap();
        assert!(err.contains("missing file"), "{err}");
        assert!(err.contains("TOKENZERO"), "{err}");
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
        assert!(
            res.path.is_some(),
            "expected tokenzero resolve via path/well_known/current_exe: {res:?}"
        );
        assert!(
            matches!(res.source, "env" | "path" | "well_known" | "current_exe"),
            "{res:?}"
        );
    }
}
