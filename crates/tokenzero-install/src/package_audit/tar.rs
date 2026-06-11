use super::*;

pub(crate) fn parse_tar_members(
    bytes: &[u8],
    artifact: &str,
    issues: &mut Vec<serde_json::Value>,
) -> Vec<ArchiveMember> {
    let mut members = Vec::new();
    let mut seen_names = HashSet::new();
    let mut offset = 0usize;
    let mut pending_long_name: Option<String> = None;
    let mut pending_long_link: Option<String> = None;
    let mut pending_pax_path: Option<PaxOverride> = None;
    let mut pending_pax_linkpath: Option<PaxOverride> = None;
    let mut global_pax_path: Option<String> = None;
    let mut global_pax_linkpath: Option<String> = None;
    let mut saw_end_marker = false;

    while offset + 512 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|byte| *byte == 0) {
            saw_end_marker = true;
            let trailing_start = offset + 512;
            if bytes[trailing_start..].iter().any(|byte| *byte != 0) {
                issues.push(serde_json::json!({
                    "code": "archive_trailing_data",
                    "path": artifact,
                    "offset": trailing_start,
                    "detail": "tar archive contains non-zero data after the end-of-archive marker; package-audit fails closed"
                }));
            }
            break;
        }
        let header_name = parse_tar_header_name(header).unwrap_or_else(|| "<unknown>".to_string());
        if members.len() >= MAX_ARCHIVE_MEMBERS {
            issues.push(serde_json::json!({
                "code": "archive_member_limit_exceeded",
                "path": artifact,
                "detail": "archive contains more members than package-audit will inspect"
            }));
            break;
        }
        if let Err(error) = validate_tar_checksum(header) {
            issues.push(serde_json::json!({
                "code": "archive_member_metadata_malformed",
                "path": artifact,
                "member": header_name.as_str(),
                "detail": format!("{error}; package-audit fails closed")
            }));
            break;
        }

        let typeflag = header[156];
        let size = match parse_tar_size(&header[124..136]) {
            Ok(size) => size,
            Err(error) => {
                issues.push(serde_json::json!({
                    "code": "archive_member_size_malformed",
                    "path": artifact,
                    "member": header_name.as_str(),
                    "detail": format!("tar member size field is {error}; package-audit fails closed")
                }));
                break;
            }
        };
        let data_start = offset + 512;
        let Some(data_end) = data_start.checked_add(size) else {
            issues.push(serde_json::json!({
                "code": "archive_member_size_malformed",
                "path": artifact,
                "member": header_name.as_str(),
                "detail": "tar member size field overflowed; package-audit fails closed"
            }));
            break;
        };
        if data_end > bytes.len() {
            issues.push(serde_json::json!({
                "code": "archive_member_payload_truncated",
                "path": artifact,
                "member": header_name.as_str(),
                "detail": "tar member payload is shorter than the declared size; package-audit fails closed"
            }));
            break;
        }
        let data = &bytes[data_start..data_end];
        audit_tar_header_name_encoding(artifact, &header_name, header, issues);
        audit_tar_owner_metadata(artifact, &header_name, header, issues);

        match typeflag {
            b'L' => match parse_tar_payload_path(data) {
                Ok(path) => {
                    pending_long_name = path;
                }
                Err(reason) => {
                    push_archive_member_name_uninspectable(artifact, &header_name, reason, issues);
                }
            },
            b'K' => match parse_tar_payload_path(data) {
                Ok(path) => {
                    pending_long_link = path;
                }
                Err(reason) => {
                    push_archive_link_target_uninspectable(
                        artifact,
                        &header_name,
                        &lossy_tar_payload_path(data)
                            .unwrap_or_else(|| "<invalid-utf8>".to_string()),
                        ArchiveMemberKind::Symlink,
                        reason,
                        issues,
                    );
                }
            },
            b'x' => match parse_pax_overrides(data) {
                Ok(pax) => {
                    let metadata_member =
                        parse_tar_header_name(header).unwrap_or_else(|| "<pax>".to_string());
                    audit_pax_metadata_fields(
                        artifact,
                        &metadata_member,
                        &pax.metadata_fields,
                        issues,
                    );
                    pending_pax_path = pax.path;
                    pending_pax_linkpath = pax.linkpath;
                }
                Err(error) => {
                    pending_pax_path = None;
                    pending_pax_linkpath = None;
                    issues.push(serde_json::json!({
                            "code": "archive_member_metadata_malformed",
                            "path": artifact,
                            "member": parse_tar_header_name(header).unwrap_or_else(|| "<pax>".to_string()),
                            "detail": format!("malformed pax extended header: {error}; package-audit fails closed")
                        }));
                }
            },
            b'g' => {
                let metadata_member =
                    parse_tar_header_name(header).unwrap_or_else(|| "<global-pax>".to_string());
                match parse_pax_overrides(data) {
                    Ok(pax) => {
                        audit_pax_metadata_fields(
                            artifact,
                            &metadata_member,
                            &pax.metadata_fields,
                            issues,
                        );
                        match pax.path {
                            Some(PaxOverride::Set(path)) => {
                                push_global_pax_override_issue(
                                    artifact,
                                    &metadata_member,
                                    "path",
                                    issues,
                                );
                                audit_public_member_name(
                                    artifact,
                                    &path,
                                    true,
                                    path.replace('\\', "/").ends_with('/'),
                                    issues,
                                );
                                global_pax_path = Some(path);
                            }
                            Some(PaxOverride::Delete) => {
                                global_pax_path = None;
                            }
                            None => {}
                        }
                        match pax.linkpath {
                            Some(PaxOverride::Set(linkpath)) => {
                                push_global_pax_override_issue(
                                    artifact,
                                    &metadata_member,
                                    "linkpath",
                                    issues,
                                );
                                audit_public_link_target(
                                    artifact,
                                    &metadata_member,
                                    &linkpath,
                                    ArchiveMemberKind::Symlink,
                                    issues,
                                );
                                global_pax_linkpath = Some(linkpath);
                            }
                            Some(PaxOverride::Delete) => {
                                global_pax_linkpath = None;
                            }
                            None => {}
                        }
                    }
                    Err(error) => {
                        issues.push(serde_json::json!({
                            "code": "archive_member_metadata_malformed",
                            "path": artifact,
                            "member": metadata_member,
                            "detail": format!("malformed pax global header: {error}; package-audit fails closed")
                        }));
                    }
                }
            }
            _ => {
                let names = tar_member_name_candidates(
                    &mut pending_long_name,
                    &mut pending_pax_path,
                    global_pax_path.as_ref(),
                    header,
                );
                for name in names {
                    let kind = match typeflag {
                        b'5' => ArchiveMemberKind::Directory,
                        b'1' => ArchiveMemberKind::Hardlink,
                        b'2' => ArchiveMemberKind::Symlink,
                        _ => ArchiveMemberKind::Path,
                    };
                    if matches!(kind, ArchiveMemberKind::Directory) && size != 0 {
                        issues.push(serde_json::json!({
                            "code": "tar_directory_payload_present",
                            "path": artifact,
                            "member": name.as_str(),
                            "declared_size": size,
                            "detail": "tar directory entry carries payload bytes; package-audit fails closed"
                        }));
                    }
                    if let Some(reason) = unsupported_tar_member_typeflag(typeflag) {
                        issues.push(serde_json::json!({
                            "code": "archive_unsupported_member_type",
                            "path": artifact,
                            "member": name.as_str(),
                            "typeflag": format!("0x{typeflag:02x}"),
                            "reason": reason,
                            "detail": "tar member uses a special or unsupported typeflag with extractor-dependent semantics; package-audit fails closed"
                        }));
                    }
                    if matches!(
                        kind,
                        ArchiveMemberKind::Hardlink | ArchiveMemberKind::Symlink
                    ) {
                        audit_tar_header_link_encoding(artifact, &name, header, kind, issues);
                    }
                    if !seen_names.insert(name.clone()) {
                        issues.push(serde_json::json!({
                            "code": "tar_duplicate_member_name",
                            "path": artifact,
                            "member": name.as_str(),
                            "detail": "tar archive contains duplicate member names with extractor-dependent overwrite behavior"
                        }));
                    }
                    let link_targets = match kind {
                        ArchiveMemberKind::Hardlink | ArchiveMemberKind::Symlink => {
                            tar_member_link_candidates(
                                &mut pending_long_link,
                                &mut pending_pax_linkpath,
                                global_pax_linkpath.as_ref(),
                                header,
                            )
                        }
                        ArchiveMemberKind::Path | ArchiveMemberKind::Directory => {
                            pending_long_link = None;
                            pending_pax_linkpath = None;
                            vec![None]
                        }
                    };
                    for link_target in link_targets {
                        if matches!(kind, ArchiveMemberKind::Path) {
                            audit_archive_executable_payload(artifact, &name, data, issues);
                        }
                        let nested_archive = if matches!(kind, ArchiveMemberKind::Path)
                            && is_supported_archive_name(&name)
                        {
                            Some(data.to_vec())
                        } else {
                            None
                        };
                        members.push(ArchiveMember {
                            name: name.clone(),
                            kind,
                            link_target,
                            nested_archive,
                        });
                    }
                }
            }
        }

        let data_blocks = size.div_ceil(512);
        offset = offset.saturating_add(512).saturating_add(data_blocks * 512);
    }
    if !saw_end_marker {
        issues.push(serde_json::json!({
            "code": "archive_member_metadata_malformed",
            "path": artifact,
            "detail": "tar archive is missing the end-of-archive marker; package-audit fails closed"
        }));
    }
    members
}

