use super::*;
use serde_json::json;
use std::path::Path;

fn root() -> &'static Path {
    Path::new("/workspace/repo")
}

fn key(op: &str, args: &Value, model: Option<&str>, class: Option<ConsistencyClass>) -> String {
    action_cache_key(ActionCacheKeyInput {
        op,
        args,
        store_root: root(),
        model_id: model,
        consistency_class: class,
    })
}

#[test]
fn tzib5y_same_logical_op_is_byte_identical_across_surfaces() {
    let mcp = json!({"path": "/workspace/repo/src/lib.rs"});
    let cli = json!({"path": "src/lib.rs"});
    let codemode = json!({"path": "./src/lib.rs", "fresh": false, "raw": false, "mode": "auto"});
    let raw_worker = json!({"path": "src/../src/lib.rs"});

    let mcp_key = key("tz_read", &mcp, None, None);
    let cli_key = key("read", &cli, None, None);
    let cm_key = key("zero.token.read", &codemode, None, None);
    let raw_key = key("read", &raw_worker, None, None);

    assert_eq!(mcp_key, cli_key);
    assert_eq!(cli_key, cm_key);
    assert_eq!(cm_key, raw_key);
    assert_eq!(mcp_key.len(), 64);
}

#[test]
fn tzib5y_defaults_to_must_block_revalidate_and_keys_model_id() {
    let args = json!({"path": "src/lib.rs"});
    let omitted = key("read", &args, None, None);
    let explicit = key(
        "read",
        &args,
        None,
        Some(ConsistencyClass::MustBlockRevalidate),
    );
    assert_eq!(omitted, explicit);

    let swr = key("read", &args, None, Some(ConsistencyClass::Swr));
    assert_ne!(omitted, swr);

    let with_model = key("read", &args, Some("gpt-4o"), None);
    assert_ne!(omitted, with_model);
    assert_eq!(
        with_model,
        key("read", &args, Some(" gpt-4o "), None),
        "model_id is trimmed"
    );
}

#[test]
fn tzib5y_envelope_is_hub_canonical_json() {
    let args = json!({"mode": "auto", "path": "a.rs", "fresh": false});
    let envelope = action_cache_envelope(ActionCacheKeyInput {
        op: "read",
        args: &args,
        store_root: root(),
        model_id: None,
        consistency_class: None,
    });
    let encoded = canonical_json(&envelope);
    assert!(encoded.contains("\"consistency_class\":\"must_block_revalidate\""));
    assert!(encoded.contains("\"schema\":\"tokenzero.actioncache.key.v1\""));
    assert_eq!(
        action_cache_key(ActionCacheKeyInput {
            op: "read",
            args: &args,
            store_root: root(),
            model_id: None,
            consistency_class: None,
        }),
        sha256_hex(&encoded)
    );
}
