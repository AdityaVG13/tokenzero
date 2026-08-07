use crate::*;

pub struct PersistResult {
    pub(crate) refs_complete: bool,
    pub(crate) error: Option<String>,
}

pub(crate) enum RecoveryStoreLease<'a> {
    Shared {
        store: RecoveryStore,
        slot: &'a Mutex<Option<RecoveryStore>>,
    },
    Owned(RecoveryStore),
}

impl std::ops::Deref for RecoveryStoreLease<'_> {
    type Target = RecoveryStore;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Shared { store, .. } | Self::Owned(store) => store,
        }
    }
}

impl std::ops::DerefMut for RecoveryStoreLease<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Shared { store, .. } | Self::Owned(store) => store,
        }
    }
}

impl Drop for RecoveryStoreLease<'_> {
    fn drop(&mut self) {
        let Self::Shared { store, slot } = self else {
            return;
        };
        let mut available = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if available.is_none() {
            let placeholder = RecoveryStore::new(None);
            *available = Some(std::mem::replace(store, placeholder));
        }
    }
}

impl TokenZeroEngine {
    /// Check out the reusable long-lived store, or construct a temporary store
    /// when another request already has it. One-shot CLI commands own their store.
    pub(crate) fn recovery_store(&self) -> RecoveryStoreLease<'_> {
        match &self.recovery_store {
            Some(slot) => {
                let store = slot
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                    .unwrap_or_else(|| RecoveryStore::new(Some(self.config.cache_path.clone())));
                RecoveryStoreLease::Shared { store, slot }
            }
            None => {
                RecoveryStoreLease::Owned(RecoveryStore::new(Some(self.config.cache_path.clone())))
            }
        }
    }

    pub(crate) fn shell_output_policy(&self) -> RunOutputPolicy {
        RunOutputPolicy {
            per_stream_capture_bytes: self.config.shell_capture_bytes,
            spill_threshold_bytes: self.config.shell_spill_bytes,
            spill_dir: Some(shell_spill_dir(&self.config.cache_path)),
        }
        .normalized()
    }
}

pub fn inner_env() -> BTreeMap<String, String> {
    BTreeMap::from([("TOKENZERO_INNER".to_string(), "1".to_string())])
}

pub fn persist_refs(
    store: &mut RecoveryStore,
    refs: &mut Vec<tokenzero_core::RefRecord>,
) -> PersistResult {
    let error = (!refs.is_empty())
        .then(|| store.persist_pending())
        .transpose()
        .err()
        .map(|err| err.to_string());
    if error.is_some() {
        refs.clear();
    }
    PersistResult {
        refs_complete: error.is_none() && prune_dead_refs(store, refs),
        error,
    }
}

pub fn push_payload_refs(
    refs: &mut Vec<tokenzero_core::RefRecord>,
    stored: &StoredPayload,
    bytes: usize,
) {
    refs.push(ref_record("blob", stored.blob_ref.clone(), bytes));
    refs.push(ref_record("file", stored.file_ref.clone(), bytes));
}

impl TokenZeroEngine {
    /// Rewrite full-hash blob refs in a tool response to durable ordinal
    /// aliases and persist the ordinal-to-full mapping before emission.
    pub fn apply_session_visible_ref_aliases(&self, response: &mut ToolResponse) {
        let full_refs = response
            .refs
            .iter()
            .filter(|record| {
                tokenzero_recovery::session_visible_blob_alias(&record.ref_id).is_some()
            })
            .map(|record| record.ref_id.clone())
            .collect::<Vec<_>>();
        if full_refs.is_empty() {
            // Scan before leasing. Most responses have no repeated path/symbol
            // atom worth aliasing, and for those the old code still took the
            // recovery-store lease and re-counted tokens over the entire visible
            // text -- pure overhead on every warm read, which measured as a
            // ~30-60% p50 regression on the warm MCP read workload.
            let Some(visible) = response.visible.as_mut() else {
                return;
            };
            if !crate::text_aliases::has_alias_candidates(&visible.text) {
                return;
            }
            let mut store = self.recovery_store();
            let Some(rewritten) = crate::text_aliases::alias_repeated_paths_and_symbols_if_changed(
                &mut store,
                &visible.text,
            ) else {
                return;
            };
            visible.text = rewritten;
            let visible_tokens = count_tokens(&visible.text);
            if let Some(accounting) = response.accounting.as_mut() {
                accounting.visible_tokens = visible_tokens;
            }
            return;
        }
        let mut store = self.recovery_store();
        let Ok(range) = store.reserve_ordinal_range(full_refs.len() as u64) else {
            return;
        };
        let mut aliases = Vec::with_capacity(full_refs.len());
        for (offset, full_ref) in full_refs.iter().enumerate() {
            let Ok(alias) = store.store_ordinal_alias_deferred(range, offset as u64, full_ref)
            else {
                return;
            };
            aliases.push((full_ref.clone(), alias));
        }
        if store.persist_pending().is_err() {
            return;
        }
        // Rewrite the complete response, not only refs/visible/telemetry.
        // `detail_ref`, safety, channels, diagnostics, and future string fields
        // must never retain a duplicate full ref after the public refs changed.
        let Ok(mut encoded) = serde_json::to_string(response) else {
            return;
        };
        for (full_ref, alias) in &aliases {
            encoded = encoded.replace(full_ref, alias);
        }
        let Ok(rewritten) = serde_json::from_str::<ToolResponse>(&encoded) else {
            return;
        };
        *response = rewritten;
        if let Some(visible) = response.visible.as_mut() {
            // Same prefilter as the no-refs branch above: only pay for the
            // path/symbol scan when the text can actually contain an atom.
            if crate::text_aliases::has_alias_candidates(&visible.text)
                && let Some(rewritten) =
                    crate::text_aliases::alias_repeated_paths_and_symbols_if_changed(
                        &mut store,
                        &visible.text,
                    )
            {
                visible.text = rewritten;
            }
        }
        if let Some(accounting) = response.accounting.as_mut() {
            if let Some(visible) = response.visible.as_ref() {
                accounting.visible_tokens = count_tokens(&visible.text);
            }
            accounting.exact_ref_tokens = Some(
                response
                    .refs
                    .iter()
                    .map(|record| count_tokens(&record.ref_id))
                    .sum(),
            );
        }
    }
}

