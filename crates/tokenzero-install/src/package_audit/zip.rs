use super::*;

pub(crate) fn parse_zip_members(
    bytes: &[u8],
    artifact: &str,
    issues: &mut Vec<serde_json::Value>,
) -> Result<Vec<ArchiveMember>, String> {
    let eocd_offset = find_zip_eocd_for_audit(bytes)?;
    let archive_comment_len = zip_u16_at(bytes, eocd_offset + 20)? as usize;
    if archive_comment_len > 0 {
        issues.push(serde_json::json!({
            "code": "zip_archive_comment_present",
            "path": artifact,
            "comment_bytes": archive_comment_len,
            "detail": "zip archive comment is public metadata outside package member inspection; package-audit fails closed"
        }));
    }
    let disk_number = zip_u16_at(bytes, eocd_offset + 4)?;
    let central_directory_disk = zip_u16_at(bytes, eocd_offset + 6)?;
    let disk_entry_count = zip_u16_at(bytes, eocd_offset + 8)?;
    let entry_count = zip_u16_at(bytes, eocd_offset + 10)?;
    let directory_size = zip_u32_at(bytes, eocd_offset + 12)?;
    let directory_offset = zip_u32_at(bytes, eocd_offset + 16)?;
    if disk_number != 0 || central_directory_disk != 0 {
        return Err(
            "split or multi-disk zip archives are not supported by package-audit".to_string(),
        );
    }
    let zip_eocd = resolve_zip_eocd(
        bytes,
        eocd_offset,
        disk_entry_count,
        entry_count,
        directory_size,
        directory_offset,
    )?;
    let entry_count = zip_usize(zip_eocd.entry_count, "zip central directory entry count")?;
    let directory_size = zip_usize(zip_eocd.directory_size, "zip central directory size")?;
    let directory_offset = zip_usize(zip_eocd.directory_offset, "zip central directory offset")?;
    if zip_eocd.disk_entry_count != zip_eocd.entry_count {
        return Err("zip central directory entry count mismatch".to_string());
    }
    let directory_end = directory_offset
        .checked_add(directory_size)
        .ok_or_else(|| "zip central directory size overflowed".to_string())?;
    if directory_end > bytes.len() {
        return Err("zip central directory points outside the archive".to_string());
    }
    if directory_end > eocd_offset {
        return Err(
            "zip central directory overlaps or follows the end-of-central-directory record"
                .to_string(),
        );
    }

    let mut members = Vec::new();
    let mut seen_names = HashSet::new();
    let mut local_records = Vec::new();
    let mut payload_budget = ZipPayloadBudget::new();
    let mut offset = directory_offset;
    for _ in 0..entry_count {
        if members.len() >= MAX_ARCHIVE_MEMBERS {
            issues.push(serde_json::json!({
                "code": "archive_member_limit_exceeded",
                "path": artifact,
                "detail": "archive contains more members than package-audit will inspect"
            }));
            break;
        }
        if offset + 46 > directory_end {
            return Err("zip central directory entry is truncated".to_string());
        }
        if zip_u32_at(bytes, offset)? != 0x0201_4b50 {
            return Err("zip central directory entry has an invalid signature".to_string());
        }
        let version_made_by = zip_u16_at(bytes, offset + 4)?;
        let central_flags = zip_u16_at(bytes, offset + 8)?;
        let compression_method = zip_u16_at(bytes, offset + 10)?;
        let crc32 = zip_u32_at(bytes, offset + 16)?;
        let compressed_size_32 = zip_u32_at(bytes, offset + 20)?;
        let uncompressed_size_32 = zip_u32_at(bytes, offset + 24)?;
        let name_len = zip_u16_at(bytes, offset + 28)? as usize;
        let extra_len = zip_u16_at(bytes, offset + 30)? as usize;
        let comment_len = zip_u16_at(bytes, offset + 32)? as usize;
        let external_attrs = zip_u32_at(bytes, offset + 38)?;
        let local_header_offset_32 = zip_u32_at(bytes, offset + 42)?;
        let name_start = offset + 46;
        let name_end = name_start
            .checked_add(name_len)
            .ok_or_else(|| "zip entry name length overflowed".to_string())?;
        if name_end > directory_end {
            return Err("zip entry name points outside the central directory".to_string());
        }
        let extra_end = name_end
            .checked_add(extra_len)
            .ok_or_else(|| "zip central directory extra field length overflowed".to_string())?;
        if extra_end > directory_end {
            return Err("zip entry extra field points outside the central directory".to_string());
        }
        let next_offset = extra_end
            .checked_add(comment_len)
            .ok_or_else(|| "zip central directory cursor overflowed".to_string())?;
        if next_offset > directory_end {
            return Err("zip entry comment points outside the central directory".to_string());
        }
        let name_bytes = &bytes[name_start..name_end];
        let name = String::from_utf8_lossy(name_bytes).to_string();
        if std::str::from_utf8(name_bytes).is_err() {
            push_archive_member_name_uninspectable(artifact, &name, "invalid_utf8", issues);
        }
        if comment_len > 0 {
            issues.push(serde_json::json!({
                "code": "zip_entry_comment_present",
                "path": artifact,
                "member": name.as_str(),
                "comment_bytes": comment_len,
                "detail": "zip entry comment is public metadata outside package member inspection; package-audit fails closed"
            }));
        }
        let central_extra = &bytes[name_end..extra_end];
        let central_zip64_needs = Zip64FieldNeeds {
            uncompressed_size: uncompressed_size_32 == u32::MAX,
            compressed_size: compressed_size_32 == u32::MAX,
            local_header_offset: local_header_offset_32 == u32::MAX,
        };
        audit_zip_extra_fields(
            artifact,
            &name,
            "central",
            central_extra,
            central_zip64_needs.any(),
            issues,
        )
        .map_err(|error| format!("zip central extra field for {name} is malformed: {error}"))?;
        let central_zip64 = zip64_extended_info(central_extra, central_zip64_needs)
            .map_err(|error| format!("zip central extra field for {name} is malformed: {error}"))?;
        let compressed_size = zip_usize(
            zip64_resolved_u32(
                compressed_size_32,
                central_zip64
                    .as_ref()
                    .and_then(|zip64| zip64.compressed_size),
                "compressed size",
            )?,
            "zip entry compressed size",
        )?;
        let uncompressed_size = zip_usize(
            zip64_resolved_u32(
                uncompressed_size_32,
                central_zip64
                    .as_ref()
                    .and_then(|zip64| zip64.uncompressed_size),
                "uncompressed size",
            )?,
            "zip entry uncompressed size",
        )?;
        let local_header_offset = zip_usize(
            zip64_resolved_u32(
                local_header_offset_32,
                central_zip64
                    .as_ref()
                    .and_then(|zip64| zip64.local_header_offset),
                "local header offset",
            )?,
            "zip local header offset",
        )?;
        let central_unicode_name = zip_unicode_path_extra(central_extra, name_bytes)
            .map_err(|error| format!("zip central extra field for {name} is malformed: {error}"))?;
        let local_header = zip_local_header(bytes, local_header_offset).map_err(|error| {
            format!(
                "zip local header for {name} could not be read: {}",
                zip_payload_error_detail(error)
            )
        })?;
        audit_zip_extra_fields(
            artifact,
            &name,
            "local",
            &local_header.extra,
            local_header.zip64_needed,
            issues,
        )
        .map_err(|error| format!("zip local extra field for {name} is malformed: {error}"))?;
        if !local_header.name_is_utf8 {
            push_archive_member_name_uninspectable(
                artifact,
                &local_header.name,
                "invalid_utf8",
                issues,
            );
        }
        let payload_end = local_header
            .data_start
            .checked_add(compressed_size)
            .ok_or_else(|| format!("zip entry payload size overflowed for {name}"))?;
        let mut local_record_end = payload_end;
        if payload_end > directory_offset {
            issues.push(serde_json::json!({
                "code": "zip_local_record_overlap",
                "path": artifact,
                "member": name.as_str(),
                "field": "central_directory",
                "payload_end": payload_end,
                "central_directory_offset": directory_offset,
                "detail": "zip local entry payload overlaps the central directory; package-audit fails closed"
            }));
        }
        let unsafe_flags = zip_unsafe_general_purpose_flags(central_flags | local_header.flags);
        if unsafe_flags != 0 {
            issues.push(serde_json::json!({
                "code": "zip_entry_uninspectable",
                "path": artifact,
                "member": name.as_str(),
                "flags": zip_flag_names(unsafe_flags),
                "detail": "zip entry uses encryption or masked metadata; package-audit fails closed"
            }));
        }
        if central_flags != local_header.flags {
            issues.push(serde_json::json!({
                "code": "zip_local_header_metadata_mismatch",
                "path": artifact,
                "member": name.as_str(),
                "field": "general_purpose_flags",
                "central": central_flags,
                "local": local_header.flags,
                "detail": "zip central-directory flags do not match the local-header flags; package-audit fails closed"
            }));
        }
        if compression_method != local_header.compression_method {
            issues.push(serde_json::json!({
                "code": "zip_local_header_metadata_mismatch",
                "path": artifact,
                "member": name.as_str(),
                "field": "compression_method",
                "central": compression_method,
                "local": local_header.compression_method,
                "detail": "zip central-directory compression method does not match the local-header method; package-audit fails closed"
            }));
        }
        let uses_data_descriptor =
            (central_flags | local_header.flags) & ZIP_FLAG_DATA_DESCRIPTOR != 0;
        if crc32 != local_header.crc32 && (!uses_data_descriptor || local_header.crc32 != 0) {
            issues.push(serde_json::json!({
                "code": "zip_local_header_metadata_mismatch",
                "path": artifact,
                "member": name.as_str(),
                "field": "crc32",
                "central": crc32,
                "local": local_header.crc32,
                "detail": "zip central-directory CRC does not match the local-header CRC; package-audit fails closed"
            }));
        }
        if uses_data_descriptor {
            let local_sizes_are_zero =
                local_header.compressed_size == 0 && local_header.uncompressed_size == 0;
            let local_sizes_match_central = local_header.compressed_size == compressed_size
                && local_header.uncompressed_size == uncompressed_size;
            if !local_sizes_are_zero && !local_sizes_match_central {
                issues.push(serde_json::json!({
                    "code": "zip_local_header_metadata_mismatch",
                    "path": artifact,
                    "member": name.as_str(),
                    "field": "data_descriptor_sizes",
                    "central_compressed_size": compressed_size,
                    "local_compressed_size": local_header.compressed_size,
                    "central_uncompressed_size": uncompressed_size,
                    "local_uncompressed_size": local_header.uncompressed_size,
                    "detail": "zip local-header sizes disagree with central-directory sizes while a data descriptor is enabled; package-audit fails closed"
                }));
            }
        }
        if central_flags & ZIP_FLAG_DATA_DESCRIPTOR == 0
            && (compressed_size != local_header.compressed_size
                || uncompressed_size != local_header.uncompressed_size)
        {
            issues.push(serde_json::json!({
                "code": "zip_local_header_metadata_mismatch",
                "path": artifact,
                "member": name.as_str(),
                "field": "sizes",
                "central_compressed_size": compressed_size,
                "local_compressed_size": local_header.compressed_size,
                "central_uncompressed_size": uncompressed_size,
                "local_uncompressed_size": local_header.uncompressed_size,
                "detail": "zip central-directory sizes do not match the local-header sizes; package-audit fails closed"
            }));
        }
        if compression_method == 0 && compressed_size != uncompressed_size {
            issues.push(serde_json::json!({
                "code": "zip_entry_size_mismatch",
                "path": artifact,
                "member": name.as_str(),
                "compressed_size": compressed_size,
                "uncompressed_size": uncompressed_size,
                "detail": "stored zip entry has different compressed and uncompressed sizes; package-audit fails closed"
            }));
        }
        if uses_data_descriptor {
            match zip_data_descriptor_matches(
                bytes,
                local_header.data_start,
                compressed_size,
                crc32,
                compressed_size,
                uncompressed_size,
                directory_offset,
            ) {
                Ok(descriptor_len) => {
                    local_record_end = payload_end
                        .checked_add(descriptor_len)
                        .ok_or_else(|| format!("zip data descriptor size overflowed for {name}"))?;
                }
                Err(detail) => {
                    issues.push(serde_json::json!({
                        "code": "zip_data_descriptor_mismatch",
                        "path": artifact,
                        "member": name.as_str(),
                        "field": "data_descriptor",
                        "detail": detail
                    }));
                }
            }
        }
        local_records.push(ZipLocalRecordRange {
            member: name.clone(),
            start: local_header_offset,
            end: local_record_end,
        });
        let names = zip_member_name_candidates(
            name.clone(),
            central_unicode_name.clone(),
            local_header.name.clone(),
            local_header.unicode_name.clone(),
        );
        let is_directory_entry = names
            .iter()
            .any(|candidate| zip_entry_is_directory(candidate, version_made_by, external_attrs));
        let has_nested_archive_name = names
            .iter()
            .any(|candidate| is_supported_archive_name(candidate));
        let payload_within_budget = !is_directory_entry
            && payload_budget.consume(artifact, &name, uncompressed_size, issues);
        if is_directory_entry {
            if compressed_size != 0 || uncompressed_size != 0 {
                issues.push(serde_json::json!({
                    "code": "zip_directory_payload_present",
                    "path": artifact,
                    "member": name.as_str(),
                    "compressed_size": compressed_size,
                    "uncompressed_size": uncompressed_size,
                    "detail": "zip directory entry carries payload bytes; package-audit fails closed"
                }));
            }
        } else if payload_within_budget {
            verify_zip_entry_payload_integrity(
                artifact,
                &name,
                bytes,
                local_header.data_start,
                compressed_size,
                compression_method,
                crc32,
                uncompressed_size,
                issues,
            );
        }
        if local_header.name != name {
            issues.push(serde_json::json!({
                "code": "zip_local_header_name_mismatch",
                "path": artifact,
                "member": name.as_str(),
                "local_member": local_header.name.as_str(),
                "detail": "zip central-directory name does not match the local-header name; package-audit fails closed across both names"
            }));
        }
        if let (Some(central_unicode), Some(local_unicode)) =
            (&central_unicode_name, &local_header.unicode_name)
            && central_unicode != local_unicode
        {
            issues.push(serde_json::json!({
                "code": "zip_local_header_metadata_mismatch",
                "path": artifact,
                "member": name.as_str(),
                "field": "unicode_path",
                "central": central_unicode,
                "local": local_unicode,
                "detail": "zip central-directory Unicode path extra field does not match the local-header Unicode path extra field; package-audit fails closed across both names"
            }));
        }
        let mut kind = ArchiveMemberKind::Path;
        let mut link_target = None;
        let mut nested_archive = None;
        if zip_entry_is_symlink(version_made_by, external_attrs) {
            kind = ArchiveMemberKind::Symlink;
            if payload_within_budget {
                match zip_entry_payload(
                    bytes,
                    local_header.data_start,
                    compressed_size,
                    compression_method,
                    crc32,
                    uncompressed_size,
                ) {
                    Ok(payload) => match parse_tar_payload_path(&payload) {
                        Ok(Some(target)) => {
                            link_target = Some(target);
                        }
                        Ok(None) => {
                            issues.push(serde_json::json!({
                                "code": "zip_symlink_target_unreadable",
                                "path": artifact,
                                "member": name.as_str(),
                                "detail": "zip symlink entry has an empty target payload"
                            }));
                        }
                        Err(reason) => {
                            push_archive_link_target_uninspectable(
                                artifact,
                                &name,
                                &lossy_tar_payload_path(&payload)
                                    .unwrap_or_else(|| "<invalid-utf8>".to_string()),
                                kind,
                                reason,
                                issues,
                            );
                        }
                    },
                    Err(ZipPayloadError::TooLarge) => {
                        issues.push(serde_json::json!({
                            "code": "zip_symlink_target_unreadable",
                            "path": artifact,
                            "member": name.as_str(),
                            "detail": "zip symlink target expands beyond the package-audit payload limit"
                        }));
                    }
                    Err(ZipPayloadError::UnsupportedCompression(method)) => {
                        issues.push(serde_json::json!({
                            "code": "zip_symlink_target_unreadable",
                            "path": artifact,
                            "member": name.as_str(),
                            "compression_method": method,
                            "detail": "zip symlink target uses unsupported compression; package-audit fails closed"
                        }));
                    }
                    Err(
                        error @ (ZipPayloadError::CrcMismatch { .. }
                        | ZipPayloadError::SizeMismatch { .. }
                        | ZipPayloadError::Malformed(_)),
                    ) => {
                        issues.push(serde_json::json!({
                            "code": "zip_symlink_target_unreadable",
                            "path": artifact,
                            "member": name.as_str(),
                            "detail": zip_payload_error_detail(error)
                        }));
                    }
                }
            }
        } else if is_directory_entry {
            kind = ArchiveMemberKind::Directory;
        } else if has_nested_archive_name && payload_within_budget {
            match zip_entry_payload(
                bytes,
                local_header.data_start,
                compressed_size,
                compression_method,
                crc32,
                uncompressed_size,
            ) {
                Ok(payload) => {
                    nested_archive = Some(payload.to_vec());
                }
                Err(ZipPayloadError::TooLarge) => {
                    issues.push(serde_json::json!({
                        "code": "nested_archive_too_large",
                        "path": artifact,
                        "member": name.as_str(),
                        "detail": "nested zip archive member expands beyond the package-audit in-memory inspection limit"
                    }));
                }
                Err(ZipPayloadError::UnsupportedCompression(method)) => {
                    issues.push(serde_json::json!({
                        "code": "nested_archive_member_unreadable",
                        "path": artifact,
                        "member": name.as_str(),
                        "compression_method": method,
                        "detail": "nested zip archive member uses unsupported compression; package-audit fails closed"
                    }));
                }
                Err(
                    error @ (ZipPayloadError::CrcMismatch { .. }
                    | ZipPayloadError::SizeMismatch { .. }
                    | ZipPayloadError::Malformed(_)),
                ) => {
                    issues.push(serde_json::json!({
                        "code": "nested_archive_member_unreadable",
                        "path": artifact,
                        "member": name.as_str(),
                        "detail": zip_payload_error_detail(error)
                    }));
                }
            }
        } else if zip_member_requires_payload_inspection(&names) && payload_within_budget {
            match zip_entry_payload(
                bytes,
                local_header.data_start,
                compressed_size,
                compression_method,
                crc32,
                uncompressed_size,
            ) {
                Ok(payload) => {
                    audit_zip_regular_file_payload(artifact, &names, payload.as_ref(), issues);
                }
                Err(ZipPayloadError::TooLarge) => {
                    issues.push(serde_json::json!({
                        "code": "zip_regular_file_uninspectable",
                        "path": artifact,
                        "member": name.as_str(),
                        "detail": "zip executable/script payload expands beyond the package-audit payload limit"
                    }));
                }
                Err(ZipPayloadError::UnsupportedCompression(method)) => {
                    issues.push(serde_json::json!({
                        "code": "zip_regular_file_uninspectable",
                        "path": artifact,
                        "member": name.as_str(),
                        "compression_method": method,
                        "detail": "zip executable/script payload uses unsupported compression; package-audit fails closed"
                    }));
                }
                Err(
                    error @ (ZipPayloadError::CrcMismatch { .. }
                    | ZipPayloadError::SizeMismatch { .. }
                    | ZipPayloadError::Malformed(_)),
                ) => {
                    issues.push(serde_json::json!({
                        "code": "zip_regular_file_uninspectable",
                        "path": artifact,
                        "member": name.as_str(),
                        "detail": zip_payload_error_detail(error)
                    }));
                }
            }
        }
        for candidate_name in names {
            if !seen_names.insert(candidate_name.clone()) {
                issues.push(serde_json::json!({
                    "code": "zip_duplicate_member_name",
                    "path": artifact,
                    "member": candidate_name.as_str(),
                    "detail": "zip archive contains duplicate member names with extractor-dependent overwrite behavior"
                }));
            }
            members.push(ArchiveMember {
                name: candidate_name,
                kind,
                link_target: link_target.clone(),
                nested_archive: nested_archive.clone(),
            });
        }
        offset = next_offset;
    }
    report_zip_local_record_layout(artifact, &mut local_records, directory_offset, issues);
    if offset != directory_end && members.len() < MAX_ARCHIVE_MEMBERS {
        return Err(
            "zip central directory contains unparsed bytes or entry count mismatch".to_string(),
        );
    }
    Ok(members)
}

