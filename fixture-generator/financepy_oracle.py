#!/usr/bin/env python3
"""FinancePy independent cross-check (ADR-0003 §5; MANUAL, not CI).

FinancePy is GPL-3.0 (verified 2026-11-03 via the GitHub API): it is used
strictly as a dev-time black-box oracle in an isolated .venv-fp — zero code
copied, never shipped, never in CI. QuantLib remains the 1e-9 primary
oracle; FinancePy provides INDEPENDENCE evidence:
  - day-count year fractions: exact agreement expected
  - discounting & fair rates: agreement to ~1e-4 (FinancePy's DiscountCurve
    re-parameterizes nodes to zero rates and re-interpolates, so node DFs
    are reproduced only to ~4e-5 — recorded as a finding, not hidden).

Usage: .venv-fp/bin/python fixture-generator/financepy_oracle.py
Writes docs/financepy-crosscheck.json
"""

import json
import os
import sys

import numpy as np
from financepy.market.curves.discount_curve import DiscountCurve
from financepy.utils.date import Date
from financepy.utils.day_count import DayCount, DayCountTypes

ROOT = os.path.join(os.path.dirname(__file__), "..")
OUT = os.path.join(ROOT, "docs", "financepy-crosscheck.json")


def fdate(iso: str) -> Date:
    y, m, d = iso.split("-")
    return Date(int(d), int(m), int(y))


def main() -> int:
    cases = json.load(open(os.path.join(ROOT, "fixtures/rates/cases.json")))["cases"]
    swaps = [c for c in cases if c["kind"] == "swap" and c["input"]["scenario"] == "upward"]
    curves = [c for c in cases if c["kind"] == "curve" and c["id"] == "curve_upward"]
    curve = curves[0]
    pillars = curve["expected"]["pillar_dates"]
    pdates = [fdate(d) for d in pillars]
    pdfs = np.array(list(pillars.values()))
    dc = DiscountCurve(pdates[0], pdates, pdfs)

    act360 = DayCount(DayCountTypes.ACT_360)
    e30360 = DayCount(DayCountTypes.THIRTY_E_360)

    report = {"oracle": "FinancePy 1.0.1 (GPL-3.0, dev-time black box, not shipped/not CI)",
              "primary_oracle": "QuantLib 1.33 (1e-9)", "scope": "upward scenario", "cases": []}

    # 1) day-count exactness on the first fixed coupon period
    sw = [c for c in swaps if c["id"] == "upward_payer5y_off"][0]
    c0 = sw["expected"]["fixed_leg"][0]
    # reconstruct accrual dates from consecutive schedule dates
    dates = [x["date"] for x in sw["expected"]["fixed_leg"]]
    start = fdate(curve["expected"]["spot_date"])
    tau_fp = e30360.year_frac(start, fdate(dates[0]))[0]
    report["cases"].append({
        "check": "30E/360 year fraction (first coupon)",
        "financepy": tau_fp, "quantlib_fixture": c0["yf"],
        "diff": abs(tau_fp - c0["yf"]), "verdict": "exact" if abs(tau_fp - c0["yf"]) < 1e-15 else "MISMATCH",
    })

    # 2) swap fair rate: annuity + telescoped float PV priced with FinancePy
    #    discounting on the SAME coupon dates
    for sw in swaps:
        exp = sw["expected"]
        annuity_fp = 0.0
        prev = fdate(curve["expected"]["spot_date"])
        for cpn in exp["fixed_leg"]:
            d = fdate(cpn["date"])
            annuity_fp += cpn["yf"] * dc.df(d)
        df_mat = dc.df(fdate(exp["fixed_leg"][-1]["date"]))
        float_pv_fp = 1.0 - df_mat
        fair_fp = float_pv_fp / annuity_fp
        q = exp["fair_rate"]
        report["cases"].append({
            "check": f"fair rate {sw['id']}",
            "financepy": fair_fp, "quantlib_fixture": q,
            "diff": abs(fair_fp - q),
            "verdict": "ok(1e-4)" if abs(fair_fp - q) < 1e-4 else "INVESTIGATE",
        })

    # 3) node DF reproduction (documents the re-parameterization finding)
    node_diffs = [abs(float(dc.df(fdate(d))) - v) for d, v in list(pillars.items())[1:]]
    report["curve_node_reproduction_max_abs_diff"] = max(node_diffs)
    report["note"] = ("FinancePy DiscountCurve re-parameterizes node DFs to zero rates and "
                      "re-interpolates; nodes reproduce only to ~4e-5. Fair-rate cross-checks "
                      "therefore use 1e-4, consistent with that oracle's own precision.")

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    json.dump(report, open(OUT, "w"), indent=1, sort_keys=True)
    print(json.dumps(report, indent=1, sort_keys=True)[:1500])
    bad = [c for c in report["cases"] if c["verdict"] not in ("exact", "ok(1e-4)")]
    print("\nCROSSCHECK:", "PASS" if not bad else f"FAIL ({len(bad)})")
    return 0 if not bad else 1


if __name__ == "__main__":
    sys.exit(main())
