use super::*;

#[test]
fn poll_returns_output_change_then_terminal_state() {
    let dir = tempfile::tempdir().unwrap();
    let registry = BackgroundJobRegistry::default();
    let launched = registry
        .start(
            vec![
                "/bin/bash".to_string(),
                "-c".to_string(),
                "sleep 0.05; printf done".to_string(),
            ],
            None,
            BTreeMap::new(),
            Duration::from_secs(2),
            dir.path().to_path_buf(),
        )
        .unwrap();
    let id = launched["job"].as_str().unwrap();
    let output = registry
        .poll(id, Duration::from_secs(1), 0, DEFAULT_JOB_TAIL_BYTES)
        .unwrap();
    assert!(output["tail"].as_str().unwrap().contains("done"));
    let cursor = output["cursor"].as_u64().unwrap() as usize;
    let terminal = if output["status"] == "exited" {
        output
    } else {
        registry
            .poll(id, Duration::from_secs(1), cursor, DEFAULT_JOB_TAIL_BYTES)
            .unwrap()
    };
    assert_eq!(terminal["status"], "exited");
    assert_eq!(
        registry
            .poll(id, Duration::ZERO, cursor, DEFAULT_JOB_TAIL_BYTES)
            .unwrap()["tail"],
        ""
    );
}

#[test]
fn poll_returns_live_bounded_delta_before_exit() {
    let dir = tempfile::tempdir().unwrap();
    let registry = BackgroundJobRegistry::default();
    let launched = registry
        .start(
            vec![
                "/bin/bash".to_string(),
                "-c".to_string(),
                "printf first; sleep 0.2; printf second".to_string(),
            ],
            None,
            BTreeMap::new(),
            Duration::from_secs(2),
            dir.path().to_path_buf(),
        )
        .unwrap();
    let id = launched["job"].as_str().unwrap();

    let first = registry.poll(id, Duration::from_secs(1), 0, 5).unwrap();
    assert_eq!(first["tail"], "first");
    assert_eq!(first["tailBytes"], 5);
    assert_eq!(first["cursor"], 5);
    assert!(first["version"].as_u64().unwrap() >= 1);

    let second = registry.poll(id, Duration::from_secs(1), 5, 64).unwrap();
    assert!(second["tail"].as_str().unwrap().contains("second"));
    let cursor = second["cursor"].as_u64().unwrap() as usize;
    assert!(cursor > 5);
    let terminal = registry
        .poll(id, Duration::from_secs(1), cursor, 64)
        .unwrap();
    assert_eq!(terminal["status"], "exited");
}

#[test]
fn unchanged_poll_is_tiny_and_supplies_backoff() {
    let dir = tempfile::tempdir().unwrap();
    let registry = BackgroundJobRegistry::default();
    let launched = registry
        .start(
            vec!["sleep".to_string(), "1".to_string()],
            None,
            BTreeMap::new(),
            Duration::from_secs(2),
            dir.path().to_path_buf(),
        )
        .unwrap();
    let id = launched["job"].as_str().unwrap();

    let observed = registry
        .poll(id, Duration::from_millis(10), 0, DEFAULT_JOB_TAIL_BYTES)
        .unwrap();
    assert_eq!(observed["status"], "running");
    assert_eq!(observed["unchanged"], true);
    assert_eq!(observed["nextPollMs"], UNCHANGED_NEXT_POLL_MS);
    assert!(observed.get("tail").is_none());
    assert!(observed.get("log").is_none());
}

#[test]
fn five_minute_silent_job_needs_at_most_seven_visible_observations() {
    let five_minutes_ms = 5 * 60 * 1_000_u64;
    let observation_cycle_ms = 30_000 + UNCHANGED_NEXT_POLL_MS;
    assert!(five_minutes_ms.div_ceil(observation_cycle_ms) <= 7);
}

#[test]
fn registry_drop_kills_running_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let registry = BackgroundJobRegistry::default();
    let launched = registry
        .start(
            vec!["sleep".to_string(), "30".to_string()],
            None,
            BTreeMap::new(),
            Duration::from_secs(60),
            dir.path().to_path_buf(),
        )
        .unwrap();
    let id = launched["job"].as_str().unwrap();
    let pid = (0..50)
        .find_map(|_| {
            let pid = registry
                .poll(id, Duration::ZERO, 0, DEFAULT_JOB_TAIL_BYTES)
                .unwrap()["pid"]
                .as_u64();
            if pid.is_none() {
                thread::sleep(Duration::from_millis(10));
            }
            pid
        })
        .expect("background child did not publish its pid") as u32;
    drop(registry);
    thread::sleep(Duration::from_millis(200));
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success());
    assert!(!alive, "background child {pid} survived registry drop");
}
