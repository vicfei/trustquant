//! Native vs WASM differential over the rates chain (M2 exit criterion) and
//! the option engine. Requires the wasm module first:
//!   cargo build -p trustquant-wasm --target wasm32-unknown-unknown --release

use serde_json::{Value, json};
use trustquant_difftest::WasmDispatcher;
use trustquant_fixtures::load_family;

fn wasm_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("TRUSTQUANT_WASM") {
        return std::path::PathBuf::from(p);
    }
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-unknown-unknown/release/trustquant_wasm.wasm")
}

fn request(tool: &str, params: Value) -> Value {
    json!({
        "schema_version": trustquant_core::contract::SCHEMA_VERSION,
        "tool": tool,
        "params": params,
    })
}

fn native(tool: &str, params: &Value) -> Value {
    match trustquant_core::dispatch(tool, params) {
        Ok(out) => {
            json!({"status": "success", "engine_id": out.engine_id, "result": out.result, "warnings": out.warnings})
        }
        Err(e) => {
            json!({"status": "error", "engine_id": null, "result": null, "warnings": [], "error": serde_json::to_value(e).unwrap()})
        }
    }
}

fn assert_close(a: f64, b: f64, what: &str) {
    assert!(
        (a - b).abs() <= 1e-12 + 1e-10 * b.abs(),
        "native vs wasm drift at {what}: {a} vs {b}"
    );
}

#[test]
fn rates_chain_matches_between_native_and_wasm() {
    let path = wasm_path();
    assert!(
        path.exists(),
        "wasm module missing: {}. Run: cargo build -p trustquant-wasm --target wasm32-unknown-unknown --release",
        path.display()
    );
    let mut wasm = WasmDispatcher::from_file(&path).expect("instantiate");

    let fam = load_family(
        &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        "rates",
    );
    let curves: Vec<&Value> = fam.cases.iter().filter(|c| c["kind"] == "curve").collect();
    let swaps: Vec<&Value> = fam.cases.iter().filter(|c| c["kind"] == "swap").collect();

    for case in curves {
        let input = &case["input"];
        let n = native("bootstrap_yield_curve", input);
        let w = wasm
            .dispatch(&request("bootstrap_yield_curve", input.clone()))
            .expect("wasm curve");
        assert_eq!(n["status"], "success", "native bootstrap failed: {n}");
        assert_eq!(n["status"], w["status"]);
        let nn = &n["result"]["nodes"];
        let wn = &w["result"]["nodes"];
        assert_eq!(nn.as_array().unwrap().len(), wn.as_array().unwrap().len());
        for (a, b) in nn.as_array().unwrap().iter().zip(wn.as_array().unwrap()) {
            assert_eq!(a["date"], b["date"], "pillar date drift");
            assert_close(
                a["discount"].as_f64().unwrap(),
                b["discount"].as_f64().unwrap(),
                "df",
            );
        }
    }
    for case in swaps {
        let input = &case["input"];
        // rebuild full curve params (swap input lacks the quotes)
        let scenario = input["scenario"].as_str().unwrap();
        let dep = match scenario {
            "flat" => 0.030,
            "upward" => 0.025,
            _ => 0.040,
        };
        let swaps_rates: Vec<f64> = match scenario {
            "flat" => vec![0.030; 10],
            "upward" => vec![
                0.027, 0.028, 0.029, 0.030, 0.031, 0.032, 0.033, 0.034, 0.035, 0.036,
            ],
            _ => vec![
                0.038, 0.036, 0.034, 0.033, 0.032, 0.031, 0.030, 0.029, 0.028, 0.027,
            ],
        };
        let params = json!({
            "eval_date": input["eval_date"], "deposit_6m": dep, "swap_rates_1y_10y": swaps_rates,
            "swap_type": input["swap_type"], "years": input["years"],
            "fixed_rate": input["fixed_rate"],
        });
        let n = native("price_vanilla_swap", &params);
        let w = wasm
            .dispatch(&request("price_vanilla_swap", params))
            .expect("wasm swap");
        assert_eq!(n["status"], "success", "native swap: {n}");
        for key in ["npv", "fair_rate", "fixed_leg_npv", "floating_leg_npv"] {
            assert_close(
                n["result"][key].as_f64().unwrap(),
                w["result"][key].as_f64().unwrap(),
                key,
            );
        }
        // schedules: dates exact, floats within tolerance (no bitwise promise)
        let (nc, wc) = (
            n["result"]["fixed_coupons"].as_array().unwrap(),
            w["result"]["fixed_coupons"].as_array().unwrap(),
        );
        assert_eq!(nc.len(), wc.len(), "coupon count drift");
        for (a, b) in nc.iter().zip(wc) {
            assert_eq!(a["date"], b["date"], "coupon date drift");
            for f in ["yf", "df", "amount"] {
                if let (Some(x), Some(y)) = (a[f].as_f64(), b[f].as_f64()) {
                    assert_close(x, y, f);
                }
            }
        }
    }
}

#[test]
fn option_and_error_paths_match_wasm() {
    let path = wasm_path();
    assert!(path.exists(), "wasm module missing (make wasm)");
    let mut wasm = WasmDispatcher::from_file(&path).expect("instantiate");
    // option
    let p = json!({"option_type":"call","spot":100.0,"strike":105.0,"risk_free_rate":0.05,
                   "dividend_yield":0.02,"volatility":0.2,"valuation_date":"2023-06-21","maturity_date":"2024-06-20"});
    let n = native("price_european_option", &p);
    let w = wasm
        .dispatch(&request("price_european_option", p))
        .expect("wasm option");
    assert_close(
        n["result"]["npv"].as_f64().unwrap(),
        w["result"]["npv"].as_f64().unwrap(),
        "npv",
    );
    // error taxonomy identical
    let bad = json!({"option_type":"call","spot":100.0,"strike":105.0,"risk_free_rate":0.05,
                     "volatility":0.2,"valuation_date":"2023-02-30","maturity_date":"2024-06-20"});
    let n = native("price_european_option", &bad);
    let w = wasm
        .dispatch(&request("price_european_option", bad))
        .expect("wasm err");
    assert_eq!(
        serde_json::to_string(&n["error"]).unwrap(),
        serde_json::to_string(&w["error"]).unwrap()
    );
}
