    use super::*;

    #[test]
    fn zero_byte_blob_has_no_line_one() {
        let err = apply_fragment_to_bytes(
            b"",
            &ZeroRefFragment::Line { start: 1, end: 1 },
        )
        .expect_err("empty blob has 0 lines");
        assert!(
            matches!(err, TokenZeroStoreError::Fragment(ref reason) if reason.contains("fragment-out-of-range")),
            "{err}"
        );
        assert!(
            err.to_string().contains("lines=0"),
            "must report line_count 0, not split_inclusive's empty remainder: {err}"
        );
    }

    #[test]
    fn zero_byte_blob_allows_empty_exclusive_byte_range() {
        let got = apply_fragment_to_bytes(b"", &ZeroRefFragment::Byte { start: 0, end: 0 })
            .expect("B0-0 on empty is a valid empty exclusive range");
        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn last_line_without_newline_is_inclusive() {
        let got = apply_fragment_to_bytes(
            b"a\nb",
            &ZeroRefFragment::Line { start: 2, end: 2 },
        )
        .expect("last line without terminator");
        assert_eq!(got, b"b");
    }

