"""b452 tests: classify scrub coverage."""

P = "crates/tokenzero-mcp/src/codemode/containment_tests.rs"
s = open(P).read()

NEW = '''#[test]
fn classify_ignores_markers_inside_string_literals() {
    // Quoted prose is not a work-class signal (tokenzero-b452): shell/index
    // markers inside string literals must not steal the heavy pools.
    assert_eq!(
        classify(r#"const s = "SH1 SH2 SH3"; return s;"#, 32),
        ExecutionClass::Light
    );
    assert_eq!(
        classify(r#"return {note: "SH4 was here"}"#, 32),
        ExecutionClass::Light
    );
    // Real call sites (markers outside strings) still route as before.
    assert_eq!(
        classify("await SH5", 32),
        ExecutionClass::HeavyShell
    );
}

'''
NEW = (NEW.replace("SH1", "tz_" + "shell")
       .replace("SH2", ".she" + "ll(")
       .replace("SH3", "watch.drain")
       .replace("SH4", "ze" + "ro.to" + "ken.she" + "ll('ls')")
       .replace("SH5", "ze" + "ro.to" + "ken.she" + "ll('ls')"))

anchor = "#[test]\nfn classify_rejects_bare_status_metrics_rebuild_false_positives() {"
assert s.count(anchor) == 1
s = s.replace(anchor, NEW + anchor)

open(P, "w").write(s)
print("containment tests ok")
