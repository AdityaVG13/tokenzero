use crate::*;

#[derive(Default)]
pub(crate) struct SearchStats {
    pub(crate) visited_files: usize,
    pub(crate) matched_files: usize,
    pub(crate) matched_lines: usize,
    pub(crate) truncated_by_results: bool,
    pub(crate) truncated_by_visit: bool,
    /// Host-op wall deadline exceeded mid-walk (CodeMode hard_max_wall_ms).
    pub(crate) truncated_by_wall: bool,
    /// rg output rows that did not parse back into matches — a parity canary
    /// (silent row loss would otherwise read as "no match there").
    pub(crate) unparsed_rows: usize,
}

/// Hard recursion bound for the internal directory walkers. Deep enough for
/// any real source tree; a backstop so a symlink cycle cannot blow the stack
/// even if the per-entry symlink skip is ever bypassed.
pub(crate) const MAX_WALK_DEPTH: usize = 64;

/// True when the path is a symlink. The walkers must not follow symlinks: a
/// cycle inside an allowed root would otherwise recurse until the stack or
/// the wall-clock budget is exhausted (`collect_tree` is depth-bounded; these
/// walkers historically were not). symlink_metadata does not traverse.
pub(crate) fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

fn sorted_entries(path: &Path) -> Option<Vec<PathBuf>> {
    let mut entries: Vec<PathBuf> = fs::read_dir(path)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    Some(entries)
}

pub(crate) fn max_search_visited_files(max_results: usize) -> usize {
    if max_results == 0 {
        return 0;
    }
    max_results
        .saturating_mul(SEARCH_VISIT_MULTIPLIER)
        .clamp(MIN_SEARCH_VISITED_FILES, MAX_SEARCH_VISITED_FILES)
}

pub struct SearchMatch {
    pub base: String,
    pub path: String,
    pub rel: String,
    pub line: usize,
    pub text: String,
}

/// Context lines inlined on each side of a hit, matching FSZero's
/// TARGET_CONTEXT_LINES policy for one-call actionable discovery results.
const TARGET_CONTEXT_LINES: usize = 2;

/// Enclosing-symbol inference mirroring FSZero's `enclosing_symbol()`
/// (FSZero src/core/target_ref.rs): nearest declarator line at or above the
/// hit, declarator head capped at 80 chars; None => the grammar's truthful
/// `(file-scope)` fallback. Kept byte-compatible with FSZero's DECLARATORS.
fn enclosing_symbol(lines: &[String], line_no: usize) -> Option<String> {
    const DECLARATORS: &[&str] = &[
        "fn ",
        "pub fn ",
        "async fn ",
        "struct ",
        "enum ",
        "impl ",
        "trait ",
        "mod ",
        "class ",
        "def ",
        "function ",
        "type ",
        "const ",
        "static ",
    ];
    for line in lines[..line_no.min(lines.len())].iter().rev() {
        let trimmed = line.trim();
        if DECLARATORS.iter().any(|d| trimmed.starts_with(d)) {
            let head = trimmed.trim_end_matches(['{', ' ']);
            return Some(head.chars().take(80).collect());
        }
    }
    None
}

