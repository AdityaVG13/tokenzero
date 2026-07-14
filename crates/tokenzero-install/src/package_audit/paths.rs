use super::*;
/// What a public archive path may not contain. `audit_public_member_name`
/// and `audit_public_link_target` share this walk and map each finding to
/// their own issue codes/details.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathClassification {
    PrivateToolState,
    NonPublicDotdir,
    Sensitive,
    LocalGenerated,
}

/// Codes/details for a path classification on a member name vs a link target.
struct ClassificationCodes {
    member_code: &'static str,
    member_detail: &'static str,
    link_code: &'static str,
    link_detail: &'static str,
}

fn classification_codes(finding: PathClassification) -> ClassificationCodes {
    match finding {
        PathClassification::PrivateToolState => ClassificationCodes {
            member_code: "private_tool_state_member",
            member_detail: "archive includes private local AI/tool state",
            link_code: "private_tool_state_link_target",
            link_detail: "archive link target points at private local AI/tool state",
        },
        PathClassification::NonPublicDotdir => ClassificationCodes {
            member_code: "non_public_dotdir_member",
            member_detail: "archive includes a non-allowlisted dot directory",
            link_code: "non_public_dotdir_link_target",
            link_detail: "archive link target points at a non-allowlisted dot directory",
        },
        PathClassification::Sensitive => ClassificationCodes {
            member_code: "sensitive_member_name",
            member_detail: "archive or artifact member name looks credential-bearing",
            link_code: "sensitive_link_target",
            link_detail: "archive link target looks credential-bearing",
        },
        PathClassification::LocalGenerated => ClassificationCodes {
            member_code: "local_generated_member",
            member_detail: "archive includes local database, backup, dump, or generated metadata",
            link_code: "local_generated_link_target",
            link_detail: "archive link target points at local database, backup, dump, or generated metadata",
        },
    }
}

/// Normalize backslashes, split into non-empty parts, and lowercase the leaf.
fn split_normalized(normalized: &str) -> (Vec<&str>, String) {
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let leaf = parts
        .last()
        .copied()
        .unwrap_or_default()
        .to_ascii_lowercase();
    (parts, leaf)
}

