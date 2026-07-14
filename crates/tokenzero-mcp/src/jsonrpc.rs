use serde_json::{Value, json};
use tokenzero_core::{MCP_SCHEMA_VERSION, McpToolSurface};

use crate::catalog::{tool_cluster_names, tool_specs_for_filter_with_health};
use crate::{TokenZeroEngine, call_tool, read_resource, resource_specs, tool_specs};

const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
pub(crate) const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[
    "2024-11-05",
    "2025-03-26",
    "2025-06-18",
    "DRAFT-2026-v1",
    "2026-07-28",
];
const LOGGING_LEVELS: &[&str] = &[
    "debug",
    "info",
    "notice",
    "warning",
    "error",
    "critical",
    "alert",
    "emergency",
];

macro_rules! rpc_methods {
    ($( $variant:ident => $name:literal ),* $(,)?) => {
        #[derive(Clone, Copy)]
        enum RpcMethod { $( $variant ),* }

        impl RpcMethod {
            const NAMES: &'static [&'static str] = &[$( $name ),*];

            fn parse(name: &str) -> Option<Self> {
                match name { $( $name => Some(Self::$variant), )* _ => None }
            }
        }
    };
}

rpc_methods! {
    Initialize => "initialize",
    Initialized => "notifications/initialized",
    Ping => "ping",
    Discover => "server/discover",
    ListResources => "resources/list",
    ListResourceTemplates => "resources/templates/list",
    ReadResource => "resources/read",
    ListPrompts => "prompts/list",
    SetLoggingLevel => "logging/setLevel",
    ListTools => "tools/list",
    CallTool => "tools/call",
}

const JSONRPC_METHODS: &[&str] = RpcMethod::NAMES;

pub fn handle_jsonrpc(engine: &TokenZeroEngine, line: &str) -> Option<String> {
    let parsed: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(err) => {
            return Some(
                jsonrpc_error(
                    Value::Null,
                    -32700,
                    "Parse error",
                    JsonRpcErrorData::parse_error(err.to_string()),
                )
                .to_string(),
            );
        }
    };
    handle_jsonrpc_value(engine, parsed).map(|value| value.to_string())
}

pub(crate) fn handle_jsonrpc_value(engine: &TokenZeroEngine, parsed: Value) -> Option<Value> {
    match parsed {
        Value::Array(batch) => handle_jsonrpc_batch(engine, batch),
        value => handle_jsonrpc_request(engine, value),
    }
}

fn handle_jsonrpc_batch(engine: &TokenZeroEngine, batch: Vec<Value>) -> Option<Value> {
    if batch.is_empty() {
        return Some(jsonrpc_error(
            Value::Null,
            -32600,
            "Invalid Request",
            JsonRpcErrorData::invalid_request(
                "empty batch",
                "Send a non-empty JSON-RPC batch or send one request object.",
            ),
        ));
    }

    let responses = batch
        .into_iter()
        .filter_map(|item| handle_jsonrpc_request(engine, item))
        .collect::<Vec<_>>();
    if responses.is_empty() {
        return None;
    }
    Some(Value::Array(responses))
}

macro_rules! rpc_try {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => return Some(error),
        }
    };
}

fn request_error(id: Value, reason: &str, fix_hint: &str) -> Value {
    jsonrpc_error(
        id,
        -32600,
        "Invalid Request",
        JsonRpcErrorData::invalid_request(reason, fix_hint),
    )
}

fn validate_request(
    parsed: &Value,
) -> Result<(&serde_json::Map<String, Value>, &str, Option<Value>), Value> {
    let object = parsed.as_object().ok_or_else(|| {
        request_error(
            Value::Null,
            "request must be a JSON object",
            "Send an object with jsonrpc, method, params, and id fields.",
        )
    })?;
    let valid_id = object.get("id").filter(|id| is_valid_jsonrpc_id(id));
    let id_for_error = valid_id.cloned().unwrap_or(Value::Null);
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(request_error(
            id_for_error,
            "jsonrpc must be \"2.0\"",
            "Set the top-level jsonrpc field to \"2.0\" and retry.",
        ));
    }
    let method = object.get("method").and_then(Value::as_str).ok_or_else(|| {
        request_error(
            id_for_error.clone(),
            "method must be a string",
            "Set method to initialize, tools/list, tools/call, resources/list, or resources/read.",
        )
    })?;
    if object.contains_key("id") && valid_id.is_none() {
        return Err(request_error(
            Value::Null,
            "id must be string, number, or null",
            "Use a string, number, or null id so the response can be correlated.",
        ));
    }
    if object
        .get("params")
        .is_some_and(|params| !(params.is_object() || params.is_array()))
    {
        return Err(request_error(
            id_for_error,
            "params must be object or array",
            "Use object params for MCP methods; omit params when no parameters are needed.",
        ));
    }
    Ok((object, method, object.get("id").cloned()))
}

