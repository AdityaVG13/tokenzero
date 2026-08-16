    use super::*;
    use std::sync::Mutex;

    #[test]
    fn engine_lock_recovers_after_poison() {
        let dir = tempfile::tempdir().expect("tempdir");
        let engine = Mutex::new(TokenZeroEngine::new(EngineConfig::for_root(dir.path())));
        let poisoned = catch_unwind(AssertUnwindSafe(|| {
            let _guard = engine.lock().expect("fresh lock");
            panic!("poison engine");
        }));
        assert!(poisoned.is_err());
        assert!(
            engine.lock().is_err(),
            "mutex must be poisoned before recovery"
        );
        let recovered = lock_engine(&engine);
        assert!(
            !recovered.session_id().is_empty(),
            "poison recover must yield a usable engine"
        );
        drop(recovered);
        let _again = lock_engine(&engine);
    }

    #[test]
    fn dispatch_catching_maps_panic_to_runtime_error() {
        let error = dispatch_catching::<()>("tz_read", || panic!("boom")).expect_err("panic");
        assert_eq!(error.kind, "runtime");
        assert_eq!(error.op.as_deref(), Some("tz_read"));
        assert!(
            error.message.contains("panicked") && error.message.contains("boom"),
            "panic payload must stay on the wire: {}",
            error.message
        );
    }

