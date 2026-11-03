//! Single-curve yield-curve bootstrapping and vanilla-swap pricing (M2, EUR
//! slice). Semantics pinned by the `rates` fixtures (QuantLib 1.33
//! PiecewiseYieldCurve<Discount, LogLinear>; deposits Act/360 + annual
//! 30E/360 swaps vs 6M Euribor-style float, spot start T+2 TARGET).
//!
//! Bootstrap is exact-sequential (no iterative solver): annual fixed coupons
//! fall on pillar dates and the floating leg telescopes in single-curve, so
//! each pillar discount solves a LINEAR equation:
//!   DF_N = (1 - R * sum_{i<N} tau_i * DF_i) / (1 + R * tau_N)

use crate::calendar::{BusinessDayConvention, CalendarId, Frequency, adjust, schedule, spot_date};
use crate::contract::{ErrorEnvelope, ToolOutput};
use crate::date::{Date, add_months_signed};
use crate::daycount::{DayCountRequest, DayCounterId, year_fraction};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const CURVE_ENGINE_ID: &str = "curve_bootstrap.v1";
pub const SWAP_ENGINE_ID: &str = "vanilla_swap.v1";

pub struct CurveSpec {
    pub eval_date: Date,
    pub deposit_6m: f64,
    pub swap_rates_1y_10y: [f64; 10],
}

impl CurveSpec {
    pub fn spot(&self) -> Result<Date, ErrorEnvelope> {
        spot_date(self.eval_date, 2)
    }
}

pub struct BootstrappedCurve {
    pub spot: Date,
    /// Pillar dates (adjusted) -> discount factor; first entry is the spot
    /// itself with DF = 1.
    pub nodes: BTreeMap<String, f64>,
    pub deposit_maturity: String,
    /// Helper re-pricing residuals (bootstrap exactness evidence).
    pub residuals: Vec<f64>,
}

fn yf(counter: DayCounterId, start: Date, end: Date) -> Result<f64, ErrorEnvelope> {
    year_fraction(&DayCountRequest {
        counter,
        start,
        end,
        ref_period: None,
    })
}

/// Unadjusted maturity of an N-year swap: spot advanced by N years with
/// end-of-month clamping (mirrors QuantLib Calendar::advance Unadjusted).
fn year_maturity(spot: Date, years: u32) -> Result<Date, ErrorEnvelope> {
    add_months_signed(spot, years as i32 * 12).map_err(ErrorEnvelope::internal)
}