pub fn served_record(content: &str, stored: &StoredPayload) -> ServedRecord {
    served_record_with_metadata(
        sha256_hex(content),
        content.len(),
        content.lines().count(),
        stored,
    )
}

pub(crate) fn served_record_with_metadata(
    content_sha256: String,
    byte_len: usize,
    line_count: usize,
    stored: &StoredPayload,
) -> ServedRecord {
    ServedRecord {
        content_sha256,
        blob_ref: stored.blob_ref.clone(),
        file_ref: stored.file_ref.clone(),
        raw_tokens: stored.raw_tokens,
        line_count,
        byte_len,
        served_at: SystemTime::now(),
        serve_count: 1,
    }
}

pub fn success_response(
    tool: &str,
    mode: Mode,
    text: String,
    refs: Vec<tokenzero_core::RefRecord>,
    accounting: (usize, usize, usize, Option<usize>),
) -> ToolResponse {
    ToolResponse::ok(
        tool,
        mode,
        text,
        refs,
        Accounting {
            raw_tokens: accounting.0,
            visible_tokens: accounting.1,
            recovery_tokens: accounting.2,
            billed_tokens: accounting.1,
            cached_tokens: 0,
            exact_ref_tokens: accounting.3,
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalPayloadPolicy {
    Inline,
    ExactRef,
}

/// Auto mode keeps local payloads inline through the configured boundary and
/// prefers an exact selector only above it. Explicit modes always win.
pub fn local_payload_policy(
    payload_bytes: usize,
    exact_ref_threshold_bytes: usize,
    mode: Mode,
    exact_ref_available: bool,
) -> LocalPayloadPolicy {
    if mode == Mode::Auto && exact_ref_available && payload_bytes > exact_ref_threshold_bytes {
        LocalPayloadPolicy::ExactRef
    } else {
        LocalPayloadPolicy::Inline
    }
}

pub fn recoverable_capsule(
    rendered: &str,
    fallback: &str,
    raw_tokens: usize,
    mode: Mode,
    max_visible_tokens: usize,
    label: &str,
    recovery_ref: Option<&str>,
    refs_complete: bool,
) -> Result<tokenzero_core::Capsule, String> {
    if refs_complete {
        tokenzero_core::make_capsule_with_recovery_ref(
            rendered,
            raw_tokens,
            mode,
            max_visible_tokens,
            Some(label),
            recovery_ref,
        )
    } else {
        Ok(tokenzero_core::Capsule {
            text: fallback.trim_end().to_string(),
            raw_tokens,
            visible_tokens: raw_tokens,
            omitted_lines: 0,
            mode,
            protected_anchors: Vec::new(),
            exact_refs: Vec::new(),
            lossy_spans: Vec::new(),
            lossy_policy_id: None,
        })
    }
}

pub fn cache_write_diagnostic(message: impl Into<String>) -> tokenzero_core::Diagnostic {
    tokenzero_core::Diagnostic {
        code: "cache_write_failed".to_string(),
        message: message.into(),
        repair: Some("fix recovery cache permissions or pass --cache-path".to_string()),
    }
}

pub fn failure_response(
    tool: &str,
    code: &str,
    message: impl Into<String>,
    repair: Option<&str>,
) -> ToolResponse {
    ToolResponse::error(tool, code, message.into(), repair.map(str::to_string))
}

pub fn path_not_allowed(tool: &str, path: &Path) -> ToolResponse {
    failure_response(
        tool,
        "path_not_allowed",
        format!("path is outside allowed roots: {}", path.display()),
        None,
    )
}

pub fn expansion_response(result: ExpansionResult, recovery_tokens: usize) -> ToolResponse {
    if result.found {
        let mut response = success_response(
            "expand",
            Mode::Exact,
            result.content,
            Vec::new(),
            (
                result.tokens,
                result.tokens,
                recovery_tokens,
                Some(count_tokens(&result.ref_id)),
            ),
        );
        if let (Some(start_line), Some(end_line), Some(line_count)) = (
            result.returned_start_line,
            result.returned_end_line,
            result.line_count,
        ) {
            response.telemetry = Some(serde_json::json!({
                "window": {
                    "clamped": result.clamped,
                    "start_line": start_line,
                    "end_line": end_line,
                    "line_count": line_count,
                }
            }));
        }
        return response;
    }
    let full_ref = &result.ref_id;
    let reason = result.reason.as_str();
    let is_window_oob = reason.starts_with("window-out-of-range");
    let exact = [
        (
            "shared-cas-missing",
            "shared_cas_missing",
            "shared CAS object missing",
        ),
        (
            "shared-cas-corruption",
            "shared_cas_corruption",
            "shared CAS object corrupted",
        ),
        (
            "shared-cas-policy",
            "shared_cas_policy",
            "shared CAS policy denied expansion",
        ),
        ("shared-cas-io", "shared_cas_io", "shared CAS I/O failure"),
        (
            "shared-cas-non-utf8",
            "shared_cas_non_utf8",
            "shared CAS object is not UTF-8 text",
        ),
        (
            "unsupported-ref-kind",
            "unsupported_ref_kind",
            "foreign non-blob ref requires its owning engine",
        ),
        ("stale-ref", "ref_stale", "ref is no longer recoverable"),
        (
            "invalid-ref",
            "invalid_ref",
            "ref is not a valid tz://, fz://, or gz:// recovery handle",
        ),
        (
            "decode-failed",
            "expand_failed",
            "ref was found but could not be decoded",
        ),
    ];
    let (code, message) = if reason == "dangling-ref" {
        ("dangling_ref", format!("{reason} (ref: {full_ref})"))
    } else if reason.starts_with("ref-not-found") {
        ("ref_not_found", format!("{reason} (ref: {full_ref})"))
    } else if is_window_oob {
        ("window_out_of_range", format!("{reason} (ref: {full_ref})"))
    } else if reason.starts_with("zeroref-") {
        ("zeroref_malformed", format!("{reason}: {full_ref}"))
    } else if let Some(code) = fragment_error_code(reason) {
        // yevj: invalid fragments fail typed ONCE — the code names the
        // fragment defect so adapters stop instead of retrying a ref that can
        // never resolve. The reason carries the parsed bounds detail.
        (code, format!("{reason} (ref: {full_ref})"))
    } else if let Some((_, code, message)) = exact.iter().find(|entry| entry.0 == reason) {
        (*code, format!("{message}: {full_ref}"))
    } else {
        ("expand_failed", format!("ref expansion failed: {full_ref}"))
    };
    let repair = if is_window_oob {
        "choose start_line/end_line within the stored payload line count (1-based inclusive)"
    } else if fragment_error_code(reason).is_some() {
        "drop the fragment suffix to expand the whole payload, or re-issue it within the stored extents"
    } else if reason == "unsupported-ref-kind" {
        "route the ref to the engine named by its scheme"
    } else {
        "align the producer and consumer shared store root, then retry with the exact ref"
    };
    let mut response = ToolResponse::error("expand", code, message, Some(repair.to_string()));
    response.telemetry = Some(serde_json::json!({
        "expand": {
            "fail_count": 1,
            "dangling_ref_count": u64::from(reason == "dangling-ref"),
            "miss_kind": code,
        }
    }));
    response
}

/// Map a recovery-store fragment failure reason to a stable typed error
/// code. Reasons may carry `; key=value` bounds detail after the kind tag.
fn fragment_error_code(reason: &str) -> Option<&'static str> {
    const FRAGMENT_REASONS: &[(&str, &str)] = &[
        ("fragment-malformed", "fragment_malformed"),
        ("fragment-reversed", "fragment_reversed"),
        ("fragment-out-of-range", "fragment_out_of_range"),
        ("fragment-not-utf8-boundary", "fragment_not_utf8_boundary"),
        ("non_utf8_line_fragment", "fragment_not_utf8_boundary"),
        ("fragment-unknown-kind", "fragment_unknown_kind"),
        ("fragment-duplicate", "fragment_duplicate"),
    ];
    FRAGMENT_REASONS
        .iter()
        .find(|(tag, _)| reason == *tag || reason.starts_with(&format!("{tag};")))
        .map(|(_, code)| *code)
}

pub fn unchanged_since_expand_ack(since_ref: &str) -> String {
    format!("unchanged since {since_ref}")
}

pub fn expand_since_diff_text(since_ref: &str, target_ref: &str, diff_body: &str) -> String {
    format!(
        "# expand {target_ref} — diff since {since_ref}
{diff_body}"
    )
}

pub fn common_content_type(content_types: &[ContentType]) -> ContentType {
    let Some(first) = content_types.first().copied() else {
        return ContentType::Unknown;
    };
    if content_types
        .iter()
        .all(|content_type| *content_type == first)
    {
        first
    } else {
        ContentType::Unknown
    }
}

pub fn exact_ref_token_count(refs: &[tokenzero_core::RefRecord]) -> usize {
    refs.iter().map(|record| count_tokens(&record.ref_id)).sum()
}

/// Re-verify advertised refs after a persist: the persist's cache merge can
/// evict entries under byte/count pressure (including refs stored earlier in
/// the same call), and a response must never advertise a ref that can no
/// longer be expanded. Returns true when every ref survived.
pub fn prune_dead_refs(store: &RecoveryStore, refs: &mut Vec<tokenzero_core::RefRecord>) -> bool {
    let before = refs.len();
    refs.retain(|record| store.has_ref(&record.ref_id));
    refs.len() == before
}

pub struct AppliedEdits {
    pub(crate) text: String,
    pub(crate) diff: String,
    pub(crate) lines_added: usize,
    pub(crate) lines_removed: usize,
}

pub struct EditFailure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) repair: Option<String>,
}