pub(crate) fn find_zip_eocd_for_audit(bytes: &[u8]) -> Result<usize, String> {
    let candidates = zip_eocd_candidates(bytes);
    match candidates.as_slice() {
        [] => Err("zip end-of-central-directory record was not found".to_string()),
        [offset] => Ok(*offset),
        _ => Err(format!(
            "zip archive contains {} plausible end-of-central-directory records; package-audit fails closed",
            candidates.len()
        )),
    }
}

pub(crate) fn zip_eocd_candidates(bytes: &[u8]) -> Vec<usize> {
    let mut candidates = Vec::new();
    if bytes.len() < 22 {
        return candidates;
    }
    let search_start = bytes.len().saturating_sub(22 + u16::MAX as usize);
    for offset in (search_start..=bytes.len() - 22).rev() {
        if bytes.get(offset..offset + 4) == Some(&[0x50, 0x4b, 0x05, 0x06])
            && zip_u16_at(bytes, offset + 20)
                .map(|len| offset + 22 + len as usize == bytes.len())
                .unwrap_or(false)
        {
            candidates.push(offset);
        }
    }
    candidates
}

pub(crate) struct ZipEocd {
    pub(crate) disk_entry_count: u64,
    pub(crate) entry_count: u64,
    pub(crate) directory_size: u64,
    pub(crate) directory_offset: u64,
}

