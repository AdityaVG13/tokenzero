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
        let (code, detail) = match finding {
            PathClassification::PrivateToolState => (
                "private_tool_state_member",
                "archive includes private local AI/tool state",
            ),
            PathClassification::NonPublicDotdir => (
                "non_public_dotdir_member",
                "archive includes a non-allowlisted dot directory",
            ),
            PathClassification::Sensitive => (
                "sensitive_member_name",
                "archive or artifact member name looks credential-bearing",
            ),
            PathClassification::LocalGenerated => (
                "local_generated_member",
                "archive includes local database, backup, dump, or generated metadata",
            ),
        };
        issues.push(serde_json::json!({
            "code": code,
            "path": artifact,
            "member": member,
            "detail": detail
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
        let (code, detail) = match finding {
            PathClassification::PrivateToolState => (
                "private_tool_state_link_target",
                "archive link target points at private local AI/tool state",
            ),
            PathClassification::NonPublicDotdir => (
                "non_public_dotdir_link_target",
                "archive link target points at a non-allowlisted dot directory",
            ),
            PathClassification::Sensitive => (
                "sensitive_link_target",
                "archive link target looks credential-bearing",
            ),
            PathClassification::LocalGenerated => (
                "local_generated_link_target",
                "archive link target points at local database, backup, dump, or generated metadata",
            ),
        };
        issues.push(serde_json::json!({
            "code": code,
            "path": artifact,
            "member": member,
            "link_target": target,
            "link_kind": link_kind,
            "detail": detail
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

pub(crate) fn is_private_tool_dotdir(part: &str) -> bool {
    matches!(
        part,
        ".aider"
            | ".anthropic"
            | ".browser-harness"
            | ".claude"
            | ".cline"
            | ".codex"
            | ".continue"
            | ".cursor"
            | ".dev-browser"
            | ".devin"
            | ".droid"
            | ".factory"
            | ".gemini"
            | ".grok"
            | ".mcp"
            | ".openai"
            | ".opencode"
            | ".playwright-mcp"
            | ".tokenzero"
            | ".windsurf"
    )
}

pub(crate) fn is_public_dotdir(part: &str) -> bool {
    matches!(
        part,
        ".azuredevops"
            | ".buildkite"
            | ".cargo"
            | ".changeset"
            | ".circleci"
            | ".devcontainer"
            | ".forgejo"
            | ".gitea"
            | ".github"
            | ".gitlab"
            | ".husky"
            | ".storybook"
            | ".vscode"
            | ".well-known"
            | ".yarn"
    )
}

pub(crate) fn is_sensitive_member_leaf(leaf: &str) -> bool {
    matches!(
        leaf,
        ".env"
            | ".npmrc"
            | ".pypirc"
            | ".netrc"
            | "id_rsa"
            | "id_dsa"
            | "id_ecdsa"
            | "id_ed25519"
            | "auth.json"
            | "credentials"
            | "credentials.json"
    ) || leaf.starts_with(".env.")
        || leaf.ends_with(".pem")
        || leaf.ends_with(".key")
        || leaf.ends_with(".p12")
        || leaf.ends_with(".pfx")
        || leaf.ends_with(".ppk")
        || leaf.ends_with(".ovpn")
        || leaf.ends_with(".kdbx")
}

pub(crate) fn is_local_generated_member_leaf(leaf: &str) -> bool {
    leaf.ends_with(".sqlite")
        || leaf.ends_with(".sqlite3")
        || leaf.ends_with(".db")
        || leaf.ends_with(".bak")
        || leaf.ends_with(".backup")
        || leaf.ends_with(".dump")
        || leaf.ends_with(".dmp")
        || leaf.contains("transcript")
        || leaf.contains("chat-export")
        || leaf.contains("chat_export")
        || leaf.contains("conversation-export")
        || leaf.contains("conversation_export")
        || leaf.contains("debug-report")
        || leaf.contains("debug_report")
        || leaf.contains("screenshot")
        || leaf.contains("screen-shot")
        || leaf.contains("screen_shot")
        || leaf.contains("local-output")
        || leaf.contains("local_output")
        || leaf.contains("agent-output")
        || leaf.contains("agent_output")
}
