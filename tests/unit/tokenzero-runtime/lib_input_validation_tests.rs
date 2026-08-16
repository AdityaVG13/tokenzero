    use super::*;
    use std::path::PathBuf;

    #[test]
    fn env_map_refuses_empty_key_and_nul() {
        let err = env_map(&["=value".into()]).expect_err("empty key");
        assert!(err.to_string().contains("non-empty"), "{err}");
        let err = env_map(&["KEY\0=value".into()]).expect_err("nul key");
        assert!(err.to_string().contains("NUL"), "{err}");
        let err = env_map(&["KEY=val\0ue".into()]).expect_err("nul value");
        assert!(err.to_string().contains("NUL"), "{err}");
        let ok = env_map(&["KEY=value".into()]).unwrap();
        assert_eq!(ok.get("KEY").map(String::as_str), Some("value"));
        let err = env_map(&["not-a-pair".into()]).expect_err("missing equals");
        assert!(err.to_string().contains("KEY=VALUE"), "{err}");
        assert!(
            !err.to_string().contains("not-a-pair"),
            "env parse errors must not echo the raw pair: {err}"
        );
        let err = env_map(&["BASH_ENV=/tmp/evil".into()]).expect_err("bash env");
        assert!(err.to_string().contains("not allowed"), "{err}");
        let err = validate_env_pair("FOO=BAR", "1").expect_err("equals in key");
        assert!(err.to_string().contains("'='"), "{err}");
    }

    #[test]
    fn plan_command_quotes_argv_for_shell_execution() {
        let posix = plan_command_for_platform(
            &["echo".into(), "$(id)".into(), "|".into(), "cat".into()],
            None,
            false,
            "posix",
        )
        .unwrap();
        assert_eq!(posix.execution_mode, ExecutionMode::Shell);
        assert_eq!(
            posix.argv.last().map(String::as_str),
            Some("echo '$(id)' | cat")
        );

        let windows = plan_command_for_platform(
            &["echo".into(), "%PATH%".into(), "|".into(), "more".into()],
            None,
            false,
            "windows",
        )
        .unwrap();
        assert_eq!(windows.execution_mode, ExecutionMode::Shell);
        let cmd = windows.argv.last().expect("cmd string");
        assert!(
            cmd.contains("%%PATH%%"),
            "cmd wrap must disable % expansion: {cmd}"
        );
        assert!(
            !cmd.contains("\"%PATH%\""),
            "display quoting must not be used for execution: {cmd}"
        );
    }

    #[test]
    fn plan_command_refuses_nul_in_argv() {
        let err = plan_command(&["echo".into(), "ok\0".into()], None, false).expect_err("nul argv");
        assert!(err.to_string().contains("NUL"), "{err}");
    }

    #[test]
    fn overflowing_timeout_fails_loud_before_spawn() {
        let err = run_command(
            &["true".into()],
            None,
            None,
            None,
            Duration::MAX,
            false,
        )
        .expect_err("Duration::MAX must not collapse to an immediate timeout");
        assert!(
            err.to_string().contains("overflows Instant"),
            "{err}"
        );
    }

    #[test]
    fn run_refuses_unexpanded_tilde_cwd_and_oversize_capture() {
        let err = run_command(
            &["true".into()],
            Some(Path::new("~")),
            None,
            None,
            Duration::from_millis(10),
            false,
        )
        .expect_err("tilde cwd");
        assert!(err.to_string().contains("unexpanded ~ cwd"), "{err}");

        let policy = RunOutputPolicy {
            per_stream_capture_bytes: MAX_SHELL_CAPTURE_BYTES + 1,
            spill_threshold_bytes: 1024,
            spill_dir: None,
        };
        let err = run_command_with_policy(
            &["true".into()],
            None,
            None,
            None,
            Duration::from_millis(10),
            false,
            policy,
        )
        .expect_err("oversize capture");
        assert!(err.to_string().contains("hard max"), "{err}");

        let policy = RunOutputPolicy {
            per_stream_capture_bytes: 1024,
            spill_threshold_bytes: 512,
            spill_dir: Some(PathBuf::from("~/spills")),
        };
        let err = run_command_with_policy(
            &["true".into()],
            None,
            None,
            None,
            Duration::from_millis(10),
            false,
            policy,
        )
        .expect_err("tilde spill");
        assert!(err.to_string().contains("unexpanded ~ spill"), "{err}");
    }