pub(crate) fn resolve_zip_eocd(
    bytes: &[u8],
    eocd_offset: usize,
    disk_entry_count: u16,
    entry_count: u16,
    directory_size: u32,
    directory_offset: u32,
) -> Result<ZipEocd, String> {
    if disk_entry_count != u16::MAX
        && entry_count != u16::MAX
        && directory_size != u32::MAX
        && directory_offset != u32::MAX
    {
        return Ok(ZipEocd {
            disk_entry_count: disk_entry_count as u64,
            entry_count: entry_count as u64,
            directory_size: directory_size as u64,
            directory_offset: directory_offset as u64,
        });
    }

    let locator_offset = eocd_offset.checked_sub(20).ok_or_else(|| {
        "zip64 end-of-central-directory locator was not found before the zip end-of-central-directory record".to_string()
    })?;
    if zip_u32_at(bytes, locator_offset)? != ZIP64_EOCD_LOCATOR_SIGNATURE {
        return Err(
            "zip64 end-of-central-directory locator was not found before the zip end-of-central-directory record".to_string(),
        );
    }
    let zip64_eocd_disk = zip_u32_at(bytes, locator_offset + 4)?;
    let zip64_eocd_offset = zip_u64_at(bytes, locator_offset + 8)?;
    let total_disks = zip_u32_at(bytes, locator_offset + 16)?;
    if zip64_eocd_disk != 0 || total_disks != 1 {
        return Err(
            "split or multi-disk zip64 archives are not supported by package-audit".to_string(),
        );
    }

    let zip64_eocd_offset = zip_usize(zip64_eocd_offset, "zip64 end-of-central-directory offset")?;
    let zip64_eocd_min_end = zip64_eocd_offset
        .checked_add(56)
        .ok_or_else(|| "zip64 end-of-central-directory offset overflowed".to_string())?;
    if zip64_eocd_min_end > bytes.len() {
        return Err("zip64 end-of-central-directory record points outside the archive".to_string());
    }
    if zip_u32_at(bytes, zip64_eocd_offset)? != ZIP64_EOCD_RECORD_SIGNATURE {
        return Err("zip64 end-of-central-directory record has an invalid signature".to_string());
    }
    let zip64_record_size = zip_u64_at(bytes, zip64_eocd_offset + 4)?;
    if zip64_record_size < 44 {
        return Err("zip64 end-of-central-directory record is truncated".to_string());
    }
    let zip64_record_size = zip_usize(
        zip64_record_size,
        "zip64 end-of-central-directory record size",
    )?;
    let zip64_record_end = zip64_eocd_offset
        .checked_add(12)
        .and_then(|offset| offset.checked_add(zip64_record_size))
        .ok_or_else(|| "zip64 end-of-central-directory size overflowed".to_string())?;
    if zip64_record_end > bytes.len() || zip64_record_end > locator_offset {
        return Err("zip64 end-of-central-directory record points outside the archive".to_string());
    }
    let disk_number = zip_u32_at(bytes, zip64_eocd_offset + 16)?;
    let central_directory_disk = zip_u32_at(bytes, zip64_eocd_offset + 20)?;
    if disk_number != 0 || central_directory_disk != 0 {
        return Err(
            "split or multi-disk zip64 archives are not supported by package-audit".to_string(),
        );
    }

    Ok(ZipEocd {
        disk_entry_count: zip_u64_at(bytes, zip64_eocd_offset + 24)?,
        entry_count: zip_u64_at(bytes, zip64_eocd_offset + 32)?,
        directory_size: zip_u64_at(bytes, zip64_eocd_offset + 40)?,
        directory_offset: zip_u64_at(bytes, zip64_eocd_offset + 48)?,
    })
}