fn classify_public_path(
    parts: &[&str],
    leaf_is_directory: bool,
    leaf: &str,
) -> Vec<PathClassification> {
    let mut found = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let is_leaf = index + 1 == parts.len();
        let lower = part.to_ascii_lowercase();
        if is_private_tool_dotdir(&lower) {
            found.push(PathClassification::PrivateToolState);
            break;
        }
        if (!is_leaf || leaf_is_directory) && lower.starts_with('.') && !is_public_dotdir(&lower) {
            found.push(PathClassification::NonPublicDotdir);
            break;
        }
    }
    if is_sensitive_member_leaf(leaf) {
        found.push(PathClassification::Sensitive);
    }
    if is_local_generated_member_leaf(leaf) {
        found.push(PathClassification::LocalGenerated);
    }
    found
}
pub(crate) fn audit_public_member_name(
    artifact: &str,
    member: &str,
    check_path_escape: bool,
    member_is_directory: bool,
    issues: &mut Vec<serde_json::Value>,
) {
    if let Some(reason) = archive_path_control_reason(member) {
        push_archive_member_name_uninspectable(artifact, member, reason, issues);
    }
    let normalized = member.replace('\\', "/");
    let (parts, leaf) = split_normalized(&normalized);
    if check_path_escape {
        if let Some(reason) = archive_path_escape_reason(&normalized, &parts) {
            issues.push(serde_json::json!({
                "code": "archive_member_path_escape",
                "path": artifact,
                "member": member,
                "reason": reason,
                "detail": "archive member path escapes the package root"
            }));
        }
    }
    if leaf.starts_with("._") {
        issues.push(serde_json::json!({
            "code": "appledouble_metadata",
            "path": artifact,
            "member": member,
            "detail": "archive contains macOS AppleDouble metadata"
        }));
    }
    for finding in classify_public_path(&parts, member_is_directory, &leaf) {
        let codes = classification_codes(finding);
        issues.push(serde_json::json!({
            "code": codes.member_code,
            "path": artifact,
            "member": member,
            "detail": codes.member_detail
        }));
    }
}
pub(crate) fn audit_public_link_target(
    artifact: &str,
    member: &str,
    target: &str,
    kind: ArchiveMemberKind,
    issues: &mut Vec<serde_json::Value>,
) {
    if let Some(reason) = archive_path_control_reason(target) {
        push_archive_link_target_uninspectable(artifact, member, target, kind, reason, issues);
    }
    let normalized = target.replace('\\', "/");
    let target_is_directory = normalized.ends_with('/');
    let (parts, leaf) = split_normalized(&normalized);
    let link_kind = kind.as_str();
    if let Some(reason) = archive_path_escape_reason(&normalized, &parts) {
        issues.push(serde_json::json!({
            "code": "archive_link_target_escape",
            "path": artifact,
            "member": member,
            "link_target": target,
            "link_kind": link_kind,
            "reason": reason,
            "detail": "archive link target escapes the package root"
        }));
    }
    for finding in classify_public_path(&parts, target_is_directory, &leaf) {
        let codes = classification_codes(finding);
        issues.push(serde_json::json!({
            "code": codes.link_code,
            "path": artifact,
            "member": member,
            "link_target": target,
            "link_kind": link_kind,
            "detail": codes.link_detail
        }));
    }
}
pub(crate) fn archive_path_escape_reason(normalized: &str, parts: &[&str]) -> Option<&'static str> {
    if normalized.starts_with('/') {
        return Some("absolute_path");
    }
    if has_windows_drive_prefix(normalized) {
        return Some("windows_drive_path");
    }
    if parts.contains(&"..") {
        return Some("parent_directory");
    }
    None
}
pub(crate) fn archive_path_control_reason(path: &str) -> Option<&'static str> {
    for ch in path.chars() {
        if ch == '\0' {
            return Some("nul_byte");
        }
        if ch.is_control() {
            return Some("control_character");
        }
    }
    None
}
pub(crate) fn audit_tar_header_name_encoding(
    artifact: &str,
    member: &str,
    header: &[u8],
    issues: &mut Vec<serde_json::Value>,
) {
    if std::str::from_utf8(nul_terminated_bytes(&header[0..100])).is_err()
        || std::str::from_utf8(nul_terminated_bytes(&header[345..500])).is_err()
    {
        push_archive_member_name_uninspectable(artifact, member, "invalid_utf8", issues);
    }
}
pub(crate) fn audit_tar_header_link_encoding(
    artifact: &str,
    member: &str,
    header: &[u8],
    kind: ArchiveMemberKind,
    issues: &mut Vec<serde_json::Value>,
) {
    if std::str::from_utf8(nul_terminated_bytes(&header[157..257])).is_err() {
        let target =
            parse_tar_header_link_name(header).unwrap_or_else(|| "<invalid-utf8>".to_string());
        push_archive_link_target_uninspectable(
            artifact,
            member,
            &target,
            kind,
            "invalid_utf8",
            issues,
        );
    }
}
pub(crate) fn push_archive_member_name_uninspectable(
    artifact: &str,
    member: &str,
    reason: &'static str,
    issues: &mut Vec<serde_json::Value>,
) {
    issues.push(serde_json::json!({
        "code": "archive_member_name_uninspectable",
        "path": artifact,
        "member": member,
        "reason": reason,
        "detail": archive_uninspectable_detail(reason, "member name")
    }));
}
pub(crate) fn push_archive_link_target_uninspectable(
    artifact: &str,
    member: &str,
    target: &str,
    kind: ArchiveMemberKind,
    reason: &'static str,
    issues: &mut Vec<serde_json::Value>,
) {
    issues.push(serde_json::json!({
        "code": "archive_link_target_uninspectable",
        "path": artifact,
        "member": member,
        "link_target": target,
        "link_kind": kind.as_str(),
        "reason": reason,
        "detail": archive_uninspectable_detail(reason, "link target")
    }));
}
pub(crate) fn archive_uninspectable_detail(reason: &str, label: &str) -> String {
    match reason {
        "invalid_utf8" => format!("archive {label} is not valid UTF-8; package-audit fails closed"),
        _ => format!("archive {label} contains a control character; package-audit fails closed"),
    }
}
pub(crate) fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

