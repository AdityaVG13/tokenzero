use super::*;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokenzero_core::{Accounting, TokenizerFamily, active_tokenizer_metadata};

#[derive(Debug, Default)]
pub struct RawWorkerV2Session {
    binding: Option<Binding>,
    shutdown: bool,
    expected_root: Option<String>,
    expected_session_id: Option<String>,
    cancel_registry: std::collections::HashMap<String, Arc<CancelState>>,
}

#[derive(Debug)]
struct Binding {
    root: String,
    session_id: String,
    revision: String,
    contract: String,
}

impl RawWorkerV2Session {
    pub fn for_binding(root: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            binding: None,
            shutdown: false,
            expected_root: Some(root.into()),
            expected_session_id: Some(session_id.into()),
            cancel_registry: std::collections::HashMap::new(),
        }
    }
}

/// Cancellation state for one in-flight v2 call. The cancel control frame
/// sets `flag` and kills the recorded child; the worker thread observes the
/// flag after dispatch returns.
#[derive(Debug, Default)]
struct CancelState {
    flag: Arc<AtomicBool>,
    process: Mutex<ChildProcess>,
}

#[derive(Debug, Default, Clone, Copy)]
struct ChildProcess {
    pid: Option<u32>,
    pgid: Option<u32>,
}

/// The cancel state of the call currently executing on the serve worker
/// thread (at most one: `max_in_flight` is 1).
static ACTIVE_CANCEL: Mutex<Option<Arc<CancelState>>> = Mutex::new(None);