pub(crate) fn zip_entry_is_symlink(version_made_by: u16, external_attrs: u32) -> bool {
    let host_os = version_made_by >> 8;
    let unix_mode = external_attrs >> 16;
    host_os == 3 && unix_mode & 0o170000 == 0o120000
}

pub(crate) fn zip_entry_is_directory(
    name: &str,
    version_made_by: u16,
    external_attrs: u32,
) -> bool {
    let host_os = version_made_by >> 8;
    let unix_mode = external_attrs >> 16;
    let dos_directory_attr = external_attrs & 0x10 != 0;
    name.ends_with('/') || (host_os == 3 && unix_mode & 0o170000 == 0o040000) || dos_directory_attr
}

pub(crate) fn zip_unsafe_general_purpose_flags(flags: u16) -> u16 {
    flags & (ZIP_FLAG_ENCRYPTED | ZIP_FLAG_STRONG_ENCRYPTION | ZIP_FLAG_MASKED_LOCAL_HEADER_VALUES)
}

pub(crate) fn zip_flag_names(flags: u16) -> Vec<&'static str> {
    let mut names = Vec::new();
    if flags & ZIP_FLAG_ENCRYPTED != 0 {
        names.push("encrypted");
    }
    if flags & ZIP_FLAG_STRONG_ENCRYPTION != 0 {
        names.push("strong_encryption");
    }
    if flags & ZIP_FLAG_MASKED_LOCAL_HEADER_VALUES != 0 {
        names.push("masked_local_header_values");
    }
    names
}

pub(crate) enum ZipPayloadError {
    TooLarge,
    UnsupportedCompression(u16),
    CrcMismatch { expected: u32, actual: u32 },
    SizeMismatch { expected: usize, actual: usize },
    Malformed(String),
}

