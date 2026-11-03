#!/usr/bin/env python3
"""In-house independent rates oracle (M2; replaces the GPL FinancePy manual
cross-check per project policy: license friction -> reimplement, ideas not
code).

Methodological independence from the Rust core by construction:
  - bootstrap: per-pillar BISECTION on the par equation (the Rust core uses
    the closed-form linear solve; numeric root-finding is a different path
    that also validates the algebra);
  - swap float leg: full cashflow summation with implied forwards from
    interpolated discount factors (the Rust core uses the telescoping
    identity float PV = DF(start) - DF(end); summation must agree to
    machine precision, which doubles as an invariant);
  - day counts re-implemented from the convention definitions.

Inputs (quote scenarios + schedule dates) come from the QuantLib fixture
family `rates` — dates are the primary oracle's domain and are consumed,
not regenerated. Pure stdlib, deterministic. Output feeds a normal CI test.

Usage: python3 fixture-generator/rates_oracle.py [fixtures_root]
Writes <root>/rates_oracle/cases.json
"""

import json
import math
import os
import sys

ROOT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "fixtures")


# ---------------- dates & day counts (independent implementations) ----------------

def days_between(a: str, b: str) -> int:
    from datetime import date

    def p(s):
        y, m, d = s.split("-")
        return date(int(y), int(m), int(d))

    return (p(b) - p(a)).days


def yf_act360(a: str, b: str) -> float:
    return days_between(a, b) / 360.0


def yf_30e360(a: str, b: str) -> float:
    ya, ma, da = (int(x) for x in a.split("-"))
    yb, mb, db = (int(x) for x in b.split("-"))
    d1 = 30 if da == 31 else da
    d2 = 30 if db == 31 else db
    return (360 * (yb - ya) + 30 * (mb - ma) + (d2 - d1)) / 360.0


# ---------------- curve: nodes + log-linear discount interpolation ----------------

class Curve:
    def __init__(self):
        self.dates = []
        self.dfs = []

    def add(self, d: str, df: float):
        self.dates.append(d)
        self.dfs.append(df)

    def df(self, d: str) -> float:
        if d in self.dates:
            return self.dfs[self.dates.index(d)]
        if d <= self.dates[0]:
            return self.dfs[0]
        if d >= self.dates[-1]:
            return self.dfs[-1]
        for i in range(len(self.dates) - 1):
            a, b = self.dates[i], self.dates[i + 1]
            if a < d < b:
                t = days_between(a, d) / days_between(a, b)
                la, lb = math.log(self.dfs[i]), math.log(self.dfs[i + 1])
                return math.exp(la + (lb - la) * t)
        raise ValueError(d)


def bisect(f, lo, hi, tol=1e-15, iters=200):
    flo = f(lo)
    for _ in range(iters):
        mid = 0.5 * (lo + hi)
        fm = f(mid)
        if abs(fm) < tol or (hi - lo) < 1e-16:
            return mid
        if (flo < 0) == (fm < 0):
            lo, flo = mid, fm
        else:
            hi = mid
    return 0.5 * (lo + hi)


def bootstrap_numeric(eval_case: dict, swap_cases: list) -> dict:
    """Per-pillar bisection bootstrap from the same quotes, using schedule
    dates dumped by the QuantLib fixture as the coupon calendar."""
    spot = eval_case["expected"]["spot_date"]
    dep_maturity = None
    curve = Curve()
    curve.add(spot, 1.0)

    dep_rate = eval_case["input"]["deposit_6m"]
    # deposit pillar date: earliest fixed-coupon-related date after spot
    # (the fixture's pillar set gives it directly)
    pillars = list(eval_case["expected"]["pillar_dates"].items())
    dep_pillar = pillars[1][0]
    tau_dep = yf_act360(spot, dep_pillar)
    dep_df = bisect(lambda x: x * (1.0 + dep_rate * tau_dep) - 1.0, 0.5, 1.0)
    curve.add(dep_pillar, dep_df)

    # annual coupon dates NEST across tenors (Forward rule on annual periods,
    # proven by the QuantLib fixtures): the 10y schedule contains every
    # annual pillar 1y..10y.
    ten = [sc for sc in swap_cases if sc["input"]["years"] == 10]
    annual_dates = [c["date"] for c in ten[0]["expected"]["fixed_leg"]]
    assert len(annual_dates) == 10
    rates = eval_case["input"]["swap_rates_1y_10y"]

    prev_date = spot
    for n in range(1, 11):
        pillar = annual_dates[n - 1]
        rate = rates[n - 1]
        # known coupons: annual dates 1..n-1 (already on the curve) with
        # 30E/360 accruals between consecutive adjusted dates
        acc = 0.0
        anchor = spot
        for d in annual_dates[: n - 1]:
            acc += yf_30e360(anchor, d) * curve.df(d)
            anchor = d
        tau_n = yf_30e360(anchor, pillar)
        # par equation, monotone increasing in x: rate*(acc + tau_n*x) + x - 1 = 0
        prev_df = curve.dfs[-1]
        x = bisect(lambda x: rate * (acc + tau_n * x) + x - 1.0, 0.5 * prev_df, min(1.0, prev_df + 1e-9))
        curve.add(pillar, x)
        prev_date = pillar

    return curve


