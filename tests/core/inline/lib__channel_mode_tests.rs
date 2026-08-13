use super::*;

#[test]
fn legacy_truthy_values_stay_action_only() {
    for raw in ["1", "on", "TRUE", " yes ", "action"] {
        let mode = ChannelMode::from_env_value(raw);
        assert_eq!(mode, ChannelMode::Action, "{raw}");
        assert!(mode.enabled());
        assert!(!mode.emits_user_message(), "{raw} must stay action-only");
    }
}

#[test]
fn terminal_mode_opts_into_receipt_user_message() {
    for raw in ["terminal", "Final"] {
        let mode = ChannelMode::from_env_value(raw);
        assert_eq!(mode, ChannelMode::Terminal, "{raw}");
        assert!(mode.enabled());
        assert!(mode.emits_user_message());
    }
}

#[test]
fn unknown_and_falsy_values_are_off() {
    for raw in ["", "0", "off", "nonsense"] {
        let mode = ChannelMode::from_env_value(raw);
        assert_eq!(mode, ChannelMode::Off, "{raw}");
        assert!(!mode.enabled());
        assert!(!mode.emits_user_message());
    }
}
