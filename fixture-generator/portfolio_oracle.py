#!/usr/bin/env python3
"""Independent portfolio oracle (ADR-0003): generates golden fixtures for
risk metrics, stress tests and optimization reference weights.

STRICTLY pure standard library — no numpy/pandas/pypfopt — so it is an
implementation independent of the provider it arbitrates. Deterministic:
no timestamps, fixed iteration order, reads only committed inputs
(data/demo_prices.csv plus embedded hand-verifiable cases).

Reference conventions (must match the Rust tools' documented conventions):
  - daily simple returns r_t = P_t/P_{t-1} - 1
  - portfolio daily return with fixed weights (daily rebalanced): sum(w*r)
  - total return: prod(1+r) - 1
  - annualized return: (1+total)**(252/N) - 1
  - annualized vol: sample std (ddof=1) of daily returns * sqrt(252)
  - max drawdown on the cumulative value curve
  - sharpe: (ann_return - rf) / ann_vol
  - mu (annualized): mean(daily simple returns) * 252
  - S (annualized): sample cov (ddof=1) of daily returns * 252
"""

import csv
import json
import math
import os
import sys

OUT = sys.argv[1] if len(sys.argv) > 1 else "../fixtures"
DATA = os.path.join(os.path.dirname(__file__), "..", "data", "demo_prices.csv")


def jnum(x: float) -> str:
    return repr(float(x))


def load_prices():
    by_ticker = {}
    dates = set()
    with open(DATA, newline="", encoding="utf-8") as f:
        for row in csv.DictReader(f):
            by_ticker.setdefault(row["ticker"], []).append(float(row["close"]))
            dates.add(row["date"])
    order = sorted(by_ticker)
    return order, by_ticker


def daily_returns(series):
    return [series[i] / series[i - 1] - 1.0 for i in range(1, len(series))]


def portfolio_metrics(prices_by_ticker, weights):
    tickers = sorted(prices_by_ticker)
    series = [prices_by_ticker[t] for t in tickers]
    w = [weights.get(t, 0.0) for t in tickers]
    total_w = sum(w)
    w = [x / total_w for x in w]
    n = len(series[0])
    port = []
    for i in range(1, n):
        r = sum(w[j] * (series[j][i] / series[j][i - 1] - 1.0) for j in range(len(tickers)))
        port.append(r)
    total_ret = 1.0
    for r in port:
        total_ret *= 1.0 + r
    total_ret -= 1.0
    nobs = len(port)
    mean = sum(port) / nobs
    var = sum((r - mean) ** 2 for r in port) / (nobs - 1)
    daily_vol = math.sqrt(var)
    ann_ret = (1.0 + total_ret) ** (252.0 / nobs) - 1.0
    ann_vol = daily_vol * math.sqrt(252.0)
    value = 1.0
    peak = 1.0
    mdd = 0.0
    for r in port:
        value *= 1.0 + r
        peak = max(peak, value)
        mdd = max(mdd, 1.0 - value / peak)
    return {
        "total_return": total_ret,
        "annualized_return": ann_ret,
        "annualized_volatility": ann_vol,
        "max_drawdown": mdd,
        "sharpe_ratio": (ann_ret - 0.0) / ann_vol,
        "observations": nobs,
    }


def stress(prices_by_ticker, weights, shocks):
    tickers = sorted(prices_by_ticker)
    last = {t: prices_by_ticker[t][-1] for t in tickers}
    total_w = sum(weights.get(t, 0.0) for t in tickers)
    base = sum((weights.get(t, 0.0) / total_w) * last[t] for t in tickers)
    shocked = sum((weights.get(t, 0.0) / total_w) * last[t] * (1.0 + shocks.get(t, 0.0)) for t in tickers)
    contributions = {t: (weights.get(t, 0.0) / total_w) * last[t] * shocks.get(t, 0.0) / base for t in tickers}
    return {
        "baseline_value": base,
        "shocked_value": shocked,
        "pnl": shocked - base,
        "portfolio_return": (shocked - base) / base,
        "contributions": contributions,
    }


def mean(xs):
    return sum(xs) / len(xs)


def sample_cov(xs, ys):
    n = len(xs)
    mx, my = mean(xs), mean(ys)
    return sum((xs[i] - mx) * (ys[i] - my) for i in range(n)) / (n - 1)