fn edit_failure(
    code: &'static str,
    message: impl Into<String>,
    repair: &'static str,
) -> Result<AppliedEdits, EditFailure> {
    Err(EditFailure {
        code,
        message: message.into(),
        repair: Some(repair.to_string()),
    })
}

/// Whole-file hunk for `create=true`: `replace` becomes the file content.
pub fn create_file_hunk(hunk: &EditHunk) -> Result<AppliedEdits, EditFailure> {
    if hunk.replace.is_empty() {
        return edit_failure(
            "no_op_hunk",
            "create hunk has an empty replace; nothing to write",
            "pass the full new-file content in replace",
        );
    }
    let mut diff = String::from("@@ hunk 1 @@ line 1");
    for line in hunk.replace.lines() {
        diff.push_str("\n+");
        diff.push_str(line);
    }
    Ok(AppliedEdits {
        text: hunk.replace.clone(),
        diff,
        lines_added: hunk.replace.lines().count(),
        lines_removed: 0,
    })
}

pub fn apply_edit_hunks(original: &str, edits: &[EditHunk]) -> Result<AppliedEdits, EditFailure> {
    let mut text = original.to_string();
    let mut sections = Vec::new();
    let mut lines_added = 0usize;
    let mut lines_removed = 0usize;
    for (index, hunk) in edits.iter().enumerate() {
        if hunk.find.is_empty() {
            return edit_failure(
                "edit_failed",
                format!("edits[{index}] has an empty find; that is only valid with create=true"),
                "pass the exact text to replace in find",
            );
        }
        if hunk.find == hunk.replace {
            return edit_failure(
                "no_op_hunk",
                format!("edits[{index}] replaces text with identical text"),
                "drop the hunk or change replace",
            );
        }
        let offsets: Vec<usize> = text.match_indices(&hunk.find).map(|(at, _)| at).collect();
        if offsets.is_empty() {
            let hint = closest_line_hint(&text, &hunk.find)
                .map(|hint| format!("; {hint}"))
                .unwrap_or_default();
            return edit_failure(
                "hunk_not_found",
                format!("edits[{index}] matched nothing{hint}"),
                "re-read the file and pass the exact current text in find",
            );
        }
        if offsets.len() > 1 && !hunk.replace_all {
            return edit_failure(
                "ambiguous_hunk",
                format!(
                    "edits[{index}] matches {} times; expected exactly one match",
                    offsets.len()
                ),
                "add surrounding context to find or set replace_all=true",
            );
        }
        for (occurrence, &offset) in offsets.iter().enumerate() {
            let label = if offsets.len() > 1 {
                format!("@@ hunk {} occurrence {} @@", index + 1, occurrence + 1)
            } else {
                format!("@@ hunk {} @@", index + 1)
            };
            let (section, added, removed) =
                render_edit_region(&text, offset, &hunk.find, &hunk.replace, &label);
            sections.push(section);
            lines_added += added;
            lines_removed += removed;
        }
        // Apply from the last occurrence backwards so earlier offsets stay
        // valid; offsets were collected non-overlapping left-to-right.
        for &offset in offsets.iter().rev() {
            text.replace_range(offset..offset + hunk.find.len(), &hunk.replace);
        }
    }
    Ok(AppliedEdits {
        text,
        diff: sections.join("\n"),
        lines_added,
        lines_removed,
    })
}