fn handle_jsonrpc_request(engine: &TokenZeroEngine, parsed: Value) -> Option<Value> {
    let (object, method, id) = rpc_try!(validate_request(&parsed));
    let id = id?;
    #[cfg(test)]
    if method == "tokenzero/internal/test-panic" {
        panic!("test-induced tool panic");
    }
    let method = match RpcMethod::parse(method) {
        Some(method) => method,
        None => {
            return Some(jsonrpc_error(
                id,
                -32601,
                "Method not found",
                JsonRpcErrorData::unknown_method(method),
            ));
        }
    };
    let result = match method {
        RpcMethod::Initialize => {
            let requested = rpc_try!(initialize_protocol_version(object, &id));
            let requested_is_supported = supported_protocol_version(requested);
            let protocol_version = if requested_is_supported {
                requested
            } else {
                DEFAULT_PROTOCOL_VERSION
            };
            json!({
                "protocolVersion": protocol_version,
                "capabilities": {
                    "logging": {},
                    "tools": {"listChanged": false},
                    "resources": {"listChanged": false},
                    "prompts": {"listChanged": false}
                },
                "serverInfo": {"name": "tokenzero", "version": env!("CARGO_PKG_VERSION")},
                "instructions": mcp_initialize_instructions(engine.config.tool_surface),
                "_meta": {
                    "tokenzero/protocolNegotiation": {
                        "requestedProtocolVersion": requested,
                        "negotiatedProtocolVersion": protocol_version,
                        "supportedProtocolVersions": SUPPORTED_PROTOCOL_VERSIONS,
                        "fallback": !requested_is_supported
                    }
                }
            })
        }
        RpcMethod::Ping | RpcMethod::Initialized => json!({}),
        RpcMethod::Discover => {
            let params = rpc_try!(meta_only_params(object, &id, "server/discover"));
            json!({
                "resultType": "complete",
                "supportedVersions": SUPPORTED_PROTOCOL_VERSIONS,
                "capabilities": {
                    "logging": {},
                    "tools": {"listChanged": false},
                    "resources": {"listChanged": false},
                    "prompts": {"listChanged": false}
                },
                "serverInfo": {"name": "tokenzero", "version": env!("CARGO_PKG_VERSION")},
                "instructions": mcp_discover_instructions(engine.config.tool_surface),
                "ttlMs": 60000,
                "cacheScope": "workspace",
                "_meta": {
                    "schema_version": MCP_SCHEMA_VERSION,
                    "status": "ok",
                    "protocolVersions": SUPPORTED_PROTOCOL_VERSIONS,
                    "toolFiltering": tool_filter_discovery(engine.config.tool_surface),
                    "clientMetaAccepted": params.and_then(|params| params.get("_meta")).is_some()
                },
                "protocolVersions": SUPPORTED_PROTOCOL_VERSIONS,
                "toolFiltering": tool_filter_discovery(engine.config.tool_surface)
            })
        }
        RpcMethod::ListResources => {
            let params = rpc_try!(object_params(object, &id, "resources/list"));
            rpc_try!(ensure_optional_cursor_param(params, &id, "resources/list"));
            json!({
                "resources": resource_specs(),
                "resultType": "complete",
                "ttlMs": 60000,
                "cacheScope": "workspace"
            })
        }
        RpcMethod::ListResourceTemplates => {
            let params = rpc_try!(object_params(object, &id, "resources/templates/list"));
            rpc_try!(ensure_optional_cursor_param(
                params,
                &id,
                "resources/templates/list",
            ));
            json!({
                "resourceTemplates": [],
                "resultType": "complete",
                "ttlMs": 60000,
                "cacheScope": "workspace"
            })
        }
        RpcMethod::ReadResource => {
            let params = rpc_try!(required_params(object, &id, "resources/read", "uri"));
            let uri = rpc_try!(required_string_param(params, &id, "resources/read", "uri"));
            rpc_try!(read_resource(engine, uri)
                .map_err(|error| jsonrpc_invalid_params_error(id.clone(), error)))
        }
        RpcMethod::ListPrompts => {
            let params = rpc_try!(object_params(object, &id, "prompts/list"));
            rpc_try!(ensure_optional_cursor_param(params, &id, "prompts/list"));
            json!({"prompts": []})
        }
        RpcMethod::SetLoggingLevel => {
            let params = rpc_try!(required_object_params(object, &id, "logging/setLevel"));
            rpc_try!(logging_level(params, &id));
            json!({})
        }
        RpcMethod::ListTools => {
            let params = rpc_try!(object_params(object, &id, "tools/list"));
            rpc_try!(ensure_optional_cursor_param(params, &id, "tools/list"));
            let filter = match tool_list_filter(params) {
                Ok(filter) => filter,
                Err(error) => return Some(jsonrpc_invalid_params_error(id, error)),
            };
            let recovery_unlocked = engine.surface_health().recovery_unlocked();
            let tools = tool_specs_for_filter_with_health(
                filter.cluster.as_deref(),
                filter.include_aliases,
                engine.config.tool_surface,
                recovery_unlocked,
            );
            let tool_count = tools.len();
            json!({
                "tools": tools,
                "resultType": "complete",
                "ttlMs": 60000,
                "cacheScope": "workspace",
                "_meta": {
                    "tokenzero/toolFilter": filter.to_meta(tool_count)
                }
            })
        }
        RpcMethod::CallTool => {
            let params = rpc_try!(required_params(object, &id, "tools/call", "name"));
            let name = rpc_try!(required_string_param(params, &id, "tools/call", "name"));
            let args = rpc_try!(tool_arguments(params, &id));
            let call_id = match &id {
                Value::Null => None,
                other => Some(other.to_string()),
            };
            rpc_try!(call_tool(engine, name, &args, call_id)
                .map_err(|error| jsonrpc_invalid_params_error(id.clone(), error)))
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn object_params<'a>(
    object: &'a serde_json::Map<String, Value>,
    id: &Value,
    method: &str,
) -> Result<Option<&'a serde_json::Map<String, Value>>, Value> {
    match object.get("params") {
        None => Ok(None),
        Some(Value::Object(params)) => Ok(Some(params)),
        Some(_) => Err(jsonrpc_invalid_params_error(
            id.clone(),
            JsonRpcErrorData::invalid_params(format!("{method} params must be object")),
        )),
    }
}

fn meta_only_params<'a>(
    object: &'a serde_json::Map<String, Value>,
    id: &Value,
    method: &str,
) -> Result<Option<&'a serde_json::Map<String, Value>>, Value> {
    let Some(params) = object_params(object, id, method)? else {
        return Ok(None);
    };
    for key in params.keys() {
        if key != "_meta" {
            return Err(jsonrpc_invalid_params_error(
                id.clone(),
                JsonRpcErrorData::invalid_params(format!(
                    "{method} params must not contain body parameter {key}; use _meta only"
                )),
            ));
        }
    }
    match params.get("_meta") {
        None | Some(Value::Object(_)) => Ok(Some(params)),
        Some(_) => Err(jsonrpc_invalid_params_error(
            id.clone(),
            JsonRpcErrorData::invalid_params(format!("{method} params._meta must be object")),
        )),
    }
}

