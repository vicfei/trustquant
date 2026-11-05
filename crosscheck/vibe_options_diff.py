#!/usr/bin/env python3
"""Cross-check Vibe-Trading's Black-Scholes options math against TrustQuant.

Run inside the Vibe venv (it imports the vibe-trading-ai package directly):

    ~/.venvs/vibe/bin/python crosscheck/vibe_options_diff.py [--quick]

For each grid case the script collects three columns:

  vibe_tool : the agent-facing MCP tool path (`options_pricing` registry tool,
              6-decimal display rounding) — what an agent actually consumes
  vibe_core : `src.quantlib.options.bs_price`/`bs_greeks` called directly at
              full double precision — the engine itself
  trustquant: the public `trustquant run` CLI (`bs_analytic.v1`, full
              precision, manifest recorded under crosscheck/.run/tqdata)

Units are aligned per crosscheck/vibe_units.md BEFORE any comparison:
Vibe theta (per day) x 365, vega (per 1 vol point) x 100, rho (per 1 rate
point) x 100; price/delta/gamma as-is. rho has no TrustQuant counterpart and
is reported as Vibe-only coverage.

Wording rule: this is a convention check between two independent
implementations. The report speaks of agreement levels and rounding floors,
never of "errors" on either side. Conclusions are pinned to the exact
versions recorded in the report header (vibe-trading-ai ships daily).
"""

from __future__ import annotations

import argparse
import datetime as dt
import importlib.metadata
import json
import math
import os
import platform
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
RUN_DIR = Path(__file__).resolve().parent / ".run"
REPORTS_DIR = Path(__file__).resolve().parent / "reports"
TRUSTQUANT_BIN = os.environ.get("TRUSTQUANT_BIN", str(Path.home() / ".local/bin/trustquant"))
TRUSTQUANT_DATA_DIR = RUN_DIR / "tqdata"

VALUATION_DATE = dt.date(2024, 1, 1)

GRID_FULL = {
    "option_type": ["call", "put"],
    "moneyness": [0.80, 0.90, 1.00, 1.10, 1.25],   # strike = moneyness * spot
    "expiry_days": [7, 30, 90, 180, 365],
    "volatility": [0.10, 0.25, 0.50],
    "risk_free_rate": [0.045],
}
GRID_QUICK = {
    "option_type": ["call"],
    "moneyness": [0.90, 1.00, 1.10],
    "expiry_days": [30, 90, 365],
    "volatility": [0.25],
    "risk_free_rate": [0.045],
}

SPOT = 100.0

# Alignment factors from crosscheck/vibe_units.md: native Vibe scale -> TrustQuant scale.
ALIGN = {"price": 1.0, "delta": 1.0, "gamma": 1.0, "theta": 365.0, "vega": 100.0, "rho": 100.0}

# PASS gate (engine level, vibe_core vs trustquant): |a-b| <= rel_tol*|b| + abs_floor.
# rel_tol 1e-9 leaves two orders of margin over the expected ~1e-13 cross-library
# normal-CDF difference; abs_floor only matters for near-zero metric values.
REL_TOL = 1e-9
ABS_FLOOR = {"price": 1e-9, "delta": 1e-10, "gamma": 1e-12, "theta": 1e-9, "vega": 1e-9}


def git_rev() -> str:
    try:
        return subprocess.run(
            ["git", "-C", str(REPO_ROOT), "rev-parse", "--short", "HEAD"],
            capture_output=True, text=True, timeout=10,
        ).stdout.strip()
    except Exception:
        return "unknown"


def build_grid(spec: dict):
    for option_type in spec["option_type"]:
        for moneyness in spec["moneyness"]:
            for expiry_days in spec["expiry_days"]:
                for vol in spec["volatility"]:
                    for r in spec["risk_free_rate"]:
                        yield {
                            "option_type": option_type,
                            "spot": SPOT,
                            "strike": round(moneyness * SPOT, 10),
                            "expiry_days": expiry_days,
                            "volatility": vol,
                            "risk_free_rate": r,
                        }


