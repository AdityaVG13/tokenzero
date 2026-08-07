//! Engine-owned fixtures for public TokenZero worker conformance.

use zero_abi::{DEFAULT_MAX_FRAME_BYTES, FrameCodecError, WorkerResponseFrame};

/// Decode every non-empty NDJSON response through the shared hub codec.
pub fn decode_worker_transcript(bytes: &[u8]) -> Result<Vec<WorkerResponseFrame>, FrameCodecError> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| zero_abi::decode_response_frame(line, DEFAULT_MAX_FRAME_BYTES))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_decoder_uses_strict_shared_shutdown_codec() {
        let canonical = b"{\"kind\":\"shutdown_ack\"}\n";
        assert!(matches!(
            decode_worker_transcript(canonical).as_deref(),
            Ok([WorkerResponseFrame::ShutdownAck])
        ));
        let mutant = b"{\"kind\":\"shutdown_ack\",\"extra\":true}\n";
        assert_eq!(
            decode_worker_transcript(mutant).unwrap_err().kind(),
            "invalid_frame"
        );
    }

    #[test]
    fn installer_prints_only_the_canonical_backend_selector() {
        let script =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/install.sh");
        let output = std::process::Command::new("bash")
            .arg(&script)
            .args(["--surface", "codemode", "--dry-run"])
            .output()
            .expect("installer dry run starts");
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).expect("UTF-8 dry run"),
            "cargo build --release -p tokenzero-worker --bin tokenzero-codemode --no-default-features\n"
        );

        let legacy = std::process::Command::new("bash")
            .arg(&script)
            .args(["--surface", "mcp", "--dry-run"])
            .output()
            .expect("legacy selector starts");
        assert_eq!(legacy.status.code(), Some(2));
        let stderr = String::from_utf8(legacy.stderr).expect("UTF-8 diagnostic");
        assert!(stderr.contains("legacy MCP artifact retired"), "{stderr}");
        assert!(
            stderr.contains("ZeroStack aggregate host adapter"),
            "{stderr}"
        );

        let blocked_root = std::env::temp_dir().join(format!(
            "tokenzero-worker-install-blocked-{}",
            std::process::id()
        ));
        let blocked = std::process::Command::new("bash")
            .arg(&script)
            .args(["--surface", "codemode", "--prefix"])
            .arg(&blocked_root)
            .output()
            .expect("blocked install starts");
        assert_eq!(blocked.status.code(), Some(2));
        let stderr = String::from_utf8(blocked.stderr).expect("UTF-8 diagnostic");
        assert!(stderr.contains("zerostack-uf1u"), "{stderr}");
        assert!(!blocked_root.exists(), "blocked install mutated its prefix");
    }

    #[test]
    fn canonical_worker_manifest_keeps_hosts_out_of_default_features() {
        let manifest = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tokenzero-codemode/Cargo.toml"),
        )
        .expect("worker manifest readable");
        let default = manifest
            .split_once("[features]")
            .and_then(|(_, features)| features.lines().find(|line| line.starts_with("default")))
            .expect("worker default feature declaration");
        assert_eq!(default.trim(), "default = []");
        let quickjs = manifest
            .lines()
            .find(|line| line.starts_with("rquickjs ="))
            .expect("rquickjs dependency declaration");
        assert!(quickjs.contains("optional = true"), "rquickjs: {quickjs}");
        assert!(
            !manifest
                .lines()
                .any(|line| line.starts_with("tokenzero-mcp-compat =")),
            "canonical worker package must not depend on the MCP adapter"
        );

        let main = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tokenzero-codemode/src/main.rs"),
        )
        .expect("worker main readable");
        for forbidden in ["rquickjs", "fastmcp", "execute_codemode", "run_stdio"] {
            assert!(!main.contains(forbidden), "raw worker imports {forbidden}");
        }
    }
}
