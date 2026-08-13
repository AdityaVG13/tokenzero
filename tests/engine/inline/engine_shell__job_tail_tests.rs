use super::*;

fn job_with_handle(log: PathBuf, file: fs::File) -> BackgroundJob {
    BackgroundJob {
        id: "job-read-test".to_string(),
        sequence: 0,
        log,
        log_file: Arc::new(Mutex::new(file)),
        state: Mutex::new(BackgroundJobState {
            status: "running",
            pid: None,
            child: None,
            exit_code: None,
            version: 0,
            completed_at: None,
            terminate_requested: false,
            log_error: None,
        }),
        changed: Condvar::new(),
        poll_interleave: None,
    }
}

#[test]
fn retained_log_handle_reads_only_the_requested_window() {
    let mut file = tempfile::tempfile().unwrap();
    let bytes = vec![b'x'; MAX_JOB_TAIL_BYTES * 4];
    file.write_all(&bytes).unwrap();
    file.flush().unwrap();
    let job = job_with_handle(PathBuf::from("retained.log"), file);

    let (window, start, log_bytes) = read_job_window(&job, 0, MAX_JOB_TAIL_BYTES).unwrap();
    assert_eq!(start, 0);
    assert_eq!(window.len(), MAX_JOB_TAIL_BYTES);
    assert_eq!(log_bytes, bytes.len());
}

#[test]
fn a_chunk_published_between_length_and_relock_does_not_lose_its_wake() {
    let file = tempfile::tempfile().unwrap();
    let hook = Arc::new(PollInterleave {
        length_observed: std::sync::Barrier::new(2),
        publication_done: std::sync::Barrier::new(2),
    });
    let mut observed = job_with_handle(PathBuf::from("interleaved.log"), file);
    observed.poll_interleave = Some(Arc::clone(&hook));
    let observed = Arc::new(observed);
    let registry = BackgroundJobRegistry::default();
    registry.insert_bounded(Arc::clone(&observed)).unwrap();

    let writer_job = Arc::clone(&observed);
    let writer = thread::spawn(move || {
        hook.length_observed.wait();
        {
            let mut log = lock(&writer_job.log_file);
            log.write_all(b"ready").unwrap();
            log.flush().unwrap();
        }
        let mut state = lock(&writer_job.state);
        state.version = state.version.saturating_add(1);
        drop(state);
        writer_job.changed.notify_all();
        hook.publication_done.wait();
    });

    let started = Instant::now();
    let result = registry
        .poll(&observed.id, Duration::from_secs(2), 0, 16)
        .unwrap();
    writer.join().unwrap();
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(result["tail"], "ready");
    assert_eq!(result["cursor"], 5);
    assert_eq!(result["changed"], true);
}

#[test]
fn retained_log_read_failure_is_loud() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("write-only.log");
    fs::write(&path, b"unreadable through this handle").unwrap();
    let file = OpenOptions::new().write(true).open(&path).unwrap();
    let job = job_with_handle(path, file);

    let error = read_job_window(&job, 0, 16).unwrap_err();
    assert!(error.contains("read background log"), "{error}");
}

#[test]
fn invalid_three_byte_tail_is_not_lossless_when_replacement_length_matches() {
    let truncated_four_byte_scalar = [0xf0, 0x90, 0x80];
    let (tail, lossless, consumed) = decode_job_tail(&truncated_four_byte_scalar);

    assert_eq!(consumed, truncated_four_byte_scalar.len());
    assert_eq!(tail.len(), truncated_four_byte_scalar.len());
    assert_eq!(tail, "�");
    assert!(
        !lossless,
        "serialized length is not UTF-8 validity evidence"
    );
}

#[test]
fn binary_tail_stays_typed_bounded_and_advances_by_consumed_raw_bytes() {
    let binary = vec![0xff; MAX_JOB_TAIL_BYTES];
    let (tail, lossless, consumed) = decode_job_tail(&binary);

    assert!(!lossless);
    assert!(tail.len() <= MAX_JOB_TAIL_BYTES);
    assert_eq!(consumed, MAX_JOB_TAIL_BYTES / 3);
    assert!(consumed > 0 && consumed < binary.len());

    let (_, _, next_consumed) = decode_job_tail(&binary[consumed..]);
    assert!(next_consumed > 0, "the next cursor must keep progressing");
}
