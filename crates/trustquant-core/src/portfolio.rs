//! Portfolio risk metrics and direct price-shock stress tests (Rust built-in
//! provider). Conventions are explicit and echoed in every result (ADR-0003):
//! daily simple returns, daily-rebalanced fixed weights, 252-day year,
//! sample std (ddof=1). Pinned by the pure-Python oracle fixtures.

use crate::contract::{ErrorEnvelope, ToolOutput};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const ENGINE_ID: &str = "portfolio_risk.v1";

pub struct PricePanel {
    pub tickers: Vec<String>,
    pub series: BTreeMap<String, Vec<f64>>,
}

impl PricePanel {
    pub fn dates_len(&self) -> usize {
        self.series.values().next().map(|v| v.len()).unwrap_or(0)
    }
}

/// Normalized weights (sum to 1) over the panel tickers.
pub fn normalize_weights(
    panel: &PricePanel,
    raw: &BTreeMap<String, f64>,
) -> Result<Vec<f64>, ErrorEnvelope> {
    if raw.is_empty() {
        return Err(ErrorEnvelope::invalid_input(
            "weights must not be empty".into(),
            "weights",
        ));
    }
    let mut out = Vec::with_capacity(panel.tickers.len());
    for t in &panel.tickers {
        let Some(w) = raw.get(t) else {
            return Err(ErrorEnvelope::invalid_input(
                format!("weight missing for ticker '{t}' present in the dataset"),
                "weights",
            ));
        };
        if !w.is_finite() || *w < 0.0 {
            return Err(ErrorEnvelope::invalid_input(
                format!("weight for '{t}' must be a finite number >= 0"),
                "weights",
            ));
        }
        out.push(*w);
    }
    let total: f64 = out.iter().sum();
    if total <= 0.0 || !total.is_finite() {
        return Err(ErrorEnvelope::invalid_input(
            "weights must sum to a positive number".into(),
            "weights",
        ));
    }
    for w in &mut out {
        *w /= total;
    }
    Ok(out)
}

fn portfolio_daily_returns(panel: &PricePanel, weights: &[f64]) -> Vec<f64> {
    let n = panel.dates_len();
    let mut out = Vec::with_capacity(n.saturating_sub(1));
    for i in 1..n {
        let mut r = 0.0;
        for (j, t) in panel.tickers.iter().enumerate() {
            let s = &panel.series[t];
            r += weights[j] * (s[i] / s[i - 1] - 1.0);
        }
        out.push(r);
    }
    out
}

pub struct RiskMetrics {
    pub total_return: f64,
    pub annualized_return: f64,
    pub annualized_volatility: f64,
    pub max_drawdown: f64,
    pub sharpe_ratio: f64,
    pub observations: usize,
}

pub fn risk_metrics(
    panel: &PricePanel,
    weights: &[f64],
    risk_free: f64,
) -> Result<RiskMetrics, ErrorEnvelope> {
    let rets = portfolio_daily_returns(panel, weights);
    let n = rets.len();
    if n < 2 {
        return Err(ErrorEnvelope::invalid_market_data(
            "need at least 3 price observations per ticker for risk metrics".into(),
        ));
    }
    let mean = rets.iter().sum::<f64>() / n as f64;
    let var = rets.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / (n - 1) as f64;
    let daily_vol = var.sqrt();
    let mut value = 1.0;
    let mut peak = 1.0;
    let mut mdd = 0.0f64;
    for r in &rets {
        value *= 1.0 + r;
        if value > peak {
            peak = value;
        }
        let dd = 1.0 - value / peak;
        if dd > mdd {
            mdd = dd;
        }
    }
    let total_return = value - 1.0;
    let ann_return = (1.0 + total_return).powf(252.0 / n as f64) - 1.0;
    let ann_vol = daily_vol * 252.0f64.sqrt();
    let sharpe = if ann_vol > 0.0 {
        (ann_return - risk_free) / ann_vol
    } else {
        f64::INFINITY
    };
    for v in [total_return, ann_return, ann_vol, mdd] {
        if !v.is_finite() {
            return Err(ErrorEnvelope::numerical_failure(format!(
                "non-finite metric (ann_vol={ann_vol})"
            )));
        }
    }
    Ok(RiskMetrics {
        total_return,
        annualized_return: ann_return,
        annualized_volatility: ann_vol,
        max_drawdown: mdd,
        sharpe_ratio: sharpe,
        observations: n,
    })
}

pub struct StressOutcome {
    pub baseline_value: f64,
    pub shocked_value: f64,
    pub pnl: f64,
    pub portfolio_return: f64,
    pub contributions: BTreeMap<String, f64>,
}

