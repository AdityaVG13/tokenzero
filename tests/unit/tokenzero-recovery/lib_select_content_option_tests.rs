    use super::*;

    #[test]
    fn malformed_around_selector_does_not_default_to_line_one() {
        let content = "one\ntwo\nthree\nfour\nfive\n".to_string();
        assert_eq!(
            select_content(
                content.clone(),
                Some("around:not-a-line"),
                None,
                None,
                None,
                None
            ),
            content,
            "garbage around: must not silently slice around line 1"
        );
    }

    #[test]
    fn malformed_selector_does_not_clear_explicit_line_window() {
        let content = "one\ntwo\nthree\nfour\nfive\n".to_string();
        assert_eq!(
            select_content(
                content.clone(),
                Some("around:xyz:1"),
                Some(5),
                Some(5),
                None,
                None
            ),
            "five\n"
        );
        assert_eq!(
            select_content(content, Some("range:nope"), Some(2), Some(2), None, None),
            "two\n"
        );
    }

    #[test]
    fn omitted_around_radius_still_defaults_to_three() {
        let content = "1\n2\n3\n4\n5\n6\n7\n8\n9\n".to_string();
        assert_eq!(
            select_content(content, Some("around:5"), None, None, None, None),
            "2\n3\n4\n5\n6\n7\n8\n"
        );
    }

    #[test]
    fn empty_content_has_zero_lines_and_rejects_line_one() {
        assert_eq!(content_line_count(""), 0);
        let mut end = Some(1usize);
        let err = clamp_line_window("", Some(1), &mut end).expect_err("L1 on empty");
        assert!(
            err.contains("line_count=0"),
            "0-byte blob must not look like one empty line: {err}"
        );
    }

    #[test]
    fn around_zero_radius_is_the_named_line() {
        let content = "1\n2\n3\n".to_string();
        assert_eq!(
            select_content(content, Some("around:2:0"), None, None, None, None),
            "2\n"
        );
    }

    #[test]
    fn zero_based_range_selector_does_not_slice_line_one() {
        let content = "one\ntwo\nthree\nfour\nfive\n".to_string();
        assert_eq!(
            select_content(content.clone(), Some("range:0-2"), None, None, None, None),
            content,
            "range:0 must not silently remap to line 1"
        );
        assert_eq!(
            select_content(content, Some("range:0-2"), Some(5), Some(5), None, None),
            "five\n",
            "malformed zero range must not clear an explicit window"
        );
    }

    #[test]
    fn zero_line_selector_does_not_slice_line_one() {
        let content = "one\ntwo\nthree\n".to_string();
        assert_eq!(
            select_content(content.clone(), Some("line:0"), None, None, None, None),
            content
        );
    }

