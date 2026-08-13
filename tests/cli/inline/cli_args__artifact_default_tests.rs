use super::*;
use std::collections::HashSet;
use std::path::Path;

fn artifact_output(cli: Cli) -> PathBuf {
    match cli.command.expect("artifact command") {
        Commands::McpSmoke(args) => args.output_json,
        Commands::McpSoak(args) => args.output_json,
        Commands::HarmEval(args) => args.output_json,
        Commands::RepoInventory(args) => args.output_json,
        Commands::PromptCachePack(args) => args.output_json,
        _ => panic!("expected an artifact command"),
    }
}

#[test]
fn shared_shape_eval_commands_have_unique_path_safe_defaults_and_keep_overrides() {
    let cases = [
        ("mcp-smoke", "results/current/rust_mcp_smoke.json"),
        ("mcp-soak", "results/current/rust_mcp_soak.json"),
        ("harm-eval", "results/current/harm_eval.json"),
        ("repo-inventory", "results/current/repo_inventory.json"),
        (
            "prompt-cache-pack",
            "results/current/prompt_cache_pack.json",
        ),
    ];
    let mut defaults = HashSet::new();

    for (command, expected) in cases {
        let output = artifact_output(Cli::try_parse_from(["tokenzero", command]).unwrap());
        assert_eq!(output, PathBuf::from(expected));
        assert_eq!(output.parent(), Some(Path::new("results/current")));
        assert_eq!(
            output.extension().and_then(|value| value.to_str()),
            Some("json")
        );
        assert!(defaults.insert(output), "duplicate default for {command}");

        let explicit = PathBuf::from(format!("/tmp/{command}-explicit.json"));
        let overridden = artifact_output(
            Cli::try_parse_from([
                "tokenzero",
                command,
                "--output-json",
                explicit.to_str().unwrap(),
            ])
            .unwrap(),
        );
        assert_eq!(overridden, explicit);
    }
    assert_eq!(defaults.len(), cases.len());
}
