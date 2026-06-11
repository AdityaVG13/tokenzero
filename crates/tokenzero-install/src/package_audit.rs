use flate2::read::{DeflateDecoder, MultiGzDecoder};
use std::borrow::Cow;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_ARCHIVE_MEMBERS: usize = 4096;
const MAX_NESTED_ARCHIVE_DEPTH: usize = 3;
const MAX_TOP_LEVEL_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_NESTED_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
const MAX_GZIP_DECOMPRESSED_BYTES: usize = 256 * 1024 * 1024;
const MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES: usize = 256 * 1024 * 1024;
const ZIP_FLAG_ENCRYPTED: u16 = 0x0001;
const ZIP_FLAG_DATA_DESCRIPTOR: u16 = 0x0008;
const ZIP_FLAG_STRONG_ENCRYPTION: u16 = 0x0040;
const ZIP_FLAG_MASKED_LOCAL_HEADER_VALUES: u16 = 0x2000;
const ZIP_DATA_DESCRIPTOR_SIGNATURE: u32 = 0x0807_4b50;
const ZIP64_EOCD_RECORD_SIGNATURE: u32 = 0x0606_4b50;
const ZIP64_EOCD_LOCATOR_SIGNATURE: u32 = 0x0706_4b50;
const ZIP64_EXTENDED_INFORMATION_EXTRA: u16 = 0x0001;

pub fn package_audit(root: &Path, artifacts: &[PathBuf]) -> serde_json::Value {
    let mut issues = Vec::new();
    let mut checked = 0usize;
    let candidates = if artifacts.is_empty() {
        vec![
            root.join("Cargo.toml"),
            root.join("package/npm/package.json"),
            root.join("packaging/homebrew/tokenzero.rb"),
        ]
    } else {
        artifacts.to_vec()
    };
    for path in candidates {
        if !path.exists() {
            continue;
        }
        checked += 1;
        audit_artifact_member_names(&path, &mut issues);
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if is_supported_archive_name(file_name) {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&path) {
            let lower = text.to_ascii_lowercase();
            let script_runtime = ["py", "thon"].concat();
            let uv_run = ["uv", " run"].concat();
            let package_install = ["pip", " install"].concat();
            if lower.contains(&format!("{script_runtime} "))
                || lower.contains(&uv_run)
                || lower.contains(&package_install)
            {
                issues.push(serde_json::json!({
                    "code": "external_runtime_dependency",
                    "path": path.display().to_string(),
                    "detail": "artifact references a non-Rust runtime"
                }));
            }
            let normalized_text = lower.replace('\\', "/");
            let normalized_path = path
                .display()
                .to_string()
                .to_ascii_lowercase()
                .replace('\\', "/");
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let looks_like_launcher = lower.starts_with("@echo off")
                || lower.starts_with("#!/bin/sh")
                || file_name == "tokenzero.cmd"
                || normalized_path.ends_with("/.tokenzero/bin/tokenzero");
            if looks_like_launcher && normalized_text.contains("target/release/tokenzero") {
                issues.push(serde_json::json!({
                    "code": "dev_runtime_launcher",
                    "path": path.display().to_string(),
                    "detail": "launcher points at a development target/release binary"
                }));
            }
            if lower.contains("raw_traces")
                || lower.contains("lab_notes")
                || lower.contains("local_only")
            {
                issues.push(serde_json::json!({
                    "code": "non_release_artifact_reference",
                    "path": path.display().to_string(),
                    "detail": "artifact references non-release material"
                }));
            }
        }
    }
    serde_json::json!({
        "schema_version": "tokenzero.package_audit.v1",
        "status": if issues.is_empty() { "ok" } else { "blocked" },
        "ok": issues.is_empty(),
        "archives_checked": checked,
        "issue_count": issues.len(),
        "issues": issues,
        "external_runtime_required_for_core": false
    })
}

fn audit_artifact_member_names(path: &Path, issues: &mut Vec<serde_json::Value>) {
    let display = path.display().to_string();
    audit_public_member_name(&display, &display, false, false, issues);

    let Some(members) = archive_members(path, issues) else {
        return;
    };
    audit_archive_members(&display, members, 0, issues);
}

