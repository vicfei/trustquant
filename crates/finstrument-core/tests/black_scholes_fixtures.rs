//! Native consumption of QuantLib golden fixtures (ADR-0006 gate 3).
//! Every black_scholes case must pass under its own tolerance; the engine's
//! theta/vega conventions are exactly what these fixtures arbitrate.

use finstrument_fixtures::{compare_case, load_family};
use serde_json::{Value, json};

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run_case(input: &Value) -> Value {
    let params = json!({
        "option_type": input["option_type"],
        "spot": input["spot"],
        "strike": input["strike"],
        "risk_free_rate": input["risk_free"],
        "dividend_yield": input["dividend"],
        "volatility": input["vol"],
        "valuation_date": input["eval_date"],
        "maturity_date": input["maturity_date"],
        "day_counter": input["day_counter"],
    });
    let out = finstrument_core::dispatch("price_european_option", &params)
        .unwrap_or_else(|e| panic!("dispatch failed: {e:?}"));
    let mut flat = json!({
        "npv": out.result["npv"],
        "delta": out.result["greeks"]["delta"],
        "gamma": out.result["greeks"]["gamma"],
        "vega": out.result["greeks"]["vega"],
        "theta": out.result["greeks"]["theta"],
    });
    if let Some(t) = input["T"].as_f64() {
        flat["T_echo"] = json!(t);
        // internal T check: engine's own t must equal fixture T
        let t_engine = out.result["t"].as_f64().unwrap();
        assert!(
            (t_engine - t).abs() <= 1e-12 + 1e-12 * t.abs(),
            "T mismatch: engine {t_engine} vs fixture {t}"
        );
    }
    flat
}

#[test]
fn all_black_scholes_cases_pass() {
    let fam = load_family(&workspace_root(), "black_scholes");
    assert_eq!(
        fam.provenance["quantlib"]["upstream_release"]
            .as_str()
            .unwrap(),
        "1.33"
    );
    let mut failures = Vec::new();
    for case in &fam.cases {
        let actual = run_case(&case["input"]);
        if let Err(e) = compare_case(
            case["id"].as_str().unwrap(),
            &case["expected"],
            &actual,
            &case["tolerance"],
        ) {
            failures.push(e);
        }
    }
    assert_eq!(fam.cases.len(), 18, "category coverage per ADR-0003");
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}

#[test]
fn date_daycount_family_still_accepted() {
    let fam = load_family(&workspace_root(), "date_daycount");
    let mut failures = Vec::new();
    for case in &fam.cases {
        if case["kind"].as_str().unwrap() != "daycount" {
            continue;
        }
        let out = finstrument_core::dispatch("day_count_probe", &case["input"]); // not a tool; compute directly below
        drop(out);
        let input = &case["input"];
        let start =
            finstrument_core::date::Date::parse_iso(input["start"].as_str().unwrap()).unwrap();
        let end = finstrument_core::date::Date::parse_iso(input["end"].as_str().unwrap()).unwrap();
        let counter =
            finstrument_core::daycount::parse_counter(input["day_counter"].as_str().unwrap())
                .unwrap();
        let refp = input["ref_period_start"].as_str().map(|a| {
            (
                finstrument_core::date::Date::parse_iso(a).unwrap(),
                finstrument_core::date::Date::parse_iso(input["ref_period_end"].as_str().unwrap())
                    .unwrap(),
            )
        });
        let yf = finstrument_core::daycount::year_fraction(
            &finstrument_core::daycount::DayCountRequest {
                counter,
                start,
                end,
                ref_period: refp,
            },
        )
        .unwrap();
        let actual = json!({"year_fraction": yf, "actual_days": end.days_since(start)});
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
