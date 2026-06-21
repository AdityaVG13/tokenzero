use super::*;
use proptest::prelude::*;

use super::support::*;


#[test]
fn repeat_render_collapses_verified_unchanged_success() {
    let stdout = (0..40)
        .map(|idx| format!("processing item {idx} done"))
        .collect::<Vec<_>>()
        .join("\n");
    let input = success_input("./generate.sh --all", &stdout, "");
    let raw = count_tokens(&shell_combined_output(
        "./generate.sh --all",
        Some(0),
        &stdout,
        "",
    ));

    let rendered = render_shell_repeat(input, 3);
    assert_eq!(rendered.output_strategy, "repeat_unchanged_shell");
    assert!(
        rendered.visible.contains("unchanged; run 3"),
        "{}",
        rendered.visible
    );
    assert!(
        rendered
            .visible
            .contains("combined_ref: tz://blob/combined"),
        "{}",
        rendered.visible
    );
    assert!(!rendered.visible.contains("processing item"));
    assert!(rendered.command_status.command_success);
    let visible_tokens = count_tokens(&rendered.visible);
    assert!(
        visible_tokens * 5 < raw,
        "visible={visible_tokens} raw={raw}\n{}",
        rendered.visible
    );
}

#[test]
fn repeat_render_never_compacts_failures_first_runs_or_explicit_modes() {
    let stdout = "step one\nstep two\n";

    let mut failed = success_input("cargo test", stdout, "error[E0308]: mismatched types");
    failed.exit_code = Some(101);
    let rendered = render_shell_repeat(failed.clone(), 5);
    assert_ne!(rendered.output_strategy, "repeat_unchanged_shell");
    assert_eq!(rendered, render_shell(failed));

    let first_run = success_input("cargo test", stdout, "");
    let rendered = render_shell_repeat(first_run.clone(), 1);
    assert_eq!(rendered, render_shell(first_run));

    let mut explicit = success_input("cargo test", stdout, "");
    explicit.mode = Mode::Passthrough;
    let rendered = render_shell_repeat(explicit.clone(), 4);
    assert_eq!(rendered, render_shell(explicit));

    let mut no_ref = success_input("cargo test", stdout, "");
    no_ref.combined_ref = None;
    let rendered = render_shell_repeat(no_ref.clone(), 4);
    assert_eq!(rendered, render_shell(no_ref));
}

#[test]
fn repeat_render_keeps_adaptive_floor_for_tiny_outputs() {
    let input = success_input("true", "", "");
    let raw = count_tokens(&shell_combined_output("true", Some(0), "", ""));
    let rendered = render_shell_repeat(input, 9);
    assert!(
        count_tokens(&rendered.visible) <= raw,
        "visible must never exceed raw: {} vs {raw}\n{}",
        count_tokens(&rendered.visible),
        rendered.visible
    );
}