pub fn bootstrap(spec: &CurveSpec) -> Result<BootstrappedCurve, ErrorEnvelope> {
    if !(0.0..=0.25).contains(&spec.deposit_6m) || !spec.deposit_6m.is_finite() {
        return Err(ErrorEnvelope::invalid_market_data(format!(
            "deposit_6m rate {} outside sane range [0, 0.25]",
            spec.deposit_6m
        )));
    }
    if spec
        .swap_rates_1y_10y
        .iter()
        .any(|r| !r.is_finite() || !(0.0..=0.25).contains(r))
    {
        return Err(ErrorEnvelope::invalid_market_data(
            "swap rates must be finite numbers in [0, 0.25]".to_string(),
        ));
    }
    let spot = spec.spot()?;
    let cal = CalendarId::Target;
    let mf = BusinessDayConvention::ModifiedFollowing;

    // --- deposit pillar: 6M Act/360 from spot ---
    let dep_maturity_unadj = add_months_signed(spot, 6).map_err(ErrorEnvelope::internal)?;
    let dep_maturity = adjust(cal, dep_maturity_unadj, mf);
    let dep_yf = yf(DayCounterId::Actual360, spot, dep_maturity)?;
    let df_dep = 1.0 / (1.0 + spec.deposit_6m * dep_yf);

    // --- swap pillars: sequential linear solves ---
    // fixed leg: annual 30E/360 on adjusted schedule dates (accrual = adjusted
    // date to adjusted date, QuantLib Schedule semantics), payer par condition
    //   R * sum tau_i DF_i = 1 - DF_N   (float telescopes: DF(spot)=1 ... DF_N)
    let mut nodes: BTreeMap<String, f64> = BTreeMap::new();
    nodes.insert(spot.to_iso(), 1.0);
    nodes.insert(dep_maturity.to_iso(), df_dep);
    let mut residuals = vec![0.0f64]; // deposit reprices exactly by construction
    let mut prev_dates: Vec<(Date, f64)> = vec![(spot, 1.0)];

    for (idx, rate) in spec.swap_rates_1y_10y.iter().enumerate() {
        let years = (idx + 1) as u32;
        let maturity_unadj = year_maturity(spot, years)?;
        let fixed_dates = schedule(cal, spot, maturity_unadj, Frequency::Annual, mf)?;
        // schedule[0] == adjusted spot; coupons are consecutive pairs
        let mut acc = 0.0f64; // sum of known coupon PVs (dates before the pillar)
        for w in fixed_dates.windows(2) {
            let (d0, d1) = (w[0], w[1]);
            if d1 >= maturity_unadj_adjusted_pillar(&fixed_dates) {
                break;
            }
            let tau = yf(DayCounterId::Thirty360European, d0, d1)?;
            let df = nodes.get(&d1.to_iso()).copied().ok_or_else(|| {
                ErrorEnvelope::internal(format!("pillar missing for {}", d1.to_iso()))
            })?;
            acc += tau * df;
        }
        let pillar = *fixed_dates.last().expect("non-empty schedule");
        let prev = fixed_dates[fixed_dates.len() - 2];
        let tau_n = yf(DayCounterId::Thirty360European, prev, pillar)?;
        let df_n = (1.0 - rate * acc) / (1.0 + rate * tau_n);
        if !df_n.is_finite() || df_n <= 0.0 {
            return Err(ErrorEnvelope::no_convergence(format!(
                "bootstrap produced invalid discount {} at pillar {}",
                df_n,
                pillar.to_iso()
            )));
        }
        // residual: (R * (acc + tau_n * df_n) + df_n) - 1  == 0 by construction
        residuals.push(rate * (acc + tau_n * df_n) + df_n - 1.0);
        nodes.insert(pillar.to_iso(), df_n);
        prev_dates.push((pillar, df_n));
    }
    let _ = &prev_dates;
    Ok(BootstrappedCurve {
        spot,
        nodes,
        deposit_maturity: dep_maturity.to_iso(),
        residuals,
    })
}

fn maturity_unadj_adjusted_pillar(fixed_dates: &[Date]) -> Date {
    *fixed_dates.last().expect("non-empty schedule")
}

/// Log-linear discount interpolation between nodes (piecewise constant
/// forward in log-DF space; the M2 declared interpolation).
pub fn discount_at(curve: &BootstrappedCurve, d: Date) -> Result<f64, ErrorEnvelope> {
    if let Some(df) = curve.nodes.get(&d.to_iso()) {
        return Ok(*df);
    }
    let keys: Vec<&String> = curve.nodes.keys().collect();
    if keys.is_empty() {
        return Err(ErrorEnvelope::internal("empty curve".into()));
    }
    let parse = |s: &str| Date::parse_iso(s).map_err(ErrorEnvelope::internal);
    let first = parse(keys[0])?;
    let last = parse(keys[keys.len() - 1])?;
    if d <= first {
        return Ok(curve.nodes[keys[0]]);
    }
    if d >= last {
        return Ok(curve.nodes[keys[keys.len() - 1]]); // extrapolation beyond last pillar is not declared in M2; clamp
    }
    for w in keys.windows(2) {
        let a = parse(w[0])?;
        let b = parse(w[1])?;
        if d > a && d < b {
            let df_a = curve.nodes[w[0]];
            let df_b = curve.nodes[w[1]];
            let t = f64::from(d.days_since(a)) / f64::from(b.days_since(a));
            return Ok(df_a * (df_b / df_a).powf(t));
        }
    }
    Err(ErrorEnvelope::internal("interpolation fell through".into()))
}

/// Simple forward rate between two dates (Act/360): (DF(a)/DF(b) - 1) / yf.
pub fn forward_act360(curve: &BootstrappedCurve, a: Date, b: Date) -> Result<f64, ErrorEnvelope> {
    let df_a = discount_at(curve, a)?;
    let df_b = discount_at(curve, b)?;
    let tau = yf(DayCounterId::Actual360, a, b)?;
    if tau <= 0.0 {
        return Err(ErrorEnvelope::invalid_market_data(
            "forward period must be positive".into(),
        ));
    }
    Ok((df_a / df_b - 1.0) / tau)
}

// ---------------- vanilla swap ----------------