pub(crate) struct ZipLocalHeader {
    pub(crate) name: String,
    pub(crate) unicode_name: Option<String>,
    pub(crate) name_is_utf8: bool,
    pub(crate) extra: Vec<u8>,
    pub(crate) zip64_needed: bool,
    pub(crate) data_start: usize,
    pub(crate) flags: u16,
    pub(crate) compression_method: u16,
    pub(crate) crc32: u32,
    pub(crate) compressed_size: usize,
    pub(crate) uncompressed_size: usize,
}

pub(crate) struct ZipLocalRecordRange {
    pub(crate) member: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn report_zip_local_record_layout(
    artifact: &str,
    records: &mut [ZipLocalRecordRange],
    central_directory_offset: usize,
    issues: &mut Vec<serde_json::Value>,
) {
    records.sort_by_key(|record| (record.start, record.end));
    let mut covered_end = 0usize;
    let mut active = 0usize;
    for index in 0..records.len() {
        if index > 0 && records[index].start < records[active].end {
            issues.push(serde_json::json!({
                "code": "zip_local_record_overlap",
                "path": artifact,
                "member": records[active].member.as_str(),
                "next_member": records[index].member.as_str(),
                "field": "local_record",
                "record_end": records[active].end,
                "next_record_start": records[index].start,
                "detail": "zip local entry records overlap each other; package-audit fails closed"
            }));
        }
        if index > 0 && records[index].end > records[active].end {
            active = index;
        }
        if records[index].start > covered_end {
            push_zip_unclaimed_local_bytes(artifact, covered_end, records[index].start, issues);
        }
        covered_end = covered_end.max(records[index].end);
    }
    if covered_end < central_directory_offset {
        push_zip_unclaimed_local_bytes(artifact, covered_end, central_directory_offset, issues);
    }
}

pub(crate) fn push_zip_unclaimed_local_bytes(
    artifact: &str,
    start: usize,
    end: usize,
    issues: &mut Vec<serde_json::Value>,
) {
    issues.push(serde_json::json!({
        "code": "zip_unclaimed_local_bytes",
        "path": artifact,
        "start": start,
        "end": end,
        "byte_count": end - start,
        "detail": "zip archive contains bytes outside declared local entry records before the central directory; package-audit fails closed"
    }));
}

pub(crate) fn zip_local_header(
    bytes: &[u8],
    local_header_offset: usize,
) -> Result<ZipLocalHeader, ZipPayloadError> {
    let fixed_header_end = local_header_offset.checked_add(30).ok_or_else(|| {
        ZipPayloadError::Malformed("zip local header offset overflowed".to_string())
    })?;
    if fixed_header_end > bytes.len() {
        return Err(ZipPayloadError::Malformed(
            "zip local header points outside the archive".to_string(),
        ));
    }
    if zip_u32_at(bytes, local_header_offset).map_err(ZipPayloadError::Malformed)? != 0x0403_4b50 {
        return Err(ZipPayloadError::Malformed(
            "zip local header has an invalid signature".to_string(),
        ));
    }
    let flags = zip_u16_at(bytes, local_header_offset + 6).map_err(ZipPayloadError::Malformed)?;
    let compression_method =
        zip_u16_at(bytes, local_header_offset + 8).map_err(ZipPayloadError::Malformed)?;
    let crc32 = zip_u32_at(bytes, local_header_offset + 14).map_err(ZipPayloadError::Malformed)?;
    let compressed_size_32 =
        zip_u32_at(bytes, local_header_offset + 18).map_err(ZipPayloadError::Malformed)?;
    let uncompressed_size_32 =
        zip_u32_at(bytes, local_header_offset + 22).map_err(ZipPayloadError::Malformed)?;
    let name_len =
        zip_u16_at(bytes, local_header_offset + 26).map_err(ZipPayloadError::Malformed)? as usize;
    let extra_len =
        zip_u16_at(bytes, local_header_offset + 28).map_err(ZipPayloadError::Malformed)? as usize;
    let data_start = fixed_header_end
        .checked_add(name_len)
        .and_then(|offset| offset.checked_add(extra_len))
        .ok_or_else(|| ZipPayloadError::Malformed("zip data offset overflowed".to_string()))?;
    if data_start > bytes.len() {
        return Err(ZipPayloadError::Malformed(
            "zip local header name or extra field points outside the archive".to_string(),
        ));
    }
    let name_start = fixed_header_end;
    let name_end = name_start.checked_add(name_len).ok_or_else(|| {
        ZipPayloadError::Malformed("zip local name length overflowed".to_string())
    })?;
    let name_bytes = &bytes[name_start..name_end];
    let local_extra = &bytes[name_end..data_start];
    let name_is_utf8 = std::str::from_utf8(name_bytes).is_ok();
    let local_zip64_needs = Zip64FieldNeeds {
        uncompressed_size: uncompressed_size_32 == u32::MAX,
        compressed_size: compressed_size_32 == u32::MAX,
        local_header_offset: false,
    };
    let local_zip64 = zip64_extended_info(local_extra, local_zip64_needs).map_err(|error| {
        ZipPayloadError::Malformed(format!("zip local extra field is malformed: {error}"))
    })?;
    let compressed_size = zip_usize(
        zip64_resolved_u32(
            compressed_size_32,
            local_zip64.as_ref().and_then(|zip64| zip64.compressed_size),
            "compressed size",
        )
        .map_err(ZipPayloadError::Malformed)?,
        "zip local compressed size",
    )
    .map_err(ZipPayloadError::Malformed)?;
    let uncompressed_size = zip_usize(
        zip64_resolved_u32(
            uncompressed_size_32,
            local_zip64
                .as_ref()
                .and_then(|zip64| zip64.uncompressed_size),
            "uncompressed size",
        )
        .map_err(ZipPayloadError::Malformed)?,
        "zip local uncompressed size",
    )
    .map_err(ZipPayloadError::Malformed)?;
    let name = String::from_utf8_lossy(name_bytes).to_string();
    let unicode_name = zip_unicode_path_extra(local_extra, name_bytes).map_err(|error| {
        ZipPayloadError::Malformed(format!("zip local extra field is malformed: {error}"))
    })?;
    Ok(ZipLocalHeader {
        name,
        unicode_name,
        name_is_utf8,
        extra: local_extra.to_vec(),
        zip64_needed: local_zip64_needs.any(),
        data_start,
        flags,
        compression_method,
        crc32,
        compressed_size,
        uncompressed_size,
    })
}

pub(crate) fn zip_member_name_candidates(
    central_name: String,
    central_unicode_name: Option<String>,
    local_name: String,
    local_unicode_name: Option<String>,
) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique_string(&mut candidates, Some(central_name));
    push_unique_string(&mut candidates, central_unicode_name);
    push_unique_string(&mut candidates, Some(local_name));
    push_unique_string(&mut candidates, local_unicode_name);
    candidates
}