/// Hunk-labelled context-1 before/after rendering of one replacement (a
/// deliberate lightweight projection, not a strict unified diff). Returns the
/// section text plus added/removed line counts.
pub fn render_edit_region(
    text: &str,
    offset: usize,
    find: &str,
    replace: &str,
    label: &str,
) -> (String, usize, usize) {
    let region_start = text[..offset].rfind('\n').map(|at| at + 1).unwrap_or(0);
    let match_end = offset + find.len();
    let region_end = text[match_end..]
        .find('\n')
        .map(|at| match_end + at)
        .unwrap_or(text.len());
    let old_lines: Vec<&str> = text[region_start..region_end].split('\n').collect();
    let new_region = format!(
        "{}{}{}",
        &text[region_start..offset],
        replace,
        &text[match_end..region_end]
    );
    let new_lines: Vec<&str> = new_region.split('\n').collect();
    let mut prefix = 0;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old_lines.len() - prefix
        && suffix < new_lines.len() - prefix
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let removed = &old_lines[prefix..old_lines.len() - suffix];
    let added = &new_lines[prefix..new_lines.len() - suffix];
    let region_first_line = text[..region_start].matches('\n').count();
    let first_changed_line = region_first_line + prefix;
    let file_lines: Vec<&str> = text.split('\n').collect();
    let mut section = format!("{label} line {}", first_changed_line + 1);
    if first_changed_line > 0 {
        if let Some(context) = file_lines.get(first_changed_line - 1) {
            if !context.is_empty() {
                section.push_str(&format!("\n {context}"));
            }
        }
    }
    for line in removed {
        section.push_str(&format!("\n-{line}"));
    }
    for line in added {
        section.push_str(&format!("\n+{line}"));
    }
    if let Some(context) = file_lines.get(first_changed_line + removed.len()) {
        if !context.is_empty() {
            section.push_str(&format!("\n {context}"));
        }
    }
    if removed.is_empty() && added.is_empty() {
        // The replacement only moved a line boundary (e.g. dropped a trailing
        // newline); there is no whole changed line to show.
        section.push_str("\n~ newline-only change");
    }
    (section, added.len(), removed.len())
}