pub struct SwapSpec<'a> {
    pub curve: &'a BootstrappedCurve,
    pub payer: bool,
    pub years: u32,
    pub fixed_rate: Option<f64>, // None = price at fair (ATM) rate
}

pub struct SwapOutcome {
    pub npv: f64,
    pub fair_rate: f64,
    pub fixed_leg_npv: f64,
    pub floating_leg_npv: f64,
    pub fixed_coupons: Vec<Value>,
    pub float_dates: Vec<Value>,
}

pub fn price_vanilla_swap(spec: &SwapSpec) -> Result<SwapOutcome, ErrorEnvelope> {
    if !(1..=10).contains(&spec.years) {
        return Err(ErrorEnvelope::unsupported_feature(format!(
            "swap tenor {}y outside M2 slice (1-10y annual fixed vs 6M float)",
            spec.years
        )));
    }
    let cal = CalendarId::Target;
    let mf = BusinessDayConvention::ModifiedFollowing;
    let spot = spec.curve.spot;
    let maturity_unadj = year_maturity(spot, spec.years)?;

    let fixed_dates = schedule(cal, spot, maturity_unadj, Frequency::Annual, mf)?;
    let float_dates_raw = schedule(cal, spot, maturity_unadj, Frequency::Semiannual, mf)?;

    // fixed leg PV and annuity
    let mut annuity = 0.0f64;
    let mut fixed_coupons = Vec::new();
    for w in fixed_dates.windows(2) {
        let (d0, d1) = (w[0], w[1]);
        let tau = yf(DayCounterId::Thirty360European, d0, d1)?;
        let df = discount_at(spec.curve, d1)?;
        annuity += tau * df;
        fixed_coupons.push(json!({"date": d1.to_iso(), "yf": tau, "df": df}));
    }
    let df_maturity = discount_at(spec.curve, *fixed_dates.last().expect("non-empty"))?;

    // float leg (single curve, telescoping): PV = DF(spot) - DF(maturity)
    let float_pv = 1.0 - df_maturity;
    let mut float_dates = Vec::new();
    for w in float_dates_raw.windows(2) {
        let tau = yf(DayCounterId::Actual360, w[0], w[1])?;
        float_dates.push(json!({"date": w[1].to_iso(), "yf": tau}));
    }

    // fair rate: R * annuity = float_pv  ->  R = float_pv / annuity
    if annuity <= 0.0 {
        return Err(ErrorEnvelope::no_convergence("non-positive annuity".into()));
    }
    let fair_rate = float_pv / annuity;
    let rate = spec.fixed_rate.unwrap_or(fair_rate);
    if !rate.is_finite() {
        return Err(ErrorEnvelope::invalid_input(
            "fixed_rate must be a finite number".into(),
            "fixed_rate",
        ));
    }
    let fixed_pv = rate * annuity;
    // payer receives float, pays fixed
    let leg_sign = if spec.payer { 1.0 } else { -1.0 };
    let npv = leg_sign * (float_pv - fixed_pv);
    for c in &mut fixed_coupons {
        let yf30 = c["yf"].as_f64().unwrap();
        c["amount"] = json!(rate * yf30);
    }
    Ok(SwapOutcome {
        npv,
        fair_rate,
        fixed_leg_npv: -leg_sign * fixed_pv,
        floating_leg_npv: leg_sign * float_pv,
        fixed_coupons,
        float_dates,
    })
}

// ---------------- tool entries ----------------

fn parse_curve_spec(params: &Value) -> Result<CurveSpec, ErrorEnvelope> {
    let eval = params
        .get("eval_date")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ErrorEnvelope::invalid_input("missing field 'eval_date'".into(), "eval_date")
        })?;
    let eval_date = Date::parse_iso(eval)
        .map_err(|e| ErrorEnvelope::invalid_input(format!("eval_date: {e}"), "eval_date"))?;
    let deposit = params
        .get("deposit_6m")
        .and_then(Value::as_f64)
        .ok_or_else(|| {
            ErrorEnvelope::invalid_input("missing field 'deposit_6m'".into(), "deposit_6m")
        })?;
    let mut rates = [0.0f64; 10];
    let arr = params
        .get("swap_rates_1y_10y")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 10)
        .ok_or_else(|| {
            ErrorEnvelope::invalid_input(
                "swap_rates_1y_10y must be an array of exactly 10 rates".into(),
                "swap_rates_1y_10y",
            )
        })?;
    for (i, v) in arr.iter().enumerate() {
        rates[i] = v.as_f64().ok_or_else(|| {
            ErrorEnvelope::invalid_input(
                format!("swap rate #{i} not a number"),
                "swap_rates_1y_10y",
            )
        })?;
    }
    Ok(CurveSpec {
        eval_date,
        deposit_6m: deposit,
        swap_rates_1y_10y: rates,
    })
}

