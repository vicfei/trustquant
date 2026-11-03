//! In-house second-oracle cross-check (replaces the GPL FinancePy manual
//! check; see fixtures/rates_oracle/provenance.json). The oracle bootstraps
//! by per-pillar BISECTION and prices swaps by full float-cashflow summation
//! — different methods than this crate's closed-form solve and telescoping —
//! so agreement is genuine independence evidence, in CI.

use std::collections::BTreeMap;
use std::path::PathBuf;

use trustquant_core::date::Date;
use trustquant_core::rates::{CurveSpec, SwapSpec, bootstrap, price_vanilla_swap};
use trustquant_fixtures::load_family;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn d(s: &str) -> Date {
    Date::parse_iso(s).unwrap()
}

#[test]
fn independent_oracle_agrees_on_curve_and_swaps() {
    let fam = load_family(&root(), "rates_oracle");
    assert_eq!(fam.cases.len(), 3, "one case per scenario");
    let mut failures = Vec::new();

    for case in &fam.cases {
        let scenario = case["input"]["scenario"].as_str().unwrap();
        let mut rates = [0.0f64; 10];
        for (i, v) in case["input"]["swap_rates_1y_10y"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            rates[i] = v.as_f64().unwrap();
        }
        let spec = CurveSpec {
            eval_date: d(case["input"]["eval_date"].as_str().unwrap()),
            deposit_6m: case["input"]["deposit_6m"].as_f64().unwrap(),
            swap_rates_1y_10y: rates,
        };
        let curve = bootstrap(&spec).unwrap_or_else(|e| panic!("{scenario}: bootstrap: {e:?}"));

        // 1) pillar discounts vs the oracle's bisection bootstrap
        let want: BTreeMap<String, f64> =
            serde_json::from_value(case["expected"]["df_pillars"].clone()).unwrap();
        for (k, w) in &want {
            let g = curve
                .nodes
                .get(k)
                .copied()
                .unwrap_or_else(|| panic!("{scenario}: missing pillar {k}"));
            if (g - w).abs() > 1e-11 + 1e-9 * w.abs() {
                failures.push(format!("{scenario}: DF[{k}] {g} vs oracle {w}"));
            }
        }

        // 2) swap values vs the oracle's cashflow-summation pricing
        let swaps = case["expected"]["swaps"].as_object().unwrap();
        for (id, exp) in swaps {
            let years = exp["years"].as_u64().expect("oracle emits years") as u32;
            let out = price_vanilla_swap(&SwapSpec {
                curve: &curve,
                payer: id.contains("payer"),
                years,
                fixed_rate: None, // price at oracle's fair for npv comparison
            })
            .unwrap_or_else(|e| panic!("{id}: swap: {e:?}"));
            let want_fair = exp["fair_rate"].as_f64().unwrap();
            if (out.fair_rate - want_fair).abs() > 1e-10 + 1e-9 * want_fair.abs() {
                failures.push(format!(
                    "{id}: fair {} vs oracle {want_fair}",
                    out.fair_rate
                ));
            }
        }

        // 3) the oracle's telescope-gap diagnostic must be machine-zero
        for (id, exp) in swaps {
            let gap = exp["telescope_gap"].as_f64().unwrap();
            assert!(
                gap < 1e-12,
                "{id}: oracle telescope gap {gap} not machine-zero"
            );
        }
    }
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}
