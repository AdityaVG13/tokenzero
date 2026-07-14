//! Platform shell quoting, command-string splitting, and syntax detection.
//! Canonical home for helpers previously duplicated at the runtime boundary.

use std::path::Path;

/// Host platform tag used by shell split/quote helpers (`windows` / `posix`).
pub fn host_shell_platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else {
        "posix"
    }
}

pub fn split_command_string(command: &str) -> Vec<String> {
    split_command_string_for_platform(command, host_shell_platform())
}

pub fn split_command_string_for_platform(command: &str, platform: &str) -> Vec<String> {
    let preserve_backslashes = matches!(platform, "windows" | "cmd" | "powershell" | "pwsh");
    let single_quote_groups = single_quote_groups_for_platform(command, platform);
    let doubled_quote_escape = doubled_quote_escape_for_platform(command, platform);
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut token_started = false;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if escaped {
            current.push(ch);
            escaped = false;
            token_started = true;
            continue;
        }
        if ch == '\\' && quote != Some('\'') && !preserve_backslashes {
            // POSIX: inside double quotes a backslash is literal unless it
            // precedes $, `, ", or \ — so "a\|b" must stay a\|b (BRE
            // alternation), not collapse to a|b.
            if quote == Some('"') && !matches!(chars.peek().copied(), Some('$' | '`' | '"' | '\\'))
            {
                current.push('\\');
                token_started = true;
                continue;
            }
            escaped = true;
            token_started = true;
            continue;
        }
        if Some(ch) == quote
            && doubled_quote_escape == Some(ch)
            && chars.peek().copied() == Some(ch)
        {
            current.push(ch);
            let _ = chars.next();
            token_started = true;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            token_started = true;
            continue;
        }
        if quote.is_none() && (ch == '"' || ch == '\'' && single_quote_groups) {
            quote = Some(ch);
            token_started = true;
            continue;
        }
        if quote.is_none() && ch.is_whitespace() {
            if token_started {
                out.push(std::mem::take(&mut current));
                token_started = false;
            }
            continue;
        }
        current.push(ch);
        token_started = true;
    }
    if escaped {
        current.push('\\');
    }
    if token_started {
        out.push(current);
    }
    out
}

fn doubled_quote_escape_for_platform(command: &str, platform: &str) -> Option<char> {
    match platform {
        "cmd" => Some('"'),
        "powershell" | "pwsh" => Some('\''),
        "windows" => {
            if first_windows_cmd_word(command)
                .as_deref()
                .is_some_and(is_powershell_shell_host)
            {
                Some('\'')
            } else {
                Some('"')
            }
        }
        _ => None,
    }
}

pub fn contains_shell_syntax(value: &str) -> bool {
    contains_shell_syntax_with_single_quotes(value, true)
}

fn contains_shell_syntax_with_single_quotes(value: &str, single_quote_groups: bool) -> bool {
    if starts_with_posix_env_assignment(value) {
        return true;
    }
    let (mut quote, mut escaped, mut at_word_start) = (None, false, true);
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if escaped {
            escaped = false;
            at_word_start = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            at_word_start = false;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            at_word_start = false;
            continue;
        }
        if quote.is_none() && (ch == '"' || ch == '\'' && single_quote_groups) {
            quote = Some(ch);
            at_word_start = false;
            continue;
        }
        let next = chars.peek().copied();
        if quote != Some('\'') && ch == '$'
            && next.is_some_and(|next| matches!(next, '(' | '{' | '_') || next.is_ascii_alphabetic())
        {
            return true;
        }
        if quote.is_none() {
            if matches!(ch, '|' | ';' | '>' | '<' | '`' | '\n') || ch == '&' && next == Some('&') {
                return true;
            }
            if ch == '~' && at_word_start
                && next.is_none_or(|next| next == '/' || next.is_whitespace() || next.is_ascii_alphanumeric())
            {
                return true;
            }
            at_word_start = ch.is_whitespace();
        } else {
            at_word_start = false;
        }
    }
    false
}

fn single_quote_groups_for_platform(value: &str, platform: &str) -> bool {
    match platform {
        "cmd" => false,
        "windows" => first_windows_cmd_word(value)
            .as_deref()
            .is_some_and(is_powershell_shell_host),
        "powershell" | "pwsh" => true,
        _ => true,
    }
}

fn first_windows_cmd_word(value: &str) -> Option<String> {
    let mut quote = false;
    let mut word = String::new();
    for ch in value.chars() {
        if ch == '"' {
            quote = !quote;
            continue;
        }
        if !quote && ch.is_whitespace() {
            break;
        }
        word.push(ch);
    }
    if word.is_empty() { None } else { Some(word) }
}

fn starts_with_posix_env_assignment(value: &str) -> bool {
    let words = split_command_string(value);
    words.len() > 1
        && words
            .first()
            .is_some_and(|word| is_posix_env_assignment(word))
}

fn is_posix_env_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

pub fn contains_platform_shell_syntax(value: &str, platform: &str) -> bool {
    contains_shell_syntax_with_single_quotes(
        value,
        single_quote_groups_for_platform(value, platform),
    ) || matches!(platform, "windows" | "powershell" | "pwsh")
        && looks_like_powershell_syntax(value)
}

