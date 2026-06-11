pub fn shell_display_command_from_argv(argv: &[String]) -> String {
    shell_display_command_from_argv_for_platform(argv, "posix")
}

pub fn shell_display_command_from_argv_for_platform(argv: &[String], platform: &str) -> String {
    let style = shell_display_quote_style(platform);
    argv.iter()
        .map(|arg| shell_display_arg(arg, style))
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Copy)]
pub(crate) enum ShellDisplayQuoteStyle {
    Posix,
    Cmd,
    PowerShell,
}

pub(crate) fn shell_display_quote_style(platform: &str) -> ShellDisplayQuoteStyle {
    match platform {
        "cmd" | "windows" => ShellDisplayQuoteStyle::Cmd,
        "powershell" | "pwsh" => ShellDisplayQuoteStyle::PowerShell,
        _ => ShellDisplayQuoteStyle::Posix,
    }
}

pub(crate) fn shell_display_arg(arg: &str, style: ShellDisplayQuoteStyle) -> String {
    if arg.is_empty() {
        return match style {
            ShellDisplayQuoteStyle::Cmd => "\"\"".to_string(),
            ShellDisplayQuoteStyle::Posix | ShellDisplayQuoteStyle::PowerShell => "''".to_string(),
        };
    }
    if arg.chars().all(|ch| is_shell_display_safe_char(ch, style)) {
        return arg.to_string();
    }
    match style {
        ShellDisplayQuoteStyle::Posix => format!("'{}'", arg.replace('\'', "'\\''")),
        ShellDisplayQuoteStyle::Cmd => format!("\"{}\"", arg.replace('"', "\"\"")),
        ShellDisplayQuoteStyle::PowerShell => format!("'{}'", arg.replace('\'', "''")),
    }
}

pub(crate) fn is_shell_display_safe_char(ch: char, style: ShellDisplayQuoteStyle) -> bool {
    ch.is_ascii_alphanumeric()
        || matches!(ch, '-' | '_' | '.' | '/' | ':' | ',' | '=' | '@')
        || matches!(
            (style, ch),
            (ShellDisplayQuoteStyle::Posix, '%')
                | (ShellDisplayQuoteStyle::Cmd, '\\')
                | (ShellDisplayQuoteStyle::PowerShell, '\\')
        )
}