pub fn stress(
    panel: &PricePanel,
    weights: &[f64],
    shocks: &BTreeMap<String, f64>,
) -> Result<StressOutcome, ErrorEnvelope> {
    for (t, s) in shocks {
        if !s.is_finite() || *s <= -1.0 {
            return Err(ErrorEnvelope::invalid_input(
                format!(
                    "shock for '{t}' must be a finite number > -1.0 (a -100% move leaves no value)"
                ),
                "shocks",
            ));
        }
        if !panel.tickers.contains(t) {
            return Err(ErrorEnvelope::invalid_input(
                format!("shock references ticker '{t}' not present in the dataset"),
                "shocks",
            ));
        }
    }
    let last: Vec<f64> = panel
        .tickers
        .iter()
        .map(|t| {
            let s = &panel.series[t];
            s[s.len() - 1]
        })
        .collect();
    let baseline: f64 = weights.iter().zip(&last).map(|(w, p)| w * p).sum();
    if baseline <= 0.0 {
        return Err(ErrorEnvelope::numerical_failure(
            "baseline portfolio value is not positive".into(),
        ));
    }
    let mut contributions = BTreeMap::new();
    let mut shocked = 0.0f64;
    for (j, t) in panel.tickers.iter().enumerate() {
        let shock = shocks.get(t).copied().unwrap_or(0.0);
        let leg = weights[j] * last[j] * (1.0 + shock);
        shocked += leg;
        contributions.insert(t.clone(), weights[j] * last[j] * shock / baseline);
    }
    let pnl = shocked - baseline;
    Ok(StressOutcome {
        baseline_value: baseline,
        shocked_value: shocked,
        pnl,
        portfolio_return: pnl / baseline,
        contributions,
    })
}

// ---------------- tool entries ----------------

pub fn analyze_portfolio_risk_tool(
    panel: &PricePanel,
    params: &Value,
) -> Result<ToolOutput, ErrorEnvelope> {
    let weights_raw: BTreeMap<String, f64> = params
        .get("weights")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ErrorEnvelope::invalid_input(
                "missing field 'weights' (ticker -> number)".into(),
                "weights",
            )
        })?
        .iter()
        .map(|(k, v)| {
            let w = v.as_f64().ok_or_else(|| {
                ErrorEnvelope::invalid_input(format!("weight for '{k}' is not a number"), "weights")
            })?;
            Ok((k.clone(), w))
        })
        .collect::<Result<_, ErrorEnvelope>>()?;
    let rf = params
        .get("risk_free_rate")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let weights = normalize_weights(panel, &weights_raw)?;
    let m = risk_metrics(panel, &weights, rf)?;
    let result = json!({
        "total_return": m.total_return,
        "annualized_return": m.annualized_return,
        "annualized_volatility": m.annualized_volatility,
        "max_drawdown": m.max_drawdown,
        "sharpe_ratio": m.sharpe_ratio,
        "observations": m.observations,
        "weights_used": panel.tickers.iter().zip(&weights).map(|(t, w)| json!({"ticker": t, "weight": w})).collect::<Vec<_>>(),
        "units": {
            "total_return": {"dimension": "ratio", "scale": "unity", "convention": "simple, geometrically compounded"},
            "annualized_return": {"dimension": "ratio", "scale": "unity", "convention": "(1+total)^(252/N)-1"},
            "annualized_volatility": {"dimension": "ratio", "scale": "unity", "convention": "daily simple returns, ddof=1, x sqrt(252)"},
            "max_drawdown": {"dimension": "ratio", "scale": "unity"},
            "sharpe_ratio": {"dimension": "ratio", "scale": "unity", "convention": "(ann_return - risk_free_rate)/ann_vol"}
        },
        "conventions": {
            "returns": "daily simple",
            "rebalancing": "daily, fixed weights",
            "annualization_days": 252,
        },
    });
    Ok(ToolOutput {
        engine_id: ENGINE_ID,
        provider_id: "builtin-rust",
        result,
        warnings: vec![],
    })
}

