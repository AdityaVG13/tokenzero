    use super::{normalize_run_invocation_args, split_run_args_without_delimiter};
    use std::ffi::OsString;

    fn argv(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    fn texts(parts: &[OsString]) -> Vec<String> {
        parts
            .iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn split_run_args_does_not_swallow_the_next_flag_as_a_path() {
        assert_eq!(
            split_run_args_without_delimiter(&argv(&["--cwd", "--timeout", "5", "echo", "ok"])),
            None,
            "--cwd must not treat --timeout as a working directory"
        );
        assert_eq!(
            split_run_args_without_delimiter(&argv(&["--timeout", "--cwd", "/tmp", "echo", "ok"])),
            None,
            "--timeout must not treat --cwd as seconds"
        );
        assert_eq!(
            split_run_args_without_delimiter(&argv(&["--cwd"])),
            None,
            "bare --cwd has no value and no child command"
        );

        let (options, command) =
            split_run_args_without_delimiter(&argv(&["--cwd", "/tmp", "echo", "ok"])).unwrap();
        assert_eq!(texts(&options), vec!["--cwd", "/tmp"]);
        assert_eq!(texts(&command), vec!["echo", "ok"]);

        let (options, command) =
            split_run_args_without_delimiter(&argv(&["--cwd=/tmp", "echo", "ok"])).unwrap();
        assert_eq!(texts(&options), vec!["--cwd=/tmp"]);
        assert_eq!(texts(&command), vec!["echo", "ok"]);
    }

    #[test]
    fn normalize_run_leaves_flag_shaped_cwd_for_clap() {
        let left = texts(&normalize_run_invocation_args(argv(&[
            "tokenzero",
            "run",
            "--cwd",
            "--timeout",
            "5",
            "echo",
            "ok",
        ])));
        assert_eq!(
            left,
            vec!["tokenzero", "run", "--cwd", "--timeout", "5", "echo", "ok"],
            "missing --cwd value must not insert -- after stealing the next flag"
        );

        let rewritten = texts(&normalize_run_invocation_args(argv(&[
            "tokenzero",
            "run",
            "--cwd",
            "/tmp",
            "echo",
            "ok",
        ])));
        assert_eq!(
            rewritten,
            vec!["tokenzero", "run", "--cwd", "/tmp", "--", "echo", "ok"]
        );
    }

