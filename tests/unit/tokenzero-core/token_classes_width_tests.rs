    use super::*;

    #[test]
    fn savings_ratio_typed_does_not_truncate_u64_counts() {
        let raw = Tok::<Raw>::new(u64::from(u32::MAX) + 100);
        let visible = Tok::<Visible>::new(100);
        let ratio = savings_ratio_typed(raw, visible);
        assert!(
            ratio > 0.999,
            "u64 token counts must not collapse through usize: {ratio}"
        );
    }

