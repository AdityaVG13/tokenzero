use super::*;

use super::support::*;

#[test]
fn cargo_cold_build_success_collapses_compile_noise_into_counts() {
    let stderr = (0..50)
        .map(|idx| format!("   Compiling crate{idx} v0.{idx}.0"))
        .chain(std::iter::once(
            "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.42s".to_string(),
        ))
        .collect::<Vec<_>>()
        .join("\n");
    let rendered = render_shell(success_input("cargo check -p demo", "", &stderr));
    let raw = count_tokens(&shell_combined_output(
        "cargo check -p demo",
        Some(0),
        "",
        &stderr,
    ));
    let visible_tokens = count_tokens(&rendered.visible);

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered.visible.contains("50 compiled"),
        "{}",
        rendered.visible
    );
    assert!(
        rendered.visible.contains("in 8.42s"),
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
    assert!(!rendered.visible.contains("Compiling crate7"));
    assert!(
        visible_tokens * 3 < raw,
        "visible={visible_tokens} raw={raw}\n{}",
        rendered.visible
    );
}

#[test]
fn cargo_success_keeps_warning_blocks_verbatim() {
    let mut stderr = (0..30)
        .map(|idx| format!("   Compiling dep{idx} v0.{idx}.0"))
        .collect::<Vec<_>>()
        .join("\n");
    stderr.push_str(
        "\nwarning: unused variable: `x`\n \
 --> src/lib.rs:5:9\n  |\n5 |     let x = 1;\n  |         ^ help: prefix with underscore\n",
    );
    stderr.push_str(
        &(30..60)
            .map(|idx| format!("   Compiling dep{idx} v0.{idx}.0"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    stderr.push_str("\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.00s\n");
    let rendered = render_shell(success_input("cargo build", "", &stderr));

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        !rendered.visible.contains("Compiling dep7"),
        "{}",
        rendered.visible
    );
    for anchor in [
        "warning: unused variable: `x`",
        "--> src/lib.rs:5:9",
        "^ help: prefix with underscore",
    ] {
        assert!(
            rendered.visible.contains(anchor),
            "protected anchor lost: {anchor}\n{}",
            rendered.visible
        );
    }
}

#[test]
fn cargo_test_success_collapses_ok_lines_and_keeps_result_verbatim() {
    let stdout = std::iter::once("running 53 tests".to_string())
            .chain((0..53).map(|idx| format!("test module::case_{idx} ... ok")))
            .chain(std::iter::once(
                "test result: ok. 53 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s"
                    .to_string(),
            ))
            .collect::<Vec<_>>()
            .join("\n");
    let rendered = render_shell(success_input("cargo test -p demo --lib", &stdout, ""));
    let raw = count_tokens(&shell_combined_output(
        "cargo test -p demo --lib",
        Some(0),
        &stdout,
        "",
    ));
    let visible_tokens = count_tokens(&rendered.visible);

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered.visible.contains("53 tests ok"),
        "{}",
        rendered.visible
    );
    assert!(
        rendered
            .visible
            .contains("test result: ok. 53 passed; 0 failed;"),
        "result line must stay verbatim: {}",
        rendered.visible
    );
    assert!(!rendered.visible.contains("case_17"));
    assert!(
        visible_tokens * 3 < raw,
        "visible={visible_tokens} raw={raw}\n{}",
        rendered.visible
    );
}

#[test]
fn passing_tests_with_critical_keyword_names_still_collapse() {
    let stdout = "running 4 tests\n\
test tests::warning_handling_works ... ok\n\
test tests::failure_paths_are_covered ... ok\n\
test tests::error_messages_render ... ok\n\
test tests::panic_recovery_guard ... ok\n\
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
    let rendered = render_shell(success_input("cargo test -p demo", stdout, ""));

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered.visible.contains("4 tests ok"),
        "{}",
        rendered.visible
    );
    assert!(
        !rendered.visible.contains("warning_handling_works"),
        "pass markers must collapse regardless of name: {}",
        rendered.visible
    );
    assert!(
        rendered
            .visible
            .contains("test result: ok. 4 passed; 0 failed;"),
        "{}",
        rendered.visible
    );
}

#[test]
fn pytest_success_collapses_progress_and_keeps_summary() {
    let stdout = "============================= test session starts ==============================\n\
platform darwin -- Python 3.12.0, pytest-8.0.0\n\
rootdir: /tmp/demo\n\
collected 12 items\n\
tests/test_a.py::test_one PASSED\n\
tests/test_a.py::test_two PASSED\n\
........                                                                 [100%]\n\
============================== 12 passed in 0.34s ==============================\n";
    let rendered = render_shell(success_input("pytest tests", stdout, ""));

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered.visible.contains("12 passed in 0.34s"),
        "{}",
        rendered.visible
    );
    assert!(!rendered.visible.contains("test_one PASSED"));
    assert!(!rendered.visible.contains("rootdir:"));
}

#[test]
fn npm_install_success_keeps_summary_and_drops_funding_noise() {
    let mut stdout = (0..20)
        .map(|idx| format!("npm http fetch GET 200 https://registry.npmjs.org/pkg-{idx} 41ms"))
        .collect::<Vec<_>>()
        .join("\n");
    stdout.push_str(
        "\nadded 57 packages, and audited 58 packages in 3s\n\
7 packages are looking for funding\n\
  run `npm fund` for details\n\
found 0 vulnerabilities\n",
    );
    let rendered = render_shell(success_input("npm install", &stdout, ""));

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered.visible.contains("added 57 packages"),
        "{}",
        rendered.visible
    );
    assert!(
        rendered.visible.contains("found 0 vulnerabilities"),
        "{}",
        rendered.visible
    );
    assert!(!rendered.visible.contains("npm fund"));
}

#[test]
fn git_clone_success_collapses_progress_to_final_state() {
    let stderr = "Cloning into 'demo'...\n\
remote: Enumerating objects: 1200, done.\n\
remote: Counting objects:  10% (120/1200)\n\
remote: Counting objects: 100% (1200/1200), done.\n\
Receiving objects:  10% (120/1200)\n\
Receiving objects:  55% (660/1200)\n\
Receiving objects: 100% (1200/1200), 2.5 MiB | 5 MiB/s, done.\n\
Resolving deltas:  40% (200/500)\n\
Resolving deltas: 100% (500/500), done.\n";
    let rendered = render_shell(success_input(
        "git clone https://example.com/demo.git",
        "",
        stderr,
    ));

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered
            .visible
            .contains("Receiving objects: 100% (1200/1200), 2.5 MiB | 5 MiB/s, done."),
        "final progress state must survive: {}",
        rendered.visible
    );
    assert!(
        !rendered.visible.contains("Receiving objects:  55%"),
        "{}",
        rendered.visible
    );
    assert!(
        rendered.visible.contains("Cloning into 'demo'..."),
        "{}",
        rendered.visible
    );
}

#[test]
fn pytest_success_collapses_bare_progress_lines() {
    let stdout = "collected 12 items\n............\n============ 12 passed in 0.34s ============\n";
    let rendered = render_shell(success_input("pytest tests -q", stdout, ""));

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered.visible.contains("12 passed in 0.34s"),
        "{}",
        rendered.visible
    );
    assert!(
        !rendered.visible.contains("............"),
        "{}",
        rendered.visible
    );
}
