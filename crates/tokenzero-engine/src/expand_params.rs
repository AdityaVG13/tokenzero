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
    /// When true, return full body (byte-exact). When false (default) and no
    /// window/selector is specified, return a capsule (preview + ref + estimates)
    /// instead of the full payload (wqw.13).
    pub raw: bool,
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
            selector: args
                .get("selector")
                .and_then(Value::as_str)
                .map(str::to_string),
            start_line: arg_u64(args, "start_line"),
            end_line: arg_u64(args, "end_line"),
            anchor_kind: args
                .get("anchor_kind")
                .and_then(Value::as_str)
                .map(str::to_string),
            symbol: args
                .get("symbol")
                .and_then(Value::as_str)
                .map(str::to_string),
            since: args
                .get("since")
                .and_then(Value::as_str)
                .map(str::to_string),
            fresh: arg_bool(args, "fresh"),
            raw: arg_bool(args, "raw"),
        })
    }

    pub fn from_codemode_args(args: &[Value]) -> Result<Self, String> {
        let Some(first) = args.first() else {
            return Err(
                "zero.token.expand/zero.expand requires a tz:// ref string as first argument \
                 (got no args); accepted forms: expand(ref), expand({ref}), or expand([ref, ...])"
                    .to_string(),
            );
        };
        // Object form `{ref, ...}` — same shape as expandMany items / MCP tool args.
        if first.is_object() {
            if args.len() > 1 {
                return Err(
                    "zero.token.expand/zero.expand object form takes a single {ref, ...} argument \
                     (options belong inside the object)"
                        .to_string(),
                );
            }
            return Self::from_expand_many_item(first);
        }
        let ref_id = first
            .as_str()
            .ok_or_else(|| {
                format!(
                    "zero.token.expand/zero.expand requires a tz:// ref string, {{ref}} object, \
                 or array of refs as first argument — got {}",
                    value_shape(first)
                )
            })?
            .to_string();
        let opts = args.get(1).and_then(Value::as_object);
        let mut params = Self {
            ref_id,
            ..Default::default()
        };
        if let Some(map) = opts {
            params.selector = map
                .get("selector")
                .and_then(Value::as_str)
                .map(str::to_string);
            params.start_line = map
                .get("start_line")
                .and_then(coerce_u64)
                .and_then(|n| usize::try_from(n).ok());
            params.end_line = map
                .get("end_line")
                .and_then(coerce_u64)
                .and_then(|n| usize::try_from(n).ok());
            params.anchor_kind = map
                .get("anchor_kind")
                .and_then(Value::as_str)
                .map(str::to_string);
            params.symbol = map
                .get("symbol")
                .and_then(Value::as_str)
                .map(str::to_string);
            params.since = map.get("since").and_then(Value::as_str).map(str::to_string);
            params.fresh = map.get("fresh").map(arg_bool_value).unwrap_or(false);
            params.raw = map.get("raw").map(arg_bool_value).unwrap_or(false);
        }
        Ok(params)
    }

    pub fn from_expand_many_item(item: &Value) -> Result<Self, String> {
        if let Some(ref_id) = item.as_str() {
            return Ok(Self {
                ref_id: ref_id.to_string(),
                ..Default::default()
            });
        }
        let map = item
            .as_object()
            .ok_or_else(|| "expandMany item must be a tz:// ref string or object".to_string())?;
        let ref_id = map
            .get("ref")
            .and_then(Value::as_str)
            .ok_or_else(|| "expandMany item object requires ref".to_string())?
            .to_string();
        Ok(Self {
            ref_id,
            selector: map
                .get("selector")
                .and_then(Value::as_str)
                .map(str::to_string),
            start_line: map
                .get("start_line")
                .and_then(coerce_u64)
                .and_then(|n| usize::try_from(n).ok()),
            end_line: map
                .get("end_line")
                .and_then(coerce_u64)
                .and_then(|n| usize::try_from(n).ok()),
            anchor_kind: map
                .get("anchor_kind")
                .and_then(Value::as_str)
                .map(str::to_string),
            symbol: map
                .get("symbol")
                .and_then(Value::as_str)
                .map(str::to_string),
            since: map.get("since").and_then(Value::as_str).map(str::to_string),
            fresh: map.get("fresh").map(arg_bool_value).unwrap_or(false),
            raw: map.get("raw").map(arg_bool_value).unwrap_or(false),
        })
    }
}

fn value_shape(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
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
        Value::String(text) => matches!(
            text.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes"
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn from_codemode_args_accepts_positional_ref_string() {
        let params = ExpandParams::from_codemode_args(&[json!("tz://blob/abc")]).unwrap();
        assert_eq!(params.ref_id, "tz://blob/abc");
        assert!(!params.fresh);
    }

    #[test]
    fn from_codemode_args_coerces_ref_object() {
        let params = ExpandParams::from_codemode_args(&[json!({
            "ref": "tz://blob/obj",
            "start_line": 2,
            "end_line": 4,
            "fresh": true,
        })])
        .unwrap();
        assert_eq!(params.ref_id, "tz://blob/obj");
        assert_eq!(params.start_line, Some(2));
        assert_eq!(params.end_line, Some(4));
        assert!(params.fresh);
    }

    #[test]
    fn from_codemode_args_rejects_object_plus_trailing_opts() {
        let err = ExpandParams::from_codemode_args(&[
            json!({"ref": "tz://blob/x"}),
            json!({"fresh": true}),
        ])
        .unwrap_err();
        assert!(
            err.contains("object form takes a single"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn from_codemode_args_typed_error_names_signature_on_bad_shape() {
        let err = ExpandParams::from_codemode_args(&[json!(42)]).unwrap_err();
        assert!(
            err.contains("requires a tz:// ref string") && err.contains("got number"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn from_codemode_args_object_missing_ref_is_typed() {
        let err = ExpandParams::from_codemode_args(&[json!({"path": "nope.txt"})]).unwrap_err();
        assert!(err.contains("requires ref"), "unexpected: {err}");
    }
}
