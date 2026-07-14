use super::*;

#[test]
fn codemode_not_exposed_as_mcp_tool() {
    let list =
        crate::catalog::tool_specs_for_filter(None, true, tokenzero_core::McpToolSurface::Classic);
    let names: Vec<_> = list.iter().map(|tool| tool.name.as_str()).collect();
    assert!(!names.contains(&"tz_codemode"), "tz_codemode must not appear in MCP tool list");
    assert!(names.contains(&"tz_read"));
    assert!(names.contains(&"tz_expand"));
}

#[test]
fn codemode_call_returns_unknown_tool() {
    let dir = tempfile::tempdir().unwrap();
    let engine = crate::TokenZeroEngine::new(crate::EngineConfig::for_root(dir.path()));
    assert!(
        call_tool(
            &engine,
            "tz_codemode",
            &json!({"plan": "search:read"}),
            None
        )
        .is_err()
    );
    assert!(call_tool(&engine, "codemode", &json!({}), None).is_err());
}

#[test]
fn batch_combines_ops_with_sections_and_unioned_refs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "alpha contents\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "beta needle\n").unwrap();
    let engine = crate::TokenZeroEngine::new(crate::EngineConfig::for_root(dir.path()));
    let args = json!({
        "ops": [
            {"tool": "read", "args": {"path": dir.path().join("a.txt").display().to_string()}},
            {"tool": "grep", "args": {"query": "needle", "path": dir.path().display().to_string()}},
            {"tool": "batch", "args": {}},
            {"tool": "no_such_tool"}
        ]
    });

    let result = call_tool(&engine, "batch", &args, None).unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("## 1 read"), "{text}");
    assert!(text.contains("alpha contents"), "{text}");
    assert!(text.contains("## 2 grep"), "{text}");
    assert!(text.contains("needle"), "{text}");
    assert!(text.contains("nested batch is not allowed"), "{text}");
    assert!(text.contains("## 4 no_such_tool — error:"), "{text}");
}

#[test]
fn batch_ops_accept_json_encoded_string_and_reject_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "alpha contents\n").unwrap();
    let engine = crate::TokenZeroEngine::new(crate::EngineConfig::for_root(dir.path()));
    let stringly = json!({
        "ops": serde_json::to_string(&json!([
            {"tool": "read", "args": {"path": dir.path().join("a.txt").display().to_string()}}
        ]))
        .unwrap()
    });

    let result = call_tool(&engine, "tz_batch", &stringly, None).unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("alpha contents"), "{text}");

    assert!(call_tool(&engine, "batch", &json!({"ops": []}), None).is_err());
    assert!(call_tool(&engine, "batch", &json!({}), None).is_err());
}

#[test]
fn arg_command_quotes_argv_metacharacters_for_display() {
    let args = json!({"argv": ["rg", "error|warning", "src/lib.rs"]});
    let (command, argv) = arg_command(&args).unwrap();

    // Display quoting is platform-styled: cmd has no single quotes.
    let expected = if cfg!(windows) {
        r#"rg "error|warning" src/lib.rs"#
    } else {
        "rg 'error|warning' src/lib.rs"
    };
    assert_eq!(command, expected);
    assert_eq!(
        argv.unwrap(),
        vec![
            "rg".to_string(),
            "error|warning".to_string(),
            "src/lib.rs".to_string()
        ]
    );

    let shell_script = json!(["false | true"]);
    let (command, argv) = arg_command(&shell_script).unwrap();
    assert_eq!(command, "false | true");
    assert_eq!(argv.unwrap(), vec!["false | true".to_string()]);
}

#[test]
fn stub_schema_string_args_coerce() {
    let args = json!({
        "raw": "true",
        "no_rewrite": "1",
        "start_line": "420",
        "end_line": " 440 ",
        "timeout_seconds": "90"
    });
    assert!(arg_bool(&args, "raw"));
    assert!(arg_bool(&args, "no_rewrite"));
    assert!(!arg_bool(&args, "missing"));
    assert_eq!(arg_u64(&args, "start_line"), Some(420));
    assert_eq!(arg_u64(&args, "end_line"), Some(440));
    assert!(arg_timeout_any(&args, &["timeout_seconds"]).is_some());

    let typed = json!({"raw": true, "start_line": 420});
    assert!(arg_bool(&typed, "raw"));
    assert_eq!(arg_u64(&typed, "start_line"), Some(420));

    let junk = json!({"raw": "yep", "start_line": "x40"});
    assert!(!arg_bool(&junk, "raw"));
    assert_eq!(arg_u64(&junk, "start_line"), None);
}

#[test]
fn path_list_accepts_json_encoded_array_string() {
    let args = json!({"path": "[\"/a/one.rs\", \"/b/two.rs\"]"});
    let paths = arg_path_list(&args, "path").unwrap();
    assert_eq!(
        paths,
        vec![PathBuf::from("/a/one.rs"), PathBuf::from("/b/two.rs")]
    );

    let plain = json!({"path": "/a/one.rs"});
    assert_eq!(
        arg_path_list(&plain, "path").unwrap(),
        vec![PathBuf::from("/a/one.rs")]
    );

    let empty = json!({"path": "[]"});
    assert!(arg_path_list(&empty, "path").is_err());
}

#[test]
fn edit_hunks_accept_json_encoded_string_and_string_booleans() {
    let stub = json!({
        "edits": "[{\"find\": \"a\", \"replace\": \"b\", \"replace_all\": \"true\"}]"
    });
    let hunks = arg_edit_hunks(&stub).unwrap();
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].find, "a");
    assert_eq!(hunks[0].replace, "b");
    assert!(hunks[0].replace_all);

    let typed = json!({"edits": [{"find": "a", "replace": "b"}]});
    let hunks = arg_edit_hunks(&typed).unwrap();
    assert!(!hunks[0].replace_all);

    assert!(arg_edit_hunks(&json!({})).is_err());
    assert!(arg_edit_hunks(&json!({"edits": []})).is_err());
    assert!(arg_edit_hunks(&json!({"edits": "[]"})).is_err());
    assert!(arg_edit_hunks(&json!({"edits": [{"find": "a"}]})).is_err());
}

#[test]
fn numeric_limits_do_not_truncate_to_usize() {
    let args = json!({"max_files": u64::MAX});

    if usize::try_from(u64::MAX).is_err() {
        assert_eq!(arg_u64(&args, "max_files"), None);
    } else {
        assert_eq!(arg_u64(&args, "max_files"), Some(usize::MAX));
    }
}

#[test]
fn windows_arg_display_quotes_findstr_metacharacters_for_cmd() {
    let argv = vec![
        "findstr".to_string(),
        "error|warning".to_string(),
        "src\\sample.txt".to_string(),
    ];

    assert_eq!(
        display_command_for_argv_on_platform(&argv, "windows"),
        "findstr \"error|warning\" src\\sample.txt"
    );
    assert_eq!(
        display_command_for_argv_on_platform(&argv, "powershell"),
        "findstr 'error|warning' src\\sample.txt"
    );

    let shell_builtin = vec![
        "echo".to_string(),
        "ok".to_string(),
        "|".to_string(),
        "findstr".to_string(),
        "ok".to_string(),
    ];
    assert_eq!(
        display_command_for_argv_on_platform(&shell_builtin, "windows"),
        "echo ok | findstr ok"
    );
}
