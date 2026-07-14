use super::*;

fn assert_not_rewritten(result: &RewriteResult) {
    assert!(!result.applied, "expected no rewrite for '{}'", result.command);
    assert_eq!(result.rewritten_command, result.command, "rewrite changed '{}'", result.command);
}

fn assert_rewritten_to(result: &RewriteResult, expected: &str) {
    assert!(result.applied, "expected rewrite for '{}'", result.command);
    assert_eq!(result.rewritten_command, expected, "'{}' rewrite mismatch", result.command);
}

const EXPECTED_FAMILIES: &[&str] = &[
    "read", "search", "tree", "git", "test", "build", "docker", "kubectl", "package", "config",
];

#[test]
fn discovers_launch_critical_families() {
    let report = discover();
    assert!(report.install_ready);
    assert!(report.mcp_ready);
    assert!(report.shell_ready);
    assert!(report.os_warnings.is_empty(), "no OS warnings on this platform");
    assert_eq!(report.supported_filters.len(), EXPECTED_FAMILIES.len());
    for filter in &report.supported_filters {
        assert!(filter.supported, "{}", filter.family);
        assert!(filter.exact_refs, "{}", filter.family);
        assert!(!filter.commands.is_empty(), "{}", filter.family);
    }
    let families: Vec<_> = report.supported_filters.iter().map(|f| f.family.as_str()).collect();
    for family in EXPECTED_FAMILIES {
        assert!(families.contains(family), "family '{family}' must be present");
    }
}

#[test]
fn compound_commands_are_left_unmodified() {
    const BENIGN: &[&str] = &[
        "cat foo.txt | grep bar", "cargo test --workspace 2>&1 | tail -40",
        "ls -la; git status", "make build && make test", "grep -r needle . || true",
        "echo \"today is $(date)\"", "echo \"`uname -a`\"", "cat $((1+1)).txt",
    ];
    for command in BENIGN {
        let result = rewrite_command(command, "safe", true);
        assert_not_rewritten(&result);
        assert_eq!(result.reason, "compound command left unmodified");
        assert!(!result.safe, "{command}");
    }
    const MUTATING: &[&str] = &[
        "git status\nrm -rf /tmp/x", "git status\rrm -rf /tmp/x", "cat foo $(rm -rf /tmp/x)",
    ];
    for command in MUTATING {
        let result = rewrite_command(command, "safe", true);
        assert_not_rewritten(&result);
        assert!(result.reason.contains("destructive mutation"), "{command}: {}", result.reason);
        assert!(!result.safe, "{command}");
    }
}

#[test]
fn expanded_destructive_commands_are_flagged() {
    // Labels make failures independent of command spelling while retaining every policy row.
    const CASES: &[(&str, &str, &str)] = &[
        ("rm", "rm -rf /tmp/x", "destructive"),
        ("shred", "shred -u secrets.txt", "destructive"),
        ("truncate", "truncate -s 0 log.txt", "destructive"),
        ("mkfs", "mkfs.ext4 /dev/sda1", "destructive"),
        ("mount", "mount /dev/sda1 /mnt", "destructive"),
        ("rsync", "rsync --delete src/ dst/", "destructive"),
        ("sed", "sed -i s/a/b/ file.txt", "in-place file edit"),
        ("perl", "perl -pi -e s/a/b/ file.txt", "in-place file edit"),
        ("find-delete", "find . -name '*.tmp' -delete", "find with side effects"),
        ("find-exec", "find . -name '*.log' -exec rm {} +", "find with side effects"),
        ("git-push", "git push origin main", "git mutation"),
        ("git-restore", "git restore .", "git mutation"),
        ("git-stash", "git stash drop", "git mutation"),
        ("git-tag", "git tag v1.0.0", "git mutation"),
        ("git-remote", "git remote add origin https://example.com/repo.git", "git mutation"),
        ("docker-run", "docker run --rm image", "docker mutation"),
        ("compose-up", "docker compose up -d", "docker mutation"),
        ("docker-cp", "docker cp file container:/tmp/file", "docker mutation"),
        ("docker-import", "docker import image.tar repo:tag", "docker mutation"),
        ("kubectl-exec", "kubectl exec -it pod -- sh", "kubectl mutation"),
        ("kubectl-cp", "kubectl cp file pod:/tmp/file", "kubectl mutation"),
        ("cargo-add", "cargo add serde", "package"),
        ("npm-uninstall", "npm uninstall left-pad", "package"),
        ("npm-ci", "npm ci", "package"),
        ("uv-pip", "uv pip install requests", "package"),
        ("xargs", "xargs rm -rf /tmp/foo", "dispatcher"),
        ("eval", "eval ls", "dispatcher"),
        ("sudo", "sudo ls", "dispatcher"),
        ("npx", "npx some-package", "dispatcher"),
        ("ssh", "ssh host uptime", "remote execution"),
        ("scp", "scp file host:/tmp/", "remote execution"),
    ];
    for &(label, command, reason) in CASES {
        let result = rewrite_command(command, "safe", true);
        assert_not_rewritten(&result);
        assert!(!result.safe, "{label}: {command}");
        assert!(result.reason.contains(reason), "{label}: {}", result.reason);
    }
}

