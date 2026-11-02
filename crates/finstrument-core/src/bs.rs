//! Black-Scholes analytic engine (`bs_analytic.v1`). Conventions are fixed
//! and echoed in every result (ADR-0006): continuously compounded rates,
//! Actual365Fixed day counter for T unless overridden, Vega per 1.00
//! volatility, Theta per year. Numerically pinned by the black_scholes
//! QuantLib fixtures; put-call parity and boundary behavior are tested as
//! invariants independent of the fixtures.

use crate::contract::{ErrorEnvelope, ToolOutput};
use crate::date::Date;
use crate::daycount::{self, DayCounterId};
use serde_json::{Value, json};

pub const ENGINE_ID: &str = "bs_analytic.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionType {
    Call,
    Put,
}

pub struct BsRequest {
    pub option_type: OptionType,
    pub spot: f64,
    pub strike: f64,
    pub risk_free_rate: f64, // continuously compounded
    pub dividend_yield: f64, // continuously compounded
    pub volatility: f64,
    pub valuation_date: Date,
    pub maturity_date: Date,
    pub day_counter: DayCounterId,
}

pub struct BsOutput {
    pub npv: f64,
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
    pub t: f64,
}

fn std_normal_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

fn std_normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + libm::erf(x / std::f64::consts::SQRT_2))
}