fn ensure_optional_cursor_param(
    params: Option<&serde_json::Map<String, Value>>,
    id: &Value,
    method: &str,
) -> Result<(), Value> {
    let Some(params) = params else {
        return Ok(());
    };
    match params.get("cursor") {
        None | Some(Value::String(_)) => Ok(()),
        Some(_) => Err(jsonrpc_invalid_params_error(
            id.clone(),
            JsonRpcErrorData::invalid_params(format!("{method} params.cursor must be string")),
        )),
    }
}

fn required_params<'a>(
    object: &'a serde_json::Map<String, Value>,
    id: &Value,
    method: &str,
    missing: &str,
) -> Result<&'a serde_json::Map<String, Value>, Value> {
    object_params(object, id, method)?.ok_or_else(|| {
        jsonrpc_invalid_params_error(
            id.clone(),
            JsonRpcErrorData::missing_param(method, missing),
        )
    })
}

fn required_object_params<'a>(
    object: &'a serde_json::Map<String, Value>,
    id: &Value,
    method: &str,
) -> Result<&'a serde_json::Map<String, Value>, Value> {
    required_params(object, id, method, "params")
}

fn initialize_protocol_version<'a>(
    object: &'a serde_json::Map<String, Value>,
    id: &Value,
) -> Result<&'a str, Value> {
    let method = "initialize";
    let params = required_object_params(object, id, method)?;
    let protocol_version = required_string_param(params, id, method, "protocolVersion")?;
    required_object_param(params, id, method, "capabilities")?;
    let client_info = required_object_param(params, id, method, "clientInfo")?;
    required_string_field(client_info, id, method, "name", "clientInfo.name")?;
    required_string_field(client_info, id, method, "version", "clientInfo.version")?;
    if client_info
        .get("title")
        .is_some_and(|title| !title.is_string())
    {
        return Err(jsonrpc_invalid_params_error(
            id.clone(),
            JsonRpcErrorData::invalid_params("initialize clientInfo.title must be string"),
        ));
    }
    Ok(protocol_version)
}

