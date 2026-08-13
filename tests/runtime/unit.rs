use tokenzero_runtime::*;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_split_roundtrips_displayed_cmd_and_powershell_args(
        arg in prop::string::string_regex("[A-Za-z0-9 _./:\\\\@%+=,;|&$()`-]{0,32}['\"]?[A-Za-z0-9 _./:\\\\@%+=,;|&$()`-]{0,32}").unwrap(),
        platform in prop::sample::select(vec!["cmd", "powershell", "posix"]),
    ) {
        let argv = vec!["tool".to_string(), arg];
        let displayed = tokenzero_core::shell_display_command_from_argv_for_platform(
            &argv,
            platform,
        );
        let parsed = split_command_string_for_platform(&displayed, platform);

        prop_assert_eq!(parsed, argv);
    }
}