pub fn bootstrap_yield_curve_tool(params: &Value) -> Result<ToolOutput, ErrorEnvelope> {
    let spec = parse_curve_spec(params)?;
    let curve = bootstrap(&spec)?;
    if curve.residuals.iter().any(|r| r.abs() > 1e-9) {
        return Err(ErrorEnvelope::no_convergence(
            "helper residuals exceed 1e-9".into(),
        ));
    }
    let nodes: Vec<Value> = curve
        .nodes
        .iter()
        .map(|(d, df)| json!({"date": d, "discount": df}))
        .collect();
    let result = json!({
        "curve_id": format!("sha256:{}", crate::canonical::digest(&json!({"nodes": curve.nodes})).map_err(ErrorEnvelope::internal)?[..24].to_string()),
        "spot_date": curve.spot.to_iso(),
        "interpolation": "log_linear_discount",
        "nodes": nodes,
        "deposit_maturity": curve.deposit_maturity,
        "helper_residuals_max": curve.residuals.iter().fold(0.0f64, |m, r| m.max(r.abs())),
        "conventions": {
            "calendar": "TARGET",
            "settlement": "T+2 business days",
            "deposit_day_counter": "Actual360",
            "fixed_leg": "annual, 30E/360, ModifiedFollowing",
            "float_leg": "6M, Actual360, ModifiedFollowing, single-curve projection",
            "zero_rate_reported": "continuous, Act/365F (see fixtures)",
        },
        "units": {
            "discount": {"dimension": "ratio", "scale": "unity"},
            "zero_rate": {"dimension": "ratio", "scale": "unity", "convention": "continuous compounding, Act/365F"},
        },
    });
    Ok(ToolOutput {
        engine_id: CURVE_ENGINE_ID,
        provider_id: "builtin-rust",
        result,
        warnings: vec![],
    })
}

pub fn price_vanilla_swap_tool(params: &Value) -> Result<ToolOutput, ErrorEnvelope> {
    let spec = parse_curve_spec(params)?;
    let curve = bootstrap(&spec)?;
    let payer = match params
        .get("swap_type")
        .and_then(Value::as_str)
        .unwrap_or("payer")
    {
        "payer" => true,
        "receiver" => false,
        other => {
            return Err(ErrorEnvelope::invalid_input(
                format!("swap_type must be payer|receiver, got '{other}'"),
                "swap_type",
            ));
        }
    };
    let years = params.get("years").and_then(Value::as_u64).ok_or_else(|| {
        ErrorEnvelope::invalid_input("missing field 'years' (1-10)".into(), "years")
    })? as u32;
    let fixed_rate = params.get("fixed_rate").and_then(Value::as_f64); // absent = ATM
    let out = price_vanilla_swap(&SwapSpec {
        curve: &curve,
        payer,
        years,
        fixed_rate,
    })?;
    let result = json!({
        "npv": out.npv,
        "fair_rate": out.fair_rate,
        "fixed_leg_npv": out.fixed_leg_npv,
        "floating_leg_npv": out.floating_leg_npv,
        "fixed_coupons": out.fixed_coupons,
        "float_leg_dates": out.float_dates,
        "units": {
            "npv": {"dimension": "currency", "scale": "unity", "convention": "per unit notional"},
            "fair_rate": {"dimension": "ratio", "scale": "unity", "convention": "annual, 30E/360"},
        },
        "conventions": {
            "calendar": "TARGET", "settlement": "T+2",
            "fixed_leg": "annual 30E/360 ModifiedFollowing",
            "float_leg": "6M Act/360, telescoped single-curve projection",
            "pricing": "curve bootstrapped from the provided quotes (same engine as bootstrap_yield_curve)",
        },
    });
    Ok(ToolOutput {
        engine_id: SWAP_ENGINE_ID,
        provider_id: "builtin-rust",
        result,
        warnings: vec![],
    })
}
