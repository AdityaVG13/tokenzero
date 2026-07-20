//! Private raw worker framed protocol (tokenzero-irx9.4).
//!
//! Trusted local composition path: invokes the typed domain dispatcher once
//! per frame. Does **not** open FastMCP catalogs, parse JavaScript, plan,
//! compact again, or rewrite envelopes. Not a third user-facing package —
//! internal mode of the selected artifact for hub/OMP composition.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::dispatcher::{DispatchOutcome, dispatch_raw_worker};
use crate::surface_handshake::{
    HandshakeSurface, PlannerOwner, CompressionOwner, SurfaceCapability,
    build_surface_capability, check_contract_compatibility, composition_trace,
    RAW_WORKER_PROTOCOL_VERSION,
};
use crate::TokenZeroEngine;

/// Framed request: one domain op, optional peer contract for fail-closed handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawWorkerRequest {
    /// Protocol marker — must be [`RAW_WORKER_PROTOCOL_VERSION`] or omitted (default).
    #[serde(default)]
    pub protocol: Option<String>,
    /// Canonical op name or alias (`tz_read`, `zero.read`, `read`, …).
    pub op: String,
    /// Domain args object.
    #[serde(default)]
    pub args: Value,
    /// Optional peer semantic contract digest (handshake).
    #[serde(default)]
    pub peer_contract_digest: Option<String>,
    /// Optional peer semantic contract version.
    #[serde(default)]
    pub peer_contract_version: Option<String>,
}

/// Framed response with composition ownership trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawWorkerResponse {
    pub ok: bool,
    pub protocol: String,
    pub op: String,
    pub surface: String,
    /// Normalized domain / tool outcome when dispatch succeeds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RawWorkerError>,
    /// Composition ownership + boundary accounting (AC: planner/compression owners).
    pub trace: Value,
    /// Catalog-free capability snapshot used for this call.
    pub capability: SurfaceCapability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawWorkerError {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
}

/// Execute one framed raw-worker request through the shared dispatcher.
///
/// Guarantees:
/// - Exactly one domain dispatch boundary (`boundary_count=1` on success path
///   after handshake; handshake failures have `boundary_count=0`).
/// - No CodeMode sandbox / JS runtime is created.
/// - Peer digest/version mismatches fail before domain execution.
pub fn execute_raw_worker_frame(
    engine: &TokenZeroEngine,
    request: &RawWorkerRequest,
) -> RawWorkerResponse {
    let capability = build_surface_capability(HandshakeSurface::RawWorker);
    let protocol = request
        .protocol
        .as_deref()
        .unwrap_or(RAW_WORKER_PROTOCOL_VERSION)
        .to_string();

    if protocol != RAW_WORKER_PROTOCOL_VERSION {
        return fail_response(
            &request.op,
            capability,
            "protocol_mismatch",
            format!(
                "raw worker protocol mismatch: local={RAW_WORKER_PROTOCOL_VERSION} peer={protocol}"
            ),
            0,
        );
    }

    if let Err(msg) = check_contract_compatibility(
        &capability,
        request.peer_contract_digest.as_deref(),
        request.peer_contract_version.as_deref(),
    ) {
        return fail_response(
            &request.op,
            capability,
            "contract_mismatch",
            msg,
            0,
        );
    }

    let args = if request.args.is_null() {
        json!({})
    } else {
        request.args.clone()
    };

    let outcome = dispatch_raw_worker(engine, &request.op, &args);
    if let Some(err) = &outcome.domain_error {
        return fail_response(
            &request.op,
            capability,
            err.kind.as_str(),
            err.message.clone(),
            1, // handshake passed; one domain boundary attempted
        );
    }
    success_response(&request.op, capability, outcome)
}

/// JSON convenience entry: parse request object, return response JSON.
pub fn execute_raw_worker_json(engine: &TokenZeroEngine, request: &Value) -> Value {
    let parsed: Result<RawWorkerRequest, _> = serde_json::from_value(request.clone());
    match parsed {
        Ok(req) => serde_json::to_value(execute_raw_worker_frame(engine, &req))
            .expect("RawWorkerResponse serializes"),
        Err(e) => {
            let capability = build_surface_capability(HandshakeSurface::RawWorker);
            serde_json::to_value(fail_response(
                "",
                capability,
                "invalid_frame",
                format!("invalid raw worker request: {e}"),
                0,
            ))
            .expect("serialize")
        }
    }
}

fn success_response(
    op: &str,
    capability: SurfaceCapability,
    outcome: DispatchOutcome,
) -> RawWorkerResponse {
    let mut result = outcome.result.value.clone();
    if let Value::Object(ref mut map) = result {
        map.insert("refs".into(), json!(outcome.result.refs));
        map.insert("op".into(), json!(outcome.op));
    }
    if let Some(resp) = &outcome.tool_response {
        if let Ok(v) = serde_json::to_value(resp) {
            if let Value::Object(ref mut map) = result {
                map.insert("tool_response".into(), v);
            }
        }
    }
    let tool_ok = outcome
        .tool_response
        .as_ref()
        .map(|r| r.status == "ok")
        .unwrap_or(outcome.domain_error.is_none());
    RawWorkerResponse {
        ok: tool_ok,
        protocol: RAW_WORKER_PROTOCOL_VERSION.into(),
        op: op.into(),
        surface: HandshakeSurface::RawWorker.as_str().into(),
        result: Some(result),
        error: None,
        trace: composition_trace(
            HandshakeSurface::RawWorker,
            PlannerOwner::Client,
            CompressionOwner::Engine,
            1,
        ),
        capability,
    }
}

fn fail_response(
    op: &str,
    capability: SurfaceCapability,
    kind: &str,
    message: String,
    boundary_count: u32,
) -> RawWorkerResponse {
    RawWorkerResponse {
        ok: false,
        protocol: RAW_WORKER_PROTOCOL_VERSION.into(),
        op: op.into(),
        surface: HandshakeSurface::RawWorker.as_str().into(),
        result: None,
        error: Some(RawWorkerError {
            kind: kind.into(),
            message,
            retryable: Some(false),
        }),
        trace: composition_trace(
            HandshakeSurface::RawWorker,
            PlannerOwner::Client,
            CompressionOwner::Engine,
            boundary_count,
        ),
        capability,
    }
}

#[cfg(test)]
mod tests {
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
        };
        let resp = execute_raw_worker_frame(&engine, &req);
        assert!(!resp.ok);
        assert_eq!(resp.error.as_ref().unwrap().kind, "contract_mismatch");
        assert_eq!(resp.trace["boundary_count"], 0);
        assert_eq!(resp.trace["planner_owner"], "client");
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
        };
        let resp = execute_raw_worker_frame(&engine, &req);
        assert!(resp.ok, "{:?}", resp.error);
        assert_eq!(resp.trace["boundary_count"], 1);
        assert_eq!(resp.surface, "raw_worker");
        assert_eq!(resp.protocol, RAW_WORKER_PROTOCOL_VERSION);
        assert!(resp.result.is_some());
        // Single planner owner (client) — no nested server_codemode claim.
        assert_eq!(resp.trace["planner_owner"], "client");
        assert_eq!(resp.trace["compression_owner"], "engine");
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
        };
        let resp = execute_raw_worker_frame(&engine, &req);
        assert!(!resp.ok);
        assert_eq!(resp.error.as_ref().unwrap().kind, "protocol_mismatch");
    }

    #[test]
    fn no_sandbox_modules_in_raw_worker_source() {
        // Static: production lines must not pull CodeMode/JS runtimes.
        // Skip the test body so the deny-list strings themselves do not trip.
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
}
