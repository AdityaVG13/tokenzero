use super::*;

#[cfg(unix)]
#[test]
fn unix_descendant_holding_pipes_returns_without_detaching_readers() {
    // Child exits immediately while a descendant keeps both pipes open.
    // Process-group terminate must close those writers so readers join
    // inside the cleanup/join grace instead of being detached on Drop.
    let argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "sleep 60 >/dev/null 2>&1 & exit 0".to_string(),
    ];
    let started = Instant::now();
    let result = run_command(&argv, None, None, None, Duration::from_secs(2), false)
        .expect("run_command should return");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "cleanup+join must finish well under the descendant sleep, took {:?}",
        started.elapsed()
    );
    assert!(result.ok || result.io_grace_expired || result.timed_out);
}
