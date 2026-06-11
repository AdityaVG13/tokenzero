#![allow(dead_code)]

use assert_cmd::prelude::*;
use std::process::{Command, Output};

pub fn reviewed_adapter_rows() -> Vec<serde_json::Value> {
    [
        "rtk",
        "ztk",
        "lean-ctx",
        "tokenpak",
        "tokenjuice",
        "context-mode",
        "caveman",
        "headroom",
        "claw",
        "compresh",
        "context-gateway",
    ]
    .iter()
    .map(|tool| {
        serde_json::json!({
            "tool": tool,
            "approval_status": "reviewed",
            "execution_allowed": true,
            "reviewed_command": format!("{tool} --version"),
            "blind_install_attempted": false
        })
    })
    .collect()
}

pub fn tokenzero_with_agent_env(args: &[&str]) -> Output {
    Command::cargo_bin("tokenzero")
        .unwrap()
        .args(args)
        .env("NO_COLOR", "1")
        .env("CI", "true")
        .env("TERM", "dumb")
        .env("SOURCE_DATE_EPOCH", "1234567890")
        .output()
        .unwrap()
}

pub fn assert_no_ansi(bytes: &[u8]) {
    assert!(
        !bytes.contains(&0x1b),
        "unexpected ANSI escape in output:\n{}",
        String::from_utf8_lossy(bytes)
    );
}
