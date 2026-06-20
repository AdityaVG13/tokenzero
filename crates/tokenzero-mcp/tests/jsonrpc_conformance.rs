use serde_json::{Value, json};
use std::fs;
use tempfile::tempdir;
use tokenzero_mcp::{EngineConfig, TokenZeroEngine, handle_jsonrpc};

#[derive(Clone, Copy)]
enum RequirementLevel {
    Must,
    Should,
}

struct ConformanceCase {
    id: &'static str,
    section: &'static str,
    level: RequirementLevel,
    description: &'static str,
    input: CaseInput,
    expected: Expected,
}

enum CaseInput {
    Json(Value),
    Raw(&'static str),
}

enum Expected {
    NoResponse,
    Result { id: Value },
    Error { id: Value, code: i64 },
    Batch(Vec<BatchExpected>),
}

enum BatchExpected {
    Result { id: Value },
    Error { id: Value, code: i64 },
}

fn jsonrpc_cases() -> Vec<ConformanceCase> {
    vec![
        ConformanceCase {
            id: "JSONRPC-2.0-PARSE-001",
            section: "JSON-RPC 2.0 Parse",
            level: RequirementLevel::Must,
            description: "Malformed JSON returns a parse error with null id",
            input: CaseInput::Raw("{bad"),
            expected: Expected::Error {
                id: Value::Null,
                code: -32700,
            },
        },
        ConformanceCase {
            id: "JSONRPC-2.0-REQ-001",
            section: "JSON-RPC 2.0 Request Object",
            level: RequirementLevel::Must,
            description: "A valid request object returns a response with the same id",
            input: CaseInput::Json(json!({"jsonrpc":"2.0","id":1,"method":"ping"})),
            expected: Expected::Result { id: json!(1) },
        },
        ConformanceCase {
            id: "JSONRPC-2.0-REQ-002",
            section: "JSON-RPC 2.0 Request Object",
            level: RequirementLevel::Must,
            description: "The jsonrpc member must be exactly 2.0",
            input: CaseInput::Json(json!({"jsonrpc":"1.0","id":"bad-version","method":"ping"})),
            expected: Expected::Error {
                id: json!("bad-version"),
                code: -32600,
            },
        },
        ConformanceCase {
            id: "JSONRPC-2.0-REQ-003",
            section: "JSON-RPC 2.0 Request Object",
            level: RequirementLevel::Must,
            description: "A missing jsonrpc member is an invalid request",
            input: CaseInput::Json(json!({"id":"missing-version","method":"ping"})),
            expected: Expected::Error {
                id: json!("missing-version"),
                code: -32600,
            },
        },
        ConformanceCase {
            id: "JSONRPC-2.0-REQ-004",
            section: "JSON-RPC 2.0 Request Object",
            level: RequirementLevel::Must,
            description: "A request must be a JSON object",
            input: CaseInput::Json(json!(1)),
            expected: Expected::Error {
                id: Value::Null,
                code: -32600,
            },
        },
        ConformanceCase {
            id: "JSONRPC-2.0-REQ-005",
            section: "JSON-RPC 2.0 Request Object",
            level: RequirementLevel::Must,
            description: "The method member must be a string",
            input: CaseInput::Json(json!({"jsonrpc":"2.0","id":"bad-method","method":7})),
            expected: Expected::Error {
                id: json!("bad-method"),
                code: -32600,
            },
        },
        ConformanceCase {
            id: "JSONRPC-2.0-REQ-006",
            section: "JSON-RPC 2.0 Request Object",
            level: RequirementLevel::Must,
            description: "The id member must be a string, number, or null",
            input: CaseInput::Json(json!({"jsonrpc":"2.0","id":{"not":"valid"},"method":"ping"})),
            expected: Expected::Error {
                id: Value::Null,
                code: -32600,
            },
        },
        ConformanceCase {
            id: "JSONRPC-2.0-REQ-007",
            section: "JSON-RPC 2.0 Request Object",
            level: RequirementLevel::Should,
            description: "The params member should be an object or array when present",
            input: CaseInput::Json(
                json!({"jsonrpc":"2.0","id":"bad-params","method":"ping","params":true}),
            ),
            expected: Expected::Error {
                id: json!("bad-params"),
                code: -32600,
            },
        },
        ConformanceCase {
            id: "JSONRPC-2.0-NOTIF-001",
            section: "JSON-RPC 2.0 Notifications",
            level: RequirementLevel::Must,
            description: "A request object without id is a notification and gets no response",
            input: CaseInput::Json(json!({"jsonrpc":"2.0","method":"ping"})),
            expected: Expected::NoResponse,
        },
        ConformanceCase {
            id: "JSONRPC-2.0-NOTIF-002",
            section: "JSON-RPC 2.0 Notifications",
            level: RequirementLevel::Must,
            description: "Unknown notification methods still get no response",
            input: CaseInput::Json(json!({"jsonrpc":"2.0","method":"unknown/notification"})),
            expected: Expected::NoResponse,
        },
        ConformanceCase {
            id: "MCP-2025-06-18-INITIALIZED-NOTIF-001",
            section: "MCP 2025-06-18 Initialized Notification",
            level: RequirementLevel::Must,
            description: "notifications/initialized is a notification and gets no response",
            input: CaseInput::Json(json!({"jsonrpc":"2.0","method":"notifications/initialized"})),
            expected: Expected::NoResponse,
        },
        ConformanceCase {
            id: "JSONRPC-2.0-METHOD-001",
            section: "JSON-RPC 2.0 Method Dispatch",
            level: RequirementLevel::Must,
            description: "Unknown request methods return Method not found",
            input: CaseInput::Json(
                json!({"jsonrpc":"2.0","id":"unknown","method":"unknown/request"}),
            ),
            expected: Expected::Error {
                id: json!("unknown"),
                code: -32601,
            },
        },
        ConformanceCase {
            id: "JSONRPC-2.0-BATCH-001",
            section: "JSON-RPC 2.0 Batch",
            level: RequirementLevel::Must,
            description: "A batch returns responses only for requests, not notifications",
            input: CaseInput::Json(json!([
                {"jsonrpc":"2.0","id":"batch-ping","method":"ping"},
                {"jsonrpc":"2.0","method":"ping"}
            ])),
            expected: Expected::Batch(vec![BatchExpected::Result {
                id: json!("batch-ping"),
            }]),
        },
        ConformanceCase {
            id: "JSONRPC-2.0-BATCH-002",
            section: "JSON-RPC 2.0 Batch",
            level: RequirementLevel::Must,
            description: "An empty batch is an invalid request",
            input: CaseInput::Json(json!([])),
            expected: Expected::Error {
                id: Value::Null,
                code: -32600,
            },
        },
        ConformanceCase {
            id: "JSONRPC-2.0-BATCH-003",
            section: "JSON-RPC 2.0 Batch",
            level: RequirementLevel::Must,
            description: "A batch containing only notifications gets no response",
            input: CaseInput::Json(json!([
                {"jsonrpc":"2.0","method":"ping"},
                {"jsonrpc":"2.0","method":"unknown/notification"}
            ])),
            expected: Expected::NoResponse,
        },
        ConformanceCase {
            id: "JSONRPC-2.0-BATCH-004",
            section: "JSON-RPC 2.0 Batch",
            level: RequirementLevel::Must,
            description: "Invalid batch elements return invalid request errors alongside valid responses",
            input: CaseInput::Json(json!([
                1,
                {"jsonrpc":"2.0","id":"batch-valid","method":"ping"}
            ])),
            expected: Expected::Batch(vec![
                BatchExpected::Error {
                    id: Value::Null,
                    code: -32600,
                },
                BatchExpected::Result {
                    id: json!("batch-valid"),
                },
            ]),
        },
    ]
}

#[test]
fn jsonrpc_request_envelope_conformance_matrix() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let cases = jsonrpc_cases();
    let mut failures = Vec::new();

