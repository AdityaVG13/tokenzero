use super::*;

#[test]
fn applied_cat_rewrite_builds_the_executed_argv() {
    let command = "cat src/lib.rs";
    let rewrite = rewrite_for_shell(command, "on", false, false);

    assert!(rewrite.applied);
    assert_eq!(rewrite.rewritten_command, "tokenzero read src/lib.rs");
    assert_eq!(
        shell_execution_argv(command, None, &rewrite),
        ["tokenzero", "read", "src/lib.rs"]
    );
}

#[test]
fn explicit_argv_is_authoritative_and_rewrite_is_truthfully_skipped() {
    let command = "cat display-only.txt";
    let rewrite = rewrite_for_shell(command, "on", false, true);
    let explicit = vec![
        "printf".to_string(),
        "%s".to_string(),
        "literal".to_string(),
    ];

    assert!(!rewrite.applied);
    assert_eq!(
        rewrite.reason,
        "explicit argv is authoritative; command rewrite skipped"
    );
    assert_eq!(
        shell_execution_argv(command, Some(explicit.clone()), &rewrite),
        explicit
    );
}

#[test]
fn explicit_argv_skip_retains_an_unsafe_command_reason() {
    let rewrite = rewrite_for_shell("rm -rf target", "on", false, true);

    assert!(!rewrite.applied);
    assert!(!rewrite.safe);
    assert!(rewrite.reason.contains("unsafe destructive mutation"));
}

#[test]
fn dispatch_child_bridge_is_thread_local_and_scope_cleared() {
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let ids = Arc::new(Mutex::new(Vec::new()));
    let threads = (0..2)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            let ids = Arc::clone(&ids);
            std::thread::spawn(move || {
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .arg("--list")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap();
                let verified =
                    VerifiedChild::capture(child, PROCESS_OWNER_SESSION, PROCESS_GENERATION);
                let child_id = verified.child_id();
                let scope = dispatch_child_scope();
                publish_dispatch_child(&verified);
                barrier.wait();
                assert_eq!(
                    dispatch_child().map(|child| child.child_id()),
                    Some(child_id)
                );
                ids.lock().unwrap().push(child_id);
                barrier.wait();
                drop(scope);
                assert!(dispatch_child().is_none());
                let status = verified
                    .wait(
                        PROCESS_OWNER_SESSION,
                        PROCESS_GENERATION,
                        Duration::from_secs(5),
                        SHELL_TEARDOWN_GRACE,
                    )
                    .unwrap();
                assert!(status.success());
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    let ids = ids.lock().unwrap();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}