pub(crate) fn zip_member_requires_payload_inspection(names: &[String]) -> bool {
    names
        .iter()
        .any(|name| is_executable_or_script_member_name(name))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn verify_zip_entry_payload_integrity(
    artifact: &str,
    member: &str,
    bytes: &[u8],
    data_start: usize,
    compressed_size: usize,
    compression_method: u16,
    expected_crc32: u32,
    expected_uncompressed_size: usize,
    issues: &mut Vec<serde_json::Value>,
) {
    match zip_entry_payload(
        bytes,
        data_start,
        compressed_size,
        compression_method,
        expected_crc32,
        expected_uncompressed_size,
    ) {
        Ok(_) => {}
        Err(ZipPayloadError::UnsupportedCompression(method)) => {
            issues.push(serde_json::json!({
                "code": "zip_entry_payload_uninspectable",
                "path": artifact,
                "member": member,
                "compression_method": method,
                "detail": "zip entry uses unsupported compression; package-audit cannot validate payload integrity and fails closed"
            }));
        }
        Err(ZipPayloadError::TooLarge) => {
            issues.push(serde_json::json!({
                "code": "zip_entry_payload_uninspectable",
                "path": artifact,
                "member": member,
                "detail": "zip entry payload expands beyond the package-audit payload limit; package-audit fails closed"
            }));
        }
        Err(
            error @ (ZipPayloadError::CrcMismatch { .. }
            | ZipPayloadError::SizeMismatch { .. }
            | ZipPayloadError::Malformed(_)),
        ) => {
            issues.push(serde_json::json!({
                "code": "zip_entry_payload_integrity_mismatch",
                "path": artifact,
                "member": member,
                "detail": zip_payload_error_detail(error)
            }));
        }
    }
}

pub(crate) fn audit_zip_regular_file_payload(
    artifact: &str,
    names: &[String],
    payload: &[u8],
    issues: &mut Vec<serde_json::Value>,
) {
    for name in names {
        audit_archive_executable_payload(artifact, name, payload, issues);
    }
}

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
    if matches!(
        leaf,
        "tokenzero" | "tokenzero.exe" | "tokenzero.cmd" | "tokenzero.js"
    ) || leaf.starts_with("tokenzero-runtime-")
    {
        return true;
    }
    if parts.contains(&"bin") && !leaf.contains('.') {
        return true;
    }
    matches!(
        Path::new(leaf)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some(
            "bat"
                | "cmd"
                | "com"
                | "cjs"
                | "dll"
                | "dylib"
                | "exe"
                | "fish"
                | "jar"
                | "js"
                | "mjs"
                | "node"
                | "php"
                | "pl"
                | "ps1"
                | "psm1"
                | "py"
                | "rb"
                | "sh"
                | "so"
                | "wasm"
                | "zsh"
        )
    )
}

#[derive(Clone, Copy)]
pub(crate) struct Zip64FieldNeeds {
    pub(crate) uncompressed_size: bool,
    pub(crate) compressed_size: bool,
    pub(crate) local_header_offset: bool,
}

impl Zip64FieldNeeds {
    fn any(self) -> bool {
        self.uncompressed_size || self.compressed_size || self.local_header_offset
    }
}

#[derive(Default)]
pub(crate) struct Zip64ExtendedInfo {
    pub(crate) uncompressed_size: Option<u64>,
    pub(crate) compressed_size: Option<u64>,
    pub(crate) local_header_offset: Option<u64>,
}

pub(crate) fn zip64_extended_info(
    extra: &[u8],
    needs: Zip64FieldNeeds,
) -> Result<Option<Zip64ExtendedInfo>, String> {
    validate_zip_extra_field_uniqueness(extra)?;
    let mut offset = 0usize;
    let mut info = None;
    while offset < extra.len() {
        let header_end = offset
            .checked_add(4)
            .ok_or_else(|| "zip extra field header offset overflowed".to_string())?;
        if header_end > extra.len() {
            return Err("zip extra field header is truncated".to_string());
        }
        let tag = u16::from_le_bytes([extra[offset], extra[offset + 1]]);
        let size = u16::from_le_bytes([extra[offset + 2], extra[offset + 3]]) as usize;
        let data_end = header_end
            .checked_add(size)
            .ok_or_else(|| "zip extra field size overflowed".to_string())?;
        if data_end > extra.len() {
            return Err("zip extra field data points outside the extra block".to_string());
        }
        if tag == ZIP64_EXTENDED_INFORMATION_EXTRA {
            if info.is_some() {
                return Err("zip64 extended information extra field is duplicated".to_string());
            }
            info = Some(parse_zip64_extended_info(
                &extra[header_end..data_end],
                needs,
            )?);
        }
        offset = data_end;
    }
    if needs.any() && info.is_none() {
        return Err("zip64 extended information extra field is missing".to_string());
    }
    Ok(info)
}

pub(crate) fn parse_zip64_extended_info(
    data: &[u8],
    needs: Zip64FieldNeeds,
) -> Result<Zip64ExtendedInfo, String> {
    let mut info = Zip64ExtendedInfo::default();
    let mut offset = 0usize;
    if needs.uncompressed_size {
        info.uncompressed_size = Some(zip_u64_from_extra(data, &mut offset, "uncompressed size")?);
    }
    if needs.compressed_size {
        info.compressed_size = Some(zip_u64_from_extra(data, &mut offset, "compressed size")?);
    }
    if needs.local_header_offset {
        info.local_header_offset = Some(zip_u64_from_extra(
            data,
            &mut offset,
            "local header offset",
        )?);
    }
    if needs.any() && offset != data.len() {
        return Err(format!(
            "zip64 extended information contains {} unclaimed bytes after fields required by 32-bit sentinels",
            data.len() - offset
        ));
    }
    Ok(info)
}

pub(crate) fn validate_zip_extra_field_uniqueness(extra: &[u8]) -> Result<(), String> {
    let mut offset = 0usize;
    let mut seen = HashSet::new();
    while offset < extra.len() {
        let header_end = offset
            .checked_add(4)
            .ok_or_else(|| "zip extra field header offset overflowed".to_string())?;
        if header_end > extra.len() {
            return Err("zip extra field header is truncated".to_string());
        }
        let tag = u16::from_le_bytes([extra[offset], extra[offset + 1]]);
        let size = u16::from_le_bytes([extra[offset + 2], extra[offset + 3]]) as usize;
        let data_end = header_end
            .checked_add(size)
            .ok_or_else(|| "zip extra field size overflowed".to_string())?;
        if data_end > extra.len() {
            return Err("zip extra field data points outside the extra block".to_string());
        }
        if !seen.insert(tag) {
            return Err(format!("zip extra field 0x{tag:04x} is duplicated"));
        }
        offset = data_end;
    }
    Ok(())
}

pub(crate) struct ZipExtraField {
    pub(crate) tag: u16,
    pub(crate) size: usize,
}

