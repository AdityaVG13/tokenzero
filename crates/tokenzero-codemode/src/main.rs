#![forbid(unsafe_code)]

use std::ffi::OsString;

fn worker_args() -> Result<Vec<String>, String> {
    std::env::args_os()
        .map(|arg: OsString| {
            arg.into_string()
                .map_err(|arg| format!("worker argument is not valid UTF-8: {arg:?}"))
        })
        .collect()
}

fn run_metadata(tail: &[String]) -> Result<Option<i32>, String> {
    if matches!(tail, [argument] if argument == "--version" || argument == "-V") {
        println!("tokenzero-codemode {}", env!("CARGO_PKG_VERSION"));
        return Ok(Some(0));
    }
    if matches!(tail, [argument] if argument == "--help" || argument == "-h") {
        println!(
            "tokenzero-codemode: canonical planner-free raw-worker v2
\
             usage: tokenzero-codemode raw-worker [--handshake|--root DIR]
\
             probe: tokenzero-codemode capabilities --json"
        );
        return Ok(Some(0));
    }
    if matches!(tail, [argument] if argument == "sbom") {
        let capability = tokenzero_engine::build_surface_capability(
            tokenzero_engine::HandshakeSurface::RawWorker,
        );
        println!(
            "{{\"schema\":\"tokenzero.raw-worker.sbom/v1\",\"artifact\":\"tokenzero-codemode\",\"package\":\"tokenzero-worker\",\"package_version\":\"{}\",\"semantic_contract_digest\":\"{}\",\"raw_worker_protocol\":\"{}\"}}",
            env!("CARGO_PKG_VERSION"),
            capability.semantic_contract_digest,
            zero_abi::RAW_WORKER_PROTOCOL_VERSION,
        );
        return Ok(Some(0));
    }
    if matches!(tail, [argument] if argument == "capabilities")
        || matches!(tail, [command, json] if command == "capabilities" && json == "--json")
    {
        let capability = tokenzero_engine::build_surface_capability(
            tokenzero_engine::HandshakeSurface::RawWorker,
        );
        println!(
            "{{\"schema\":\"tokenzero.raw-worker.capabilities/v1\",\"package\":{{\"abi_digest\":\"{}\"}},\"protocol\":\"{}\"}}",
            capability.semantic_contract_digest,
            zero_abi::RAW_WORKER_PROTOCOL_VERSION,
        );
        return Ok(Some(0));
    }
    Ok(None)
}

fn run(args: &[String]) -> Result<i32, String> {
    if let Some(code) = run_metadata(&args[1..])? {
        return Ok(code);
    }
    tokenzero_engine::maybe_run_raw_worker_from_args(args)?
        .ok_or_else(|| "unsupported canonical worker command".to_string())
}

fn main() {
    let code = worker_args()
        .and_then(|args| run(&args))
        .unwrap_or_else(|error| {
            eprintln!("tokenzero-codemode: {error}");
            2
        });
    std::process::exit(code);
}
