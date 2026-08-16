    use super::{is_broken_pipe, map_stdout_write};
    use std::io::{self, ErrorKind};

    #[test]
    fn broken_pipe_is_a_clean_write_not_a_panic() {
        let err = io::Error::new(ErrorKind::BrokenPipe, "closed pipe");
        assert!(is_broken_pipe(&err));
        map_stdout_write(Err(err)).expect("broken pipe must not fail the CLI process");
    }

    #[test]
    fn other_stdout_errors_still_fail_loud() {
        let err = io::Error::new(ErrorKind::PermissionDenied, "stdout");
        assert!(!is_broken_pipe(&err));
        let message = map_stdout_write(Err(err))
            .expect_err("permission errors must stay visible")
            .to_string();
        assert!(message.contains("stdout") || message.contains("Permission"), "{message}");
    }

