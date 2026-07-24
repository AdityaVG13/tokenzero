use super::*;

impl TokenZeroEngine {
    /// One-call multi-hunk read+verify+edit. Hunks apply sequentially against
    /// the evolving text and the batch is all-or-nothing: any failed hunk
    /// aborts before a single byte reaches disk. The pre-image blob ref is
    /// the undo ref.
    pub fn edit(
        &self,
        path: &Path,
        edits: &[EditHunk],
        create: bool,
        dry_run: bool,
        mode: Mode,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        if !self.path_allowed(path) {
            return path_not_allowed("edit", path);
        }
        if edits.is_empty() {
            return failure_response(
                "edit",
                "edit_failed",
                "no edit hunks provided",
                Some("pass at least one {find, replace} hunk"),
            );
        }
        if create && (edits.len() != 1 || !edits[0].find.is_empty()) {
            return failure_response(
                "edit",
                "edit_failed",
                "create=true requires exactly one hunk with an empty find",
                Some(r#"pass edits=[{"find": "", "replace": "<full new-file content>"}]"#),
            );
        }
        let old_text = if create {
            if path.exists() {
                return failure_response(
                    "edit",
                    "edit_failed",
                    format!("create=true but file already exists: {}", path.display()),
                    Some("drop create=true to edit the existing content"),
                );
            }
            String::new()
        } else {
            let bytes = match fs::read(path) {
                Ok(bytes) => bytes,
                Err(err) => {
                    return failure_response(
                        "edit",
                        "edit_failed",
                        format!("could not read {}: {err}", path.display()),
                        Some("pass create=true to create a new file"),
                    );
                }
            };
            match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => {
                    return failure_response(
                        "edit",
                        "not_utf8",
                        format!(
                            "{} is not valid UTF-8; edit only handles text files",
                            path.display()
                        ),
                        None,
                    );
                }
            }
        };
        let applied = if create {
            create_file_hunk(&edits[0])
        } else {
            apply_edit_hunks(&old_text, edits)
        };
        let applied = match applied {
            Ok(applied) => applied,
            Err(failure) => {
                return ToolResponse::error("edit", failure.code, failure.message, failure.repair);
            }
        };
        let mut store = self.recovery_store();
        // Pre-image blob is the undo ref; post-image blob/file refs recover
        // the new content. Persist before writing so undo survives the write.
        let pre_stored = store.store_payload_deferred(
            &old_text,
            detect_content_type(&old_text, Some(path)),
            Some(path),
            None,
            None,
        );
        let post_stored = store.store_payload_deferred(
            &applied.text,
            detect_content_type(&applied.text, Some(path)),
            Some(path),
            None,
            None,
        );
        let mut refs = Vec::with_capacity(3);
        push_payload_refs(&mut refs, &post_stored, applied.text.len());
        refs.push(ref_record("undo", pre_stored.blob_ref, old_text.len()));
        let persisted = persist_refs(&mut store, &mut refs);
        let refs_complete = persisted.refs_complete;
        let storage_error = persisted.error;
        if !dry_run {
            if let Err(err) = write_atomic(path, applied.text.as_bytes()) {
                return failure_response(
                    "edit",
                    "edit_failed",
                    format!("could not write {}: {err}", path.display()),
                    Some("check directory permissions"),
                );
            }
            // Seed the seen-set with the post-image so the canonical
            // read → edit → re-read flow serves an unchanged note instead of
            // re-paying the hunks as a diff. Same persistence rule as
            // read/search serves: refs that failed to persist never become a
            // dedup base.
            if storage_error.is_none()
                && refs_complete
                && self.config.session_dedup
                && !applied.text.is_empty()
            {
                self.session_apply(
                    vec![(
                        ServeKey::File {
                            path: comparable_path(path),
                            start: None,
                            end: None,
                        },
                        served_record(&applied.text, &post_stored),
                    )],
                    &SessionSummary::default(),
                );
            }
        }
        let (prefix, status) = if dry_run {
            ("dry-run: ", "would apply")
        } else {
            ("", "applied")
        };
        let header = format!(
            "# edit {} — {prefix}{} hunks {status} (+{} -{} lines)",
            path.display(),
            edits.len(),
            applied.lines_added,
            applied.lines_removed,
        );
        let assembled = if applied.diff.is_empty() {
            header
        } else {
            format!("{header}\n{}", applied.diff)
        };
        let assembled_tokens = count_tokens(&assembled);
        let capsule = recoverable_capsule(
            &assembled,
            &assembled,
            assembled_tokens,
            mode,
            max_visible_tokens,
            &format!("edit {}", path.display()),
            None,
            refs_complete,
        );
        let exact_refs_available = !refs.is_empty();
        let mut response = capsule_response!("edit", mode, capsule, refs, store.recovery_tokens);
        response.content_type = Some(ContentType::Diff.to_string());
        if !dry_run {
            response.ack = None;
            if let Some(visible) = response.visible.as_mut() {
                visible.text.clear();
            }
            if let Some(accounting) = response.accounting.as_mut() {
                accounting.visible_tokens = 0;
                accounting.billed_tokens = 0;
            }
        }
        if storage_error.is_some() {
            response.diagnostic = Some(cache_write_diagnostic(
                "could not persist recovery cache for edit pre/post images",
            ));
        }
        response.telemetry = Some(json!({
            "path": path.display().to_string(),
            "hunks": edits.len(),
            "lines_added": applied.lines_added,
            "lines_removed": applied.lines_removed,
            "create": create,
            "dry_run": dry_run,
            "transport_status": if storage_error.is_some() { "degraded" } else { "ok" },
            "degraded": storage_error.is_some(),
            "storage_error": storage_error,
            "exact_refs_available": exact_refs_available
        }));
        response
    }
}
