//! Optimize fixtures through the FULL channel (runtime -> sidecar ->
//! PyPortfolioOpt) against the independent pure-Python oracle's analytic
//! references (ADR-0003). Requires the pinned provider environment:
//!   trustquant provider install   (or .venv with sidecar/requirements.txt)
//! The test fails loudly when the provider is absent — no silent skipping.

use serde_json::{Value, json};
use trustquant_fixtures::load_family;
use trustquant_runtime::provider::ProviderManager;

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn demo_dataset_id() -> String {
    let env = trustquant_runtime::execute(
        "load_market_dataset",
        &json!({"builtin": "demo_prices"}),
        None,
    );
    env.result.expect("demo dataset loads")["dataset_id"]
        .as_str()
        .expect("dataset_id")
        .to_string()
}

#[test]
fn optimize_matches_analytic_references() {
    let fam = load_family(&workspace_root(), "optimize");
    let ds = demo_dataset_id();
    let pm = ProviderManager::new();
    let mut exact_checked = 0;
    let mut invariant_checked = 0;
    let mut failures = Vec::new();

    for case in &fam.cases {
        let id = case["id"].as_str().unwrap();
        let objective = case["input"]["objective"]
            .as_str()
            .unwrap_or("min_volatility");
        let side_params = if case["input"].get("prices_inline").is_some() {
            json!({
                "tickers": case["input"]["prices_inline"].as_object().unwrap().keys().collect::<Vec<_>>(),
                "prices": case["input"]["prices_inline"],
                "objective": objective,
                "risk_free_rate": case["input"].get("risk_free_rate").and_then(Value::as_f64).unwrap_or(0.0),
                "max_weight": case["input"].get("max_weight").and_then(Value::as_f64).unwrap_or(1.0),
            })
        } else {
            json!({
                "tickers": ["SYNTH_A", "SYNTH_B", "SYNTH_C"],
                "dataset_id": ds,
                "objective": objective,
                "risk_free_rate": case["input"].get("risk_free_rate").and_then(Value::as_f64).unwrap_or(0.0),
                "max_weight": case["input"].get("max_weight").and_then(Value::as_f64).unwrap_or(1.0),
            })
        };
        // dataset case: fetch prices from the loaded dataset store
        let side_params = if side_params.get("dataset_id").is_some() {
            let did = side_params["dataset_id"].as_str().unwrap().to_string();
            let ds_json = std::fs::read_to_string(
                trustquant_runtime::ctx()
                    .dataset_store
                    .datasets_dir()
                    .join(format!("{}.json", did)),
            )
            .expect("dataset stored");
            let parsed: Value = serde_json::from_str(&ds_json).unwrap();
            json!({
                "tickers": parsed["tickers"],
                "prices": parsed["close"],
                "objective": side_params["objective"],
                "risk_free_rate": side_params["risk_free_rate"],
                "max_weight": side_params["max_weight"],
            })
        } else {
            side_params
        };

        let result = match pm.call("optimize_portfolio", &side_params) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!(
                    "{id}: provider error: {} ({})",
                    e.error_type_str(),
                    e.message
                ));
                continue;
            }
        };

        // invariants (every case): long-only bounds + sum-to-1 within solver tolerance
        let weights = &result["weights"];
        let tickers: Vec<String> = weights.as_object().unwrap().keys().cloned().collect();
        let sum: f64 = tickers.iter().map(|t| weights[t].as_f64().unwrap()).sum();
        let max_w = side_params["max_weight"].as_f64().unwrap();
        let mut invariant_ok = (sum - 1.0).abs() < 1e-3;
        for t in &tickers {
            let v = weights[t].as_f64().unwrap();
            if v < -1e-6 || v > max_w + 1e-4 {
                invariant_ok = false;
            }
        }
        if !invariant_ok {
            failures.push(format!("{id}: constraint invariants violated (sum={sum})"));
        }
        invariant_checked += 1;

        // exact analytic comparisons where the oracle provides them
        if let Some(exp_w) = case["expected"].get("weights") {
            for (t, w_ref) in exp_w.as_object().unwrap() {
                let got = weights[t].as_f64().unwrap();
                let want = w_ref.as_f64().unwrap();
                if (got - want).abs() > 5e-4 + 1e-6 * want.abs() {
                    failures.push(format!("{id}: weight[{t}] = {got} vs analytic {want}"));
                }
            }
            let er = result["expected_return"].as_f64().unwrap();
            let er_ref = case["expected"]["expected_return"].as_f64().unwrap();
            if (er - er_ref).abs() > 1e-8 + 1e-6 * er_ref.abs() {
                failures.push(format!("{id}: expected_return {er} vs analytic {er_ref}"));
            }
            let vol = result["volatility"].as_f64().unwrap();
            let vol_ref = case["expected"]["volatility"].as_f64().unwrap();
            if (vol - vol_ref).abs() > 1e-10 + 1e-6 * vol_ref.abs() {
                failures.push(format!("{id}: volatility {vol} vs analytic {vol_ref}"));
            }
            exact_checked += 1;
        }
    }
    assert_eq!(fam.cases.len(), 5);
    assert!(
        exact_checked >= 3,
        "expected at least 3 exact-reference cases, got {exact_checked}"
    );
    assert!(invariant_checked >= 5);
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}
