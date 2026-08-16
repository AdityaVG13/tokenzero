    use super::*;

    #[test]
    fn retained_floor_rejects_u64_width_loss() {
        assert_eq!(retained_floor(100), Some(99));
        assert_eq!(retained_floor(u64::MAX), None);
        EvictionSlackGuard::new(u64::MAX, u64::MAX)
            .unwrap()
            .guard_eviction(1)
            .expect_err("overflowing 99% floor must refuse eviction");
    }

    #[test]
    fn slack_ppm_does_not_truncate_u64_to_negative_i64() {
        let guard = EvictionSlackGuard::new(u64::MAX, 1).unwrap();
        assert_eq!(
            guard.slack_ppm(),
            i64::MAX,
            "PPM above i64::MAX must saturate, not wrap negative"
        );
    }

