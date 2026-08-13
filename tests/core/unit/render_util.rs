use super::*;

#[test]
fn summarize_tokens_keeps_critical_lines_even_over_budget() {
    let mut lines: Vec<String> = (0..300).map(|idx| format!("noise line {idx}")).collect();
    lines[150] = "error[E0308]: mismatched types in src/lib.rs:42".to_string();
    lines[222] = "warning: unused import reported".to_string();
    let text = lines.join("\n");
    let summary = summarize_tokens(&text, 60, "");

    assert!(summary.contains("error[E0308]: mismatched types in src/lib.rs:42"));
    assert!(summary.contains("warning: unused import reported"));
    assert!(summary.contains("exact ref available"));
    assert!(count_tokens(&summary) < count_tokens(&text) / 4);
}

#[test]
fn repo_inventory_requires_inventory_only_segments() {
    assert!(is_repo_inventory_command("ls -la src"));
    assert!(is_repo_inventory_command(
        "find . -type f | sort | wc -l && find . -type f | sort"
    ));
    // "ls -" substring inside another word is not an inventory command.
    assert!(!is_repo_inventory_command("tools -v"));
    // A non-lister segment means its output would be swallowed as paths.
    assert!(!is_repo_inventory_command(
        "ls -d .graphzero; graphzero index ."
    ));
    assert!(!is_repo_inventory_command("ls src && cargo build"));
}

#[test]
fn mixed_multi_command_never_takes_search_view() {
    // A non-search segment's output must not be labeled as search matches.
    assert!(!is_search_shell_command(
        "grep -rn foo src/; ls crates/"
    ));
    assert!(!is_search_shell_command(
        "ls crates && grep -rln foo crates"
    ));
    assert!(!is_search_shell_command(
        "grep foo file | xargs rm"
    ));
    // Pure search plus line filters keeps the search view.
    assert!(is_search_shell_command(
        "grep -rn foo src/ | head -20"
    ));
    assert!(is_search_shell_command(
        "rg foo | sort | uniq -c | tail -5"
    ));
    assert!(is_search_shell_command(
        "grep foo a.txt"
    ));
    // Filters alone are not a search.
    assert!(!is_search_shell_command(
        "head -5 a.txt"
    ));

    let structured = structured_shell_view(
        "grep -rn foo src/; ls crates/",
        "src/a.rs:1:foo\ncrate-a\ncrate-b",
        "",
    );
    assert!(
        !structured.starts_with("search_summary:"),
        "mixed command must not render as search matches: {structured}"
    );

    let family = shell_family(
        "grep -rn foo src/; ls crates/",
        "src/a.rs:1:foo\ncrate-a",
        "",
    );
    assert_ne!(family, "search");
    assert_eq!(
        shell_family("grep -rn foo src/", "src/a.rs:1:foo", ""),
        "search"
    );
}

#[test]
fn critical_lines_marks_every_gap_instead_of_silently_dropping() {
    let mut lines: Vec<String> = (0..180).map(|idx| format!("pattern-{idx}")).collect();
    lines[82] = "*.actual".to_string();
    let text = lines.join("\n");
    let view = critical_lines(&text, 3);

    assert!(view.contains("*.actual"));
    assert!(
        view.contains("... omitted 79 lines; exact ref available ..."),
        "leading gap must be marked: {view}"
    );
    assert!(
        view.contains("... omitted 94 lines; exact ref available ..."),
        "trailing gap must be marked: {view}"
    );
    assert_eq!(view.lines().count(), 9, "7 kept + 2 markers: {view}");
}

#[test]
fn critical_lines_interior_gap_and_no_marker_when_nothing_elided() {
    let text = "error: one\nnoise\nnoise\nnoise\nerror: two";
    let full = critical_lines(text, 3);
    assert_eq!(full, text, "all lines kept must be marker-free");

    let mut lines: Vec<String> = (0..20).map(|idx| format!("n{idx}")).collect();
    lines[0] = "error: head".to_string();
    lines[19] = "error: tail".to_string();
    let gapped = critical_lines(&lines.join("\n"), 1);
    assert!(gapped.contains("... omitted 16 lines; exact ref available ..."));

    assert_eq!(critical_lines("just noise\nmore noise", 3), "");
}

#[test]
fn error_block_marks_gaps_like_critical_lines() {
    let mut lines: Vec<String> = (0..30).map(|idx| format!("n{idx}")).collect();
    lines[15] = "assertion failed: left == right".to_string();
    let view = error_block(&lines.join("\n"), 2);
    assert!(view.contains("assertion failed"));
    assert!(view.contains("... omitted 13 lines; exact ref available ..."));
    assert!(view.contains("... omitted 12 lines; exact ref available ..."));
}

