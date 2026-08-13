use super::*;

#[test]
fn tzgaou_guidance_hints_regex_literal_and_truncation() {
    assert!(looks_like_regex("alpha[0-9]+"));
    assert!(looks_like_regex("^foo$"));
    assert!(!looks_like_regex("fn alpha()"));
    assert!(!looks_like_regex("fn alpha() {"));
    assert!(looks_like_regex("x{2,4}"));
    assert!(!looks_like_regex("no_such_token"));
    assert_eq!(
        guidance_hint("grep", "alpha[0-9]+", false),
        Some("no regex match; try find for a literal")
    );
    assert_eq!(
        guidance_hint("find", "alpha[0-9]+", false),
        Some("find is literal; use grep for regex")
    );
    assert_eq!(guidance_hint("grep", "no_such_token", false), None);
    assert_eq!(
        guidance_hint("glob", "**/*.zig", true),
        Some("results truncated; narrow the path or raise max_files")
    );
    let note = with_guidance(
        "# grep alpha[0-9]+ — 0 matches".into(),
        "grep",
        "alpha[0-9]+",
        false,
    );
    assert!(note.starts_with("# grep alpha[0-9]+ — 0 matches"));
    assert!(note.contains("try find for a literal"));

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lib.rs"), "fn alpha() {}\n").unwrap();
    let engine = crate::TokenZeroEngine::new(crate::EngineConfig::for_root(dir.path()));
    let grep = engine.grep(
        "alpha[0-9]+",
        &[dir.path().to_path_buf()],
        Mode::Auto,
        20,
        4000,
    );
    let text = grep.visible.as_ref().unwrap().text.as_str();
    assert!(text.contains("0 matches"), "{text}");
    assert!(text.contains("try find for a literal"), "{text}");
    let find = engine.find(
        "alpha[0-9]+",
        &[dir.path().to_path_buf()],
        Mode::Auto,
        20,
        4000,
    );
    let text = find.visible.as_ref().unwrap().text.as_str();
    assert!(text.contains("0 matches"), "{text}");
    assert!(text.contains("use grep for regex"), "{text}");
    let literal = engine.grep(
        "no_such_token",
        &[dir.path().to_path_buf()],
        Mode::Auto,
        20,
        4000,
    );
    assert_eq!(
        literal.visible.as_ref().unwrap().text,
        "# grep no_such_token — 0 matches"
    );
}

#[test]
fn tzn67z_budget_exhausted_is_distinct_from_not_found() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "needle-one\n").unwrap();
    std::fs::write(dir.path().join("b.rs"), "needle-two\n").unwrap();
    let engine = crate::TokenZeroEngine::new(crate::EngineConfig::for_root(dir.path()));
    let roots = [dir.path().to_path_buf()];

    let miss = engine.find("no-such-token-n67z", &roots, Mode::Auto, 20, 4000);
    assert_eq!(miss.budget_exhausted, Some(false), "{miss:?}");
    assert!(
        miss.remaining_budget_tokens.is_some_and(|left| left > 0),
        "{miss:?}"
    );
    assert_ne!(
        miss.diagnostic.as_ref().map(|d| d.code.as_str()),
        Some("budget_exhausted")
    );
    assert!(
        miss.visible
            .as_ref()
            .is_some_and(|v| v.text.contains("0 matches")),
        "{miss:?}"
    );

    let exhausted = engine.find("no-such-token-n67z", &roots, Mode::Auto, 0, 4000);
    assert_eq!(exhausted.budget_exhausted, Some(true), "{exhausted:?}");
    assert_eq!(exhausted.remaining_budget_tokens, Some(0), "{exhausted:?}");
    assert_eq!(
        exhausted.diagnostic.as_ref().map(|d| d.code.as_str()),
        Some("budget_exhausted"),
        "{exhausted:?}"
    );
    assert!(
        exhausted
            .visible
            .as_ref()
            .is_some_and(|v| v.text.contains("scan truncated")),
        "{exhausted:?}"
    );

    let capped = engine.find("needle", &roots, Mode::Auto, 1, 4000);
    assert_eq!(capped.budget_exhausted, Some(true), "{capped:?}");
    assert_eq!(capped.remaining_budget_tokens, Some(0), "{capped:?}");
}

fn decode_grouped_paths(rendered: &str) -> Vec<PathBuf> {
    let mut root: Option<PathBuf> = None;
    let mut directories: Vec<String> = Vec::new();
    let mut outside_roots = false;
    let mut paths = Vec::new();
    for line in rendered.lines() {
        if let Some(encoded) = line.strip_prefix("# root: ") {
            let decoded: String = serde_json::from_str(encoded).unwrap();
            root = Some(PathBuf::from(decoded));
            directories.clear();
            outside_roots = false;
            continue;
        }
        if line == "# outside-roots" {
            root = None;
            directories.clear();
            outside_roots = true;
            continue;
        }
        if outside_roots {
            let decoded: String = serde_json::from_str(line).unwrap();
            paths.push(PathBuf::from(decoded));
            continue;
        }
        let spaces = line.bytes().take_while(|byte| *byte == b' ').count();
        assert_eq!(spaces % 2, 0, "indentation must use two spaces: {line:?}");
        let depth = spaces / 2;
        let encoded = &line[spaces..];
        let is_directory = encoded.ends_with('/');
        let encoded = encoded.strip_suffix('/').unwrap_or(encoded);
        let component: String = serde_json::from_str(encoded).unwrap();
        directories.truncate(depth);
        if is_directory {
            assert_eq!(directories.len(), depth);
            directories.push(component);
            continue;
        }
        assert_eq!(directories.len(), depth);
        let mut path = root.clone().expect("path row requires a root header");
        for directory in &directories {
            path.push(directory);
        }
        path.push(component);
        paths.push(path);
    }
    paths
}