def solve_linear(a, b):
    """Gaussian elimination with partial pivoting, pure python."""
    n = len(b)
    m = [row[:] + [b[i]] for i, row in enumerate(a)]
    for col in range(n):
        piv = max(range(col, n), key=lambda r: abs(m[r][col]))
        m[col], m[piv] = m[piv], m[col]
        for r in range(col + 1, n):
            factor = m[r][col] / m[col][col]
            for c in range(col, n + 1):
                m[r][c] -= factor * m[col][c]
    x = [0.0] * n
    for i in range(n - 1, -1, -1):
        x[i] = (m[i][n] - sum(m[i][j] * x[j] for j in range(i + 1, n))) / m[i][i]
    return x


def invert_apply(cov, vec):
    """Return cov^{-1} vec without forming the inverse."""
    return solve_linear([row[:] for row in cov], vec[:])


def min_var_weights(returns_by_ticker):
    """Unconstrained (long-only-interior must be verified) global min-variance."""
    tickers = sorted(returns_by_ticker)
    n = len(tickers)
    daily = [returns_by_ticker[t] for t in tickers]
    s_daily = [[sample_cov(daily[i], daily[j]) * 252.0 for j in range(n)] for i in range(n)]
    ones = [1.0] * n
    sinv_ones = invert_apply(s_daily, ones)
    denom = sum(sinv_ones)
    w = {t: sinv_ones[i] / denom for i, t in enumerate(tickers)}
    interior = all(v > 1e-6 for v in w.values())
    return w, s_daily, interior


def tangency_weights(returns_by_ticker, rf):
    tickers = sorted(returns_by_ticker)
    n = len(tickers)
    daily = [returns_by_ticker[t] for t in tickers]
    mu = [mean(daily[i]) * 252.0 for i in range(n)]
    s_daily = [[sample_cov(daily[i], daily[j]) * 252.0 for j in range(n)] for i in range(n)]
    excess = [mu[i] - rf for i in range(n)]
    raw = invert_apply(s_daily, excess)
    total = sum(raw)
    if total <= 0:
        return None, False
    w = {t: raw[i] / total for i, t in enumerate(tickers)}
    interior = all(v > 1e-6 for v in w.values())
    return w, interior


def annualized_perf(returns_by_ticker, weights):
    tickers = sorted(returns_by_ticker)
    n = len(tickers)
    daily = [returns_by_ticker[t] for t in tickers]
    mu = [mean(daily[i]) * 252.0 for i in range(n)]
    s = [[sample_cov(daily[i], daily[j]) * 252.0 for j in range(n)] for i in range(n)]
    ret = sum(weights[tickers[i]] * mu[i] for i in range(n))
    vol = math.sqrt(
        sum(
            s[i][j] * weights[tickers[i]] * weights[tickers[j]]
            for i in range(n)
            for j in range(n)
        )
    )
    return ret, vol



def mini_two_asset():
    """Seeded GBM pair engineered (verified) to give INTERIOR tangency AND
    interior min-variance solutions, so both objectives have exact analytic
    references. Deterministic: random.Random with fixed seeds."""
    import random

    def series(seed, mu, vol, n=60):
        rng = random.Random(seed)
        out = [100.0]
        for _ in range(n):
            z = rng.gauss(0, 1)
            out.append(out[-1] * math.exp((mu - 0.5 * vol * vol) / 252 + vol / math.sqrt(252) * z))
        return out

    return {"P": series(1, 0.10, 0.15), "Q": series(4, 0.15, 0.30)}


