use super::expand_params::ExpandParams;
use super::*;
use tokenzero_recovery::is_expandable_ref;

/// Documented cap for `raw: true` expands (yevj): exact bytes are returned up
/// to this many bytes; beyond it the expand fails typed with a fragment
/// repair hint. Env-overridable for harnesses with larger budgets.
pub const EXPAND_RAW_MAX_BYTES: usize = 256 * 1024;
pub const EXPAND_RAW_MAX_BYTES_ENV: &str = "TOKENZERO_EXPAND_RAW_MAX_BYTES";

fn expand_raw_max_bytes() -> usize {
    expand_raw_max_bytes_from(std::env::var(EXPAND_RAW_MAX_BYTES_ENV).ok().as_deref())
}

fn expand_raw_max_bytes_from(raw: Option<&str>) -> usize {
    raw.and_then(|raw| raw.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(EXPAND_RAW_MAX_BYTES)
}

/// High-precision secret patterns masked on expands where raw recovery was
/// NOT explicitly authorized (`raw` absent/false). Deliberately narrow:
/// every pattern here is an unambiguous credential shape. The stored bytes
/// are never modified; only the visible body is masked.
const SECRET_PREFIXES: &[(&str, &str)] = &[
    ("AKIA", "aws-access-key"),                 // AKIA + 16 uppercase/digits
    ("ASIA", "aws-session-key"),                // ASIA + 16 uppercase/digits
    ("ghp_", "github-pat"),                     // ghp_ + 36
    ("gho_", "github-oauth"),                   // gho_ + 36
    ("ghs_", "github-server"),                  // ghs_ + 36
    ("ghr_", "github-refresh"),                 // ghr_ + 36
    ("github_pat_", "github-fine-grained-pat"), // github_pat_ + 22+
    ("xoxb-", "slack-bot"),
    ("xoxp-", "slack-user"),
    ("xoxa-", "slack-app"),
    ("xoxr-", "slack-refresh"),
    ("xoxs-", "slack-session"),
    ("sk-", "api-key-sk"), // sk- + 20+ token chars
];

const PRIVATE_KEY_BEGIN: &str = "-----BEGIN";
const PRIVATE_KEY_MARKER: &str = "PRIVATE KEY-----";

fn is_token_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

/// Mask unambiguous credential shapes in `text`, returning the masked text
/// and the number of masked spans. Conservative by design: false positives
/// cost exactness, false negatives leak, so only credential shapes with no
/// legitimate prose reading are matched.
pub(crate) fn mask_expansion_secrets(text: &str) -> (String, usize) {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut masked = 0usize;
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        // PEM private key block: mask from BEGIN line through the END line.
        let first_line_end = text[cursor..].find('\n').unwrap_or(text.len() - cursor);
        if text[cursor..].starts_with(PRIVATE_KEY_BEGIN)
            && text[cursor..cursor + first_line_end].contains(PRIVATE_KEY_MARKER)
        {
            let after_begin = &text[cursor..];
            if let Some(end_rel) = after_begin.find("-----END") {
                let end_line_rel = after_begin[end_rel..]
                    .find('\n')
                    .map(|i| end_rel + i + 1)
                    .unwrap_or(after_begin.len());
                out.push_str("[tz-masked:private-key-block]");
                if end_line_rel < after_begin.len() {
                    out.push('\n');
                }
                cursor += end_line_rel;
                masked += 1;
                continue;
            }
        }
        let mut matched = false;
        for (prefix, kind) in SECRET_PREFIXES {
            if !text[cursor..].starts_with(prefix) {
                continue;
            }
            let run_start = cursor + prefix.len();
            let mut run_end = run_start;
            while run_end < bytes.len() && is_token_char(bytes[run_end]) {
                run_end += 1;
            }
            let run_len = run_end - run_start;
            let min_run = match *prefix {
                "AKIA" | "ASIA" => 16,
                "sk-" => 20,
                "github_pat_" => 22,
                _ => 10,
            };
            // AWS key ids are uppercase+digits only; enforce that shape.
            if (*prefix == "AKIA" || *prefix == "ASIA")
                && !bytes[run_start..run_end]
                    .iter()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            {
                continue;
            }
            if run_len >= min_run {
                out.push_str(&format!("[tz-masked:{kind}]"));
                cursor = run_end;
                masked += 1;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }
        // Copy one full character (never split a multibyte char).
        let next = (cursor + 1..=text.len())
            .find(|&index| text.is_char_boundary(index))
            .unwrap_or(text.len());
        out.push_str(&text[cursor..next]);
        cursor = next;
    }
    (out, masked)
}

fn norm_opt(value: &Option<String>) -> String {
    value.clone().unwrap_or_default()
}

fn expand_serve_key(params: &ExpandParams) -> ServeKey {
    ServeKey::Expand {
        ref_id: params.ref_id.clone(),
        start_line: params.start_line,
        end_line: params.end_line,
        selector_norm: norm_opt(&params.selector),
        symbol_norm: norm_opt(&params.symbol),
        anchor_kind_norm: norm_opt(&params.anchor_kind),
    }
}

/// Expand from the engine-owned warm store, reloading the active cache once only
/// after a local miss. A successful warm lookup remains disk-free, while refs
/// persisted by another process become visible without restarting the engine.
fn expand_with_reload_on_miss(
    store: &mut RecoveryStore,
    active_cache: &Path,
    ref_id: &str,
    selector: Option<&str>,
    start_line: Option<usize>,
    end_line: Option<usize>,
    anchor_kind: Option<&str>,
    symbol: Option<&str>,
) -> ExpansionResult {
    let result = store.expand(ref_id, selector, start_line, end_line, anchor_kind, symbol);
    if result.found {
        return result;
    }

    let mut refreshed = RecoveryStore::new(Some(active_cache.to_path_buf()));
    refreshed.recovery_count = store.recovery_count;
    refreshed.recovery_tokens = store.recovery_tokens;
    refreshed.legacy_read_count = store.legacy_read_count;
    let refreshed_result =
        refreshed.expand(ref_id, selector, start_line, end_line, anchor_kind, symbol);
    if refreshed_result.found {
        *store = refreshed;
        refreshed_result
    } else {
        result
    }
}

fn resolve_slice(
    store: &mut RecoveryStore,
    params: &ExpandParams,
    active_cache: &Path,
) -> Result<ExpansionResult, Box<ToolResponse>> {
    let selector = params.selector.as_deref().or(Some("raw"));
    let anchor = params.anchor_kind.as_deref();
    let symbol = params.symbol.as_deref();
    let result = expand_with_reload_on_miss(
        store,
        active_cache,
        &params.ref_id,
        selector,
        params.start_line,
        params.end_line,
        anchor,
        symbol,
    );
    if result.found {
        return Ok(result);
    }
    // Router-owned fallback (surface-exclusivity-1r9): try the legacy dual-
    // filename sibling store before surfacing a miss to the agent.
    if let Some(sibling_path) = legacy_sibling_holding_ref(&params.ref_id, active_cache) {
        let mut sibling = RecoveryStore::new(Some(sibling_path));
        let sibling_result = sibling.expand(
            &params.ref_id,
            selector,
            params.start_line,
            params.end_line,
            anchor,
            symbol,
        );
        if sibling_result.found {
            return Ok(sibling_result);
        }
    }
    Err(Box::new(annotate_expand_miss(
        expansion_response(result, store.recovery_tokens),
        &params.ref_id,
        active_cache,
    )))
}

/// When expand misses after internal sibling retry, name the active store.
/// If a historical sibling still reports the ref but failed to expand, keep
/// `store_mismatch` diagnostics for operators aligning `--cache-path`.
fn annotate_expand_miss(
    mut response: ToolResponse,
    ref_id: &str,
    active_cache: &Path,
) -> ToolResponse {
    let Some(err) = response.error.as_mut() else {
        return response;
    };
    if err.code != "ref_not_found" && err.code != "expand_failed" {
        return response;
    }
    let active = active_cache.display().to_string();
    if let Some(other_path) = legacy_sibling_holding_ref(ref_id, active_cache) {
        err.code = "store_mismatch".to_string();
        err.message = format!(
            "store_mismatch: ref present in {other} but not in active store {active} \
(mint and expand must share --cache-path / TOKENZERO_CACHE_PATH); original: {}",
            err.message,
            other = other_path.display(),
        );
    } else {
        err.message = format!(
            "{} [store: {active}; mint and expand must share --cache-path]",
            err.message
        );
    }
    err.message = format!("-{ref_id} (unavailable)\n{}", err.message);
    response
}

/// One-shot migration probe for the pre-wqw.8 dual-filename split only.
fn legacy_sibling_holding_ref(ref_id: &str, active_cache: &Path) -> Option<PathBuf> {
    let parent = active_cache.parent()?;
    let name = active_cache.file_name()?.to_string_lossy();
    let sibling = if name.contains("codemode-recovery") {
        parent.join("recovery-cache.json")
    } else if name.contains("recovery-cache") {
        parent.join("codemode-recovery.json")
    } else {
        return None;
    };
    if sibling == *active_cache || !sibling.is_file() {
        return None;
    }
    let other = RecoveryStore::new(Some(sibling.clone()));
    other.has_ref(ref_id).then_some(sibling)
}

impl TokenZeroEngine {
    pub fn expand_with_params(&self, params: ExpandParams) -> ToolResponse {
        let ref_id = params.ref_id.clone();
        let mut response = self.expand_with_params_inner(params);
        let ok = response.error.is_none();
        let code = response.error.as_ref().map(|err| err.code.as_str());
        // Health probe for crash-only unlock (wqw.9). invalid_ref is a client
        // mistake and does not open recovery.
        self.surface_health().record_expand_outcome(ok, code);
        if ok {
            // vz89.10: re-expansion of a session-exposed object always succeeds
            // and is marked as a session replay (recovery accounting class).
            if let Some(replays) = self.record_session_reexpansion(&ref_id) {
                let telemetry = response
                    .telemetry
                    .get_or_insert_with(|| serde_json::json!({}));
                if let Some(map) = telemetry.as_object_mut() {
                    map.insert(
                        "session_exposure_replay".to_string(),
                        serde_json::json!(replays),
                    );
                }
            }
        }
        response
    }

    fn expand_with_params_inner(&self, params: ExpandParams) -> ToolResponse {
        if let Some((message, _)) = crate::wall::check_active_wall_deadline() {
            return failure_response("expand", "hard_max_wall_ms", message, None);
        }
        if !is_expandable_ref(&params.ref_id) {
            return failure_response(
                "expand",
                "invalid_ref",
                format!(
                    "ref must start with tz://, fz://, or gz://, got: {}",
                    params.ref_id
                ),
                None,
            );
        }

        let key = expand_serve_key(&params);
        let _flight = if self.config.session_dedup {
            self.begin_serve_flight(vec![key.clone()])
        } else {
            self.begin_serve_flight(Vec::new())
        };

        let mut store = self.recovery_store();
        let mut summary = SessionSummary::default();
        let mut pending: Vec<(ServeKey, ServedRecord)> = Vec::new();

        if let Some(since_ref) = params.since.as_deref().filter(|_| !params.fresh) {
            if !is_expandable_ref(since_ref) {
                return failure_response(
                    "expand",
                    "invalid_ref",
                    format!("since must start with tz://, fz://, or gz://, got: {since_ref}"),
                    None,
                );
            }
            let since_result = crate::perf_profile::_profile_expand_resolve(|| {
                expand_with_reload_on_miss(
                    &mut store,
                    &self.config.cache_path,
                    since_ref,
                    params.selector.as_deref().or(Some("raw")),
                    params.start_line,
                    params.end_line,
                    params.anchor_kind.as_deref(),
                    params.symbol.as_deref(),
                )
            });
            if !since_result.found {
                let code = match since_result.reason.as_str() {
                    "stale-ref" => "ref_stale",
                    "dangling-ref" => "dangling_ref",
                    "invalid-ref" => "invalid_ref",
                    _ => "expand_failed",
                };
                return failure_response(
                    "expand",
                    code,
                    format!("since ref is not recoverable: {since_ref}"),
                    None,
                );
            }
            let target = match crate::perf_profile::_profile_expand_resolve(|| {
                resolve_slice(&mut store, &params, &self.config.cache_path)
            }) {
                Ok(target) => target,
                Err(response) => return *response,
            };
            self.rehydrate_working_set_expand(&mut store, &params);
            let (text, diff) = if since_result.content == target.content {
                (unchanged_since_expand_ack(since_ref), None)
            } else if let Some(render) = diff::unified_diff(&since_result.content, &target.content)
            {
                (
                    expand_since_diff_text(since_ref, &params.ref_id, &render.text),
                    Some(DiffTelemetry {
                        hunks: render.hunks,
                        plus: render.plus,
                        minus: render.minus,
                        base_ref: since_ref.to_string(),
                    }),
                )
            } else {
                (unchanged_since_expand_ack(since_ref), None)
            };
            let tokens = count_tokens(&text);
            if let Some(telemetry) = diff {
                summary.note_diff(telemetry, 0);
            }
            if self.config.session_dedup {
                pending.push(self.pending_expand_record(key, &params, &target.content, &mut store));
            }
            let mut response = success_response(
                "expand",
                Mode::Exact,
                text,
                Vec::new(),
                (
                    tokens,
                    tokens,
                    store.recovery_tokens,
                    Some(count_tokens(&params.ref_id)),
                ),
            );
            // since= diffs are terminal recovery output too: adapters must
            // not re-compact the diff body.
            response.recovery = Some(tokenzero_core::RecoveryReceipt {
                terminal: true,
                do_not_recompact: true,
                exact_bytes: false, // diff render, not verbatim bytes
            });
            response.telemetry = summary.telemetry();
            crate::perf_profile::_profile_expand_session_apply(|| {
                self.session_apply(pending, &summary);
            });
            return response;
        }

        let target = match crate::perf_profile::_profile_expand_resolve(|| {
            resolve_slice(&mut store, &params, &self.config.cache_path)
        }) {
            Ok(t) => t,
            Err(resp) => return *resp,
        };
        self.rehydrate_working_set_expand(&mut store, &params);

        // yevj: `raw: true` is the explicitly-authorized exact-bytes request
        // and is bounded by its documented cap; beyond the cap the expand
        // fails typed once with a fragment repair hint (never a silent
        // truncation).
        if params.raw && target.content.len() > expand_raw_max_bytes() {
            return failure_response(
                "expand",
                "expand_raw_cap_exceeded",
                format!(
                    "raw expand of {} is {} bytes, over the {}-byte raw cap",
                    params.ref_id,
                    target.content.len(),
                    expand_raw_max_bytes()
                ),
                Some(
                    "request a bounded window instead: append a byte fragment (#B<start>-<end>) \
                     or line window (#L<a>-L<b>) to the ref, or raise TOKENZERO_EXPAND_RAW_MAX_BYTES",
                ),
            );
        }

        // Explicit expand is the recovery contract: it ALWAYS returns exact
        // bytes. Replacing content with an "identical to … (unchanged)" ack
        // here broke byte-exact recovery (release-claim audits) and forced a
        // fresh re-call exactly when the model had decided it needed the
        // bytes — the capability-loss the compression doctrine forbids.
        // Seen-set economics stay on the implicit serve paths (read/find
        // spills) and on explicit `since=` diffs; serves are still RECORDED
        // below so those paths keep learning from expands.
        //
        // yevj secret gate: when raw recovery was NOT explicitly authorized
        // (raw absent/false), unambiguous credential shapes are masked in the
        // visible body; the stored bytes are never modified. `raw: true` is
        // the explicit authorization and returns exact bytes.
        let target = if params.raw {
            target
        } else {
            crate::perf_profile::_profile_expand_mask(|| {
                let (masked_text, masked_count) = mask_expansion_secrets(&target.content);
                if masked_count == 0 {
                    target
                } else {
                    let mut masked_target = target;
                    masked_target.content = masked_text;
                    summary.note_secret_masking(masked_count);
                    masked_target
                }
            })
        };

        if self.config.session_dedup {
            pending.push(self.pending_expand_record(key, &params, &target.content, &mut store));
        }
        let mut response = expansion_response(target, store.recovery_tokens);
        if response.error.is_none() {
            response.recovery = Some(tokenzero_core::RecoveryReceipt {
                terminal: true,
                do_not_recompact: true,
                exact_bytes: params.raw || summary.secret_masked_count() == 0,
            });
        }
        // Merge (not replace): windowed expands already carry window metadata
        // in telemetry, and masking notes must survive alongside it.
        if let Some(extra) = summary.telemetry() {
            let telemetry = response
                .telemetry
                .get_or_insert_with(|| serde_json::json!({}));
            if let (Some(map), Some(extra_map)) = (telemetry.as_object_mut(), extra.as_object()) {
                for (key, value) in extra_map {
                    map.insert(key.clone(), value.clone());
                }
            }
        }
        crate::perf_profile::_profile_expand_session_apply(|| {
            self.session_apply(pending, &summary);
        });
        response
    }

    fn rehydrate_working_set_expand(&self, store: &mut RecoveryStore, params: &ExpandParams) {
        let Ok(mut working_set) = self.working_set.lock() else {
            return;
        };
        let _ = working_set.handle_fault_hook(
            store,
            &params.ref_id,
            params.start_line,
            params.end_line,
        );
    }

    fn pending_expand_record(
        &self,
        key: ServeKey,
        params: &ExpandParams,
        content: &str,
        store: &mut RecoveryStore,
    ) -> (ServeKey, ServedRecord) {
        let stored = store.store_payload_deferred_batch(
            content,
            ContentType::Unknown,
            None,
            params.start_line,
            params.end_line,
        );
        let _ = store.persist_pending();
        (key, served_record(content, &stored))
    }

    pub fn expand(
        &self,
        ref_id: &str,
        selector: Option<&str>,
        start_line: Option<usize>,
        end_line: Option<usize>,
        anchor_kind: Option<&str>,
        symbol: Option<&str>,
    ) -> ToolResponse {
        self.expand_with_params(ExpandParams {
            ref_id: ref_id.to_string(),
            selector: selector.map(str::to_string),
            start_line,
            end_line,
            anchor_kind: anchor_kind.map(str::to_string),
            symbol: symbol.map(str::to_string),
            since: None,
            fresh: false,
            raw: false,
        })
    }

    /// Lossless full-text search over the persisted recovery cache.
    pub fn recall(
        &self,
        query: &str,
        max_hits: usize,
        mode: Mode,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        if query.trim().is_empty() {
            return failure_response(
                "recall",
                "invalid_query",
                "recall requires a non-empty query",
                None,
            );
        }
        let outcome = recall::recall_search(&self.config.cache_path, query, max_hits.max(1));
        let mut refs = Vec::new();
        let mut listed: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut lines = Vec::with_capacity(outcome.hits.len() + 1);
        if !outcome.hits.is_empty() {
            lines.push(format!(
                "# recall {} — {} hits across {} stored payloads{}",
                zero_hit_label(query),
                outcome.hits.len(),
                outcome.payloads_searched,
                if outcome.truncated {
                    " (hit limit reached)"
                } else {
                    ""
                }
            ));
        }
        for hit in &outcome.hits {
            lines.push(format!(
                "{} {}:{}: {}",
                hit.ref_id, hit.label, hit.line, hit.text
            ));
            if listed.insert(hit.ref_id.as_str()) {
                refs.push(ref_record("recall", hit.ref_id.clone(), 0));
            }
        }
        let assembled = lines.join("\n");
        let raw_tokens = count_tokens(&assembled);
        let capsule = make_capsule_with_raw_tokens(
            &assembled,
            raw_tokens,
            mode,
            max_visible_tokens,
            Some(&format!("recall {}", zero_hit_label(query))),
        );
        let mut response = capsule_response!("recall", mode, capsule, refs, 0);
        response.content_type = Some(ContentType::SearchResult.to_string());
        response.telemetry = Some(json!({
            "query": query,
            "hits": outcome.hits.len(),
            "payloads_searched": outcome.payloads_searched,
            "truncated_by_results": outcome.truncated,
            "transport_status": if outcome.unreadable { "degraded" } else { "ok" },
            "degraded": outcome.unreadable
        }));
        if outcome.unreadable {
            response.diagnostic = Some(tokenzero_core::Diagnostic {
                code: "recall_cache_unreadable".to_string(),
                message: "recovery cache exists but could not be read or parsed".to_string(),
                repair: Some(
                    "run tokenzero mem to inspect the cache, or pass --cache-path".to_string(),
                ),
            });
        }
        if outcome.hits.is_empty() {
            apply_zero_hit_note(
                &mut response,
                mode,
                format!("# recall {} — 0 matches", zero_hit_label(query)),
            );
        }
        response
    }

    /// Store `stored_text` as the canonical recoverable payload while
    /// rendering `rendered_text` (a lossless compact projection of it) as the
    /// visible capsule. Accounting keeps raw tokens from the stored payload.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ingest_with_tool(
        &self,
        tool: &str,
        stored_text: &str,
        rendered_text: &str,
        kind: ContentType,
        mode: Mode,
        source: &str,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        let mut response = self.ingest(stored_text, kind, mode, source);
        response.tool = tool.to_string();
        if let Some(accounting) = response.accounting.as_mut() {
            let capsule = make_capsule_with_raw_tokens(
                rendered_text,
                accounting.raw_tokens,
                mode,
                max_visible_tokens,
                Some(source),
            );
            accounting.visible_tokens = capsule.visible_tokens;
            if let Some(visible) = response.visible.as_mut() {
                visible.text = capsule.text;
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_expands_alias_persisted_after_engine_construction() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("recovery-cache.json");
        let mut config = EngineConfig::for_root(dir.path());
        config.cache_path = cache.clone();
        config.session_dedup = false;
        let engine = TokenZeroEngine::new(config);

        let text = "payload persisted by another store view";
        let alias = {
            let mut other_process_store = RecoveryStore::new(Some(cache));
            let stored = other_process_store
                .store_payload(text, ContentType::Unknown, None, None, None)
                .unwrap();
            other_process_store.ensure_session_visible_alias(&stored.blob_ref)
        };

        let response = engine.expand(&alias, Some("raw"), None, None, None, None);
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(response.visible.as_ref().unwrap().text, text);
    }

    fn engine_with_store(dir: &tempfile::TempDir) -> (TokenZeroEngine, RecoveryStore) {
        let cache = dir.path().join("recovery-cache.json");
        let mut config = EngineConfig::for_root(dir.path());
        config.cache_path = cache.clone();
        config.session_dedup = false;
        let store = RecoveryStore::new(Some(cache));
        (TokenZeroEngine::new(config), store)
    }

    fn store_blob(store: &mut RecoveryStore, text: &str) -> String {
        store
            .store_payload(text, ContentType::Unknown, None, None, None)
            .unwrap()
            .blob_ref
    }

    fn expand_raw(engine: &TokenZeroEngine, ref_id: &str, raw: bool) -> ToolResponse {
        engine.expand_with_params(ExpandParams {
            ref_id: ref_id.to_string(),
            selector: None,
            start_line: None,
            end_line: None,
            anchor_kind: None,
            symbol: None,
            since: None,
            fresh: false,
            raw,
        })
    }

    #[test]
    fn raw_expand_over_cap_fails_typed_with_fragment_repair_hint() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, mut store) = engine_with_store(&dir);
        let payload = "x".repeat(EXPAND_RAW_MAX_BYTES + 64);
        let blob = store_blob(&mut store, &payload);
        let response = expand_raw(&engine, &blob, true);
        let error = response.error.expect("over-cap raw expand must fail typed");
        assert_eq!(error.code, "expand_raw_cap_exceeded");
        assert!(
            error.repair.as_deref().unwrap_or_default().contains("#B"),
            "repair hint must point at byte fragments: {:?}",
            error.repair
        );
        // The cap gates explicit raw recovery only; a normal expand still
        // returns the body (masked-gate aside, plain payload here).
        let response = expand_raw(&engine, &blob, false);
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(response.visible.as_ref().unwrap().text, payload);
    }

    #[test]
    fn expand_raw_cap_env_parse_contract() {
        assert_eq!(expand_raw_max_bytes_from(None), EXPAND_RAW_MAX_BYTES);
        assert_eq!(expand_raw_max_bytes_from(Some("1024")), 1024);
        assert_eq!(expand_raw_max_bytes_from(Some("0")), EXPAND_RAW_MAX_BYTES);
        assert_eq!(
            expand_raw_max_bytes_from(Some("junk")),
            EXPAND_RAW_MAX_BYTES
        );
    }

    #[test]
    fn expand_masks_unambiguous_secret_unless_raw_authorized() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, mut store) = engine_with_store(&dir);
        let secret = format!("ghp_{}", "a1B2".repeat(9));
        let text = format!("deploy token: {secret} eof");
        let blob = store_blob(&mut store, &text);

        let masked = expand_raw(&engine, &blob, false);
        assert!(masked.error.is_none(), "{:?}", masked.error);
        let body = &masked.visible.as_ref().unwrap().text;
        assert!(body.contains("[tz-masked:github-pat]"), "{body}");
        assert!(!body.contains(&secret), "secret must not leak: {body}");
        let receipt = masked.recovery.as_ref().expect("terminal receipt");
        assert!(receipt.terminal && receipt.do_not_recompact);
        assert!(!receipt.exact_bytes, "masked body is not byte-exact");
        let telemetry = masked.telemetry.expect("masking telemetry");
        assert_eq!(telemetry["secret_masking"]["masked_spans"], 1);
        assert_eq!(telemetry["secret_masking"]["stored_bytes_modified"], false);

        // raw=true is the explicit authorization: exact bytes, and proves the
        // store itself was never modified by the masked expand.
        let exact = expand_raw(&engine, &blob, true);
        assert!(exact.error.is_none(), "{:?}", exact.error);
        assert_eq!(exact.visible.as_ref().unwrap().text, text);
        let receipt = exact.recovery.as_ref().expect("terminal receipt");
        assert!(receipt.terminal && receipt.do_not_recompact && receipt.exact_bytes);
    }

    #[test]
    fn expand_masks_pem_private_key_block() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, mut store) = engine_with_store(&dir);
        let text =
            "-----BEGIN PRIVATE KEY-----\nMIIEvgIBADANBg==\n-----END PRIVATE KEY-----\nafter";
        let blob = store_blob(&mut store, text);
        let masked = expand_raw(&engine, &blob, false);
        let body = &masked.visible.as_ref().unwrap().text;
        assert!(body.contains("[tz-masked:private-key-block]"), "{body}");
        assert!(!body.contains("MIIEvgIBADANBg=="), "{body}");
        assert!(body.contains("after"), "{body}");
    }

    #[test]
    fn masking_ignores_prose_and_short_lookalikes() {
        let (out, count) = mask_expansion_secrets("ask- politely, sk-short, ghp_abc, AKIA123 done");
        assert_eq!(count, 0, "{out}");
        assert_eq!(out, "ask- politely, sk-short, ghp_abc, AKIA123 done");
    }
}