pub(crate) fn tar_member_name_candidates(
    pending_long_name: &mut Option<String>,
    pending_pax_path: &mut Option<PaxOverride>,
    global_pax_path: Option<&String>,
    header: &[u8],
) -> Vec<String> {
    let mut candidates = Vec::new();
    let pending_pax_path = pending_pax_path.take();
    let suppress_global_pax_path = matches!(pending_pax_path, Some(PaxOverride::Delete));
    push_unique_string(&mut candidates, pending_long_name.take());
    if let Some(PaxOverride::Set(path)) = pending_pax_path {
        push_unique_string(&mut candidates, Some(path));
    }
    if !suppress_global_pax_path {
        push_unique_string(&mut candidates, global_pax_path.cloned());
    }
    push_unique_string(&mut candidates, parse_tar_header_name(header));
    candidates
}

pub(crate) fn tar_member_link_candidates(
    pending_long_link: &mut Option<String>,
    pending_pax_linkpath: &mut Option<PaxOverride>,
    global_pax_linkpath: Option<&String>,
    header: &[u8],
) -> Vec<Option<String>> {
    let mut candidates = Vec::new();
    let pending_pax_linkpath = pending_pax_linkpath.take();
    let suppress_global_pax_linkpath = matches!(pending_pax_linkpath, Some(PaxOverride::Delete));
    push_unique_string(&mut candidates, pending_long_link.take());
    if let Some(PaxOverride::Set(linkpath)) = pending_pax_linkpath {
        push_unique_string(&mut candidates, Some(linkpath));
    }
    if !suppress_global_pax_linkpath {
        push_unique_string(&mut candidates, global_pax_linkpath.cloned());
    }
    push_unique_string(&mut candidates, parse_tar_header_link_name(header));
    if candidates.is_empty() {
        vec![None]
    } else {
        candidates.into_iter().map(Some).collect()
    }
}