/// Cheap near-miss hint for hunk_not_found: the first file line containing
/// the find's first non-empty line, clamped for the error message.
pub fn closest_line_hint(text: &str, find: &str) -> Option<String> {
    let probe = find.lines().find(|line| !line.trim().is_empty())?.trim();
    let (number, line) = text
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(probe))?;
    let trimmed = line.trim();
    let shown: String = trimmed.chars().take(80).collect();
    let ellipsis = if trimmed.chars().count() > 80 {
        "…"
    } else {
        ""
    };
    Some(format!("closest line {}: {shown}{ellipsis}", number + 1))
}

/// Write via a temp file in the same directory plus rename so a crash or
/// concurrent reader never observes a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "tz-edit".to_string());
    let temp_path = directory.join(format!(".{file_name}.tz-edit-{}", std::process::id()));
    fs::write(&temp_path, bytes)?;
    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&temp_path);
            Err(err)
        }
    }
}

pub fn degraded_shell_response(
    command: &str,
    mode: Mode,
    output: &str,
    error: String,
) -> ToolResponse {
    let mut response = ToolResponse::ok(
        "shell",
        Mode::Passthrough,
        output.to_string(),
        Vec::new(),
        Accounting {
            raw_tokens: count_tokens(output),
            visible_tokens: count_tokens(output),
            recovery_tokens: 0,
            billed_tokens: count_tokens(output),
            cached_tokens: 0,
            exact_ref_tokens: Some(0),
        },
    );
    response.content_type = Some(ContentType::ShellOutput.to_string());
    response.diagnostic = Some(tokenzero_core::Diagnostic {
        code: "cache_write_failed".to_string(),
        message: format!("could not persist exact shell bytes for {command}"),
        repair: Some("rerun after fixing recovery cache permissions".to_string()),
    });
    response.telemetry = Some(json!({
        "command": command,
        "requested_mode": mode.to_string(),
        "transport_status": "degraded",
        "degraded": true,
        "storage_error": error,
        "output_strategy": "exact_passthrough_storage_failed"
    }));
    response
}

/// A dedup/diff substitution computed during the read loop but applied only
/// after the recovery refs it advertises have actually persisted — a note
/// that replaces content with refs is only safe when the refs resolve.
pub enum PendingSubstitution {
    Dedup {
        idx: usize,
        note: String,
        note_tokens: usize,
        full_tokens: usize,
        serve_count: usize,
        cross_session: bool,
    },
    Diff {
        idx: usize,
        text: String,
        diff_tokens: usize,
        full_tokens: usize,
        telemetry: DiffTelemetry,
    },
}

/// Seen-set note for an identical re-read (docs/codemode.md §5a). Both refs
/// are the freshly minted ones for this serve, so the note alone recovers
/// the exact bytes even if the client compacted the earlier payload away.
/// Callers must only emit it after those refs persisted.
pub fn unchanged_read_note(path: &Path, text: &str, stored: &StoredPayload) -> String {
    format!(
        "unchanged: {} (served earlier this session)\n# {} — {} lines, {} tokens; full bytes: expand {}",
        stored.file_ref,
        path.display(),
        text.lines().count(),
        stored.raw_tokens,
        stored.blob_ref
    )
}

/// Seen-set note for identical re-run find/grep output; the echoed query is
/// clamped exactly like zero-hit notes.
pub fn unchanged_search_note(
    tool: &str,
    query: &str,
    output: &str,
    stored: &StoredPayload,
) -> String {
    format!(
        "unchanged: {} (served earlier this session)\n# {tool} {} — {} matches, {} tokens; full results: expand {}",
        stored.file_ref,
        zero_hit_label(query),
        output.lines().count(),
        stored.raw_tokens,
        stored.blob_ref
    )
}

