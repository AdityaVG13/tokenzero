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
    let deduped = dedupe_lines_structural(&text, 6);

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
fn diff_structured_and_dedupe_renderers_keep_critical_evidence() {
    let diff = "diff --git a/a b/a\n@@ -1 +1 @@\n-old\n+new\n";
    assert!(diff_summary(diff, 20).contains("@@ -1 +1 @@"));
    assert!(diff_summary(diff, 20).contains("+new"));

    let json = r#"[{"name":"api","status":"ok"},{"name":"db","status":"failed"}]"#;
    let structured = structured_shell_view("docker ps --format json", json, "");
    assert!(structured.contains("abnormal"));
    assert!(structured.contains("failed"));

    let repeated = dedupe_lines("tick\ntick\ntick\nerror: boom\n", 8);
    assert!(repeated.contains("repeated 2 more"));
    assert!(repeated.contains("error: boom"));
}

#[test]
fn repo_inventory_and_secret_masking_are_safe_visible_views() {
    let inventory = repo_inventory_view(
        "find . -type f | sort | wc -l && find . -type f | sort",
        "2\nsrc/lib.rs\nCargo.toml\n",
    );
    assert!(inventory.contains("repo_inventory"));
    assert!(inventory.contains("files_seen"));
    assert!(inventory.contains("src/lib.rs"));

    let masked = mask_visible_secrets("token=abc123\nAuthorization sk-proj-secret");
    assert!(masked.contains("token=[masked]"));
    assert!(!masked.contains("abc123"));
    assert!(!masked.contains("sk-proj-secret"));
}