const PRIVATE_TOOL_DOTDIRS: &[&str] = &[
    ".aider",
    ".anthropic",
    ".browser-harness",
    ".claude",
    ".cline",
    ".codex",
    ".continue",
    ".cursor",
    ".dev-browser",
    ".devin",
    ".droid",
    ".factory",
    ".gemini",
    ".grok",
    ".mcp",
    ".openai",
    ".opencode",
    ".playwright-mcp",
    ".tokenzero",
    ".windsurf",
];
const PUBLIC_DOTDIRS: &[&str] = &[
    ".azuredevops",
    ".buildkite",
    ".cargo",
    ".changeset",
    ".circleci",
    ".devcontainer",
    ".forgejo",
    ".gitea",
    ".github",
    ".gitlab",
    ".husky",
    ".storybook",
    ".vscode",
    ".well-known",
    ".yarn",
];
const SENSITIVE_LEAVES: &[&str] = &[
    ".env",
    ".npmrc",
    ".pypirc",
    ".netrc",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "auth.json",
    "credentials",
    "credentials.json",
];
const SENSITIVE_SUFFIXES: &[&str] = &[".pem", ".key", ".p12", ".pfx", ".ppk", ".ovpn", ".kdbx"];
const LOCAL_GENERATED_SUFFIXES: &[&str] =
    &[".sqlite", ".sqlite3", ".db", ".bak", ".backup", ".dump", ".dmp"];
const LOCAL_GENERATED_NEEDLES: &[&str] = &[
    "transcript",
    "chat-export",
    "chat_export",
    "conversation-export",
    "conversation_export",
    "debug-report",
    "debug_report",
    "screenshot",
    "screen-shot",
    "screen_shot",
    "local-output",
    "local_output",
    "agent-output",
    "agent_output",
];
pub(crate) fn is_private_tool_dotdir(part: &str) -> bool {
    PRIVATE_TOOL_DOTDIRS.contains(&part)
}
pub(crate) fn is_public_dotdir(part: &str) -> bool {
    PUBLIC_DOTDIRS.contains(&part)
}
pub(crate) fn is_sensitive_member_leaf(leaf: &str) -> bool {
    SENSITIVE_LEAVES.contains(&leaf)
        || leaf.starts_with(".env.")
        || SENSITIVE_SUFFIXES.iter().any(|suffix| leaf.ends_with(suffix))
}
pub(crate) fn is_local_generated_member_leaf(leaf: &str) -> bool {
    LOCAL_GENERATED_SUFFIXES
        .iter()
        .any(|suffix| leaf.ends_with(suffix))
        || LOCAL_GENERATED_NEEDLES
            .iter()
            .any(|needle| leaf.contains(needle))
}

const EXECUTABLE_LEAVES: &[&str] =
    &["tokenzero", "tokenzero.exe", "tokenzero.cmd", "tokenzero.js"];
const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "bat", "cmd", "com", "cjs", "dll", "dylib", "exe", "fish", "jar", "js", "mjs", "node", "php",
    "pl", "ps1", "psm1", "py", "rb", "sh", "so", "wasm", "zsh",
];
/// Format-agnostic executable/script payload audit (tar members and zip files).
pub(crate) fn audit_archive_executable_payload(
    artifact: &str,
    member: &str,
    payload: &[u8],
    issues: &mut Vec<serde_json::Value>,
) {
    if !is_executable_or_script_member_name(member) {
        return;
    }
    let Ok(text) = std::str::from_utf8(payload) else {
        return;
    };
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
            "path": artifact,
            "member": member,
            "detail": "archive executable/script member references a non-Rust runtime"
        }));
    }
    let normalized_text = lower.replace('\\', "/");
    let normalized_member = member.to_ascii_lowercase().replace('\\', "/");
    let leaf = normalized_member.rsplit('/').next().unwrap_or_default();
    let looks_like_launcher = lower.starts_with("@echo off")
        || lower.starts_with("#!/bin/sh")
        || leaf == "tokenzero.cmd"
        || normalized_member.ends_with("/.tokenzero/bin/tokenzero");
    if looks_like_launcher && normalized_text.contains("target/release/tokenzero") {
        issues.push(serde_json::json!({
            "code": "dev_runtime_launcher",
            "path": artifact,
            "member": member,
            "detail": "archive executable/script member points at a development target/release binary"
        }));
    }
    if lower.contains("raw_traces") || lower.contains("lab_notes") || lower.contains("local_only") {
        issues.push(serde_json::json!({
            "code": "non_release_artifact_reference",
            "path": artifact,
            "member": member,
            "detail": "archive executable/script member references non-release material"
        }));
    }
}
pub(crate) fn is_executable_or_script_member_name(name: &str) -> bool {
    let normalized = name.replace('\\', "/").to_ascii_lowercase();
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let Some(leaf) = parts.last().copied() else {
        return false;
    };
    if EXECUTABLE_LEAVES.contains(&leaf) || leaf.starts_with("tokenzero-runtime-") {
        return true;
    }
    if parts.contains(&"bin") && !leaf.contains('.') {
        return true;
    }
    let ext = leaf.rsplit_once('.').map(|(_, e)| e).unwrap_or_default();
    EXECUTABLE_EXTENSIONS.contains(&ext)
}