/// Diff-aware re-read (docs/codemode.md §5b): recover the previously served
/// bytes through the existing recovery API, render a unified diff, and
/// return it only when strictly cheaper than the full render. Any miss —
/// pruned base, oversized side, tie or larger diff — returns `None` and the
/// caller serves full. The base expansion is charged as recovery tokens on
/// `store`, keeping recovery-adjusted savings honest.
pub fn diff_since_served(
    store: &mut RecoveryStore,
    path: &Path,
    text: &str,
    previous: &ServedRecord,
    stored: &StoredPayload,
    full_tokens: usize,
) -> Option<(String, usize, DiffTelemetry)> {
    if text.len() > DIFF_MAX_BYTES
        || previous.byte_len > DIFF_MAX_BYTES
        || text.lines().count() > DIFF_MAX_LINES
        || previous.line_count > DIFF_MAX_LINES
    {
        return None;
    }
    // Diff bases are an internal session optimization, not a user recovery
    // request. If the base is no longer DURABLE (external prune removed the
    // cache/ref-index under this live process), fall back to the full render:
    // serving a diff would reference a base the agent cannot expand later.
    // In-memory state alone does not count (bxqo.1 / F-021).
    if !store.has_ref_durable(&previous.blob_ref) {
        return None;
    }
    let base = store.expand(&previous.blob_ref, Some("raw"), None, None, None, None);
    if !base.found {
        return None;
    }
    let render = diff::unified_diff(&base.content, text)?;
    let assembled = format!(
        "# read {} — changed since served this session (diff vs {})\n{}\nfull file: expand {}",
        path.display(),
        previous.blob_ref,
        render.text,
        stored.blob_ref
    );
    let diff_tokens = count_tokens(&assembled);
    if diff_tokens >= full_tokens {
        return None;
    }
    Some((
        assembled,
        diff_tokens,
        DiffTelemetry {
            hunks: render.hunks,
            plus: render.plus,
            minus: render.minus,
            base_ref: previous.blob_ref.clone(),
        },
    ))
}

pub fn pick_cheaper<'a>(flat: &'a str, compact: &'a str) -> (&'a str, bool) {
    if count_tokens(compact) < count_tokens(flat) {
        (compact, true)
    } else {
        (flat, false)
    }
}

pub fn preview(text: &str) -> String {
    const MAX_LINES: usize = 6;
    const MAX_CHARS: usize = 320;

    let lines = text.lines().collect::<Vec<_>>();
    let shown = lines.len().min(MAX_LINES);
    let more = lines.len().saturating_sub(shown);
    let marker = (more > 0).then(|| format!("\n+{more} more lines"));
    let marker_chars = marker.as_deref().map_or(0, |value| value.chars().count());
    let body_chars = MAX_CHARS.saturating_sub(marker_chars);
    let mut value = lines[..shown].join("\n");
    if value.chars().count() > body_chars {
        value = value.chars().take(body_chars).collect();
    }
    if let Some(marker) = marker {
        value.push_str(&marker);
    }
    value
}

#[cfg(test)]
mod preview_tests {
    use std::sync::Mutex;

    use tempfile::tempdir;
    use tokenzero_core::{
        Accounting, ChannelSeparation, ContentType, Mode, RefRecord, ToolResponse,
    };
    use tokenzero_recovery::{ExpansionResult, RecoveryStore};

    use super::{
        EngineConfig, LocalPayloadPolicy, RecoveryStoreLease, TokenZeroEngine, expansion_response,
        local_payload_policy, preview,
    };

    #[test]
    fn shared_recovery_store_lease_returns_its_store_without_optional_state() {
        let slot = Mutex::new(None);
        {
            let mut lease = RecoveryStoreLease::Shared {
                store: RecoveryStore::new(None),
                slot: &slot,
            };
            lease.recovery_count = 7;
        }
        let available = slot.lock().unwrap();
        assert_eq!(
            available.as_ref().map(|store| store.recovery_count),
            Some(7)
        );
    }

    #[test]
    fn capsule_payload_policy_respects_threshold_boundaries_and_modes() {
        let threshold = 1024;
        let fixtures = [
            (threshold - 1, Mode::Auto, true, LocalPayloadPolicy::Inline),
            (threshold, Mode::Auto, true, LocalPayloadPolicy::Inline),
            (
                threshold + 1,
                Mode::Auto,
                true,
                LocalPayloadPolicy::ExactRef,
            ),
            (threshold + 1, Mode::Auto, false, LocalPayloadPolicy::Inline),
            (
                threshold + 1,
                Mode::Passthrough,
                true,
                LocalPayloadPolicy::Inline,
            ),
            (threshold + 1, Mode::Exact, true, LocalPayloadPolicy::Inline),
        ];
        for (bytes, mode, exact_ref_available, expected) in fixtures {
            assert_eq!(
                local_payload_policy(bytes, threshold, mode, exact_ref_available),
                expected,
                "bytes={bytes} mode={mode:?} exact_ref_available={exact_ref_available}"
            );
        }
    }

    #[test]
    fn expansion_response_reports_clamped_window_metadata() {
        let mut result = ExpansionResult::ok(
            "tz://blob/test#L1-L200".to_string(),
            Some("raw".to_string()),
            "a
b
"
            .to_string(),
        );
        result.clamped = true;
        result.returned_start_line = Some(1);
        result.returned_end_line = Some(2);
        result.line_count = Some(2);

        let response = expansion_response(result, 0);
        let window = &response.telemetry.as_ref().unwrap()["window"];
        assert_eq!(window["clamped"], true);
        assert_eq!(window["start_line"], 1);
        assert_eq!(window["end_line"], 2);
        assert_eq!(window["line_count"], 2);
    }

