use super::*;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Default)]
pub struct RawWorkerV2Session {
    binding: Option<Binding>,
    shutdown: bool,
    expected_root: Option<String>,
    expected_session_id: Option<String>,
}

#[derive(Debug)]
struct Binding {
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
        }
    }
}

fn revision() -> String {
    std::env::var("ZEROSTACK_WORKER_REVISION")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

fn error(id: Option<&str>, kind: &str, message: impl Into<String>, trace: Option<Value>) -> Value {
    let mut value =
        json!({"kind":"error","error":{"kind":kind,"message":message.into(),"retryable":false}});
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
        value = error(
            None,
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

fn effect_class(op: &str) -> &'static str {
    match op {
        "shell"
        | "tz_shell"
        | "zero.shell"
        | "compact"
        | "tz_compact"
        | "zero.compact"
        | "ingest"
        | "tz_ingest"
        | "zero.ingest" => "irreversible",
        _ => "read_only",
    }
}

pub fn execute_raw_worker_v2_frame(
    engine: &TokenZeroEngine,
    session: &mut RawWorkerV2Session,
    line: &[u8],
) -> Vec<u8> {
    if line.len() > raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES {
        return encode(error(
            None,
            "frame_too_large",
            "inbound frame exceeds 1 MiB",
            None,
        ));
    }
    if let Err(e) = raw_worker_v2_protocol::decode_request_frame(
        line,
        raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES,
    ) {
        return encode(error(None, "invalid_frame", e.to_string(), None));
    }
    let frame: Value = match serde_json::from_slice(line) {
        Ok(v) => v,
        Err(e) => return encode(error(None, "invalid_json", e.to_string(), None)),
    };
    let kind = frame["kind"].as_str().unwrap_or_default();
    let request = &frame["request"];

    if kind == "handshake" {
        if session.binding.is_some() {
            return encode(error(
                None,
                "already_bound",
                "session is already bound",
                None,
            ));
        }
        let cap = local_capability();
        let rev = revision();
        let contract = cap["semantic_contract_digest"].as_str().unwrap_or_default();
        let registry = cap["operation_registry_digest"]
            .as_str()
            .unwrap_or_default();
        let root = request["root"].as_str().unwrap_or_default();
        let session_id = request["session_id"].as_str().unwrap_or_default();
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
                .is_some_and(|v| v != registry)
            || request
                .get("expected_worker_revision")
                .and_then(Value::as_str)
                .is_some_and(|v| v != rev);
        if mismatch {
            return encode(error(
                None,
                "binding_mismatch",
                "worker handshake binding mismatch",
                None,
            ));
        }
        session.binding = Some(Binding {
            session_id: session_id.into(),
            revision: rev.clone(),
            contract: contract.into(),
        });
        return encode(json!({"kind":"handshake_ack","ack":{
            "protocol_version":raw_worker_v2_protocol::RAW_WORKER_PROTOCOL_VERSION,
            "binding":{"engine":"tokenzero","root":root,"session_id":session_id,"worker_revision":rev,
                "semantic_contract_version":cap["semantic_contract_version"],"semantic_contract_digest":contract,
                "operation_registry_digest":registry,"ref_scheme":"tz"},
            "capabilities":{"cancellation":false,"deadlines":true,"approvals":false,"revert":false,"snapshots":false},
            "limits":{"max_frame_bytes":1048576,"max_output_bytes":65536,"max_in_flight":1,"default_deadline_ms":30000},
            "protocol_digest":raw_worker_v2_protocol::raw_worker_protocol_digest_hex()
        }}));
    }
    if kind == "shutdown" {
        session.shutdown = true;
        return encode(json!({"kind":"shutdown_ack"}));
    }
    let Some(binding) = session.binding.as_ref() else {
        return encode(error(
            None,
            "handshake_required",
            "v2 handshake required before calls",
            None,
        ));
    };
    if session.shutdown {
        return encode(error(
            None,
            "session_shutdown",
            "session has shut down",
            None,
        ));
    }
    if kind == "cancel" {
        return encode(
            json!({"kind":"cancel_ack","request_id":request["request_id"],"cancelled":false}),
        );
    }

    let id = request["request_id"].as_str().unwrap_or_default();
    let op = request["op"].as_str().unwrap_or_default();
    let trace = request["trace"].clone();
    if trace["request_id"].as_str() != Some(id)
        || trace["worker_revision"].as_str() != Some(binding.revision.as_str())
        || trace["contract_digest"].as_str() != Some(binding.contract.as_str())
    {
        return encode(error(
            Some(id),
            "trace_binding_mismatch",
            "trace does not match handshake binding",
            Some(trace),
        ));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if request
        .get("deadline_unix_ms")
        .and_then(Value::as_u64)
        .is_some_and(|v| v <= now)
    {
        return encode(error(
            Some(id),
            "deadline_exceeded",
            "deadline expired before dispatch",
            Some(trace),
        ));
    }
    if forbidden(op) {
        return encode(error(
            Some(id),
            "unsupported_operation",
            "planner, JavaScript, and MCP operations are forbidden",
            Some(trace),
        ));
    }
    let v1 =
        json!({"id":id,"op":op,"args":request["args"],"peer_contract_digest":binding.contract});
    let response = execute_raw_worker_json(engine, &v1);
    if response["ok"].as_bool() != Some(true) {
        let e = &response["error"];
        return encode(json!({"kind":"error","request_id":id,"error":{
            "kind":e["kind"].as_str().unwrap_or("operation_failed"),
            "message":e["message"].as_str().unwrap_or("operation failed"),
            "retryable":e["retryable"].as_bool().unwrap_or(false),"details":e.get("details").cloned().unwrap_or(Value::Null)
        },"trace":trace}));
    }
    let value = response.get("result").cloned().unwrap_or(Value::Null);
    let mut owned_refs = Vec::new();
    refs(&value, &mut owned_refs);
    encode(
        json!({"kind":"result","request_id":id,"result":{"value":value,"metadata":{
            "effect":effect_class(op),
            "approval":{"state":"not_required"},"revert":{"supported":false},
            "ownership":{"engine":"tokenzero","session_id":binding.session_id,"refs":owned_refs},"trace":trace
        }}}),
    )
}

pub fn run_raw_worker_v2_serve(opts: &RawWorkerServeOptions) -> i32 {
    let engine = engine_from_options(opts);
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let session_id =
        std::env::var("ZEROSTACK_SESSION_ID").unwrap_or_else(|_| "tokenzero-raw-worker".into());
    let mut session =
        RawWorkerV2Session::for_binding(opts.root.to_string_lossy().into_owned(), session_id);
    loop {
        match read_bounded_frame(&mut input, raw_worker_v2_protocol::DEFAULT_MAX_FRAME_BYTES) {
            Ok(BoundedFrame::Eof) => return 0,
            Ok(BoundedFrame::TooLarge) => {
                let response = encode(error(
                    None,
                    "frame_too_large",
                    "inbound frame exceeds 1 MiB",
                    None,
                ));
                if output
                    .write_all(&response)
                    .and_then(|_| output.flush())
                    .is_err()
                {
                    return 2;
                }
            }
            Ok(BoundedFrame::Line(line)) => {
                let response = execute_raw_worker_v2_frame(&engine, &mut session, &line);
                if output
                    .write_all(&response)
                    .and_then(|_| output.flush())
                    .is_err()
                {
                    return 2;
                }
                if session.shutdown {
                    return 0;
                }
            }
            Err(_) => return 2,
        }
    }
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
        assert_eq!(response["ack"]["binding"]["ref_scheme"], "tz");
        assert_eq!(response["ack"]["limits"]["max_frame_bytes"], 1_048_576);
        assert_eq!(response["ack"]["capabilities"]["cancellation"], false);
    }

    #[test]
    fn skew_and_pre_handshake_calls_are_rejected() {
        let mut session = RawWorkerV2Session::default();
        let response = send(&mut session, handshake("skewed"));
        assert_eq!(response["error"]["kind"], "binding_mismatch");
        let response = send(
            &mut session,
            json!({"kind":"cancel","request":{"request_id":"r"}}),
        );
        assert_eq!(response["error"]["kind"], "handshake_required");
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
    fn v2_effect_class_marks_shell_and_compaction_storage_writes() {
        assert_eq!(effect_class("read"), "read_only");
        assert_eq!(effect_class("shell"), "irreversible");
        assert_eq!(effect_class("compact"), "irreversible");
        assert_eq!(effect_class("ingest"), "irreversible");
    }
}