fn logging_level<'a>(
    params: &'a serde_json::Map<String, Value>,
    id: &Value,
) -> Result<&'a str, Value> {
    let level = required_string_param(params, id, "logging/setLevel", "level")?;
    if is_valid_logging_level(level) {
        Ok(level)
    } else {
        Err(jsonrpc_invalid_params_error(
            id.clone(),
            JsonRpcErrorData::invalid_logging_level(level),
        ))
    }
}

fn required_object_param<'a>(
    params: &'a serde_json::Map<String, Value>,
    id: &Value,
    method: &str,
    param: &str,
) -> Result<&'a serde_json::Map<String, Value>, Value> {
    match params.get(param) {
        Some(Value::Object(value)) => Ok(value),
        Some(_) => Err(jsonrpc_invalid_params_error(
            id.clone(),
            JsonRpcErrorData::invalid_params(format!("{method} params.{param} must be object")),
        )),
        None => Err(jsonrpc_invalid_params_error(
            id.clone(),
            JsonRpcErrorData::missing_param(method, param),
        )),
    }
}

fn required_string_param<'a>(
    params: &'a serde_json::Map<String, Value>,
    id: &Value,
    method: &str,
    param: &str,
) -> Result<&'a str, Value> {
    required_string_field(params, id, method, param, param)
}

fn required_string_field<'a>(
    params: &'a serde_json::Map<String, Value>,
    id: &Value,
    method: &str,
    key: &str,
    error_param: &str,
) -> Result<&'a str, Value> {
    match params.get(key) {
        Some(Value::String(value)) => Ok(value),
        Some(_) => Err(jsonrpc_invalid_params_error(
            id.clone(),
            JsonRpcErrorData::invalid_params(format!(
                "{method} params.{error_param} must be string"
            )),
        )),
        None => Err(jsonrpc_invalid_params_error(
            id.clone(),
            JsonRpcErrorData::missing_param(method, error_param),
        )),
    }
}

fn tool_arguments(params: &serde_json::Map<String, Value>, id: &Value) -> Result<Value, Value> {
    match params.get("arguments") {
        None => Ok(json!({})),
        Some(Value::Object(_)) => Ok(params["arguments"].clone()),
        Some(_) => Err(jsonrpc_invalid_params_error(
            id.clone(),
            JsonRpcErrorData::invalid_params("tools/call arguments must be object"),
        )),
    }
}

fn is_valid_jsonrpc_id(value: &Value) -> bool {
    value.is_string() || value.is_number() || value.is_null()
}

