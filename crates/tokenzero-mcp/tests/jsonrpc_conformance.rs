use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;
use tokenzero_mcp::{EngineConfig, TokenZeroEngine, handle_jsonrpc};

struct Server {
    dir: TempDir,
    engine: TokenZeroEngine,
}

impl Server {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
        Self { dir, engine }
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.dir.path().join(name)
    }

    fn response(&self, input: Value) -> Value {
        self.raw(&input.to_string()).unwrap_or_else(|| panic!("expected response to {input}"))
    }

    fn raw(&self, input: &str) -> Option<Value> {
        handle_jsonrpc(&self.engine, input).map(|text| serde_json::from_str(&text).unwrap())
    }
}

fn req(id: &str, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

#[derive(Clone, Copy)]
enum Level {
    Must,
    Should,
}

enum Input {
    Json(Value),
    Raw(&'static str),
}

enum Reply {
    None,
    Result(Value),
    Error(Value, i64),
    Batch(Vec<Reply>),
}
type RpcCase = (&'static str, &'static str, Level, &'static str, Input, Reply);
type InvalidCase = (&'static str, &'static str, Value, &'static str);

fn rpc_cases() -> Vec<RpcCase> {
    use Input::{Json, Raw};
    use Level::{Must, Should};
    use Reply::{Batch, Error, None, Result};
    vec![
        ("JSONRPC-2.0-PARSE-001", "JSON-RPC 2.0 Parse", Must, "Malformed JSON returns a parse error with null id", Raw("{bad"), Error(Value::Null, -32700)),
        ("JSONRPC-2.0-REQ-001", "JSON-RPC 2.0 Request Object", Must, "A valid request object returns a response with the same id", Json(json!({"jsonrpc":"2.0","id":1,"method":"ping"})), Result(json!(1))),
        ("JSONRPC-2.0-REQ-002", "JSON-RPC 2.0 Request Object", Must, "The jsonrpc member must be exactly 2.0", Json(json!({"jsonrpc":"1.0","id":"bad-version","method":"ping"})), Error(json!("bad-version"), -32600)),
        ("JSONRPC-2.0-REQ-003", "JSON-RPC 2.0 Request Object", Must, "A missing jsonrpc member is an invalid request", Json(json!({"id":"missing-version","method":"ping"})), Error(json!("missing-version"), -32600)),
        ("JSONRPC-2.0-REQ-004", "JSON-RPC 2.0 Request Object", Must, "A request must be a JSON object", Json(json!(1)), Error(Value::Null, -32600)),
        ("JSONRPC-2.0-REQ-005", "JSON-RPC 2.0 Request Object", Must, "The method member must be a string", Json(json!({"jsonrpc":"2.0","id":"bad-method","method":7})), Error(json!("bad-method"), -32600)),
        ("JSONRPC-2.0-REQ-006", "JSON-RPC 2.0 Request Object", Must, "The id member must be a string, number, or null", Json(json!({"jsonrpc":"2.0","id":{"not":"valid"},"method":"ping"})), Error(Value::Null, -32600)),
        ("JSONRPC-2.0-REQ-007", "JSON-RPC 2.0 Request Object", Should, "The params member should be an object or array when present", Json(json!({"jsonrpc":"2.0","id":"bad-params","method":"ping","params":true})), Error(json!("bad-params"), -32600)),
        ("JSONRPC-2.0-NOTIF-001", "JSON-RPC 2.0 Notifications", Must, "A request object without id is a notification and gets no response", Json(json!({"jsonrpc":"2.0","method":"ping"})), None),
        ("JSONRPC-2.0-NOTIF-002", "JSON-RPC 2.0 Notifications", Must, "Unknown notification methods still get no response", Json(json!({"jsonrpc":"2.0","method":"unknown/notification"})), None),
        ("MCP-2025-06-18-INITIALIZED-NOTIF-001", "MCP 2025-06-18 Initialized Notification", Must, "notifications/initialized is a notification and gets no response", Json(json!({"jsonrpc":"2.0","method":"notifications/initialized"})), None),
        ("JSONRPC-2.0-METHOD-001", "JSON-RPC 2.0 Method Dispatch", Must, "Unknown request methods return Method not found", Json(json!({"jsonrpc":"2.0","id":"unknown","method":"unknown/request"})), Error(json!("unknown"), -32601)),
        ("JSONRPC-2.0-BATCH-001", "JSON-RPC 2.0 Batch", Must, "A batch returns responses only for requests, not notifications", Json(json!([{"jsonrpc":"2.0","id":"batch-ping","method":"ping"},{"jsonrpc":"2.0","method":"ping"}])), Batch(vec![Result(json!("batch-ping"))])),
        ("JSONRPC-2.0-BATCH-002", "JSON-RPC 2.0 Batch", Must, "An empty batch is an invalid request", Json(json!([])), Error(Value::Null, -32600)),
        ("JSONRPC-2.0-BATCH-003", "JSON-RPC 2.0 Batch", Must, "A batch containing only notifications gets no response", Json(json!([{"jsonrpc":"2.0","method":"ping"},{"jsonrpc":"2.0","method":"unknown/notification"}])), None),
        ("JSONRPC-2.0-BATCH-004", "JSON-RPC 2.0 Batch", Must, "Invalid batch elements return invalid request errors alongside valid responses", Json(json!([1,{"jsonrpc":"2.0","id":"batch-valid","method":"ping"}])), Batch(vec![Error(Value::Null, -32600), Result(json!("batch-valid"))])),
    ]
}

#[test]
fn jsonrpc_request_envelope_conformance_matrix() {
    let server = Server::new();
    let cases = rpc_cases();
    let failures: Vec<_> = cases.iter().filter_map(|case| {
        run_case(&server, case).err().map(|reason| format!("{}: {}: {reason}", case.0, case.3))
    }).collect();
    assert!(failures.is_empty(), "{}", render_report(&cases, &failures));
}

#[test]
fn jsonrpc_invalid_params_errors_include_structured_data() {
    let server = Server::new();
    let cases = [
        ("missing tool name", json!({"jsonrpc":"2.0","id":"missing-tool","method":"tools/call","params":{}}), "missing_param", "param", json!("name")),
        ("unknown tool", json!({"jsonrpc":"2.0","id":"unknown-tool","method":"tools/call","params":{"name":"does_not_exist","arguments":{}}}), "unknown_tool", "tool", json!("does_not_exist")),
        ("unknown resource", json!({"jsonrpc":"2.0","id":"unknown-resource","method":"resources/read","params":{"uri":"resource://tokenzero/missing"}}), "unknown_resource", "uri", json!("resource://tokenzero/missing")),
    ];
    for (label, input, kind, field, expected) in cases {
        let actual = server.response(input);
        let data = &actual["error"]["data"];
        assert_eq!(actual["error"]["code"], -32602, "{label}: {actual}");
        assert!(data.is_object(), "{label}: expected object data, got {data}");
        assert_eq!(data["kind"], kind, "{label}: {actual}");
        assert_eq!(data[field], expected, "{label}: {actual}");
        assert!(data["reason"].as_str().is_some_and(|s| !s.is_empty()), "{label}: missing reason in {actual}");
        match kind {
            "unknown_tool" => assert!(array_has(data, "available_tools", "tz_read"), "{label}: missing available tool list in {actual}"),
            "unknown_resource" => assert!(array_has(data, "available_resources", "resource://tokenzero/capabilities"), "{label}: missing available resource list in {actual}"),
            _ => {}
        }
    }
}

#[test]
fn jsonrpc_protocol_errors_include_structured_data() {
    let server = Server::new();
    let cases = [
        ("parse error", Input::Raw("{bad"), -32700, Value::Null, "parse_error", None),
        ("invalid non-object request", Input::Json(json!(1)), -32600, Value::Null, "invalid_request", None),
        ("invalid params envelope", Input::Json(json!({"jsonrpc":"2.0","id":"bad-params-envelope","method":"ping","params":true})), -32600, json!("bad-params-envelope"), "invalid_request", None),
        ("unknown method", Input::Json(json!({"jsonrpc":"2.0","id":"unknown-method","method":"unknown/request"})), -32601, json!("unknown-method"), "unknown_method", Some(("method", json!("unknown/request")))),
    ];
    for (label, input, code, id, kind, field) in cases {
        let payload = match input {
            Input::Json(value) => value.to_string(),
            Input::Raw(text) => text.into(),
        };
        let actual = server.raw(&payload).unwrap_or_else(|| panic!("{label}: expected protocol error response"));
        let data = &actual["error"]["data"];
        assert_eq!(actual["id"], id, "{label}: {actual}");
        assert_eq!(actual["error"]["code"], code, "{label}: {actual}");
        assert_eq!(data["kind"], kind, "{label}: {actual}");
        assert_protocol_error_data(data).unwrap_or_else(|reason| panic!("{label}: {reason}"));
        if let Some((name, expected)) = field {
            assert_eq!(data[name], expected, "{label}: {actual}");
        }
        if kind == "unknown_method" {
            assert!(array_has(data, "available_methods", "tools/list"), "{label}: missing available method list in {actual}");
            assert!(array_has(data, "available_methods", "notifications/initialized"), "{label}: missing initialized notification compatibility method in {actual}");
        }
    }
}

fn initialize_params(version: &str) -> Value {
    json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"tokenzero-conformance-client","version":"1.0.0"}})
}

fn assert_init(result: &Value, version: &str) {
    assert_eq!(result["protocolVersion"], version);
    for capability in ["logging", "tools", "resources", "prompts"] {
        assert!(result["capabilities"][capability].is_object());
    }
    assert_eq!(result["serverInfo"]["name"], "tokenzero");
    assert!(result["serverInfo"]["version"].as_str().is_some_and(|s| !s.is_empty()), "{result}");
}

fn assert_negotiation(value: &Value, requested: &str, negotiated: &str, fallback: bool) {
    assert_eq!(value["requestedProtocolVersion"], requested);
    assert_eq!(value["negotiatedProtocolVersion"], negotiated);
    assert_eq!(value["fallback"], fallback);
}

#[test]
fn mcp_initialize_conformance_matrix() {
    let server = Server::new();
    let stable = server.response(json!({"jsonrpc":"2.0","id":"init-stable","method":"initialize","params":initialize_params("2025-06-18")}));
    assert_init(&stable["result"], "2025-06-18");
    assert_negotiation(&stable["result"]["_meta"]["tokenzero/protocolNegotiation"], "2025-06-18", "2025-06-18", false);

    let initialized = server.response(json!({"jsonrpc":"2.0","id":"initialized-legacy-request","method":"notifications/initialized","params":{}}));
    assert_eq!(initialized["result"], json!({}));

    let unsupported = server.response(json!({"jsonrpc":"2.0","id":"init-unsupported","method":"initialize","params":initialize_params("1900-01-01")}));
    assert_init(&unsupported["result"], "2025-06-18");
    let negotiation = &unsupported["result"]["_meta"]["tokenzero/protocolNegotiation"];
    assert_negotiation(negotiation, "1900-01-01", "2025-06-18", true);
    assert!(array_has(negotiation, "supportedProtocolVersions", "2025-06-18"), "{unsupported}");

    assert_invalid_cases(&server, &[
        ("MCP-2025-06-18-INIT-PARAMS-001", "initialize params are required", json!({"jsonrpc":"2.0","id":"init-no-params","method":"initialize"}), "missing_param"),
        ("MCP-2025-06-18-INIT-PARAMS-002", "initialize params must be object", json!({"jsonrpc":"2.0","id":"init-array","method":"initialize","params":[]}), "invalid_params"),
        ("MCP-2025-06-18-INIT-PROTOCOL-001", "initialize protocolVersion is required", json!({"jsonrpc":"2.0","id":"init-no-version","method":"initialize","params":{"capabilities":{},"clientInfo":{"name":"client","version":"1.0.0"}}}), "missing_param"),
        ("MCP-2025-06-18-INIT-PROTOCOL-002", "initialize protocolVersion must be string", json!({"jsonrpc":"2.0","id":"init-number-version","method":"initialize","params":{"protocolVersion":1,"capabilities":{},"clientInfo":{"name":"client","version":"1.0.0"}}}), "invalid_params"),
        ("MCP-2025-06-18-INIT-CAPS-001", "initialize capabilities are required", json!({"jsonrpc":"2.0","id":"init-no-caps","method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"client","version":"1.0.0"}}}), "missing_param"),
        ("MCP-2025-06-18-INIT-CAPS-002", "initialize capabilities must be object", json!({"jsonrpc":"2.0","id":"init-array-caps","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":[],"clientInfo":{"name":"client","version":"1.0.0"}}}), "invalid_params"),
        ("MCP-2025-06-18-INIT-CLIENT-001", "initialize clientInfo is required", json!({"jsonrpc":"2.0","id":"init-no-client","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}), "missing_param"),
        ("MCP-2025-06-18-INIT-CLIENT-002", "initialize clientInfo must be object", json!({"jsonrpc":"2.0","id":"init-array-client","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":[]}}), "invalid_params"),
        ("MCP-2025-06-18-INIT-CLIENT-003", "initialize clientInfo.name is required", json!({"jsonrpc":"2.0","id":"init-client-no-name","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"version":"1.0.0"}}}), "missing_param"),
        ("MCP-2025-06-18-INIT-CLIENT-004", "initialize clientInfo.version is required", json!({"jsonrpc":"2.0","id":"init-client-no-version","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"client"}}}), "missing_param"),
        ("MCP-2025-06-18-INIT-CLIENT-005", "initialize clientInfo.title must be string when present", json!({"jsonrpc":"2.0","id":"init-client-title-number","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"client","version":"1.0.0","title":7}}}), "invalid_params"),
    ]);
}

#[test]
fn mcp_logging_set_level_conformance_matrix() {
    let server = Server::new();
    for level in ["debug", "info", "notice", "warning", "error", "critical", "alert", "emergency"] {
        let response = server.response(json!({"jsonrpc":"2.0","id":format!("log-{level}"),"method":"logging/setLevel","params":{"level":level}}));
        assert!(response["result"].is_object(), "{level}: {response}");
        assert!(response.get("error").is_none(), "{level}: {response}");
    }
    assert_invalid_cases(&server, &[
        ("MCP-2025-06-18-LOGGING-LEVEL-001", "logging/setLevel params are required", json!({"jsonrpc":"2.0","id":"log-no-params","method":"logging/setLevel"}), "missing_param"),
        ("MCP-2025-06-18-LOGGING-LEVEL-002", "logging/setLevel params must be object", json!({"jsonrpc":"2.0","id":"log-array-params","method":"logging/setLevel","params":[]}), "invalid_params"),
        ("MCP-2025-06-18-LOGGING-LEVEL-003", "logging/setLevel level is required", json!({"jsonrpc":"2.0","id":"log-missing-level","method":"logging/setLevel","params":{}}), "missing_param"),
        ("MCP-2025-06-18-LOGGING-LEVEL-004", "logging/setLevel level must be string", json!({"jsonrpc":"2.0","id":"log-number-level","method":"logging/setLevel","params":{"level":1}}), "invalid_params"),
        ("MCP-2025-06-18-LOGGING-LEVEL-005", "logging/setLevel level must be a valid syslog severity", json!({"jsonrpc":"2.0","id":"log-trace-level","method":"logging/setLevel","params":{"level":"trace"}}), "invalid_param_value"),
    ]);
    let missing = server.response(json!({"jsonrpc":"2.0","id":"log-missing-level-options","method":"logging/setLevel","params":{}}));
    assert!(array_has(&missing["error"]["data"], "available_options", "info"), "{missing}");
    let invalid = server.response(json!({"jsonrpc":"2.0","id":"log-invalid-level-options","method":"logging/setLevel","params":{"level":"trace"}}));
    let data = &invalid["error"]["data"];
    assert_eq!(data["parameter"], "level");
    assert_eq!(data["provided"], "trace");
    assert!(array_has(data, "available_levels", "warning"), "{invalid}");
    assert_eq!(data["suggested_tool_calls"][0]["method"], "logging/setLevel", "{invalid}");
}

#[test]
fn mcp_server_discover_conformance_matrix() {
    let server = Server::new();
    let discovered = server.response(json!({"jsonrpc":"2.0","id":"discover-draft","method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientInfo":{"name":"tokenzero-conformance-client","version":"1.0.0"},"io.modelcontextprotocol/clientCapabilities":{}}}}));
    let result = &discovered["result"];
    assert_eq!(result["resultType"], "complete", "{discovered}");
    assert!(array_has(result, "supportedVersions", "2026-07-28"), "{discovered}");
    assert!(result["capabilities"]["tools"].is_object(), "server/discover must report tool capabilities: {discovered}");
    assert!(result["capabilities"]["resources"].is_object(), "server/discover must report resource capabilities: {discovered}");
    assert_non_empty_string(&result["serverInfo"]["name"], "server/discover must report serverInfo.name");
    assert_non_empty_string(&result["serverInfo"]["version"], "server/discover must report serverInfo.version");
    assert_non_empty_string(&result["instructions"], "server/discover optional instructions, when present");
    assert_eq!(result["_meta"]["clientMetaAccepted"], true, "server/discover should preserve that standard _meta was accepted: {discovered}");
    assert_eq!(result["protocolVersions"], result["supportedVersions"], "legacy protocolVersions alias must stay in lockstep with draft supportedVersions: {discovered}");

    let no_params = server.response(json!({"jsonrpc":"2.0","id":"discover-no-params","method":"server/discover"}));
    assert_eq!(no_params["result"]["resultType"], "complete", "{no_params}");
    assert_eq!(no_params["result"]["_meta"]["clientMetaAccepted"], false, "{no_params}");
    assert_invalid_cases(&server, &[
        ("MCP-DRAFT-DISCOVER-PARAMS-001", "server/discover params must be object when present", json!({"jsonrpc":"2.0","id":"discover-array","method":"server/discover","params":[]}), "invalid_params"),
        ("MCP-DRAFT-DISCOVER-PARAMS-002", "server/discover params._meta must be object when present", json!({"jsonrpc":"2.0","id":"discover-bad-meta","method":"server/discover","params":{"_meta":[]}}), "invalid_params"),
        ("MCP-DRAFT-DISCOVER-PARAMS-003", "server/discover carries no body params beyond standard _meta", json!({"jsonrpc":"2.0","id":"discover-extra","method":"server/discover","params":{"protocolVersion":"2026-07-28"}}), "invalid_params"),
    ]);
}

#[test]
fn mcp_method_params_conformance_matrix() {
    let server = Server::new();
    assert_invalid_cases(&server, &[
        ("MCP-2025-06-18-TOOLS-LIST-PARAMS-001", "tools/list params must be an object when present", json!({"jsonrpc":"2.0","id":"tools-list-array","method":"tools/list","params":[]}), "invalid_params"),
        ("MCP-2025-06-18-RESOURCES-LIST-PARAMS-001", "resources/list params must be an object when present", json!({"jsonrpc":"2.0","id":"resources-list-array","method":"resources/list","params":[]}), "invalid_params"),
        ("MCP-2025-06-18-RESOURCES-TEMPLATES-LIST-PARAMS-001", "resources/templates/list params must be an object when present", json!({"jsonrpc":"2.0","id":"resources-templates-list-array","method":"resources/templates/list","params":[]}), "invalid_params"),
        ("MCP-2025-06-18-PROMPTS-LIST-PARAMS-001", "prompts/list params must be an object when present", json!({"jsonrpc":"2.0","id":"prompts-list-array","method":"prompts/list","params":[]}), "invalid_params"),
        ("MCP-2025-06-18-RESOURCES-READ-PARAMS-001", "resources/read params must be an object with uri", json!({"jsonrpc":"2.0","id":"resources-read-array","method":"resources/read","params":[]}), "invalid_params"),
        ("MCP-2025-06-18-RESOURCES-READ-PARAMS-002", "resources/read params.uri must be a string", json!({"jsonrpc":"2.0","id":"resources-read-uri-number","method":"resources/read","params":{"uri":7}}), "invalid_params"),
        ("MCP-2025-06-18-TOOLS-CALL-PARAMS-001", "tools/call params must be an object with name", json!({"jsonrpc":"2.0","id":"tools-call-array","method":"tools/call","params":[]}), "invalid_params"),
        ("MCP-2025-06-18-TOOLS-CALL-PARAMS-002", "tools/call params.name must be a string", json!({"jsonrpc":"2.0","id":"tools-call-name-number","method":"tools/call","params":{"name":7}}), "invalid_params"),
        ("MCP-2025-06-18-TOOLS-CALL-ARGS-001", "tools/call arguments must be an object when present", json!({"jsonrpc":"2.0","id":"tools-call-args-array","method":"tools/call","params":{"name":"shell","arguments":["echo","should-not-run"]}}), "invalid_params"),
        ("MCP-2025-06-18-TOOLS-LIST-CURSOR-001", "tools/list params.cursor must be a string when present", json!({"jsonrpc":"2.0","id":"tools-list-cursor-number","method":"tools/list","params":{"cursor":7}}), "invalid_params"),
        ("MCP-2025-06-18-RESOURCES-LIST-CURSOR-001", "resources/list params.cursor must be a string when present", json!({"jsonrpc":"2.0","id":"resources-list-cursor-number","method":"resources/list","params":{"cursor":7}}), "invalid_params"),
        ("MCP-2025-06-18-RESOURCES-TEMPLATES-LIST-CURSOR-001", "resources/templates/list params.cursor must be a string when present", json!({"jsonrpc":"2.0","id":"resources-templates-list-cursor-number","method":"resources/templates/list","params":{"cursor":7}}), "invalid_params"),
        ("MCP-2025-06-18-PROMPTS-LIST-CURSOR-001", "prompts/list params.cursor must be a string when present", json!({"jsonrpc":"2.0","id":"prompts-list-cursor-number","method":"prompts/list","params":{"cursor":7}}), "invalid_params"),
    ]);
}

#[test]
fn mcp_result_shape_conformance_matrix() {
    let server = Server::new();
    let fixture = server.path("fixture.txt");
    fs::write(&fixture, "tokenzero conformance fixture\n").unwrap();

    let resources = server.response(req("resources-shape", "resources/list", json!({})));
    assert_list_shape(&resources, "resources", "resources/list");
    let resources_array = resources["result"]["resources"].as_array().unwrap();
    assert!(!resources_array.is_empty(), "{resources}");
    for resource in resources_array {
        for field in ["uri", "name", "mimeType"] {
            assert_non_empty_string(&resource[field], &format!("resources/list resource.{field}"));
        }
        if let Some(description) = resource.get("description") {
            assert_non_empty_string(description, "resources/list resource.description");
        }
    }

    let templates = server.response(req("resource-templates-shape", "resources/templates/list", json!({})));
    assert_list_shape(&templates, "resourceTemplates", "resources/templates/list");
    assert!(templates["result"]["resourceTemplates"].as_array().unwrap().is_empty(), "TokenZero exposes concrete resources, not URI templates: {templates}");

    let prompts = server.response(req("prompts-shape", "prompts/list", json!({})));
    assert_list_shape(&prompts, "prompts", "prompts/list");
    let tools = server.response(req("tools-shape", "tools/list", json!({})));
    assert_list_shape(&tools, "tools", "tools/list");
    let tools_array = tools["result"]["tools"].as_array().unwrap();
    assert!(!tools_array.is_empty(), "{tools}");
    for tool in tools_array {
        assert_non_empty_string(&tool["name"], "{label} tool.name");
        assert_non_empty_string(&tool["description"], "{label} tool.description");
        assert_eq!(tool["inputSchema"]["type"], "object", "tools/list tool.inputSchema must be an object schema: {tool}");
    }

    let read = server.response(req("read-resource-shape", "resources/read", json!({"uri":"resource://tokenzero/capabilities"})));
    assert!(read["result"].get("contents").is_some(), "{read}");
    let contents = read["result"]["contents"].as_array().unwrap();
    assert!(!contents.is_empty(), "{read}");
    for content in contents {
        for field in ["uri", "mimeType", "text"] {
            assert_non_empty_string(&content[field], &format!("resources/read contents[].{field}"));
        }
    }

    let call = server.response(req("tool-call-shape", "tools/call", json!({"name":"read","arguments":{"path":fixture.display().to_string(),"raw":true}})));
    assert!(call.get("error").is_none(), "{call}");
    let result = &call["result"];
    let content = result["content"].as_array().unwrap();
    assert!(!content.is_empty(), "{call}");
    assert_eq!(content[0]["type"], "text", "{call}");
    assert_non_empty_string(&content[0]["text"], "tools/call content[].text");
    assert!(result.get("structuredContent").is_none(), "tools/call results are text-only unless TOKENZERO_MCP_ENVELOPE is set: {call}");
    assert!(result.get("isError").is_none() || result["isError"] == false, "successful tools/call must not report isError true: {call}");
}

#[test]
fn mcp_tool_result_conformance_marks_tool_originated_errors() {
    let server = Server::new();
    let actual = server.response(req("tool-origin-error", "tools/call", json!({"name":"read","arguments":{"path":"/__tokenzero_conformance_outside_root__"}})));
    assert!(actual.get("error").is_none(), "tool-originated errors must not be protocol errors: {actual}");
    assert_eq!(actual["result"]["isError"], true, "{actual}");
    assert!(actual["result"]["content"][0]["text"].as_str().is_some_and(|s| s.contains("outside allowed roots")), "{actual}");
}

#[test]
fn recall_tool_searches_stored_payloads_end_to_end() {
    let server = Server::new();
    let file = server.path("data.txt");
    fs::write(&file, "unique_recall_marker line\n").unwrap();
    server.response(req("recall-seed", "tools/call", json!({"name":"read","arguments":{"path":file.display().to_string()}})));
    let recalled = server.response(req("recall-query", "tools/call", json!({"name":"recall","arguments":{"query":"UNIQUE_RECALL_MARKER","max_hits":"5"}})));
    let text = recalled["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("unique_recall_marker"), "{text}");
    assert!(text.contains("tz://"), "{text}");
}

#[test]
fn zero_hit_search_renders_note_above_refs_footer() {
    let server = Server::new();
    fs::write(server.path("lib.rs"), "fn alpha() {}\n").unwrap();
    let response = server.response(req("zero-hit-grep", "tools/call", json!({"name":"grep","arguments":{"query":"no_such_token","path":server.dir.path().display().to_string()}})));
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("# grep no_such_token — 0 matches"), "{text}");
    assert!(lines.next().is_some_and(|line| line.starts_with("refs: tz://blob/")), "refs must stay recoverable from the text content: {text}");
}

#[test]
fn tools_call_edit_applies_stub_string_edits_end_to_end() {
    let server = Server::new();
    let file = server.path("lib.rs");
    fs::write(&file, "fn alpha() {}\nfn beta() {}\n").unwrap();
    let listed = server.response(json!({"jsonrpc":"2.0","id":"edit-list","method":"tools/list","params":{}}));
    let names = tool_names(&listed);
    assert!(names.iter().any(|n| *n == "tz_edit"), "{names:?}");
    assert!(names.iter().any(|n| *n == "edit"), "{names:?}");

    let docs = server.response(json!({"jsonrpc":"2.0","id":"edit-docs","method":"resources/read","params":{"uri":"resource://tokenzero/tools"}}));
    let docs_payload: Value = serde_json::from_str(docs["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert!(docs_payload["tools"].as_array().unwrap().iter().any(|tool| tool["name"] == "tz_edit"), "tz_edit must appear in the resource catalog");

    let edits = serde_json::to_string(&json!([{"find":"fn alpha() {}","replace":"fn alpha() -> u8 { 1 }","replace_all":"false"}])).unwrap();
    let response = server.response(req("edit-call", "tools/call", json!({"name":"edit","arguments":{"path":file.display().to_string(),"edits":edits,"dry_run":"false"}})));
    assert!(response.get("error").is_none(), "{response}");
    let result = &response["result"];
    assert!(result.get("isError").is_none() || result["isError"] == false, "{response}");
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.lines().next().is_some_and(|line| line.starts_with(&format!("# edit {} — 1 hunks applied", file.display()))), "{text}");
    let footer = text.lines().find(|line| line.starts_with("refs: ")).unwrap_or_else(|| panic!("missing refs footer: {text}"));
    assert!(footer.contains("tz://blob/"), "{footer}");
    let undo_ref = footer.split_whitespace().find_map(|part| part.strip_prefix("undo:")).unwrap_or_else(|| panic!("missing labeled undo ref: {footer}"));
    assert!(undo_ref.starts_with("tz://blob/"), "{footer}");
    assert_eq!(fs::read_to_string(&file).unwrap(), "fn alpha() -> u8 { 1 }\nfn beta() {}\n");
    let expanded = server.response(req("edit-undo-expand", "tools/call", json!({"name":"expand","arguments":{"ref":undo_ref}})));
    let expanded_text = expanded["result"]["content"][0]["text"].as_str().unwrap();
    assert!(expanded_text.contains("fn alpha() {}"), "{expanded_text}");
}

#[test]
fn tools_list_supports_agent_friendly_cluster_filtering() {
    let server = Server::new();
    let default = server.response(json!({"jsonrpc":"2.0","id":"tools-default","method":"tools/list","params":{}}));
    let default_names = tool_names(&default);
    assert!(default_names.len() > 7, "default remains full compatibility catalog: {default}");
    assert!(default_names.iter().any(|n| *n == "tz_read"));
    assert!(default_names.iter().any(|n| *n == "read"));

    let material = server.response(json!({"jsonrpc":"2.0","id":"tools-material","method":"tools/list","params":{"_meta":{"tokenzero/toolCluster":"material"}}}));
    let material_names = tool_names(&material);
    assert!(material_names.len() <= 7, "material cluster must stay small enough for agent menus: {material}");
    for name in ["tz_read", "tz_expand"] {
        assert!(material_names.iter().any(|candidate| *candidate == name));
    }
    for name in ["tz_shell", "read"] {
        assert!(!material_names.iter().any(|candidate| *candidate == name));
    }
    assert_eq!(material["result"]["_meta"]["tokenzero/toolFilter"]["cluster"], "material");
    assert_eq!(material["result"]["_meta"]["tokenzero/toolFilter"]["includeAliases"], false);

    let execution = server.response(json!({"jsonrpc":"2.0","id":"tools-execution","method":"tools/list","params":{"profile":"execution"}}));
    let execution_names = tool_names(&execution);
    assert!(execution_names.len() <= 7, "execution cluster must stay small enough for agent menus: {execution}");
    assert!(execution_names.iter().any(|n| *n == "tz_shell"));
    assert!(!execution_names.iter().any(|n| *n == "tz_read"));

    let aliased = server.response(json!({"jsonrpc":"2.0","id":"tools-material-aliases","method":"tools/list","params":{"_meta":{"tokenzero/toolCluster":"material","tokenzero/includeAliases":true}}}));
    let aliased_names = tool_names(&aliased);
    for name in ["tz_read", "read"] {
        assert!(aliased_names.iter().any(|candidate| *candidate == name));
    }
    assert_eq!(aliased["result"]["_meta"]["tokenzero/toolFilter"]["includeAliases"], true);

    let invalid = server.response(json!({"jsonrpc":"2.0","id":"tools-bad-cluster","method":"tools/list","params":{"_meta":{"tokenzero/toolCluster":"matrial"}}}));
    let data = &invalid["error"]["data"];
    assert_eq!(invalid["error"]["code"], -32602, "{invalid}");
    assert_eq!(data["kind"], "unknown_tool_cluster", "{invalid}");
    assert_eq!(data["error_type"], "INVALID_ARGUMENT", "{invalid}");
    assert!(array_has(data, "available_options", "material"), "{invalid}");
    assert_eq!(data["suggestions"][0]["value"], "material", "{invalid}");
}

fn assert_invalid_cases(server: &Server, cases: &[InvalidCase]) {
    for (id, description, input, kind) in cases {
        let actual = server.response(input.clone());
        assert_eq!(actual["error"]["code"], -32602, "{id}: {description}: {actual}");
        assert_eq!(actual["error"]["data"]["kind"], *kind, "{id}: {description}: {actual}");
        assert_protocol_error_data(&actual["error"]["data"]).unwrap_or_else(|reason| panic!("{id}: {description}: {reason}"));
    }
}

fn array_has(value: &Value, field: &str, needle: &str) -> bool {
    value[field].as_array().is_some_and(|items| items.iter().any(|item| item == needle))
}

fn tool_names(response: &Value) -> Vec<&str> {
    response["result"]["tools"].as_array().unwrap().iter().filter_map(|tool| tool["name"].as_str()).collect()
}

fn assert_list_shape(response: &Value, key: &str, label: &str) {
    assert!(response.get("error").is_none(), "{label}: {response}");
    assert!(response["result"][key].is_array(), "{label}: result.{key} must be array: {response}");
    if let Some(cursor) = response["result"].get("nextCursor") {
        assert!(cursor.is_string(), "{label}: result.nextCursor must be string when present: {response}");
    }
}

fn assert_non_empty_string(value: &Value, label: &str) {
    assert!(value.as_str().is_some_and(|s| !s.is_empty()), "{label} must be a non-empty string, got {value}");
}

fn run_case(server: &Server, case: &RpcCase) -> Result<(), String> {
    let payload = match &case.4 {
        Input::Json(value) => value.to_string(),
        Input::Raw(text) => (*text).into(),
    };
    let actual = server.raw(&payload);
    match (&case.5, actual) {
        (Reply::None, None) => Ok(()),
        (Reply::None, Some(value)) => Err(format!("expected no response, got {value}")),
        (Reply::Result(id), Some(value)) => assert_result(&value, id),
        (Reply::Result(_), None) => Err("expected result response, got no response".into()),
        (Reply::Error(id, code), Some(value)) => assert_error(&value, id, *code),
        (Reply::Error(_, _), None) => Err("expected error response, got no response".into()),
        (Reply::Batch(expected), Some(Value::Array(actual))) => {
            if actual.len() != expected.len() {
                return Err(format!(
                    "expected {} batch responses, got {}: {actual:?}",
                    expected.len(),
                    actual.len()
                ));
            }
            for (index, (expected, actual)) in expected.iter().zip(&actual).enumerate() {
                let result = match expected {
                    Reply::Result(id) => assert_result(actual, id),
                    Reply::Error(id, code) => assert_error(actual, id, *code),
                    _ => unreachable!(),
                };
                result.map_err(|reason| format!("batch[{index}]: {reason}"))?;
            }
            Ok(())
        }
        (Reply::Batch(_), Some(value)) => Err(format!("expected batch array, got {value}")),
        (Reply::Batch(_), None) => Err("expected batch response, got no response".into()),
    }
}

fn assert_result(actual: &Value, id: &Value) -> Result<(), String> {
    if actual["jsonrpc"] != "2.0" {
        return Err(format!("missing jsonrpc 2.0 in {actual}"));
    }
    if &actual["id"] != id {
        return Err(format!("expected id {id}, got {}", actual["id"]));
    }
    if !actual.get("result").is_some_and(Value::is_object) {
        return Err(format!("expected object result, got {actual}"));
    }
    if actual.get("error").is_some() {
        return Err(format!("result response included error: {actual}"));
    }
    Ok(())
}

fn assert_error(actual: &Value, id: &Value, code: i64) -> Result<(), String> {
    if actual["jsonrpc"] != "2.0" {
        return Err(format!("missing jsonrpc 2.0 in {actual}"));
    }
    if &actual["id"] != id {
        return Err(format!("expected id {id}, got {}", actual["id"]));
    }
    if actual["error"]["code"] != code {
        return Err(format!(
            "expected error code {code}, got {} in {actual}",
            actual["error"]["code"]
        ));
    }
    if actual.get("result").is_some() {
        return Err(format!("error response included result: {actual}"));
    }
    assert_protocol_error_data(&actual["error"]["data"])
}

fn assert_protocol_error_data(data: &Value) -> Result<(), String> {
    if !data.is_object() {
        return Err(format!("expected object error.data, got {data}"));
    }
    for field in ["kind", "reason", "fix_hint"] {
        if data[field].as_str().is_none_or(str::is_empty) {
            return Err(format!("missing data.{field} in {data}"));
        }
    }
    if !data["recoverable"].is_boolean() {
        return Err(format!("missing data.recoverable in {data}"));
    }
    Ok(())
}

fn render_report(cases: &[RpcCase], failures: &[String]) -> String {
    let mut sections = std::collections::BTreeMap::<&str, (usize, usize, usize)>::new();
    for case in cases {
        let entry = sections.entry(case.1).or_default();
        match case.2 {
            Level::Must => entry.0 += 1,
            Level::Should => entry.1 += 1,
        }
        entry.2 += 1;
    }
    let mut report = String::from("# MCP JSON-RPC Conformance Report\n\n| Section | MUST | SHOULD | Tested |\n|---------|------|--------|--------|\n");
    for (section, (must, should, tested)) in sections {
        report.push_str(&format!("| {section} | {must} | {should} | {tested} |\n"));
    }
    report.push_str("\nFailures:\n");
    if failures.is_empty() {
        report.push_str("- none\n");
    }
    for failure in failures {
        report.push_str(&format!("- {failure}\n"));
    }
    report
}