#[test]
fn grouped_path_output_round_trips_escaped_prefix_trie() {
    let root = PathBuf::from("workspace root");
    let second_root = PathBuf::from("unicode-root");
    let mut paths = vec![
        root.join("src").join(" leading space.rs"),
        root.join("src").join("line\nbreak.rs"),
        root.join("src").join("quote\"name.rs"),
        root.join("src").join("nested[name]").join("µ.rs"),
        second_root.join("δ").join("tail.rs"),
        PathBuf::from("outside").join("orphan.rs"),
    ];
    let mut expected = paths.clone();
    expected.sort();
    paths.reverse();
    let roots = vec![root, second_root];
    let rendered = grouped_path_output(&paths, &roots);
    assert_eq!(rendered, grouped_path_output(&expected, &roots));
    let mut decoded = decode_grouped_paths(&rendered);
    decoded.sort();
    assert_eq!(decoded, expected);
    assert_eq!(rendered.matches("\"src\"/").count(), 1);
    assert!(rendered.contains("line\\nbreak.rs"));
    assert!(rendered.contains("quote\\\"name.rs"));
    assert!(rendered.contains("µ.rs"));
    assert!(rendered.contains("# outside-roots"));
}

#[test]
fn grouped_path_output_canonicalizes_roots_and_uses_most_specific_match() {
    let broad = PathBuf::from("workspace");
    let nested = broad.join("src");
    let disjoint = PathBuf::from("other");
    let mut expected = vec![
        nested.join("lib.rs"),
        broad.join("README.md"),
        disjoint.join("tail.rs"),
    ];
    let mut reversed_paths = expected.clone();
    reversed_paths.reverse();

    let canonical = grouped_path_output(
        &expected,
        &[
            broad.clone(),
            nested.clone(),
            disjoint.clone(),
            broad.clone(),
        ],
    );
    let permuted = grouped_path_output(&reversed_paths, &[disjoint, nested.clone(), broad]);
    assert_eq!(canonical, permuted);
    assert_eq!(canonical.matches("# root: ").count(), 3);

    let nested_header = format!(
        "# root: {}\n\"lib.rs\"",
        serde_json::to_string(&display_path(&nested)).unwrap()
    );
    assert!(
        canonical.contains(&nested_header),
        "nested file must bind to its most-specific root: {canonical}"
    );

    let mut decoded = decode_grouped_paths(&canonical);
    decoded.sort();
    expected.sort();
    assert_eq!(decoded, expected);
}

#[test]
fn hit_output_matches_fszero_target_ref_grammar() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("demo.rs");
    std::fs::write(
        &file,
        "fn a() {}\nfn b() {}\nneedle here\nfn c() {}\nfn d() {}\n",
    )
    .unwrap();
    let path = file.display().to_string();
    let matches = vec![SearchMatch {
        base: dir.path().display().to_string(),
        path: path.clone(),
        rel: "demo.rs".to_string(),
        line: 3,
        text: "needle here".to_string(),
    }];
    let rendered = hit_search_output(&matches, "literal");
    // 631q: the nearest declarator at/above the hit (fn b() at L2) is the
    // enclosing symbol, matching FSZero's enclosing_symbol() inference.
    let expected = format!(
        "HIT {path}#L1-L5 kind=literal sym=fn b() {{}}\n\
         | 1: fn a() {{}}\n\
         | 2: fn b() {{}}\n\
         | 3: needle here\n\
         | 4: fn c() {{}}\n\
         | 5: fn d() {{}}"
    );
    assert_eq!(rendered, expected);
}

#[test]
fn hit_output_infers_enclosing_symbol_for_function_body() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.rs");
    std::fs::write(
        &file,
        "use std::io;\n\npub fn parse_config() {\n    let a = 1;\n    let needle_line = a;\n    println!(\"{}\", needle_line);\n}\n",
    )
    .unwrap();
    let path = file.display().to_string();
    let matches = vec![SearchMatch {
        base: dir.path().display().to_string(),
        path: path.clone(),
        rel: "config.rs".to_string(),
        line: 5,
        text: "    let needle_line = a;".to_string(),
    }];
    let rendered = hit_search_output(&matches, "literal");
    // Declarator head drops the trailing " {" like FSZero does.
    assert!(
        rendered.starts_with(&format!(
            "HIT {path}#L3-L7 kind=literal sym=pub fn parse_config()\n"
        )),
        "{rendered}"
    );
}

