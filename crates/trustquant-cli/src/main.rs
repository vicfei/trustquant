//! trustquant: the single binary serving CLI, MCP and (future) HTTP from
//! one Runtime implementation (v2 architecture §4.1).
//!
//!   trustquant run request.json      # execute + record manifest
//!   trustquant replay <id> [--json]  # re-execute + verify digests
//!   trustquant manifest <id>         # fetch a manifest
//!   trustquant mcp [--stdio]         # stdio MCP server
//!   trustquant provider check        # external provider channel check
//!   trustquant version

use serde_json::json;
use std::process::ExitCode;
use trustquant_core::contract::RequestEnvelope;
use trustquant_runtime::{mcp, replay_calculation};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => cmd_run(&args[1..]),
        Some("replay") => cmd_replay(&args[1..]),
        Some("manifest") => cmd_manifest(&args[1..]),
        Some("mcp") => ExitCode::from(u8::try_from(mcp::run_stdio()).unwrap_or(1)),
        Some("provider") => cmd_provider(&args[1..]),
        Some("serve") => cmd_serve(&args[1..]),
        Some("version") | Some("--version") => {
            println!(
                "trustquant {} (runtime {}, core {}, schema {})",
                env!("CARGO_PKG_VERSION"),
                trustquant_runtime::RUNTIME_VERSION,
                trustquant_core::CORE_VERSION,
                trustquant_core::contract::SCHEMA_VERSION
            );
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "usage: trustquant <command>\n  run request.json\n  replay <calculation-id> [--json]\n  manifest <calculation-id>\n  mcp\n  serve [--port N]\n  provider check|install\n  version"
            );
            ExitCode::from(2)
        }
    }
}

fn cmd_run(args: &[String]) -> ExitCode {
    let Some(path) = args.first().cloned() else {
        eprintln!("usage: trustquant run request.json");
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
        let env = trustquant_runtime::ResponseEnvelope::error(e, request.request_id.as_deref());
        println!("{}", serde_json::to_string_pretty(&env).unwrap());
        return ExitCode::FAILURE;
    }
    let (envelope, manifest) = trustquant_runtime::execute_and_record(&request);
    println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
    if let Some(m) = manifest {
        eprintln!(
            "recorded calculation_id={} at {}",
            m.calculation_id,
            trustquant_runtime::ctx()
                .manifest_store
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
        eprintln!("usage: trustquant replay <calculation-id> [--json]");
        return ExitCode::from(2);
    };
    let as_json = args.iter().any(|a| a == "--json");
    match replay_calculation(&id) {
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
            let env = trustquant_runtime::ResponseEnvelope::error(e, None);
            println!("{}", serde_json::to_string_pretty(&env).unwrap());
            ExitCode::FAILURE
        }
    }
}

fn cmd_manifest(args: &[String]) -> ExitCode {
    let Some(id) = args.first().cloned() else {
        eprintln!("usage: trustquant manifest <calculation-id>");
        return ExitCode::from(2);
    };
    match trustquant_runtime::get_manifest(&id) {
        Ok(m) => {
            println!("{}", serde_json::to_string_pretty(&m).unwrap());
            ExitCode::SUCCESS
        }
        Err(e) => {
            let env = trustquant_runtime::ResponseEnvelope::error(e, None);
            println!("{}", serde_json::to_string_pretty(&env).unwrap());
            ExitCode::FAILURE
        }
    }
}

fn cmd_provider(args: &[String]) -> ExitCode {
    if args.first().map(String::as_str) == Some("install") {
        // Explicit, user-invoked installation (ADR-0002 §3). This is the ONLY
        // code path that touches the network for the provider — first-run
        // computation never installs anything.
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let venv = root.join(".venv");
        let reqs = root.join("sidecar/requirements.txt");
        println!(
            "creating venv at {} (explicit install; this step uses the network)",
            venv.display()
        );
        let status = std::process::Command::new("python3")
            .args(["-m", "venv", venv.to_str().unwrap()])
            .status()
            .map_err(|e| format!("python3 -m venv failed: {e}"));
        match status {
            Ok(st) if st.success() => {}
            other => {
                eprintln!("venv creation failed: {other:?}");
                return ExitCode::FAILURE;
            }
        }
        let pip = venv.join("bin/pip");
        let index = std::env::var("PIP_INDEX_URL")
            .unwrap_or_else(|_| "https://pypi.org/simple".to_string());
        println!("installing pinned requirements from {reqs:?} via {index}");
        let st = std::process::Command::new(pip)
            .args(["install", "-r", reqs.to_str().unwrap(), "-i", &index])
            .status()
            .expect("pip spawn failed");
        if st.success() {
            println!("provider installed; verify with: trustquant provider check");
            return ExitCode::SUCCESS;
        }
        eprintln!("pip install failed; set PIP_INDEX_URL to a reachable mirror if needed");
        return ExitCode::FAILURE;
    }
    if args.first().map(String::as_str) != Some("check") {
        eprintln!(
            "usage: trustquant provider check   (channel check)\n       trustquant provider install (explicit pinned env setup)"
        );
        return ExitCode::from(2);
    }
    let script = trustquant_sidecar::sidecar_script_path();
    println!("interpreter: {}", trustquant_sidecar::python_command());
    println!("script:      {}", script.display());
    match trustquant_sidecar::SidecarHandle::start(&script, std::time::Duration::from_secs(15)) {
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

fn cmd_serve(args: &[String]) -> ExitCode {
    let port = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(7860);
    match trustquant_runtime::http::serve(port) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("serve failed: {e}");
            ExitCode::FAILURE
        }
    }
}
