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