fn audit_archive_members(
    artifact: &str,
    members: Vec<ArchiveMember>,
    depth: usize,
    issues: &mut Vec<serde_json::Value>,
) {
    for member in members {
        audit_public_member_name(
            artifact,
            &member.name,
            true,
            matches!(member.kind, ArchiveMemberKind::Directory),
            issues,
        );
        if let Some(link_target) = member.link_target.as_deref() {
            audit_public_link_target(artifact, &member.name, link_target, member.kind, issues);
        }
        if let Some(nested_bytes) = member.nested_archive.as_deref() {
            if nested_bytes.len() > MAX_NESTED_ARCHIVE_BYTES {
                issues.push(serde_json::json!({
                    "code": "nested_archive_too_large",
                    "path": artifact,
                    "member": member.name.as_str(),
                    "detail": "nested archive exceeds the package-audit in-memory inspection limit"
                }));
                continue;
            }
            if depth >= MAX_NESTED_ARCHIVE_DEPTH {
                issues.push(serde_json::json!({
                    "code": "nested_archive_depth_exceeded",
                    "path": artifact,
                    "member": member.name.as_str(),
                    "detail": "nested archive exceeds the package-audit recursion limit"
                }));
                continue;
            }
            let nested_artifact = format!("{artifact}!{}", member.name);
            if let Some(nested_members) =
                archive_members_from_bytes(&member.name, nested_bytes, &nested_artifact, issues)
            {
                audit_archive_members(&nested_artifact, nested_members, depth + 1, issues);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ArchiveMemberKind {
    Path,
    Directory,
    Hardlink,
    Symlink,
}

impl ArchiveMemberKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Directory => "directory",
            Self::Hardlink => "hardlink",
            Self::Symlink => "symlink",
        }
    }
}

struct ArchiveMember {
    name: String,
    kind: ArchiveMemberKind,
    link_target: Option<String>,
    nested_archive: Option<Vec<u8>>,
}

fn archive_members(path: &Path, issues: &mut Vec<serde_json::Value>) -> Option<Vec<ArchiveMember>> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if !is_supported_archive_name(file_name) {
        return None;
    }
    let artifact = path.display().to_string();
    match fs::metadata(path) {
        Ok(metadata) if metadata.len() > MAX_TOP_LEVEL_ARCHIVE_BYTES => {
            issues.push(serde_json::json!({
                "code": "archive_file_too_large",
                "path": artifact,
                "size": metadata.len(),
                "limit": MAX_TOP_LEVEL_ARCHIVE_BYTES,
                "detail": "top-level archive exceeds the package-audit read budget; package-audit fails closed before loading it into memory"
            }));
            return None;
        }
        Ok(_) => {}
        Err(error) => {
            issues.push(serde_json::json!({
                "code": "archive_member_listing_failed",
                "path": artifact,
                "detail": format!("failed to stat archive: {error}")
            }));
            return None;
        }
    }
    match fs::read(path) {
        Ok(bytes) => archive_members_from_bytes(file_name, &bytes, &artifact, issues),
        Err(error) => {
            issues.push(serde_json::json!({
                "code": "archive_member_listing_failed",
                "path": artifact,
                "detail": format!("failed to read archive: {error}")
            }));
            None
        }
    }
}

fn archive_members_from_bytes(
    name: &str,
    bytes: &[u8],
    artifact: &str,
    issues: &mut Vec<serde_json::Value>,
) -> Option<Vec<ArchiveMember>> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".tar") {
        return Some(parse_tar_members(bytes, artifact, issues));
    }
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") || lower.ends_with(".crate") {
        match gzip_decompress_bytes(bytes) {
            Ok(decompressed) => return Some(parse_tar_members(&decompressed, artifact, issues)),
            Err(ArchivePayloadError::TooLarge) => {
                issues.push(serde_json::json!({
                    "code": "archive_member_listing_too_large",
                    "path": artifact,
                    "detail": "gzip archive expands beyond the package-audit decompression limit"
                }));
            }
            Err(ArchivePayloadError::Malformed(error)) => {
                issues.push(serde_json::json!({
                    "code": "archive_member_listing_unavailable",
                    "path": artifact,
                    "detail": format!("gzip archive member listing failed: {error}")
                }));
            }
        }
        return None;
    }
    if lower.ends_with(".zip") {
        return match parse_zip_members(bytes, artifact, issues) {
            Ok(members) => Some(members),
            Err(error) => {
                issues.push(serde_json::json!({
                    "code": "archive_member_listing_failed",
                    "path": artifact,
                    "detail": error
                }));
                None
            }
        };
    }
    None
}

fn is_supported_archive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".tar")
        || lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".crate")
        || lower.ends_with(".zip")
}

enum ArchivePayloadError {
    TooLarge,
    Malformed(String),
}

struct ZipPayloadBudget {
    remaining: usize,
    exhausted: bool,
}

impl ZipPayloadBudget {
    fn new() -> Self {
        Self {
            remaining: MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES,
            exhausted: false,
        }
    }

    fn consume(
        &mut self,
        artifact: &str,
        member: &str,
        uncompressed_size: usize,
        issues: &mut Vec<serde_json::Value>,
    ) -> bool {
        if self.exhausted {
            return false;
        }
        if uncompressed_size > self.remaining {
            self.exhausted = true;
            issues.push(serde_json::json!({
                "code": "zip_total_payload_size_exceeded",
                "path": artifact,
                "member": member,
                "uncompressed_size": uncompressed_size,
                "limit": MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES,
                "detail": "zip archive aggregate uncompressed payload size exceeds the package-audit budget; package-audit fails closed"
            }));
            return false;
        }
        self.remaining -= uncompressed_size;
        true
    }
}

fn gzip_decompress_bytes(bytes: &[u8]) -> Result<Vec<u8>, ArchivePayloadError> {
    read_bounded_decoder(MultiGzDecoder::new(bytes), MAX_GZIP_DECOMPRESSED_BYTES)
}

fn deflate_decompress_bytes(bytes: &[u8]) -> Result<Vec<u8>, ArchivePayloadError> {
    read_bounded_decoder(DeflateDecoder::new(bytes), MAX_NESTED_ARCHIVE_BYTES)
}

fn read_bounded_decoder<R: Read>(
    decoder: R,
    max_bytes: usize,
) -> Result<Vec<u8>, ArchivePayloadError> {
    let mut output = Vec::new();
    let limit = max_bytes.saturating_add(1) as u64;
    decoder
        .take(limit)
        .read_to_end(&mut output)
        .map_err(|error| ArchivePayloadError::Malformed(error.to_string()))?;
    if output.len() > max_bytes {
        return Err(ArchivePayloadError::TooLarge);
    }
    Ok(output)
}

mod paths;
mod tar;
mod zip;

use paths::*;
use tar::*;
use zip::*;

#[cfg(test)]
mod tests;
