use super::*;

#[test]
fn discovers_launch_critical_families() {
    let report = discover();
    let families: Vec<_> = report
        .supported_filters
        .iter()
        .map(|f| f.family.as_str())
        .collect();
    for family in [
        "read", "search", "tree", "git", "test", "build", "docker", "kubectl", "package",
    ] {
        assert!(families.contains(&family));
    }
}

#[test]
fn cat_rewrites_to_read() {
    let result = rewrite_command("cat README.md", "safe", true);
    assert!(result.applied);
    assert_eq!(result.rewritten_command, "tokenzero read README.md");
}

#[test]
fn destructive_commands_are_unmodified() {
    let result = rewrite_command("git push origin main", "safe", true);
    assert!(!result.applied);
    assert!(!result.safe);
    assert_eq!(result.rewritten_command, "git push origin main");
}

#[test]
fn compound_commands_are_left_unmodified() {
    for command in [
        "cat foo.txt | grep bar",
        "cargo test --workspace 2>&1 | tail -40",
        "ls -la; git status",
        "make build && make test",
        "grep -r needle . || true",
        "git status\nrm -rf /tmp/x",
        "git status\rrm -rf /tmp/x",
    ] {
        let result = rewrite_command(command, "safe", true);
        assert!(!result.applied, "{command}");
        assert_eq!(result.rewritten_command, command);
        assert_eq!(result.reason, "compound command left unmodified");
        assert!(!result.safe, "compounds are never vouched: {command}");
    }
}

#[test]
fn command_substitution_counts_as_compound() {
    for command in [
        "cat foo $(rm -rf /tmp/x)",
        "echo \"today is $(date)\"",
        "echo \"`uname -a`\"",
        "cat $((1+1)).txt",
    ] {
        let result = rewrite_command(command, "safe", true);
        assert!(!result.applied, "{command}");
        assert_eq!(
            result.reason, "compound command left unmodified",
            "{command}"
        );
        assert!(!result.safe, "{command}");
    }
}

#[test]
fn dispatchers_and_remote_execution_are_never_vouched() {
    for (command, fragment) in [
        ("xargs rm -rf /tmp/foo", "dispatcher"),
        ("eval ls", "dispatcher"),
        ("sudo ls", "dispatcher"),
        ("npx some-package", "dispatcher"),
        ("ssh host uptime", "remote execution"),
        ("scp file host:/tmp/", "remote execution"),
    ] {
        let result = rewrite_command(command, "safe", true);
        assert!(!result.applied, "{command}");
        assert!(!result.safe, "{command}");
        assert!(
            result.reason.contains(fragment),
            "{command}: {}",
            result.reason
        );
    }
}

#[test]
fn expanded_destructive_commands_are_flagged() {
    for command in [
        "shred -u secrets.txt",
        "truncate -s 0 log.txt",
        "mkfs.ext4 /dev/sda1",
        "mount /dev/sda1 /mnt",
        "rsync --delete src/ dst/",
        "sed -i s/a/b/ file.txt",
        "perl -pi -e s/a/b/ file.txt",
        "find . -name '*.tmp' -delete",
        "find . -name '*.log' -exec rm {} +",
        "git restore .",
        "git stash drop",
        "git tag v1.0.0",
        "git remote add origin https://example.com/repo.git",
        "docker run --rm image",
        "docker compose up -d",
        "docker cp file container:/tmp/file",
        "docker import image.tar repo:tag",
        "kubectl exec -it pod -- sh",
        "kubectl cp file pod:/tmp/file",
        "cargo add serde",
        "npm uninstall left-pad",
        "npm ci",
        "uv pip install requests",
    ] {
        let result = rewrite_command(command, "safe", true);
        assert!(!result.applied, "{command}");
        assert!(!result.safe, "{command}");
        assert_eq!(result.rewritten_command, command, "{command}");
    }
}

#[test]
fn unknown_families_are_not_vouched() {
    let result = rewrite_command("frobnicate --all", "safe", true);
    assert!(!result.applied);
    assert_eq!(result.reason, "unsupported command family");
    assert!(!result.safe);
}

