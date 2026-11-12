#!/usr/bin/env python3
"""Differential: Vibe-Trading Options Lab vs TrustQuant (QuantLib-pinned).

First cross-implementation check against an external agent-facing product
(the Big-Three integration track; see crosscheck/vibe_options/CONVENTIONS.md).
Vibe is MIT-licensed; this script is manual/optional (heavy pip install) —
`python3 crosscheck/vibe_options/run_diff.py` after
`pip install vibe-trading-ai==0.1.19` into a venv with VIBE_MCP set.

Method: identical inputs by construction — expiry_days is derived from the
actual calendar-day difference of the date pair, so both engines see the
same T. Conventions applied per CONVENTIONS.md (Vibe vega is per 1% vol,
theta is per calendar day; TrustQuant reports per 1.00 vol / per year).
"""

import json
import os
import subprocess
import sys
import time
from datetime import date

VIBE_MCP = os.environ.get("VIBE_MCP", "vibe-trading-mcp")  # venv path or PATH
TQ = os.environ.get("TRUSTQUANT_BIN", os.path.expanduser("~/.local/bin/trustquant"))
DATA_DIR = os.environ.get("TRUSTQUANT_DATA_DIR", "/tmp/tq-vibe-diff")
TOL = 1e-5  # Vibe prints 6 decimals

VAL = date(2023, 6, 21)

CASES = []
for opt in ("call", "put"):
    for spot, strike in [(100, 100), (100, 105), (100, 95), (80, 100), (120, 100)]:
        for vol in (0.05, 0.2, 0.5, 0.9):
            for rate in (0.0, 0.05, -0.005):
                for days in (30, 365, 366, 730, 1825):
                    CASES.append(dict(option_type=opt, spot=spot, strike=strike,
                                      vol=vol, rate=rate, days=days))
# prune to a deterministic sample of 60 for a fast manual run
CASES = [CASES[i] for i in range(0, len(CASES), len(CASES) // 60 + 1)][:60]


class VibeMcp:
    def __init__(self):
        self.p = subprocess.Popen(
            [VIBE_MCP, "--transport", "stdio"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1)

    def send(self, o):
        self.p.stdin.write(json.dumps(o) + "\n")
        self.p.stdin.flush()

    def call(self, name, args, timeout=90):
        self.send({"jsonrpc": "2.0", "id": name, "method": "tools/call",
                   "params": {"name": name, "arguments": args}})
        deadline = time.time() + timeout
        while time.time() < deadline:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("vibe mcp closed stdout")
            d = json.loads(line)
            if d.get("id") == name:
                return json.loads(d["result"]["content"][0]["text"])
        raise TimeoutError(name)

    def close(self):
        self.p.kill()


def trustquant(case):
    mat = date.fromordinal(VAL.toordinal() + case["days"])
    req = {"schema_version": "0.2", "tool": "price_european_option", "params": {
        "option_type": case["option_type"], "spot": case["spot"], "strike": case["strike"],
        "risk_free_rate": case["rate"], "dividend_yield": 0.0, "volatility": case["vol"],
        "valuation_date": VAL.isoformat(), "maturity_date": mat.isoformat(),
        "day_counter": "Actual365Fixed"}}
    out = subprocess.run([TQ, "run", "/dev/stdin"], input=json.dumps(req),
                         capture_output=True, text=True,
                         env={**os.environ, "TRUSTQUANT_DATA_DIR": DATA_DIR})
    env = json.loads(out.stdout)
    if env["status"] != "success":
        raise RuntimeError(env.get("error", {}).get("message", "tq failed"))
    return env["result"], env.get("calculation_id")


def main():
    vibe = VibeMcp()
    vibe.send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
               "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                          "clientInfo": {"name": "tq-diff", "version": "0"}}})
    vibe.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    rows, fails = [], 0
    for i, c in enumerate(CASES):
        v = vibe.call("analyze_options", dict(spot=c["spot"], strike=c["strike"],
                                              expiry_days=c["days"],
                                              risk_free_rate=c["rate"],
                                              volatility=c["vol"],
                                              option_type=c["option_type"]))
        t, calc_id = trustquant(c)
        g = t["greeks"]
        checks = {
            "npv": (v["price"], t["npv"]),
            "delta": (v["delta"], g["delta"]),
            "gamma": (v["gamma"], g["gamma"]),
            "vega(/100)": (v["vega"], g["vega"] / 100.0),
            "theta(/365)": (v["theta"], g["theta"] / 365.0),
            "T": (v["inputs"]["T_years"], t["t"]),
        }
        bad = [k for k, (a, b) in checks.items() if abs(a - b) > TOL + 1e-9 * abs(b)]
        fails += bool(bad)
        rows.append({"case": {**c, "maturity": date.fromordinal(
            VAL.toordinal() + c["days"]).isoformat()},
            "vibe": {k: v[k] for k in ("price", "delta", "gamma", "vega", "theta")},
            "trustquant": {"npv": t["npv"], "delta": g["delta"], "gamma": g["gamma"],
                           "vega": g["vega"], "theta": g["theta"], "t": t["t"]},
            "calculation_id": calc_id, "mismatches": bad})
        print(f"[{i+1}/{len(CASES)}] {'FAIL ' + ','.join(bad) if bad else 'ok'}  "
              f"npv {v['price']} vs {t['npv']:.9f}")
    vibe.close()

    verdict = "PASS" if fails == 0 else f"FAIL ({fails}/{len(CASES)})"
    report = {"verdict": verdict, "cases": len(CASES), "tolerance": TOL,
              "conventions": "see CONVENTIONS.md",
              "vibe_version": "0.1.19", "trustquant": "0.2.0 (QuantLib 1.33-pinned)",
              "rows": rows}
    out = os.path.join(os.path.dirname(__file__), "report-latest.json")
    json.dump(report, open(out, "w"), indent=1)
    print(f"\nVERDICT: {verdict}  (report: {out})")
    return 0 if fails == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
