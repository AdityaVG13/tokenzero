use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn missing_spawn_pipe_is_a_typed_runtime_error() {
    let error = required_child_pipe::<()>(None, "stdout").unwrap_err();
    assert!(matches!(&error, RuntimeError::MissingPipe("stdout")));
    assert_eq!(
        error.to_string(),
        "spawned command stdout pipe is unavailable"
    );
}

#[test]
fn standalone_posix_builtins_are_planned_through_the_shell() {
    for argv in [
        vec!["exit".to_string(), "7".to_string()],
        vec!["cd".to_string(), "..".to_string()],
        vec!["export".to_string(), "A=1".to_string()],
    ] {
        let plan = plan_command_for_platform(&argv, None, false, "posix").unwrap();
        assert_eq!(plan.execution_mode, ExecutionMode::Shell);
        assert_eq!(plan.argv[0], "/bin/bash");
    }
}

#[cfg(unix)]
#[test]
fn command_pipes_capture_both_streams_and_write_stdin() {
    let argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "cat; printf err >&2".to_string(),
    ];
    let result = run_command(
        &argv,
        None,
        None,
        Some("input"),
        Duration::from_secs(2),
        false,
    )
    .unwrap();
    assert!(result.ok);
    assert_eq!(result.stdout, "input");
    assert_eq!(result.stderr, "err");
    assert!(result.stdout_capture.captured_utf8_lossless);
    assert!(result.stderr_capture.captured_utf8_lossless);
    assert_eq!(
        result.stdout_capture.full_stream_sha256.as_deref(),
        Some(tokenzero_core::sha256_hex("input").as_str())
    );
    assert_eq!(
        result.stderr_capture.full_stream_sha256.as_deref(),
        Some(tokenzero_core::sha256_hex("err").as_str())
    );
}

#[cfg(unix)]
#[test]
fn invalid_utf8_capture_is_never_labeled_lossless() {
    let argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "printf '\\377'".to_string(),
    ];
    let result = run_command(&argv, None, None, None, Duration::from_secs(2), false).unwrap();
    assert!(result.ok);
    assert!(!result.stdout_capture.captured_utf8_lossless);
    assert_eq!(result.stdout_capture.bytes_seen, 1);
    assert_eq!(
        result.stdout_capture.full_stream_sha256.as_deref(),
        Some(lowercase_hex(&Sha256::digest([0xff])).as_str())
    );
}
#[cfg(unix)]
#[test]
fn request_cancellation_terminates_the_owned_shell_tree() {
    let argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "sleep 60".to_string(),
    ];
    let cancelled = Arc::new(AtomicBool::new(false));
    let trigger = Arc::clone(&cancelled);
    let trigger_thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        trigger.store(true, Ordering::Release);
    });
    let started = Instant::now();
    let error = run_command_with_policy_observers_with_child_and_cancel(
        &argv,
        None,
        None,
        None,
        Duration::from_secs(30),
        false,
        RunOutputPolicy::default(),
        |_, _, _| {},
        |_, _| {},
        |_| {},
        || cancelled.load(Ordering::Acquire),
    )
    .expect_err("cancelled command must not complete successfully");
    trigger_thread.join().unwrap();
    assert!(matches!(error, RuntimeError::Cancelled));
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "cancellation took {:?}",
        started.elapsed()
    );
}

#[test]
fn io_pool_reuses_threads_across_one_hundred_jobs() {
    let before = io_pool_spawned_threads();
    for i in 0..100 {
        let worker = spawn_io_worker("pool probe", move || Ok(i));
        let result = worker.receiver.recv().expect("pooled job result");
        assert_eq!(result.expect("job ok"), i);
    }
    let after = io_pool_spawned_threads();
    assert_eq!(after, io_pool_size());
    if before > 0 {
        assert_eq!(after, before);
    }
}
