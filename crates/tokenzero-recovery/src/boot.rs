//! O(1) session boot manifest and delta persistence.
//! Boot metadata stays separate from the recovery snapshot so session open never
//! deserializes the store or enumerates the repository.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokenzero_core::count_tokens;

const MANIFEST_VERSION: u32 = 1;
const DELTA_VERSION: u32 = 1;
const STORE_VERSION: u32 = 1;
const ID_HEX_LEN: usize = 12;
const EMPTY_ID: &str = "000000000000";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootManifest {
    version: u32,
    root_digest: String,
    manifest_id: String,
    store_version: u32,
    toc_ref: String,
    working_set_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionDelta {
    version: u32,
    manifest_id: String,
    session_hwm: u64,
    #[serde(default)]
    added_refs: Vec<String>,
    #[serde(default)]
    changed_refs: Vec<String>,
    #[serde(default)]
    deleted_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BootTokenComponents {
    pub manifest: usize,
    pub delta: usize,
    pub toc_working_set: usize,
    pub other: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionBoot {
    pub schema: &'static str,
    pub mode: &'static str,
    pub wire: String,
    pub manifest_id: String,
    pub delta_ref: String,
    pub manifest_path: PathBuf,
    pub delta_path: PathBuf,
    pub telemetry: BootTokenComponents,
}

/// Open a session without loading the recovery snapshot or walking the repo.
/// Missing metadata is initialized atomically. Unknown, older, newer, corrupt,
/// or unreadable metadata is left untouched and gets a bounded legacy fallback.
pub fn open_session_boot(
    cache_path: &Path,
    root: &Path,
    allowed_roots: &[PathBuf],
) -> std::io::Result<SessionBoot> {
    let manifest_path = cache_path.with_file_name("boot-manifest.json");
    let delta_path = cache_path.with_file_name("boot-session-delta.json");
    let root_digest = root_digest(root, allowed_roots);

    let (manifest, delta, mode) = match load_manifest(&manifest_path) {
        MetadataLoad::Compatible(manifest) if manifest.root_digest == root_digest => {
            match load_delta(&delta_path, &manifest.manifest_id) {
                MetadataLoad::Compatible(delta) => (manifest, delta, "manifest_delta"),
                MetadataLoad::Missing => {
                    let delta = empty_delta(&manifest.manifest_id);
                    atomic_write_json(&delta_path, &delta)?;
                    (manifest, delta, "manifest_delta")
                }
                MetadataLoad::Incompatible => {
                    let delta = empty_delta(&manifest.manifest_id);
                    (manifest, delta, "legacy_fallback")
                }
            }
        }
        MetadataLoad::Missing => {
            let manifest = new_manifest(root_digest);
            match load_delta(&delta_path, &manifest.manifest_id) {
                MetadataLoad::Compatible(delta) => {
                    atomic_write_json(&manifest_path, &manifest)?;
                    (manifest, delta, "manifest_delta")
                }
                MetadataLoad::Missing => {
                    let delta = empty_delta(&manifest.manifest_id);
                    atomic_write_json(&manifest_path, &manifest)?;
                    atomic_write_json(&delta_path, &delta)?;
                    (manifest, delta, "manifest_delta")
                }
                MetadataLoad::Incompatible => {
                    let delta = empty_delta(&manifest.manifest_id);
                    (manifest, delta, "legacy_fallback")
                }
            }
        }
        MetadataLoad::Compatible(_) | MetadataLoad::Incompatible => {
            let manifest = new_manifest(root_digest);
            let delta = empty_delta(&manifest.manifest_id);
            (manifest, delta, "legacy_fallback")
        }
    };

    Ok(build_boot(mode, manifest, delta, manifest_path, delta_path))
}

fn build_boot(
    mode: &'static str,
    manifest: BootManifest,
    delta: SessionDelta,
    manifest_path: PathBuf,
    delta_path: PathBuf,
) -> SessionBoot {
    let delta_ref = delta_id(&delta);
    let manifest_part = format!(
        "TZ/1 root={} m={} v={}",
        manifest.root_digest, manifest.manifest_id, manifest.store_version
    );
    let delta_part = format!(" d={delta_ref}");
    let toc_part = format!(" toc={} ws={}", manifest.toc_ref, manifest.working_set_ref);
    let other_part = if mode == "manifest_delta" {
        String::new()
    } else {
        " fallback=legacy".to_string()
    };
    let wire = format!("{manifest_part}{delta_part}{toc_part}{other_part}");
    let manifest_tokens = count_tokens(&manifest_part);
    let through_delta = count_tokens(&(manifest_part.clone() + &delta_part));
    let through_toc = count_tokens(&(manifest_part.clone() + &delta_part + &toc_part));
    let total = count_tokens(&wire);
    let telemetry = BootTokenComponents {
        manifest: manifest_tokens,
        delta: through_delta.saturating_sub(manifest_tokens),
        toc_working_set: through_toc.saturating_sub(through_delta),
        other: total.saturating_sub(through_toc),
        total,
    };
    SessionBoot {
        schema: "tokenzero.session-boot.v1",
        mode,
        wire,
        manifest_id: manifest.manifest_id,
        delta_ref,
        manifest_path,
        delta_path,
        telemetry,
    }
}

fn new_manifest(root_digest: String) -> BootManifest {
    let seed = format!(
        "v={MANIFEST_VERSION}|root={root_digest}|store={STORE_VERSION}|toc={EMPTY_ID}|ws={EMPTY_ID}"
    );
    BootManifest {
        version: MANIFEST_VERSION,
        root_digest,
        manifest_id: short_digest(seed.as_bytes()),
        store_version: STORE_VERSION,
        toc_ref: EMPTY_ID.to_string(),
        working_set_ref: EMPTY_ID.to_string(),
    }
}

fn empty_delta(manifest_id: &str) -> SessionDelta {
    SessionDelta {
        version: DELTA_VERSION,
        manifest_id: manifest_id.to_string(),
        session_hwm: 0,
        added_refs: Vec::new(),
        changed_refs: Vec::new(),
        deleted_refs: Vec::new(),
    }
}

enum MetadataLoad<T> {
    Missing,
    Compatible(T),
    Incompatible,
}

fn read_metadata(path: &Path) -> MetadataLoad<Vec<u8>> {
    match fs::read(path) {
        Ok(bytes) => MetadataLoad::Compatible(bytes),
        Err(error) if error.kind() == ErrorKind::NotFound => MetadataLoad::Missing,
        Err(_) => MetadataLoad::Incompatible,
    }
}

fn load_manifest(path: &Path) -> MetadataLoad<BootManifest> {
    let bytes = match read_metadata(path) {
        MetadataLoad::Missing => return MetadataLoad::Missing,
        MetadataLoad::Compatible(bytes) => bytes,
        MetadataLoad::Incompatible => return MetadataLoad::Incompatible,
    };
    let Ok(manifest) = serde_json::from_slice::<BootManifest>(&bytes) else {
        return MetadataLoad::Incompatible;
    };
    if manifest.version != MANIFEST_VERSION
        || manifest.store_version != STORE_VERSION
        || !is_fixed_id(&manifest.root_digest)
        || !is_fixed_id(&manifest.manifest_id)
        || !is_fixed_id(&manifest.toc_ref)
        || !is_fixed_id(&manifest.working_set_ref)
    {
        return MetadataLoad::Incompatible;
    }
    MetadataLoad::Compatible(manifest)
}

fn load_delta(path: &Path, manifest_id: &str) -> MetadataLoad<SessionDelta> {
    let bytes = match read_metadata(path) {
        MetadataLoad::Missing => return MetadataLoad::Missing,
        MetadataLoad::Compatible(bytes) => bytes,
        MetadataLoad::Incompatible => return MetadataLoad::Incompatible,
    };
    let Ok(delta) = serde_json::from_slice::<SessionDelta>(&bytes) else {
        return MetadataLoad::Incompatible;
    };
    let refs_are_valid = delta
        .added_refs
        .iter()
        .chain(&delta.changed_refs)
        .chain(&delta.deleted_refs)
        .all(|reference| is_fixed_id(reference));
    if delta.version != DELTA_VERSION || delta.manifest_id != manifest_id || !refs_are_valid {
        return MetadataLoad::Incompatible;
    }
    MetadataLoad::Compatible(delta)
}

fn delta_id(delta: &SessionDelta) -> String {
    if delta.session_hwm == 0
        && delta.added_refs.is_empty()
        && delta.changed_refs.is_empty()
        && delta.deleted_refs.is_empty()
    {
        return EMPTY_ID.to_string();
    }
    serde_json::to_vec(delta)
        .map(|bytes| short_digest(&bytes))
        .unwrap_or_else(|_| EMPTY_ID.to_string())
}

fn root_digest(root: &Path, allowed_roots: &[PathBuf]) -> String {
    let mut roots = allowed_roots
        .iter()
        .map(|path| normalize(path))
        .collect::<Vec<_>>();
    roots.push(normalize(root));
    roots.sort();
    roots.dedup();
    short_digest(roots.join("\n").as_bytes())
}

fn normalize(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_fixed_id(value: &str) -> bool {
    value.len() == ID_HEX_LEN && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn short_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest[..ID_HEX_LEN / 2]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
    let body = serde_json::to_vec_pretty(value)
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error))?;
    fs::write(&tmp, body)?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn boot_is_bounded_and_independent_of_repo_size() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let small = open_session_boot(&cache, dir.path(), &[dir.path().to_path_buf()]).unwrap();
        for idx in 0..10_000 {
            fs::write(dir.path().join(format!("f{idx}")), b"x").unwrap();
        }
        let large = open_session_boot(&cache, dir.path(), &[dir.path().to_path_buf()]).unwrap();
        assert_eq!(small.manifest_id, large.manifest_id);
        assert_eq!(small.telemetry.total, large.telemetry.total);
        assert!(large.telemetry.total < 100, "{}", large.wire);
        assert_eq!(
            large.telemetry.total,
            large.telemetry.manifest
                + large.telemetry.delta
                + large.telemetry.toc_working_set
                + large.telemetry.other
        );
    }

    #[test]
    fn incompatible_manifest_falls_back_without_overwrite() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let manifest = cache.with_file_name("boot-manifest.json");
        fs::write(&manifest, br#"{"version":99}"#).unwrap();
        let before = fs::read(&manifest).unwrap();
        let boot = open_session_boot(&cache, dir.path(), &[]).unwrap();
        assert_eq!(boot.mode, "legacy_fallback");
        assert_eq!(fs::read(&manifest).unwrap(), before);
        assert!(boot.telemetry.total < 100);
    }

    #[test]
    fn corrupt_delta_with_payload_falls_back_without_overwrite() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let first = open_session_boot(&cache, dir.path(), &[]).unwrap();
        let delta = cache.with_file_name("boot-session-delta.json");
        let body = format!(
            r#"{{"version":1,"manifest_id":"{}","session_hwm":1,"payload":"forbidden"}}"#,
            first.manifest_id
        );
        fs::write(&delta, body).unwrap();
        let before = fs::read(&delta).unwrap();
        let boot = open_session_boot(&cache, dir.path(), &[]).unwrap();
        assert_eq!(boot.mode, "legacy_fallback");
        assert_eq!(fs::read(&delta).unwrap(), before);
    }

    #[test]
    fn missing_manifest_and_delta_are_written_and_reused() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let first = open_session_boot(&cache, dir.path(), &[]).unwrap();
        let second = open_session_boot(&cache, dir.path(), &[]).unwrap();
        assert_eq!(first.mode, "manifest_delta");
        assert_eq!(first.manifest_id, second.manifest_id);
        assert_eq!(second.delta_ref, EMPTY_ID);
        assert!(first.manifest_path.is_file());
        assert!(first.delta_path.is_file());
    }
}