#[cfg(unix)]
fn kill_process_tree(pid: Option<u32>, pgid: Option<u32>) {
    // `--` before the target is load-bearing: without it a negative process
    // group id is misparsed as a signal/option and the kill silently no-ops.
    if let Some(group) = pgid {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", "--", &format!("-{group}")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    } else if let Some(pid) = pid {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", "--", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[cfg(not(unix))]
fn kill_process_tree(_pid: Option<u32>, _pgid: Option<u32>) {}

/// shell_hooks entry: record the dispatched child under the active cancel
/// state; when cancellation already landed, kill immediately (spawn/cancel
/// race is decided in favor of the cancel).
fn v2_note_child(pid: Option<u32>, pgid: Option<u32>, _state: &'static str) {
    let Some(cancel) = ACTIVE_CANCEL
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().cloned())
    else {
        return;
    };
    {
        let mut process = cancel.process.lock().unwrap_or_else(|p| p.into_inner());
        if pid.is_some() {
            process.pid = pid;
        }
        if pgid.is_some() {
            process.pgid = pgid;
        }
    }
    if cancel.flag.load(Ordering::SeqCst) {
        let process = *cancel.process.lock().unwrap_or_else(|p| p.into_inner());
        kill_process_tree(process.pid, process.pgid);
    }
}

fn revision() -> String {
    std::env::var("ZEROSTACK_WORKER_REVISION")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

fn error(id: Option<&str>, kind: &str, message: impl Into<String>, trace: Option<Value>) -> Value {
    error_with(id, kind, message, trace, false)
}

fn error_with(
    id: Option<&str>,
    kind: &str,
    message: impl Into<String>,
    trace: Option<Value>,
    retryable: bool,
) -> Value {
    let mut value = json!({"kind":"error","error":{"kind":kind,"message":message.into(),"retryable":retryable}});
    if let Some(id) = id {
        value["request_id"] = json!(id);
    }
    if let Some(trace) = trace {
        value["trace"] = trace;
    }
    value
}

fn encode(mut value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap_or_default();
    if bytes.len() + 1 > raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES {
        let request_id = value["request_id"].as_str().map(str::to_string);
        value = error(
            request_id.as_deref(),
            "frame_too_large",
            "outbound frame exceeds 1 MiB",
            None,
        );
        bytes = serde_json::to_vec(&value).expect("fixed error serializes");
    }
    bytes.push(b'\n');
    bytes
}

fn local_capability() -> Value {
    serde_json::to_value(crate::surface_handshake::build_surface_capability(
        crate::surface_handshake::HandshakeSurface::RawWorker,
    ))
    .expect("capability serializes")
}

fn refs(value: &Value, output: &mut Vec<Value>) {
    match value {
        Value::String(v) if v.starts_with("tz://") => output.push(json!(v)),
        Value::Array(v) => v.iter().for_each(|v| refs(v, output)),
        Value::Object(v) => v.values().for_each(|v| refs(v, output)),
        _ => {}
    }
}

fn forbidden(op: &str) -> bool {
    let op = op.to_ascii_lowercase();
    matches!(
        op.as_str(),
        "plan"
            | "planner"
            | "js"
            | "javascript"
            | "mcp"
            | "execute_code"
            | "tz_execute_code"
            | "codemode_search"
            | "tz_codemode_search"
            | "codemode_describe"
            | "tz_codemode_describe"
            | "tools/call"
            | "tools/list"
    ) || op.starts_with("planner.")
        || op.starts_with("javascript.")
        || op.starts_with("mcp.")
}

impl RawWorkerV2Session {
    fn register_cancel(&mut self, id: &str) -> Arc<CancelState> {
        let cancel = Arc::new(CancelState::default());
        self.cancel_registry.insert(id.to_string(), cancel.clone());
        cancel
    }

    fn finish_call(&mut self, id: &str) {
        self.cancel_registry.remove(id);
    }

    /// Cancel an in-flight call: set the flag, then kill the recorded child
    /// process (group) so shell and search work stop inside the declared
    /// bound. Returns false for unknown or already-finished request ids.
    fn cancel_call(&mut self, id: &str) -> bool {
        match self.cancel_registry.remove(id) {
            Some(cancel) => {
                cancel.flag.store(true, Ordering::SeqCst);
                let process = *cancel.process.lock().unwrap_or_else(|p| p.into_inner());
                kill_process_tree(process.pid, process.pgid);
                true
            }
            None => false,
        }
    }

    /// Cancel every active or queued call before the session dispatch thread
    /// is joined. A child spawned after this point observes the flag in
    /// `v2_note_child` and is killed immediately.
    fn cancel_all(&mut self) {
        for (_, cancel) in self.cancel_registry.drain() {
            cancel.flag.store(true, Ordering::SeqCst);
            let process = *cancel.process.lock().unwrap_or_else(|p| p.into_inner());
            kill_process_tree(process.pid, process.pgid);
        }
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

const DEFAULT_DEADLINE_MS: u64 = 30_000;

fn is_shell_op(op: &str) -> bool {
    matches!(op, "shell" | "tz_shell" | "zero.shell")
}

fn effect_class(op: &str) -> &'static str {
    match op {
        "shell" | "tz_shell" | "zero.shell" | "compact" | "tz_compact" | "zero.compact"
        | "ingest" | "tz_ingest" | "zero.ingest" => "irreversible",
        _ => "read_only",
    }
}

fn worker_tokenizer_id() -> &'static str {
    match active_tokenizer_metadata().map(|metadata| metadata.family) {
        Some(TokenizerFamily::Cl100k) => "estimator:tokenzero-cl100k-average-v1",
        Some(TokenizerFamily::O200k) => "estimator:tokenzero-o200k-average-v1",
        Some(TokenizerFamily::SentencePiece) => "estimator:tokenzero-sentencepiece-average-v1",
        None => "estimator:tokenzero-lexical-v1",
    }
}

fn checked_u64_count(field: &str, value: usize) -> Result<u64, String> {
    u64::try_from(value).map_err(|_| format!("{field} exceeds the raw-worker accounting range"))
}

fn worker_token_accounting(
    value: &Value,
) -> Result<raw_worker_v2_protocol::WorkerTokenAccountingV1, String> {
    let accounting_value = value
        .get("accounting")
        .ok_or_else(|| "successful domain result omitted accounting".to_string())?;
    let accounting: Accounting = serde_json::from_value(accounting_value.clone())
        .map_err(|error| format!("invalid domain accounting: {error}"))?;
    let worker = raw_worker_v2_protocol::WorkerTokenAccountingV1 {
        tokenizer_id: worker_tokenizer_id().to_string(),
        count_kind: raw_worker_v2_protocol::WorkerTokenCountKind::Estimate,
        raw_tokens: checked_u64_count("raw_tokens", accounting.raw_tokens)?,
        visible_tokens: checked_u64_count("visible_tokens", accounting.visible_tokens)?,
        recovery_tokens: checked_u64_count("recovery_tokens", accounting.recovery_tokens)?,
        billed_tokens: checked_u64_count("billed_tokens", accounting.billed_tokens)?,
        cached_tokens: checked_u64_count("cached_tokens", accounting.cached_tokens)?,
        exact_ref_tokens: accounting
            .exact_ref_tokens
            .map(|tokens| checked_u64_count("exact_ref_tokens", tokens))
            .transpose()?,
    };
    zero_abi::validate_worker_token_accounting_v1(&worker)
        .map_err(|error| format!("invalid worker token accounting: {error}"))?;
    Ok(worker)
}

fn attach_engine_timeline(
    mut frame: Value,
    requested: bool,
    elapsed: std::time::Duration,
) -> Value {
    if requested {
        let duration_ns = elapsed.as_nanos().min(u128::from(u64::MAX)) as u64;
        let duration_ns = duration_ns.max(1);
        let timeline = raw_worker_v2_protocol::EngineStageTimelineV1 {
            total_ns: duration_ns,
            spans: vec![raw_worker_v2_protocol::EngineStageSpanV1 {
                stage: "tokenzero.raw_worker_call".to_string(),
                start_ns: 0,
                duration_ns,
            }],
        };
        frame["engine_timeline"] =
            serde_json::to_value(timeline).expect("engine timeline serializes");
    }
    frame
}

/// A validated call ready for dispatch, carrying cloned binding fields so
/// the session lock is never held while work runs.
#[derive(Debug)]
struct CallCtx {
    id: String,
    op: String,
    args: Value,
    trace: Value,
    deadline_unix_ms: Option<u64>,
    engine_stage_timeline_requested: bool,
    worker_token_accounting_requested: bool,
    session_id: String,
    contract: String,
}

enum RoutedFrame {
    Respond(Vec<u8>),
    Dispatch(CallCtx),
}

pub fn execute_raw_worker_v2_frame(
    engine: &TokenZeroEngine,
    session: &mut RawWorkerV2Session,
    line: &[u8],
) -> Vec<u8> {
    match route_frame(session, line) {
        RoutedFrame::Respond(bytes) => bytes,
        RoutedFrame::Dispatch(ctx) => {
            let id = ctx.id.clone();
            let cancel = session.register_cancel(&ctx.id);
            let value = run_call_registered(engine, ctx, cancel);
            session.finish_call(&id);
            encode(value)
        }
    }
}

/// Frame routing without dispatch: control frames (handshake/shutdown/cancel)
/// and validation failures produce an encoded response; valid calls come back
/// for dispatch so the serve loop can run them off the read loop.
fn route_frame(session: &mut RawWorkerV2Session, line: &[u8]) -> RoutedFrame {
    if let Err(e) = raw_worker_v2_protocol::decode_request_frame(
        line,
        raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES,
    ) {
        return RoutedFrame::Respond(encode(error(None, e.kind(), e.to_string(), None)));
    }
    let frame: Value = match serde_json::from_slice(line) {
        Ok(v) => v,
        Err(e) => {
            return RoutedFrame::Respond(encode(error(None, "invalid_json", e.to_string(), None)));
        }
    };
    let kind = frame["kind"].as_str().unwrap_or_default();
    let request = &frame["request"];

    if kind == "handshake" {
        let cap = local_capability();
        let rev = revision();
        let contract = cap["semantic_contract_digest"].as_str().unwrap_or_default();
        let registry = cap["operation_registry_digest"]
            .as_str()
            .unwrap_or_default();
        let root = request["root"].as_str().unwrap_or_default();
        let session_id = request["session_id"].as_str().unwrap_or_default();
        if let Some(existing) = session.binding.as_ref() {
            // A revision swap on the host side is survivable: the same
            // root+session may re-handshake to rebind. A different root or
            // session on an established binding stays terminal.
            if existing.root != root || existing.session_id != session_id {
                return RoutedFrame::Respond(encode(error(
                    None,
                    "already_bound",
                    "session is already bound",
                    None,
                )));
            }
        }
        let revision_mismatch = request
            .get("expected_worker_revision")
            .and_then(Value::as_str)
            .is_some_and(|v| v != rev);
        let mismatch = request["protocol_version"].as_str()
            != Some(raw_worker_v2_protocol::RAW_WORKER_PROTOCOL_VERSION)
            || root.is_empty()
            || session_id.is_empty()
            || session
                .expected_root
                .as_deref()
                .is_some_and(|expected| expected != root)
            || session
                .expected_session_id
                .as_deref()
                .is_some_and(|expected| expected != session_id)
            || request["expected_engine"].as_str() != Some("tokenzero")
            || request["expected_contract_digest"].as_str() != Some(contract)
            || request
                .get("expected_registry_digest")
                .and_then(Value::as_str)
                .is_some_and(|v| v != registry);
        if mismatch {
            return RoutedFrame::Respond(encode(error(
                None,
                "binding_mismatch",
                "worker handshake binding mismatch",
                None,
            )));
        }
        if revision_mismatch {
            // Stale revision pin: retryable so the host re-handshakes against
            // the current revision instead of terminally aborting the plan.
            return RoutedFrame::Respond(encode(error_with(
                None,
                "worker_revision_changed",
                "worker revision changed; re-handshake without the stale expected_worker_revision pin",
                None,
                true,
            )));
        }
        session.binding = Some(Binding {
            root: root.into(),
            session_id: session_id.into(),
            revision: rev.clone(),
            contract: contract.into(),
        });
        return RoutedFrame::Respond(encode(json!({"kind":"handshake_ack","ack":{
            "protocol_version":raw_worker_v2_protocol::RAW_WORKER_PROTOCOL_VERSION,
            "binding":{"engine":"tokenzero","root":root,"session_id":session_id,"worker_revision":rev,
                "semantic_contract_version":cap["semantic_contract_version"],"semantic_contract_digest":contract,
                "operation_registry_digest":registry,"ref_scheme":"tz://"},
            "capabilities":{"cancellation":true,"deadlines":true,"approvals":false,"revert":false,"snapshots":false},
            "limits":{"max_frame_bytes":1048576,"max_output_bytes":65536,"max_in_flight":1,"default_deadline_ms":DEFAULT_DEADLINE_MS},
            "protocol_digest":raw_worker_v2_protocol::raw_worker_protocol_digest_hex()
        }})));
    }
    if kind == "shutdown" {
        session.shutdown = true;
        return RoutedFrame::Respond(encode(json!({"kind":"shutdown_ack"})));
    }
    if session.binding.is_none() {
        return RoutedFrame::Respond(encode(error(
            None,
            "handshake_required",
            "v2 handshake required before calls",
            None,
        )));
    }
    if session.shutdown {
        return RoutedFrame::Respond(encode(error(
            None,
            "session_shutdown",
            "session has shut down",
            None,
        )));
    }
    if kind == "cancel" {
        let cancelled = session.cancel_call(request["request_id"].as_str().unwrap_or_default());
        return RoutedFrame::Respond(encode(
            json!({"kind":"cancel_ack","request_id":request["request_id"],"cancelled":cancelled,"process_kill_supported":cfg!(unix)}),
        ));
    }
    let binding = session.binding.as_ref().expect("binding checked above");
    let validation_started = Instant::now();
    match validate_call(binding, &frame) {
        Ok(ctx) => RoutedFrame::Dispatch(ctx),
        Err(value) => {
            let timeline_requested = request
                .get("telemetry_request")
                .and_then(|request| request.get("engine_stage_timeline"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            RoutedFrame::Respond(encode(attach_engine_timeline(
                value,
                timeline_requested,
                validation_started.elapsed(),
            )))
        }
    }
}

fn validate_call(binding: &Binding, frame: &Value) -> Result<CallCtx, Value> {
    let request = &frame["request"];
    let id = request["request_id"].as_str().unwrap_or_default();
    let op = request["op"].as_str().unwrap_or_default();
    let trace = request["trace"].clone();
    if trace["request_id"].as_str() != Some(id)
        || trace["contract_digest"].as_str() != Some(binding.contract.as_str())
    {
        return Err(error(
            Some(id),
            "trace_binding_mismatch",
            "trace does not match handshake binding",
            Some(trace),
        ));
    }
    if trace["worker_revision"].as_str() != Some(binding.revision.as_str()) {
        // Revision drift between handshake and call: typed retryable so the
        // host re-handshakes and retries instead of killing the plan.
        return Err(error_with(
            Some(id),
            "worker_revision_changed",
            "worker revision changed since handshake; re-handshake and retry the call",
            Some(trace),
            true,
        ));
    }
    let deadline_unix_ms = request.get("deadline_unix_ms").and_then(Value::as_u64);
    let telemetry_request = request.get("telemetry_request");
    let engine_stage_timeline_requested = telemetry_request
        .and_then(|request| request.get("engine_stage_timeline"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let worker_token_accounting_requested = telemetry_request
        .and_then(|request| request.get("worker_token_accounting"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if deadline_unix_ms.is_some_and(|v| v <= unix_ms()) {
        return Err(error(
            Some(id),
            "deadline_exceeded",
            "deadline expired before dispatch",
            Some(trace),
        ));
    }
    if forbidden(op) {
        return Err(error(
            Some(id),
            "unsupported_operation",
            "planner, JavaScript, and MCP operations are forbidden",
            Some(trace),
        ));
    }
    Ok(CallCtx {
        id: id.to_string(),
        op: op.to_string(),
        args: request["args"].clone(),
        trace,
        deadline_unix_ms,
        engine_stage_timeline_requested,
        worker_token_accounting_requested,
        session_id: binding.session_id.clone(),
        contract: binding.contract.clone(),
    })
}

/// Run a validated call under the active cancel registration and the wall
/// deadline derived from `deadline_unix_ms` (default 30 s, matching the
/// advertised handshake limit).
fn run_call_registered(engine: &TokenZeroEngine, ctx: CallCtx, cancel: Arc<CancelState>) -> Value {
    let started = Instant::now();
    let value = if cancel.flag.load(Ordering::SeqCst) {
        json!({"kind":"error","request_id":ctx.id.clone(),"error":{
            "kind":"cancelled","message":"call cancelled before dispatch","retryable":false
        },"trace":ctx.trace.clone()})
    } else {
        *ACTIVE_CANCEL.lock().unwrap_or_else(|p| p.into_inner()) = Some(cancel.clone());
        let value = dispatch_call(engine, &ctx, &cancel);
        *ACTIVE_CANCEL.lock().unwrap_or_else(|p| p.into_inner()) = None;
        value
    };
    attach_engine_timeline(
        value,
        ctx.engine_stage_timeline_requested,
        started.elapsed(),
    )
}

fn verified_cancelled_shell_partial_result(ctx: &CallCtx, response: &Value) -> Option<Value> {
    if !is_shell_op(&ctx.op) || response["ok"].as_bool() != Some(true) {
        return None;
    }
    let result = response.get("result")?;
    let tool_response = result.get("tool_response")?;
    let refs = tool_response.get("refs")?.as_array()?;
    let verified = !refs.is_empty()
        && tool_response["status"].as_str() == Some("ok")
        && tool_response["safety"]["refs_cover_full_output"].as_bool() == Some(true)
        && tool_response["telemetry"]["refs_cover_full_output"].as_bool() == Some(true);
    verified.then(|| result.clone())
}

/// Dispatch a validated call. Cancellation observed after dispatch maps to a
/// typed `cancelled` error; the remaining deadline is pushed into shell work
/// as a process timeout and into search/expand loops as wall checkpoints.
fn dispatch_call(engine: &TokenZeroEngine, ctx: &CallCtx, cancel: &Arc<CancelState>) -> Value {
    let remaining = ctx
        .deadline_unix_ms
        .map(|deadline| deadline.saturating_sub(unix_ms()))
        .unwrap_or(DEFAULT_DEADLINE_MS)
        .max(1);
    let wall = crate::wall::WallDeadline::new(Instant::now(), remaining);
    let response = crate::wall::with_host_wall_deadline_and_cancel(
        wall,
        Arc::clone(&cancel.flag),
        || {
            let mut args = ctx.args.clone();
            if is_shell_op(&ctx.op) {
                if let Value::Object(ref mut map) = args {
                    let requested = ["timeout_ms", "timeoutMs", "shell_timeout_ms"]
                        .iter()
                        .find_map(|key| map.get(*key).and_then(Value::as_u64));
                    map.insert(
                        "timeout_ms".to_string(),
                        json!(requested.map_or(remaining, |r| r.min(remaining))),
                    );
                }
            }
            match crate::domain::execute_raw_worker_value(engine, &ctx.op, &args) {
                Some(Ok(value)) => json!({"ok":true,"result":value}),
                Some(Err(error)) => json!({"ok":false,"error":{
                    "kind":error.kind,"message":error.message,"retryable":false
                }}),
                None => {
                    let v1 = json!({"id":ctx.id.clone(),"op":ctx.op.clone(),"args":args,"peer_contract_digest":ctx.contract.clone()});
                    execute_raw_worker_json(engine, &v1)
                }
            }
        },
    );
    if cancel.flag.load(Ordering::SeqCst) {
        let mut cancelled = json!({"kind":"error","request_id":ctx.id.clone(),"error":{
            "kind":"cancelled","message":"call cancelled by control frame","retryable":false
        },"trace":ctx.trace.clone()});
        if let Some(partial_result) = verified_cancelled_shell_partial_result(ctx, &response) {
            cancelled["error"]["details"] = json!({
                "partial_result": partial_result,
                "artifact_scope": "full_observed_stdout_stderr_streams",
                "temporal_interleaving_claimed": false
            });
        }
        return cancelled;
    }
    if response["ok"].as_bool() != Some(true) {
        let e = &response["error"];
        return json!({"kind":"error","request_id":ctx.id.clone(),"error":{
            "kind":e["kind"].as_str().unwrap_or("operation_failed"),
            "message":e["message"].as_str().unwrap_or("operation failed"),
            "retryable":e["retryable"].as_bool().unwrap_or(false),"details":e.get("details").cloned().unwrap_or(Value::Null)
        },"trace":ctx.trace.clone()});
    }
    let value = response.get("result").cloned().unwrap_or(Value::Null);
    let worker_token_accounting = if ctx.worker_token_accounting_requested {
        match worker_token_accounting(&value) {
            Ok(accounting) => Some(accounting),
            Err(message) => {
                return json!({"kind":"error","request_id":ctx.id.clone(),"error":{
                    "kind":"invalid_token_accounting","message":message,"retryable":false
                },"trace":ctx.trace.clone()});
            }
        }
    } else {
        None
    };
    let mut owned_refs = Vec::new();
    // Job tails are arbitrary shell bytes. A line beginning with `tz://` is
    // content, not a minted ref, so job results never contribute ownership.
    if ctx.op != zero_abi::TOKEN_JOB_OPERATION_V1 {
        refs(&value, &mut owned_refs);
    }
    let mut frame = json!({"kind":"result","request_id":ctx.id.clone(),"result":{"value":value,"metadata":{
        "effect":effect_class(ctx.op.as_str()),
        "approval":{"state":"not_required"},"revert":{"supported":false},
        "ownership":{"engine":"tokenzero","session_id":ctx.session_id.clone(),"refs":owned_refs},"trace":ctx.trace.clone()
    }}});
    if let Some(accounting) = worker_token_accounting {
        frame["worker_token_accounting"] =
            serde_json::to_value(accounting).expect("worker token accounting serializes");
    }
    frame
}

struct CallJob {
    ctx: CallCtx,
    cancel: Arc<CancelState>,
}

fn write_response(writer: &Mutex<std::io::Stdout>, response: &[u8]) -> std::io::Result<()> {
    let mut out = writer
        .lock()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "writer poisoned"))?;
    out.write_all(response)?;
    out.flush()
}

fn terminate_raw_worker_v2_session(session: &Mutex<RawWorkerV2Session>) {
    {
        let mut guard = session.lock().unwrap_or_else(|poison| poison.into_inner());
        guard.cancel_all();
    }
    // The job registry is process-global and therefore has no reliable static
    // destructor. Mark every live job for termination before serve can exit;
    // a child published after this scan observes the mark and is killed too.
    crate::engine_shell::terminate_all_background_jobs();
}

/// Serve loop: the read loop handles control frames immediately (handshake,
/// shutdown, and cancel — cancellation must reach active work, so it can
/// never queue behind a running call) while calls dispatch on a single worker
/// thread, preserving the advertised `max_in_flight: 1` execution bound.
pub fn run_raw_worker_v2_serve(opts: &RawWorkerServeOptions) -> i32 {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let writer = Arc::new(Mutex::new(std::io::stdout()));
    let session_id =
        std::env::var("ZEROSTACK_SESSION_ID").unwrap_or_else(|_| "tokenzero-raw-worker".into());
    let session = Arc::new(Mutex::new(RawWorkerV2Session::for_binding(
        opts.root.to_string_lossy().into_owned(),
        session_id,
    )));
    crate::shell_hooks::install(crate::shell_hooks::ShellHooks::with_note_child(
        v2_note_child,
    ));
    let (tx, rx) = std::sync::mpsc::channel::<CallJob>();
    let worker = {
        let session = Arc::clone(&session);
        let writer = Arc::clone(&writer);
        let worker_opts = RawWorkerServeOptions {
            root: opts.root.clone(),
            cache_path: opts.cache_path.clone(),
            handshake_only: false,
            once_json: None,
        };
        std::thread::spawn(move || {
            let engine = engine_from_options(&worker_opts);
            while let Ok(job) = rx.recv() {
                let id = job.ctx.id.clone();
                let value = run_call_registered(&engine, job.ctx, job.cancel);
                if let Ok(mut guard) = session.lock() {
                    guard.finish_call(&id);
                }
                if write_response(&writer, &encode(value)).is_err() {
                    return;
                }
            }
        })
    };
    let exit_code = loop {
        match read_bounded_frame(&mut input, raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES) {
            Ok(BoundedFrame::Eof) => break 0,
            Ok(BoundedFrame::TooLarge) => {
                let response = encode(error(
                    None,
                    "frame_too_large",
                    "inbound frame exceeds 1 MiB",
                    None,
                ));
                if write_response(&writer, &response).is_err() {
                    break 2;
                }
            }
            Ok(BoundedFrame::Line(line)) => {
                let mut guard = session.lock().unwrap_or_else(|p| p.into_inner());
                match route_frame(&mut guard, &line) {
                    RoutedFrame::Respond(response) => {
                        let shutdown = guard.shutdown;
                        drop(guard);
                        if write_response(&writer, &response).is_err() {
                            break 2;
                        }
                        if shutdown {
                            break 0;
                        }
                    }
                    RoutedFrame::Dispatch(ctx) => {
                        let cancel = guard.register_cancel(&ctx.id);
                        drop(guard);
                        if tx.send(CallJob { ctx, cancel }).is_err() {
                            break 2;
                        }
                    }
                }
            }
            Err(_) => break 2,
        }
    };
    terminate_raw_worker_v2_session(&session);
    drop(tx);
    // Raw-worker entrypoints immediately pass this code to `process::exit`.
    // Joining here would let disconnected work retain the dedicated process
    // past the session boundary; dropping the handle lets process teardown
    // stop any non-cooperative in-process work after descendants are killed.
    drop(worker);
    exit_code
}

enum BoundedFrame {
    Eof,
    Line(Vec<u8>),
    TooLarge,
}

fn read_bounded_frame<R: BufRead>(reader: &mut R, maximum: usize) -> std::io::Result<BoundedFrame> {
    let mut line = Vec::with_capacity(4096);
    let mut too_large = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if line.is_empty() && !too_large {
                return Ok(BoundedFrame::Eof);
            }
            return Ok(if too_large || line.len() > maximum {
                BoundedFrame::TooLarge
            } else {
                BoundedFrame::Line(line)
            });
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        if !too_large {
            if line.len().saturating_add(take) > maximum.saturating_add(1) {
                too_large = true;
                line.clear();
            } else {
                line.extend_from_slice(&available[..take]);
            }
        }
        reader.consume(take);
        if newline.is_some() {
            let content = line.strip_suffix(b"\n").unwrap_or(&line);
            let content_len = content.strip_suffix(b"\r").unwrap_or(content).len();
            return Ok(if too_large || content_len > maximum {
                BoundedFrame::TooLarge
            } else {
                BoundedFrame::Line(line)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> TokenZeroEngine {
        engine_from_options(&RawWorkerServeOptions::default())
    }

    fn handshake(expected_revision: &str) -> Value {
        let cap = local_capability();
        json!({"kind":"handshake","request":{
            "protocol_version":"zerostack.raw_worker.v2","root":"/fixture/repo","session_id":"session-1",
            "expected_engine":"tokenzero","expected_worker_revision":expected_revision,
            "expected_contract_digest":cap["semantic_contract_digest"],
            "expected_registry_digest":cap["operation_registry_digest"]
        }})
    }

    fn send(session: &mut RawWorkerV2Session, frame: Value) -> Value {
        serde_json::from_slice(&execute_raw_worker_v2_frame(
            &engine(),
            session,
            &serde_json::to_vec(&frame).unwrap(),
        ))
        .unwrap()
    }

    #[test]
    fn golden_handshake_reports_binding_limits_and_metadata_capabilities() {
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        let response = send(&mut session, handshake(&rev));
        assert_eq!(response["kind"], "handshake_ack");
        assert_eq!(response["ack"]["binding"]["worker_revision"], rev);
        assert_eq!(response["ack"]["binding"]["ref_scheme"], "tz://");
        assert_eq!(response["ack"]["limits"]["max_frame_bytes"], 1_048_576);
        assert_eq!(response["ack"]["capabilities"]["cancellation"], true);
    }

    #[test]
    fn requested_worker_token_accounting_matches_domain_accounting_and_hub_abi() {
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        let cap = local_capability();
        send(&mut session, handshake(&rev));
        let trace = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req-accounting","trace_id":"trace",
            "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
        let response = send(
            &mut session,
            json!({"kind":"call","request":{
                "request_id":"req-accounting",
                "op":"mem",
                "args":{},
                "telemetry_request":{
                    "engine_stage_timeline":true,
                    "worker_token_accounting":true
                },
                "trace":trace
            }}),
        );
        assert_eq!(response["kind"], "result", "{response}");
        let accounting: raw_worker_v2_protocol::WorkerTokenAccountingV1 =
            serde_json::from_value(response["worker_token_accounting"].clone()).unwrap();
        zero_abi::validate_worker_token_accounting_v1(&accounting).unwrap();
        assert_eq!(
            accounting.count_kind,
            raw_worker_v2_protocol::WorkerTokenCountKind::Estimate
        );
        assert!(accounting.tokenizer_id.starts_with("estimator:"));
        let timeline: raw_worker_v2_protocol::EngineStageTimelineV1 =
            serde_json::from_value(response["engine_timeline"].clone()).unwrap();
        zero_abi::validate_engine_stage_timeline_v1(&timeline).unwrap();
        assert_eq!(timeline.spans[0].stage, "tokenzero.raw_worker_call");
        let domain = &response["result"]["value"]["accounting"];
        assert_eq!(
            accounting.raw_tokens,
            domain["raw_tokens"].as_u64().unwrap()
        );
        assert_eq!(
            accounting.visible_tokens,
            domain["visible_tokens"].as_u64().unwrap()
        );
        assert_eq!(
            accounting.recovery_tokens,
            domain["recovery_tokens"].as_u64().unwrap()
        );
        assert_eq!(
            accounting.billed_tokens,
            domain["billed_tokens"].as_u64().unwrap()
        );
        assert_eq!(
            accounting.cached_tokens,
            domain["cached_tokens"].as_u64().unwrap()
        );
        let encoded = serde_json::to_vec(&response).unwrap();
        let decoded = zero_abi::decode_response_frame(
            &encoded,
            raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES,
        )
        .unwrap();
        assert!(matches!(
            decoded,
            zero_abi::WorkerResponseFrame::Result {
                worker_token_accounting: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn unrequested_accounting_preserves_the_legacy_response_shape() {
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        let cap = local_capability();
        send(&mut session, handshake(&rev));
        let trace = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req-legacy","trace_id":"trace",
            "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
        let response = send(
            &mut session,
            json!({"kind":"call","request":{
                "request_id":"req-legacy","op":"mem","args":{},"trace":trace
            }}),
        );
        assert_eq!(response["kind"], "result", "{response}");
        assert!(response.get("worker_token_accounting").is_none());
        assert!(response.get("engine_timeline").is_none());
    }

    #[test]
    fn requested_timeline_is_preserved_on_typed_dispatch_errors() {
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        let cap = local_capability();
        send(&mut session, handshake(&rev));
        let trace = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req-error","trace_id":"trace",
            "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
        let response = send(
            &mut session,
            json!({"kind":"call","request":{
                "request_id":"req-error",
                "op":"execute_code",
                "args":{},
                "telemetry_request":{
                    "engine_stage_timeline":true,
                    "worker_token_accounting":true
                },
                "trace":trace
            }}),
        );
        assert_eq!(response["kind"], "error");
        assert_eq!(response["error"]["kind"], "unsupported_operation");
        assert!(response.get("worker_token_accounting").is_none());
        let timeline: raw_worker_v2_protocol::EngineStageTimelineV1 =
            serde_json::from_value(response["engine_timeline"].clone()).unwrap();
        zero_abi::validate_engine_stage_timeline_v1(&timeline).unwrap();
        zero_abi::decode_response_frame(
            &serde_json::to_vec(&response).unwrap(),
            raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES,
        )
        .unwrap();
    }

    #[test]
    fn missing_or_inconsistent_domain_accounting_fails_loudly() {
        assert!(worker_token_accounting(&json!({})).is_err());
        let error = worker_token_accounting(&json!({"accounting":{
            "raw_tokens":10,
            "visible_tokens":5,
            "recovery_tokens":0,
            "billed_tokens":1,
            "cached_tokens":2,
            "exact_ref_tokens":0
        }}))
        .unwrap_err();
        assert!(
            error.contains("cached_tokens exceeds billed_tokens"),
            "{error}"
        );
    }

    #[test]
    fn skew_and_pre_handshake_calls_are_rejected() {
        let mut session = RawWorkerV2Session::default();
        let response = send(&mut session, handshake("skewed"));
        assert_eq!(response["error"]["kind"], "worker_revision_changed");
        assert_eq!(response["error"]["retryable"], true);
        let response = send(
            &mut session,
            json!({"kind":"cancel","request":{"request_id":"r"}}),
        );
        assert_eq!(response["error"]["kind"], "handshake_required");
    }

    #[test]
    fn handshake_rebind_after_revision_swap_is_survivable() {
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        let first = send(&mut session, handshake(&rev));
        assert_eq!(first["kind"], "handshake_ack");
        // Same root+session re-handshakes cleanly (host-side revision swap).
        let second = send(&mut session, handshake(&rev));
        assert_eq!(second["kind"], "handshake_ack");
        assert_eq!(second["ack"]["binding"]["worker_revision"], rev);
    }

    #[test]
    fn rehandshake_with_foreign_session_stays_terminal() {
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        send(&mut session, handshake(&rev));
        let cap = local_capability();
        let foreign = json!({"kind":"handshake","request":{
            "protocol_version":"zerostack.raw_worker.v2","root":"/fixture/repo","session_id":"session-2",
            "expected_engine":"tokenzero","expected_worker_revision":rev,
            "expected_contract_digest":cap["semantic_contract_digest"],
            "expected_registry_digest":cap["operation_registry_digest"]
        }});
        let response = send(&mut session, foreign);
        assert_eq!(response["error"]["kind"], "already_bound");
        assert_eq!(response["error"]["retryable"], false);
    }

    #[test]
    fn stale_trace_revision_is_retryable_and_recoverable() {
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        let cap = local_capability();
        send(&mut session, handshake(&rev));
        let stale = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req","trace_id":"trace",
            "worker_revision":"stale-rev","contract_digest":cap["semantic_contract_digest"]});
        let response = send(
            &mut session,
            json!({"kind":"call","request":{"request_id":"req","op":"read",
            "args":{},"trace":stale}}),
        );
        assert_eq!(response["error"]["kind"], "worker_revision_changed");
        assert_eq!(response["error"]["retryable"], true);
        assert_eq!(response["trace"]["trace_id"], "trace");
        // Recovery: re-handshake, then the call dispatches with a fresh trace.
        let rebind = send(&mut session, handshake(&rev));
        assert_eq!(rebind["kind"], "handshake_ack");
        let fresh = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req","trace_id":"trace",
            "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
        let response = send(
            &mut session,
            json!({"kind":"call","request":{"request_id":"req","op":"read",
            "args":{},"trace":fresh}}),
        );
        assert_ne!(response["error"]["kind"], "worker_revision_changed");
    }

    #[test]
    fn inbound_bound_is_enforced_before_json_parse() {
        let mut session = RawWorkerV2Session::default();
        let response: Value = serde_json::from_slice(&execute_raw_worker_v2_frame(
            &engine(),
            &mut session,
            &vec![b'x'; 1_048_577],
        ))
        .unwrap();
        assert_eq!(response["error"]["kind"], "frame_too_large");
    }

    #[test]
    fn oversized_frame_fails_closed_without_parsing_request_id() {
        let mut session = RawWorkerV2Session::default();
        let mut frame = serde_json::to_vec(&json!({
            "kind": "call",
            "request": {"request_id": "req-oversized", "op": "token.read"}
        }))
        .unwrap();
        frame.extend(std::iter::repeat(b' ').take(1_048_600));
        let response: Value = serde_json::from_slice(&execute_raw_worker_v2_frame(
            &engine(),
            &mut session,
            &frame,
        ))
        .unwrap();
        assert_eq!(response["error"]["kind"], "frame_too_large");
        assert!(response.get("request_id").is_none());
    }

    #[test]
    fn in_bound_typed_frame_error_round_trips_request_id() {
        let mut session = RawWorkerV2Session::default();
        let line = br#"{"kind":"call","request":{"request_id":"req-7"},"extra":}"#.to_vec();
        let response: Value =
            serde_json::from_slice(&execute_raw_worker_v2_frame(&engine(), &mut session, &line))
                .unwrap();
        assert_eq!(response["error"]["kind"], "invalid_frame");
        assert!(response.get("request_id").is_none());
    }

    #[test]
    fn empty_and_malformed_frames_return_typed_invalid_frame() {
        let mut session = RawWorkerV2Session::default();
        for line in [b"\n".to_vec(), b"{not json".to_vec()] {
            let response: Value = serde_json::from_slice(&execute_raw_worker_v2_frame(
                &engine(),
                &mut session,
                &line,
            ))
            .unwrap();
            assert_eq!(response["error"]["kind"], "invalid_frame");
        }
    }

    #[test]
    fn bounded_reader_rejects_oversized_line_without_unbounded_growth() {
        let mut oversized = vec![b'x'; 1_048_600];
        oversized.push(b'\n');
        oversized.extend_from_slice(b"{}\n");
        let mut reader = std::io::BufReader::new(std::io::Cursor::new(oversized));
        let first =
            read_bounded_frame(&mut reader, raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES)
                .unwrap();
        assert!(matches!(first, BoundedFrame::TooLarge));
        let second =
            read_bounded_frame(&mut reader, raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES)
                .unwrap();
        assert!(matches!(second, BoundedFrame::Line(line) if line == b"{}\n"));
    }

    #[test]
    fn expired_deadline_preserves_typed_trace_and_cancel_truth() {
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        let cap = local_capability();
        send(&mut session, handshake(&rev));
        let trace = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req","trace_id":"trace",
            "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
        let response = send(
            &mut session,
            json!({"kind":"call","request":{"request_id":"req","op":"read",
            "args":{},"deadline_unix_ms":1,"trace":trace}}),
        );
        assert_eq!(response["error"]["kind"], "deadline_exceeded");
        assert_eq!(response["trace"]["trace_id"], "trace");
        let response = send(
            &mut session,
            json!({"kind":"cancel","request":{"request_id":"unknown"}}),
        );
        assert_eq!(response["cancelled"], false);
    }

    #[test]
    fn cross_root_replay_against_bound_session_fails_closed() {
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        send(&mut session, handshake(&rev));
        let cap = local_capability();
        let replay = json!({"kind":"handshake","request":{
            "protocol_version":"zerostack.raw_worker.v2","root":"/fixture/other","session_id":"session-1",
            "expected_engine":"tokenzero","expected_worker_revision":rev,
            "expected_contract_digest":cap["semantic_contract_digest"],
            "expected_registry_digest":cap["operation_registry_digest"]
        }});
        let response = send(&mut session, replay);
        assert_eq!(response["error"]["kind"], "already_bound");
        assert_eq!(response["error"]["retryable"], false);
    }

    #[test]
    fn registry_engine_and_contract_mismatches_fail_closed_typed() {
        let cap = local_capability();
        let rev = revision();
        for (field, bad) in [
            ("expected_registry_digest", json!("deadbeef")),
            ("expected_engine", json!("not-tokenzero")),
            ("expected_contract_digest", json!("deadbeef")),
        ] {
            let mut session = RawWorkerV2Session::default();
            let mut frame = handshake(&rev);
            frame["request"][field] = bad;
            let response = send(&mut session, frame);
            assert_eq!(
                response["error"]["kind"], "binding_mismatch",
                "field {field} must fail closed"
            );
            assert_eq!(response["error"]["retryable"], false);
            assert!(session.binding.is_none());
        }
        let _ = cap;
    }

    #[test]
    fn absent_optional_digest_pins_are_allowed_skew() {
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        let mut frame = handshake(&rev);
        let request = frame["request"].as_object_mut().unwrap();
        request.remove("expected_registry_digest");
        request.remove("expected_worker_revision");
        let response = send(&mut session, frame);
        assert_eq!(response["kind"], "handshake_ack");
    }

    /// Serializes tests that dispatch through the process-global
    /// ACTIVE_CANCEL slot so parallel test threads cannot clobber it.
    static DISPATCH_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn cancel_control_frame_stops_dispatched_shell_work() {
        let _dispatch_guard = DISPATCH_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        crate::shell_hooks::install(crate::shell_hooks::ShellHooks::with_note_child(
            v2_note_child,
        ));
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        let cap = local_capability();
        send(&mut session, handshake(&rev));
        let trace = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req-cancel","trace_id":"trace",
            "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
        let frame = json!({"kind":"call","request":{"request_id":"req-cancel","op":"shell",
            "args":{"command":"printf partial-before-cancel; sleep 30"},"trace":trace}});
        let ctx = validate_call(session.binding.as_ref().unwrap(), &frame).unwrap();
        let cancel = session.register_cancel(&ctx.id);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let engine = engine();
            // Engine construction is slow on cold stores; only the dispatch
            // window counts for the cancel bound.
            ready_tx.send(()).expect("ready signal");
            run_call_registered(&engine, ctx, cancel)
        });
        ready_rx.recv().expect("worker ready");
        let started = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(400));
        let ack = send(
            &mut session,
            json!({"kind":"cancel","request":{"request_id":"req-cancel"}}),
        );
        assert_eq!(ack["kind"], "cancel_ack");
        assert_eq!(ack["cancelled"], true);
        assert_eq!(ack["process_kill_supported"], cfg!(unix));
        let value = worker.join().expect("worker joins after cancel");
        assert_eq!(value["error"]["kind"], "cancelled");
        assert_eq!(value["error"]["retryable"], false);
        let details = &value["error"]["details"];
        assert_eq!(
            details["artifact_scope"],
            "full_observed_stdout_stderr_streams"
        );
        assert_eq!(details["temporal_interleaving_claimed"], false);
        let partial = &details["partial_result"]["tool_response"];
        assert_eq!(partial["safety"]["refs_cover_full_output"], true);
        assert_eq!(partial["telemetry"]["refs_cover_full_output"], true);
        let stdout_ref = partial["telemetry"]["stdout_ref"].as_str().unwrap();
        let expanded = engine().expand(stdout_ref, Some("raw"), None, None, None, None);
        assert_eq!(expanded.visible.unwrap().text, "partial-before-cancel");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "cancelled call must not run to completion"
        );
        crate::shell_hooks::reset();
    }

    #[test]
    fn cancel_of_unknown_request_id_reports_false() {
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        send(&mut session, handshake(&rev));
        let ack = send(
            &mut session,
            json!({"kind":"cancel","request":{"request_id":"missing"}}),
        );
        assert_eq!(ack["cancelled"], false);
    }

    #[test]
    fn deadline_reaches_dispatched_shell_work() {
        let _dispatch_guard = DISPATCH_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        let cap = local_capability();
        send(&mut session, handshake(&rev));
        let trace = json!({"runtime_id":"rt","cell_id":"cell","request_id":"req-dl","trace_id":"trace",
            "worker_revision":rev,"contract_digest":cap["semantic_contract_digest"]});
        let deadline = unix_ms() + 800;
        let started = std::time::Instant::now();
        let response = send(
            &mut session,
            json!({"kind":"call","request":{"request_id":"req-dl","op":"shell",
            "args":{"command":"sleep 30"},"deadline_unix_ms":deadline,"trace":trace}}),
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "deadline must reach the dispatched shell process"
        );
        let text = response.to_string();
        assert!(
            text.contains("\"timeout\":true") || text.contains("timed_out"),
            "shell run must report timeout enforcement: {text}"
        );
    }

    #[test]
    fn background_shell_and_job_use_the_shared_typed_private_path_free_boundary() {
        let _dispatch_guard = DISPATCH_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        let cap = local_capability();
        send(&mut session, handshake(&rev));
        let trace = |request_id: &str| {
            json!({
                "runtime_id":"rt",
                "cell_id":"cell",
                "request_id":request_id,
                "trace_id":request_id,
                "worker_revision":rev,
                "contract_digest":cap["semantic_contract_digest"],
            })
        };
        let command = if cfg!(windows) {
            "powershell -NoProfile -Command \"[Console]::Out.Write('tz://not-a-ref')\""
        } else {
            "printf 'tz://not-a-ref'"
        };
        let launched = send(
            &mut session,
            json!({"kind":"call","request":{
                "request_id":"req-job-launch",
                "op":"shell",
                "args":{"command":command,"background":true,"timeout_ms":2_000},
                "trace":trace("req-job-launch")
            }}),
        );
        assert_eq!(launched["kind"], "result", "{launched}");
        let launch = &launched["result"]["value"];
        assert_eq!(launch["cursor"], 0);
        assert_eq!(launch["version"], 0);
        assert!(
            launch.get("log").is_none(),
            "private log path leaked: {launch}"
        );
        assert_eq!(launch.as_object().unwrap().len(), 3, "{launch}");
        let id = launch["job"].as_str().unwrap().to_string();

        let polled = send(
            &mut session,
            json!({"kind":"call","request":{
                "request_id":"req-job-poll",
                "op":zero_abi::TOKEN_JOB_OPERATION_V1,
                "args":{"id":id,"waitMs":30_000,"since":0,"tailBytes":64},
                "trace":trace("req-job-poll")
            }}),
        );
        assert_eq!(polled["kind"], "result", "{polled}");
        let value = polled["result"]["value"].clone();
        assert!(
            value.get("log").is_none(),
            "private log path leaked: {value}"
        );
        let typed: zero_abi::TokenJobPollResultV1 = serde_json::from_value(value.clone()).unwrap();
        typed.validate().unwrap();
        assert!(typed.tail.contains("tz://not-a-ref"), "{value}");
        assert_eq!(polled["result"]["metadata"]["ownership"]["refs"], json!([]));

        let unknown = send(
            &mut session,
            json!({"kind":"call","request":{
                "request_id":"req-job-unknown",
                "op":zero_abi::TOKEN_JOB_OPERATION_V1,
                "args":{"id":id,"privateLog":"/private/session/job.log"},
                "trace":trace("req-job-unknown")
            }}),
        );
        assert_eq!(unknown["kind"], "error", "{unknown}");
        assert_eq!(unknown["error"]["kind"], "validation");
    }

    #[cfg(unix)]
    #[test]
    fn raw_session_shutdown_terminates_a_background_process_group() {
        struct ResetBackgroundTermination;
        impl Drop for ResetBackgroundTermination {
            fn drop(&mut self) {
                crate::engine_shell::reset_background_job_termination_for_tests();
            }
        }

        let _dispatch_guard = DISPATCH_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        crate::engine_shell::reset_background_job_termination_for_tests();
        let _reset = ResetBackgroundTermination;
        let mut session = RawWorkerV2Session::default();
        let rev = revision();
        let cap = local_capability();
        send(&mut session, handshake(&rev));
        let trace = json!({
            "runtime_id":"rt","cell_id":"cell","request_id":"req-long-job",
            "trace_id":"req-long-job","worker_revision":rev,
            "contract_digest":cap["semantic_contract_digest"],
        });
        let launched = send(
            &mut session,
            json!({"kind":"call","request":{
                "request_id":"req-long-job","op":"shell",
                "args":{"command":"sleep 30","background":true,"timeout_ms":60_000},
                "trace":trace
            }}),
        );
        assert_eq!(launched["kind"], "result", "{launched}");
        let id = launched["result"]["value"]["job"]
            .as_str()
            .unwrap()
            .to_string();
        let probe_engine = engine();
        let pid = (0..100)
            .find_map(|_| {
                let pid = probe_engine
                    .shell_job_wait(&id, std::time::Duration::ZERO, 0, 1)
                    .unwrap()["pid"]
                    .as_u64();
                if pid.is_none() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                pid
            })
            .expect("background child did not publish its pid");

        let shutdown = send(
            &mut session,
            json!({"kind":"shutdown","request":{"reason":"test"}}),
        );
        assert_eq!(shutdown["kind"], "shutdown_ack");
        let session = Mutex::new(session);
        terminate_raw_worker_v2_session(&session);
        let gone = (0..20).any(|_| {
            let status = std::process::Command::new("kill")
                .args(["-0", "--", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            if status.success() {
                std::thread::sleep(std::time::Duration::from_millis(50));
                false
            } else {
                true
            }
        });
        assert!(gone, "background child {pid} survived raw session shutdown");
    }

    #[test]
    fn v2_effect_class_marks_shell_and_compaction_storage_writes() {
        assert_eq!(effect_class("read"), "read_only");
        assert_eq!(effect_class("shell"), "irreversible");
        assert_eq!(effect_class("compact"), "irreversible");
        assert_eq!(effect_class("ingest"), "irreversible");
    }
}
