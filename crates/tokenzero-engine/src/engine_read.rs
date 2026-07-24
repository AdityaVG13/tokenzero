use super::*;

impl TokenZeroEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn read(
        &self,
        paths: &[PathBuf],
        mode: Mode,
        start_line: Option<usize>,
        end_line: Option<usize>,
        raw: bool,
        max_files: usize,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        self.read_with_options(
            paths,
            mode,
            start_line,
            end_line,
            raw,
            max_files,
            max_visible_tokens,
            ServeOptions::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn read_with_options(
        &self,
        paths: &[PathBuf],
        mode: Mode,
        start_line: Option<usize>,
        end_line: Option<usize>,
        raw: bool,
        max_files: usize,
        max_visible_tokens: usize,
        options: ServeOptions,
    ) -> ToolResponse {
        let response = self.read_with_options_inner(
            paths,
            mode,
            start_line,
            end_line,
            raw,
            max_files,
            max_visible_tokens,
            options,
        );
        let ok = response.error.is_none();
        let code = response.error.as_ref().map(|err| err.code.as_str());
        self.surface_health().record_read_outcome(ok, code);
        response
    }

    #[allow(clippy::too_many_arguments)]
    fn read_with_options_inner(
        &self,
        paths: &[PathBuf],
        mode: Mode,
        start_line: Option<usize>,
        end_line: Option<usize>,
        raw: bool,
        max_files: usize,
        max_visible_tokens: usize,
        options: ServeOptions,
    ) -> ToolResponse {
        // Single-flight the serve so a second pipelined identical read waits
        // for this one to record its serve before it looks up the seen-set
        // (otherwise both miss and both serve full). Keyed per path+range, so
        // disjoint reads still run fully concurrently. Held until after
        // session_apply via the guard's lifetime.
        let _flight = if self.config.session_dedup {
            let keys = paths
                .iter()
                .take(max_files)
                .map(|path| ServeKey::File {
                    path: comparable_path(path),
                    start: start_line,
                    end: end_line,
                })
                .collect();
            self.begin_serve_flight(keys)
        } else {
            self.begin_serve_flight(Vec::new())
        };
        let mut store = self.recovery_store();
        let mut visible_parts = Vec::new();
        let mut raw_visible_parts = Vec::new();
        let mut refs = Vec::new();
        let mut raw_tokens = 0usize;
        let mut visible_tokens = 0usize;
        let mut storage_errors = Vec::new();
        let mut content_types = Vec::new();
        let mut bytes_read = 0usize;
        let mut summary = SessionSummary::default();
        let mut working_set_anchor = None;
        // Serve records are applied only after every path succeeded: an
        // error response serves nothing, so nothing may be marked as seen.
        let mut pending: Vec<(ServeKey, ServedRecord)> = Vec::new();
        // Dedup/diff substitutions are buffered and applied only after this
        // call's refs persist: a note replaces content with refs, which is
        // only safe when the refs are actually recoverable.
        let mut substitutions: Vec<PendingSubstitution> = Vec::new();
        for path in paths.iter().take(max_files) {
            if !self.path_allowed(path) {
                return path_not_allowed("read", path);
            }
            let source_start = start_line;
            let source_end = end_line;
            let text_result = if let Some(start) = start_line {
                read_line_range_from_file(path, start, end_line.unwrap_or(start))
            } else {
                fs::read_to_string(path)
            };
            let mut text = match text_result {
                Ok(text) => text,
                Err(err) => {
                    // "could not read X (read_failed)" with no cause stranded
                    // live sessions guessing between missing file, directory,
                    // and permissions. Name the reason and the obvious next op.
                    let hint = if path.is_dir() {
                        " (path is a directory - use tree)"
                    } else if !path.exists() {
                        " (no such file)"
                    } else {
                        ""
                    };
                    return ToolResponse::error(
                        "read",
                        "read_failed",
                        format!("could not read {}: {err}{hint}", path.display()),
                        None,
                    );
                }
            };
            bytes_read += text.len();
            let line_count = text.lines().count();
            if paths.len() == 1 {
                let anchor_start = source_start.unwrap_or(1);
                let anchor_end = source_end
                    .unwrap_or_else(|| anchor_start.saturating_add(line_count.saturating_sub(1)));
                working_set_anchor = Some(tokenzero_recovery::working_set::SpanAnchor {
                    path: path.clone(),
                    symbol: None,
                    start_line: anchor_start,
                    end_line: anchor_end,
                });
            }
            let ctype = detect_content_type(&text, Some(path));
            content_types.push(ctype);
            let stored = if paths.len() == 1
                && source_start.is_none()
                && source_end.is_none()
                && text.len() >= 64 * 1024
            {
                store.store_source_backed_payload_deferred_batch(&text, ctype, path)
            } else {
                store.store_payload_deferred_batch(
                    &text,
                    ctype,
                    Some(path),
                    source_start,
                    source_end,
                )
            };
            refs.push(ref_record("blob", stored.blob_ref.clone(), text.len()));
            refs.push(ref_record("file", stored.file_ref.clone(), text.len()));
            let capsule = if raw {
                tokenzero_core::Capsule {
                    text: text.trim_end().to_string(),
                    raw_tokens: stored.raw_tokens,
                    visible_tokens: stored.raw_tokens,
                    omitted_lines: 0,
                    mode,
                    protected_anchors: Vec::new(),
                    exact_refs: Vec::new(),
                    lossy_spans: Vec::new(),
                    lossy_policy_id: None,
                }
            } else {
                tokenzero_core::make_capsule_with_recovery_ref(
                    &text,
                    stored.raw_tokens,
                    mode,
                    max_visible_tokens,
                    Some(&path.display().to_string()),
                    Some(&stored.file_ref),
                )
            };
            let part_text = capsule.text;
            let part_tokens = capsule.visible_tokens;
            // Session redundancy layer (docs/routing.md §5). Zero-payload
            // notes are cheap and stay untouched: empty payloads skip the
            // layer entirely (notes are never deduped).
            if self.config.session_dedup && !text.is_empty() {
                let key = ServeKey::File {
                    path: comparable_path(path),
                    start: source_start,
                    end: source_end,
                };
                let content_sha256 = stored
                    .blob_ref
                    .strip_prefix("tz://blob/")
                    .filter(|digest| digest.len() == 64)
                    .map(str::to_owned)
                    .unwrap_or_else(|| sha256_hex(&text));
                // raw keeps the verbatim-slice contract, passthrough keeps
                // its verbatim-payload contract, and fresh is the per-call
                // opt-out; all three bypass the replacement render but still
                // record the serve below so later calls can dedup.
                let bypass = raw || matches!(mode, Mode::Passthrough) || options.fresh;
                match self.session_lookup(&key, &content_sha256) {
                    SeenState::Unchanged {
                        serve_count,
                        cross_session,
                    } if !bypass => {
                        let note = unchanged_read_note(path, &text, &stored);
                        let note_tokens = count_tokens(&note);
                        // ROI guard: a note that costs as much as the full
                        // render is never emitted.
                        if note_tokens < part_tokens {
                            substitutions.push(PendingSubstitution::Dedup {
                                idx: visible_parts.len(),
                                note,
                                note_tokens,
                                full_tokens: part_tokens,
                                serve_count: serve_count + 1,
                                cross_session,
                            });
                        }
                    }
                    SeenState::Changed { previous } if !bypass && self.config.diff_reads => {
                        if let Some((diff_text, diff_tokens, telemetry)) = diff_since_served(
                            &mut store,
                            path,
                            &text,
                            &previous,
                            &stored,
                            part_tokens,
                        ) {
                            substitutions.push(PendingSubstitution::Diff {
                                idx: visible_parts.len(),
                                text: diff_text,
                                diff_tokens,
                                full_tokens: part_tokens,
                                telemetry,
                            });
                        }
                    }
                    _ => {}
                }
                pending.push((
                    key,
                    served_record_with_metadata(content_sha256, text.len(), line_count, &stored),
                ));
            }
            raw_tokens += capsule.raw_tokens;
            visible_tokens += part_tokens;
            if !raw {
                let trimmed_len = text.trim_end().len();
                text.truncate(trimmed_len);
                raw_visible_parts.push(text);
            }
            visible_parts.push(part_text);
        }
        let persisted = persist_refs(&mut store, &mut refs);
        if let Some(error) = persisted.error {
            storage_errors.push(error);
        }
        let refs_complete = persisted.refs_complete;
        if !refs_complete && !raw {
            visible_parts = raw_visible_parts;
            visible_tokens = raw_tokens;
        }
        let full_bytes = joined_bytes(&visible_parts);
        // Dedup/diff notes advertise refs in place of content: apply them
        // only when persistence succeeded AND every ref survived eviction.
        // Degraded storage always serves full — the bytes are in the text,
        // which is unconditionally safe.
        if storage_errors.is_empty() && refs_complete {
            for substitution in substitutions {
                match substitution {
                    PendingSubstitution::Dedup {
                        idx,
                        note,
                        note_tokens,
                        full_tokens,
                        serve_count,
                        cross_session,
                    } => {
                        summary.note_dedup(serve_count, full_tokens - note_tokens, cross_session);
                        visible_tokens -= full_tokens - note_tokens;
                        visible_parts[idx] = note;
                    }
                    PendingSubstitution::Diff {
                        idx,
                        text,
                        diff_tokens,
                        full_tokens,
                        telemetry,
                    } => {
                        summary.note_diff(telemetry, full_tokens - diff_tokens);
                        visible_tokens -= full_tokens - diff_tokens;
                        visible_parts[idx] = text;
                    }
                }
            }
        }
        if self.config.session_dedup {
            let delta_bytes = joined_bytes(&visible_parts);
            summary.note_wire_bytes(full_bytes, delta_bytes);
        }
        let exact_refs_available = !refs.is_empty();
        let exact_ref_tokens = exact_ref_token_count(&refs);
        let mut response = success_response(
            "read",
            mode,
            visible_parts.join("\n\n"),
            refs,
            (
                raw_tokens,
                visible_tokens,
                store.recovery_tokens,
                Some(exact_ref_tokens),
            ),
        );
        response.content_type = Some(common_content_type(&content_types).to_string());
        if !storage_errors.is_empty() {
            response.diagnostic = Some(cache_write_diagnostic(
                "could not persist recovery cache for one or more read paths",
            ));
            response.telemetry = Some(json!({
                "transport_status": "degraded",
                "degraded": true,
                "storage_errors": storage_errors,
                "exact_refs_available": exact_refs_available
            }));
        }
        let working_set_replaced = !raw
            && !matches!(mode, Mode::Passthrough)
            && working_set_anchor.is_some_and(|anchor| {
                self.admit_working_set_response(&mut store, &mut response, anchor)
            });
        // A serve whose refs failed to persist, or whose visible bytes were
        // replaced by working-set eviction, must not become a dedup base.
        if storage_errors.is_empty() && refs_complete && !working_set_replaced {
            let (from_hwm, to_hwm) = self.session_apply(pending, &summary);
            summary.set_watermark(from_hwm, to_hwm);
        }
        // Merge — never overwrite — so degraded-storage markers survive a
        // dedup/diff serve in the same response.
        if let Some(extra) = summary.telemetry() {
            merge_telemetry(&mut response, extra);
        }
        // Raw reads keep the verbatim slice contract even when it is empty;
        // raw=true does not imply Mode::Passthrough, so guard it explicitly.
        if !raw && bytes_read == 0 {
            let label = zero_hit_label(
                &paths
                    .iter()
                    .take(max_files)
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            apply_zero_hit_note(&mut response, mode, format!("# read {label} — 0 bytes"));
        }
        response
    }
}