pub(crate) fn audit_zip_extra_fields(
    artifact: &str,
    member: &str,
    field_location: &str,
    extra: &[u8],
    zip64_needed: bool,
    issues: &mut Vec<serde_json::Value>,
) -> Result<(), String> {
    for field in zip_extra_fields(extra)? {
        match field.tag {
            ZIP64_EXTENDED_INFORMATION_EXTRA if zip64_needed => {}
            0x7075 => {}
            ZIP64_EXTENDED_INFORMATION_EXTRA => {
                issues.push(serde_json::json!({
                    "code": "zip_extra_field_present",
                    "path": artifact,
                    "member": member,
                    "field_location": field_location,
                    "tag": format!("0x{:04x}", field.tag),
                    "size": field.size,
                    "detail": "zip64 extra field is present without a required 32-bit size or offset sentinel; package-audit fails closed without exposing field values"
                }));
            }
            _ => {
                issues.push(serde_json::json!({
                    "code": "zip_extra_field_present",
                    "path": artifact,
                    "member": member,
                    "field_location": field_location,
                    "tag": format!("0x{:04x}", field.tag),
                    "size": field.size,
                    "detail": "zip extra field carries unsupported public metadata; package-audit fails closed without exposing field values"
                }));
            }
        }
    }
    Ok(())
}

pub(crate) fn zip_extra_fields(extra: &[u8]) -> Result<Vec<ZipExtraField>, String> {
    let mut offset = 0usize;
    let mut fields = Vec::new();
    while offset < extra.len() {
        let header_end = offset
            .checked_add(4)
            .ok_or_else(|| "zip extra field header offset overflowed".to_string())?;
        if header_end > extra.len() {
            return Err("zip extra field header is truncated".to_string());
        }
        let tag = u16::from_le_bytes([extra[offset], extra[offset + 1]]);
        let size = u16::from_le_bytes([extra[offset + 2], extra[offset + 3]]) as usize;
        let data_end = header_end
            .checked_add(size)
            .ok_or_else(|| "zip extra field size overflowed".to_string())?;
        if data_end > extra.len() {
            return Err("zip extra field data points outside the extra block".to_string());
        }
        fields.push(ZipExtraField { tag, size });
        offset = data_end;
    }
    Ok(fields)
}

pub(crate) fn zip_u64_from_extra(
    data: &[u8],
    offset: &mut usize,
    field: &str,
) -> Result<u64, String> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| format!("zip64 {field} offset overflowed"))?;
    let value = data
        .get(*offset..end)
        .ok_or_else(|| format!("zip64 extended information is missing {field}"))?;
    *offset = end;
    Ok(u64::from_le_bytes([
        value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
    ]))
}

pub(crate) fn zip64_resolved_u32(
    value: u32,
    zip64_value: Option<u64>,
    field: &str,
) -> Result<u64, String> {
    if value == u32::MAX {
        zip64_value.ok_or_else(|| {
            format!(
                "zip64 extended information is missing required {field}; package-audit fails closed"
            )
        })
    } else {
        Ok(value as u64)
    }
}

pub(crate) fn zip_unicode_path_extra(
    extra: &[u8],
    header_name: &[u8],
) -> Result<Option<String>, String> {
    let mut offset = 0usize;
    let mut unicode_name = None;
    while offset < extra.len() {
        let header_end = offset
            .checked_add(4)
            .ok_or_else(|| "zip extra field header offset overflowed".to_string())?;
        if header_end > extra.len() {
            return Err("zip extra field header is truncated".to_string());
        }
        let tag = u16::from_le_bytes([extra[offset], extra[offset + 1]]);
        let size = u16::from_le_bytes([extra[offset + 2], extra[offset + 3]]) as usize;
        let data_end = header_end
            .checked_add(size)
            .ok_or_else(|| "zip extra field size overflowed".to_string())?;
        if data_end > extra.len() {
            return Err("zip extra field data points outside the extra block".to_string());
        }
        if tag == 0x7075 {
            if unicode_name.is_some() {
                return Err("zip unicode path extra field is duplicated".to_string());
            }
            unicode_name = Some(parse_zip_unicode_path_extra(
                &extra[header_end..data_end],
                header_name,
            )?);
        }
        offset = data_end;
    }
    Ok(unicode_name)
}

pub(crate) fn parse_zip_unicode_path_extra(
    data: &[u8],
    header_name: &[u8],
) -> Result<String, String> {
    if data.len() < 5 {
        return Err("zip unicode path extra field is missing CRC metadata".to_string());
    }
    if data[0] != 1 {
        return Err(format!(
            "zip unicode path extra field has unsupported version {}",
            data[0]
        ));
    }
    if data.len() == 5 {
        return Err("zip unicode path extra field is missing a unicode name".to_string());
    }
    let expected_crc = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
    let actual_crc = zip_crc32(header_name);
    if expected_crc != actual_crc {
        return Err("zip unicode path extra field CRC does not match the header name".to_string());
    }
    let name = std::str::from_utf8(&data[5..])
        .map_err(|_| "zip unicode path extra field name is not valid UTF-8".to_string())?;
    if name.is_empty() {
        return Err("zip unicode path extra field name is empty".to_string());
    }
    Ok(name.to_string())
}

pub(crate) fn zip_crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            if crc & 1 == 1 {
                crc = (crc >> 1) ^ 0xedb8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

pub(crate) fn zip_entry_payload(
    bytes: &[u8],
    data_start: usize,
    compressed_size: usize,
    compression_method: u16,
    expected_crc32: u32,
    expected_uncompressed_size: usize,
) -> Result<Cow<'_, [u8]>, ZipPayloadError> {
    let data_end = data_start
        .checked_add(compressed_size)
        .ok_or_else(|| ZipPayloadError::Malformed("zip data size overflowed".to_string()))?;
    if data_end > bytes.len() {
        return Err(ZipPayloadError::Malformed(
            "zip entry payload points outside the archive".to_string(),
        ));
    }
    let payload = &bytes[data_start..data_end];
    let decoded = match compression_method {
        0 => Ok(Cow::Borrowed(payload)),
        8 => deflate_decompress_bytes(payload)
            .map(Cow::Owned)
            .map_err(|error| match error {
                ArchivePayloadError::TooLarge => ZipPayloadError::TooLarge,
                ArchivePayloadError::Malformed(error) => ZipPayloadError::Malformed(format!(
                    "zip deflate payload could not be decoded: {error}"
                )),
            }),
        method => Err(ZipPayloadError::UnsupportedCompression(method)),
    }?;
    let actual_crc32 = zip_crc32(decoded.as_ref());
    if actual_crc32 != expected_crc32 {
        return Err(ZipPayloadError::CrcMismatch {
            expected: expected_crc32,
            actual: actual_crc32,
        });
    }
    if decoded.len() != expected_uncompressed_size {
        return Err(ZipPayloadError::SizeMismatch {
            expected: expected_uncompressed_size,
            actual: decoded.len(),
        });
    }
    Ok(decoded)
}

