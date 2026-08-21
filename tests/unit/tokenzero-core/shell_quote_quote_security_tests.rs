
use super::*;

#[test]
fn quote_windows_cmd_doubles_percent_and_quotes() {
    assert_eq!(quote_windows_cmd("%PATH%"), "\"%%PATH%%\"");
    assert_eq!(quote_windows_cmd("foo\"bar"), "\"foo\"\"bar\"");
    assert_eq!(quote_windows_cmd("a&b"), "\"a&b\"");
}

#[test]
fn quote_posix_single_quotes_command_substitutions() {
    assert_eq!(quote_posix("$(id)"), "'$(id)'");
    assert_eq!(quote_posix("it's"), "'it'\"'\"'s'");
}