pub(crate) fn unsupported_tar_member_typeflag(typeflag: u8) -> Option<&'static str> {
    match typeflag {
        b'\0' | b'0' | b'1' | b'2' | b'5' => None,
        b'3' => Some("character_device"),
        b'4' => Some("block_device"),
        b'6' => Some("fifo"),
        b'7' => Some("contiguous_file"),
        b'A' => Some("acl"),
        b'D' => Some("directory_dump"),
        b'M' => Some("multi_volume_continuation"),
        b'N' => Some("legacy_long_name"),
        b'S' | b's' => Some("sparse_file"),
        b'V' => Some("volume_header"),
        b'X' => Some("extended_header"),
        _ => Some("unknown"),
    }
}

pub(crate) fn push_unique_string(candidates: &mut Vec<String>, value: Option<String>) {
    if let Some(value) = value
        && !candidates.iter().any(|candidate| candidate == &value)
    {
        candidates.push(value);
    }
}

pub(crate) fn parse_tar_header_name(header: &[u8]) -> Option<String> {
    let name = nul_terminated(&header[0..100]);
    if name.is_empty() {
        return None;
    }
    let prefix = nul_terminated(&header[345..500]);
    if prefix.is_empty() {
        Some(name)
    } else {
        Some(format!("{prefix}/{name}"))
    }
}

pub(crate) fn parse_tar_header_link_name(header: &[u8]) -> Option<String> {
    let link = nul_terminated(&header[157..257]);
    (!link.is_empty()).then_some(link)
}

pub(crate) fn nul_terminated(bytes: &[u8]) -> String {
    String::from_utf8_lossy(nul_terminated_bytes(bytes))
        .trim()
        .to_string()
}

pub(crate) fn nul_terminated_bytes(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    &bytes[..end]
}

