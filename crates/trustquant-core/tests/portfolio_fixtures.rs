//! Native consumption of the pure-Python oracle fixtures for risk metrics
//! and stress tests (ADR-0003; oracle independent of the Rust code paths).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};
use trustquant_core::portfolio::{PricePanel, normalize_weights, risk_metrics, stress};
use trustquant_fixtures::{compare_case, load_family};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Rebuild the price panel for a fixture case: either the committed demo
/// dataset or an inline {ticker: [closes]} payload.
fn panel_for(input: &Value) -> PricePanel {
    if let Some(inline) = input.get("prices_inline").and_then(Value::as_object) {
        let mut series = BTreeMap::new();
        for (t, arr) in inline {
            let closes: Vec<f64> = arr
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_f64)
                .collect();
            series.insert(t.clone(), closes);
        }
        let tickers = series.keys().cloned().collect();
        return PricePanel { tickers, series };
    }
    // dataset: "demo_prices" -> load the committed synthetic CSV
    let csv = workspace_root().join("data/demo_prices.csv");
    let mut series: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut reader = csv::lite_reader(csv.to_str().unwrap());
    while let Some(row) = reader.next() {
        let (t, c) = row;
        series.entry(t).or_default().push(c);
    }
    let tickers = series.keys().cloned().collect();
    PricePanel { tickers, series }
}

mod csv {
    /// Minimal CSV reader for the committed fixture file (test-only).
    pub struct LiteReader {
        rows: std::vec::IntoIter<(String, f64)>,
    }

    pub fn lite_reader(path: &str) -> LiteReader {
        let text = std::fs::read_to_string(path).expect("read demo_prices.csv");
        let mut rows = Vec::new();
        for line in text.lines().skip(1) {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() == 3 {
                rows.push((
                    parts[1].to_string(),
                    parts[2].parse::<f64>().expect("close parses"),
                ));
            }
        }
        LiteReader {
            rows: rows.into_iter(),
        }
    }

    impl LiteReader {
        pub fn next(&mut self) -> Option<(String, f64)> {
            self.rows.next()
        }
    }
}

#[test]
fn all_risk_cases_pass() {
    let fam = load_family(&workspace_root(), "risk_metrics");
    let mut failures = Vec::new();
    for case in &fam.cases {
        let panel = panel_for(&case["input"]);
        let weights: BTreeMap<String, f64> = case["input"]["weights"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_f64().unwrap()))
            .collect();
        let w = normalize_weights(&panel, &weights).unwrap();
        let m = risk_metrics(&panel, &w, 0.0).unwrap();
        let actual = json!({
            "total_return": m.total_return,
            "annualized_return": m.annualized_return,
            "annualized_volatility": m.annualized_volatility,
            "max_drawdown": m.max_drawdown,
            "sharpe_ratio": m.sharpe_ratio,
            "observations": m.observations,
        });
        if let Err(e) = compare_case(
            case["id"].as_str().unwrap(),
            &case["expected"],
            &actual,
            &case["tolerance"],
        ) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}

#[test]
fn all_stress_cases_pass() {
    let fam = load_family(&workspace_root(), "stress_test");
    let mut failures = Vec::new();
    for case in &fam.cases {
        let panel = panel_for(&case["input"]);
        let weights: BTreeMap<String, f64> = case["input"]["weights"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_f64().unwrap()))
            .collect();
        let w = normalize_weights(&panel, &weights).unwrap();
        let shocks: BTreeMap<String, f64> = case["input"]["shocks"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_f64().unwrap()))
            .collect();
        let o = stress(&panel, &w, &shocks).unwrap();
        let actual = json!({
            "baseline_value": o.baseline_value,
            "shocked_value": o.shocked_value,
            "pnl": o.pnl,
            "portfolio_return": o.portfolio_return,
        });
        if let Err(e) = compare_case(
            case["id"].as_str().unwrap(),
            &case["expected"],
            &actual,
            &case["tolerance"],
        ) {
            failures.push(e);
        }
        // contribution invariant against oracle expectation
        let contrib_sum: f64 = o.contributions.values().sum();
        assert!(
            (contrib_sum - o.portfolio_return).abs() < 1e-10,
            "{}: contributions {} != return {}",
            case["id"].as_str().unwrap(),
            contrib_sum,
            o.portfolio_return
        );
    }
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}
