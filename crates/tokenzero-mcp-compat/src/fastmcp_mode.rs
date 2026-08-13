use serde_json::Value;
use std::sync::{Arc, Mutex};

use tokenzero_core::McpToolSurface;
use tokenzero_core::count_tokens;
use tokenzero_core::operation_abi::Mutability;
use zero_abi::{
    ALL_DISPATCH_ERROR_CLASSES, ApprovalRequirement, CanonicalOperation, CanonicalRegistry,
    CanonicalResource, EffectClass, EffectPolicy, EngineIdentity, PermitRequirement,
    RefOwnership as SharedRefOwnership, RegistryEngine, TelemetrySchema,
};
use zero_mcp::{
    CapabilityDescriptor, DomainAdapterRegistration, FastMcpTransport, McpAliasMetadata,
    McpCallContext, McpDispatchError, McpDispatchOutput, McpDispatcher, McpErrorPresentation,
    McpResourceOutput, McpResourceReader, McpTextContent, McpTransportConfig, SurfaceKind,
    SurfaceRegistration,
};

use crate::catalog::{TOOL_ALIASES, canonical_tool_specs, resource_specs};
use crate::resources::build_resource_payload;
use crate::surface_health::surface_includes;
use crate::{EngineConfig, TokenZeroEngine, call_tool_fastmcp};