pub(crate) fn zip_data_descriptor_matches(
    bytes: &[u8],
    data_start: usize,
    compressed_size: usize,
    expected_crc32: u32,
    expected_compressed_size: usize,
    expected_uncompressed_size: usize,
    descriptor_limit: usize,
) -> Result<usize, String> {
    let descriptor_offset = data_start
        .checked_add(compressed_size)
        .ok_or_else(|| "zip data descriptor offset overflowed".to_string())?;
    if descriptor_offset >= descriptor_limit {
        return Err(
            "zip data descriptor is missing before the central directory; package-audit fails closed"
                .to_string(),
        );
    }
    let expected_compressed_size = expected_compressed_size as u64;
    let expected_uncompressed_size = expected_uncompressed_size as u64;
    let expected = (
        expected_crc32,
        expected_compressed_size,
        expected_uncompressed_size,
    );
    let mut candidates = Vec::new();

    if let Some(fields) = zip_data_descriptor_fields_32(bytes, descriptor_offset, descriptor_limit)
    {
        if fields == expected {
            return Ok(12);
        }
        candidates.push(format!(
            "unsigned descriptor CRC {:08x}, compressed_size {}, uncompressed_size {}",
            fields.0, fields.1, fields.2
        ));
    }
    if let Some(fields) = zip_data_descriptor_fields_64(bytes, descriptor_offset, descriptor_limit)
    {
        if fields == expected {
            return Ok(20);
        }
        candidates.push(format!(
            "unsigned zip64 descriptor CRC {:08x}, compressed_size {}, uncompressed_size {}",
            fields.0, fields.1, fields.2
        ));
    }

    if zip_data_descriptor_has_signature(bytes, descriptor_offset, descriptor_limit)
        && let Some(signed_offset) = descriptor_offset.checked_add(4)
    {
        if let Some(fields) = zip_data_descriptor_fields_32(bytes, signed_offset, descriptor_limit)
        {
            if fields == expected {
                return Ok(16);
            }
            candidates.push(format!(
                "signed descriptor CRC {:08x}, compressed_size {}, uncompressed_size {}",
                fields.0, fields.1, fields.2
            ));
        }
        if let Some(fields) = zip_data_descriptor_fields_64(bytes, signed_offset, descriptor_limit)
        {
            if fields == expected {
                return Ok(24);
            }
            candidates.push(format!(
                "signed zip64 descriptor CRC {:08x}, compressed_size {}, uncompressed_size {}",
                fields.0, fields.1, fields.2
            ));
        }
    }

    let observed = if candidates.is_empty() {
        "no complete 32-bit or zip64 descriptor was found before the central directory".to_string()
    } else {
        candidates.join("; ")
    };
    Err(format!(
        "zip data descriptor does not match central-directory metadata; expected CRC {expected_crc32:08x}, compressed_size {expected_compressed_size}, uncompressed_size {expected_uncompressed_size}; {observed}; package-audit fails closed"
    ))
}

pub(crate) fn zip_data_descriptor_fields_32(
    bytes: &[u8],
    offset: usize,
    limit: usize,
) -> Option<(u32, u64, u64)> {
    let crc32 = zip_descriptor_range(bytes, offset, 4, limit)?;
    let compressed_size = zip_descriptor_range(bytes, offset.checked_add(4)?, 4, limit)?;
    let uncompressed_size = zip_descriptor_range(bytes, offset.checked_add(8)?, 4, limit)?;
    Some((
        u32::from_le_bytes([crc32[0], crc32[1], crc32[2], crc32[3]]),
        u32::from_le_bytes([
            compressed_size[0],
            compressed_size[1],
            compressed_size[2],
            compressed_size[3],
        ]) as u64,
        u32::from_le_bytes([
            uncompressed_size[0],
            uncompressed_size[1],
            uncompressed_size[2],
            uncompressed_size[3],
        ]) as u64,
    ))
}

pub(crate) fn zip_data_descriptor_fields_64(
    bytes: &[u8],
    offset: usize,
    limit: usize,
) -> Option<(u32, u64, u64)> {
    let crc32 = zip_descriptor_range(bytes, offset, 4, limit)?;
    let compressed_size = zip_descriptor_range(bytes, offset.checked_add(4)?, 8, limit)?;
    let uncompressed_size = zip_descriptor_range(bytes, offset.checked_add(12)?, 8, limit)?;
    Some((
        u32::from_le_bytes([crc32[0], crc32[1], crc32[2], crc32[3]]),
        u64::from_le_bytes([
            compressed_size[0],
            compressed_size[1],
            compressed_size[2],
            compressed_size[3],
            compressed_size[4],
            compressed_size[5],
            compressed_size[6],
            compressed_size[7],
        ]),
        u64::from_le_bytes([
            uncompressed_size[0],
            uncompressed_size[1],
            uncompressed_size[2],
            uncompressed_size[3],
            uncompressed_size[4],
            uncompressed_size[5],
            uncompressed_size[6],
            uncompressed_size[7],
        ]),
    ))
}

pub(crate) fn zip_data_descriptor_has_signature(bytes: &[u8], offset: usize, limit: usize) -> bool {
    zip_descriptor_range(bytes, offset, 4, limit)
        == Some(&ZIP_DATA_DESCRIPTOR_SIGNATURE.to_le_bytes())
}

pub(crate) fn zip_descriptor_range(
    bytes: &[u8],
    offset: usize,
    len: usize,
    limit: usize,
) -> Option<&[u8]> {
    let end = offset.checked_add(len)?;
    if end > limit {
        return None;
    }
    bytes.get(offset..end)
}

pub(crate) fn zip_payload_error_detail(error: ZipPayloadError) -> String {
    match error {
        ZipPayloadError::TooLarge => {
            "zip payload expands beyond the package-audit payload limit".to_string()
        }
        ZipPayloadError::UnsupportedCompression(method) => {
            format!("zip payload uses unsupported compression method {method}")
        }
        ZipPayloadError::CrcMismatch { expected, actual } => format!(
            "zip payload CRC mismatch (expected {expected:08x}, actual {actual:08x}); package-audit fails closed"
        ),
        ZipPayloadError::SizeMismatch { expected, actual } => format!(
            "zip payload uncompressed size mismatch (expected {expected}, actual {actual}); package-audit fails closed"
        ),
        ZipPayloadError::Malformed(detail) => detail,
    }
}

pub(crate) fn zip_array_at<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], String> {
    let end = offset
        .checked_add(N)
        .ok_or_else(|| "zip field offset overflowed".to_string())?;
    let data = bytes
        .get(offset..end)
        .ok_or_else(|| "zip field points outside the archive".to_string())?;
    let mut field = [0; N];
    field.copy_from_slice(data);
    Ok(field)
}

pub(crate) fn zip_u16_at(bytes: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(zip_array_at(bytes, offset)?))
}

pub(crate) fn zip_u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(zip_array_at(bytes, offset)?))
}

pub(crate) fn zip_u64_at(bytes: &[u8], offset: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(zip_array_at(bytes, offset)?))
}

pub(crate) fn zip_usize(value: u64, field: &str) -> Result<usize, String> {
    usize::try_from(value).map_err(|_| format!("{field} is too large for this host"))
}
