//! M2 acceptance: calendar + curve + swap semantics against the QuantLib
//! 1.33 fixtures. Discrete results (dates) must match EXACTLY; floats within
//! per-metric tolerance. Helper residuals prove bootstrap exactness.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};
use trustquant_core::calendar::{
    CalendarId, adjust, advance_months, is_business_day, parse_convention,
};
use trustquant_core::date::{Date, Weekday};
use trustquant_core::daycount::{DayCountRequest, DayCounterId, year_fraction};
use trustquant_core::rates::{CurveSpec, SwapSpec, bootstrap, discount_at, price_vanilla_swap};
use trustquant_fixtures::{compare_case, load_family};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn d(s: &str) -> Date {
    Date::parse_iso(s).unwrap()
}

fn cal_of(name: &str) -> CalendarId {
    match name {
        "TARGET" => CalendarId::Target,
        "WeekendsOnly" => CalendarId::WeekendsOnly,
        "NullCalendar" => CalendarId::NullCalendar,
        _ => panic!("unknown calendar {name}"),
    }
}

#[test]
fn all_calendar_cases_pass() {
    let fam = load_family(&root(), "calendar");
    let mut failures = Vec::new();
    assert_eq!(fam.cases.len(), 80);
    for case in &fam.cases {
        let id = case["id"].as_str().unwrap();
        let input = &case["input"];
        let cal = cal_of(input["calendar"].as_str().unwrap());
        let date = d(input["date"].as_str().unwrap());
        let op = input["op"].as_str().unwrap();
        let actual = if op == "is_business_day" {
            json!({"is_business_day": is_business_day(cal, date)})
        } else if let Some(conv_name) = op.strip_prefix("adjust:") {
            let conv = parse_convention(conv_name).unwrap();
            let r = adjust(cal, date, conv);
            json!({"iso": r.to_iso(), "weekday": r.weekday().as_str()})
        } else if let Some(conv_name) = op.strip_prefix("advance_months:") {
            let conv = parse_convention(conv_name).unwrap();
            let months = input["months"].as_i64().unwrap() as i32;
            let r = advance_months(cal, date, months, conv).unwrap();
            json!({"iso": r.to_iso(), "weekday": r.weekday().as_str()})
        } else {
            panic!("unknown op {op}");
        };
        if let Err(e) = compare_case(id, &case["expected"], &actual, &case["tolerance"]) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}

fn spec_from(input: &Value) -> CurveSpec {
    let mut rates = [0.0f64; 10];
    for (i, v) in input["swap_rates_1y_10y"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        rates[i] = v.as_f64().unwrap();
    }
    CurveSpec {
        eval_date: d(input["eval_date"].as_str().unwrap()),
        deposit_6m: input["deposit_6m"].as_f64().unwrap(),
        swap_rates_1y_10y: rates,
    }
}

fn cont_zero_act365(curve: &trustquant_core::rates::BootstrappedCurve, date: &str) -> f64 {
    let t = year_fraction(&DayCountRequest {
        counter: DayCounterId::Actual365Fixed,
        start: curve.spot,
        end: d(date),
        ref_period: None,
    })
    .unwrap();
    let df = discount_at(curve, d(date)).unwrap();
    -(df.ln()) / t
}

#[test]
fn curve_bootstrap_matches_quantlib() {
    let fam = load_family(&root(), "rates");
    let curves: Vec<&Value> = fam.cases.iter().filter(|c| c["kind"] == "curve").collect();
    assert_eq!(curves.len(), 3);
    let mut failures = Vec::new();
    for case in curves {
        let id = case["id"].as_str().unwrap();
        let spec = spec_from(&case["input"]);
        let curve = bootstrap(&spec).unwrap_or_else(|e| panic!("{id}: bootstrap failed: {e:?}"));

        // spot exact
        assert_eq!(
            curve.spot.to_iso(),
            case["expected"]["spot_date"].as_str().unwrap(),
            "{id}"
        );

        // pillar set must be identical
        let want: BTreeMap<String, f64> =
            serde_json::from_value(case["expected"]["pillar_dates"].clone()).unwrap();
        let got: BTreeMap<String, f64> = curve
            .nodes
            .iter()
            .map(|(k, v)| {
                // fixture dumps full precision; ours is f64 native
                (k.clone(), *v)
            })
            .collect();
        assert_eq!(got.len(), want.len(), "{id}: pillar count");
        for (k, w) in &want {
            let g = got[k];
            if (g - w).abs() > 1e-10 + 1e-9 * w.abs() {
                failures.push(format!("{id}: DF[{k}] = {g} vs QuantLib {w}"));
            }
        }

        // zero rates (continuous, Act/365F) at every pillar
        let zeros: BTreeMap<String, f64> =
            serde_json::from_value(case["expected"]["zero_rates_cont_365"].clone()).unwrap();
        for (k, w) in &zeros {
            if *k == curve.spot.to_iso() {
                continue;
            }
            let g = cont_zero_act365(&curve, k);
            if (g - w).abs() > 1e-9 + 1e-8 * w.abs() {
                failures.push(format!("{id}: zero[{k}] = {g} vs QuantLib {w}"));
            }
        }

        // bootstrap exactness: residuals ~ 0 (ours linear by construction;
        // QuantLib's dumped residuals must also be ~0)
        let ql_res: Vec<f64> = case["expected"]["helper_residuals"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_f64)
            .collect();
        assert!(
            ql_res.iter().all(|r| r.abs() < 1e-9),
            "{id}: QuantLib residuals not ~0"
        );
        assert!(
            curve.residuals.iter().all(|r| r.abs() < 1e-9),
            "{id}: our residuals not ~0"
        );
    }
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}

#[test]
fn vanilla_swap_matches_quantlib() {
    let fam = load_family(&root(), "rates");
    let swaps: Vec<&Value> = fam.cases.iter().filter(|c| c["kind"] == "swap").collect();
    assert_eq!(swaps.len(), 9);
    let mut failures = Vec::new();
    for case in swaps {
        let id = case["id"].as_str().unwrap();
        let input = &case["input"];
        let spec = spec_from(&json!({
            "eval_date": input["eval_date"],
            "deposit_6m": rates_dep(input["scenario"].as_str().unwrap()),
            "swap_rates_1y_10y": rates_swaps(input["scenario"].as_str().unwrap()),
        }));
        let curve = bootstrap(&spec).unwrap();
        let out = price_vanilla_swap(&SwapSpec {
            curve: &curve,
            payer: input["swap_type"] == "payer",
            years: input["years"].as_u64().unwrap() as u32,
            fixed_rate: input["fixed_rate"].as_f64(),
        })
        .unwrap_or_else(|e| panic!("{id}: swap failed: {e:?}"));

        // floats: NPV / fair / legs
        for (key, got) in [
            ("npv", out.npv),
            ("fair_rate", out.fair_rate),
            ("fixed_leg_npv", out.fixed_leg_npv),
            ("floating_leg_npv", out.floating_leg_npv),
        ] {
            let want = case["expected"][key].as_f64().unwrap();
            if (got - want).abs() > 1e-10 + 1e-9 * want.abs() {
                failures.push(format!("{id}: {key} = {got} vs QuantLib {want}"));
            }
        }

        // discrete: fixed coupon dates and year fractions must match exactly
        let want_fixed: Vec<(String, f64)> = case["expected"]["fixed_leg"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["date"].as_str().unwrap().to_string(),
                    c["yf"].as_f64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            out.fixed_coupons.len(),
            want_fixed.len(),
            "{id}: fixed coupon count"
        );
        for (i, (wd, wyf)) in want_fixed.iter().enumerate() {
            let got = &out.fixed_coupons[i];
            assert_eq!(got["date"].as_str().unwrap(), wd, "{id}: fixed date {i}");
            let gyf = got["yf"].as_f64().unwrap();
            assert!(
                (gyf - wyf).abs() < 1e-15,
                "{id}: fixed yf {i}: {gyf} vs {wyf}"
            );
        }
        let want_float: Vec<String> = case["expected"]["float_leg_dates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["date"].as_str().unwrap().to_string())
            .collect();
        let got_float: Vec<String> = out
            .float_dates
            .iter()
            .map(|c| c["date"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(got_float, want_float, "{id}: float dates");
    }
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}

fn rates_dep(scenario: &str) -> f64 {
    match scenario {
        "flat" => 0.030,
        "upward" => 0.025,
        _ => 0.040,
    }
}

fn rates_swaps(scenario: &str) -> Vec<f64> {
    match scenario {
        "flat" => vec![0.030; 10],
        "upward" => vec![
            0.027, 0.028, 0.029, 0.030, 0.031, 0.032, 0.033, 0.034, 0.035, 0.036,
        ],
        _ => vec![
            0.038, 0.036, 0.034, 0.033, 0.032, 0.031, 0.030, 0.029, 0.028, 0.027,
        ],
    }
    .into_iter()
    .collect()
}

#[test]
fn weekday_helper_exposed() {
    // weekday enum used by calendar tests
    assert_eq!(d("2023-06-23").weekday().as_str(), "Fri");
    let _ = Weekday::Mon;
}
