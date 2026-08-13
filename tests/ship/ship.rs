//! Bounded release proof for TokenZero's user-visible CLI and installer.

use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use tempfile::tempdir;

/// True when the runner exported the public binary path. A plain
/// `cargo test --workspace` (no ship env) skips these so development
/// workflows stay green without weakening the release gate.
fn ship_env_set() -> bool {
    std::env::var_os("TOKENZERO_SHIP_BIN").is_some()
}

fn tokenzero() -> Command {
    let binary = std::env::var_os("TOKENZERO_SHIP_BIN")
        .map(PathBuf::from)
        .expect("TOKENZERO_SHIP_BIN must be set to run ship tests");
    let mut command = Command::new(binary);
    for key in [
        "TOKENZERO_ROOT",
        "TOKENZERO_CACHE_PATH",
        "TOKENZERO_SHARED_STORE",
        "ZEROSTACK_SHARED_STORE",
        "ZEROSTACK_STORE_ROOT",
    ] {
        command.env_remove(key);
    }
    command.env("NO_COLOR", "1").env("CI", "true");
    command
}

fn success(output: &Output) -> bool {
    output.status.success() && output.stderr.is_empty()
}

fn json_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

fn installer() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../packaging/install.sh")
}

fn first_ref(value: &Value, kind: &str) -> String {
    let prefix = format!("tz://{kind}/");
    value["refs"]
        .as_array()
        .expect("refs array")
        .iter()
        .find(|row| {
            row.as_str()
                .is_some_and(|ref_text| ref_text.starts_with(&prefix))
        })
        .and_then(|row| row.as_str())
        .unwrap_or_else(|| panic!("missing {kind} ref: {value}"))
        .to_owned()
}

#[test]
fn ship_help_and_capabilities_are_machine_usable() {
    if !ship_env_set() {
        eprintln!("skipped: TOKENZERO_SHIP_BIN/TOKENZERO_SHIP_WORKER_BIN not set");
        return;
    }
    let help = tokenzero().output().expect("help");
    let capabilities = tokenzero()
        .args(["capabilities", "--json"])
        .output()
        .expect("capabilities");
    let value = json_stdout(&capabilities);
    let oracle = |help: &[u8], value: &Value| {
        help.windows(b"tokenzero run --json".len())
            .any(|window| window == b"tokenzero run --json")
            && value["schema_version"] == "tokenzero.capabilities.v1"
            && value["tool"] == "tokenzero"
    };
    assert!(
        success(&help) && success(&capabilities) && oracle(&help.stdout, &value),
        "help and capabilities contract"
    );
}

#[test]
fn ship_capabilities_are_deterministic_and_env_clean() {
    if !ship_env_set() {
        eprintln!("skipped: TOKENZERO_SHIP_BIN/TOKENZERO_SHIP_WORKER_BIN not set");
        return;
    }
    let run = || {
        tokenzero()
            .env("TERM", "dumb")
            .env("SOURCE_DATE_EPOCH", "1234567890")
            .args(["capabilities", "--json"])
            .output()
            .expect("capabilities")
    };
    let first = run();
    let second = run();
    let oracle = |left: &[u8], right: &[u8]| left == right && !left.contains(&0x1b);
    assert!(
        success(&first) && success(&second) && oracle(&first.stdout, &second.stdout),
        "capabilities must be deterministic and ANSI-free"
    );
}