pub(crate) fn parse_tar_payload_path(payload: &[u8]) -> Result<Option<String>, &'static str> {
    let raw_path = tar_payload_path_bytes(payload);
    let path = std::str::from_utf8(raw_path)
        .map_err(|_| "invalid_utf8")?
        .trim_end_matches('\n')
        .to_string();
    Ok((!path.is_empty()).then_some(path))
}

pub(crate) fn lossy_tar_payload_path(payload: &[u8]) -> Option<String> {
    let path = String::from_utf8_lossy(tar_payload_path_bytes(payload))
        .trim_end_matches('\n')
        .to_string();
    (!path.is_empty()).then_some(path)
}

pub(crate) fn tar_payload_path_bytes(payload: &[u8]) -> &[u8] {
    match payload.iter().position(|byte| *byte == 0) {
        Some(end) => &payload[..end],
        None => payload,
    }
}

#[derive(Default)]
pub(crate) struct PaxOverrides {
    pub(crate) path: Option<PaxOverride>,
    pub(crate) linkpath: Option<PaxOverride>,
    pub(crate) metadata_fields: Vec<String>,
}

pub(crate) enum PaxOverride {
    Set(String),
    Delete,
}

pub(crate) fn parse_pax_overrides(payload: &[u8]) -> Result<PaxOverrides, String> {
    let mut offset = 0usize;
    let mut overrides = PaxOverrides::default();

    while offset < payload.len() {
        let space = payload[offset..]
            .iter()
            .position(|byte| *byte == b' ')
            .ok_or_else(|| "record length separator is missing".to_string())?;
        let length = std::str::from_utf8(&payload[offset..offset + space])
            .map_err(|_| "record length is not utf-8".to_string())?
            .parse::<usize>()
            .map_err(|_| "record length is not numeric".to_string())?;
        let record_start = offset + space + 1;
        let record_end = offset
            .checked_add(length)
            .ok_or_else(|| "record length overflowed".to_string())?;
        if length == 0 || record_start > record_end || record_end > payload.len() {
            return Err("record length points outside payload".to_string());
        }

        let mut record = &payload[record_start..record_end];
        if !record.ends_with(b"\n") {
            return Err("record is missing trailing newline".to_string());
        }
        record = &record[..record.len() - 1];
        let separator = record
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or_else(|| "record key/value separator is missing".to_string())?;
        let key = parse_pax_key(&record[..separator])?;
        let value = &record[separator + 1..];
        if key == "path" {
            if value.is_empty() {
                overrides.path = Some(PaxOverride::Delete);
                offset = record_end;
                continue;
            }
            if overrides.path.is_some() {
                return Err("duplicate path override".to_string());
            }
            let path = std::str::from_utf8(value)
                .map_err(|_| "path override is not valid UTF-8".to_string())?;
            overrides.path = Some(PaxOverride::Set(path.to_string()));
        } else if key == "linkpath" {
            if value.is_empty() {
                overrides.linkpath = Some(PaxOverride::Delete);
                offset = record_end;
                continue;
            }
            if overrides.linkpath.is_some() {
                return Err("duplicate linkpath override".to_string());
            }
            let linkpath = std::str::from_utf8(value)
                .map_err(|_| "linkpath override is not valid UTF-8".to_string())?;
            overrides.linkpath = Some(PaxOverride::Set(linkpath.to_string()));
        } else {
            push_unique_string(
                &mut overrides.metadata_fields,
                Some(pax_metadata_field_label(key)),
            );
        }

        offset = record_end;
    }

    Ok(overrides)
}

pub(crate) fn parse_pax_key(key: &[u8]) -> Result<&str, String> {
    if key.is_empty() {
        return Err("record key is empty".to_string());
    }
    let key = std::str::from_utf8(key).map_err(|_| "record key is not valid UTF-8".to_string())?;
    if key.chars().any(|ch| ch.is_control()) {
        return Err("record key contains a control character".to_string());
    }
    Ok(key)
}

pub(crate) fn pax_metadata_field_label(key: &str) -> String {
    let lower = key.to_ascii_lowercase();
    if lower.starts_with("schily.xattr.") {
        return "SCHILY.xattr.*".to_string();
    }
    if lower.starts_with("libarchive.xattr.") {
        return "LIBARCHIVE.xattr.*".to_string();
    }
    if lower.starts_with("security.") {
        return "security.*".to_string();
    }
    if key.len() <= 64
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return key.to_string();
    }
    format!("custom_key_len_{}", key.len())
}