fn supported_protocol_version(value: &str) -> bool {
    SUPPORTED_PROTOCOL_VERSIONS.contains(&value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolListFilter {
    cluster: Option<String>,
    include_aliases: bool,
}

impl Default for ToolListFilter {
    fn default() -> Self {
        Self {
            cluster: None,
            include_aliases: true,
        }
    }
}

impl ToolListFilter {
    fn to_meta(&self, tools_returned: usize) -> Value {
        // Full filter guidance lives in server/discover and the capabilities resource.
        json!({
            "profile": self.cluster.as_deref().unwrap_or("full"),
            "cluster": self.cluster.as_deref().unwrap_or("all"),
            "includeAliases": self.include_aliases,
            "availableClusters": tool_cluster_names(),
            "toolsReturned": tools_returned
        })
    }
}

pub(crate) fn tool_filter_discovery(surface: McpToolSurface) -> Value {
    match surface {
        McpToolSurface::Classic => json!({
            "surface": "classic",
            "default": {
                "profile": "full",
                "cluster": "all",
                "includeAliases": true
            },
            "recommended": [
                {
                    "profile": "material",
                    "params": {"_meta": {"tokenzero/toolCluster": "material"}},
                    "description": "Read, search, tree, glob, and exact-ref recovery tools."
                },
                {
                    "profile": "execution",
                    "params": {"_meta": {"tokenzero/toolCluster": "execution"}},
                    "description": "Shell, ingest, cache, rewrite, discovery, and memory tools."
                }
            ],
            "acceptedParams": {
                "_meta.tokenzero/toolCluster": tool_cluster_names(),
                "_meta.tokenzero/includeAliases": "boolean, defaults false when a cluster is selected",
                "cluster": "top-level compatibility alias for tokenzero/toolCluster",
                "profile": "top-level compatibility alias; accepted values are full, all, material, execution"
            }
        }),
        McpToolSurface::CodeMode => json!({
            "surface": "codemode",
            "default": {"profile": "codemode", "cluster": "codemode", "includeAliases": false},
            "recommended": [{
                "profile": "codemode",
                "params": {},
                "description": "Exactly tz_execute_code, tz_codemode_search, and tz_codemode_describe."
            }],
            "acceptedParams": {}
        }),
    }
}

fn mcp_initialize_instructions(surface: McpToolSurface) -> &'static str {
    match surface {
        McpToolSurface::Classic => {
            "TokenZero compacts tool output and stores exact bytes behind tz:// refs; recover them with tz_expand. Short tool names (read, find, grep, glob, tree, shell, ingest, expand, mem, cache_pack, rewrite, discover) are aliases of the tz_* tools. Full per-tool docs: resources/read resource://tokenzero/tools."
        }
        McpToolSurface::CodeMode => {
            "TokenZero CodeMode exposes exactly tz_execute_code, tz_codemode_search, and tz_codemode_describe. Per-op MCP tools are unavailable in this mode."
        }
    }
}

fn mcp_discover_instructions(surface: McpToolSurface) -> &'static str {
    match surface {
        McpToolSurface::Classic => {
            "Use tools/list for JSON Schema input contracts, resources/list for discovery resources, and tool text output (refs: footers, shell command_success) after tools/call."
        }
        McpToolSurface::CodeMode => {
            "Use tz_codemode_describe name=capabilities, tz_codemode_search for methods, then tz_execute_code for recipe/json/js plans."
        }
    }
}

fn tool_list_filter(
    params: Option<&serde_json::Map<String, Value>>,
) -> Result<ToolListFilter, JsonRpcErrorData> {
    let Some(params) = params else {
        return Ok(ToolListFilter::default());
    };
    let meta = match params.get("_meta") {
        Some(Value::Object(meta)) => Some(meta),
        Some(Value::Null) | None => None,
        Some(_) => {
            return Err(JsonRpcErrorData::invalid_params(
                "tools/list params._meta must be object when present",
            ));
        }
    };
    let requested_cluster = optional_string_param(
        params,
        meta,
        &[
            "tokenzero/toolCluster",
            "tokenzero/profile",
            "toolCluster",
            "tool_cluster",
            "cluster",
            "profile",
        ],
        "cluster",
    )?;
    let requested_aliases = optional_bool_param(
        params,
        meta,
        &[
            "tokenzero/includeAliases",
            "includeAliases",
            "include_aliases",
            "aliases",
        ],
        "includeAliases",
    )?;
    let cluster = match requested_cluster {
        Some(cluster) => normalize_tool_cluster(&cluster)?,
        None => None,
    };
    let include_aliases = requested_aliases.unwrap_or(cluster.is_none());
    Ok(ToolListFilter {
        cluster,
        include_aliases,
    })
}

fn normalize_tool_cluster(raw: &str) -> Result<Option<String>, JsonRpcErrorData> {
    let normalized = raw.trim().to_ascii_lowercase().replace(['_', ' '], "-");
    match normalized.as_str() {
        "" | "all" | "full" => Ok(None),
        "material" | "materials" | "context" | "read" | "reader" | "search" => {
            Ok(Some("material".to_string()))
        }
        "execution" | "exec" | "runtime" | "run" | "shell" => Ok(Some("execution".to_string())),
        other => Err(JsonRpcErrorData::unknown_tool_cluster(other)),
    }
}