pub fn looks_like_powershell_syntax(value: &str) -> bool {
    if first_unquoted_word(value).is_some_and(|word| is_powershell_command_word(&word)) {
        return true;
    }
    let (mut quote, mut escaped) = (None, false);
    let type_tail = value.rfind("]::");
    let mut chars = value.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '`' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            continue;
        }
        if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
            continue;
        }
        if quote != Some('\'') {
            if ch == '$' && chars.peek().is_some_and(|(_, next)| is_powershell_variable_start(*next)) {
                return true;
            }
            if ch == '[' && type_tail.is_some_and(|tail| index < tail) {
                return true;
            }
        }
    }
    false
}

fn first_unquoted_word(value: &str) -> Option<String> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut word = String::new();
    for ch in value.chars() {
        if escaped {
            if quote.is_none() || quote == Some('"') {
                word.push(ch);
            }
            escaped = false;
            continue;
        }
        if ch == '`' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            continue;
        }
        if quote.is_none() && (ch == '\'' || ch == '"') {
            quote = Some(ch);
            continue;
        }
        if quote.is_none() && ch.is_whitespace() {
            break;
        }
        word.push(ch);
    }
    if word.is_empty() { None } else { Some(word) }
}

fn is_powershell_variable_start(ch: char) -> bool {
    ch == '{' || ch == '_' || ch == '?' || ch.is_ascii_alphabetic()
}

pub fn is_windows_shell_host(value: &str) -> bool {
    is_powershell_shell_host(value) || windows_shell_host_stem(value) == "cmd"
}

pub fn is_powershell_shell_host(value: &str) -> bool {
    matches!(
        windows_shell_host_stem(value).as_str(),
        "powershell" | "pwsh"
    )
}

pub fn windows_shell_host_stem(value: &str) -> String {
    let leaf = value.rsplit(['\\', '/']).next().unwrap_or(value);
    Path::new(leaf)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(leaf)
        .to_ascii_lowercase()
}

const POWERSHELL_KEYWORDS: &[&str] = &[
    "foreach", "where", "if", "else", "elseif", "for", "while", "try", "catch", "finally",
    "param", "function",
];
const POWERSHELL_VERBS: &[&str] = &[
    "add", "clear", "convertfrom", "convertto", "copy", "export", "foreach", "format", "get",
    "import", "invoke", "join", "move", "new", "out", "pop", "push", "remove", "resolve",
    "select", "set", "sort", "split", "start", "stop", "tee", "test", "where", "write",
];

fn is_powershell_command_word(word: &str) -> bool {
    let Some((verb, noun)) = word.split_once('-') else {
        let lower = word.to_ascii_lowercase();
        return POWERSHELL_KEYWORDS.contains(&lower.as_str());
    };
    if noun.is_empty() {
        return false;
    }
    let verb = verb.to_ascii_lowercase();
    POWERSHELL_VERBS.contains(&verb.as_str())
}

pub fn argv_has_shell_operator_tokens(argv: &[String]) -> bool {
    argv.iter().any(|arg| is_shell_operator_token(arg))
}

pub fn is_shell_operator_token(arg: &str) -> bool {
    matches!(
        arg,
        "|" | "||" | "&&" | ";" | ">" | ">>" | "<" | "<<" | "2>" | "2>>" | "&>"
    )
}

const WINDOWS_SHELL_BUILTINS: &[&str] = &[
    "assoc", "break", "call", "cd", "chdir", "cls", "color", "copy", "date", "del", "dir",
    "echo", "erase", "exit", "for", "ftype", "if", "md", "mkdir", "mklink", "move", "path",
    "pause", "popd", "prompt", "pushd", "rd", "rem", "ren", "rename", "rmdir", "set", "shift",
    "start", "time", "title", "type", "ver", "verify", "vol",
];

pub fn is_windows_shell_builtin(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    WINDOWS_SHELL_BUILTINS.contains(&lower.as_str())
}

fn is_unquoted_safe(value: &str, extra: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

pub fn quote_posix(value: &str) -> String {
    if value.is_empty() {
        "''".to_string()
    } else if is_unquoted_safe(value, "-_./:@%+=") {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

pub fn quote_windows_cmd(value: &str) -> String {
    if value.is_empty() {
        "\"\"".to_string()
    } else if is_unquoted_safe(value, "-_./:\\@+=") {
        value.to_string()
    } else {
        let mut quoted = String::with_capacity(value.len() + 2);
        quoted.push('"');
        for ch in value.chars() {
            match ch {
                '"' => quoted.push_str("\\\""),
                '%' => quoted.push_str("%%"),
                '^' => quoted.push_str("^^"),
                _ => quoted.push(ch),
            }
        }
        quoted.push('"');
        quoted
    }
}

pub fn quote_powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub fn quote_for(platform: &str, args: &[String]) -> String {
    args.iter()
        .map(|arg| match platform {
            "windows" | "cmd" => quote_windows_cmd(arg),
            "powershell" | "pwsh" => quote_powershell(arg),
            _ => quote_posix(arg),
        })
        .collect::<Vec<_>>()
        .join(" ")
}