def call_vibe_tool(registry, case: dict) -> dict:
    payload = {
        "spot": case["spot"],
        "strike": case["strike"],
        "expiry_days": case["expiry_days"],
        "risk_free_rate": case["risk_free_rate"],
        "volatility": case["volatility"],
        "option_type": case["option_type"],
    }
    return json.loads(registry.execute("options_pricing", payload))


def call_vibe_core(case: dict) -> dict:
    from src.quantlib.options import bs_greeks, bs_price

    T = case["expiry_days"] / 365.0
    price = bs_price(case["spot"], case["strike"], T, case["risk_free_rate"],
                     case["volatility"], case["option_type"], 0.0)
    greeks = bs_greeks(case["spot"], case["strike"], T, case["risk_free_rate"],
                       case["volatility"], case["option_type"], 0.0)
    return {"price": float(price), **{k: float(v) for k, v in greeks.items()}}


def call_trustquant(case: dict) -> dict:
    maturity = VALUATION_DATE + dt.timedelta(days=case["expiry_days"])
    request = {
        "schema_version": "0.2",
        "tool": "price_european_option",
        "params": {
            "option_type": case["option_type"],
            "spot": case["spot"],
            "strike": case["strike"],
            "risk_free_rate": case["risk_free_rate"],
            "dividend_yield": 0.0,
            "volatility": case["volatility"],
            "valuation_date": VALUATION_DATE.isoformat(),
            "maturity_date": maturity.isoformat(),
        },
    }
    req_path = RUN_DIR / f"req_{os.getpid()}_{next(call_trustquant._seq)}.json"
    req_path.write_text(json.dumps(request))
    proc = subprocess.run(
        [TRUSTQUANT_BIN, "run", str(req_path)],
        capture_output=True, text=True, timeout=60,
        env={**os.environ, "TRUSTQUANT_DATA_DIR": str(TRUSTQUANT_DATA_DIR)},
    )
    if proc.returncode != 0:
        raise RuntimeError(f"trustquant run failed: {proc.stderr.strip()[:400]}")
    return json.loads(proc.stdout)


call_trustquant._seq = iter(range(10 ** 9))


def trustquant_version() -> str:
    proc = subprocess.run([TRUSTQUANT_BIN, "version"], capture_output=True, text=True, timeout=30)
    out = proc.stdout.strip() or proc.stderr.strip() or "unknown"
    match = re.search(r"\d+\.\d+\.\d+", out)
    return match.group(0) if match else out


def replay_sample(calculation_id: str) -> dict:
    proc = subprocess.run(
        [TRUSTQUANT_BIN, "replay", calculation_id, "--json"],
        capture_output=True, text=True, timeout=60,
        env={**os.environ, "TRUSTQUANT_DATA_DIR": str(TRUSTQUANT_DATA_DIR)},
    )
    try:
        parsed = json.loads(proc.stdout)
    except json.JSONDecodeError:
        parsed = {}
    ok = (
        proc.returncode == 0
        and parsed.get("manifest_hash_valid") is True
        and parsed.get("result_digest_match") is True
    )
    return {"exit": proc.returncode, "ok": ok, "json": parsed}


