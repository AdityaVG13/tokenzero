use crate::*;

pub(crate) struct PersistResult {
    pub(crate) refs_complete: bool,
    pub(crate) error: Option<String>,
}

impl TokenZeroEngine {
    pub(crate) fn recovery_store(&self) -> RecoveryStore {
        RecoveryStore::new(Some(self.config.cache_path.clone()))
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

pub(crate) fn inner_env() -> BTreeMap<String, String> {
    BTreeMap::from([("TOKENZERO_INNER".to_string(), "1".to_string())])
}

pub(crate) fn persist_refs(
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

pub(crate) fn push_payload_refs(
    refs: &mut Vec<tokenzero_core::RefRecord>,
    stored: &StoredPayload,
    bytes: usize,
) {
    refs.push(ref_record("blob", stored.blob_ref.clone(), bytes));
    refs.push(ref_record("file", stored.file_ref.clone(), bytes));
}

pub(crate) fn served_record(content: &str, stored: &StoredPayload) -> ServedRecord {
    ServedRecord {
        content_sha256: sha256_hex(content),
        blob_ref: stored.blob_ref.clone(),
        file_ref: stored.file_ref.clone(),
        raw_tokens: stored.raw_tokens,
        line_count: content.lines().count(),
        byte_len: content.len(),
        served_at: SystemTime::now(),
        serve_count: 1,
    }
}

pub(crate) fn success_response(
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
            exact_ref_tokens: accounting.3,
        },
    )
}

pub(crate) fn recoverable_capsule(
    rendered: &str,
    fallback: &str,
    raw_tokens: usize,
    mode: Mode,
    max_visible_tokens: usize,
    label: &str,
    refs_complete: bool,
) -> tokenzero_core::Capsule {
    if refs_complete {
        make_capsule_with_raw_tokens(rendered, raw_tokens, mode, max_visible_tokens, Some(label))
    } else {
        tokenzero_core::Capsule {
            text: fallback.trim_end().to_string(),
            raw_tokens,
            visible_tokens: raw_tokens,
            omitted_lines: 0,
            mode,
        }
    }
}

pub(crate) fn cache_write_diagnostic(message: impl Into<String>) -> tokenzero_core::Diagnostic {
    tokenzero_core::Diagnostic {
        code: "cache_write_failed".to_string(),
        message: message.into(),
        repair: Some("fix recovery cache permissions or pass --cache-path".to_string()),
    }
}

pub(crate) fn failure_response(
    tool: &str,
    code: &str,
    message: impl Into<String>,
    repair: Option<&str>,
) -> ToolResponse {
    ToolResponse::error(tool, code, message.into(), repair.map(str::to_string))
}

pub(crate) fn path_not_allowed(tool: &str, path: &Path) -> ToolResponse {
    failure_response(
        tool,
        "path_not_allowed",
        format!("path is outside allowed roots: {}", path.display()),
        None,
    )
}

pub(crate) fn expansion_response(result: ExpansionResult, recovery_tokens: usize) -> ToolResponse {
    if result.found {
        return success_response(
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
    let (code, message) = if reason.starts_with("ref-not-found") || reason == "dangling-ref" {
        ("ref_not_found", format!("{reason} (ref: {full_ref})"))
    } else if is_window_oob {
        ("window_out_of_range", format!("{reason} (ref: {full_ref})"))
    } else if reason.starts_with("zeroref-") {
        ("zeroref_malformed", format!("{reason}: {full_ref}"))
    } else if let Some((_, code, message)) = exact.iter().find(|entry| entry.0 == reason) {
        (*code, format!("{message}: {full_ref}"))
    } else {
        ("expand_failed", format!("ref expansion failed: {full_ref}"))
    };
    let repair = if is_window_oob {
        "choose start_line/end_line within the stored payload line count (1-based inclusive)"
    } else if reason == "unsupported-ref-kind" {
        "route the ref to the engine named by its scheme"
    } else {
        "align the producer and consumer shared store root, then retry with the exact ref"
    };
    ToolResponse::error("expand", code, message, Some(repair.to_string()))
}

pub(crate) fn unchanged_since_expand_ack(since_ref: &str) -> String {
    format!("unchanged since {since_ref}")
}

pub(crate) fn expand_since_diff_text(since_ref: &str, target_ref: &str, diff_body: &str) -> String {
    format!(
        "# expand {target_ref} — diff since {since_ref}
{diff_body}"
    )
}

pub(crate) fn common_content_type(content_types: &[ContentType]) -> ContentType {
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

pub(crate) fn exact_ref_token_count(refs: &[tokenzero_core::RefRecord]) -> usize {
    refs.iter().map(|record| count_tokens(&record.ref_id)).sum()
}

/// Re-verify advertised refs after a persist: the persist's cache merge can
/// evict entries under byte/count pressure (including refs stored earlier in
/// the same call), and a response must never advertise a ref that can no
/// longer be expanded. Returns true when every ref survived.
pub(crate) fn prune_dead_refs(
    store: &RecoveryStore,
    refs: &mut Vec<tokenzero_core::RefRecord>,
) -> bool {
    let before = refs.len();
    refs.retain(|record| store.has_ref(&record.ref_id));
    refs.len() == before
}

pub(crate) struct AppliedEdits {
    pub(crate) text: String,
    pub(crate) diff: String,
    pub(crate) lines_added: usize,
    pub(crate) lines_removed: usize,
}

pub(crate) struct EditFailure {
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
pub(crate) fn create_file_hunk(hunk: &EditHunk) -> Result<AppliedEdits, EditFailure> {
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

pub(crate) fn apply_edit_hunks(
    original: &str,
    edits: &[EditHunk],
) -> Result<AppliedEdits, EditFailure> {
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
pub(crate) fn render_edit_region(
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
pub(crate) fn closest_line_hint(text: &str, find: &str) -> Option<String> {
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
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
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

pub(crate) fn degraded_shell_response(
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
pub(crate) enum PendingSubstitution {
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

/// Seen-set note for an identical re-read (docs/routing.md §5a). Both refs
/// are the freshly minted ones for this serve, so the note alone recovers
/// the exact bytes even if the client compacted the earlier payload away.
/// Callers must only emit it after those refs persisted.
pub(crate) fn unchanged_read_note(path: &Path, text: &str, stored: &StoredPayload) -> String {
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
pub(crate) fn unchanged_search_note(
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

/// Diff-aware re-read (docs/routing.md §5b): recover the previously served
/// bytes through the existing recovery API, render a unified diff, and
/// return it only when strictly cheaper than the full render. Any miss —
/// pruned base, oversized side, tie or larger diff — returns `None` and the
/// caller serves full. The base expansion is charged as recovery tokens on
/// `store`, keeping recovery-adjusted savings honest.
pub(crate) fn diff_since_served(
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
    // request. If the current cache no longer has the base, fall back to the
    // full render instead of reviving an older same-ref payload through the
    // per-user cross-root index.
    if !store.has_ref(&previous.blob_ref) {
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

pub(crate) fn pick_cheaper<'a>(flat: &'a str, compact: &'a str) -> (&'a str, bool) {
    if count_tokens(compact) < count_tokens(flat) {
        (compact, true)
    } else {
        (flat, false)
    }
}

pub(crate) fn preview(text: &str) -> String {
    let mut value = text.lines().take(8).collect::<Vec<_>>().join("\n");
    if value.len() > 1000 {
        value.truncate(1000);
    }
    value
}

pub(crate) fn captured_stream_text(
    text: &str,
    capture: &StreamCapture,
    stream_name: &str,
) -> String {
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

pub fn cli_json(response: &ToolResponse) -> String {
    serde_json::to_string_pretty(response).unwrap_or_else(|_| {
        format!("{{\"schema_version\":\"{CLI_SCHEMA_VERSION}\",\"status\":\"error\"}}")
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

pub(crate) fn is_compact_shell_response(response: &ToolResponse) -> bool {
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