/// Preserve the legacy FastMCP content projection byte-for-byte while the hub
/// owns registration, cancellation, and stdio transport.
pub(crate) fn fastmcp_content_texts_from_tool_result(
    result: &Value,
) -> Result<Vec<String>, String> {
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if is_error {
        let err_text = result
            .get("content")
            .and_then(|content| content.get(0))
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("tool error");
        return Err(err_text.to_owned());
    }
    let primary_text = result
        .get("content")
        .and_then(|content| content.get(0))
        .and_then(|content| content.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(structured) = result.get("structuredContent") {
        if let Some(folded) = scalar_folded_codemode_v2(primary_text, structured) {
            return Ok(vec![folded]);
        }
        let metadata = serde_json::to_string(structured).unwrap_or_default();
        if metadata != "null" {
            return Ok(vec![primary_text.to_owned(), metadata]);
        }
        return Ok(vec![primary_text.to_owned()]);
    }
    let mut contents = vec![primary_text.to_owned()];
    if primary_text.starts_with("ok tz") || primary_text.starts_with("err ") {
        return Ok(contents);
    }
    let mut metadata = serde_json::Map::new();
    if let Some(result_type) = result.get("resultType").and_then(Value::as_str) {
        metadata.insert("resultType".into(), Value::String(result_type.to_owned()));
    }
    if let Some(recovery) = result.get("recovery") {
        metadata.insert("recovery".into(), recovery.clone());
    }
    let metadata = serde_json::to_string(&Value::Object(metadata)).unwrap_or_default();
    if metadata != "{}" {
        contents.push(metadata);
    }
    Ok(contents)
}

fn scalar_folded_codemode_v2(primary_text: &str, structured: &Value) -> Option<String> {
    let object = structured.as_object()?;
    let value = object.get("value")?;
    if !(value.is_string() || value.is_number() || value.is_boolean()) {
        return None;
    }
    let value_text = serde_json::to_string(value).ok()?;
    if count_tokens(&value_text) > 16 {
        return None;
    }
    let ack = object
        .get("ack")
        .and_then(Value::as_str)
        .unwrap_or(primary_text);
    if ack.contains(&format!(" ={value_text}")) {
        return Some(ack.to_owned());
    }
    let (prefix, suffix) = ack.rsplit_once(" t:")?;
    Some(format!("{prefix} ={value_text} t:{suffix}"))
}

fn canonical_id(operation_name: &str, cluster: &str) -> String {
    let method = operation_name.strip_prefix("tz_").unwrap_or(operation_name);
    format!("{cluster}.{method}")
}

fn effect_policy(mutability: Mutability) -> EffectPolicy {
    match mutability {
        Mutability::ReadOnly => EffectPolicy {
            effect_class: EffectClass::ReadOnly,
            permit: PermitRequirement::NotRequired,
            approval: ApprovalRequirement::NotRequired,
        },
        Mutability::WorkspaceMutating => EffectPolicy {
            effect_class: EffectClass::ReversibleMutation,
            permit: PermitRequirement::Required,
            approval: ApprovalRequirement::NotRequired,
        },
        Mutability::StoreOnly => EffectPolicy {
            effect_class: EffectClass::ReversibleMutation,
            permit: PermitRequirement::Required,
            approval: ApprovalRequirement::NotRequired,
        },
    }
}

fn mcp_aliases_for(target: &str) -> Vec<String> {
    TOOL_ALIASES
        .iter()
        .filter_map(|(alias, canonical)| (*canonical == target).then(|| (*alias).to_owned()))
        .collect()
}

fn canonical_operation(
    operation: &tokenzero_core::operation_abi::Operation,
    description: &str,
) -> CanonicalOperation {
    CanonicalOperation {
        canonical_id: canonical_id(operation.name, operation.cluster),
        description: description.to_owned(),
        aliases: mcp_aliases_for(operation.name),
        args_schema: operation.args.schema.clone(),
        output_schema: Some(operation.results.schema.clone()),
        mcp_tool_name: Some(operation.name.to_owned()),
        effect_policy: effect_policy(operation.mutability),
        errors: ALL_DISPATCH_ERROR_CLASSES.to_vec(),
    }
}

fn canonical_resource(spec: crate::catalog::ResourceSpec) -> CanonicalResource {
    CanonicalResource {
        uri: spec.uri,
        name: spec.name,
        description: spec.description,
        mime_type: Some(spec.mime_type),
    }
}

fn surface_registration(engine: &TokenZeroEngine, surface: McpToolSurface) -> SurfaceRegistration {
    let operations = canonical_tool_specs()
        .iter()
        .filter(|seed| surface_includes(surface, seed.name))
        .map(|seed| {
            let operation = tokenzero_core::operation_abi::operation_by_name(seed.name)
                .expect("MCP catalog tool must exist in the operation ABI");
            canonical_operation(operation, seed.summary)
        })
        .collect::<Vec<_>>();
    let capabilities = operations
        .iter()
        .map(|operation| {
            let (cluster, method) = operation
                .canonical_id
                .split_once('.')
                .expect("namespaced canonical operation id");
            CapabilityDescriptor::new(cluster, method)
        })
        .collect();
    let resources = resource_specs()
        .into_iter()
        .map(canonical_resource)
        .collect();
    let registry = CanonicalRegistry {
        version: zero_abi::CANONICAL_DISPATCH_VERSION.to_owned(),
        engine: RegistryEngine::TokenZero,
        operations,
        resources,
    };
    let adapter = DomainAdapterRegistration {
        engine: EngineIdentity::TokenZero,
        registry,
        ref_ownership: SharedRefOwnership {
            engine: EngineIdentity::TokenZero,
            session_id: engine.session_id().to_owned(),
            refs: Vec::new(),
            snapshot: None,
        },
        telemetry_schema: TelemetrySchema::V1,
        capabilities,
    };
    let mut registration = SurfaceRegistration::new(SurfaceKind::Mcp, "TokenZero", adapter);
    registration.instructions = Some(
        match surface {
            McpToolSurface::Classic => fastmcp_instructions(),
            McpToolSurface::CodeMode => fastmcp_codemode_instructions(),
        }
        .to_owned(),
    );
    registration
}

struct EngineDispatcher {
    engine: Arc<Mutex<TokenZeroEngine>>,
}

impl McpDispatcher for EngineDispatcher {
    fn dispatch(
        &self,
        tool: &str,
        arguments: Value,
        context: &McpCallContext,
    ) -> Result<Value, McpDispatchError> {
        match self.dispatch_output(tool, arguments, context)? {
            McpDispatchOutput::Json(value) => Ok(value),
            McpDispatchOutput::Text(items) => Ok(Value::Array(
                items
                    .into_iter()
                    .map(|item| Value::String(item.text))
                    .collect(),
            )),
        }
    }

    fn dispatch_output(
        &self,
        tool: &str,
        arguments: Value,
        _context: &McpCallContext,
    ) -> Result<McpDispatchOutput, McpDispatchError> {
        let engine = self.engine.lock().map_err(|_| {
            McpDispatchError::new("runtime", "TokenZero engine lock poisoned", false).with_op(tool)
        })?;
        let result = call_tool_fastmcp(&engine, tool, &arguments, None).map_err(|error| {
            McpDispatchError::new("runtime", error.message_text(), false).with_op(tool)
        })?;
        let texts = crate::fastmcp_mode::fastmcp_content_texts_from_tool_result(&result)
            .map_err(|message| McpDispatchError::new("runtime", message, false).with_op(tool))?;
        Ok(McpDispatchOutput::Text(
            texts.into_iter().map(McpTextContent::new).collect(),
        ))
    }
}

struct EngineResourceReader {
    engine: Arc<Mutex<TokenZeroEngine>>,
}

impl McpResourceReader for EngineResourceReader {
    fn read(&self, uri: &str, context: &McpCallContext) -> Result<Value, McpDispatchError> {
        match self.read_output(uri, context)? {
            McpResourceOutput::Json(value) => Ok(value),
            McpResourceOutput::Text(text) | McpResourceOutput::Blob(text) => {
                Ok(Value::String(text))
            }
        }
    }

    fn read_output(
        &self,
        uri: &str,
        _context: &McpCallContext,
    ) -> Result<McpResourceOutput, McpDispatchError> {
        let engine = self.engine.lock().map_err(|_| {
            McpDispatchError::new("runtime", "TokenZero engine lock poisoned", false).with_op(uri)
        })?;
        let payload = build_resource_payload(&engine, uri).map_err(|error| {
            McpDispatchError::new("runtime", error.message_text(), false).with_op(uri)
        })?;
        Ok(McpResourceOutput::Text(payload))
    }
}

/// One-mode instruction text preserved from the legacy FastMCP carrier.
pub fn fastmcp_instructions() -> &'static str {
    "TokenZero MCP surface. Tools: read, find, grep, glob, tree, edit, recall, batch, \
     fetch, shell, ingest, expand, mem, cache_pack, rewrite, discover, plus tz_* aliases. \
     If your harness supports ZeroStack CodeMode (zero_execute plans), use the CodeMode \
     mode INSTEAD — never both at once. \
     Full per-tool docs: resources/read resource://tokenzero/tools."
}

