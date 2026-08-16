    use super::posix_shell_matrix_command;
    use std::path::Path;

    #[test]
    fn posix_shell_matrix_command_quotes_paths_with_spaces() {
        let cmd = posix_shell_matrix_command(
            Path::new("/tmp/Token Zero/tokenzero"),
            Path::new("/tmp/cache dir.json"),
        );
        assert!(
            cmd.contains("'/tmp/Token Zero/tokenzero'"),
            "exe path with spaces must be POSIX-quoted: {cmd}"
        );
        assert!(
            cmd.contains("'/tmp/cache dir.json'"),
            "cache path with spaces must be POSIX-quoted: {cmd}"
        );
        assert!(
            !cmd.contains("Zero/tokenzero run"),
            "unquoted space must not split the exe token: {cmd}"
        );
    }

    #[test]
    fn posix_shell_matrix_command_quotes_dollar_and_apostrophe_paths() {
        let dollar = posix_shell_matrix_command(
            Path::new("/tmp/tokenzero"),
            Path::new("/tmp/cache $dir.json"),
        );
        assert!(
            dollar.contains("'/tmp/cache $dir.json'"),
            "dollar in cache path must be single-quoted so the shell cannot expand it: {dollar}"
        );
        let apostrophe = posix_shell_matrix_command(
            Path::new("/tmp/it's bin/tokenzero"),
            Path::new("/tmp/cache.json"),
        );
        assert!(
            apostrophe.contains("'/tmp/it'\"'\"'s bin/tokenzero'"),
            "apostrophe in exe path must use POSIX nested quoting: {apostrophe}"
        );
    }