    for case in &cases {
        if let Err(reason) = run_case(&engine, case) {
            failures.push(format!("{}: {}: {reason}", case.id, case.description));
        }
    }

    assert!(failures.is_empty(), "{}", render_report(&cases, &failures));
}

#[test]
fn jsonrpc_invalid_params_errors_include_structured_data() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let cases = [
        (
            "missing tool name",
            json!({"jsonrpc":"2.0","id":"missing-tool","method":"tools/call","params":{}}),
            "missing_param",
            "param",
            json!("name"),
        ),
        (
            "unknown tool",
            json!({"jsonrpc":"2.0","id":"unknown-tool","method":"tools/call","params":{"name":"does_not_exist","arguments":{}}}),
            "unknown_tool",
            "tool",
            json!("does_not_exist"),
        ),
        (
            "unknown resource",
            json!({"jsonrpc":"2.0","id":"unknown-resource","method":"resources/read","params":{"uri":"resource://tokenzero/missing"}}),
            "unknown_resource",
            "uri",
            json!("resource://tokenzero/missing"),
        ),
    ];

    for (label, input, kind, field, expected_value) in cases {
        let response = handle_jsonrpc(&engine, &input.to_string())
            .unwrap_or_else(|| panic!("{label}: expected error response"));
        let actual: Value = serde_json::from_str(&response).unwrap();
        let data = &actual["error"]["data"];

        assert_eq!(actual["error"]["code"], -32602, "{label}: {actual}");
        assert!(
            data.is_object(),
            "{label}: expected object data, got {data}"
        );
        assert_eq!(data["kind"], kind, "{label}: {actual}");
        assert_eq!(data[field], expected_value, "{label}: {actual}");
        assert!(
            data["reason"]
                .as_str()
                .is_some_and(|reason| !reason.is_empty()),
            "{label}: missing reason in {actual}"
        );

        match kind {
            "unknown_tool" => assert!(
                data["available_tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool == "tz_read"),
                "{label}: missing available tool list in {actual}"
            ),
            "unknown_resource" => assert!(
                data["available_resources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|uri| uri == "resource://tokenzero/capabilities"),
                "{label}: missing available resource list in {actual}"
            ),
            _ => {}
        }
    }
}

#[test]
fn jsonrpc_protocol_errors_include_structured_data() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let cases = [
        (
            "parse error",
            CaseInput::Raw("{bad"),
            -32700,
            Value::Null,
            "parse_error",
            None,
        ),
        (
            "invalid non-object request",
            CaseInput::Json(json!(1)),
            -32600,
            Value::Null,
            "invalid_request",
            None,
        ),
        (
            "invalid params envelope",
            CaseInput::Json(json!({
                "jsonrpc":"2.0",
                "id":"bad-params-envelope",
                "method":"ping",
                "params":true
            })),
            -32600,
            json!("bad-params-envelope"),
            "invalid_request",
            None,
        ),
        (
            "unknown method",
            CaseInput::Json(json!({
                "jsonrpc":"2.0",
                "id":"unknown-method",
                "method":"unknown/request"
            })),
            -32601,
            json!("unknown-method"),
            "unknown_method",
            Some(("method", json!("unknown/request"))),
        ),
    ];

    for (label, input, code, id, kind, field) in cases {
        let payload = match input {
            CaseInput::Json(value) => value.to_string(),
            CaseInput::Raw(text) => text.to_string(),
        };
        let response = handle_jsonrpc(&engine, &payload)
            .unwrap_or_else(|| panic!("{label}: expected protocol error response"));
        let actual: Value = serde_json::from_str(&response).unwrap();
        let data = &actual["error"]["data"];

        assert_eq!(actual["id"], id, "{label}: {actual}");
        assert_eq!(actual["error"]["code"], code, "{label}: {actual}");
        assert_eq!(data["kind"], kind, "{label}: {actual}");
        assert_protocol_error_data(data).unwrap_or_else(|reason| panic!("{label}: {reason}"));

        if let Some((field_name, expected_value)) = field {
            assert_eq!(data[field_name], expected_value, "{label}: {actual}");
        }
        if kind == "unknown_method" {
            assert!(
                data["available_methods"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|method| method == "tools/list"),
                "{label}: missing available method list in {actual}"
            );
            assert!(
                data["available_methods"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|method| method == "notifications/initialized"),
                "{label}: missing initialized notification compatibility method in {actual}"
            );
        }
    }
}