#[test]
fn hit_output_infers_python_def_and_hit_on_declarator_line() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("app.py");
    std::fs::write(
        &file,
        "import os\n\ndef handle():\n    needle = 1\n    return needle\n",
    )
    .unwrap();
    let path = file.display().to_string();
    let matches = vec![SearchMatch {
        base: dir.path().display().to_string(),
        path: path.clone(),
        rel: "app.py".to_string(),
        line: 4,
        text: "    needle = 1".to_string(),
    }];
    let rendered = hit_search_output(&matches, "regex");
    assert!(
        rendered.starts_with(&format!("HIT {path}#L2-L5 kind=regex sym=def handle():\n")),
        "{rendered}"
    );
    // A hit ON the declarator line itself reports that declarator.
    let matches = vec![SearchMatch {
        base: dir.path().display().to_string(),
        path: path.clone(),
        rel: "app.py".to_string(),
        line: 3,
        text: "def handle():".to_string(),
    }];
    let rendered = hit_search_output(&matches, "literal");
    assert!(
        rendered.starts_with(&format!(
            "HIT {path}#L1-L5 kind=literal sym=def handle():\n"
        )),
        "{rendered}"
    );
}

#[test]
fn hit_output_file_scope_when_no_declarator_above() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "# comment\nneedle here\nfn x() {}\n").unwrap();
    let path = file.display().to_string();
    let matches = vec![SearchMatch {
        base: dir.path().display().to_string(),
        path: path.clone(),
        rel: "notes.txt".to_string(),
        line: 2,
        text: "needle here".to_string(),
    }];
    let rendered = hit_search_output(&matches, "literal");
    assert!(
        rendered.starts_with(&format!("HIT {path}#L1-L3 kind=literal sym=(file-scope)\n")),
        "{rendered}"
    );
}

#[test]
fn hit_output_falls_back_to_matched_line_when_file_unreadable() {
    let matches = vec![SearchMatch {
        base: "/base".to_string(),
        path: "/base/gone.txt".to_string(),
        rel: "gone.txt".to_string(),
        line: 7,
        text: "hit text".to_string(),
    }];
    let rendered = hit_search_output(&matches, "regex");
    assert_eq!(
        rendered,
        "HIT /base/gone.txt#L7-L7 kind=regex sym=(file-scope)\n| 7: hit text"
    );
}

#[test]
fn adjacent_matches_sharing_a_context_window_emit_one_hit_record() {
    // 5irj: the original two-match find fixture (alpha at L1 and alphabet
    // at L3 in a 3-line file) clamps both TARGET_CONTEXT_LINES=2 windows
    // to L1-L3. The byte-identical windows must collapse to exactly one
    // HIT header while every matching line stays visible.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tiny.txt");
    std::fs::write(&file, "alpha\nbeta\nalphabet\n").unwrap();
    let path = file.display().to_string();
    let root = dir.path().display().to_string();
    let matches = vec![
        SearchMatch {
            base: root.clone(),
            path: path.clone(),
            rel: "tiny.txt".to_string(),
            line: 1,
            text: "alpha".to_string(),
        },
        SearchMatch {
            base: root,
            path: path.clone(),
            rel: "tiny.txt".to_string(),
            line: 3,
            text: "alphabet".to_string(),
        },
    ];
    let rendered = hit_search_output(&matches, "literal");
    assert_eq!(rendered.matches("HIT ").count(), 1, "{rendered}");
    assert_eq!(
        rendered,
        format!(
            "HIT {path}#L1-L3 kind=literal sym=(file-scope)\n| 1: alpha\n| 2: beta\n| 3: alphabet"
        )
    );
}

#[test]
fn distinct_windows_and_symbols_stay_distinct() {
    // 5irj: dedupe must only collapse byte-identical (start, stop, kind,
    // sym) windows. Hits with different windows or different enclosing
    // symbols keep their own HIT records.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("wide.rs");
    std::fs::write(
        &file,
        "fn a() {}\nfirst hit here\nfn b() {}\nsecond hit here\nfn c() {}\n",
    )
    .unwrap();
    let path = file.display().to_string();
    let root = dir.path().display().to_string();
    let matches = vec![
        SearchMatch {
            base: root.clone(),
            path: path.clone(),
            rel: "wide.rs".to_string(),
            line: 2,
            text: "first hit here".to_string(),
        },
        SearchMatch {
            base: root,
            path: path.clone(),
            rel: "wide.rs".to_string(),
            line: 4,
            text: "second hit here".to_string(),
        },
    ];
    let rendered = hit_search_output(&matches, "literal");
    assert_eq!(rendered.matches("HIT ").count(), 2, "{rendered}");
    assert!(rendered.contains("sym=fn a() {}"), "{rendered}");
    assert!(rendered.contains("sym=fn b() {}"), "{rendered}");
    assert!(rendered.contains("| 2: first hit here"), "{rendered}");
    assert!(rendered.contains("| 4: second hit here"), "{rendered}");
}