fn optional_string_param(
    params: &serde_json::Map<String, Value>,
    meta: Option<&serde_json::Map<String, Value>>,
    keys: &[&str],
    label: &str,
) -> Result<Option<String>, JsonRpcErrorData> {
    for key in keys {
        let value = params
            .get(*key)
            .or_else(|| meta.and_then(|meta| meta.get(*key)));
        match value {
            Some(Value::String(value)) => return Ok(Some(value.clone())),
            Some(Value::Null) | None => {}
            Some(_) => {
                return Err(JsonRpcErrorData::invalid_params(format!(
                    "tools/list params.{label} must be string"
                )));
            }
        }
    }
    Ok(None)
}

fn optional_bool_param(
    params: &serde_json::Map<String, Value>,
    meta: Option<&serde_json::Map<String, Value>>,
    keys: &[&str],
    label: &str,
) -> Result<Option<bool>, JsonRpcErrorData> {
    for key in keys {
        let value = params
            .get(*key)
            .or_else(|| meta.and_then(|meta| meta.get(*key)));
        match value {
            Some(Value::Bool(value)) => return Ok(Some(*value)),
            Some(Value::String(value)) if value.eq_ignore_ascii_case("true") => {
                return Ok(Some(true));
            }
            Some(Value::String(value)) if value.eq_ignore_ascii_case("false") => {
                return Ok(Some(false));
            }
            Some(Value::Null) | None => {}
            Some(_) => {
                return Err(JsonRpcErrorData::invalid_params(format!(
                    "tools/list params.{label} must be boolean"
                )));
            }
        }
    }
    Ok(None)
}

fn is_valid_logging_level(value: &str) -> bool {
    LOGGING_LEVELS.contains(&value)
}

#[derive(Debug, Clone)]
pub(crate) struct JsonRpcErrorData {
    value: Value,
}

impl JsonRpcErrorData {
    fn recoverable(
        kind: &'static str,
        error_type: &'static str,
        message: String,
        fix_hint: String,
        available_options: Value,
        suggestions: Value,
        suggested_tool_calls: Value,
        extra: Value,
    ) -> Self {
        let mut value = json!({
            "kind": kind,
            "error_type": error_type,
            "message": message,
            "recoverable": true,
            "reason": message,
            "fix_hint": fix_hint,
            "available_options": available_options,
            "suggestions": suggestions,
            "suggested_tool_calls": suggested_tool_calls,
        });
        value
            .as_object_mut()
            .expect("error data object")
            .extend(extra.as_object().expect("error extras").clone());
        Self { value }
    }

    /// Human-readable message for inline rendering (batch sub-op errors).
    pub(crate) fn message_text(&self) -> String {
        self.value
            .get("message")
            .or_else(|| self.value.get("reason"))
            .and_then(Value::as_str)
            .unwrap_or("invalid arguments")
            .to_string()
    }

    pub(crate) fn internal_error(reason: impl Into<String>) -> Self {
        Self::recoverable(
            "internal_error",
            "INTERNAL",
            reason.into(),
            "Retry the request; the server isolated an internal fault and remains available."
                .into(),
            json!([]),
            json!([]),
            json!([]),
            json!({}),
        )
    }

    fn invalid_params(reason: impl Into<String>) -> Self {
        Self::recoverable(
            "invalid_params",
            "INVALID_ARGUMENT",
            reason.into(),
            "Check the method input schema from tools/list or resources/list, then retry with object params.".into(),
            json!(["tools/list", "resources/list"]),
            json!([]),
            json!([
                {"method": "tools/list", "params": {}},
                {"method": "resources/list", "params": {}}
            ]),
            json!({}),
        )
    }

