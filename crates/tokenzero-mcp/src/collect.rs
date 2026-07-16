use crate::*;

#[derive(Default)]
pub(crate) struct SearchStats {
    pub(crate) visited_files: usize,
    pub(crate) matched_files: usize,
    pub(crate) matched_lines: usize,
    pub(crate) truncated_by_results: bool,
    pub(crate) truncated_by_visit: bool,
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

pub(crate) struct SearchMatch {
    pub(crate) base: String,
    pub(crate) path: String,
    pub(crate) rel: String,
    pub(crate) line: usize,
    pub(crate) text: String,
}

pub(crate) fn flat_search_output(matches: &[SearchMatch]) -> String {
    matches
        .iter()
        .map(|m| format!("{}:{}:{}", m.path, m.line, m.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lossless compact projection of search matches: one `# root:` header per
/// searched root, matches grouped by file with the path emitted once.
pub(crate) fn grouped_search_output(matches: &[SearchMatch]) -> String {
    let mut lines = Vec::new();
    let mut current_base: Option<&str> = None;
    let mut idx = 0;
    while idx < matches.len() {
        let m = &matches[idx];
        if current_base != Some(m.base.as_str()) {
            lines.push(format!("# root: {}", m.base));
            current_base = Some(m.base.as_str());
        }
        let mut end = idx + 1;
        while end < matches.len() && matches[end].base == m.base && matches[end].rel == m.rel {
            end += 1;
        }
        if end - idx == 1 {
            lines.push(format!("{}:{}:{}", m.rel, m.line, m.text));
        } else {
            lines.push(format!("{}:", m.rel));
            for file_match in &matches[idx..end] {
                lines.push(format!("  {}: {}", file_match.line, file_match.text));
            }
        }
        idx = end;
    }
    lines.join("\n")
}

/// Lossless compact projection of glob matches: relative paths under one
/// `# root:` header per contributing root.
pub(crate) fn grouped_path_output(paths: &[PathBuf], roots: &[PathBuf]) -> String {
    let mut sections: Vec<(String, Vec<String>)> = roots
        .iter()
        .map(|root| (root.display().to_string(), Vec::new()))
        .collect();
    let mut leftovers: Vec<String> = Vec::new();
    'outer: for path in paths {
        for (idx, root) in roots.iter().enumerate() {
            if let Ok(rel) = path.strip_prefix(root) {
                if !rel.as_os_str().is_empty() {
                    sections[idx].1.push(display_path(rel));
                    continue 'outer;
                }
            }
        }
        leftovers.push(display_path(path));
    }
    let mut lines = Vec::new();
    for (root, rows) in sections {
        if rows.is_empty() {
            continue;
        }
        lines.push(format!("# root: {root}"));
        lines.extend(rows);
    }
    lines.extend(leftovers);
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
        if stats.truncated_by_results || stats.truncated_by_visit {
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
pub(crate) fn find_rg_in_path() -> Option<PathBuf> {
    crate::binary_resolve::resolve_rg_binary()
        .ok()
        .map(|resolved| resolved.path)
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
    for root in roots {
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
        let output = command
            .arg("--")
            .arg(query)
            .arg(root)
            .output()
            .map_err(|err| RgFailure::Unavailable(format!("rg spawn failed: {err}")))?;
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
pub(crate) fn parse_rg_line(line: &str, base: &str) -> Option<SearchMatch> {
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