pub fn fastmcp_codemode_instructions() -> &'static str {
    "TokenZero aggregate binding metadata is consumed by ZeroStack. This classic \
     compatibility crate does not register or execute an engine-local CodeMode \
     surface; the aggregate host dispatches dotted bindings through raw-worker v2."
}

fn alias_metadata(surface: McpToolSurface) -> Vec<McpAliasMetadata> {
    TOOL_ALIASES
        .iter()
        .filter(|(_, target)| surface_includes(surface, target))
        .map(|(alias, target)| {
            let operation = tokenzero_core::operation_abi::operation_by_name(target)
                .expect("MCP alias target must exist in the operation ABI");
            McpAliasMetadata {
                canonical_id: canonical_id(operation.name, operation.cluster),
                name: (*alias).to_owned(),
                description: Some(crate::catalog::alias_summary(target)),
                input_schema: serde_json::json!({"type": "object"}),
                output_schema: None,
            }
        })
        .collect()
}

/// Start the hub-owned FastMCP stdio server.
pub fn run_fastmcp_stdio(config: EngineConfig) -> ! {
    let surface = config.tool_surface;
    if surface != McpToolSurface::Classic {
        eprintln!(
            "tokenzero-mcp: engine-local CodeMode was retired; use the ZeroStack aggregate host"
        );
        std::process::exit(2);
    }
    let engine = Arc::new(Mutex::new(TokenZeroEngine::new(config)));
    let registration = {
        let guard = engine.lock().expect("new TokenZeroEngine lock");
        surface_registration(&guard, surface)
    };
    let dispatcher: Arc<dyn McpDispatcher> = Arc::new(EngineDispatcher {
        engine: Arc::clone(&engine),
    });
    let reader: Arc<dyn McpResourceReader> = Arc::new(EngineResourceReader { engine });
    let aliases = alias_metadata(surface);
    let transport = match FastMcpTransport::with_resources(
        registration,
        dispatcher,
        reader,
        McpTransportConfig::default(),
    )
    .and_then(|transport| transport.with_server_identity("tokenzero", env!("CARGO_PKG_VERSION")))
    .map(|transport| transport.with_error_presentation(McpErrorPresentation::PlainMessage))
    .and_then(|transport| transport.with_alias_metadata(aliases))
    {
        Ok(transport) => transport,
        Err(error) => {
            eprintln!("tokenzero: invalid ZeroStack surface registration: {error}");
            std::process::exit(2);
        }
    };
    transport.run_stdio()
}

#[cfg(test)]
#[path = "../../../tests/mcp-compat/inline/fastmcp_mode__tests.rs"]
mod tests;
