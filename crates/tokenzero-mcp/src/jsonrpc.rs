use serde_json::{Value, json};
use tokenzero_core::{MCP_SCHEMA_VERSION, McpToolSurface};

use crate::catalog::{canonical_allowed_on_surface, tool_cluster_names, tool_specs_for_filter};
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

fn handle_jsonrpc_request(engine: &TokenZeroEngine, parsed: Value) -> Option<Value> {
    let Some(object) = parsed.as_object() else {
        return Some(jsonrpc_error(
            Value::Null,
            -32600,
            "Invalid Request",
            JsonRpcErrorData::invalid_request(
                "request must be a JSON object",
                "Send an object with jsonrpc, method, params, and id fields.",
            ),
        ));
    };
    let id_for_error = object
        .get("id")
        .filter(|id| is_valid_jsonrpc_id(id))
        .cloned()
        .unwrap_or(Value::Null);
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(jsonrpc_error(
            id_for_error,
            -32600,
            "Invalid Request",
            JsonRpcErrorData::invalid_request(
                "jsonrpc must be \"2.0\"",
                "Set the top-level jsonrpc field to \"2.0\" and retry.",
            ),
        ));
    }
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return Some(jsonrpc_error(
            id_for_error,
            -32600,
            "Invalid Request",
            JsonRpcErrorData::invalid_request(
                "method must be a string",
                "Set method to initialize, tools/list, tools/call, resources/list, or resources/read.",
            ),
        ));
    };
    if let Some(id) = object.get("id") {
        if !is_valid_jsonrpc_id(id) {
            return Some(jsonrpc_error(
                Value::Null,
                -32600,
                "Invalid Request",
                JsonRpcErrorData::invalid_request(
                    "id must be string, number, or null",
                    "Use a string, number, or null id so the response can be correlated.",
                ),
            ));
        }
    }
    if let Some(params) = object.get("params") {
        if !(params.is_object() || params.is_array()) {
            return Some(jsonrpc_error(
                id_for_error,
                -32600,
                "Invalid Request",
                JsonRpcErrorData::invalid_request(
                    "params must be object or array",
                    "Use object params for MCP methods; omit params when no parameters are needed.",
                ),
            ));
        }
    }
    if !object.contains_key("id") {
        return None;
    }
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let requested = match initialize_protocol_version(object, &id) {
                Ok(requested) => requested,
                Err(error) => return Some(error),
            };
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
        "ping" | "notifications/initialized" => json!({}),
        #[cfg(test)]
        "tokenzero/internal/test-panic" => panic!("test-induced tool panic"),
        "server/discover" => {
            let params = match meta_only_params(object, &id, "server/discover") {
                Ok(params) => params,
                Err(error) => return Some(error),
            };
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
        "resources/list" => {
            let params = match object_params(object, &id, "resources/list") {
                Ok(params) => params,
                Err(error) => return Some(error),
            };
            if let Err(error) = ensure_optional_cursor_param(params, &id, "resources/list") {
                return Some(error);
            }
            json!({
                "resources": resource_specs(),
                "resultType": "complete",
                "ttlMs": 60000,
                "cacheScope": "workspace"
            })
        }
        "resources/templates/list" => {
            let params = match object_params(object, &id, "resources/templates/list") {
                Ok(params) => params,
                Err(error) => return Some(error),
            };
            if let Err(error) =
                ensure_optional_cursor_param(params, &id, "resources/templates/list")
            {
                return Some(error);
            }
            json!({
                "resourceTemplates": [],
                "resultType": "complete",
                "ttlMs": 60000,
                "cacheScope": "workspace"
            })
        }
        "resources/read" => {
            let params = match object_params(object, &id, "resources/read") {
                Ok(Some(params)) => params,
                Ok(None) => {
                    return Some(jsonrpc_invalid_params_error(
                        id,
                        JsonRpcErrorData::missing_param("resources/read", "uri"),
                    ));
                }
                Err(error) => return Some(error),
            };
            let uri = match required_string_param(params, &id, "resources/read", "uri") {
                Ok(uri) => uri,
                Err(error) => return Some(error),
            };
            match read_resource(engine, uri) {
                Ok(value) => value,
                Err(error) => {
                    return Some(jsonrpc_invalid_params_error(id, error));
                }
            }
        }
        "prompts/list" => {
            let params = match object_params(object, &id, "prompts/list") {
                Ok(params) => params,
                Err(error) => return Some(error),
            };
            if let Err(error) = ensure_optional_cursor_param(params, &id, "prompts/list") {
                return Some(error);
            }
            json!({"prompts": []})
        }
        "logging/setLevel" => {
            let params = match required_object_params(object, &id, "logging/setLevel") {
                Ok(params) => params,
                Err(error) => return Some(error),
            };
            if let Err(error) = logging_level(params, &id) {
                return Some(error);
            }
            json!({})
        }
        "tools/list" => {
            let params = match object_params(object, &id, "tools/list") {
                Ok(params) => params,
                Err(error) => return Some(error),
            };
            if let Err(error) = ensure_optional_cursor_param(params, &id, "tools/list") {
                return Some(error);
            }
            let filter = match tool_list_filter(params) {
                Ok(filter) => filter,
                Err(error) => return Some(jsonrpc_invalid_params_error(id, error)),
            };
            let tools = tool_specs_for_filter(
                filter.cluster.as_deref(),
                filter.include_aliases,
                engine.config.tool_surface,
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
        "tools/call" => {
            let params = match object_params(object, &id, "tools/call") {
                Ok(Some(params)) => params,
                Ok(None) => {
                    return Some(jsonrpc_invalid_params_error(
                        id,
                        JsonRpcErrorData::missing_param("tools/call", "name"),
                    ));
                }
                Err(error) => return Some(error),
            };
            let name = match required_string_param(params, &id, "tools/call", "name") {
                Ok(name) => name,
                Err(error) => return Some(error),
            };
            let args = match tool_arguments(params, &id) {
                Ok(args) => args,
                Err(error) => return Some(error),
            };
            let call_id = match &id {
                Value::Null => None,
                other => Some(other.to_string()),
            };
            match call_tool(engine, name, &args, call_id) {
                Ok(value) => value,
                Err(error) => {
                    return Some(jsonrpc_invalid_params_error(id, error));
                }
            }
        }
        _ => {
            return Some(jsonrpc_error(
                id,
                -32601,
                "Method not found",
                JsonRpcErrorData::unknown_method(method),
            ));
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

fn required_object_params<'a>(
    object: &'a serde_json::Map<String, Value>,
    id: &Value,
    method: &str,
) -> Result<&'a serde_json::Map<String, Value>, Value> {
    match object_params(object, id, method)? {
        Some(params) => Ok(params),
        None => Err(jsonrpc_invalid_params_error(
            id.clone(),
            JsonRpcErrorData::missing_param(method, "params"),
        )),
    }
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

const JSONRPC_METHODS: &[&str] = &[
    "initialize",
    "notifications/initialized",
    "ping",
    "server/discover",
    "resources/list",
    "resources/templates/list",
    "resources/read",
    "prompts/list",
    "logging/setLevel",
    "tools/list",
    "tools/call",
];

#[derive(Debug, Clone)]
pub(crate) struct JsonRpcErrorData {
    value: Value,
}

impl JsonRpcErrorData {
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
        let reason = reason.into();
        Self {
            value: json!({
                "kind": "internal_error",
                "error_type": "INTERNAL",
                "message": reason,
                "recoverable": true,
                "reason": reason,
                "fix_hint": "Retry the request; the server isolated an internal fault and remains available.",
                "available_options": [],
                "suggestions": [],
                "suggested_tool_calls": []
            }),
        }
    }

    fn invalid_params(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            value: json!({
                "kind": "invalid_params",
                "error_type": "INVALID_ARGUMENT",
                "message": reason,
                "recoverable": true,
                "reason": reason,
                "fix_hint": "Check the method input schema from tools/list or resources/list, then retry with object params.",
                "available_options": ["tools/list", "resources/list"],
                "suggestions": [],
                "suggested_tool_calls": [
                    {"method": "tools/list", "params": {}},
                    {"method": "resources/list", "params": {}}
                ]
            }),
        }
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
        let message = format!("missing {param}");
        Self {
            value: json!({
                "kind": "missing_param",
                "error_type": "INVALID_ARGUMENT",
                "message": message,
                "recoverable": true,
                "method": method,
                "param": param,
                "parameter": param,
                "reason": message,
                "fix_hint": fix_hint,
                "available_options": available_options,
                "suggestions": [],
                "suggested_tool_calls": suggested_tool_calls
            }),
        }
    }

    pub(crate) fn unknown_tool(name: &str) -> Self {
        let available_options = tool_names();
        let suggestions = similar_options(name, &available_options);
        Self {
            value: json!({
                "kind": "unknown_tool",
                "error_type": "NOT_FOUND",
                "message": format!("unknown tool: {name}"),
                "recoverable": true,
                "entity_type": "tool",
                "provided": name,
                "tool": name,
                "reason": format!("unknown tool: {name}"),
                "fix_hint": "Call tools/list, then retry tools/call with one of available_options as params.name.",
                "available_options": available_options,
                "available_tools": tool_names(),
                "suggestions": suggestions,
                "suggested_tool_calls": [
                    {"method": "tools/list", "params": {}}
                ]
            }),
        }
    }

    fn invalid_logging_level(level: &str) -> Self {
        let available_options = logging_levels();
        let suggestions = similar_options(level, &available_options);
        Self {
            value: json!({
                "kind": "invalid_param_value",
                "error_type": "INVALID_ARGUMENT",
                "message": format!("unsupported logging/setLevel level: {level}"),
                "recoverable": true,
                "method": "logging/setLevel",
                "param": "level",
                "parameter": "level",
                "provided": level,
                "reason": format!("unsupported logging/setLevel level: {level}"),
                "fix_hint": "Set params.level to one of available_options.",
                "available_options": available_options,
                "available_levels": logging_levels(),
                "suggestions": suggestions,
                "suggested_tool_calls": [
                    {"method": "logging/setLevel", "params": {"level": "info"}}
                ]
            }),
        }
    }

    pub(crate) fn unknown_resource(uri: &str) -> Self {
        let available_options = resource_uris();
        let suggestions = similar_options(uri, &available_options);
        Self {
            value: json!({
                "kind": "unknown_resource",
                "error_type": "NOT_FOUND",
                "message": format!("unknown resource: {uri}"),
                "recoverable": true,
                "entity_type": "resource",
                "provided": uri,
                "uri": uri,
                "reason": format!("unknown resource: {uri}"),
                "fix_hint": "Call resources/list, then retry resources/read with one of available_options as params.uri.",
                "available_options": available_options,
                "available_resources": resource_uris(),
                "suggestions": suggestions,
                "suggested_tool_calls": [
                    {"method": "resources/list", "params": {}}
                ]
            }),
        }
    }

    fn unknown_tool_cluster(cluster: &str) -> Self {
        let mut available_options = vec!["all".to_string(), "full".to_string()];
        available_options.extend(
            tool_cluster_names()
                .into_iter()
                .map(|cluster| cluster.to_string()),
        );
        let suggestions = similar_options(cluster, &available_options);
        Self {
            value: json!({
                "kind": "unknown_tool_cluster",
                "error_type": "INVALID_ARGUMENT",
                "message": format!("unknown tools/list cluster: {cluster}"),
                "recoverable": true,
                "entity_type": "tool_cluster",
                "provided": cluster,
                "parameter": "cluster",
                "reason": format!("unknown tools/list cluster: {cluster}"),
                "fix_hint": "Use tools/list params._meta.tokenzero/toolCluster with one of available_options, or omit it for the full catalog.",
                "available_options": available_options,
                "available_clusters": tool_cluster_names(),
                "suggestions": suggestions,
                "suggested_tool_calls": [
                    {"method": "tools/list", "params": {"_meta": {"tokenzero/toolCluster": "material"}}},
                    {"method": "tools/list", "params": {"_meta": {"tokenzero/toolCluster": "execution"}}}
                ]
            }),
        }
    }

    fn unknown_method(method: &str) -> Self {
        let available_options = JSONRPC_METHODS
            .iter()
            .map(|method| (*method).to_string())
            .collect::<Vec<_>>();
        let suggestions = similar_options(method, &available_options);
        Self {
            value: json!({
                "kind": "unknown_method",
                "error_type": "NOT_FOUND",
                "message": format!("unknown JSON-RPC method: {method}"),
                "recoverable": true,
                "entity_type": "method",
                "provided": method,
                "method": method,
                "reason": format!("unknown JSON-RPC method: {method}"),
                "fix_hint": "Call server/discover for protocol capabilities or use one of available_options.",
                "available_options": available_options,
                "available_methods": JSONRPC_METHODS,
                "suggestions": suggestions,
                "suggested_tool_calls": [
                    {"method": "server/discover", "params": {}}
                ]
            }),
        }
    }

    pub(crate) fn parse_error(reason: String) -> Self {
        Self {
            value: json!({
                "kind": "parse_error",
                "error_type": "INVALID_REQUEST",
                "message": reason,
                "recoverable": true,
                "reason": reason,
                "fix_hint": "Send one valid JSON-RPC 2.0 request object per line, or use Content-Length framed JSON.",
                "available_options": JSONRPC_METHODS,
                "suggestions": [],
                "suggested_tool_calls": []
            }),
        }
    }

    fn invalid_request(reason: impl Into<String>, fix_hint: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            value: json!({
                "kind": "invalid_request",
                "error_type": "INVALID_REQUEST",
                "message": reason,
                "recoverable": true,
                "reason": reason,
                "fix_hint": fix_hint.into(),
                "available_options": JSONRPC_METHODS,
                "suggestions": [],
                "suggested_tool_calls": [
                    {"method": "server/discover", "params": {}}
                ]
            }),
        }
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