#[test]
fn ship_read_expand_round_trips_exact_bytes() {
    if !ship_env_set() {
        eprintln!("skipped: TOKENZERO_SHIP_BIN/TOKENZERO_SHIP_WORKER_BIN not set");
        return;
    }
    let root = tempdir().expect("root");
    let cache = root.path().join("cache.json");
    let file = root.path().join("payload.txt");
    let payload = "BEGIN\n".to_owned() + &"bounded payload line\n".repeat(4_000) + "END\n";
    fs::write(&file, &payload).expect("fixture");
    let read = tokenzero()
        .current_dir(root.path())
        .args([
            "read",
            file.to_str().unwrap(),
            "--max-visible-tokens",
            "64",
            "--cache-path",
            cache.to_str().unwrap(),
            "--allowed-root",
            root.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("read");
    let reference = first_ref(&json_stdout(&read), "blob");
    let expanded = tokenzero()
        .current_dir(root.path())
        .args([
            "expand",
            &reference,
            "--cache-path",
            cache.to_str().unwrap(),
            "--raw",
        ])
        .output()
        .expect("expand");
    let oracle = |bytes: &[u8]| bytes == payload.as_bytes();
    assert!(
        success(&read) && success(&expanded) && oracle(&expanded.stdout),
        "read/expand must recover exact bytes"
    );
}

#[test]
fn ship_find_returns_bounded_visible_evidence_and_refs() {
    if !ship_env_set() {
        eprintln!("skipped: TOKENZERO_SHIP_BIN/TOKENZERO_SHIP_WORKER_BIN not set");
        return;
    }
    let root = tempdir().expect("root");
    let cache = root.path().join("cache.json");
    fs::write(root.path().join("sample.txt"), "alpha\nbeta\nalphabet\n").unwrap();
    let output = tokenzero()
        .current_dir(root.path())
        .env("TOKENZERO_SEARCH_BACKEND", "internal")
        .args([
            "find",
            "alpha",
            "sample.txt",
            "--cache-path",
            cache.to_str().unwrap(),
            "--allowed-root",
            root.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("find");
    let value = json_stdout(&output);
    let oracle = |value: &Value| {
        value["visible"]
            .as_str()
            .is_some_and(|text| text.contains("alpha") && text.len() < 4_096)
            && value["refs"]
                .as_array()
                .is_some_and(|refs| !refs.is_empty())
    };
    assert!(
        success(&output) && oracle(&value),
        "find must return bounded evidence and recovery refs"
    );
}

#[test]
fn ship_run_preserves_child_stdout_and_failure_status() {
    if !ship_env_set() {
        eprintln!("skipped: TOKENZERO_SHIP_BIN/TOKENZERO_SHIP_WORKER_BIN not set");
        return;
    }
    let output = tokenzero()
        .args(["run", "--json", "--", "sh", "-c", "printf ship-run; exit 7"])
        .output()
        .expect("run");
    let oracle = |output: &Output| {
        output.status.code() == Some(7)
            && output
                .stdout
                .windows(b"ship-run".len())
                .any(|window| window == b"ship-run")
    };
    assert!(
        oracle(&output),
        "run must preserve child stdout and exit status"
    );
}

#[test]
fn ship_run_executes_standalone_posix_builtins_through_shell() {
    if !ship_env_set() {
        eprintln!("skipped: TOKENZERO_SHIP_BIN/TOKENZERO_SHIP_WORKER_BIN not set");
        return;
    }
    let root = tempdir().expect("root");
    let output = tokenzero()
        .current_dir(root.path())
        .args(["run", "--json", "--", "cd", "."])
        .output()
        .expect("standalone cd");
    let value = json_stdout(&output);
    let plan = tokenzero()
        .args(["run", "--explain-runtime", "--", "cd", "."])
        .output()
        .expect("standalone cd runtime plan");
    let plan_value = json_stdout(&plan);
    assert!(
        output.status.success()
            && value["status"] == "ok"
            && value["tool"] == "shell"
            && plan.status.success()
            && plan_value["execution_mode"] == "shell",
        "standalone POSIX builtins must route through a shell"
    );
}

#[test]
fn ship_doctor_reports_isolated_store_resolution() {
    if !ship_env_set() {
        eprintln!("skipped: TOKENZERO_SHIP_BIN/TOKENZERO_SHIP_WORKER_BIN not set");
        return;
    }
    let root = tempdir().expect("root");
    fs::create_dir_all(root.path().join(".zerostack/tokenzero")).unwrap();
    let output = tokenzero()
        .args(["doctor", "--root", root.path().to_str().unwrap(), "--json"])
        .output()
        .expect("doctor");
    let value = json_stdout(&output);
    let oracle = |value: &Value| {
        value["store_resolution"]["effective_cache_path"]
            .as_str()
            .is_some_and(|path| path.contains(".zerostack/tokenzero"))
    };
    assert!(
        output.status.success() && oracle(&value),
        "doctor must expose project-isolated store resolution"
    );
}

#[test]
fn ship_hook_rewrites_actionable_input_and_fails_open_otherwise() {
    if !ship_env_set() {
        eprintln!("skipped: TOKENZERO_SHIP_BIN/TOKENZERO_SHIP_WORKER_BIN not set");
        return;
    }
    fn hook(payload: &str) -> Output {
        let mut child = tokenzero()
            .args(["hook", "claude-code"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("hook");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
    let actionable = hook(r#"{"tool_name":"Bash","tool_input":{"command":"printf ship-hook"}}"#);
    let unrelated = hook(r#"{"tool_name":"Other","tool_input":{}}"#);
    let value = json_stdout(&actionable);
    let oracle = |value: &Value, unrelated: &Output| {
        value["hookSpecificOutput"]["permissionDecision"] == "allow"
            && value["hookSpecificOutput"]["updatedInput"]["command"]
                .as_str()
                .is_some_and(|command| command.contains("tokenzero") && command.contains("run"))
            && success(unrelated)
            && unrelated.stdout.is_empty()
    };
    assert!(
        success(&actionable) && oracle(&value, &unrelated),
        "hook must rewrite actionable input and ignore unrelated input"
    );
}

#[test]
fn ship_absolute_path_rejection_does_not_leak_bytes() {
    if !ship_env_set() {
        eprintln!("skipped: TOKENZERO_SHIP_BIN/TOKENZERO_SHIP_WORKER_BIN not set");
        return;
    }
    let root = tempdir().expect("root");
    let outside = tempdir().expect("outside");
    let secret = "SHIP-SECRET-MUST-NOT-LEAK";
    let file = outside.path().join("secret.txt");
    fs::write(&file, secret).unwrap();
    let output = tokenzero()
        .current_dir(root.path())
        .args([
            "read",
            file.to_str().unwrap(),
            "--allowed-root",
            root.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("rejected read");
    let oracle = |status_ok: bool, stdout: &[u8], stderr: &[u8]| {
        !status_ok
            && !stdout
                .windows(secret.len())
                .any(|window| window == secret.as_bytes())
            && !stderr
                .windows(secret.len())
                .any(|window| window == secret.as_bytes())
    };
    assert!(
        oracle(output.status.success(), &output.stdout, &output.stderr),
        "outside-root read must fail without leaking bytes"
    );
}

#[test]
fn ship_installer_dry_run_is_canonical_and_non_mutating() {
    if !ship_env_set() {
        eprintln!("skipped: TOKENZERO_SHIP_BIN/TOKENZERO_SHIP_WORKER_BIN not set");
        return;
    }
    let root = tempdir().expect("root");
    let prefix = root.path().join("prefix");
    let bin = root.path().join("bin");
    let output = Command::new("bash")
        .arg(installer())
        .args(["--surface", "codemode", "--dry-run", "--prefix"])
        .arg(&prefix)
        .arg("--bin-dir")
        .arg(&bin)
        .output()
        .expect("installer dry run");
    let expected = b"cargo build --release -p tokenzero-worker --bin tokenzero-codemode --no-default-features\n";
    let oracle = |bytes: &[u8], mutated: bool| bytes == expected && !mutated;
    assert!(
        success(&output) && oracle(&output.stdout, prefix.exists() || bin.exists()),
        "installer dry run must be canonical and non-mutating"
    );
}

#[test]
fn ship_uninstall_preserves_unowned_cli() {
    if !ship_env_set() {
        eprintln!("skipped: TOKENZERO_SHIP_BIN/TOKENZERO_SHIP_WORKER_BIN not set");
        return;
    }
    let root = tempdir().expect("root");
    let prefix = root.path().join("prefix");
    let bin = root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let cli = bin.join("tokenzero");
    fs::write(&cli, b"canonical cli").unwrap();
    let output = Command::new("bash")
        .arg(installer())
        .arg("--uninstall")
        .arg("--prefix")
        .arg(&prefix)
        .arg("--bin-dir")
        .arg(&bin)
        .output()
        .expect("uninstall");
    let actual = fs::read(&cli).expect("unowned CLI remains");
    let oracle = |bytes: &[u8]| bytes == b"canonical cli";
    assert!(
        output.status.success() && oracle(&actual),
        "uninstall must preserve an unowned CLI"
    );
}