    fn missing_param(method: &str, param: &str) -> Self {
        let (available_options, fix_hint, suggested_tool_calls) = match (method, param) {
            ("tools/call", "name") => (
                tool_names(),
                "Set params.name to a listed tool name; call tools/list for schemas first.",
                json!([{"method": "tools/list", "params": {}}]),
            ),
            ("resources/read", "uri") => (
                resource_uris(),
                "Set params.uri to a listed resource URI; call resources/list first.",
                json!([{"method": "resources/list", "params": {}}]),
            ),
            ("logging/setLevel", "level") => (
                logging_levels(),
                "Set params.level to a supported logging level.",
                json!([{"method": "logging/setLevel", "params": {"level": "info"}}]),
            ),
            _ => (
                Vec::new(),
                "Add the missing parameter and retry with object params.",
                json!([]),
            ),
        };
        Self::recoverable(
            "missing_param",
            "INVALID_ARGUMENT",
            format!("missing {param}"),
            fix_hint.into(),
            json!(available_options),
            json!([]),
            suggested_tool_calls,
            json!({"method": method, "param": param, "parameter": param}),
        )
    }

    pub(crate) fn unknown_tool(name: &str) -> Self {
        let available_options = tool_names();
        Self::recoverable(
            "unknown_tool",
            "NOT_FOUND",
            format!("unknown tool: {name}"),
            "Call tools/list, then retry tools/call with one of available_options as params.name."
                .into(),
            json!(available_options),
            json!(similar_options(name, &tool_names())),
            json!([{"method": "tools/list", "params": {}}]),
            json!({
                "entity_type": "tool",
                "provided": name,
                "tool": name,
                "available_tools": tool_names(),
            }),
        )
    }

    /// Crash-only / surface-health policy refusal (wqw.9). Distinct from
    /// unknown_tool so agents see the recovery ladder instead of "not found".
    pub(crate) fn policy_refusal(name: &str, message: String) -> Self {
        Self {
            value: json!({
                "kind": "policy_refusal",
                "error_type": "POLICY",
                "message": message,
                "recoverable": true,
                "entity_type": "tool",
                "provided": name,
                "tool": name,
                "reason": message,
                "fix_hint": "When primary surface is healthy use zero.token.* via tz_execute_code. After expand X0, tz_expand/tz_read unlock for recovery only.",
                "policy": "crash_only_recovery",
                "suggested_tool_calls": [
                    {"method": "tools/call", "params": {"name": "tz_execute_code", "arguments": {"plan": "return await zero.token.expand('<ref>')"}}},
                    {"method": "resources/read", "params": {"uri": "resource://tokenzero/metrics"}}
                ]
            }),
        }
    }

    fn invalid_logging_level(level: &str) -> Self {
        let available_options = logging_levels();
        Self::recoverable(
            "invalid_param_value",
            "INVALID_ARGUMENT",
            format!("unsupported logging/setLevel level: {level}"),
            "Set params.level to one of available_options.".into(),
            json!(available_options),
            json!(similar_options(level, &logging_levels())),
            json!([{"method": "logging/setLevel", "params": {"level": "info"}}]),
            json!({
                "method": "logging/setLevel",
                "param": "level",
                "parameter": "level",
                "provided": level,
                "available_levels": logging_levels(),
            }),
        )
    }

    pub(crate) fn unknown_resource(uri: &str) -> Self {
        let available_options = resource_uris();
        Self::recoverable(
            "unknown_resource",
            "NOT_FOUND",
            format!("unknown resource: {uri}"),
            "Call resources/list, then retry resources/read with one of available_options as params.uri.".into(),
            json!(available_options),
            json!(similar_options(uri, &resource_uris())),
            json!([{"method": "resources/list", "params": {}}]),
            json!({
                "entity_type": "resource",
                "provided": uri,
                "uri": uri,
                "available_resources": resource_uris(),
            }),
        )
    }

    fn unknown_tool_cluster(cluster: &str) -> Self {
        let mut available_options = vec!["all".to_string(), "full".to_string()];
        available_options.extend(
            tool_cluster_names()
                .into_iter()
                .map(|cluster| cluster.to_string()),
        );
        let suggestions = similar_options(cluster, &available_options);
        Self::recoverable(
            "unknown_tool_cluster",
            "INVALID_ARGUMENT",
            format!("unknown tools/list cluster: {cluster}"),
            "Use tools/list params._meta.tokenzero/toolCluster with one of available_options, or omit it for the full catalog.".into(),
            json!(available_options),
            json!(suggestions),
            json!([
                {"method": "tools/list", "params": {"_meta": {"tokenzero/toolCluster": "material"}}},
                {"method": "tools/list", "params": {"_meta": {"tokenzero/toolCluster": "execution"}}}
            ]),
            json!({
                "entity_type": "tool_cluster",
                "provided": cluster,
                "parameter": "cluster",
                "available_clusters": tool_cluster_names(),
            }),
        )
    }

