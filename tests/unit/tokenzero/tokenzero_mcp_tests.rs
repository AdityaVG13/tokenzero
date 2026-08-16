    use super::{
        argv_without_option_values, is_broken_pipe, map_stdout_write, parse_flag,
        require_classic_surface_flags, stdio_root_from_args, VALUE_FLAGS,
    };
    use std::io::{self, ErrorKind};
    use std::path::PathBuf;
    use tokenzero_install::packaging::reject_non_stdio_args;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn parse_flag_absent_is_none() {
        assert_eq!(
            parse_flag(&args(&["tokenzero-mcp", "install"]), "--prefix").unwrap(),
            None
        );
    }

    #[test]
    fn parse_flag_space_and_equals_forms() {
        assert_eq!(
            parse_flag(
                &args(&["tokenzero-mcp", "install", "--prefix", "/tmp/tz"]),
                "--prefix"
            )
            .unwrap()
            .as_deref(),
            Some("/tmp/tz")
        );
        assert_eq!(
            parse_flag(
                &args(&["tokenzero-mcp", "install", "--prefix=/tmp/tz"]),
                "--prefix"
            )
            .unwrap()
            .as_deref(),
            Some("/tmp/tz")
        );
    }

    #[test]
    fn parse_flag_rejects_missing_or_flag_shaped_values() {
        let missing = parse_flag(&args(&["tokenzero-mcp", "install", "--prefix"]), "--prefix")
            .expect_err("bare --prefix must fail loud");
        assert!(missing.contains("requires a value"), "{missing}");

        let empty = parse_flag(
            &args(&["tokenzero-mcp", "install", "--prefix="]),
            "--prefix",
        )
        .expect_err("empty --prefix= must fail loud");
        assert!(empty.contains("requires a value"), "{empty}");

        let stolen = parse_flag(
            &args(&[
                "tokenzero-mcp",
                "install",
                "--prefix",
                "--binary",
                "/tmp/bin",
            ]),
            "--prefix",
        )
        .expect_err("--prefix must not swallow the next flag");
        assert!(stolen.contains("--binary"), "{stolen}");
        assert_eq!(
            parse_flag(
                &args(&[
                    "tokenzero-mcp",
                    "install",
                    "--prefix",
                    "--binary",
                    "/tmp/bin",
                ]),
                "--binary"
            )
            .unwrap()
            .as_deref(),
            Some("/tmp/bin")
        );
    }

    #[test]
    fn parse_flag_equals_form_keeps_dash_leading_values() {
        assert_eq!(
            parse_flag(
                &args(&["tokenzero-mcp", "install", "--prefix=-"]),
                "--prefix"
            )
            .unwrap()
            .as_deref(),
            Some("-")
        );
        assert_eq!(
            parse_flag(
                &args(&["tokenzero-mcp", "uninstall", "--prefix=-dash-dir"]),
                "--prefix"
            )
            .unwrap()
            .as_deref(),
            Some("-dash-dir")
        );
        let later_missing = parse_flag(
            &args(&[
                "tokenzero-mcp",
                "install",
                "--prefix",
                "/tmp/tz",
                "--prefix",
            ]),
            "--prefix",
        )
        .expect_err("a later bare --prefix must fail even after an earlier valid value");
        assert!(
            later_missing.contains("requires a value"),
            "{later_missing}"
        );
    }

    #[test]
    fn argv_without_option_values_does_not_treat_flag_values_as_verbs() {
        let stripped = argv_without_option_values(&args(&[
            "tokenzero-mcp",
            "install",
            "--prefix",
            "sbom",
            "--binary",
            "help",
        ]));
        assert!(
            stripped.iter().any(|a| a == "install"),
            "install verb must remain: {stripped:?}"
        );
        assert!(
            !stripped.iter().any(|a| a == "sbom" || a == "help"),
            "option values must not look like verbs: {stripped:?}"
        );

        let mode = argv_without_option_values(&args(&["tokenzero-mcp", "--mode", "mcp"]));
        assert!(
            mode.iter().any(|a| a == "--mode"),
            "flag must remain so unknown options still fail loud: {mode:?}"
        );
        assert!(
            !mode.iter().any(|a| a == "mcp"),
            "--mode mcp must not be an unknown subcommand: {mode:?}"
        );
    }

    #[test]
    fn stripped_stdio_flags_are_accepted_by_reject_non_stdio_args() {
        for argv in [
            args(&["tokenzero-mcp", "--mode", "mcp"]),
            args(&["tokenzero-mcp", "--mode=mcp"]),
            args(&["tokenzero-mcp", "--root", "/tmp/ws"]),
            args(&["tokenzero-mcp", "--repo", "/tmp/repo"]),
        ] {
            let verbs = argv_without_option_values(&argv);
            reject_non_stdio_args("tokenzero-mcp", &verbs).unwrap_or_else(|error| {
                panic!("stdio flags must survive verb-stripping: {argv:?} verbs={verbs:?} {error}")
            });
        }
    }

    #[test]
    fn require_classic_surface_flags_accepts_mcp_and_refuses_codemode_aliases() {
        require_classic_surface_flags(&args(&["tokenzero-mcp", "--mode=mcp"])).unwrap();
        require_classic_surface_flags(&args(&["tokenzero-mcp", "--mode", "classic"])).unwrap();
        require_classic_surface_flags(&args(&["tokenzero-mcp", "--tool-surface", "mcp"])).unwrap();

        let refused =
            require_classic_surface_flags(&args(&["tokenzero-mcp", "--tool-surface", "codemode"]))
                .expect_err("--tool-surface codemode must fail as loudly as --mode=codemode");
        assert!(refused.contains("codemode"), "{refused}");

        let invalid = require_classic_surface_flags(&args(&["tokenzero-mcp", "--mode", "foobar"]))
            .expect_err("unknown --mode must not fall through to stdio");
        assert!(
            invalid.contains("foobar") || invalid.contains("unsupported"),
            "{invalid}"
        );

        let stolen =
            require_classic_surface_flags(&args(&["tokenzero-mcp", "--mode", "--root", "/tmp/ws"]))
                .expect_err("bare --mode must not steal the next flag");
        assert!(
            stolen.contains("--root") || stolen.contains("requires a value"),
            "{stolen}"
        );

        let shadowed = require_classic_surface_flags(&args(&[
            "tokenzero-mcp",
            "--mode",
            "mcp",
            "--mode=codemode",
        ]))
        .expect_err("later --mode=codemode must not be shadowed by an earlier --mode mcp");
        assert!(shadowed.contains("codemode"), "{shadowed}");

        let later_space = require_classic_surface_flags(&args(&[
            "tokenzero-mcp",
            "--mode=mcp",
            "--tool-surface",
            "mcp",
            "--mode",
            "codemode",
        ]))
        .expect_err("later space-form --mode codemode must still refuse");
        assert!(later_space.contains("codemode"), "{later_space}");
    }

    #[test]
    fn stdio_root_from_args_honors_root_and_repo() {
        let cwd = PathBuf::from("/cwd");
        assert_eq!(
            stdio_root_from_args(&args(&["tokenzero-mcp"]), cwd.clone()).unwrap(),
            cwd
        );
        assert_eq!(
            stdio_root_from_args(
                &args(&["tokenzero-mcp", "--root", "/tmp/ws"]),
                PathBuf::from("/cwd")
            )
            .unwrap(),
            PathBuf::from("/tmp/ws")
        );
        assert_eq!(
            stdio_root_from_args(
                &args(&["tokenzero-mcp", "--repo=/tmp/repo"]),
                PathBuf::from("/cwd")
            )
            .unwrap(),
            PathBuf::from("/tmp/repo")
        );
        let disagree = stdio_root_from_args(
            &args(&["tokenzero-mcp", "--root", "/tmp/a", "--repo", "/tmp/b"]),
            PathBuf::from("/cwd"),
        )
        .expect_err("disagreeing --root/--repo must fail loud");
        assert!(disagree.contains("/tmp/a"), "{disagree}");
        assert!(disagree.contains("/tmp/b"), "{disagree}");

        assert_eq!(
            stdio_root_from_args(
                &args(&["tokenzero-mcp", "--root", "/tmp/ws", "--repo", "/tmp/ws/"]),
                PathBuf::from("/cwd")
            )
            .unwrap(),
            PathBuf::from("/tmp/ws"),
            "trailing slash is the same path, not a disagreement"
        );

        let duplicate_root = stdio_root_from_args(
            &args(&["tokenzero-mcp", "--root", "/tmp/a", "--root=/tmp/b"]),
            PathBuf::from("/cwd"),
        )
        .expect_err("duplicate disagreeing --root must not keep the first value");
        assert!(duplicate_root.contains("/tmp/a"), "{duplicate_root}");
        assert!(duplicate_root.contains("/tmp/b"), "{duplicate_root}");

        assert_eq!(
            stdio_root_from_args(
                &args(&["tokenzero-mcp", "--root", "/tmp/ws", "--root=/tmp/ws/"]),
                PathBuf::from("/cwd")
            )
            .unwrap(),
            PathBuf::from("/tmp/ws"),
            "duplicate --root with a trailing slash is the same path"
        );
    }

    #[test]
    fn argv_without_option_values_strips_every_stdio_and_install_value_flag() {
        assert!(
            VALUE_FLAGS.contains(&"--allowed-root"),
            "CLI mcp-server --allowed-root is a value flag sibling of --root/--prefix"
        );
        for flag in VALUE_FLAGS {
            let stripped = argv_without_option_values(&args(&["tokenzero-mcp", flag, "help"]));
            assert!(
                !stripped.iter().any(|a| a == "help"),
                "{flag} value must not be scanned as a verb: {stripped:?}"
            );
            assert!(
                stripped.iter().any(|a| a == *flag),
                "{flag} itself must remain so unknown options still fail loud: {stripped:?}"
            );
        }
    }

    #[test]
    fn cli_mcp_server_value_flags_are_not_help_verbs_then_fail_loud() {
        for flag in [
            "--allowed-root",
            "--default-mode",
            "--shell-timeout-seconds",
            "--timeout",
            "--idle-timeout-seconds",
        ] {
            let argv = args(&["tokenzero-mcp", flag, "help"]);
            let verbs = argv_without_option_values(&argv);
            assert!(
                !verbs.iter().any(|a| a == "help"),
                "{flag} help must not open the help verb: {verbs:?}"
            );
            let Err(error) = reject_non_stdio_args("tokenzero-mcp", &verbs) else {
                panic!("{flag} must fail as an unsupported option after stripping its value")
            };
            assert!(
                error.contains(flag),
                "{flag} must fail as an unsupported option after stripping its value: {error}"
            );
        }
    }

    #[test]
    fn broken_pipe_is_a_clean_write_not_a_panic() {
        let err = io::Error::new(ErrorKind::BrokenPipe, "closed pipe");
        assert!(is_broken_pipe(&err));
        map_stdout_write(Err(err)).expect("broken pipe must not fail the MCP packaging CLI");
    }

    #[test]
    fn other_stdout_errors_still_fail_loud() {
        let err = io::Error::new(ErrorKind::PermissionDenied, "stdout");
        assert!(!is_broken_pipe(&err));
        let message = map_stdout_write(Err(err))
            .expect_err("permission errors must stay visible")
            .to_string();
        assert!(
            message.contains("stdout") || message.contains("Permission"),
            "{message}"
        );
    }