#[test]
fn mcp_initialize_conformance_matrix() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let stable = response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "init-stable",
            "method": "initialize",
            "params": initialize_params("2025-06-18")
        }),
    );
    assert_eq!(stable["result"]["protocolVersion"], "2025-06-18");
    let stable_negotiation = &stable["result"]["_meta"]["tokenzero/protocolNegotiation"];
    assert_eq!(stable_negotiation["requestedProtocolVersion"], "2025-06-18");
    assert_eq!(
        stable_negotiation["negotiatedProtocolVersion"],
        "2025-06-18"
    );
    assert_eq!(stable_negotiation["fallback"], false);
    assert!(stable["result"]["capabilities"]["logging"].is_object());
    assert!(stable["result"]["capabilities"]["tools"].is_object());
    assert!(stable["result"]["capabilities"]["resources"].is_object());
    assert!(stable["result"]["capabilities"]["prompts"].is_object());
    assert_eq!(stable["result"]["serverInfo"]["name"], "tokenzero");
    assert!(
        stable["result"]["serverInfo"]["version"]
            .as_str()
            .is_some_and(|version| !version.is_empty()),
        "{stable}"
    );

    let initialized_with_id = response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "initialized-legacy-request",
            "method": "notifications/initialized",
            "params": {}
        }),
    );
    assert_eq!(initialized_with_id["result"], json!({}));

    let unsupported = response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "init-unsupported",
            "method": "initialize",
            "params": initialize_params("1900-01-01")
        }),
    );
    assert_eq!(unsupported["result"]["protocolVersion"], "2025-06-18");
    let unsupported_negotiation = &unsupported["result"]["_meta"]["tokenzero/protocolNegotiation"];
    assert_eq!(
        unsupported_negotiation["requestedProtocolVersion"],
        "1900-01-01"
    );
    assert_eq!(
        unsupported_negotiation["negotiatedProtocolVersion"],
        "2025-06-18"
    );
    assert_eq!(unsupported_negotiation["fallback"], true);
    assert!(
        unsupported_negotiation["supportedProtocolVersions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|version| version == "2025-06-18"),
        "{unsupported}"
    );

    let invalid_cases = [
        (
            "MCP-2025-06-18-INIT-PARAMS-001",
            "initialize params are required",
            json!({"jsonrpc":"2.0","id":"init-no-params","method":"initialize"}),
            "missing_param",
        ),
        (
            "MCP-2025-06-18-INIT-PARAMS-002",
            "initialize params must be object",
            json!({"jsonrpc":"2.0","id":"init-array","method":"initialize","params":[]}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-INIT-PROTOCOL-001",
            "initialize protocolVersion is required",
            json!({"jsonrpc":"2.0","id":"init-no-version","method":"initialize","params":{"capabilities":{},"clientInfo":{"name":"client","version":"1.0.0"}}}),
            "missing_param",
        ),
        (
            "MCP-2025-06-18-INIT-PROTOCOL-002",
            "initialize protocolVersion must be string",
            json!({"jsonrpc":"2.0","id":"init-number-version","method":"initialize","params":{"protocolVersion":1,"capabilities":{},"clientInfo":{"name":"client","version":"1.0.0"}}}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-INIT-CAPS-001",
            "initialize capabilities are required",
            json!({"jsonrpc":"2.0","id":"init-no-caps","method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"client","version":"1.0.0"}}}),
            "missing_param",
        ),
        (
            "MCP-2025-06-18-INIT-CAPS-002",
            "initialize capabilities must be object",
            json!({"jsonrpc":"2.0","id":"init-array-caps","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":[],"clientInfo":{"name":"client","version":"1.0.0"}}}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-INIT-CLIENT-001",
            "initialize clientInfo is required",
            json!({"jsonrpc":"2.0","id":"init-no-client","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}),
            "missing_param",
        ),
        (
            "MCP-2025-06-18-INIT-CLIENT-002",
            "initialize clientInfo must be object",
            json!({"jsonrpc":"2.0","id":"init-array-client","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":[]}}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-INIT-CLIENT-003",
            "initialize clientInfo.name is required",
            json!({"jsonrpc":"2.0","id":"init-client-no-name","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"version":"1.0.0"}}}),
            "missing_param",
        ),
        (
            "MCP-2025-06-18-INIT-CLIENT-004",
            "initialize clientInfo.version is required",
            json!({"jsonrpc":"2.0","id":"init-client-no-version","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"client"}}}),
            "missing_param",
        ),
        (
            "MCP-2025-06-18-INIT-CLIENT-005",
            "initialize clientInfo.title must be string when present",
            json!({"jsonrpc":"2.0","id":"init-client-title-number","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"client","version":"1.0.0","title":7}}}),
            "invalid_params",
        ),
    ];

    for (case_id, description, input, expected_kind) in invalid_cases {
        let actual = response_json(&engine, input);
        assert_invalid_params_kind(&actual, expected_kind, case_id, description);
    }
}

#[test]
fn mcp_logging_set_level_conformance_matrix() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    for level in [
        "debug",
        "info",
        "notice",
        "warning",
        "error",
        "critical",
        "alert",
        "emergency",
    ] {
        let response = response_json(
            &engine,
            json!({"jsonrpc":"2.0","id":format!("log-{level}"),"method":"logging/setLevel","params":{"level":level}}),
        );
        assert!(response["result"].is_object(), "{level}: {response}");
        assert!(response.get("error").is_none(), "{level}: {response}");
    }

    let invalid_cases = [
        (
            "MCP-2025-06-18-LOGGING-LEVEL-001",
            "logging/setLevel params are required",
            json!({"jsonrpc":"2.0","id":"log-no-params","method":"logging/setLevel"}),
            "missing_param",
        ),
        (
            "MCP-2025-06-18-LOGGING-LEVEL-002",
            "logging/setLevel params must be object",
            json!({"jsonrpc":"2.0","id":"log-array-params","method":"logging/setLevel","params":[]}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-LOGGING-LEVEL-003",
            "logging/setLevel level is required",
            json!({"jsonrpc":"2.0","id":"log-missing-level","method":"logging/setLevel","params":{}}),
            "missing_param",
        ),
        (
            "MCP-2025-06-18-LOGGING-LEVEL-004",
            "logging/setLevel level must be string",
            json!({"jsonrpc":"2.0","id":"log-number-level","method":"logging/setLevel","params":{"level":1}}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-LOGGING-LEVEL-005",
            "logging/setLevel level must be a valid syslog severity",
            json!({"jsonrpc":"2.0","id":"log-trace-level","method":"logging/setLevel","params":{"level":"trace"}}),
            "invalid_param_value",
        ),
    ];

    for (case_id, description, input, expected_kind) in invalid_cases {
        let actual = response_json(&engine, input);
        assert_invalid_params_kind(&actual, expected_kind, case_id, description);
    }

    let missing_level = response_json(
        &engine,
        json!({"jsonrpc":"2.0","id":"log-missing-level-options","method":"logging/setLevel","params":{}}),
    );
    assert!(
        missing_level["error"]["data"]["available_options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|level| level == "info"),
        "{missing_level}"
    );

    let invalid_level = response_json(
        &engine,
        json!({"jsonrpc":"2.0","id":"log-invalid-level-options","method":"logging/setLevel","params":{"level":"trace"}}),
    );
    let data = &invalid_level["error"]["data"];
    assert_eq!(data["parameter"], "level");
    assert_eq!(data["provided"], "trace");
    assert!(
        data["available_levels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|level| level == "warning"),
        "{invalid_level}"
    );
    assert_eq!(
        data["suggested_tool_calls"][0]["method"], "logging/setLevel",
        "{invalid_level}"
    );
}

#[test]
fn mcp_server_discover_conformance_matrix() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let discovered = response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "discover-draft",
            "method": "server/discover",
            "params": {
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientInfo": {
                        "name": "tokenzero-conformance-client",
                        "version": "1.0.0"
                    },
                    "io.modelcontextprotocol/clientCapabilities": {}
                }
            }
        }),
    );
    let result = &discovered["result"];
    assert_eq!(result["resultType"], "complete", "{discovered}");
    assert!(
        result["supportedVersions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|version| version == "2026-07-28"),
        "{discovered}"
    );
    assert!(
        result["capabilities"]["tools"].is_object(),
        "server/discover must report tool capabilities: {discovered}"
    );
    assert!(
        result["capabilities"]["resources"].is_object(),
        "server/discover must report resource capabilities: {discovered}"
    );
    assert!(
        result["serverInfo"]["name"]
            .as_str()
            .is_some_and(|name| !name.is_empty()),
        "server/discover must report serverInfo.name: {discovered}"
    );
    assert!(
        result["serverInfo"]["version"]
            .as_str()
            .is_some_and(|version| !version.is_empty()),
        "server/discover must report serverInfo.version: {discovered}"
    );
    assert_non_empty_string(
        &result["instructions"],
        "server/discover optional instructions, when present",
    );
    assert_eq!(
        result["_meta"]["clientMetaAccepted"], true,
        "server/discover should preserve that standard _meta was accepted: {discovered}"
    );
    assert_eq!(
        result["protocolVersions"], result["supportedVersions"],
        "legacy protocolVersions alias must stay in lockstep with draft supportedVersions: {discovered}"
    );

    let no_params = response_json(
        &engine,
        json!({"jsonrpc":"2.0","id":"discover-no-params","method":"server/discover"}),
    );
    assert_eq!(no_params["result"]["resultType"], "complete", "{no_params}");
    assert_eq!(
        no_params["result"]["_meta"]["clientMetaAccepted"], false,
        "{no_params}"
    );

    let invalid_cases = [
        (
            "MCP-DRAFT-DISCOVER-PARAMS-001",
            "server/discover params must be object when present",
            json!({"jsonrpc":"2.0","id":"discover-array","method":"server/discover","params":[]}),
        ),
        (
            "MCP-DRAFT-DISCOVER-PARAMS-002",
            "server/discover params._meta must be object when present",
            json!({"jsonrpc":"2.0","id":"discover-bad-meta","method":"server/discover","params":{"_meta":[]}}),
        ),
        (
            "MCP-DRAFT-DISCOVER-PARAMS-003",
            "server/discover carries no body params beyond standard _meta",
            json!({"jsonrpc":"2.0","id":"discover-extra","method":"server/discover","params":{"protocolVersion":"2026-07-28"}}),
        ),
    ];

    for (case_id, description, input) in invalid_cases {
        let actual = response_json(&engine, input);
        assert_invalid_params_kind(&actual, "invalid_params", case_id, description);
    }
}

#[test]
fn mcp_method_params_conformance_matrix() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let cases = [
        (
            "MCP-2025-06-18-TOOLS-LIST-PARAMS-001",
            "tools/list params must be an object when present",
            json!({"jsonrpc":"2.0","id":"tools-list-array","method":"tools/list","params":[]}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-RESOURCES-LIST-PARAMS-001",
            "resources/list params must be an object when present",
            json!({"jsonrpc":"2.0","id":"resources-list-array","method":"resources/list","params":[]}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-RESOURCES-TEMPLATES-LIST-PARAMS-001",
            "resources/templates/list params must be an object when present",
            json!({"jsonrpc":"2.0","id":"resources-templates-list-array","method":"resources/templates/list","params":[]}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-PROMPTS-LIST-PARAMS-001",
            "prompts/list params must be an object when present",
            json!({"jsonrpc":"2.0","id":"prompts-list-array","method":"prompts/list","params":[]}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-RESOURCES-READ-PARAMS-001",
            "resources/read params must be an object with uri",
            json!({"jsonrpc":"2.0","id":"resources-read-array","method":"resources/read","params":[]}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-RESOURCES-READ-PARAMS-002",
            "resources/read params.uri must be a string",
            json!({"jsonrpc":"2.0","id":"resources-read-uri-number","method":"resources/read","params":{"uri":7}}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-TOOLS-CALL-PARAMS-001",
            "tools/call params must be an object with name",
            json!({"jsonrpc":"2.0","id":"tools-call-array","method":"tools/call","params":[]}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-TOOLS-CALL-PARAMS-002",
            "tools/call params.name must be a string",
            json!({"jsonrpc":"2.0","id":"tools-call-name-number","method":"tools/call","params":{"name":7}}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-TOOLS-CALL-ARGS-001",
            "tools/call arguments must be an object when present",
            json!({"jsonrpc":"2.0","id":"tools-call-args-array","method":"tools/call","params":{"name":"shell","arguments":["echo","should-not-run"]}}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-TOOLS-LIST-CURSOR-001",
            "tools/list params.cursor must be a string when present",
            json!({"jsonrpc":"2.0","id":"tools-list-cursor-number","method":"tools/list","params":{"cursor":7}}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-RESOURCES-LIST-CURSOR-001",
            "resources/list params.cursor must be a string when present",
            json!({"jsonrpc":"2.0","id":"resources-list-cursor-number","method":"resources/list","params":{"cursor":7}}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-RESOURCES-TEMPLATES-LIST-CURSOR-001",
            "resources/templates/list params.cursor must be a string when present",
            json!({"jsonrpc":"2.0","id":"resources-templates-list-cursor-number","method":"resources/templates/list","params":{"cursor":7}}),
            "invalid_params",
        ),
        (
            "MCP-2025-06-18-PROMPTS-LIST-CURSOR-001",
            "prompts/list params.cursor must be a string when present",
            json!({"jsonrpc":"2.0","id":"prompts-list-cursor-number","method":"prompts/list","params":{"cursor":7}}),
            "invalid_params",
        ),
    ];

    for (case_id, description, input, expected_kind) in cases {
        let response = handle_jsonrpc(&engine, &input.to_string())
            .unwrap_or_else(|| panic!("{case_id}: expected protocol error response"));
        let actual: Value = serde_json::from_str(&response).unwrap();

        assert_eq!(
            actual["error"]["code"], -32602,
            "{case_id}: {description}: {actual}"
        );
        assert_eq!(
            actual["error"]["data"]["kind"], expected_kind,
            "{case_id}: {description}: {actual}"
        );
        assert!(
            actual["error"]["data"]["reason"]
                .as_str()
                .is_some_and(|reason| !reason.is_empty()),
            "{case_id}: missing reason in {actual}"
        );
    }
}

#[test]
fn mcp_result_shape_conformance_matrix() {
    let dir = tempdir().unwrap();
    let fixture_path = dir.path().join("fixture.txt");
    fs::write(&fixture_path, "tokenzero conformance fixture\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let resources = response_json(
        &engine,
        json!({"jsonrpc":"2.0","id":"resources-shape","method":"resources/list","params":{}}),
    );
    assert_list_result_shape(&resources, "resources", "resources/list");
    let resources_array = resources["result"]["resources"].as_array().unwrap();
    assert!(!resources_array.is_empty(), "{resources}");
    for resource in resources_array {
        assert_non_empty_string(&resource["uri"], "resources/list resource.uri");
        assert_non_empty_string(&resource["name"], "resources/list resource.name");
        assert_non_empty_string(&resource["mimeType"], "resources/list resource.mimeType");
        if let Some(description) = resource.get("description") {
            assert_non_empty_string(description, "resources/list resource.description");
        }
    }

    let resource_templates = response_json(
        &engine,
        json!({"jsonrpc":"2.0","id":"resource-templates-shape","method":"resources/templates/list","params":{}}),
    );
    assert_list_result_shape(
        &resource_templates,
        "resourceTemplates",
        "resources/templates/list",
    );
    assert!(
        resource_templates["result"]["resourceTemplates"]
            .as_array()
            .unwrap()
            .is_empty(),
        "TokenZero exposes concrete resources, not URI templates: {resource_templates}"
    );

    let prompts = response_json(
        &engine,
        json!({"jsonrpc":"2.0","id":"prompts-shape","method":"prompts/list","params":{}}),
    );
    assert_list_result_shape(&prompts, "prompts", "prompts/list");

    let tools = response_json(
        &engine,
        json!({"jsonrpc":"2.0","id":"tools-shape","method":"tools/list","params":{}}),
    );
    assert_list_result_shape(&tools, "tools", "tools/list");
    let tools_array = tools["result"]["tools"].as_array().unwrap();
    assert!(!tools_array.is_empty(), "{tools}");
    for tool in tools_array {
        assert_non_empty_string(&tool["name"], "tools/list tool.name");
        assert_non_empty_string(&tool["description"], "tools/list tool.description");
        assert_eq!(
            tool["inputSchema"]["type"], "object",
            "tools/list tool.inputSchema must be an object schema: {tool}"
        );
    }

    let resource_read = response_json(
        &engine,
        json!({"jsonrpc":"2.0","id":"read-resource-shape","method":"resources/read","params":{"uri":"resource://tokenzero/capabilities"}}),
    );
    assert!(
        resource_read["result"].get("contents").is_some(),
        "{resource_read}"
    );
    let contents = resource_read["result"]["contents"].as_array().unwrap();
    assert!(!contents.is_empty(), "{resource_read}");
    for content in contents {
        assert_non_empty_string(&content["uri"], "resources/read contents[].uri");
        assert_non_empty_string(&content["mimeType"], "resources/read contents[].mimeType");
        assert_non_empty_string(&content["text"], "resources/read contents[].text");
    }

    let tool_call = response_json(
        &engine,
        json!({
            "jsonrpc":"2.0",
            "id":"tool-call-shape",
            "method":"tools/call",
            "params":{"name":"read","arguments":{"path":fixture_path.display().to_string(),"raw":true}}
        }),
    );
    assert!(tool_call.get("error").is_none(), "{tool_call}");
    let result = &tool_call["result"];
    let content = result["content"].as_array().unwrap();
    assert!(!content.is_empty(), "{tool_call}");
    assert_eq!(content[0]["type"], "text", "{tool_call}");
    assert_non_empty_string(&content[0]["text"], "tools/call content[].text");
    assert!(
        result.get("structuredContent").is_none(),
        "tools/call results are text-only unless TOKENZERO_MCP_ENVELOPE is set: {tool_call}"
    );
    assert!(
        result.get("isError").is_none() || result["isError"] == false,
        "successful tools/call must not report isError true: {tool_call}"
    );
}

#[test]
fn mcp_tool_result_conformance_marks_tool_originated_errors() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let input = json!({
        "jsonrpc": "2.0",
        "id": "tool-origin-error",
        "method": "tools/call",
        "params": {
            "name": "read",
            "arguments": {"path": "/__tokenzero_conformance_outside_root__"}
        }
    });

    let response = handle_jsonrpc(&engine, &input.to_string())
        .unwrap_or_else(|| panic!("expected tool result response"));
    let actual: Value = serde_json::from_str(&response).unwrap();

    assert!(
        actual.get("error").is_none(),
        "tool-originated errors must not be protocol errors: {actual}"
    );
    assert_eq!(actual["result"]["isError"], true, "{actual}");
    assert!(
        actual["result"]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("outside allowed roots")),
        "{actual}"
    );
}

#[test]
fn recall_tool_searches_stored_payloads_end_to_end() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "unique_recall_marker line\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "recall-seed",
            "method": "tools/call",
            "params": {"name": "read", "arguments": {"path": file.display().to_string()}}
        }),
    );
    // Alias name + stringly max_hits exercises stub-client coercion.
    let recalled = response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "recall-query",
            "method": "tools/call",
            "params": {"name": "recall", "arguments": {"query": "UNIQUE_RECALL_MARKER", "max_hits": "5"}}
        }),
    );
    let text = recalled["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("unique_recall_marker"), "{text}");
    assert!(text.contains("tz://"), "{text}");
}

#[test]
fn zero_hit_search_renders_note_above_refs_footer() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn alpha() {}\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "zero-hit-grep",
            "method": "tools/call",
            "params": {
                "name": "grep",
                "arguments": {
                    "query": "no_such_token",
                    "path": dir.path().display().to_string()
                }
            }
        }),
    );

    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("# grep no_such_token — 0 matches"),
        "{text}"
    );
    assert!(
        lines
            .next()
            .is_some_and(|line| line.starts_with("refs: tz://blob/")),
        "refs must stay recoverable from the text content: {text}"
    );
}

#[test]
fn tools_call_edit_applies_stub_string_edits_end_to_end() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn alpha() {}\nfn beta() {}\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let listed = response_json(
        &engine,
        json!({"jsonrpc":"2.0","id":"edit-list","method":"tools/list","params":{}}),
    );
    let names = tool_names(&listed);
    assert!(names.iter().any(|name| name == "tz_edit"), "{names:?}");
    assert!(names.iter().any(|name| name == "edit"), "{names:?}");

    let docs = response_json(
        &engine,
        json!({"jsonrpc":"2.0","id":"edit-docs","method":"resources/read","params":{"uri":"resource://tokenzero/tools"}}),
    );
    let docs_payload: Value =
        serde_json::from_str(docs["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert!(
        docs_payload["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "tz_edit"),
        "tz_edit must appear in the resource catalog"
    );

    // Stub-schema clients send the edits array as a JSON-encoded string and
    // booleans as strings; the server coerces both.
    let edits = serde_json::to_string(&json!([
        {"find": "fn alpha() {}", "replace": "fn alpha() -> u8 { 1 }", "replace_all": "false"}
    ]))
    .unwrap();
    let response = response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "edit-call",
            "method": "tools/call",
            "params": {
                "name": "edit",
                "arguments": {
                    "path": file.display().to_string(),
                    "edits": edits,
                    "dry_run": "false"
                }
            }
        }),
    );
    assert!(response.get("error").is_none(), "{response}");
    let result = &response["result"];
    assert!(
        result.get("isError").is_none() || result["isError"] == false,
        "{response}"
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.lines().next().is_some_and(
            |line| line.starts_with(&format!("# edit {} — 1 hunks applied", file.display()))
        ),
        "{text}"
    );
    // Post-image blob/file refs are listed verbatim; the undo ref must also
    // be on the wire verbatim (labeled) — the default envelope is text-only,
    // so the footer is the ONLY way a client can learn the undo ref id.
    let footer = text
        .lines()
        .find(|line| line.starts_with("refs: "))
        .unwrap_or_else(|| panic!("missing refs footer: {text}"));
    assert!(footer.contains("tz://blob/"), "{footer}");
    let undo_ref = footer
        .split_whitespace()
        .find_map(|part| part.strip_prefix("undo:"))
        .unwrap_or_else(|| panic!("missing labeled undo ref: {footer}"));
    assert!(undo_ref.starts_with("tz://blob/"), "{footer}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "fn alpha() -> u8 { 1 }\nfn beta() {}\n"
    );
    // The labeled undo ref recovers the exact pre-image through the wire.
    let expanded = response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "edit-undo-expand",
            "method": "tools/call",
            "params": {"name": "expand", "arguments": {"ref": undo_ref}}
        }),
    );
    let expanded_text = expanded["result"]["content"][0]["text"].as_str().unwrap();
    assert!(expanded_text.contains("fn alpha() {}"), "{expanded_text}");
}

#[test]
fn tools_list_supports_agent_friendly_cluster_filtering() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let default = response_json(
        &engine,
        json!({"jsonrpc":"2.0","id":"tools-default","method":"tools/list","params":{}}),
    );
    let default_names = tool_names(&default);
    assert!(
        default_names.len() > 7,
        "default remains full compatibility catalog: {default}"
    );
    assert!(default_names.iter().any(|name| name == "tz_read"));
    assert!(default_names.iter().any(|name| name == "read"));

    let material = response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "tools-material",
            "method": "tools/list",
            "params": {"_meta": {"tokenzero/toolCluster": "material"}}
        }),
    );
    let material_names = tool_names(&material);
    assert!(
        material_names.len() <= 7,
        "material cluster must stay small enough for agent menus: {material}"
    );
    assert!(material_names.iter().any(|name| name == "tz_read"));
    assert!(material_names.iter().any(|name| name == "tz_expand"));
    assert!(!material_names.iter().any(|name| name == "tz_shell"));
    assert!(!material_names.iter().any(|name| name == "read"));
    assert_eq!(
        material["result"]["_meta"]["tokenzero/toolFilter"]["cluster"],
        "material"
    );
    assert_eq!(
        material["result"]["_meta"]["tokenzero/toolFilter"]["includeAliases"],
        false
    );

    let execution = response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "tools-execution",
            "method": "tools/list",
            "params": {"profile": "execution"}
        }),
    );
    let execution_names = tool_names(&execution);
    assert!(
        execution_names.len() <= 7,
        "execution cluster must stay small enough for agent menus: {execution}"
    );
    assert!(execution_names.iter().any(|name| name == "tz_shell"));
    assert!(!execution_names.iter().any(|name| name == "tz_read"));

    let aliased = response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "tools-material-aliases",
            "method": "tools/list",
            "params": {
                "_meta": {
                    "tokenzero/toolCluster": "material",
                    "tokenzero/includeAliases": true
                }
            }
        }),
    );
    let aliased_names = tool_names(&aliased);
    assert!(aliased_names.iter().any(|name| name == "tz_read"));
    assert!(aliased_names.iter().any(|name| name == "read"));
    assert_eq!(
        aliased["result"]["_meta"]["tokenzero/toolFilter"]["includeAliases"],
        true
    );

    let invalid = response_json(
        &engine,
        json!({
            "jsonrpc": "2.0",
            "id": "tools-bad-cluster",
            "method": "tools/list",
            "params": {"_meta": {"tokenzero/toolCluster": "matrial"}}
        }),
    );
    assert_eq!(invalid["error"]["code"], -32602, "{invalid}");
    assert_eq!(
        invalid["error"]["data"]["kind"], "unknown_tool_cluster",
        "{invalid}"
    );
    assert_eq!(
        invalid["error"]["data"]["error_type"], "INVALID_ARGUMENT",
        "{invalid}"
    );
    assert!(
        invalid["error"]["data"]["available_options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "material"),
        "{invalid}"
    );
    assert_eq!(
        invalid["error"]["data"]["suggestions"][0]["value"], "material",
        "{invalid}"
    );
}

