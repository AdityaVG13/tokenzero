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
        // Bind caller-supplied paths to the configured root so relative
        // arguments target `call_root`, never the process working directory.
        let path = self.resolve_call_path(path);
        if !self.path_allowed(&path) {
            return path_not_allowed("edit", &path);
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
            let bytes = match fs::read(&path) {
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
            detect_content_type(&old_text, Some(&path)),
            Some(&path),
            None,
            None,
        );
        let post_stored = store.store_payload_deferred(
            &applied.text,
            detect_content_type(&applied.text, Some(&path)),
            Some(&path),
            None,
            None,
        );
        let mut refs = Vec::with_capacity(3);
        push_payload_refs(&mut refs, &post_stored, applied.text.len());
        refs.push(ref_record("undo", pre_stored.blob_ref, old_text.len()));
        let persisted = persist_refs(&mut store, &mut refs);
        let refs_complete = persisted.refs_complete;
        let storage_error = persisted.error;
        // 1zpq (F-ENG-005): never mutate with a dead undo. When the recovery
        // pre/post images did not persist completely, fail closed before any
        // byte reaches disk instead of writing and only attaching a degraded
        // diagnostic. Dry-run stays non-mutating and reports the degradation
        // truthfully below.
        if !dry_run && (storage_error.is_some() || !refs_complete) {
            return failure_response(
                "edit",
                "edit_recovery_unavailable",
                format!(
                    "edit blocked before writing {}: recovery pre/post images could not be persisted ({}) so undo would be lost; the file was not modified",
                    path.display(),
                    storage_error.unwrap_or_else(|| "refs incomplete after pruning".to_string()),
                ),
                Some("choose a writable recovery cache file with --cache-path <file>, or free disk space, then retry the edit"),
            );
        }
        if !dry_run {
            if let Err(err) = write_atomic(&path, applied.text.as_bytes()) {
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
                            path: comparable_path(&path),
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
        let capsule = match recoverable_capsule(
            &assembled,
            &assembled,
            assembled_tokens,
            mode,
            max_visible_tokens,
            &format!("edit {}", path.display()),
            None,
            refs_complete,
        ) {
            Ok(capsule) => capsule,
            Err(error) => return capsule_error_response("edit", error),
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Engine whose recovery store is forced to fail: the store's parent
    /// directory is blocked by a regular file, so `persist()` errors at
    /// `create_dir_all` before any snapshot or journal write can succeed.
    /// (load_state swallows parse errors, so a non-JSON file at the cache
    /// path itself would not trip the store.)
    fn engine_with_blocked_cache(dir: &std::path::Path) -> TokenZeroEngine {
        fs::write(dir.join("cache-blocker"), b"not a directory").unwrap();
        let mut config = EngineConfig::for_root(dir);
        config.cache_path = dir.join("cache-blocker").join("recovery-cache.json");
        config.session_dedup = false;
        config.diff_reads = false;
        config.fetch_enabled = false;
        TokenZeroEngine::new(config)
    }

    #[test]
    fn edit_fails_closed_when_recovery_persist_fails_and_file_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("target.txt");
        fs::write(&path, "original-line\n").unwrap();
        let engine = engine_with_blocked_cache(dir.path());

        let response = engine.edit(
            &path,
            &[EditHunk {
                find: "original-line".to_string(),
                replace: "mutated-line".to_string(),
                replace_all: false,
            }],
            false,
            false,
            Mode::Auto,
            4000,
        );

        assert_eq!(response.status, "error", "{response:?}");
        let error = response.error.as_ref().expect("typed error");
        assert_eq!(error.code, "edit_recovery_unavailable", "{response:?}");
        assert!(
            error.repair.is_some(),
            "error must carry a one-step-correctable repair: {error:?}"
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "original-line\n",
            "file must remain byte-exact when undo cannot persist"
        );
    }

    #[test]
    fn create_fails_closed_when_recovery_persist_fails_and_no_file_written() {
        let dir = tempfile::tempdir().unwrap();
        let new_path = dir.path().join("new.txt");
        let engine = engine_with_blocked_cache(dir.path());

        let response = engine.edit(
            &new_path,
            &[EditHunk {
                find: String::new(),
                replace: "brand-new\n".to_string(),
                replace_all: false,
            }],
            true,
            false,
            Mode::Auto,
            4000,
        );

        assert_eq!(response.status, "error", "{response:?}");
        assert_eq!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some("edit_recovery_unavailable"),
            "{response:?}"
        );
        assert!(
            !new_path.exists(),
            "create must not write a file when undo cannot persist"
        );
    }

    #[test]
    fn dry_run_stays_non_mutating_and_reports_degraded_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("target.txt");
        fs::write(&path, "original-line\n").unwrap();
        let engine = engine_with_blocked_cache(dir.path());

        let response = engine.edit(
            &path,
            &[EditHunk {
                find: "original-line".to_string(),
                replace: "mutated-line".to_string(),
                replace_all: false,
            }],
            false,
            true,
            Mode::Auto,
            4000,
        );

        assert_eq!(response.status, "ok", "{response:?}");
        assert!(
            response.diagnostic.is_some(),
            "dry-run must report degraded recovery truthfully: {response:?}"
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "original-line\n",
            "dry-run must never mutate"
        );
    }
}