pub(crate) fn audit_pax_metadata_fields(
    artifact: &str,
    member: &str,
    fields: &[String],
    issues: &mut Vec<serde_json::Value>,
) {
    if fields.is_empty() {
        return;
    }
    issues.push(serde_json::json!({
        "code": "archive_pax_metadata_present",
        "path": artifact,
        "member": member,
        "fields": fields,
        "detail": "tar PAX extended header carries unsupported public metadata fields; package-audit fails closed without exposing field values"
    }));
}

pub(crate) fn push_global_pax_override_issue(
    artifact: &str,
    member: &str,
    field: &'static str,
    issues: &mut Vec<serde_json::Value>,
) {
    issues.push(serde_json::json!({
        "code": "archive_global_pax_override_present",
        "path": artifact,
        "member": member,
        "field": field,
        "detail": "tar global PAX path/linkpath overrides have extractor-dependent scope; package-audit fails closed without exposing override values"
    }));
}

pub(crate) fn parse_tar_size(bytes: &[u8]) -> Result<usize, String> {
    if bytes.first().is_some_and(|byte| byte & 0x80 != 0) {
        return parse_tar_base256_usize(bytes);
    }
    parse_tar_octal(bytes).ok_or_else(|| "malformed".to_string())
}

pub(crate) fn parse_tar_base256_usize(bytes: &[u8]) -> Result<usize, String> {
    let Some(first) = bytes.first() else {
        return Err("malformed".to_string());
    };
    if first & 0x40 != 0 {
        return Err("negative base-256 value".to_string());
    }

    let mut value = (first & 0x7f) as u128;
    for byte in &bytes[1..] {
        value = value
            .checked_mul(256)
            .and_then(|value| value.checked_add(*byte as u128))
            .ok_or_else(|| "base-256 value overflowed".to_string())?;
    }
    usize::try_from(value).map_err(|_| "base-256 value is too large".to_string())
}

pub(crate) fn parse_tar_octal(bytes: &[u8]) -> Option<usize> {
    let text = nul_terminated(bytes);
    usize::from_str_radix(text.trim(), 8).ok()
}

pub(crate) fn validate_tar_checksum(header: &[u8]) -> Result<(), String> {
    let stored = parse_tar_octal(&header[148..156])
        .ok_or_else(|| "tar header checksum field is malformed".to_string())?
        as u32;
    let unsigned = tar_checksum_unsigned(header);
    let signed = tar_checksum_signed(header);
    if stored == unsigned || signed.is_some_and(|signed| stored == signed) {
        Ok(())
    } else {
        Err(format!(
            "tar header checksum mismatch (stored {stored:o}, expected {unsigned:o})"
        ))
    }
}

pub(crate) fn tar_checksum_unsigned(header: &[u8]) -> u32 {
    header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                b' ' as u32
            } else {
                *byte as u32
            }
        })
        .sum()
}

pub(crate) fn tar_checksum_signed(header: &[u8]) -> Option<u32> {
    let checksum: i64 = header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                b' ' as i64
            } else {
                (*byte as i8) as i64
            }
        })
        .sum();
    u32::try_from(checksum).ok()
}

pub(crate) fn audit_tar_owner_metadata(
    artifact: &str,
    member: &str,
    header: &[u8],
    issues: &mut Vec<serde_json::Value>,
) {
    let mut fields = Vec::new();
    if tar_numeric_owner_field_is_private(&header[108..116]) {
        fields.push("uid");
    }
    if tar_numeric_owner_field_is_private(&header[116..124]) {
        fields.push("gid");
    }
    if tar_named_owner_field_is_private(&header[265..297]) {
        fields.push("uname");
    }
    if tar_named_owner_field_is_private(&header[297..329]) {
        fields.push("gname");
    }
    if !fields.is_empty() {
        issues.push(serde_json::json!({
            "code": "archive_private_owner_metadata",
            "path": artifact,
            "member": member,
            "fields": fields,
            "detail": "tar header exposes non-root owner metadata; package-audit fails closed"
        }));
    }
}

pub(crate) fn tar_numeric_owner_field_is_private(field: &[u8]) -> bool {
    let value = nul_terminated(field);
    if value.is_empty() {
        return false;
    }
    usize::from_str_radix(value.trim(), 8) != Ok(0)
}

pub(crate) fn tar_named_owner_field_is_private(field: &[u8]) -> bool {
    let value = nul_terminated(field).to_ascii_lowercase();
    !matches!(value.as_str(), "" | "0" | "root" | "wheel")
}
