//! ZeroStack store schema stamps for TokenZero segments.
//!
//! Shared `.zerostack` layout: stamp major.minor on ActionCache segments,
//! `shadow.jsonl`, and the recovery blobs manifest. Newer major is refused;
//! older minor degrades. `shadow.jsonl` is a fixed ring. ActionCache writes
//! use a commit marker so a torn temp is never promoted.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Current TokenZero store schema (ZeroRef sibling contract).
pub const STORE_SCHEMA_MAJOR: u16 = 1;
pub const STORE_SCHEMA_MINOR: u16 = 0;
pub const STORE_SCHEMA_NAME: &str = "tokenzero.store";
/// Fixed ring size for `shadow.jsonl`. Never unbounded.
pub const SHADOW_JSONL_RING_CAP: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreSchemaVersion {
    pub major: u16,
    pub minor: u16,
}

impl StoreSchemaVersion {
    pub const CURRENT: Self = Self {
        major: STORE_SCHEMA_MAJOR,
        minor: STORE_SCHEMA_MINOR,
    };

    pub fn stamp(self) -> StoreSchemaStamp {
        StoreSchemaStamp {
            schema: STORE_SCHEMA_NAME,
            major: self.major,
            minor: self.minor,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreSchemaStamp {
    pub schema: &'static str,
    pub major: u16,
    pub minor: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaAdmit {
    Accept,
    /// Same major, older or newer compatible minor: read with defaults.
    DegradeMinor,
    /// Older major we can still parse with a degraded reader.
    DegradeOlderMajor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaSkewError {
    NewerMajor { found: StoreSchemaVersion },
    MissingStamp,
    WrongSchema { found: String },
}

impl std::fmt::Display for SchemaSkewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NewerMajor { found } => write!(
                f,
                "tokenzero store schema {}.{} is newer than supported {}.{} (refuse newer major)",
                found.major, found.minor, STORE_SCHEMA_MAJOR, STORE_SCHEMA_MINOR
            ),
            Self::MissingStamp => write!(f, "tokenzero store segment is missing a schema stamp"),
            Self::WrongSchema { found } => {
                write!(
                    f,
                    "tokenzero store schema name {found:?} is not {STORE_SCHEMA_NAME}"
                )
            }
        }
    }
}

impl std::error::Error for SchemaSkewError {}

/// Admit a found stamp against the current TokenZero store schema.
pub fn admit_store_schema(stamp: &StoreSchemaStamp) -> Result<SchemaAdmit, SchemaSkewError> {
    admit_store_schema_against(stamp, StoreSchemaVersion::CURRENT)
}

/// Admit a found stamp against an explicit current version (tests + callers).
pub fn admit_store_schema_against(
    stamp: &StoreSchemaStamp,
    current: StoreSchemaVersion,
) -> Result<SchemaAdmit, SchemaSkewError> {
    if stamp.schema != STORE_SCHEMA_NAME {
        return Err(SchemaSkewError::WrongSchema {
            found: stamp.schema.to_string(),
        });
    }
    let found = StoreSchemaVersion {
        major: stamp.major,
        minor: stamp.minor,
    };
    if found.major > current.major {
        return Err(SchemaSkewError::NewerMajor { found });
    }
    if found.major < current.major {
        return Ok(SchemaAdmit::DegradeOlderMajor);
    }
    if found.minor == current.minor {
        Ok(SchemaAdmit::Accept)
    } else {
        Ok(SchemaAdmit::DegradeMinor)
    }
}

/// Append one shadow line and trim to the fixed ring cap.
pub fn append_shadow_jsonl(path: &Path, line: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")?;
    drop(file);
    trim_shadow_ring(path)
}

fn trim_shadow_ring(path: &Path) -> io::Result<()> {
    let text = fs::read_to_string(path)?;
    let mut lines: Vec<&str> = text.lines().collect();
    if lines.len() <= SHADOW_JSONL_RING_CAP {
        return Ok(());
    }
    lines = lines.split_off(lines.len() - SHADOW_JSONL_RING_CAP);
    let mut out = String::new();
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    fs::write(path, out)
}

/// Crash-safe ActionCache segment write: temp + commit marker + rename.
pub fn write_actioncache_segment(dest: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = tmp_path(dest);
    let commit = commit_path(dest);
    fs::write(&tmp, bytes)?;
    let digest = hex_sha256(bytes);
    fs::write(&commit, digest.as_bytes())?;
    fs::rename(&tmp, dest)?;
    let _ = fs::remove_file(commit);
    Ok(())
}

/// Recover a segment after crash: promote a committed temp, discard an uncommitted one.
pub fn recover_actioncache_segment(dest: &Path) -> io::Result<Option<PathBuf>> {
    let tmp = tmp_path(dest);
    let commit = commit_path(dest);
    if dest.exists() {
        let _ = fs::remove_file(&tmp);
        let _ = fs::remove_file(&commit);
        return Ok(Some(dest.to_path_buf()));
    }
    if tmp.exists() && commit.exists() {
        let expected = fs::read_to_string(&commit)?;
        let bytes = fs::read(&tmp)?;
        if hex_sha256(&bytes) == expected.trim() {
            fs::rename(&tmp, dest)?;
            let _ = fs::remove_file(commit);
            return Ok(Some(dest.to_path_buf()));
        }
    }
    let _ = fs::remove_file(&tmp);
    let _ = fs::remove_file(&commit);
    Ok(None)
}

fn tmp_path(dest: &Path) -> PathBuf {
    dest.with_extension("tmp")
}

fn commit_path(dest: &Path) -> PathBuf {
    dest.with_extension("commit")
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod store_schema_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn tzgd0b_admit_refuses_newer_major_and_degrades_older_minor() {
        let current = StoreSchemaVersion::CURRENT.stamp();
        assert_eq!(admit_store_schema(&current).unwrap(), SchemaAdmit::Accept);

        let reader = StoreSchemaVersion { major: 1, minor: 1 };
        let older_minor = StoreSchemaStamp {
            schema: STORE_SCHEMA_NAME,
            major: 1,
            minor: 0,
        };
        assert_eq!(
            admit_store_schema_against(&older_minor, reader).unwrap(),
            SchemaAdmit::DegradeMinor
        );

        let newer_minor = StoreSchemaStamp {
            schema: STORE_SCHEMA_NAME,
            major: STORE_SCHEMA_MAJOR,
            minor: STORE_SCHEMA_MINOR.saturating_add(1),
        };
        assert_eq!(
            admit_store_schema(&newer_minor).unwrap(),
            SchemaAdmit::DegradeMinor
        );

        let older_major = StoreSchemaStamp {
            schema: STORE_SCHEMA_NAME,
            major: 0,
            minor: 9,
        };
        assert_eq!(
            admit_store_schema(&older_major).unwrap(),
            SchemaAdmit::DegradeOlderMajor
        );

        let newer_major = StoreSchemaStamp {
            schema: STORE_SCHEMA_NAME,
            major: STORE_SCHEMA_MAJOR + 1,
            minor: 0,
        };
        let err = admit_store_schema(&newer_major).unwrap_err();
        assert!(
            matches!(err, SchemaSkewError::NewerMajor { found } if found.major == STORE_SCHEMA_MAJOR + 1)
        );
        assert!(err.to_string().contains("refuse newer major"));
    }

    #[test]
    fn tzgd0b_shadow_jsonl_is_a_fixed_ring() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("shadow.jsonl");
        for idx in 0..(SHADOW_JSONL_RING_CAP + 40) {
            append_shadow_jsonl(&path, &format!("{{\"n\":{idx}}}")).unwrap();
        }
        let lines: Vec<String> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(lines.len(), SHADOW_JSONL_RING_CAP);
        assert_eq!(lines[0], format!("{{\"n\":{}}}", 40));
        assert_eq!(
            lines.last().unwrap(),
            &format!("{{\"n\":{}}}", SHADOW_JSONL_RING_CAP + 39)
        );
    }

    #[test]
    fn tzgd0b_torn_actioncache_write_is_not_promoted() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("actions").join("seg-1.json");
        let payload = br#"{"schema":"tokenzero.store","major":1,"minor":0,"k":1}"#;
        write_actioncache_segment(&dest, payload).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), payload);

        // Crash before commit: leftover tmp without marker is discarded.
        let tmp = dest.with_extension("tmp");
        fs::remove_file(&dest).unwrap();
        fs::write(&tmp, b"torn-partial").unwrap();
        assert!(recover_actioncache_segment(&dest).unwrap().is_none());
        assert!(!tmp.exists());
        assert!(!dest.exists());

        // Commit marker present: temp is promoted.
        fs::write(&tmp, payload).unwrap();
        fs::write(dest.with_extension("commit"), hex_sha256(payload)).unwrap();
        let recovered = recover_actioncache_segment(&dest).unwrap().unwrap();
        assert_eq!(recovered, dest);
        assert_eq!(fs::read(&dest).unwrap(), payload);
    }
}