#[test]
fn disabled_mode_reports_honest_safety() {
    for (command, safe) in [("rm -rf /tmp/x", false), ("cat README.md", true), ("git push origin main", false)] {
        let result = rewrite_command(command, "off", false);
        assert_eq!(result.reason, "disabled");
        assert_eq!(result.safe, safe, "{command}");
    }
    assert_eq!(rewrite_command("cat README.md", "off", false).family, "read");
    let result = rewrite_command("frobnicate --all", "safe", true);
    assert_not_rewritten(&result);
    assert_eq!(result.reason, "unsupported command family");
    assert!(!result.safe);
    assert_eq!(result.family, "unknown");
}

#[test]
fn read_only_finds_and_passthroughs_stay_vouched() {
    for (command, family) in [
        ("head -n 5 foo.txt", "read"), ("find . -name '*.rs'", "tree"),
        ("git status", "git"), ("git diff", "git"), ("docker ps", "docker"),
        ("kubectl get pods", "kubectl"),
    ] {
        let result = rewrite_command(command, "safe", true);
        assert!(result.safe, "{command}");
        assert_eq!(result.family, family, "{command}");
        assert_eq!(result.rewritten_command, command, "{command}");
    }
    let result = rewrite_command("cat README.md", "safe", true);
    assert!(result.safe);
    assert_eq!(result.family, "read");
    assert_rewritten_to(&result, "tokenzero read README.md");
    let result = rewrite_command("cat 'a|b.txt'", "safe", true);
    assert_rewritten_to(&result, "tokenzero read 'a|b.txt'");
    assert!(result.safe);
    assert_eq!(result.family, "read");
}

#[test]
fn argument_payloads_are_never_classified_as_intent() {
    for command in [r#"br create --description "will write and remove things""#, r#"echo "rm -rf""#, r#"printf '%s' "drop table""#] {
        assert_eq!(unsafe_reason(command), None, "payload changed policy: {command}");
    }
    for command in [r#"git commit -m "documentation only""#, r#"git commit -m "delete old write path""#, r#"git push -o ci.variable="message=read only""#] {
        assert!(unsafe_reason(command).is_some_and(|r| r.contains("git mutation")), "{command}");
    }
}

#[test]
fn mutation_is_detected_at_every_shell_command_position() {
    for command in [
        "echo ok && rm x", "echo ok || rm x", "echo ok; rm x", "echo ok | rm x",
        "echo ok\nrm x", "$(rm x)", "echo $(rm x)", "echo \u{60}rm x\u{60}",
        r#"sh -c 'rm x'"#, r#"bash -c "echo ok && rm x""#, r#"/bin/sh -c 'git push origin main'"#,
    ] {
        assert!(unsafe_reason(command).is_some(), "mutation escaped policy: {command}");
    }
}

#[test]
fn quoted_command_text_is_data_but_substitutions_still_execute() {
    for command in [r#"echo "rm -rf""#, r#"printf '%s' 'git push'"#] {
        assert_eq!(unsafe_reason(command), None, "quoted data changed policy: {command}");
    }
    for command in [r#"echo "$(rm x)""#, "printf '%s' \u{60}git push\u{60}"] {
        assert!(unsafe_reason(command).is_some(), "substitution escaped policy: {command}");
    }
}

#[cfg(not(windows))]
#[test]
fn backslash_escaped_quotes_split_correctly() {
    assert_eq!(split_words(r#"cat "a\"b.txt""#), vec!["cat".to_string(), "a\"b.txt".to_string()]);
    assert!(has_shell_operators(r#"echo \" | rm -rf /tmp/x"#));
    assert!(!has_shell_operators(r"cat foo\;bar.txt"));
}

#[test]
fn quiet_flags_injected_for_noisy_toolchains() {
    for (command, expected) in [
        ("cargo build --workspace", "cargo build --workspace -q"),
        ("cargo check -p demo", "cargo check -p demo -q"),
        ("cargo clippy --all-targets", "cargo clippy --all-targets -q"),
        ("cargo test -p demo", "cargo test -p demo -q"),
        ("git clone https://example.com/demo.git", "git clone https://example.com/demo.git --quiet"),
        ("git fetch origin", "git fetch origin --quiet"),
        ("git pull origin main", "git pull origin main --quiet"),
        ("npm test", "npm test --silent"),
        ("npm run build", "npm run build --silent"),
    ] {
        let result = rewrite_command(command, "safe", true);
        assert_rewritten_to(&result, expected);
        assert!(result.safe, "{command}");
    }
}

#[test]
fn bounded_rewrites_respect_existing_limits() {
    for command in ["tree -L 0", "tree -L2 src", "tree --depth=4 src", "git log --max-count=5", "git log -n5", "git log -n 5"] {
        let result = rewrite_command(command, "safe", true);
        assert_not_rewritten(&result);
        assert!(result.safe, "{command}");
    }
    for command in [
        "cargo build -q", "cargo test --workspace -- --nocapture", "cargo check --verbose",
        "git clone --progress https://example.com/demo.git", "git fetch -v origin",
        "npm test --silent", "npm run build --loglevel=warn", "pnpm test", "yarn test", "go test ./...",
    ] {
        assert_not_rewritten(&rewrite_command(command, "safe", true));
    }
}

#[test]
fn quiet_injection_never_touches_mutations_or_compounds() {
    for command in ["git push origin main", "npm install left-pad", "cargo install ripgrep", "cargo build && cargo test", "git pull origin main || true"] {
        assert_not_rewritten(&rewrite_command(command, "safe", true));
    }
}