#[test]
fn disabled_mode_reports_honest_safety() {
    let dangerous = rewrite_command("rm -rf /tmp/x", "off", false);
    assert!(!dangerous.applied);
    assert_eq!(dangerous.reason, "disabled");
    assert!(!dangerous.safe);

    let benign = rewrite_command("cat README.md", "off", false);
    assert!(!benign.applied);
    assert_eq!(benign.reason, "disabled");
    assert!(benign.safe);
}

#[test]
fn read_only_finds_and_passthroughs_stay_vouched() {
    for command in [
        "find . -name '*.rs'",
        "git status",
        "docker ps",
        "kubectl get pods",
        "head -n 5 foo.txt",
    ] {
        let result = rewrite_command(command, "safe", true);
        assert!(result.safe, "{command}");
        assert_eq!(result.rewritten_command, command, "{command}");
    }
}

#[cfg(not(windows))]
#[test]
fn backslash_escaped_quotes_split_correctly() {
    assert_eq!(
        split_words(r#"cat "a\"b.txt""#),
        vec!["cat".to_string(), "a\"b.txt".to_string()]
    );
    // An escaped quote must not flip quote state and hide a real pipe.
    assert!(has_shell_operators(r#"echo \" | rm -rf /tmp/x"#));
    // An escaped operator is not an operator.
    assert!(!has_shell_operators(r"cat foo\;bar.txt"));
}

#[test]
fn quiet_flags_injected_for_noisy_toolchains() {
    for (command, expected) in [
        ("cargo build --workspace", "cargo build --workspace -q"),
        ("cargo check -p demo", "cargo check -p demo -q"),
        (
            "cargo clippy --all-targets",
            "cargo clippy --all-targets -q",
        ),
        ("cargo test -p demo", "cargo test -p demo -q"),
        (
            "git clone https://example.com/demo.git",
            "git clone https://example.com/demo.git --quiet",
        ),
        ("git fetch origin", "git fetch origin --quiet"),
        ("git pull origin main", "git pull origin main --quiet"),
        ("npm test", "npm test --silent"),
        ("npm run build", "npm run build --silent"),
    ] {
        let result = rewrite_command(command, "safe", true);
        assert!(result.applied, "{command}");
        assert!(result.safe, "{command}");
        assert_eq!(result.rewritten_command, expected, "{command}");
    }
}

#[test]
fn bounded_rewrites_respect_existing_limits() {
    for command in [
        "tree -L 0",
        "tree -L2 src",
        "tree --depth=4 src",
        "git log --max-count=5",
        "git log -n5",
        "git log -n 5",
    ] {
        let result = rewrite_command(command, "safe", true);
        assert_eq!(result.rewritten_command, command, "{command}");
        assert!(!result.applied, "{command}");
        assert!(result.safe, "{command}");
    }
}

#[test]
fn quiet_injection_respects_explicit_verbosity_and_passthrough_separators() {
    for command in [
        "cargo build -q",
        "cargo test --workspace -- --nocapture",
        "cargo check --verbose",
        "git clone --progress https://example.com/demo.git",
        "git fetch -v origin",
        "npm test --silent",
        "npm run build --loglevel=warn",
        "pnpm test",
        "yarn test",
        "go test ./...",
    ] {
        let result = rewrite_command(command, "safe", true);
        assert_eq!(result.rewritten_command, command, "{command}");
        assert!(!result.applied, "{command}");
    }
}

#[test]
fn quiet_injection_never_touches_mutations_or_compounds() {
    for command in [
        "git push origin main",
        "npm install left-pad",
        "cargo install ripgrep",
        "cargo build && cargo test",
        "git pull origin main || true",
    ] {
        let result = rewrite_command(command, "safe", true);
        assert_eq!(result.rewritten_command, command, "{command}");
        assert!(!result.applied, "{command}");
    }
}

#[test]
fn quoted_operators_do_not_count_as_compound() {
    let result = rewrite_command("cat 'a|b.txt'", "safe", true);
    assert!(result.applied);
    assert_eq!(result.rewritten_command, "tokenzero read 'a|b.txt'");
}
