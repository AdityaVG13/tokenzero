use super::*;

fn hit(line: usize, text: &str) -> RecallHit {
    RecallHit {
        ref_id: "tz://blob/abc".to_string(),
        label: "unknown".to_string(),
        line,
        text: text.to_string(),
    }
}

#[test]
fn grouped_search_recall_factors_repeated_ref_and_root_losslessly() {
    let hits = vec![
        hit(1, "/tmp/repo/src/a.rs:10:alpha"),
        hit(2, "/tmp/repo/src/b.rs:20:beta"),
        hit(3, "/tmp/repo/tests/c.rs:30:gamma"),
    ];
    let rendered = render_hits(&hits);
    assert_eq!(rendered.matches("tz://blob/abc").count(), 1, "{rendered}");
    let mut lines = rendered.lines();
    assert_eq!(lines.next(), Some("tz://blob/abc unknown#L1-3"));
    assert_eq!(lines.next(), Some("# root: /tmp/repo/"));
    let recovered = lines
        .map(|line| format!("/tmp/repo/{line}"))
        .collect::<Vec<_>>();
    assert_eq!(
        recovered,
        hits.iter().map(|hit| hit.text.clone()).collect::<Vec<_>>()
    );
    let flat = hits
        .iter()
        .map(|hit| format!("{} {}:{}: {}", hit.ref_id, hit.label, hit.line, hit.text))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.len() < flat.len());
    assert!(tokenzero_core::count_tokens(&rendered) < tokenzero_core::count_tokens(&flat));
}

#[test]
fn windows_drive_root_keeps_the_separator() {
    let hits = vec![hit(4, r"C:\a.rs:10:alpha"), hit(9, r"C:\b.rs:20:beta")];
    let rendered = render_hits(&hits);
    let mut lines = rendered.lines();
    assert_eq!(lines.next(), Some("tz://blob/abc unknown#L4,9"));
    assert_eq!(lines.next(), Some(r"# root: C:\"));
    assert_eq!(lines.next(), Some("a.rs:10:alpha"));
    assert_eq!(lines.next(), Some("b.rs:20:beta"));
}

#[test]
fn shared_prose_separator_is_not_mislabeled_as_a_path_root() {
    let hits = vec![
        hit(2, "docs/routing explains alpha"),
        hit(5, "docs/routing explains beta"),
    ];
    let rendered = render_hits(&hits);
    assert!(!rendered.contains("# root:"), "{rendered}");
    assert!(rendered.contains("#L2,5"), "{rendered}");
    assert!(
        rendered.contains("2: docs/routing explains alpha"),
        "{rendered}"
    );
    assert!(
        rendered.contains("5: docs/routing explains beta"),
        "{rendered}"
    );
}

#[test]
fn single_recall_hit_keeps_flat_shape_when_grouping_is_not_cheaper() {
    let hits = vec![hit(7, "/tmp/repo/src/a.rs:10:alpha")];
    assert_eq!(
        render_hits(&hits),
        "tz://blob/abc unknown:7: /tmp/repo/src/a.rs:10:alpha"
    );
}

#[test]
fn repeated_suffix_is_factored_and_reconstructs_exactly() {
    let hits = vec![
        hit(
            1,
            "/tmp/repo/mod_0000/file_0000_000.rs:500:// line 0499 pub fn BENCH_NEEDLE_FN(x: usize) -> bool { true }",
        ),
        hit(
            2,
            "/tmp/repo/mod_0002/file_0002_000.rs:500:// line 0499 pub fn BENCH_NEEDLE_FN(x: usize) -> bool { true }",
        ),
    ];
    let rendered = render_hits(&hits);
    let mut lines = rendered.lines();
    assert_eq!(lines.next(), Some("tz://blob/abc unknown#L1-2"));
    assert_eq!(lines.next(), Some("# root: /tmp/repo/"));
    assert_eq!(
        lines.next(),
        Some("# suffix: :500:// line 0499 pub fn BENCH_NEEDLE_FN(x: usize) -> bool { true }")
    );
    assert_eq!(lines.next(), Some("mod_0000/file_0000_000.rs"));
    assert_eq!(lines.next(), Some("mod_0002/file_0002_000.rs"));
    assert!(lines.next().is_none(), "{rendered}");
    let mut rows = rendered
        .lines()
        .skip_while(|line| !line.starts_with("mod_"));
    let root = "/tmp/repo/";
    let suffix = ":500:// line 0499 pub fn BENCH_NEEDLE_FN(x: usize) -> bool { true }";
    for hit in &hits {
        let row = rows.next().expect("row for hit");
        assert_eq!(format!("{root}{row}{suffix}"), hit.text);
    }
    let flat = hits
        .iter()
        .map(|hit| format!("{} {}:{}: {}", hit.ref_id, hit.label, hit.line, hit.text))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.len() < flat.len());
    assert!(tokenzero_core::count_tokens(&rendered) < tokenzero_core::count_tokens(&flat));
}

#[test]
fn unequal_suffixes_keep_root_only_projection() {
    let hits = vec![
        hit(1, "/tmp/repo/src/a.rs:10:alpha"),
        hit(2, "/tmp/repo/src/b.rs:20:beta"),
    ];
    let rendered = render_hits(&hits);
    assert!(!rendered.contains("# suffix:"), "{rendered}");
    assert!(rendered.contains("# root: /tmp/repo/src/"), "{rendered}");
    assert!(rendered.contains("a.rs:10:alpha"), "{rendered}");
    assert!(rendered.contains("b.rs:20:beta"), "{rendered}");
}
