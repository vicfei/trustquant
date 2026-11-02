//! Gate 1 of ADR-0006: library, CLI and MCP must return equivalent results
//! and identical error envelopes for the same requests. Equivalence key =
//! canonical result digest. The CLI runs as a real subprocess
//! (CARGO_BIN_EXE_finstrument) with an isolated FINSTRUMENT_DATA_DIR.

use finstrument_core::contract::RequestEnvelope;
use finstrument_runtime::mcp::handle_message;
use serde_json::{Value, json};
use std::process::Command;

fn isolated_data_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "finstr-conf-{}-{}",
        tag,
        finstrument_runtime::store::new_calculation_id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn request_json() -> Value {
    json!({
        "schema_version": "0.1",
        "request_id": "conformance-1",
        "tool": "price_european_option",
        "params": {
            "option_type": "call",
            "spot": 100.0,
            "strike": 105.0,
            "risk_free_rate": 0.05,
            "dividend_yield": 0.02,
            "volatility": 0.2,
            "valuation_date": "2023-06-21",
            "maturity_date": "2024-06-20"
        }
    })
}

fn bad_request_json() -> Value {
    json!({
        "schema_version": "0.1",
        "tool": "price_european_option",
        "params": {
            "option_type": "call",
            "spot": 100.0,
            "strike": 105.0,
            "risk_free_rate": 0.05,
            "volatility": 0.2,
            "valuation_date": "2023-13-01",
            "maturity_date": "2024-06-20"
        }
    })
}

fn digest_of(env: &Value) -> String {
    finstrument_core::canonical::digest(&env["result"]).unwrap()
}

fn error_of(env: &Value) -> String {
    serde_json::to_string(&env["error"]).unwrap()
}

fn run_cli(data_dir: &std::path::Path, request: &Value) -> Value {
    let req_file = data_dir.join("request.json");
    std::fs::write(&req_file, serde_json::to_string(request).unwrap()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_finstrument"))
        .arg("run")
        .arg(req_file.to_str().unwrap())
        .env("FINSTRUMENT_DATA_DIR", data_dir)
        .output()
        .expect("spawn finstrument");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "CLI stdout not JSON: {e}; stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn run_mcp(request: &Value) -> Value {
    let arguments = json!({
        "schema_version": request["schema_version"],
        "request_id": request["request_id"],
        "params": request["params"],
    });
    let resp = handle_message(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": request["tool"], "arguments": arguments}
    }))
    .unwrap();
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    serde_json::from_str(text).unwrap()
}

fn run_lib(request: &Value) -> Value {
    let env = finstrument_runtime::execute(
        request["tool"].as_str().unwrap(),
        &request["params"],
        request["request_id"].as_str(),
    );
    serde_json::to_value(&env).unwrap()
}

#[test]
fn same_request_equivalent_across_lib_cli_mcp() {
    let data_dir = isolated_data_dir("ok");
    let req = request_json();

    let lib = run_lib(&req);
    let mcp = run_mcp(&req);
    let cli = run_cli(&data_dir, &req);

    assert_eq!(lib["status"], "success");
    let d_lib = digest_of(&lib);
    let d_mcp = digest_of(&mcp);
    let d_cli = digest_of(&cli);
    assert_eq!(d_lib, d_mcp, "lib vs mcp result digest");
    assert_eq!(d_lib, d_cli, "lib vs cli result digest");

    // conventions echoed identically everywhere (gate 2 evidence)
    assert_eq!(
        mcp["result"]["conventions"]["rates"],
        "continuously_compounded"
    );
    assert_eq!(
        mcp["result"]["units"]["vega"]["convention"],
        "per 1.00 volatility (not per 1%)"
    );
    assert_eq!(
        mcp["result"]["conventions"]["day_counter"],
        "Actual365Fixed"
    );

    // CLI recorded a manifest; replay round-trip must pass (gate 5)
    let cid = cli["calculation_id"].as_str().unwrap();
    let replay = Command::new(env!("CARGO_BIN_EXE_finstrument"))
        .args(["replay", cid, "--json"])
        .env("FINSTRUMENT_DATA_DIR", &data_dir)
        .output()
        .expect("spawn replay");
    let report: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(
        report["result_digest_match"], true,
        "replay report: {report}"
    );
    assert_eq!(report["manifest_hash_valid"], true);
    std::fs::remove_dir_all(&data_dir).ok();
}

#[test]
fn same_bad_request_identical_errors_across_entries() {
    let data_dir = isolated_data_dir("err");
    let req = bad_request_json();

    let lib = run_lib(&req);
    let mcp = run_mcp(&req);
    let cli = run_cli(&data_dir, &req);

    for env in [&lib, &mcp, &cli] {
        assert_eq!(env["status"], "error");
        assert_eq!(env["error"]["error_type"], "invalid_input");
    }
    let e_lib = error_of(&lib);
    assert_eq!(e_lib, error_of(&mcp), "lib vs mcp error envelope");
    assert_eq!(e_lib, error_of(&cli), "lib vs cli error envelope");
    std::fs::remove_dir_all(&data_dir).ok();
}

#[test]
fn replay_of_unknown_id_is_replay_unavailable() {
    let data_dir = isolated_data_dir("miss");
    let out = Command::new(env!("CARGO_BIN_EXE_finstrument"))
        .args(["replay", "00000000-0000-4000-8000-000000000dead", "--json"])
        .env("FINSTRUMENT_DATA_DIR", &data_dir)
        .output()
        .expect("spawn replay");
    let env: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(env["status"], "error");
    assert_eq!(env["error"]["error_type"], "replay_unavailable");
    assert_eq!(env["error"]["correctable"], false);
    std::fs::remove_dir_all(&data_dir).ok();
}

#[test]
fn request_envelope_roundtrip_type() {
    let req: RequestEnvelope = serde_json::from_value(request_json()).unwrap();
    req.validate_schema_version().unwrap();
}
