use super::*;

impl TokenZeroEngine {
    /// Fetch a URL through the system curl with a TTL'd cache over the
    /// recovery store: a fresh-enough prior fetch serves the stored body
    /// without touching the network. Every serve carries exact refs; `fresh`
    /// bypasses the TTL.
    pub fn fetch(
        &self,
        url: &str,
        ttl_seconds: Option<usize>,
        fresh: bool,
        mode: Mode,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return failure_response(
                "fetch",
                "invalid_url",
                format!("fetch requires an http(s) URL, got {url}"),
                None,
            );
        }
        if !self.config.fetch_enabled {
            return ToolResponse::error(
                "fetch",
                "fetch_disabled",
                "network fetches are disabled by default",
                Some(format!(
                    "set {FETCH_ENABLED_ENV}=on (optionally {FETCH_ALLOW_ENV}=host1,host2) to enable"
                )),
            );
        }
        let ttl_secs = ttl_seconds.unwrap_or(24 * 60 * 60) as u64;
        let index_path = fetch_index_path(&self.config.cache_path);
        if let Err(blocked) = validate_fetch_target(
            url,
            &self.config.fetch_allow_hosts,
            &self.config.fetch_deny_hosts,
        ) {
            return ToolResponse::error("fetch", blocked.code, blocked.message, blocked.repair);
        }
        if !fresh {
            if let Some(entry) = load_fetch_index(&index_path).entries.get(url) {
                let age = epoch_secs().saturating_sub(entry.fetched_at_secs);
                if age <= ttl_secs {
                    let mut store = self.recovery_store();
                    let cached = store.expand(&entry.blob_ref, Some("raw"), None, None, None, None);
                    if cached.found {
                        let recovery_tokens = store.recovery_tokens;
                        return self.fetch_response(
                            url,
                            &cached.content,
                            mode,
                            max_visible_tokens,
                            true,
                            age,
                            recovery_tokens,
                            &index_path,
                        );
                    }
                }
            }
        }
        let curl = match self.config.curl_path_override.clone() {
            Some(path) => path,
            None => match resolve_curl_binary() {
                Ok(resolved) => resolved.path,
                Err(err) => {
                    return failure_response(
                        "fetch",
                        "fetch_failed",
                        err.message,
                        Some("install curl or set TOKENZERO_CURL_PATH"),
                    );
                }
            },
        };
        // Redirects are followed manually so every hop's target is validated
        // (and pinned) like the entry URL — a redirect to an internal address
        // is the classic SSRF bypass.
        const MAX_FETCH_REDIRECTS: usize = 5;
        let mut current_url = url.to_string();
        let mut redirect_hops = 0usize;
        let body = loop {
            let target = match validate_fetch_target(
                &current_url,
                &self.config.fetch_allow_hosts,
                &self.config.fetch_deny_hosts,
            ) {
                Ok(target) => target,
                Err(blocked) => {
                    return ToolResponse::error(
                        "fetch",
                        blocked.code,
                        blocked.message,
                        blocked.repair,
                    );
                }
            };
            let mut argv: Vec<String> = vec![
                curl.display().to_string(),
                "-sS".to_string(),
                "--max-time".to_string(),
                "30".to_string(),
                "--proto".to_string(),
                "=http,https".to_string(),
                "-w".to_string(),
                format!("\n{FETCH_META_MARKER} %{{http_code}} %{{redirect_url}}"),
            ];
            if let Some(ip) = target.pinned_ip {
                argv.push("--resolve".to_string());
                argv.push(format!("{}:{}:{}", target.host, target.port, ip));
            }
            argv.push(current_url.clone());
            let child_env = inner_env();
            let output_policy = self.shell_output_policy();
            let result = match run_command_with_policy(
                &argv,
                None,
                Some(&child_env),
                None,
                Duration::from_secs(45),
                false,
                output_policy,
            ) {
                Ok(result) => result,
                Err(err) => {
                    return failure_response(
                        "fetch",
                        "fetch_failed",
                        format!("could not run curl: {err}"),
                        Some("install curl or set TOKENZERO_CURL_PATH"),
                    );
                }
            };
            if !result.ok || result.exit_code != Some(0) {
                let stderr: String = result.stderr.trim().chars().take(300).collect();
                return failure_response(
                    "fetch",
                    "fetch_failed",
                    format!("curl exited with {:?}: {stderr}", result.exit_code),
                    Some("check the URL and network access"),
                );
            }
            let (body, http_code, redirect_url) = split_fetch_meta(&result.stdout);
            match (http_code, redirect_url) {
                (Some(code), Some(next)) if (300..400).contains(&code) => {
                    redirect_hops += 1;
                    if redirect_hops > MAX_FETCH_REDIRECTS {
                        return failure_response(
                            "fetch",
                            "too_many_redirects",
                            format!("more than {MAX_FETCH_REDIRECTS} redirects from {url}"),
                            None,
                        );
                    }
                    current_url = next;
                }
                _ => break body,
            }
        };
        self.fetch_response(
            url,
            &body,
            mode,
            max_visible_tokens,
            false,
            0,
            0,
            &index_path,
        )
    }

    /// Shared fetch render: store the body (content-addressed refs are
    /// identical for cache hits, keeping every serve recoverable), update the
    /// TTL index on fresh fetches, capsule within budget.
    #[allow(clippy::too_many_arguments)]
    fn fetch_response(
        &self,
        url: &str,
        body: &str,
        mode: Mode,
        max_visible_tokens: usize,
        cache_hit: bool,
        age_seconds: u64,
        recovery_tokens: usize,
        index_path: &Path,
    ) -> ToolResponse {
        let mut store = self.recovery_store();
        let ctype = detect_content_type(body, None);
        let stored = store.store_payload_deferred(body, ctype, None, None, None);
        let mut refs = Vec::with_capacity(2);
        push_payload_refs(&mut refs, &stored, body.len());
        let persisted = persist_refs(&mut store, &mut refs);
        let refs_complete = persisted.refs_complete;
        let storage_error = persisted.error;
        // An evicted blob must not enter the fetch index: a later cache hit
        // would advertise a ref that cannot be expanded.
        if !cache_hit && storage_error.is_none() && refs_complete {
            record_fetch(index_path, url, &stored.blob_ref, body.len());
        }
        let capsule = recoverable_capsule(
            body,
            body,
            stored.raw_tokens,
            mode,
            max_visible_tokens,
            &format!("fetch {}", zero_hit_label(url)),
            refs_complete,
        );
        let mut response = capsule_response!(
            "fetch",
            mode,
            capsule,
            refs,
            recovery_tokens + store.recovery_tokens
        );
        response.content_type = Some(ctype.to_string());
        response.telemetry = Some(json!({
            "url": url,
            "cache_hit": cache_hit,
            "age_seconds": age_seconds,
            "bytes": body.len(),
            "transport_status": if storage_error.is_some() { "degraded" } else { "ok" },
            "degraded": storage_error.is_some(),
            "storage_error": storage_error.clone(),
        }));
        if storage_error.is_some() {
            response.diagnostic = Some(cache_write_diagnostic(
                "could not persist recovery cache for fetch output",
            ));
        }
        if body.is_empty() {
            apply_zero_hit_note(
                &mut response,
                mode,
                format!("# fetch {} — 0 bytes", zero_hit_label(url)),
            );
        }
        response
    }
}