pub fn price(req: &BsRequest) -> Result<BsOutput, ErrorEnvelope> {
    if req.spot <= 0.0 {
        return Err(ErrorEnvelope::invalid_input(
            "spot must be > 0".into(),
            "spot",
        ));
    }
    if req.strike <= 0.0 {
        return Err(ErrorEnvelope::invalid_input(
            "strike must be > 0".into(),
            "strike",
        ));
    }
    if req.volatility < 0.0 {
        return Err(ErrorEnvelope::invalid_input(
            "volatility must be >= 0".into(),
            "volatility",
        ));
    }
    if req.maturity_date <= req.valuation_date {
        return Err(ErrorEnvelope::invalid_input(
            "maturity_date must be after valuation_date".into(),
            "maturity_date",
        ));
    }
    let dc = daycount::DayCountRequest {
        counter: req.day_counter,
        start: req.valuation_date,
        end: req.maturity_date,
        ref_period: None,
    };
    let t = daycount::year_fraction(&dc)?;
    let (npv, delta, gamma, vega, theta) = bs_values(
        req.option_type,
        req.spot,
        req.strike,
        req.risk_free_rate,
        req.dividend_yield,
        req.volatility,
        t,
    );
    for v in [npv, delta, gamma, vega, theta] {
        if !v.is_finite() {
            return Err(ErrorEnvelope::numerical_failure(format!(
                "engine produced a non-finite value (T={t}, vol={})",
                req.volatility
            )));
        }
    }
    Ok(BsOutput {
        npv,
        delta,
        gamma,
        vega,
        theta,
        t,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn bs_values(
    option_type: OptionType,
    s: f64,
    k: f64,
    r: f64,
    q: f64,
    vol: f64,
    t: f64,
) -> (f64, f64, f64, f64, f64) {
    let sq_t = t.sqrt();
    let vol_sq_t = vol * sq_t;
    // Zero-volatility limit: deterministic discounted intrinsic.
    if vol_sq_t <= 0.0 || vol == 0.0 {
        let (carry, disc) = ((-q * t).exp(), (-r * t).exp());
        let fwd = s * carry / disc;
        let intrinsic = match option_type {
            OptionType::Call => (fwd - k).max(0.0),
            OptionType::Put => (k - fwd).max(0.0),
        };
        let npv = intrinsic * disc;
        let delta = match option_type {
            OptionType::Call if fwd > k => carry,
            OptionType::Put if k > fwd => -carry,
            _ => 0.0,
        };
        return (npv, delta, 0.0, 0.0, 0.0);
    }
    let d1 = ((s / k).ln() + (r - q + 0.5 * vol * vol) * t) / vol_sq_t;
    let d2 = d1 - vol_sq_t;
    let (cdf_d1, cdf_d2, pdf_d1) = (std_normal_cdf(d1), std_normal_cdf(d2), std_normal_pdf(d1));
    let (carry, disc) = ((-q * t).exp(), (-r * t).exp());
    let (npv, delta, common_term);
    match option_type {
        OptionType::Call => {
            npv = s * carry * cdf_d1 - k * disc * cdf_d2;
            delta = carry * cdf_d1;
        }
        OptionType::Put => {
            npv = k * disc * (1.0 - cdf_d2) - s * carry * (1.0 - cdf_d1);
            delta = -carry * (1.0 - cdf_d1);
        }
    }
    common_term = -s * carry * pdf_d1 * vol / (2.0 * sq_t);
    let theta = match option_type {
        OptionType::Call => common_term + q * s * carry * cdf_d1 - r * k * disc * cdf_d2,
        OptionType::Put => {
            common_term - q * s * carry * (1.0 - cdf_d1) + r * k * disc * (1.0 - cdf_d2)
        }
    };
    let gamma = carry * pdf_d1 / (s * vol_sq_t);
    let vega = s * carry * pdf_d1 * sq_t;
    (npv, delta, gamma, vega, theta)
}

/// Tool entry: full validation + result with per-metric unit semantics and
/// echoed conventions (gate 2 of ADR-0006).
pub fn price_european_option_tool(params: &Value) -> Result<ToolOutput, ErrorEnvelope> {
    let get_f64 = |key: &str| -> Result<f64, ErrorEnvelope> {
        params.get(key).and_then(Value::as_f64).ok_or_else(|| {
            ErrorEnvelope::invalid_input(format!("missing or non-numeric field '{key}'"), key)
        })
    };
    let get_date = |key: &str| -> Result<Date, ErrorEnvelope> {
        let s = params
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| ErrorEnvelope::invalid_input(format!("missing field '{key}'"), key))?;
        Date::parse_iso(s).map_err(|e| ErrorEnvelope::invalid_input(format!("{key}: {e}"), key))
    };
    let style = params
        .get("exercise_style")
        .and_then(Value::as_str)
        .unwrap_or("european");
    if style != "european" {
        return Err(ErrorEnvelope::unsupported_feature(format!(
            "exercise_style '{style}' is not supported in this version (european only); no value was computed"
        )));
    }
    let option_type = match params.get("option_type").and_then(Value::as_str) {
        Some("call") => OptionType::Call,
        Some("put") => OptionType::Put,
        Some(other) => {
            return Err(ErrorEnvelope::invalid_input(
                format!("option_type must be call|put, got '{other}'"),
                "option_type",
            ));
        }
        None => {
            return Err(ErrorEnvelope::invalid_input(
                "missing field 'option_type'".into(),
                "option_type",
            ));
        }
    };
    let day_counter = daycount::parse_counter(
        params
            .get("day_counter")
            .and_then(Value::as_str)
            .unwrap_or("Actual365Fixed"),
    )?;
    let req = BsRequest {
        option_type,
        spot: get_f64("spot")?,
        strike: get_f64("strike")?,
        risk_free_rate: get_f64("risk_free_rate")?,
        dividend_yield: params
            .get("dividend_yield")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        volatility: get_f64("volatility")?,
        valuation_date: get_date("valuation_date")?,
        maturity_date: get_date("maturity_date")?,
        day_counter,
    };
    let out = price(&req)?;
    let result = json!({
        "npv": out.npv,
        "greeks": {
            "delta": out.delta,
            "gamma": out.gamma,
            "vega": out.vega,
            "theta": out.theta,
        },
        "t": out.t,
        "units": {
            "npv": { "dimension": "currency", "scale": "units_of_spot" },
            "delta": { "dimension": "dimensionless", "scale": "per_unit_spot" },
            "gamma": { "dimension": "dimensionless", "scale": "per_unit_spot_squared" },
            "vega": { "dimension": "dimensionless", "scale": "per_unit_vol",
                       "convention": "per 1.00 volatility (not per 1%)" },
            "theta": { "dimension": "currency", "scale": "unity", "convention": "per year" },
        },
        "conventions": {
            "model": "black_scholes_merton",
            "rates": "continuously_compounded",
            "dividend": "continuously_compounded",
            "day_counter": format!("{:?}", req.day_counter),
            "valuation_date": req.valuation_date.to_iso(),
            "maturity_date": req.maturity_date.to_iso(),
        },
        "option": {
            "option_type": match option_type { OptionType::Call => "call", OptionType::Put => "put" },
            "exercise_style": "european",
            "spot": req.spot,
            "strike": req.strike,
            "risk_free_rate": req.risk_free_rate,
            "dividend_yield": req.dividend_yield,
            "volatility": req.volatility,
        },
    });
    Ok(ToolOutput {
        engine_id: ENGINE_ID,
        provider_id: "builtin-rust",
        result,
        warnings: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::Date;

    fn req(call: bool, s: f64, k: f64, r: f64, q: f64, vol: f64, _t: f64) -> BsRequest {
        BsRequest {
            option_type: if call {
                OptionType::Call
            } else {
                OptionType::Put
            },
            spot: s,
            strike: k,
            risk_free_rate: r,
            dividend_yield: q,
            volatility: vol,
            valuation_date: Date::parse_iso("2023-01-01").unwrap(),
            maturity_date: Date::parse_iso("2024-01-01").unwrap(),
            day_counter: DayCounterId::Actual365Fixed,
        }
    }
    // t is recomputed inside price(); these helpers price via bs_values.
    fn v(
        call: bool,
        s: f64,
        k: f64,
        r: f64,
        q: f64,
        vol: f64,
        t: f64,
    ) -> (f64, f64, f64, f64, f64) {
        bs_values(
            if call {
                OptionType::Call
            } else {
                OptionType::Put
            },
            s,
            k,
            r,
            q,
            vol,
            t,
        )
    }

    #[test]
    fn put_call_parity_holds_on_grid() {
        let mut checked = 0;
        for s in [80.0, 95.0, 100.0, 105.0, 120.0] {
            for t in [0.1, 1.0, 3.0] {
                for (r, q) in [(0.0, 0.0), (0.03, 0.01)] {
                    for vol in [0.05, 0.3, 0.8] {
                        let (c, _, _, _, _) = v(true, s, 100.0, r, q, vol, t);
                        let (p, _, _, _, _) = v(false, s, 100.0, r, q, vol, t);
                        let lhs = c - p;
                        let rhs = s * (-q * t).exp() - 100.0 * (-r * t).exp();
                        assert!(
                            (lhs - rhs).abs() <= 1e-9 * rhs.abs().max(1.0),
                            "parity broken: S={s} T={t} r={r} q={q} vol={vol}: {lhs} vs {rhs}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 90);
    }

    #[test]
    fn bounds_and_monotonicity() {
        for vol in [0.1, 0.4] {
            let (c, delta, gamma, _, _) = v(true, 100.0, 100.0, 0.02, 0.0, vol, 1.0);
            assert!(c < 100.0 && c > 0.0, "call price bounds");
            assert!((0.0..=1.0).contains(&delta), "call delta in (0,1)");
            assert!(gamma >= 0.0);
        }
        // upper bound: call <= S e^{-qT}
        let (c, _, _, _, _) = v(true, 100.0, 50.0, 0.05, 0.02, 0.3, 2.0);
        assert!(c <= 100.0 * (-0.04f64).exp() + 1e-9);
    }

    #[test]
    fn zero_vol_limit_is_discounted_intrinsic() {
        let (npv, delta, gamma, vega, _) = v(true, 120.0, 100.0, 0.05, 0.02, 0.0, 1.0);
        // Discounted intrinsic: S e^{-qT} - K e^{-rT} (forward 123.66 > K).
        let expected = 120.0 * (-0.02f64).exp() - 100.0 * (-0.05f64).exp();
        assert!((npv - expected).abs() < 1e-12);
        assert_eq!(delta, (-0.02f64).exp());
        assert_eq!(gamma, 0.0);
        assert_eq!(vega, 0.0);
        let (npv, _, _, _, _) = v(false, 120.0, 100.0, 0.05, 0.02, 0.0, 1.0);
        assert_eq!(npv, 0.0, "OTM put at zero vol is worthless");
        let _ = req(true, 100.0, 100.0, 0.0, 0.0, 0.0, 1.0); // silence unused warning path
    }
}