def main() -> int:
    tickers, prices = load_prices()
    returns = {t: daily_returns(prices[t]) for t in tickers}

    # ---------------- risk fixtures ----------------
    risk_cases = []
    wsets = [
        ("equal_weight", {t: 1.0 / len(tickers) for t in tickers}),
        ("tilt_c", {t: (0.6 if t == "SYNTH_C" else 0.2) for t in tickers}),
    ]
    for name, w in wsets:
        m = portfolio_metrics(prices, w)
        risk_cases.append({
            "id": f"risk_{name}",
            "input": {"weights": {k: round(v, 6) for k, v in w.items()}, "dataset": "demo_prices"},
            "expected": m,
            "tolerance": {"mode": "mixed", "exact_metrics": ["observations"],
                          "float_metrics": {k: {"atol": 1e-12, "rtol": 1e-10, "scale_type": "expected_magnitude"}
                                            for k in ["total_return", "annualized_return", "annualized_volatility",
                                                     "max_drawdown", "sharpe_ratio"]}},
        })
    # Hand-verifiable tiny case: prices A=[100,110,99], B=[50,50,60]
    tiny_prices = {"A": [100.0, 110.0, 99.0], "B": [50.0, 50.0, 60.0]}
    tiny_w = {"A": 0.5, "B": 0.5}
    # day1: A 10%, B 0% -> r=0.05 ; day2: A -10%, B +20% -> r=0.05
    # total = 1.05*1.05-1 = 0.1025 ; mdd = 0 ; vol = 0 (both days equal!)
    tiny = portfolio_metrics(tiny_prices, tiny_w)
    assert abs(tiny["total_return"] - 0.1025) < 1e-12 and tiny["max_drawdown"] == 0.0 and tiny["annualized_volatility"] < 1e-14, tiny
    risk_cases.append({
        "id": "risk_tiny_handcheck",
        "input": {"weights": tiny_w, "prices_inline": tiny_prices},
        "expected": tiny,
        "tolerance": {"mode": "mixed", "exact_metrics": ["observations"],
                      "float_metrics": {"total_return": {"atol": 1e-15, "rtol": 1e-12, "scale_type": "expected_magnitude"},
                                        "annualized_volatility": {"atol": 1e-13, "rtol": 1e-12, "scale_type": "expected_magnitude"}}},
    })

    # ---------------- stress fixtures ----------------
    stress_cases = []
    scenarios = [
        ("equity_shock", {"SYNTH_A": -0.20, "SYNTH_B": -0.25, "SYNTH_C": -0.10}),
        ("mixed_shock", {"SYNTH_A": -0.10, "SYNTH_B": 0.15, "SYNTH_C": 0.0}),
        ("single_crash", {"SYNTH_B": -0.50}),
    ]
    for name, shocks in scenarios:
        w = {t: 1.0 / len(tickers) for t in tickers}
        r = stress(prices, w, shocks)
        assert abs(sum(r["contributions"].values()) - r["portfolio_return"]) < 1e-12, "contribution invariant"
        stress_cases.append({
            "id": f"stress_{name}",
            "input": {"weights": w, "dataset": "demo_prices", "shocks": shocks},
            "expected": r,
            "tolerance": {"mode": "mixed", "exact_metrics": [],
                          "float_metrics": {k: {"atol": 1e-12, "rtol": 1e-10, "scale_type": "expected_magnitude"}
                                            for k in ["baseline_value", "shocked_value", "pnl", "portfolio_return"]}},
        })

    # ---------------- optimize fixtures ----------------
    opt_cases = []
    wmin, _, interior = min_var_weights(returns)
    if not interior:
        raise SystemExit("min-var weights not interior; regenerate demo data with another seed")
    ret, vol = annualized_perf(returns, wmin)
    opt_cases.append({
        "id": "opt_min_vol_analytic",
        "input": {"dataset": "demo_prices", "objective": "min_volatility", "max_weight": 1.0},
        "expected": {"weights": wmin, "expected_return": ret, "volatility": vol},
        "tolerance": {"mode": "mixed",
                      "exact_metrics": [],
                      "float_metrics": {
                          **{f"weight_{t}": {"atol": 5e-4, "rtol": 1e-6, "scale_type": "expected_magnitude"} for t in tickers},
                          "expected_return": {"atol": 1e-8, "rtol": 1e-6, "scale_type": "expected_magnitude"},
                          "volatility": {"atol": 1e-10, "rtol": 1e-6, "scale_type": "expected_magnitude"}}},
    })
    wmin_b, _, interior_b = (None, None, True)
    # bounded case: max_weight 0.5 — reference is invariant-only (solution hits bounds)
    wtan, tan_interior = tangency_weights(returns, 0.02)
    if wtan is not None and tan_interior:
        rtan, vtan = annualized_perf(returns, wtan)
        opt_cases.append({
            "id": "opt_max_sharpe_analytic",
            "input": {"dataset": "demo_prices", "objective": "max_sharpe", "risk_free_rate": 0.02, "max_weight": 1.0},
            "expected": {"weights": wtan, "expected_return": rtan, "volatility": vtan},
            "tolerance": {"mode": "mixed", "exact_metrics": [],
                          "float_metrics": {
                              **{f"weight_{t}": {"atol": 5e-4, "rtol": 1e-6, "scale_type": "expected_magnitude"} for t in tickers},
                              "expected_return": {"atol": 1e-8, "rtol": 1e-6, "scale_type": "expected_magnitude"},
                              "volatility": {"atol": 1e-10, "rtol": 1e-6, "scale_type": "expected_magnitude"}}},
        })
    else:
        opt_cases.append({
            "id": "opt_max_sharpe_invariants",
            "input": {"dataset": "demo_prices", "objective": "max_sharpe", "risk_free_rate": 0.02, "max_weight": 1.0},
            "expected": {"note": "tangency not interior; invariants only"},
            "tolerance": {"mode": "exact"},
        })
    mini = mini_two_asset()
    mini_rets = {t: daily_returns(mini[t]) for t in mini}
    wmin2, _, imin2 = min_var_weights(mini_rets)
    wtan2, itan2 = tangency_weights(mini_rets, 0.02)
    if not (imin2 and itan2):
        raise SystemExit("mini_two_asset no longer interior; re-engineer seeds")
    r2, v2 = annualized_perf(mini_rets, wmin2)
    rt2, vt2 = annualized_perf(mini_rets, wtan2)
    opt_cases.append({
        "id": "opt_min_vol_mini_exact",
        "input": {"prices_inline": mini, "objective": "min_volatility", "max_weight": 1.0},
        "expected": {"weights": wmin2, "expected_return": r2, "volatility": v2},
        "tolerance": {"mode": "mixed", "exact_metrics": [],
                      "float_metrics": {
                          **{f"weight_{t}": {"atol": 5e-4, "rtol": 1e-6, "scale_type": "expected_magnitude"} for t in sorted(mini)},
                          "expected_return": {"atol": 1e-8, "rtol": 1e-6, "scale_type": "expected_magnitude"},
                          "volatility": {"atol": 1e-10, "rtol": 1e-6, "scale_type": "expected_magnitude"}}},
    })
    opt_cases.append({
        "id": "opt_max_sharpe_mini_exact",
        "input": {"prices_inline": mini, "objective": "max_sharpe", "risk_free_rate": 0.02, "max_weight": 1.0},
        "expected": {"weights": wtan2, "expected_return": rt2, "volatility": vt2},
        "tolerance": {"mode": "mixed", "exact_metrics": [],
                      "float_metrics": {
                          **{f"weight_{t}": {"atol": 5e-4, "rtol": 1e-6, "scale_type": "expected_magnitude"} for t in sorted(mini)},
                          "expected_return": {"atol": 1e-8, "rtol": 1e-6, "scale_type": "expected_magnitude"},
                          "volatility": {"atol": 1e-10, "rtol": 1e-6, "scale_type": "expected_magnitude"}}},
    })
    opt_cases.append({
        "id": "opt_min_vol_bounded_invariants",
        "input": {"dataset": "demo_prices", "objective": "min_volatility", "max_weight": 0.5},
        "expected": {"note": "bounded case; invariants only"},
        "tolerance": {"mode": "exact"},
    })

    os.makedirs(f"{OUT}/risk_metrics", exist_ok=True)
    os.makedirs(f"{OUT}/stress_test", exist_ok=True)
    os.makedirs(f"{OUT}/optimize", exist_ok=True)

    def write_family(name, cases, note):
        with open(f"{OUT}/{name}/cases.json", "w") as f:
            f.write(json.dumps({"schema": 1, "family": name, "cases": cases}, indent=1, sort_keys=True))
        with open(f"{OUT}/{name}/provenance.json", "w") as f:
            f.write(json.dumps({
                "schema": 1, "family": name, "note": note,
                "generated_by": "portfolio_oracle.py (pure stdlib, independent implementation)",
                "input": "data/demo_prices.csv (seed 20260101) + embedded hand-check cases",
                "adr": "ADR-0003",
            }, indent=1, sort_keys=True))

    write_family("risk_metrics", risk_cases,
                 "Daily simple returns; daily-rebalanced fixed weights; 252-day annualization; ddof=1.")
    write_family("stress_test", stress_cases,
                 "Direct price shocks on last close, per-unit-notional baseline; contributions sum to portfolio_return.")
    write_family("optimize", opt_cases,
                 "Analytic references: global min-variance w = S^-1 1 / (1' S^-1 1); tangency w ~ S^-1 (mu - rf); "
                 "mu = mean daily simple returns x 252, S = sample cov (ddof=1) x 252 — matching the provider's "
                 "documented estimators. Bounded/non-interior cases use invariant-only references (ADR-0003 honesty rule).")
    print(f"wrote {len(risk_cases)} risk, {len(stress_cases)} stress, {len(opt_cases)} optimize cases")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
