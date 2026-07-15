use super::*;

impl TokenZeroEngine {
    pub fn ingest(&self, text: &str, kind: ContentType, mode: Mode, source: &str) -> ToolResponse {
        let mut store = self.recovery_store();
        let mut refs = Vec::new();
        let mut storage_error = None;
        match store.store_payload(text, kind, None, None, None) {
            Ok(stored) => {
                refs.push(ref_record("blob", stored.blob_ref, text.len()));
                refs.push(ref_record("file", stored.file_ref, text.len()));
            }
            Err(err) => {
                storage_error = Some(err.to_string());
            }
        }
        let refs_complete = prune_dead_refs(&store, &mut refs);
        let capsule = if refs_complete {
            make_capsule(text, mode, self.config.max_visible_tokens, Some(source))
        } else {
            let raw_tokens = count_tokens(text);
            tokenzero_core::Capsule {
                text: text.trim_end().to_string(),
                raw_tokens,
                visible_tokens: raw_tokens,
                omitted_lines: 0,
                mode,
            }
        };
        let mut response = capsule_response!("ingest", mode, capsule, refs, store.recovery_tokens);
        response.content_type = Some(kind.to_string());
        if let Some(error) = storage_error {
            response.diagnostic = Some(tokenzero_core::Diagnostic {
                code: "cache_write_failed".to_string(),
                message: "could not persist recovery cache for ingested content".to_string(),
                repair: Some("fix recovery cache permissions or pass --cache-path".to_string()),
            });
            response.telemetry = Some(json!({
                "transport_status": "degraded",
                "degraded": true,
                "storage_error": error,
                "exact_refs_available": false
            }));
        }
        if text.is_empty() {
            apply_zero_hit_note(&mut response, mode, "# ingest — 0 bytes".to_string());
        }
        response
    }
}
