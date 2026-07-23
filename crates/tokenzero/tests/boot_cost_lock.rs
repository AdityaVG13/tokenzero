use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn boot_cost_lock_covers_real_small_and_23k_corpora() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .to_path_buf();
    let label = format!("ci-lock-{}", std::process::id());

    let unit = Command::new("python3")
        .args(["-m", "unittest", "benchmarks.test_boot_cost"])
        .current_dir(&repo)
        .output()
        .expect("run boot-cost contract tests");
    assert!(
        unit.status.success(),
        "boot-cost contract tests failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&unit.stdout),
        String::from_utf8_lossy(&unit.stderr)
    );

    let gate = Command::new("python3")
        .args(["benchmarks/boot-cost.py", "--label", &label])
        .env("TOKENZERO_BOOT_BENCH_BIN", env!("CARGO_BIN_EXE_tokenzero"))
        .current_dir(&repo)
        .output()
        .expect("run boot-cost gate");
    let evidence = repo
        .join("benchmarks/boot-cost")
        .join(format!("{label}.json"));
    let _ = fs::remove_file(&evidence);
    assert!(
        gate.status.success(),
        "boot-cost gate failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&gate.stdout),
        String::from_utf8_lossy(&gate.stderr)
    );
}