    #[test]
    fn expansion_response_maps_fragment_failures_to_typed_codes() {
        let cases = [
            ("fragment-malformed", "fragment_malformed"),
            ("fragment-reversed", "fragment_reversed"),
            (
                "fragment-out-of-range; start=0 end=99 len=4",
                "fragment_out_of_range",
            ),
            (
                "fragment-not-utf8-boundary; start=1 end=3 len=4",
                "fragment_not_utf8_boundary",
            ),
            ("non_utf8_line_fragment", "fragment_not_utf8_boundary"),
            ("fragment-unknown-kind", "fragment_unknown_kind"),
            ("fragment-duplicate", "fragment_duplicate"),
        ];
        for (reason, code) in cases {
            let result = ExpansionResult::missing(
                "tz://blob/test#B0+99".to_string(),
                Some("raw".to_string()),
                reason,
            );
            let response = expansion_response(result, 0);
            let error = response.error.as_ref().expect(reason);
            assert_eq!(error.code, code, "reason {reason}");
            assert!(
                error.message.contains(reason.split(';').next().unwrap()),
                "message keeps the typed reason detail: {}",
                error.message
            );
            assert!(
                error
                    .repair
                    .as_deref()
                    .is_some_and(|repair| repair.contains("fragment")),
                "fragment repair hint: {:?}",
                error.repair
            );
        }
    }

    #[test]
    fn expansion_response_preserves_typed_misses_for_ledger() {
        for (reason, code, dangling) in [
            ("dangling-ref", "dangling_ref", 1),
            ("stale-ref", "ref_stale", 0),
            ("ref-not-found", "ref_not_found", 0),
        ] {
            let result =
                ExpansionResult::missing("tz://o/7/23".to_owned(), Some("raw".to_owned()), reason);
            let response = expansion_response(result, 0);
            assert_eq!(response.error.as_ref().unwrap().code, code);
            let expand = &response.telemetry.as_ref().unwrap()["expand"];
            assert_eq!(expand["fail_count"], 1);
            assert_eq!(expand["dangling_ref_count"], dangling);
            assert_eq!(expand["miss_kind"], code);
        }
    }

    #[test]
    fn session_alias_rewrites_every_ref_field_and_survives_restart() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("recovery.json");
        let mut config = EngineConfig::for_root(dir.path());
        config.cache_path = cache.clone();
        let engine = TokenZeroEngine::new(config);
        let full_ref = engine
            .recovery_store()
            .store_blob("exact payload", ContentType::Unknown)
            .unwrap();
        let mut response = ToolResponse::ok(
            "read",
            Mode::Auto,
            format!("visible {full_ref}"),
            vec![RefRecord {
                kind: "blob".into(),
                ref_id: full_ref.clone(),
                bytes: 13,
                live: true,
            }],
            Accounting {
                raw_tokens: 3,
                visible_tokens: 3,
                recovery_tokens: 0,
                billed_tokens: 3,
                cached_tokens: 0,
                exact_ref_tokens: None,
            },
        );
        response.telemetry = Some(serde_json::json!({"ref": full_ref}));
        response.safety = Some(serde_json::json!({"anchor": full_ref}));
        response.channels = Some(ChannelSeparation {
            action: "read".into(),
            status_line: format!("ref={full_ref}"),
            user_message: Some(format!("expand {full_ref}")),
        });

        engine.apply_session_visible_ref_aliases(&mut response);
        let alias = response.refs[0].ref_id.clone();
        assert!(alias.starts_with("tz://o/"), "{alias}");
        assert_eq!(response.detail_ref.as_deref(), Some(alias.as_str()));
        assert!(
            !serde_json::to_string(&response)
                .unwrap()
                .contains(&full_ref),
            "every response field must use the same alias: {response:?}"
        );
        drop(engine);

        let mut restarted = RecoveryStore::new(Some(cache));
        let expanded = restarted.expand(&alias, Some("raw"), None, None, None, None);
        assert!(expanded.found, "{}", expanded.reason);
        assert_eq!(expanded.content, "exact payload");
    }

    #[test]
    fn multiline_preview_is_bounded_and_reports_omitted_lines() {
        let text = (1..=9)
            .map(|line| format!("line {line}: {}", "x".repeat(80)))
            .collect::<Vec<_>>()
            .join("\n");
        let rendered = preview(&text);
        assert!(rendered.lines().count() <= 7);
        assert!(rendered.chars().count() <= 320);
        assert!(rendered.ends_with("+3 more lines"), "{rendered}");
        assert!(rendered.contains('\n'));
    }
}

pub fn captured_stream_text(text: &str, capture: &StreamCapture, stream_name: &str) -> String {
    if !capture.truncated {
        return text.to_string();
    }
    let mut value = text.to_string();
    if !value.is_empty() && !value.ends_with('\n') {
        value.push('\n');
    }
    value.push_str(&format!(
        "[tokenzero:{stream_name} truncated: captured {} of {} bytes",
        capture.captured_bytes, capture.bytes_seen
    ));
    if let Some(path) = capture.spill_path.as_deref() {
        value.push_str(&format!("; spill_path: {path}"));
    }
    value.push_str("]\n");
    value
}