fn initialize_params(protocol_version: &str) -> Value {
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {},
        "clientInfo": {
            "name": "tokenzero-conformance-client",
            "version": "1.0.0"
        }
    })
}

fn response_json(engine: &TokenZeroEngine, input: Value) -> Value {
    let response =
        handle_jsonrpc(engine, &input.to_string()).unwrap_or_else(|| panic!("expected response"));
    serde_json::from_str(&response).unwrap()
}

fn tool_names(response: &Value) -> Vec<String> {
    response["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect()
}

fn assert_list_result_shape(response: &Value, key: &str, label: &str) {
    assert!(response.get("error").is_none(), "{label}: {response}");
    assert!(
        response["result"][key].is_array(),
        "{label}: result.{key} must be array: {response}"
    );
    if let Some(cursor) = response["result"].get("nextCursor") {
        assert!(
            cursor.is_string(),
            "{label}: result.nextCursor must be string when present: {response}"
        );
    }
}

fn assert_non_empty_string(value: &Value, label: &str) {
    assert!(
        value.as_str().is_some_and(|text| !text.is_empty()),
        "{label} must be a non-empty string, got {value}"
    );
}

fn assert_invalid_params_kind(
    actual: &Value,
    expected_kind: &str,
    case_id: &str,
    description: &str,
) {
    assert_eq!(
        actual["error"]["code"], -32602,
        "{case_id}: {description}: {actual}"
    );
    assert_eq!(
        actual["error"]["data"]["kind"], expected_kind,
        "{case_id}: {description}: {actual}"
    );
    assert_protocol_error_data(&actual["error"]["data"])
        .unwrap_or_else(|reason| panic!("{case_id}: {description}: {reason}"));
}

fn run_case(engine: &TokenZeroEngine, case: &ConformanceCase) -> Result<(), String> {
    let payload = match &case.input {
        CaseInput::Json(value) => value.to_string(),
        CaseInput::Raw(text) => text.to_string(),
    };
    let response = handle_jsonrpc(engine, &payload)
        .map(|text| serde_json::from_str::<Value>(&text).map_err(|err| err.to_string()))
        .transpose()?;

    match (&case.expected, response) {
        (Expected::NoResponse, None) => Ok(()),
        (Expected::NoResponse, Some(actual)) => Err(format!("expected no response, got {actual}")),
        (Expected::Result { id }, Some(actual)) => assert_result_response(&actual, id),
        (Expected::Result { .. }, None) => Err("expected result response, got no response".into()),
        (Expected::Error { id, code }, Some(actual)) => assert_error_response(&actual, id, *code),
        (Expected::Error { .. }, None) => Err("expected error response, got no response".into()),
        (Expected::Batch(expected), Some(Value::Array(actual))) => {
            if actual.len() != expected.len() {
                return Err(format!(
                    "expected {} batch responses, got {}: {actual:?}",
                    expected.len(),
                    actual.len()
                ));
            }
            for (index, (expected, actual)) in expected.iter().zip(actual.iter()).enumerate() {
                match expected {
                    BatchExpected::Result { id } => assert_result_response(actual, id)
                        .map_err(|reason| format!("batch[{index}]: {reason}"))?,
                    BatchExpected::Error { id, code } => {
                        assert_error_response(actual, id, *code)
                            .map_err(|reason| format!("batch[{index}]: {reason}"))?
                    }
                }
            }
            Ok(())
        }
        (Expected::Batch(_), Some(actual)) => Err(format!("expected batch array, got {actual}")),
        (Expected::Batch(_), None) => Err("expected batch response, got no response".into()),
    }
}

fn assert_result_response(actual: &Value, expected_id: &Value) -> Result<(), String> {
    if actual["jsonrpc"] != "2.0" {
        return Err(format!("missing jsonrpc 2.0 in {actual}"));
    }
    if &actual["id"] != expected_id {
        return Err(format!("expected id {expected_id}, got {}", actual["id"]));
    }
    if !actual.get("result").is_some_and(Value::is_object) {
        return Err(format!("expected object result, got {actual}"));
    }
    if actual.get("error").is_some() {
        return Err(format!("result response included error: {actual}"));
    }
    Ok(())
}

fn assert_error_response(
    actual: &Value,
    expected_id: &Value,
    expected_code: i64,
) -> Result<(), String> {
    if actual["jsonrpc"] != "2.0" {
        return Err(format!("missing jsonrpc 2.0 in {actual}"));
    }
    if &actual["id"] != expected_id {
        return Err(format!("expected id {expected_id}, got {}", actual["id"]));
    }
    if actual["error"]["code"] != expected_code {
        return Err(format!(
            "expected error code {expected_code}, got {} in {actual}",
            actual["error"]["code"]
        ));
    }
    if actual.get("result").is_some() {
        return Err(format!("error response included result: {actual}"));
    }
    assert_protocol_error_data(&actual["error"]["data"])?;
    Ok(())
}

fn assert_protocol_error_data(data: &Value) -> Result<(), String> {
    if !data.is_object() {
        return Err(format!("expected object error.data, got {data}"));
    }
    if data["kind"].as_str().is_none_or(str::is_empty) {
        return Err(format!("missing data.kind in {data}"));
    }
    if data["reason"].as_str().is_none_or(str::is_empty) {
        return Err(format!("missing data.reason in {data}"));
    }
    if data["fix_hint"].as_str().is_none_or(str::is_empty) {
        return Err(format!("missing data.fix_hint in {data}"));
    }
    if !data["recoverable"].is_boolean() {
        return Err(format!("missing data.recoverable in {data}"));
    }
    Ok(())
}

fn render_report(cases: &[ConformanceCase], failures: &[String]) -> String {
    let mut sections = std::collections::BTreeMap::<&str, (usize, usize, usize)>::new();
    for case in cases {
        let entry = sections.entry(case.section).or_default();
        match case.level {
            RequirementLevel::Must => entry.0 += 1,
            RequirementLevel::Should => entry.1 += 1,
        }
        entry.2 += 1;
    }

    let mut report = String::from("# MCP JSON-RPC Conformance Report\n\n");
    report.push_str("| Section | MUST | SHOULD | Tested |\n");
    report.push_str("|---------|------|--------|--------|\n");
    for (section, (must, should, tested)) in sections {
        report.push_str(&format!("| {section} | {must} | {should} | {tested} |\n"));
    }
    report.push_str("\nFailures:\n");
    if failures.is_empty() {
        report.push_str("- none\n");
    } else {
        for failure in failures {
            report.push_str(&format!("- {failure}\n"));
        }
    }
    report
}