def price_swap_cashflows(curve: Curve, case: dict, scenario_rates: dict) -> dict:
    spot = curve.dates[0]
    rate = case["input"]["fixed_rate"]
    fixed_dates = [c["date"] for c in case["expected"]["fixed_leg"]]
    float_dates = [c["date"] for c in case["expected"]["float_leg_dates"]]

    annuity = 0.0
    prev = spot
    for d in fixed_dates:
        annuity += yf_30e360(prev, d) * curve.df(d)
        prev = d
    df_mat = curve.df(fixed_dates[-1])

    # float leg: FULL cashflow summation (no telescoping identity)
    float_pv = 0.0
    prev = spot
    for d in float_dates:
        tau = yf_act360(prev, d)
        df_prev = curve.df(prev)
        df_now = curve.df(d)
        fwd = (df_prev / df_now - 1.0) / tau
        float_pv += df_now * fwd * tau
        prev = d

    fair = float_pv / annuity
    fixed_pv = rate * annuity
    sign = 1.0 if case["input"]["swap_type"] == "payer" else -1.0
    return {
        "npv": sign * (float_pv - fixed_pv),
        "fair_rate": fair,
        "fixed_leg_npv": -sign * fixed_pv,
        "floating_leg_npv": sign * float_pv,
        # independence invariants, reported not asserted:
        "telescope_gap": abs(float_pv - (1.0 - df_mat)),
    }


def main() -> int:
    cases = json.load(open(os.path.join(ROOT, "rates", "cases.json")))["cases"]
    curves = {c["input"].get("scenario") or c["id"].replace("curve_", ""): c
              for c in cases if c["kind"] == "curve"}
    swaps = [c for c in cases if c["kind"] == "swap"]

    out_cases = []
    for scenario, eval_case in sorted(curves.items()):
        sc_swaps = [s for s in swaps if s["input"]["scenario"] == scenario]
        curve = bootstrap_numeric(eval_case, sc_swaps)
        # pillar DFs via numeric bootstrap
        ql_pillars = {d: v for d, v in eval_case["expected"]["pillar_dates"].items()}
        df_out = {}
        for d, v in zip(curve.dates, curve.dfs):
            if d == curve.dates[0]:
                continue
            df_out[d] = v
        swap_out = {}
        for s in sc_swaps:
            p = price_swap_cashflows(curve, s, eval_case["input"])
            swap_out[s["id"]] = {"years": s["input"]["years"],
                                 **{k: p[k] for k in ("npv", "fair_rate", "fixed_leg_npv", "floating_leg_npv", "telescope_gap")}}
        out_cases.append({
            "id": f"oracle_{scenario}",
            "input": {"scenario": scenario,
                      "eval_date": eval_case["input"]["eval_date"],
                      "deposit_6m": eval_case["input"]["deposit_6m"],
                      "swap_rates_1y_10y": eval_case["input"]["swap_rates_1y_10y"]},
            "expected": {"df_pillars": df_out, "swaps": swap_out,
                         "ql_pillars_for_reference": ql_pillars},
            "tolerance": {"mode": "mixed", "exact_metrics": [],
                          "float_metrics": {"df": {"atol": 1e-11, "rtol": 1e-9, "scale_type": "expected_magnitude"},
                                            "value": {"atol": 1e-10, "rtol": 1e-9, "scale_type": "expected_magnitude"}}},
        })

    os.makedirs(os.path.join(ROOT, "rates_oracle"), exist_ok=True)
    with open(os.path.join(ROOT, "rates_oracle", "cases.json"), "w") as f:
        json.dump({"schema": 1, "family": "rates_oracle", "cases": out_cases}, f, indent=1, sort_keys=True)
    with open(os.path.join(ROOT, "rates_oracle", "provenance.json"), "w") as f:
        json.dump({
            "schema": 1, "family": "rates_oracle",
            "note": ("In-house independent second oracle (Apache-2.0; replaces the GPL FinancePy "
                     "manual cross-check per project policy). Method independence: bisection "
                     "bootstrap (vs closed-form linear solve in Rust), full float-cashflow "
                     "summation (vs telescoping), independently coded day counts. Schedule dates "
                     "are consumed from the QuantLib `rates` fixtures (primary oracle's domain)."),
            "generated_by": "rates_oracle.py (pure stdlib)", "adr": "ADR-0003",
        }, f, indent=1, sort_keys=True)
    gaps = max(max(v["telescope_gap"] for v in c["expected"]["swaps"].values()) for c in out_cases)
    print(f"rates_oracle: {len(out_cases)} scenarios; max telescope gap {gaps:.2e}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
