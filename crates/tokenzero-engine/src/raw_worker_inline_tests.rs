    use super::*;
    use crate::config::EngineConfig;
    use tempfile::tempdir;

    fn engine() -> (tempfile::TempDir, TokenZeroEngine) {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let cache = root.join("cache.json");
        let mut cfg = EngineConfig::for_root(&root);
        cfg.cache_path = cache;
        cfg.session_dedup = false;
        cfg.fetch_enabled = false;
        (dir, TokenZeroEngine::new(cfg))
    }

    #[test]
    fn handshake_mismatch_does_not_dispatch() {
        let (_dir, engine) = engine();
        let req = RawWorkerRequest {
            protocol: Some(RAW_WORKER_PROTOCOL_VERSION.into()),
            op: "tz_mem".into(),
            args: json!({}),
            peer_contract_digest: Some("00".repeat(32)),
            peer_contract_version: None,
            control: None,
        };
        let resp = execute_raw_worker_frame(&engine, &req);
        assert!(!resp.ok);
        assert!(resp.result.is_none());
        assert_eq!(resp.error.as_ref().unwrap().kind, "contract_mismatch");
        assert_eq!(resp.trace["boundary_count"], 0);
    }

    #[test]
    fn raw_worker_dispatches_once_with_trace() {
        let (_dir, engine) = engine();
        let local = build_surface_capability(HandshakeSurface::RawWorker);
        let req = RawWorkerRequest {
            protocol: Some(RAW_WORKER_PROTOCOL_VERSION.into()),
            op: "tz_mem".into(),
            args: json!({}),
            peer_contract_digest: Some(local.semantic_contract_digest.clone()),
            peer_contract_version: Some(local.semantic_contract_version.clone()),
            control: None,
        };
        let resp = execute_raw_worker_frame(&engine, &req);
        assert!(resp.ok, "{:?}", resp.error);
        assert_eq!(resp.trace["boundary_count"], 1);
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn protocol_mismatch_fails_closed() {
        let (_dir, engine) = engine();
        let req = RawWorkerRequest {
            protocol: Some("other.protocol.v9".into()),
            op: "tz_mem".into(),
            args: json!({}),
            peer_contract_digest: None,
            peer_contract_version: None,
            control: None,
        };
        let resp = execute_raw_worker_frame(&engine, &req);
        assert!(!resp.ok);
        assert!(resp.result.is_none());
        assert_eq!(resp.error.as_ref().unwrap().kind, "protocol_mismatch");
    }

    #[test]
    fn tool_response_errors_are_fail_envelope_with_retryable() {
        let (dir, engine) = engine();
        // Missing path → tool error, not domain_error only.
        let missing = dir.path().join("__no_such_file__.txt");
        let req = RawWorkerRequest {
            protocol: Some(RAW_WORKER_PROTOCOL_VERSION.into()),
            op: "tz_read".into(),
            args: json!({"path": missing.display().to_string()}),
            peer_contract_digest: None,
            peer_contract_version: None,
            control: None,
        };
        let resp = execute_raw_worker_frame(&engine, &req);
        assert!(!resp.ok, "missing path must be ok=false");
        assert!(
            resp.result.is_none(),
            "result must be null/absent on tool failure, got {:?}",
            resp.result
        );
        let err = resp.error.expect("typed error required");
        assert!(!err.kind.is_empty());
        assert!(!err.message.is_empty());
        // retryable is always populated (bool field).
        let _ = err.retryable;
    }

    #[test]
    fn policy_error_fail_envelope() {
        let (_dir, engine) = engine();
        let req = RawWorkerRequest {
            protocol: Some(RAW_WORKER_PROTOCOL_VERSION.into()),
            op: "tz_read".into(),
            args: json!({"path": "/etc/passwd"}),
            peer_contract_digest: None,
            peer_contract_version: None,
            control: None,
        };
        let resp = execute_raw_worker_frame(&engine, &req);
        assert!(!resp.ok);
        assert!(resp.result.is_none());
        let err = resp.error.expect("error");
        assert!(
            err.kind == "policy"
                || err.kind.contains("path")
                || err.message.to_ascii_lowercase().contains("path")
                || err.kind == "not_found"
                || err.kind == "runtime"
                || err.kind == "validation",
            "unexpected kind {}",
            err.kind
        );
    }

    #[test]
    fn control_handshake_returns_capability() {
        let (_dir, engine) = engine();
        let req = RawWorkerRequest {
            protocol: Some(RAW_WORKER_PROTOCOL_VERSION.into()),
            op: String::new(),
            args: json!({}),
            peer_contract_digest: None,
            peer_contract_version: None,
            control: Some("handshake".into()),
        };
        let resp = execute_raw_worker_frame(&engine, &req);
        assert!(resp.ok);
        let result = resp.result.expect("handshake result");
        assert_eq!(result["schema"], "zerostack.surface.v1");
        assert_eq!(result["surface"], "raw_worker");
    }

    #[test]
    fn parse_raw_worker_argv_handshake() {
        let args = vec![
            "tokenzero-mcp".into(),
            "raw-worker".into(),
            "--handshake".into(),
        ];
        let opts = parse_raw_worker_argv(&args).expect("parse");
        assert!(opts.handshake_only);
    }

    #[test]
    fn no_sandbox_modules_in_raw_worker_source() {
        let src = include_str!("raw_worker.rs");
        let production: String = src
            .lines()
            .take_while(|l| !l.contains("mod tests"))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in ["rquickjs", "execute_codemode", "fastmcp_mode"] {
            assert!(
                !production.contains(needle),
                "raw_worker production code must not reference {needle}"
            );
        }
    }
