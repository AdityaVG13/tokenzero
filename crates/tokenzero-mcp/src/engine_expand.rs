use super::expand_params::ExpandParams;
use super::*;
use tokenzero_recovery::is_expandable_ref;

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

fn resolve_slice(
    store: &mut RecoveryStore,
    params: &ExpandParams,
    active_cache: &Path,
) -> Result<ExpansionResult, Box<ToolResponse>> {
    let selector = params.selector.as_deref().or(Some("raw"));
    let anchor = params.anchor_kind.as_deref();
    let symbol = params.symbol.as_deref();
    let result = store.expand(
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
        let response = self.expand_with_params_inner(params);
        let ok = response.error.is_none();
        let code = response.error.as_ref().map(|err| err.code.as_str());
        // Health probe for crash-only unlock (wqw.9). invalid_ref is a client
        // mistake and does not open recovery.
        self.surface_health().record_expand_outcome(ok, code);
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
            let since_result = store.expand(
                since_ref,
                params.selector.as_deref().or(Some("raw")),
                params.start_line,
                params.end_line,
                params.anchor_kind.as_deref(),
                params.symbol.as_deref(),
            );
            if !since_result.found {
                let code = match since_result.reason.as_str() {
                    "stale-ref" => "ref_stale",
                    "dangling-ref" => "ref_not_found",
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
            let target = match resolve_slice(&mut store, &params, &self.config.cache_path) {
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
            response.telemetry = summary.telemetry();
            self.session_apply(pending, &summary);
            return response;
        }

        let target = match resolve_slice(&mut store, &params, &self.config.cache_path) {
            Ok(t) => t,
            Err(resp) => return *resp,
        };
        self.rehydrate_working_set_expand(&mut store, &params);

        // Explicit expand is the recovery contract: it ALWAYS returns exact
        // bytes. Replacing content with an "identical to … (unchanged)" ack
        // here broke byte-exact recovery (release-claim audits) and forced a
        // fresh re-call exactly when the model had decided it needed the
        // bytes — the capability-loss the compression doctrine forbids.
        // Seen-set economics stay on the implicit serve paths (read/find
        // spills) and on explicit `since=` diffs; serves are still RECORDED
        // below so those paths keep learning from expands.

        if self.config.session_dedup {
            pending.push(self.pending_expand_record(key, &params, &target.content, &mut store));
        }
        let response = expansion_response(target, store.recovery_tokens);
        self.session_apply(pending, &summary);
        response
    }

    fn rehydrate_working_set_expand(&self, store: &mut RecoveryStore, params: &ExpandParams) {
        let Ok(mut working_set) = self.working_set.lock() else {
            return;
        };
        let _ =
            working_set.rehydrate_ref(store, &params.ref_id, params.start_line, params.end_line);
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
