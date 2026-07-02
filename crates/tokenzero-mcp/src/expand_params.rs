//! Shared expand parameter parsing for MCP, CodeMode, and the engine.

use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct ExpandParams {
    pub ref_id: String,
    pub selector: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub anchor_kind: Option<String>,
    pub symbol: Option<String>,
    pub since: Option<String>,
    pub fresh: bool,
}

impl ExpandParams {
    pub fn from_tool_args(args: &Value) -> Result<Self, String> {
        let ref_id = args
            .get("ref")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing ref".to_string())?
            .to_string();
        Ok(Self {
            ref_id,
            selector: args.get("selector").and_then(Value::as_str).map(str::to_string),
            start_line: arg_u64(args, "start_line"),
            end_line: arg_u64(args, "end_line"),
            anchor_kind: args.get("anchor_kind").and_then(Value::as_str).map(str::to_string),
            symbol: args.get("symbol").and_then(Value::as_str).map(str::to_string),
            since: args.get("since").and_then(Value::as_str).map(str::to_string),
            fresh: arg_bool(args, "fresh"),
        })
    }

    pub fn from_codemode_args(args: &[Value]) -> Result<Self, String> {
        let ref_id = args
            .first()
            .and_then(Value::as_str)
            .ok_or_else(|| {
                "zero.token.expand/zero.expand requires a tz:// ref string as first argument".to_string()
            })?
            .to_string();
        let opts = args.get(1).and_then(Value::as_object);
        let mut params = Self { ref_id, ..Default::default() };
        if let Some(map) = opts {
            params.selector = map.get("selector").and_then(Value::as_str).map(str::to_string);
            params.start_line = map.get("start_line").and_then(coerce_u64).and_then(|n| usize::try_from(n).ok());
            params.end_line = map.get("end_line").and_then(coerce_u64).and_then(|n| usize::try_from(n).ok());
            params.anchor_kind = map.get("anchor_kind").and_then(Value::as_str).map(str::to_string);
            params.symbol = map.get("symbol").and_then(Value::as_str).map(str::to_string);
            params.since = map.get("since").and_then(Value::as_str).map(str::to_string);
            params.fresh = map.get("fresh").map(arg_bool_value).unwrap_or(false);
        }
        Ok(params)
    }

    pub fn from_expand_many_item(item: &Value) -> Result<Self, String> {
        if let Some(ref_id) = item.as_str() {
            return Ok(Self { ref_id: ref_id.to_string(), ..Default::default() });
        }
        let map = item.as_object().ok_or_else(|| "expandMany item must be a tz:// ref string or object".to_string())?;
        let ref_id = map.get("ref").and_then(Value::as_str).ok_or_else(|| "expandMany item object requires ref".to_string())?.to_string();
        Ok(Self {
            ref_id,
            selector: map.get("selector").and_then(Value::as_str).map(str::to_string),
            start_line: map.get("start_line").and_then(coerce_u64).and_then(|n| usize::try_from(n).ok()),
            end_line: map.get("end_line").and_then(coerce_u64).and_then(|n| usize::try_from(n).ok()),
            anchor_kind: map.get("anchor_kind").and_then(Value::as_str).map(str::to_string),
            symbol: map.get("symbol").and_then(Value::as_str).map(str::to_string),
            since: map.get("since").and_then(Value::as_str).map(str::to_string),
            fresh: map.get("fresh").map(arg_bool_value).unwrap_or(false),
        })
    }
}

fn arg_u64(args: &Value, key: &str) -> Option<usize> {
    coerce_u64(args.get(key)?).and_then(|value| usize::try_from(value).ok())
}

fn coerce_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn arg_bool(args: &Value, key: &str) -> bool {
    args.get(key).map(arg_bool_value).unwrap_or(false)
}

fn arg_bool_value(value: &Value) -> bool {
    match value {
        Value::Bool(v) => *v,
        Value::String(text) => matches!(text.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes"),
        _ => false,
    }
}
