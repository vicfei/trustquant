//! Gate 1 of ADR-0006: library, CLI and MCP must return equivalent results
//! and identical error envelopes for the same requests. Equivalence key =
//! canonical result digest. The CLI runs as a real subprocess
//! (CARGO_BIN_EXE_trustquant) with an isolated TRUSTQUANT_DATA_DIR.

use serde_json::{Value, json};
use std::process::Command;
use trustquant_core::contract::RequestEnvelope;
use trustquant_runtime::mcp::handle_message;

fn isolated_data_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "finstr-conf-{}-{}",
        tag,
        trustquant_runtime::store::new_calculation_id()
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

/// serde_json's text parser can shift certain 17-digit decimals by one ulp
/// (deterministically; see ADR-0001 known limitations). Cross-entry equality
/// therefore compares NORMALIZED TEXT: every side is round-tripped through
/// the same serialize/parse pipeline so any drift is applied identically.
fn normalized_text(env: &Value) -> String {
    let once = serde_json::to_string(&env["result"]).unwrap();
    let parsed: Value = serde_json::from_str(&once).unwrap();
    serde_json::to_string(&parsed).unwrap()
}

fn digest_of(env: &Value) -> String {
    trustquant_core::canonical::digest(
        &serde_json::from_str::<Value>(&normalized_text(env)).unwrap(),
    )
    .unwrap()
}

fn error_of(env: &Value) -> String {
    serde_json::to_string(&env["error"]).unwrap()
}

fn run_cli(data_dir: &std::path::Path, request: &Value) -> Value {
    let req_file = data_dir.join("request.json");
    std::fs::write(&req_file, serde_json::to_string(request).unwrap()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_trustquant"))
        .arg("run")
        .arg(req_file.to_str().unwrap())
        .env("TRUSTQUANT_DATA_DIR", data_dir)
        .output()
        .expect("spawn trustquant");
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
    let env = trustquant_runtime::execute(
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
    let replay = Command::new(env!("CARGO_BIN_EXE_trustquant"))
        .args(["replay", cid, "--json"])
        .env("TRUSTQUANT_DATA_DIR", &data_dir)
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
    let out = Command::new(env!("CARGO_BIN_EXE_trustquant"))
        .args(["replay", "00000000-0000-4000-8000-000000000dead", "--json"])
        .env("TRUSTQUANT_DATA_DIR", &data_dir)
        .output()
        .expect("spawn replay");
    let env: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(env["status"], "error");
    assert_eq!(env["error"]["error_type"], "replay_unavailable");
    assert_eq!(env["error"]["correctable"], false);
    std::fs::remove_dir_all(&data_dir).ok();
}

#[test]
fn portfolio_tools_equivalent_across_entries() {
    let data_dir = isolated_data_dir("port");
    // 1) load via CLI (also persists the dataset into the isolated store)
    let load_req = json!({
        "schema_version": "0.1",
        "tool": "load_market_dataset",
        "params": {"builtin": "demo_prices"}
    });
    let cli_load = run_cli(&data_dir, &load_req);
    assert_eq!(cli_load["status"], "success", "cli load: {cli_load}");
    let dataset_id = cli_load["result"]["dataset_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 2b) also load in-process (lib/mcp read the test-process store; the
    // content-addressed id is identical)
    let lib_load = run_lib(&load_req);
    assert_eq!(lib_load["status"], "success", "lib load: {lib_load}");
    assert_eq!(
        lib_load["result"]["dataset_id"].as_str().unwrap(),
        dataset_id
    );

    // 2) risk + stress with the same dataset through all three entries
    for tool in ["analyze_portfolio_risk", "stress_test_portfolio"] {
        let params = if tool == "analyze_portfolio_risk" {
            json!({"dataset": dataset_id, "weights": {"SYNTH_A": 0.34, "SYNTH_B": 0.33, "SYNTH_C": 0.33}})
        } else {
            json!({"dataset": dataset_id, "weights": {"SYNTH_A": 0.34, "SYNTH_B": 0.33, "SYNTH_C": 0.33},
                   "scenarios": [{"name": "s", "shocks": {"SYNTH_A": -0.2}}]})
        };
        let req = json!({"schema_version": "0.1", "request_id": "conf-port", "tool": tool, "params": params});
        let lib = run_lib(&req);
        let mcp = run_mcp(&req);
        // CLI subprocess needs the isolated data dir to see the dataset
        let cli = run_cli(&data_dir, &req);
        assert_eq!(lib["status"], "success", "{tool}: {lib}");
        let d_lib = digest_of(&lib);
        assert_eq!(d_lib, digest_of(&mcp), "{tool}: lib vs mcp");
        assert_eq!(d_lib, digest_of(&cli), "{tool}: lib vs cli");
    }
    std::fs::remove_dir_all(&data_dir).ok();
}

#[test]
fn optimize_equivalent_across_entries_and_recorded_by_provider() {
    let data_dir = isolated_data_dir("opt");
    let load_req = json!({"schema_version": "0.1", "tool": "load_market_dataset", "params": {"builtin": "demo_prices"}});
    let cli_load = run_cli(&data_dir, &load_req);
    assert_eq!(cli_load["status"], "success");
    let dataset_id = cli_load["result"]["dataset_id"]
        .as_str()
        .unwrap()
        .to_string();
    let lib_load = run_lib(&load_req);
    assert_eq!(lib_load["status"], "success", "lib load: {lib_load}");
    let req = json!({
        "schema_version": "0.1", "request_id": "conf-opt", "tool": "optimize_portfolio",
        "params": {"dataset": dataset_id, "objective": "min_volatility", "max_weight": 0.6}
    });
    let lib = run_lib(&req);
    let mcp = run_mcp(&req);
    let cli = run_cli(&data_dir, &req);
    assert_eq!(lib["status"], "success", "optimize via lib: {lib}");
    let d_lib = digest_of(&lib);
    assert_eq!(d_lib, digest_of(&mcp), "optimize: lib vs mcp");
    assert_eq!(d_lib, digest_of(&cli), "optimize: lib vs cli");
    // provider identity is recorded, not silently substituted
    assert_eq!(mcp["provider_id"], "python-sidecar");
    assert_eq!(mcp["engine_id"], "portfolio_optimize.v1");
    // same inputs -> deterministic result on the same environment (replay)
    let cid = cli["calculation_id"].as_str().unwrap();
    let replay = Command::new(env!("CARGO_BIN_EXE_trustquant"))
        .args(["replay", cid, "--json"])
        .env("TRUSTQUANT_DATA_DIR", &data_dir)
        .output()
        .expect("spawn replay");
    let report: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(
        report["result_digest_match"], true,
        "optimize replay: {report}"
    );
    std::fs::remove_dir_all(&data_dir).ok();
}

#[test]
fn request_envelope_roundtrip_type() {
    let req: RequestEnvelope = serde_json::from_value(request_json()).unwrap();
    req.validate_schema_version().unwrap();
}
