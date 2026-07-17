//! Containment controller / classify / execute integration tests.

use super::*;
use std::sync::Arc;
use std::thread;

    #[test]
    fn light_execute_returns_busy_when_analysis_permit_held() {
        let path = analysis_permit_path();
        let _ = fs::remove_dir_all(&path);
        let slots = default_analysis_concurrency().max(1);
        let deadline = Instant::now() + Duration::from_secs(5);
        let holders: Vec<_> = (0..slots)
            .map(|idx| {
                MachinePermit::acquire_slots(
                    &path,
                    slots,
                    deadline,
                    &format!("test-analysis-holder-{idx}"),
                )
                .unwrap_or_else(|e| panic!("pre-hold analysis slot {idx}/{slots}: {e}"))
            })
            .collect();

        let opts = CodeModeOptions {
            hard_max_wall_ms: 120,
            ..CodeModeOptions::default()
        };
        let result = execute(
            "return {ok:true}",
            &opts,
            || CodeModeResult::completed(json!({"ok": true}), Vec::new(), 0, 0, 0),
        );
        let err = result
            .error
            .as_ref()
            .expect("expected busy error from held analysis permit");
        assert!(
            err.retryable,
            "analysis permit contention must be retryable: {err:?}"
        );
        assert!(
            err.kind == "busy" || err.message.contains("machine_permit_busy"),
            "unexpected error: {err:?}"
        );
        drop(holders);
    }

    #[test]
    fn status_plans_bypass_analysis_permit() {
        let path = analysis_permit_path();
        let _ = fs::remove_dir_all(&path);
        let slots = default_analysis_concurrency().max(1);
        let holder = MachinePermit::acquire_slots(
            &path,
            slots,
            Instant::now() + Duration::from_secs(5),
            "test-analysis-holder",
        )
        .expect("pre-hold analysis permit");

        let result = execute(
            "search: containment",
            &CodeModeOptions::default(),
            || CodeModeResult::completed(json!({"status": "ok"}), Vec::new(), 0, 0, 0),
        );
        assert!(
            result.error.is_none(),
            "status/search catalog plans must stay ungated: {result:?}"
        );
        drop(holder);
    }

    #[test]
    fn expand_only_plans_bypass_analysis_permit() {
        let path = analysis_permit_path();
        let _ = fs::remove_dir_all(&path);
        let slots = default_analysis_concurrency().max(1);
        let holders: Vec<_> = (0..slots)
            .map(|idx| {
                MachinePermit::acquire_slots(
                    &path,
                    slots,
                    Instant::now() + Duration::from_secs(5),
                    &format!("test-expand-bypass-holder-{idx}"),
                )
                .unwrap_or_else(|e| panic!("pre-hold analysis slot {idx}/{slots}: {e}"))
            })
            .collect();

        let result = execute(
            r#"return await zero.expand("tz://blob/deadbeef")"#,
            &CodeModeOptions::default(),
            || CodeModeResult::completed(json!({"text": "ok"}), Vec::new(), 0, 0, 0),
        );
        assert!(
            result.error.is_none(),
            "expand-only recovery must not wait on analysis permit: {result:?}"
        );
        drop(holders);
    }

    #[test]
    fn classify_expand_only_as_status_keeps_mixed_light() {
        assert_eq!(
            classify(r#"return await zero.expand("tz://blob/abc")"#, 32),
            ExecutionClass::Status
        );
        assert_eq!(
            classify(r#"return await zero.token.expand(ref)"#, 32),
            ExecutionClass::Status
        );
        assert_eq!(
            classify(
                r#"return await zero.token.expandMany(["tz://blob/a","tz://blob/b"])"#,
                32
            ),
            ExecutionClass::Status
        );
        assert_eq!(
            classify(
                r#"const e = await zero.expand(c.ref); return e;"#,
                32
            ),
            ExecutionClass::Status
        );
        // Expand + find/search must stay analysis-gated.
        assert_eq!(
            classify(
                r#"const hits = await zero.find("x"); return await zero.expand(hits.ref);"#,
                32
            ),
            ExecutionClass::Light
        );
        assert_eq!(
            classify(
                r#"await zero.fs.compound("read", {path: "src/expand.rs"})"#,
                32
            ),
            ExecutionClass::Light
        );
    }

    #[test]
    fn default_analysis_concurrency_is_core_budgeted() {
        let cores = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(4);
        let got = default_analysis_concurrency();
        let expect = (cores / 4).clamp(1, DEFAULT_ANALYSIS_CONCURRENCY_CAP);
        assert_eq!(got, expect);
        assert!(got >= 1);
    }

    #[test]
    fn default_index_concurrency_is_core_budgeted() {
        let cores = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(4);
        let got = default_index_concurrency();
        let expect = (cores / 8).clamp(1, DEFAULT_INDEX_CONCURRENCY_CAP);
        assert_eq!(got, expect);
        assert!(got >= 1);
        assert!(got <= DEFAULT_INDEX_CONCURRENCY_CAP);
    }

    #[test]
    fn index_execute_returns_busy_when_index_permit_held() {
        let path = index_permit_path();
        let _ = fs::remove_dir_all(&path);
        let slots = default_index_concurrency().max(1);
        let deadline = Instant::now() + Duration::from_secs(5);
        let holders: Vec<_> = (0..slots)
            .map(|idx| {
                MachinePermit::acquire_slots(
                    &path,
                    slots,
                    deadline,
                    &format!("test-index-holder-{idx}"),
                )
                .unwrap_or_else(|e| panic!("pre-hold index slot {idx}/{slots}: {e}"))
            })
            .collect();

        let opts = CodeModeOptions {
            hard_max_wall_ms: 120,
            ..CodeModeOptions::default()
        };
        let result = execute(
            "await zero.token.index({rebuild:true})",
            &opts,
            || CodeModeResult::completed(json!({"ok": true}), Vec::new(), 0, 0, 0),
        );
        let err = result
            .error
            .as_ref()
            .expect("expected busy error from held index permit");
        assert!(
            err.retryable,
            "index permit contention must be retryable: {err:?}"
        );
        assert!(
            err.kind == "busy" || err.message.contains("machine_permit_busy"),
            "unexpected error: {err:?}"
        );
        drop(holders);
    }

    #[test]
    fn classify_routes_index_markers_before_light() {
        assert_eq!(
            classify("await zero.fs.index({path:'.'})", 32),
            ExecutionClass::Index
        );
        assert_eq!(
            classify("await watch.drain()", 32),
            ExecutionClass::Index
        );
        assert_eq!(
            classify("await zero.token.index({rebuild:true})", 32),
            ExecutionClass::Index
        );
        assert_eq!(
            classify("return {ok:true}", 32),
            ExecutionClass::Light
        );
        assert_eq!(
            classify("await zero.token.shell('ls')", 32),
            ExecutionClass::HeavyShell
        );
    }

    #[test]
    fn classify_rejects_bare_status_metrics_rebuild_false_positives() {
        // Paths / return shapes / prose must not ungate Status or steal Index.
        assert_eq!(
            classify(
                r#"await zero.fs.compound("read", {path: "docs/status.md"})"#,
                32
            ),
            ExecutionClass::Light
        );
        assert_eq!(
            classify(r#"return {status: "ok", metrics: 1}"#, 32),
            ExecutionClass::Light
        );
        assert_eq!(
            classify(r#"await zero.token.shell("echo status metrics")"#, 32),
            ExecutionClass::HeavyShell
        );
        assert_eq!(
            classify(r#"return {note: "rebuild later"}"#, 32),
            ExecutionClass::Light
        );
        assert_eq!(
            classify("await rebuild_index()", 32),
            ExecutionClass::Light
        );
        // API-shaped markers still classify correctly.
        assert_eq!(classify("codemode.status", 32), ExecutionClass::Status);
        assert_eq!(classify("search: containment", 32), ExecutionClass::Status);
        assert_eq!(classify("describe:zero.read", 32), ExecutionClass::Status);
        assert_eq!(
            classify("await zero.token.index({rebuild:true})", 32),
            ExecutionClass::Index
        );
    }

    #[test]
    fn snapshot_exposes_index_max_active() {
        let snap = snapshot();
        assert!(
            snap.get("index_max_active")
                .and_then(|v| v.as_u64())
                .is_some_and(|v| v >= 1),
            "snapshot must expose index_max_active: {snap}"
        );
    }

    #[test]
    fn map_acquire_error_distinguishes_busy_vs_fatal() {
        let busy = map_acquire_error(AcquireError::Busy(
            "codemode permit /tmp/x is held by live process(es) across 1 slots".into(),
        ));
        let busy_err = busy
            .error
            .as_ref()
            .expect("busy mapping must produce an error");
        assert!(
            busy_err.retryable,
            "live-holder timeout must stay retryable: {busy_err:?}"
        );
        assert_eq!(busy_err.kind, "busy");
        assert!(
            busy_err.message.contains("machine_permit_busy"),
            "unexpected busy mapping: {busy_err:?}"
        );

        let fatal = map_acquire_error(AcquireError::Fatal(
            "create codemode permit /tmp/x/slot-0: Permission denied (os error 13)".into(),
        ));
        let fatal_err = fatal
            .error
            .as_ref()
            .expect("fatal mapping must produce an error");
        assert!(
            !fatal_err.retryable,
            "Fatal permit I/O must not be retryable: {fatal_err:?}"
        );
        assert_eq!(fatal_err.kind, "substrate");
        assert!(
            fatal_err.message.contains("machine_permit_io"),
            "unexpected fatal mapping: {fatal_err:?}"
        );
        assert!(
            !fatal_err.message.contains("machine_permit_busy"),
            "Fatal must not be labeled busy: {fatal_err:?}"
        );
    }

    #[test]
    fn in_process_analysis_slot_wait_returns_busy_on_wall_deadline() {
        let ctrl = Controller::new(Config {
            max_active: 1,
            max_queue_depth: 8,
            cost_threshold: 32,
            analysis_max_active: 1,
            index_max_active: 1,
        });
        let hold = ctrl
            .acquire_slot(
                ExecutionClass::Light,
                Instant::now() + Duration::from_secs(30),
            )
            .expect("holder acquires analysis slot");
        let started = Instant::now();
        let err = ctrl
            .acquire_slot(
                ExecutionClass::Light,
                Instant::now() + Duration::from_millis(80),
            )
            .expect_err("contender must not hang past wall deadline");
        let elapsed = started.elapsed();
        drop(hold);
        let busy = err.error.as_ref().expect("busy error");
        assert!(busy.retryable, "deadline busy must be retryable: {busy:?}");
        assert_eq!(busy.kind, "busy");
        assert!(
            busy.message.contains("analysis_queue_busy"),
            "unexpected busy code: {busy:?}"
        );
        assert!(
            elapsed < Duration::from_millis(1500),
            "wall-bounded wait must return promptly, took {elapsed:?}"
        );
    }

    #[test]
    fn analysis_in_process_slots_independent_of_heavy_max_active() {
        let ctrl = Controller::new(Config {
            max_active: 1,
            max_queue_depth: 8,
            cost_threshold: 32,
            analysis_max_active: 2,
            index_max_active: 1,
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let heavy = ctrl
            .acquire_slot(ExecutionClass::HeavyShell, deadline)
            .expect("heavy slot");
        let a = ctrl
            .acquire_slot(ExecutionClass::Light, deadline)
            .expect("analysis must not wait on heavy max_active=1");
        let b = ctrl
            .acquire_slot(ExecutionClass::Light, deadline)
            .expect("second analysis slot within analysis_max_active");
        let contested = ctrl.acquire_slot(
            ExecutionClass::Light,
            Instant::now() + Duration::from_millis(80),
        );
        assert!(
            contested.is_err(),
            "third analysis must busy at analysis_max_active=2: {contested:?}"
        );
        drop(a);
        drop(b);
        drop(heavy);
    }

    #[test]
    fn in_process_slot_wait_wakes_when_holder_releases() {
        let ctrl = Arc::new(Controller::new(Config {
            max_active: 1,
            max_queue_depth: 8,
            cost_threshold: 32,
            analysis_max_active: 1,
            index_max_active: 1,
        }));
        let hold = ctrl
            .acquire_slot(
                ExecutionClass::Index,
                Instant::now() + Duration::from_secs(30),
            )
            .expect("index holder");
        let ctrl_waiter = Arc::clone(&ctrl);
        let waiter = thread::spawn(move || {
            let slot = ctrl_waiter
                .acquire_slot(
                    ExecutionClass::Index,
                    Instant::now() + Duration::from_secs(2),
                )
                .expect("waiter must acquire after release");
            drop(slot);
        });
        thread::sleep(Duration::from_millis(50));
        drop(hold);
        waiter.join().expect("waiter thread");
    }

    #[test]
    fn snapshot_exposes_separate_in_process_actives() {
        let snap = snapshot();
        assert!(
            snap.get("active_analysis").and_then(|v| v.as_u64()).is_some(),
            "snapshot must expose active_analysis: {snap}"
        );
        assert!(
            snap.get("active_index").and_then(|v| v.as_u64()).is_some(),
            "snapshot must expose active_index: {snap}"
        );
    }

