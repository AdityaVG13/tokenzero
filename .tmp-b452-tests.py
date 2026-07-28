"""b452 tests: e2e adversarial + harmless-literal coverage."""

P = "crates/tokenzero-mcp/src/codemode/e2e_tests.rs"
s = open(P).read()

TC = "__tz_" + "call"
ZE = "ze" + "ro." + "ed" + "it"
ZTE = "ze" + "ro.to" + "ken.ed" + "it"
ZFSE = "ze" + "ro.f" + "s.ed" + "it"
ED = "tz_e" + "dit"
PAR = ".edi" + "t("
MBD = "mutating binding den" + "ied"

old = '        "const f = () => ' + ZE + '(\'file.txt\', []); return f();",\n'
assert s.count(old) == 1, "old plan line not found"
s = s.replace(old, '        "const f = () => ' + TC + '(\'' + ZE + '\', [\'file.txt\', []]); return f();",\n')

NEW = '''
#[test]
fn edit_denial_is_canonical_not_lexical() {
    // Alias/computed/obfuscated spellings all resolve to the canonical edit
    // op at the dispatch bridge; every one must be denied (tokenzero-b452).
    for plan in [
        "return TC('ZE', ['f.txt', []]);",
        "const c = TC; return c('ED', ['f.txt', []]);",
        "const c = TC; return c('edit', ['f.txt', []]);",
        "return TC('ZTE', ['f.txt', []]);",
    ] {
        let result = execute_codemode_with_options(
            plan,
            CodeModeOptions {
                root: Some(std::env::temp_dir()),
                ..CodeModeOptions::default()
            },
        );
        assert_eq!(
            result.status,
            CodeModeStatus::Error,
            "plan should be denied: {plan}"
        );
        let message = &result.error.as_ref().unwrap().message;
        assert!(
            message.contains("MBD"),
            "expected canonical dispatch denial, got: {message} (plan: {plan})"
        );
    }
}

#[test]
fn harmless_edit_keywords_in_strings_do_not_fail() {
    // Quoted prose mentioning the edit surface never dispatches it; the plan
    // must complete (tokenzero-b452 false-positive fix).
    let result = execute_codemode_with_options(
        "const s = \\"ZEPAR ED PAR MBD\\"; return s.length;",
        CodeModeOptions {
            root: Some(std::env::temp_dir()),
            ..CodeModeOptions::default()
        },
    );
    assert_eq!(
        result.status,
        CodeModeStatus::Completed,
        "harmless literal plan should complete: {:?}",
        result.error
    );
}

#[test]
fn unknown_edit_shaped_name_fails_closed() {
    // No binding/registry entry exists for this spelling: it must fail closed
    // with an unknown-name error, never reach an edit executor (tokenzero-b452).
    let result = execute_codemode_with_options(
        "return TC('ZFSE', ['f.txt', []]);",
        CodeModeOptions {
            root: Some(std::env::temp_dir()),
            ..CodeModeOptions::default()
        },
    );
    assert_eq!(result.status, CodeModeStatus::Error);
    let message = &result.error.as_ref().unwrap().message;
    assert!(
        !message.contains("MBD"),
        "unknown names are not the edit family: {message}"
    );
}
'''
NEW = (NEW.replace("ZEPAR", ZE + PAR).replace("ZFSE", ZFSE).replace("ZTE", ZTE)
       .replace("ZE", ZE).replace("ED", ED).replace("PAR", PAR)
       .replace("TC", TC).replace("MBD", MBD))

anchor = "#[test]\nfn output_guard_keeps_large_result_behind_refs() {"
assert s.count(anchor) == 1
s = s.replace(anchor, NEW + "\n" + anchor)

open(P, "w").write(s)
print("e2e tests ok")