def rel_diff(a: float, b: float) -> float:
    if b == 0.0:
        return math.inf if a != 0.0 else 0.0
    return abs(a - b) / abs(b)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--quick", action="store_true", help="small smoke grid instead of the full grid")
    args = parser.parse_args()

    RUN_DIR.mkdir(parents=True, exist_ok=True)
    TRUSTQUANT_DATA_DIR.mkdir(parents=True, exist_ok=True)
    REPORTS_DIR.mkdir(parents=True, exist_ok=True)

    from src.tools import build_registry

    registry = build_registry(include_shell_tools=False)

    vibe_ver = importlib.metadata.version("vibe-trading-ai")
    tq_ver = trustquant_version()
    ran_at = dt.datetime.now().astimezone().isoformat(timespec="seconds")

    grid = list(build_grid(GRID_QUICK if args.quick else GRID_FULL))
    rows = []
    first_calc_id = None
    for case in grid:
        tool = call_vibe_tool(registry, case)
        core = call_vibe_core(case)
        tq_raw = call_trustquant(case)
        if tq_raw.get("status") != "success":
            raise RuntimeError(f"trustquant non-success for {case}: {tq_raw}")
        tq_result = tq_raw["result"]
        if first_calc_id is None:
            first_calc_id = tq_raw["calculation_id"]

        tq_metrics = {
            "price": tq_result["npv"],
            "delta": tq_result["greeks"]["delta"],
            "gamma": tq_result["greeks"]["gamma"],
            "theta": tq_result["greeks"]["theta"],
            "vega": tq_result["greeks"]["vega"],
        }
        row = {"case": case, "calculation_id": tq_raw["calculation_id"], "metrics": {}}
        for metric, factor in ALIGN.items():
            tool_aligned = tool[metric] * factor if metric in tool else None
            core_aligned = core[metric] * factor
            tq_value = tq_metrics.get(metric)
            entry = {
                "vibe_tool_aligned": tool_aligned,
                "vibe_core_aligned": core_aligned,
                "trustquant": tq_value,
                "diff_tool_minus_tq": None if tq_value is None or tool_aligned is None
                else tool_aligned - tq_value,
                "diff_core_minus_tq": None if tq_value is None else core_aligned - tq_value,
                # null when the TrustQuant value is exactly zero (deep OTM): a relative
                # diff is undefined there and the absolute floor governs instead.
                "rel_core": None if tq_value is None or tq_value == 0.0
                else rel_diff(core_aligned, tq_value),
            }
            row["metrics"][metric] = entry
        rows.append(row)

    # Replay one representative calculation for the audit trail.
    replay = replay_sample(first_calc_id)

    # Summary: engine level (core vs tq) drives the verdict; tool level must sit
    # within the 6-dp rounding floor of the engine-level diff.
    summary = {}
    verdict = "agree"
    for metric in ["price", "delta", "gamma", "theta", "vega"]:
        # Relative diffs are only meaningful when the metric itself dominates the
        # absolute floor; near-zero cases (deep OTM, days to expiry) are floor-gated.
        sig = [r for r in rows
               if abs(r["metrics"][metric]["trustquant"]) > 1000 * ABS_FLOOR[metric]]
        core_abs = max(abs(r["metrics"][metric]["diff_core_minus_tq"]) for r in rows)
        worst_rel = max((r["metrics"][metric]["rel_core"] for r in sig), default=0.0)
        gate_ok = all(
            abs(r["metrics"][metric]["diff_core_minus_tq"])
            <= REL_TOL * abs(r["metrics"][metric]["trustquant"]) + ABS_FLOOR[metric]
            for r in rows
        )
        # Rounding-floor attribution: |tool - tq| - |core - tq| <= 5e-7 * scale (+ eps).
        floor = 0.5e-6 * ALIGN[metric]
        rounding_ok = all(
            abs(r["metrics"][metric]["diff_tool_minus_tq"])
            - abs(r["metrics"][metric]["diff_core_minus_tq"])
            <= floor * (1 + 1e-6) + 1e-12
            for r in rows
        )
        if not gate_ok:
            verdict = "investigate"
        summary[metric] = {
            "max_abs_diff_core": core_abs,
            "max_rel_diff_core_significant": worst_rel,
            "near_zero_cases_floor_gated": len(rows) - len(sig),
            "engine_gate_pass": gate_ok,
            "tool_diff_within_rounding_floor": rounding_ok,
            "rounding_floor": floor,
        }
    rho_note = {
        "vibe_reports": True,
        "trustquant_reports": False,
        "note": "rho: Vibe-only coverage; TrustQuant result schema omits rho (coverage gap, not a disagreement)",
    }

    replay_ok = replay["ok"]

    report = {
        "kind": "vibe-trading x trustquant options crosscheck",
        "wording": "convention check between independent implementations; no error attribution",
        "ran_at": ran_at,
        "versions": {
            "vibe-trading-ai": vibe_ver,
            "trustquant": tq_ver,
            "python": sys.version.split()[0],
            "platform": platform.platform(),
            "trustquant_repo_rev": git_rev(),
        },
        "grid": GRID_QUICK if args.quick else GRID_FULL,
        "case_count": len(rows),
        "day_count_alignment": "maturity = valuation + expiry_days, so Act/365F gives exactly expiry_days/365 on both sides",
        "alignment": {k: v for k, v in ALIGN.items() if v != 1.0},
        "gate": {"rel_tol": REL_TOL, "abs_floor": ABS_FLOOR},
        "summary": summary,
        "rho": rho_note,
        "replay": {"calculation_id": first_calc_id, "ok": replay_ok, "detail": replay},
        "verdict": verdict,
        "rows": rows,
    }

    stem = f"vibe{vibe_ver}_vs_tq{tq_ver}"
    json_path = REPORTS_DIR / f"{stem}.json"
    json_path.write_text(json.dumps(report, indent=1))

    # Markdown rendering
    lines = [
        "# Vibe-Trading × TrustQuant — options crosscheck (convention check)",
        "",
        f"Ran {ran_at} · {len(rows)} cases · verdict: **{verdict}**",
        "",
        f"- vibe-trading-ai **{vibe_ver}** (tool path `options_pricing` + core `src.quantlib.options`)",
        f"- trustquant **{tq_ver}** (`bs_analytic.v1` via `trustquant run`; repo rev {git_rev()})",
        f"- day-count aligned by construction: {report['day_count_alignment']}",
        "- unit alignment before comparison (see [../vibe_units.md](../vibe_units.md)): "
        "Vibe theta × 365, vega × 100, rho × 100; price/delta/gamma as-is",
        "",
        "## Engine-level agreement (vibe_core full precision vs trustquant)",
        "",
        "| metric | max abs diff | max rel diff | gate (rel ≤ 1e-9 + floor) |",
        "|---|---|---|---|",
    ]
    for metric, s in summary.items():
        lines.append(
            f"| {metric} | {s['max_abs_diff_core']:.3e} | {s['max_rel_diff_core_significant']:.3e} | "
            f"{'PASS' if s['engine_gate_pass'] else 'FAIL'} |"
        )
    lines += [
        "",
        f"(max rel diff is taken over cases where the metric dominates the absolute floor; "
        f"{sum(s['near_zero_cases_floor_gated'] for s in summary.values())} metric-cases with near-zero "
        f"values — deep OTM at days-to-expiry — are gated by the absolute floor instead.)",
        "",
        "## Tool-level diffs vs rounding floor",
        "",
        "The MCP tool rounds to 6 decimals at native scale; after alignment that is a "
        "granularity floor per metric. Every tool-level diff must sit within that floor "
        "of the engine-level diff.",
        "",
        "| metric | rounding floor (aligned) | all cases within floor |",
        "|---|---|---|",
    ]
    for metric, s in summary.items():
        lines.append(f"| {metric} | {s['rounding_floor']:.3e} | {'yes' if s['tool_diff_within_rounding_floor'] else 'NO'} |")
    lines += [
        "",
        "## Coverage notes",
        "",
        f"- {rho_note['note']}",
        "- Vibe tool layer expresses no dividend yield (q = 0 hardcoded); all cases use q = 0.",
        f"- Audit trail: sample `trustquant replay {first_calc_id}` → "
        f"{'REPLAY PASS' if replay_ok else 'replay not confirmed'} (same version, same environment).",
        "",
        "Machine-readable rows: " + json_path.name,
        "",
    ]
    md_path = REPORTS_DIR / f"{stem}.md"
    md_path.write_text("\n".join(lines))

    print(f"cases={len(rows)} verdict={verdict}")
    for metric, s in summary.items():
        print(f"{metric:6s} max_abs={s['max_abs_diff_core']:.3e} "
              f"max_rel={s['max_rel_diff_core_significant']:.3e} gate={'PASS' if s['engine_gate_pass'] else 'FAIL'} "
              f"rounding={'ok' if s['tool_diff_within_rounding_floor'] else 'NO'}")
    print(f"replay {'PASS' if replay_ok else 'NOT CONFIRMED'} ({first_calc_id})")
    print(f"report: {md_path}")
    print(f"json:   {json_path}")
    return 0 if verdict == "agree" and replay_ok else 1


if __name__ == "__main__":
    sys.exit(main())
