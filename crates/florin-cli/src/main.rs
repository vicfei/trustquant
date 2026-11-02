//! florin: the single binary serving CLI, MCP and (future) HTTP from
//! one Runtime implementation (v2 architecture §4.1).
//!
//!   florin run request.json      # execute + record manifest
//!   florin replay <id> [--json]  # re-execute + verify digests
//!   florin manifest <id>         # fetch a manifest
//!   florin mcp [--stdio]         # stdio MCP server
//!   florin provider check        # external provider channel check
//!   florin version

use florin_core::contract::RequestEnvelope;
use florin_runtime::{mcp, replay_calculation, store::ManifestStore};
use serde_json::json;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => cmd_run(&args[1..]),
        Some("replay") => cmd_replay(&args[1..]),
        Some("manifest") => cmd_manifest(&args[1..]),
        Some("mcp") => ExitCode::from(u8::try_from(mcp::run_stdio()).unwrap_or(1)),
        Some("provider") => cmd_provider(&args[1..]),
        Some("version") | Some("--version") => {
            println!(
                "florin {} (runtime {}, core {}, schema {})",
                env!("CARGO_PKG_VERSION"),
                florin_runtime::RUNTIME_VERSION,
                florin_core::CORE_VERSION,
                florin_core::contract::SCHEMA_VERSION
            );
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "usage: florin <command>\n  run request.json\n  replay <calculation-id> [--json]\n  manifest <calculation-id>\n  mcp\n  provider check\n  version"
            );
            ExitCode::from(2)
        }
    }
}

fn cmd_run(args: &[String]) -> ExitCode {
    let Some(path) = args.first().cloned() else {
        eprintln!("usage: florin run request.json");
        return ExitCode::from(2);
    };
    let body = match std::fs::read_to_string(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::from(2);
        }
    };
    let request: RequestEnvelope = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("invalid request envelope: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = request.validate_schema_version() {
        let env = florin_runtime::ResponseEnvelope::error(e, request.request_id.as_deref());
        println!("{}", serde_json::to_string_pretty(&env).unwrap());
        return ExitCode::FAILURE;
    }
    let store = ManifestStore::default_store();
    let (envelope, manifest) = florin_runtime::execute_and_record(&store, &request);
    println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
    if let Some(m) = manifest {
        eprintln!(
            "recorded calculation_id={} at {}",
            m.calculation_id,
            store
                .calculations_dir()
                .join(format!("{}.json", m.calculation_id))
                .display()
        );
    }
    if envelope.status == "error" {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_replay(args: &[String]) -> ExitCode {
    let Some(id) = args.first().cloned() else {
        eprintln!("usage: florin replay <calculation-id> [--json]");
        return ExitCode::from(2);
    };
    let as_json = args.iter().any(|a| a == "--json");
    let store = ManifestStore::default_store();
    match replay_calculation(&store, &id) {
        Ok(report) => {
            if as_json {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                println!("calculation_id      {}", report.calculation_id);
                println!("tool                {}", report.tool);
                println!("manifest_hash_valid {}", report.manifest_hash_valid);
                println!("result_digest_match {}", report.result_digest_match);
                println!("stored_digest       {}", report.stored_result_digest);
                println!("recomputed_digest   {}", report.recomputed_result_digest);
                println!("REPLAY: {}", if report.passed() { "PASS" } else { "FAIL" });
            }
            if report.passed() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            let env = florin_runtime::ResponseEnvelope::error(e, None);
            println!("{}", serde_json::to_string_pretty(&env).unwrap());
            ExitCode::FAILURE
        }
    }
}

fn cmd_manifest(args: &[String]) -> ExitCode {
    let Some(id) = args.first().cloned() else {
        eprintln!("usage: florin manifest <calculation-id>");
        return ExitCode::from(2);
    };
    let store = ManifestStore::default_store();
    match florin_runtime::get_manifest(&store, &id) {
        Ok(m) => {
            println!("{}", serde_json::to_string_pretty(&m).unwrap());
            ExitCode::SUCCESS
        }
        Err(e) => {
            let env = florin_runtime::ResponseEnvelope::error(e, None);
            println!("{}", serde_json::to_string_pretty(&env).unwrap());
            ExitCode::FAILURE
        }
    }
}

fn cmd_provider(args: &[String]) -> ExitCode {
    if args.first().map(String::as_str) != Some("check") {
        eprintln!("usage: florin provider check   (checks the external Python sidecar channel)");
        return ExitCode::from(2);
    }
    let script = florin_sidecar::sidecar_script_path();
    println!("interpreter: {}", florin_sidecar::python_command());
    println!("script:      {}", script.display());
    match florin_sidecar::SidecarHandle::start(&script, std::time::Duration::from_secs(15)) {
        Ok(mut sc) => {
            println!(
                "provider:    {} v{}",
                sc.hello["provider"].as_str().unwrap_or("?"),
                sc.provider_version()
            );
            println!("protocol:    {}", sc.hello["protocol"]);
            println!("environment: {}", sc.hello["environment"]);
            let sum = sc.call(
                "echo_add",
                &json!({"a": 2, "b": 40}),
                std::time::Duration::from_secs(5),
            );
            match sum {
                Ok(r) => {
                    println!("roundtrip:   echo_add(2,40) = {}", r["sum"]);
                    println!("PROVIDER: OK");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    let env = e.into_envelope();
                    println!("PROVIDER: FAILED ({})", env.error_type_str());
                    ExitCode::FAILURE
                }
            }
        }
        Err(e) => {
            let env = e.into_envelope();
            println!("{}", serde_json::to_string_pretty(&env).unwrap());
            println!("PROVIDER: UNAVAILABLE (no silent fallback; see ADR-0002)");
            ExitCode::FAILURE
        }
    }
}