/// Compatibility opt-out for the default slim CLI ToolResponse envelope.
/// `0`/`off`/`false`/`no` selects the full forensic envelope.
pub const SLIM_ENVELOPE_ENV: &str = "TOKENZERO_SLIM_ENVELOPE";

static FULL_CLI_ENVELOPE_REQUESTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Select the full forensic envelope for this CLI process (`--json=full`).
pub fn request_full_cli_envelope() {
    FULL_CLI_ENVELOPE_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn slim_envelope_enabled() -> bool {
    if FULL_CLI_ENVELOPE_REQUESTED.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    std::env::var(SLIM_ENVELOPE_ENV)
        .map(|raw| {
            !matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            )
        })
        .unwrap_or(true)
}

pub fn cli_json(response: &ToolResponse) -> String {
    if slim_envelope_enabled() {
        return slim_cli_json(response);
    }
    serde_json::to_string_pretty(response).unwrap_or_else(|_| {
        format!("{{\"schema_version\":\"{CLI_SCHEMA_VERSION}\",\"status\":\"error\"}}")
    })
}

/// Slim projection: keeps the stable minimum envelope, payload, and every
/// durable ref as a bare string. `--json=full` restores advisory accounting,
/// telemetry, mode, and content-type blocks. Deterministic for the same input.
fn slim_cli_json(response: &ToolResponse) -> String {
    let mut doc = serde_json::Map::new();
    doc.insert(
        "schema_version".into(),
        serde_json::json!(response.schema_version),
    );
    doc.insert("status".into(), serde_json::json!(response.status));
    doc.insert("tool".into(), serde_json::json!(response.tool));
    if let Some(ack) = &response.ack {
        doc.insert("ack".into(), serde_json::json!(ack));
    }
    if let Some(visible) = &response.visible {
        // The capsule wrapper ({kind,text}) costs ~28B per call and "capsule"
        // is the only kind the CLI ever emits, so slim carries the bare text.
        doc.insert("visible".into(), serde_json::json!(visible.text));
    }
    if !response.refs.is_empty() {
        doc.insert(
            "refs".into(),
            serde_json::json!(
                response
                    .refs
                    .iter()
                    .map(|record| record.ref_id.as_str())
                    .collect::<Vec<_>>()
            ),
        );
    }
    // detail_ref is defined as refs.first() (tokenzero-core ToolResponse::new),
    // so restating it costs a full 74B ref for zero information. Emit it only
    // when it is not already recoverable from the refs array.
    if let Some(detail_ref) = &response.detail_ref {
        if !response
            .refs
            .iter()
            .any(|record| record.ref_id == *detail_ref)
        {
            doc.insert("detail_ref".into(), serde_json::json!(detail_ref));
        }
    }
    if let Some(error) = &response.error {
        doc.insert(
            "error".into(),
            serde_json::to_value(error).unwrap_or(serde_json::Value::Null),
        );
        if let Some(diagnostic) = &response.diagnostic {
            doc.insert(
                "diagnostic".into(),
                serde_json::to_value(diagnostic).unwrap_or(serde_json::Value::Null),
            );
        }
    }
    if let Some(safety) = &response.safety {
        doc.insert("safety".into(), safety.clone());
    }
    if let Some(recovery) = &response.recovery {
        doc.insert(
            "recovery".into(),
            serde_json::to_value(recovery).unwrap_or(serde_json::Value::Null),
        );
    }
    if let Some(channels) = &response.channels {
        doc.insert(
            "channels".into(),
            serde_json::to_value(channels).unwrap_or(serde_json::Value::Null),
        );
    }
    serde_json::to_string(&serde_json::Value::Object(doc)).unwrap_or_else(|_| {
        format!(
            "{{\"schema_version\":\"{CLI_SCHEMA_VERSION}\",\"status\":\"error\",\"tool\":\"{}\"}}",
            response.tool
        )
    })
}

pub fn render_text(response: &ToolResponse) -> String {
    if let Some(error) = &response.error {
        return format!("error: {} ({})\n", error.message, error.code);
    }
    let mut out = String::new();
    if let Some(visible) = &response.visible {
        out.push_str(visible.text.trim_end());
        out.push('\n');
    }
    if !is_compact_shell_response(response) {
        for record in &response.refs {
            // Full shell capsules already anchor their refs in the header;
            // appending those again doubles every ref line. Only refs the
            // visible text does not carry (e.g. capture_ref) are added.
            if out.contains(&record.ref_id) {
                continue;
            }
            out.push_str(&format!("{}_ref: {}\n", record.kind, record.ref_id));
        }
    }
    out
}

pub fn is_compact_shell_response(response: &ToolResponse) -> bool {
    response.tool == "shell"
        && matches!(
            response
                .telemetry
                .as_ref()
                .and_then(|value| value.get("output_strategy"))
                .and_then(|value| value.as_str()),
            Some("compact_adaptive_shell") | Some("minimal_envelope_shell")
        )
}
