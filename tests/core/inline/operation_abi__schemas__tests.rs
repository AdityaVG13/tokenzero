use super::*;

#[test]
fn shell_rewrite_description_declares_explicit_argv_authority() {
    let schema = shell_schema();
    let description = schema["properties"]["rewrite"]["description"]
        .as_str()
        .expect("shell rewrite description");

    assert!(description.contains("when `argv` is omitted"));
    assert!(description.contains("Explicit `argv` is authoritative"));
}
