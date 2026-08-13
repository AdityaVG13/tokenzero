use super::*;

fn job(sequence: u64, status: &'static str) -> Arc<BackgroundJob> {
    Arc::new(BackgroundJob {
        id: format!("job-{sequence}"),
        sequence,
        log: PathBuf::from(format!("job-{sequence}.log")),
        log_file: Arc::new(Mutex::new(tempfile::tempfile().unwrap())),
        state: Mutex::new(BackgroundJobState {
            status,
            pid: None,
            child: None,
            exit_code: None,
            version: u64::from(status != "running"),
            completed_at: (status != "running").then(Instant::now),
            terminate_requested: false,
            log_error: None,
        }),
        changed: Condvar::new(),
    })
}

#[test]
fn terminating_registry_rejects_late_background_launches() {
    let registry = BackgroundJobRegistry::default();
    registry.terminate_all();
    let error = registry
        .start(
            vec!["never-spawn".to_string()],
            None,
            BTreeMap::new(),
            Duration::from_secs(1),
            PathBuf::from("unused"),
        )
        .unwrap_err();
    assert_eq!(
        error,
        "background jobs are unavailable during session teardown"
    );
}

#[test]
fn expired_terminal_jobs_are_pruned_but_recent_terminal_jobs_are_retained() {
    let registry = BackgroundJobRegistry::default();
    let expired = job(0, "exited");
    lock(&expired.state).completed_at =
        Instant::now().checked_sub(COMPLETED_JOB_TTL + Duration::from_secs(1));
    lock(&registry.jobs).insert(expired.id.clone(), expired);
    registry.insert_bounded(job(1, "exited")).unwrap();

    let retained = lock(&registry.jobs);
    assert!(!retained.contains_key("job-0"));
    assert!(retained.contains_key("job-1"));
}

#[test]
fn background_registry_evicts_completed_and_rejects_all_running() {
    let registry = BackgroundJobRegistry::default();
    for sequence in 0..MAX_BACKGROUND_JOBS as u64 {
        registry.insert_bounded(job(sequence, "exited")).unwrap();
    }
    registry
        .insert_bounded(job(MAX_BACKGROUND_JOBS as u64, "exited"))
        .unwrap();
    let retained = registry.jobs.lock().unwrap();
    assert_eq!(retained.len(), MAX_BACKGROUND_JOBS);
    assert!(!retained.contains_key("job-0"));
    drop(retained);

    let running = BackgroundJobRegistry::default();
    for sequence in 0..MAX_BACKGROUND_JOBS as u64 {
        running.insert_bounded(job(sequence, "running")).unwrap();
    }
    let error = running
        .insert_bounded(job(MAX_BACKGROUND_JOBS as u64, "running"))
        .unwrap_err();
    assert!(error.contains("registry is full"));
    assert_eq!(running.jobs.lock().unwrap().len(), MAX_BACKGROUND_JOBS);
}