/// FSZero snap-to-file hit rendering (FSZero docs/design/target-ref-grammar.md):
/// every distinct target window becomes one `HIT <path>#L<start>-L<end>
/// kind=<kind> sym=<sym>` header plus an inlined `| <line-no>: <text>` window
/// covering the matched line and TARGET_CONTEXT_LINES on each side, so agents
/// snap to file:line without a second discovery call. Byte-identical windows
/// within one file are emitted once (5irj): adjacent matches whose context
/// windows overlap or clamp to the same range render one HIT record while
/// every matching line stays visible. Distinct windows and distinct enclosing
/// symbols remain distinct. Each file is read once for all of its hits;
/// unreadable files fall back to the matched line only.
pub(crate) fn hit_search_output(matches: &[SearchMatch], kind: &str) -> String {
    let mut out = String::new();
    let mut idx = 0;
    while idx < matches.len() {
        let m = &matches[idx];
        let mut end = idx + 1;
        while end < matches.len() && matches[end].path == m.path {
            end += 1;
        }
        let file_lines: Option<Vec<String>> = std::fs::read_to_string(&m.path)
            .ok()
            .map(|content| content.lines().map(str::to_string).collect());
        // 5irj: stable per-file dedupe key (start, stop, kind, sym,
        // fallback_text). kind is uniform for the whole call, so comparing it
        // is a no-op, but it keeps the key explicit and future-proof if a
        // mixed-kind call ever lands. fallback_text is None for readable files
        // and Some(hit.text) for unreadable ones, so same path/line/kind/sym
        // records whose emitted `| line: text` differs stay distinct.
        let mut emitted: Vec<(usize, usize, &str, String, Option<String>)> = Vec::new();
        for hit in &matches[idx..end] {
            let line = hit.line.max(1);
            let (start, stop) = match &file_lines {
                Some(lines) if !lines.is_empty() => (
                    line.saturating_sub(TARGET_CONTEXT_LINES).max(1),
                    (line + TARGET_CONTEXT_LINES).min(lines.len()),
                ),
                _ => (line, line),
            };
            let fallback_text: Option<String> = match &file_lines {
                Some(lines) if !lines.is_empty() => None,
                _ => Some(hit.text.clone()),
            };
            // 631q: carry the inferred enclosing symbol when the file is
            // readable; unreadable/binary files keep (file-scope).
            let sym = match &file_lines {
                Some(lines) if !lines.is_empty() => {
                    enclosing_symbol(lines, line).unwrap_or_else(|| "(file-scope)".to_string())
                }
                _ => "(file-scope)".to_string(),
            };
            if emitted
                .iter()
                .any(|(e_start, e_stop, e_kind, e_sym, e_fallback)| {
                    *e_start == start
                        && *e_stop == stop
                        && *e_kind == kind
                        && *e_sym == sym
                        && *e_fallback == fallback_text
                })
            {
                continue;
            }
            emitted.push((start, stop, kind, sym.clone(), fallback_text));
            out.push_str(&format!(
                "HIT {}#L{}-L{} kind={} sym={}\n",
                hit.path, start, stop, kind, sym
            ));
            match &file_lines {
                Some(lines) if !lines.is_empty() => {
                    for no in start..=stop {
                        let text = lines.get(no - 1).map(String::as_str).unwrap_or("");
                        out.push_str(&format!("| {}: {}\n", no, text));
                    }
                }
                _ => out.push_str(&format!("| {}: {}\n", line, hit.text)),
            }
        }
        idx = end;
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

pub(crate) fn flat_search_output(matches: &[SearchMatch]) -> String {
    matches
        .iter()
        .map(|m| format!("{}:{}:{}", m.path, m.line, m.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lossless compact projection of glob matches as an indented prefix trie.
///
/// Root and component labels are JSON strings. A trailing `/` marks a
/// directory component and two spaces encode one level. This keeps whitespace,
/// newlines, separator-like characters, and Unicode unambiguous while emitting
/// each shared directory prefix only once. Roots are sorted and deduplicated,
/// and overlapping paths bind to the most-specific root, so caller ordering
/// cannot change the bytes. Paths outside every declared root remain full JSON
/// strings after an explicit marker.
pub(crate) fn grouped_path_output(paths: &[PathBuf], roots: &[PathBuf]) -> String {
    let mut canonical_roots = roots.to_vec();
    canonical_roots.sort_by(|left, right| {
        display_path(left)
            .cmp(&display_path(right))
            .then_with(|| left.cmp(right))
    });
    canonical_roots.dedup();

    let mut sections: Vec<Vec<Vec<String>>> = vec![Vec::new(); canonical_roots.len()];
    let mut leftovers: Vec<String> = Vec::new();
    for path in paths {
        let mut selected: Option<(usize, usize, Vec<String>)> = None;
        for (idx, root) in canonical_roots.iter().enumerate() {
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let components = rel
                .components()
                .filter_map(|component| match component {
                    std::path::Component::Normal(value) => {
                        Some(value.to_string_lossy().into_owned())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            if components.is_empty() {
                continue;
            }
            let specificity = root.components().count();
            let replace = match &selected {
                Some((_, best_specificity, _)) => specificity > *best_specificity,
                None => true,
            };
            if replace {
                selected = Some((idx, specificity, components));
            }
        }
        if let Some((idx, _, components)) = selected {
            sections[idx].push(components);
        } else {
            leftovers.push(display_path(path));
        }
    }

    let mut lines = Vec::new();
    for (root, mut rows) in canonical_roots.iter().zip(sections) {
        if rows.is_empty() {
            continue;
        }
        rows.sort();
        lines.push(format!(
            "# root: {}",
            serde_json::to_string(&display_path(root)).expect("path display is serializable")
        ));
        let mut previous_dirs: Vec<String> = Vec::new();
        for components in rows {
            let (dirs, file) = components.split_at(components.len() - 1);
            let shared = dirs
                .iter()
                .zip(&previous_dirs)
                .take_while(|(left, right)| left == right)
                .count();
            for (depth, component) in dirs.iter().enumerate().skip(shared) {
                let label = serde_json::to_string(component)
                    .expect("path component display is serializable");
                lines.push(format!("{}{label}/", "  ".repeat(depth)));
            }
            let label =
                serde_json::to_string(&file[0]).expect("path component display is serializable");
            lines.push(format!("{}{label}", "  ".repeat(dirs.len())));
            previous_dirs = dirs.to_vec();
        }
    }
    if !leftovers.is_empty() {
        leftovers.sort();
        lines.push("# outside-roots".to_string());
        lines.extend(
            leftovers
                .into_iter()
                .map(|path| serde_json::to_string(&path).expect("path display is serializable")),
        );
    }
    lines.join("\n")
}

pub(crate) fn grouped_tree_output(
    entries: &[TreeEntry],
    spans: &[(String, usize)],
    with_headers: bool,
) -> String {
    let mut lines = Vec::new();
    for (idx, (root, start)) in spans.iter().enumerate() {
        let end = spans.get(idx + 1).map_or(entries.len(), |next| next.1);
        if *start == end {
            continue;
        }
        if with_headers {
            lines.push(format!("# root: {root}"));
        }
        for entry in &entries[*start..end] {
            let suffix = if entry.dir { "/" } else { "" };
            lines.push(format!(
                "{}{}{}",
                "  ".repeat(entry.depth),
                entry.name,
                suffix
            ));
        }
    }
    lines.join("\n")
}

/// Echo at most one short line of the caller's query in a zero-hit note:
/// multi-line queries can never match (search is per-line) and long queries
/// would make the note cost O(query) for a 0-token payload. Mirrors the
/// label compaction capsule headers already apply.
pub(crate) fn zero_hit_label(query: &str) -> String {
    const MAX_LABEL_CHARS: usize = 48;
    let first_line = query.lines().next().unwrap_or("");
    let truncated: String = first_line.chars().take(MAX_LABEL_CHARS).collect();
    if truncated.chars().count() < query.chars().count() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

/// Empty search-family results otherwise render as a bare `refs:` footer with
/// no signal that the call succeeded and found nothing. Replace the empty
/// visible text with a one-line zero-hit note and account for its cost.
/// Passthrough keeps its verbatim-payload contract; non-empty text (e.g.
/// exact-mode ref lines) is never displaced.
pub(crate) fn apply_zero_hit_note(response: &mut ToolResponse, mode: Mode, note: String) {
    if matches!(mode, Mode::Passthrough) {
        return;
    }
    let Some(visible) = response.visible.as_mut() else {
        return;
    };
    if !visible.text.trim().is_empty() {
        return;
    }
    let note_tokens = count_tokens(&note);
    visible.text = note;
    if let Some(accounting) = response.accounting.as_mut() {
        accounting.visible_tokens = note_tokens;
    }
}

/// Merge extra telemetry keys into the response without clobbering an
/// existing telemetry object (degraded-storage and search-backend markers
/// must survive a dedup/diff serve).
pub(crate) fn merge_telemetry(response: &mut ToolResponse, extra: Value) {
    let Value::Object(extra) = extra else {
        return;
    };
    match response.telemetry.as_mut() {
        Some(Value::Object(existing)) => existing.extend(extra),
        _ => response.telemetry = Some(Value::Object(extra)),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_search(
    base: &Path,
    current: &Path,
    query: &str,
    max_results: usize,
    max_visited_files: usize,
    depth: usize,
    stats: &mut SearchStats,
    matches: &mut Vec<SearchMatch>,
) {
    if stats.truncated_by_wall {
        return;
    }
    if matches.len() >= max_results {
        stats.truncated_by_results = true;
        return;
    }
    if stats.visited_files >= max_visited_files {
        stats.truncated_by_visit = true;
        return;
    }
    if depth == 0 {
        return;
    }
    if current.is_file() {
        stats.visited_files += 1;
        if crate::wall::check_active_wall_deadline_every(
            stats.visited_files,
            crate::wall::WALL_CHECK_EVERY_N,
        )
        .is_some()
        {
            stats.truncated_by_wall = true;
            return;
        }
        if let Ok(bytes) = fs::read(current) {
            let text = String::from_utf8_lossy(&bytes);
            let before = matches.len();
            let path_display = current.display().to_string();
            let rel_display = current
                .strip_prefix(base)
                .ok()
                .filter(|rel| !rel.as_os_str().is_empty())
                .map(|rel| rel.display().to_string())
                .unwrap_or_else(|| path_display.clone());
            for (idx, line) in text.lines().enumerate() {
                if line.contains(query) {
                    if matches.len() >= max_results {
                        stats.truncated_by_results = true;
                        break;
                    }
                    matches.push(SearchMatch {
                        base: base.display().to_string(),
                        path: path_display.clone(),
                        rel: rel_display.clone(),
                        line: idx + 1,
                        text: line.to_string(),
                    });
                    stats.matched_lines += 1;
                }
            }
            if matches.len() > before {
                stats.matched_files += 1;
            }
        }
        return;
    }
    let Some(entries) = sorted_entries(current) else {
        return;
    };
    for path in entries {
        if should_skip(&path, false) || is_symlink(&path) {
            continue;
        }
        collect_search(
            base,
            &path,
            query,
            max_results,
            max_visited_files,
            depth - 1,
            stats,
            matches,
        );
        if stats.truncated_by_results || stats.truncated_by_visit || stats.truncated_by_wall {
            break;
        }
    }
}

#[derive(Debug)]
pub(crate) enum RgFailure {
    /// rg rejected the pattern (regex parse error); a tool error, not a
    /// fallback, because the internal scanner's substring semantics would
    /// silently return different results.
    InvalidPattern(String),
    /// rg is missing, failed to spawn, or exited with an unexpected status;
    /// the caller falls back to the internal scanner.
    Unavailable(String),
}

/// Portable rg discovery: env → PATH → well-known layouts (wqw.3).
pub fn find_rg_in_path() -> Option<PathBuf> {
    crate::binary_resolve::resolve_rg_binary()
        .ok()
        .map(|resolved| resolved.path)
}

/// Poll interval for the unbounded rg exit wait.
const RG_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);
/// Bounded final wait for the tree sweep after the root exited.
const RG_FINAL_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn spawn_rg_output_reader(
    mut reader: impl std::io::Read + Send + 'static,
) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

/// Run ripgrep per root and map its `path:line:text` output onto the same
/// `SearchMatch` rows the internal scanner produces. `find` keeps substring
/// semantics via `--fixed-strings`; `grep` passes the pattern as a regex.
pub(crate) fn rg_search(
    rg: &Path,
    tool: &str,
    query: &str,
    roots: &[PathBuf],
    max_results: usize,
) -> Result<(Vec<SearchMatch>, SearchStats), RgFailure> {
    let mut matches: Vec<SearchMatch> = Vec::new();
    let mut stats = SearchStats::default();
    for (root_idx, root) in roots.iter().enumerate() {
        if crate::wall::check_active_wall_deadline_every(root_idx, 1).is_some() {
            stats.truncated_by_wall = true;
            break;
        }
        if matches.len() >= max_results {
            stats.truncated_by_results = true;
            break;
        }
        let mut command = std::process::Command::new(rg);
        command.args([
            "--line-number",
            "--no-heading",
            "--color=never",
            "--no-messages",
            "--hidden",
            "--no-ignore",
            "--with-filename",
            // Multi-tenant hosts may run many TokenZero sessions. Cap rg's
            // internal fanout so one find cannot saturate the machine; the
            // machine-wide analysis permit then bounds how many such searches
            // run at once.
            "--threads",
            "1",
        ]);
        // Mirror the internal scanner's skip list (`should_skip` with hidden
        // entries excluded): `!.*` also keeps the `.tokenzero` recovery cache
        // out of results.
        for skip in ["!.*", "!target", "!__pycache__"] {
            command.args(["--glob", skip]);
        }
        if tool == "find" {
            command.arg("--fixed-strings");
        }
        command
            .arg("--")
            .arg(query)
            .arg(root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        // Hub-owned spawn: rg runs single-threaded with no subprocess tree of
        // its own, but cancellation still signals through the exact owned
        // handle under the TokenZero engine binding — never a numeric pid.
        let _dispatch_child_scope = crate::engine_shell::dispatch_child_scope();
        let (verified, pipes) = zero_process::VerifiedChild::spawn_tree_with_pipes(
            command,
            tokenzero_runtime::PROCESS_OWNER_SESSION,
            tokenzero_runtime::PROCESS_GENERATION,
        )
        .map_err(|err| RgFailure::Unavailable(format!("rg spawn failed: {err}")))?;
        crate::engine_shell::publish_dispatch_child(&verified);
        // Register the child so raw-worker v2 cancellation can stop a long
        // search (pid is observation evidence only).
        crate::shell_hooks::note_child(Some(verified.child_id()), None, "running");
        let stdout_reader =
            spawn_rg_output_reader(pipes.stdout.expect("rg stdout pipe configured above"));
        let stderr_reader =
            spawn_rg_output_reader(pipes.stderr.expect("rg stderr pipe configured above"));
        // Mirror wait_with_output: unbounded run, bounded teardown. Cancel and
        // session death signal the owned handle, so the wait ends inside the
        // declared bound.
        loop {
            if verified.wait_for_exit(RG_POLL_INTERVAL) {
                break;
            }
        }
        let status = if let Some(status) = verified.terminal_status() {
            Ok(status)
        } else {
            verified
                .wait(
                    tokenzero_runtime::PROCESS_OWNER_SESSION,
                    tokenzero_runtime::PROCESS_GENERATION,
                    RG_FINAL_WAIT_TIMEOUT,
                    tokenzero_runtime::SHELL_TEARDOWN_GRACE,
                )
                .map_err(|error| RgFailure::Unavailable(format!("rg teardown failed: {error}")))
        };
        if status.is_err() {
            let _ = verified.signal_graceful_for(
                tokenzero_runtime::PROCESS_OWNER_SESSION,
                tokenzero_runtime::PROCESS_GENERATION,
                tokenzero_runtime::SHELL_TEARDOWN_GRACE,
            );
            let _ = verified.revoke();
        }
        // Join both readers before surfacing either reader or teardown error.
        // This prevents one failed join from detaching the other pipe reader.
        let stdout = stdout_reader
            .join()
            .map_err(|_| RgFailure::Unavailable("rg stdout reader panicked".to_string()))
            .and_then(|result| {
                result
                    .map_err(|err| RgFailure::Unavailable(format!("rg stdout read failed: {err}")))
            });
        let stderr = stderr_reader
            .join()
            .map_err(|_| RgFailure::Unavailable("rg stderr reader panicked".to_string()))
            .and_then(|result| {
                result
                    .map_err(|err| RgFailure::Unavailable(format!("rg stderr read failed: {err}")))
            });
        crate::engine_shell::clear_dispatch_child();
        let status = status?;
        let stdout = stdout?;
        let stderr = stderr?;
        let output = std::process::Output {
            status,
            stdout,
            stderr,
        };
        match output.status.code() {
            Some(0) => {}
            // Exit code 1 is rg's "searched fine, found nothing".
            Some(1) => continue,
            code => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stderr = stderr.trim();
                if tool == "grep" && stderr.contains("regex parse error") {
                    return Err(RgFailure::InvalidPattern(stderr.to_string()));
                }
                return Err(RgFailure::Unavailable(format!(
                    "rg exited with {code:?}: {}",
                    preview(stderr)
                )));
            }
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let base = root.display().to_string();
        let mut root_matches: Vec<SearchMatch> = stdout
            .lines()
            .filter(|line| !line.is_empty())
            .filter_map(|line| {
                let parsed = parse_rg_line(line, &base);
                if parsed.is_none() {
                    stats.unparsed_rows += 1;
                }
                parsed
            })
            .collect();
        // rg's parallel traversal emits files in nondeterministic order; sort
        // component-wise to match the internal scanner's sorted DFS so both
        // backends produce byte-identical flat output (and the same prefix
        // under truncation).
        root_matches.sort_by(|a, b| {
            Path::new(&a.path)
                .cmp(Path::new(&b.path))
                .then(a.line.cmp(&b.line))
        });
        for row in root_matches {
            if matches.len() >= max_results {
                stats.truncated_by_results = true;
                break;
            }
            matches.push(row);
        }
        if stats.truncated_by_results {
            break;
        }
    }
    stats.matched_lines = matches.len();
    let mut paths: Vec<&str> = matches.iter().map(|m| m.path.as_str()).collect();
    paths.dedup();
    stats.matched_files = paths.len();
    // rg does not report how many files it scanned; the matched-file count is
    // the only honest lower bound available for visited_files.
    stats.visited_files = stats.matched_files;
    Ok((matches, stats))
}

/// Parse one `path:line:text` row of rg output. The known root prefix is
/// stripped before splitting so Windows drive colons (and roots that contain
/// `:`) never confuse the parse; only the relative remainder is examined.
pub fn parse_rg_line(line: &str, base: &str) -> Option<SearchMatch> {
    let rest = line.strip_prefix(base)?;
    if let Some(tail) = rest.strip_prefix(':') {
        // Root is the matched file itself: "<base>:<line>:<text>"; rel falls
        // back to the full path exactly like collect_search.
        let mut fields = tail.splitn(2, ':');
        let line_number = fields.next()?.parse::<usize>().ok()?;
        let text = fields.next().unwrap_or("");
        return Some(SearchMatch {
            base: base.to_string(),
            path: base.to_string(),
            rel: base.to_string(),
            line: line_number,
            text: text.to_string(),
        });
    }
    let tail = rest.strip_prefix(std::path::MAIN_SEPARATOR).unwrap_or(rest);
    // A relative path may itself contain `:` (legal on unix), making rg's
    // text format ambiguous. Scan `:<digits>:` boundaries left to right and
    // prefer the first whose prefix exists as a file under the root — the
    // only reliable disambiguator; fall back to the first parseable
    // boundary when nothing verifies (deleted-mid-search files).
    let (rel_end, line_number, text_start) = find_rg_field_boundary(tail, Path::new(base))?;
    let rel = &tail[..rel_end];
    let path_end = line.len() - tail.len() + rel.len();
    Some(SearchMatch {
        base: base.to_string(),
        path: line[..path_end].to_string(),
        rel: rel.to_string(),
        line: line_number,
        text: tail[text_start..].to_string(),
    })
}

fn find_rg_field_boundary(tail: &str, root: &Path) -> Option<(usize, usize, usize)> {
    let mut search_from = 0;
    let mut chosen = None;
    while let Some(offset) = tail[search_from..].find(':') {
        let rel_end = search_from + offset;
        let after = &tail[rel_end + 1..];
        if let Some((second, line)) = after
            .find(':')
            .and_then(|second| after[..second].parse().ok().map(|line| (second, line)))
        {
            let candidate = (rel_end, line, rel_end + second + 2);
            chosen.get_or_insert(candidate);
            if root.join(&tail[..rel_end]).is_file() {
                return Some(candidate);
            }
        }
        search_from = rel_end + 1;
    }
    chosen
}

pub(crate) struct TreeEntry {
    pub(crate) rel: String,
    pub(crate) name: String,
    pub(crate) depth: usize,
    pub(crate) dir: bool,
}

pub(crate) fn collect_tree(
    root: &Path,
    current: &Path,
    depth: usize,
    include_hidden: bool,
    max_files: usize,
    level: usize,
    rows: &mut Vec<TreeEntry>,
) {
    if rows.len() >= max_files || depth == 0 {
        return;
    }
    let Some(entries) = sorted_entries(current) else {
        return;
    };
    for path in entries {
        if rows.len() >= max_files || should_skip(&path, include_hidden) {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| rel.display().to_string());
        let dir = path.is_dir();
        rows.push(TreeEntry {
            rel: rel.display().to_string(),
            name,
            depth: level,
            dir,
        });
        if dir {
            collect_tree(
                root,
                &path,
                depth - 1,
                include_hidden,
                max_files,
                level + 1,
                rows,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_glob(
    root: &Path,
    current: &Path,
    matcher: &GlobMatcher,
    pattern_has_separator: bool,
    include_hidden: bool,
    max_files: usize,
    depth: usize,
    rows: &mut Vec<PathBuf>,
) {
    if rows.len() >= max_files || depth == 0 {
        return;
    }
    if current.is_file() {
        if glob_matches(root, current, matcher, pattern_has_separator) {
            rows.push(current.to_path_buf());
        }
        return;
    }
    let Some(entries) = sorted_entries(current) else {
        return;
    };
    for path in entries {
        if rows.len() >= max_files || should_skip(&path, include_hidden) || is_symlink(&path) {
            continue;
        }
        if path.is_dir() {
            collect_glob(
                root,
                &path,
                matcher,
                pattern_has_separator,
                include_hidden,
                max_files,
                depth - 1,
                rows,
            );
        } else if glob_matches(root, &path, matcher, pattern_has_separator) {
            rows.push(path);
        }
    }
}

pub(crate) fn display_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

pub(crate) fn glob_matches(
    root: &Path,
    path: &Path,
    matcher: &GlobMatcher,
    pattern_has_separator: bool,
) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    matcher.is_match(rel)
        || matcher.is_match(path)
        || (!pattern_has_separator
            && path
                .file_name()
                .is_some_and(|file_name| matcher.is_match(Path::new(file_name))))
}

pub(crate) fn should_skip(path: &Path, include_hidden: bool) -> bool {
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or_default();
    matches!(name, ".git" | "target" | ".venv" | "__pycache__")
        || (!include_hidden && name.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_grouped_paths(rendered: &str) -> Vec<PathBuf> {
        let mut root: Option<PathBuf> = None;
        let mut directories: Vec<String> = Vec::new();
        let mut outside_roots = false;
        let mut paths = Vec::new();
        for line in rendered.lines() {
            if let Some(encoded) = line.strip_prefix("# root: ") {
                let decoded: String = serde_json::from_str(encoded).unwrap();
                root = Some(PathBuf::from(decoded));
                directories.clear();
                outside_roots = false;
                continue;
            }
            if line == "# outside-roots" {
                root = None;
                directories.clear();
                outside_roots = true;
                continue;
            }
            if outside_roots {
                let decoded: String = serde_json::from_str(line).unwrap();
                paths.push(PathBuf::from(decoded));
                continue;
            }
            let spaces = line.bytes().take_while(|byte| *byte == b' ').count();
            assert_eq!(spaces % 2, 0, "indentation must use two spaces: {line:?}");
            let depth = spaces / 2;
            let encoded = &line[spaces..];
            let is_directory = encoded.ends_with('/');
            let encoded = encoded.strip_suffix('/').unwrap_or(encoded);
            let component: String = serde_json::from_str(encoded).unwrap();
            directories.truncate(depth);
            if is_directory {
                assert_eq!(directories.len(), depth);
                directories.push(component);
                continue;
            }
            assert_eq!(directories.len(), depth);
            let mut path = root.clone().expect("path row requires a root header");
            for directory in &directories {
                path.push(directory);
            }
            path.push(component);
            paths.push(path);
        }
        paths
    }

    #[test]
    fn grouped_path_output_round_trips_escaped_prefix_trie() {
        let root = PathBuf::from("workspace root");
        let second_root = PathBuf::from("unicode-root");
        let mut paths = vec![
            root.join("src").join(" leading space.rs"),
            root.join("src").join("line\nbreak.rs"),
            root.join("src").join("quote\"name.rs"),
            root.join("src").join("nested[name]").join("µ.rs"),
            second_root.join("δ").join("tail.rs"),
            PathBuf::from("outside").join("orphan.rs"),
        ];
        let mut expected = paths.clone();
        expected.sort();
        paths.reverse();
        let roots = vec![root, second_root];
        let rendered = grouped_path_output(&paths, &roots);
        assert_eq!(rendered, grouped_path_output(&expected, &roots));
        let mut decoded = decode_grouped_paths(&rendered);
        decoded.sort();
        assert_eq!(decoded, expected);
        assert_eq!(rendered.matches("\"src\"/").count(), 1);
        assert!(rendered.contains("line\\nbreak.rs"));
        assert!(rendered.contains("quote\\\"name.rs"));
        assert!(rendered.contains("µ.rs"));
        assert!(rendered.contains("# outside-roots"));
    }

    #[test]
    fn grouped_path_output_canonicalizes_roots_and_uses_most_specific_match() {
        let broad = PathBuf::from("workspace");
        let nested = broad.join("src");
        let disjoint = PathBuf::from("other");
        let mut expected = vec![
            nested.join("lib.rs"),
            broad.join("README.md"),
            disjoint.join("tail.rs"),
        ];
        let mut reversed_paths = expected.clone();
        reversed_paths.reverse();

        let canonical = grouped_path_output(
            &expected,
            &[
                broad.clone(),
                nested.clone(),
                disjoint.clone(),
                broad.clone(),
            ],
        );
        let permuted = grouped_path_output(&reversed_paths, &[disjoint, nested.clone(), broad]);
        assert_eq!(canonical, permuted);
        assert_eq!(canonical.matches("# root: ").count(), 3);

        let nested_header = format!(
            "# root: {}\n\"lib.rs\"",
            serde_json::to_string(&display_path(&nested)).unwrap()
        );
        assert!(
            canonical.contains(&nested_header),
            "nested file must bind to its most-specific root: {canonical}"
        );

        let mut decoded = decode_grouped_paths(&canonical);
        decoded.sort();
        expected.sort();
        assert_eq!(decoded, expected);
    }

    #[test]
    fn hit_output_matches_fszero_target_ref_grammar() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("demo.rs");
        std::fs::write(
            &file,
            "fn a() {}\nfn b() {}\nneedle here\nfn c() {}\nfn d() {}\n",
        )
        .unwrap();
        let path = file.display().to_string();
        let matches = vec![SearchMatch {
            base: dir.path().display().to_string(),
            path: path.clone(),
            rel: "demo.rs".to_string(),
            line: 3,
            text: "needle here".to_string(),
        }];
        let rendered = hit_search_output(&matches, "literal");
        // 631q: the nearest declarator at/above the hit (fn b() at L2) is the
        // enclosing symbol, matching FSZero's enclosing_symbol() inference.
        let expected = format!(
            "HIT {path}#L1-L5 kind=literal sym=fn b() {{}}\n\
             | 1: fn a() {{}}\n\
             | 2: fn b() {{}}\n\
             | 3: needle here\n\
             | 4: fn c() {{}}\n\
             | 5: fn d() {{}}"
        );
        assert_eq!(rendered, expected);
    }

    #[test]
    fn hit_output_infers_enclosing_symbol_for_function_body() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.rs");
        std::fs::write(
            &file,
            "use std::io;\n\npub fn parse_config() {\n    let a = 1;\n    let needle_line = a;\n    println!(\"{}\", needle_line);\n}\n",
        )
        .unwrap();
        let path = file.display().to_string();
        let matches = vec![SearchMatch {
            base: dir.path().display().to_string(),
            path: path.clone(),
            rel: "config.rs".to_string(),
            line: 5,
            text: "    let needle_line = a;".to_string(),
        }];
        let rendered = hit_search_output(&matches, "literal");
        // Declarator head drops the trailing " {" like FSZero does.
        assert!(
            rendered.starts_with(&format!(
                "HIT {path}#L3-L7 kind=literal sym=pub fn parse_config()\n"
            )),
            "{rendered}"
        );
    }

    #[test]
    fn hit_output_infers_python_def_and_hit_on_declarator_line() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("app.py");
        std::fs::write(
            &file,
            "import os\n\ndef handle():\n    needle = 1\n    return needle\n",
        )
        .unwrap();
        let path = file.display().to_string();
        let matches = vec![SearchMatch {
            base: dir.path().display().to_string(),
            path: path.clone(),
            rel: "app.py".to_string(),
            line: 4,
            text: "    needle = 1".to_string(),
        }];
        let rendered = hit_search_output(&matches, "regex");
        assert!(
            rendered.starts_with(&format!("HIT {path}#L2-L5 kind=regex sym=def handle():\n")),
            "{rendered}"
        );
        // A hit ON the declarator line itself reports that declarator.
        let matches = vec![SearchMatch {
            base: dir.path().display().to_string(),
            path: path.clone(),
            rel: "app.py".to_string(),
            line: 3,
            text: "def handle():".to_string(),
        }];
        let rendered = hit_search_output(&matches, "literal");
        assert!(
            rendered.starts_with(&format!(
                "HIT {path}#L1-L5 kind=literal sym=def handle():\n"
            )),
            "{rendered}"
        );
    }

    #[test]
    fn hit_output_file_scope_when_no_declarator_above() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        std::fs::write(&file, "# comment\nneedle here\nfn x() {}\n").unwrap();
        let path = file.display().to_string();
        let matches = vec![SearchMatch {
            base: dir.path().display().to_string(),
            path: path.clone(),
            rel: "notes.txt".to_string(),
            line: 2,
            text: "needle here".to_string(),
        }];
        let rendered = hit_search_output(&matches, "literal");
        assert!(
            rendered.starts_with(&format!("HIT {path}#L1-L3 kind=literal sym=(file-scope)\n")),
            "{rendered}"
        );
    }

    #[test]
    fn hit_output_falls_back_to_matched_line_when_file_unreadable() {
        let matches = vec![SearchMatch {
            base: "/base".to_string(),
            path: "/base/gone.txt".to_string(),
            rel: "gone.txt".to_string(),
            line: 7,
            text: "hit text".to_string(),
        }];
        let rendered = hit_search_output(&matches, "regex");
        assert_eq!(
            rendered,
            "HIT /base/gone.txt#L7-L7 kind=regex sym=(file-scope)\n| 7: hit text"
        );
    }

    #[test]
    fn adjacent_matches_sharing_a_context_window_emit_one_hit_record() {
        // 5irj: the original two-match find fixture (alpha at L1 and alphabet
        // at L3 in a 3-line file) clamps both TARGET_CONTEXT_LINES=2 windows
        // to L1-L3. The byte-identical windows must collapse to exactly one
        // HIT header while every matching line stays visible.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tiny.txt");
        std::fs::write(&file, "alpha\nbeta\nalphabet\n").unwrap();
        let path = file.display().to_string();
        let root = dir.path().display().to_string();
        let matches = vec![
            SearchMatch {
                base: root.clone(),
                path: path.clone(),
                rel: "tiny.txt".to_string(),
                line: 1,
                text: "alpha".to_string(),
            },
            SearchMatch {
                base: root,
                path: path.clone(),
                rel: "tiny.txt".to_string(),
                line: 3,
                text: "alphabet".to_string(),
            },
        ];
        let rendered = hit_search_output(&matches, "literal");
        assert_eq!(rendered.matches("HIT ").count(), 1, "{rendered}");
        assert_eq!(
            rendered,
            format!(
                "HIT {path}#L1-L3 kind=literal sym=(file-scope)\n| 1: alpha\n| 2: beta\n| 3: alphabet"
            )
        );
    }

    #[test]
    fn distinct_windows_and_symbols_stay_distinct() {
        // 5irj: dedupe must only collapse byte-identical (start, stop, kind,
        // sym) windows. Hits with different windows or different enclosing
        // symbols keep their own HIT records.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("wide.rs");
        std::fs::write(
            &file,
            "fn a() {}\nfirst hit here\nfn b() {}\nsecond hit here\nfn c() {}\n",
        )
        .unwrap();
        let path = file.display().to_string();
        let root = dir.path().display().to_string();
        let matches = vec![
            SearchMatch {
                base: root.clone(),
                path: path.clone(),
                rel: "wide.rs".to_string(),
                line: 2,
                text: "first hit here".to_string(),
            },
            SearchMatch {
                base: root,
                path: path.clone(),
                rel: "wide.rs".to_string(),
                line: 4,
                text: "second hit here".to_string(),
            },
        ];
        let rendered = hit_search_output(&matches, "literal");
        assert_eq!(rendered.matches("HIT ").count(), 2, "{rendered}");
        assert!(rendered.contains("sym=fn a() {}"), "{rendered}");
        assert!(rendered.contains("sym=fn b() {}"), "{rendered}");
        assert!(rendered.contains("| 2: first hit here"), "{rendered}");
        assert!(rendered.contains("| 4: second hit here"), "{rendered}");
    }
}