pub fn stress_test_tool(panel: &PricePanel, params: &Value) -> Result<ToolOutput, ErrorEnvelope> {
    let weights_raw: BTreeMap<String, f64> = params
        .get("weights")
        .and_then(Value::as_object)
        .ok_or_else(|| ErrorEnvelope::invalid_input("missing field 'weights'".into(), "weights"))?
        .iter()
        .map(|(k, v)| {
            let w = v.as_f64().ok_or_else(|| {
                ErrorEnvelope::invalid_input(format!("weight for '{k}' is not a number"), "weights")
            })?;
            Ok((k.clone(), w))
        })
        .collect::<Result<_, ErrorEnvelope>>()?;
    let weights = normalize_weights(panel, &weights_raw)?;
    let scenarios = params
        .get("scenarios")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| {
            ErrorEnvelope::invalid_input("missing non-empty field 'scenarios'".into(), "scenarios")
        })?;
    let mut out_scenarios = Vec::with_capacity(scenarios.len());
    let mut warnings = Vec::new();
    for (idx, sc) in scenarios.iter().enumerate() {
        let name = sc
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("scenario_{idx}"));
        let shocks: BTreeMap<String, f64> = sc
            .get("shocks")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ErrorEnvelope::invalid_input(
                    format!("scenario '{name}' missing shocks"),
                    "scenarios",
                )
            })?
            .iter()
            .map(|(k, v)| {
                let s = v.as_f64().ok_or_else(|| {
                    ErrorEnvelope::invalid_input(
                        format!("shock for '{k}' is not a number"),
                        "shocks",
                    )
                })?;
                Ok((k.clone(), s))
            })
            .collect::<Result<_, ErrorEnvelope>>()?;
        if shocks.is_empty() {
            return Err(ErrorEnvelope::invalid_input(
                format!("scenario '{name}' has empty shocks"),
                "shocks",
            ));
        }
        let o = stress(panel, &weights, &shocks)?;
        // Contribution invariant: sum == portfolio_return (guard against drift)
        let contrib_sum: f64 = o.contributions.values().sum();
        if (contrib_sum - o.portfolio_return).abs() > 1e-9 {
            warnings.push(format!(
                "scenario '{name}': contributions sum {contrib_sum} deviates from return {}",
                o.portfolio_return
            ));
        }
        out_scenarios.push(json!({
            "name": name,
            "shocks": shocks,
            "baseline_value": o.baseline_value,
            "shocked_value": o.shocked_value,
            "pnl": o.pnl,
            "portfolio_return": o.portfolio_return,
            "contributions": o.contributions,
        }));
    }
    let result = json!({
        "scenarios": out_scenarios,
        "weights_used": panel.tickers.iter().zip(&weights).map(|(t, w)| json!({"ticker": t, "weight": w})).collect::<Vec<_>>(),
        "units": {
            "baseline_value": {"dimension": "currency", "scale": "unity", "convention": "per unit portfolio notional"},
            "portfolio_return": {"dimension": "ratio", "scale": "unity", "convention": "direct price shock on last close"},
            "contributions": {"dimension": "ratio", "scale": "unity", "convention": "sums to portfolio_return per scenario"}
        },
        "conventions": {"shock_model": "direct instantaneous price shock", "baseline": "last close in dataset"},
    });
    Ok(ToolOutput {
        engine_id: ENGINE_ID,
        provider_id: "builtin-rust",
        result,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> PricePanel {
        let mut series = BTreeMap::new();
        series.insert("A".to_string(), vec![100.0, 110.0, 99.0]);
        series.insert("B".to_string(), vec![50.0, 50.0, 60.0]);
        PricePanel {
            tickers: vec!["A".into(), "B".into()],
            series,
        }
    }

    #[test]
    fn tiny_handcheck_metrics() {
        let p = panel();
        let w =
            normalize_weights(&p, &BTreeMap::from([("A".into(), 0.5), ("B".into(), 0.5)])).unwrap();
        let m = risk_metrics(&p, &w, 0.0).unwrap();
        assert!((m.total_return - 0.1025).abs() < 1e-12);
        assert_eq!(m.max_drawdown, 0.0);
        assert!(m.annualized_volatility < 1e-14);
        assert_eq!(m.observations, 2);
    }

    #[test]
    fn stress_contribution_invariant() {
        let p = panel();
        let w =
            normalize_weights(&p, &BTreeMap::from([("A".into(), 0.5), ("B".into(), 0.5)])).unwrap();
        let o = stress(
            &p,
            &w,
            &BTreeMap::from([("A".into(), -0.10), ("B".into(), 0.20)]),
        )
        .unwrap();
        let cs: f64 = o.contributions.values().sum();
        assert!((cs - o.portfolio_return).abs() < 1e-12);
        // A 50/50 into last closes A=99, B=60 -> baseline 79.5
        assert!((o.baseline_value - 79.5).abs() < 1e-12);
        assert!((o.shocked_value - (89.1 * 0.5 + 72.0 * 0.5)).abs() < 1e-12);
    }

    #[test]
    fn rejects_negative_weights_and_unknown_ticker() {
        let p = panel();
        assert!(normalize_weights(&p, &BTreeMap::from([("A".into(), -0.1)])).is_err());
        assert!(normalize_weights(&p, &BTreeMap::from([("Z".into(), 1.0)])).is_err());
    }
}