#[test]
fn diagnostic_shell_view_never_silently_elides() {
    let mut lines: Vec<String> = (0..181)
        .map(|idx| format!("ignore-pattern-{idx}"))
        .collect();
    lines[82] = "*.actual".to_string();
    let view = diagnostic_shell_view(&lines.join("\n"), "", 700);
    assert!(view.contains("*.actual"));
    assert!(
        view.contains("omitted") && view.contains("exact ref available"),
        "elision must be visible: {view}"
    );
}

#[test]
fn structural_dedupe_collapses_digit_varying_runs_but_not_criticals() {
    let text = (0..40)
        .map(|idx| format!("Receiving chunk {idx} of 40 (eta {idx}s)"))
        .chain([
            "error: socket reset".to_string(),
            "error: socket reset".to_string(),
        ])
        .collect::<Vec<_>>()
        .join("\n");
    let deduped = dedupe_lines_impl(&text, 6, true);

    assert!(deduped.contains("similar lines collapsed"), "{deduped}");
    assert_eq!(
        deduped.matches("error: socket reset").count(),
        2,
        "critical lines must never collapse: {deduped}"
    );
    assert!(deduped.lines().count() < 10, "{deduped}");
}

#[test]
fn classifier_covers_required_diagnostic_families() {
    let cases = [
        ("cargo test", "test", "diagnostic"),
        ("cargo build", "build", "diagnostic"),
        ("cargo check", "build", "diagnostic"),
        ("cargo clippy", "build", "diagnostic"),
        ("pytest", "python-test", "diagnostic"),
        ("python -m unittest", "python-test", "diagnostic"),
        ("npm test", "test", "diagnostic"),
        ("vitest", "test", "diagnostic"),
        ("jest", "test", "diagnostic"),
        ("eslint src", "lint", "diagnostic"),
        ("tsc --noEmit", "lint", "diagnostic"),
        ("ruff check", "lint", "diagnostic"),
        ("mypy pkg", "lint", "diagnostic"),
        ("go test ./...", "go-test", "diagnostic"),
    ];
    for (command, family, policy) in cases {
        let decision = decide_shell_policy(command, "", "error: failed", Some(1), Mode::Auto);
        assert_eq!(decision.family, family, "{command}");
        assert_eq!(decision.policy, policy, "{command}");
    }
}

#[test]
fn diff_renderer_keeps_patch_evidence() {
    let diff = "diff --git a/a b/a\n@@ -1 +1 @@\n-old\n+new\n";
    assert!(diff_summary(diff, 20).contains("@@ -1 +1 @@"));
    assert!(diff_summary(diff, 20).contains("+new"));
    // Empty diff falls back to summarize_lines.
    let fallback = diff_summary("no diff content here", 20);
    assert!(fallback.contains("no diff content here"));
}

#[test]
fn secret_masking_covers_key_prefix_and_known_tokens() {
    let masked = mask_visible_secrets("token=abc123\nAuthorization sk-proj-secret");
    assert!(masked.contains("token=[masked]"));
    assert!(!masked.contains("abc123"));
    assert!(!masked.contains("sk-proj-secret"));
    // ghp_ and AKIA are masked at the word level when no =key short-circuits.
    let word_level = mask_visible_secrets("ghp_abc and AKIA123 are secrets");
    assert!(!word_level.contains("ghp_abc"), "{}", word_level);
    assert!(!word_level.contains("AKIA123"), "{}", word_level);
    // password= key short-circuits (stops scanning after the match).
    let key_match = mask_visible_secrets("ghp_abc password=x api_key=z");
    assert!(key_match.contains("password=[masked]"), "{}", key_match);
    // Each =key works on its own line.
    let secrets = mask_visible_secrets("secret=y");
    assert!(secrets.contains("secret=[masked]"), "{}", secrets);
    let api = mask_visible_secrets("api_key=z");
    assert!(api.contains("api_key=[masked]"), "{}", api);
}

#[test]
fn secret_masking_covers_authorization_and_bearer_markers() {
    // P20-F1 / tokenzero-g3y.20: SECRET_MARKERS classifies these but masking
    // previously left the credential payload visible.
    let auth = mask_visible_secrets("authorization: Bearer abcdefghijklmnop");
    assert!(auth.contains("authorization:[masked]"), "{auth}");
    assert!(!auth.contains("abcdefghijklmnop"), "{auth}");
    assert!(!auth.contains("Bearer"), "{auth}");
    let bearer = mask_visible_secrets("bearer abcdefghijklmnop");
    assert!(bearer.contains("bearer [masked]"), "{bearer}");
    assert!(!bearer.contains("abcdefghijklmnop"), "{bearer}");
    let mixed_case = mask_visible_secrets("Authorization: Bearer xyz-secret-value");
    assert!(
        mixed_case.starts_with("Authorization:[masked]"),
        "{mixed_case}"
    );
    assert!(!mixed_case.contains("xyz-secret-value"), "{mixed_case}");
}