    fn unknown_method(method: &str) -> Self {
        let available_options = JSONRPC_METHODS.iter().map(ToString::to_string).collect::<Vec<_>>();
        let suggestions = similar_options(method, &available_options);
        Self::recoverable(
            "unknown_method",
            "NOT_FOUND",
            format!("unknown JSON-RPC method: {method}"),
            "Call server/discover for protocol capabilities or use one of available_options."
                .into(),
            json!(available_options),
            json!(suggestions),
            json!([{"method": "server/discover", "params": {}}]),
            json!({
                "entity_type": "method",
                "provided": method,
                "method": method,
                "available_methods": JSONRPC_METHODS,
            }),
        )
    }

    pub(crate) fn parse_error(reason: String) -> Self {
        Self::recoverable(
            "parse_error",
            "INVALID_REQUEST",
            reason,
            "Send one valid JSON-RPC 2.0 request object per line, or use Content-Length framed JSON.".into(),
            json!(JSONRPC_METHODS),
            json!([]),
            json!([]),
            json!({}),
        )
    }

    fn invalid_request(reason: impl Into<String>, fix_hint: impl Into<String>) -> Self {
        Self::recoverable(
            "invalid_request",
            "INVALID_REQUEST",
            reason.into(),
            fix_hint.into(),
            json!(JSONRPC_METHODS),
            json!([]),
            json!([{"method": "server/discover", "params": {}}]),
            json!({}),
        )
    }
}

impl From<String> for JsonRpcErrorData {
    fn from(reason: String) -> Self {
        Self::invalid_params(reason)
    }
}

fn jsonrpc_invalid_params_error(id: Value, data: JsonRpcErrorData) -> Value {
    jsonrpc_error(id, -32602, "Invalid params", data)
}

pub(crate) fn jsonrpc_error(id: Value, code: i64, message: &str, data: JsonRpcErrorData) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
            "data": data.value
        }
    })
}

fn tool_names() -> Vec<String> {
    tool_specs().into_iter().map(|tool| tool.name).collect()
}

fn resource_uris() -> Vec<String> {
    resource_specs()
        .into_iter()
        .map(|resource| resource.uri)
        .collect()
}

fn logging_levels() -> Vec<String> {
    LOGGING_LEVELS.iter().copied().map(str::to_string).collect()
}

fn similar_options(input: &str, options: &[String]) -> Vec<Value> {
    let mut scored = options
        .iter()
        .map(|option| (option, similarity_score(input, option)))
        .filter(|(_, score)| *score >= 0.35)
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(right.0))
    });
    scored
        .into_iter()
        .take(5)
        .map(|(value, score)| {
            json!({
                "value": value,
                "score": (score * 100.0).round() / 100.0
            })
        })
        .collect()
}

fn similarity_score(left: &str, right: &str) -> f64 {
    let left = left.to_ascii_lowercase();
    let right = right.to_ascii_lowercase();
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    if left == right {
        return 1.0;
    }
    let distance = levenshtein_distance(&left, &right);
    let max_len = left.chars().count().max(right.chars().count()) as f64;
    let edit_score = 1.0 - (distance as f64 / max_len);
    let containment_bonus = if left.contains(&right) || right.contains(&left) {
        0.2
    } else {
        0.0
    };
    (edit_score + containment_bonus).clamp(0.0, 1.0)
}

fn levenshtein_distance(left: &str, right: &str) -> usize {
    let right_chars = right.chars().collect::<Vec<_>>();
    let mut costs = (0..=right_chars.len()).collect::<Vec<_>>();
    for (left_idx, left_char) in left.chars().enumerate() {
        let mut previous_diagonal = costs[0];
        costs[0] = left_idx + 1;
        for (right_idx, right_char) in right_chars.iter().enumerate() {
            let previous_cost = costs[right_idx + 1];
            let substitution = previous_diagonal + usize::from(left_char != *right_char);
            let insertion = costs[right_idx] + 1;
            let deletion = costs[right_idx + 1] + 1;
            costs[right_idx + 1] = substitution.min(insertion.min(deletion));
            previous_diagonal = previous_cost;
        }
    }
    costs[right_chars.len()]
}
