#!/usr/bin/env python3
"""Experiment: raw-LLM option prices vs the TrustQuant oracle.

Motivation (see README.md in this directory): agent trading frameworks
(TradingAgents-style debate stacks, hedge-fund agent crews) let numeric
claims travel through the conversation as text. What happens when the LLM
itself has to produce the number — no tools, no code — and the number is
checkable? Black-Scholes is the ideal probe: closed-form, deterministic,
oracle-pinned.

Modes:

    run_experiment.py --run     # call the DeepSeek API for 80 seeded cases,
                                # price each with trustquant, write raw.json
    run_experiment.py           # analyze committed raw.json -> stats.json,
                                # two charts, exit 0

The committed raw.json IS the dataset of the article; --run refreshes it
(temperature 0 is not perfectly deterministic server-side, so re-runs drift
slightly — that is documented, not hidden).

Honesty rules baked in: single model (deepseek-flash), single prompt shape,
zero-shot, price only. Greeks and unit conventions are out of scope here and
covered instead by the Vibe-Trading crosscheck in crosscheck/.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import re
import subprocess
import sys
import urllib.request
from datetime import date, timedelta
from pathlib import Path

HERE = Path(__file__).resolve().parent
TRUSTQUANT_BIN = os.environ.get("TRUSTQUANT_BIN", "trustquant")
TQ_DATA = HERE / ".tqdata"
VALUATION = date(2024, 1, 1)

SEED = 20261106
N_CASES = 80
MODEL = "deepseek-flash"
API_URL = "https://api.deepseek.com/chat/completions"

PROMPT = (
    "Estimate the Black-Scholes price of a European {opt} option:\n"
    "spot = {spot:.0f}\n"
    "strike = {strike:.2f}\n"
    "time to expiry = {days:.0f} calendar days\n"
    "risk-free rate = {rate_pct:.2f}% per year, continuously compounded\n"
    "volatility = {vol_pct:.1f}% per year\n"
    "no dividends.\n"
    "Answer with ONLY the number (price per unit of spot), no explanation, "
    "no code, no tools. If you must estimate, estimate."
)


def build_cases() -> list[dict]:
    rng = random.Random(SEED)
    cases = []
    for _ in range(N_CASES):
        moneyness = rng.uniform(0.85, 1.15)
        days = int(rng.uniform(14, 365))
        vol = rng.uniform(0.10, 0.50)
        r = rng.uniform(0.0, 0.06)
        opt = rng.choice(["call", "put"])
        cases.append({
            "case_id": len(cases),
            "option_type": opt,
            "spot": 100.0,
            "strike": round(moneyness * 100.0, 2),
            "days": days,
            "volatility": round(vol, 4),
            "risk_free_rate": round(r, 4),
            "moneyness": round(moneyness, 4),
            "valuation_date": VALUATION.isoformat(),
            "maturity_date": (VALUATION + timedelta(days=days)).isoformat(),
        })
    return cases


def ask_deepseek(case: dict, api_key: str) -> dict:
    body = json.dumps({
        "model": MODEL,
        "temperature": 0.0,
        # deepseek-flash reasons at length (it hand-computes the normal CDF);
        # a small max_tokens burns out inside reasoning_content with an empty
        # final answer — recorded here because it is itself a finding.
        "max_tokens": 16384,
        "messages": [{"role": "user", "content": PROMPT.format(
            opt=case["option_type"], spot=case["spot"], strike=case["strike"],
            days=case["days"], rate_pct=case["risk_free_rate"] * 100,
            vol_pct=case["volatility"] * 100)}],
    }).encode()
    req = urllib.request.Request(API_URL, data=body, headers={
        "Content-Type": "application/json",
        "Authorization": f"Bearer {api_key}",
    })
    with urllib.request.urlopen(req, timeout=300) as resp:
        out = json.load(resp)
    choice = out["choices"][0]
    message = choice["message"]
    usage = out.get("usage", {}).get("completion_tokens_details", {})
    return {"raw_answer": message.get("content") or "",
            "reasoning_tail": (message.get("reasoning_content") or "")[-160:],
            "finish_reason": choice.get("finish_reason"),
            "reasoning_tokens": usage.get("reasoning_tokens")}


def parse_number(text: str) -> float | None:
    if not text:
        return None
    m = re.findall(r"-?\d+(?:\.\d+)?", text.replace(",", ""))
    if not m:
        return None
    return float(m[0])


def oracle(case: dict) -> dict:
    TQ_DATA.mkdir(parents=True, exist_ok=True)
    req = {
        "schema_version": "0.2",
        "tool": "price_european_option",
        "params": {
            "option_type": case["option_type"],
            "spot": case["spot"],
            "strike": case["strike"],
            "risk_free_rate": case["risk_free_rate"],
            "dividend_yield": 0.0,
            "volatility": case["volatility"],
            "valuation_date": case["valuation_date"],
            "maturity_date": case["maturity_date"],
        },
    }
    p = HERE / f".req_{os.getpid()}.json"
    p.write_text(json.dumps(req))
    proc = subprocess.run([TRUSTQUANT_BIN, "run", str(p)], capture_output=True,
                          text=True, timeout=60,
                          env={**os.environ, "TRUSTQUANT_DATA_DIR": str(TQ_DATA)})
    out = json.loads(proc.stdout)
    if out.get("status") != "success":
        raise RuntimeError(f"oracle failed for case {case['case_id']}: {out}")
    return {"oracle_price": out["result"]["npv"],
            "calculation_id": out["calculation_id"]}


def mode_run() -> None:
    api_key = os.environ.get("DEEPSEEK_API_KEY")
    if not api_key:
        sys.exit("DEEPSEEK_API_KEY not set")
    cases = build_cases()

    def one(case: dict) -> dict:
        last_err = None
        for attempt in range(2):
            try:
                ans = ask_deepseek(case, api_key)
                orc = oracle(case)
                llm_price = parse_number(ans["raw_answer"])
                return {**case, **orc,
                        "llm_raw": ans["raw_answer"].strip()[:300],
                        "llm_price": llm_price,
                        "finish_reason": ans["finish_reason"],
                        "reasoning_tokens": ans["reasoning_tokens"],
                        "reasoning_tail": ans["reasoning_tail"]}
            except Exception as exc:            # one retry, then record the miss
                last_err = exc
        return {**case, "oracle_price": None, "calculation_id": None,
                "llm_raw": f"<harness error: {type(last_err).__name__}>",
                "llm_price": None}

    rows: list = [None] * len(cases)
    done = 0
    lock = __import__("threading").Lock()

    def save() -> None:
        payload = {
            "model": MODEL,
            "temperature": 0.0,
            "seed": SEED,
            "n_cases": len(cases),
            "oracle": "trustquant bs_analytic.v1 (QuantLib-1.33-pinned fixtures)",
            "rows": [r for r in rows if r is not None],
        }
        (HERE / "raw.json").write_text(json.dumps(payload, indent=1))

    from concurrent.futures import ThreadPoolExecutor
    with ThreadPoolExecutor(max_workers=6) as pool:
        futures = {pool.submit(one, c): i for i, c in enumerate(cases)}
        import concurrent.futures as cf
        for fut in cf.as_completed(futures):
            i = futures[fut]
            rows[i] = fut.result()
            with lock:
                done += 1
                save()
                r = rows[i]
                rel = None
                if r["llm_price"] is not None and r["oracle_price"]:
                    rel = (r["llm_price"] - r["oracle_price"]) / r["oracle_price"]
                print(f"[{done}/{len(cases)}] {r['option_type']:4s} K={r['strike']:7.2f} "
                      f"T={r['days']:3d}d vol={r['volatility']:.2f} "
                      f"oracle={r['oracle_price']} llm={r['llm_price']} "
                      f"rel={'n/a' if rel is None else f'{rel:+.1%}'}", flush=True)
    save()
    print(f"raw data: {HERE / 'raw.json'}")


def bucket(rel: float) -> str:
    a = abs(rel)
    if a < 0.01:
        return "<1%"
    if a < 0.05:
        return "1–5%"
    if a < 0.10:
        return "5–10%"
    if a < 0.25:
        return "10–25%"
    return "≥25%"


def mode_analyze() -> dict:
    data = json.loads((HERE / "raw.json").read_text())
    rows = []
    for r in data["rows"]:
        if r["llm_price"] is None or not r["oracle_price"]:
            rows.append({**r, "rel_err": None, "bucket": "no-answer"})
            continue
        rel = (r["llm_price"] - r["oracle_price"]) / r["oracle_price"]
        rows.append({**r, "rel_err": rel, "bucket": bucket(rel)})
    answered = [r for r in rows if r["rel_err"] is not None]
    absrel = sorted(abs(r["rel_err"]) for r in answered)

    def pct(p):
        return absrel[min(len(absrel) - 1, int(p * len(absrel)))]

    buckets = ["<1%", "1–5%", "5–10%", "10–25%", "≥25%", "no-answer"]
    counts = {b: sum(1 for r in rows if r["bucket"] == b) for b in buckets}

    # breakdown by moneyness tercile
    def band(m):
        return ("OTM (K<S)" if m < 0.98 else "ATM (0.98–1.02)" if m <= 1.02 else "ITM (K>S)")
    bands = {}
    for name in ("OTM (K<S)", "ATM (0.98–1.02)", "ITM (K>S)"):
        sub = [abs(r["rel_err"]) for r in answered if band(r["moneyness"]) == name]
        if sub:
            bands[name] = {"n": len(sub), "median_abs_rel": sorted(sub)[len(sub)//2],
                           "p90_abs_rel": sorted(sub)[min(len(sub)-1, int(0.9*len(sub)))]}

    stats = {
        "model": data["model"], "n": len(rows), "answered": len(answered),
        "no_answer": counts["no-answer"],
        "no_answer_burned_budget": sum(
            1 for r in rows if r["rel_err"] is None and r.get("finish_reason") == "length"),
        "median_reasoning_tokens": sorted(
            r.get("reasoning_tokens") or 0 for r in rows)[len(rows)//2],
        "median_abs_rel": sorted(absrel)[len(absrel)//2],
        "mean_abs_rel": sum(absrel)/len(absrel),
        "p90_abs_rel": pct(0.9),
        "max_abs_rel": absrel[-1],
        "buckets": counts,
        "by_moneyness": bands,
        "within_5pct": sum(1 for a in absrel if a < 0.05),
        "off_by_25pct_or_more": sum(1 for a in absrel if a >= 0.25),
    }
    (HERE / "stats.json").write_text(json.dumps(stats, indent=1))

    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots(figsize=(7.2, 4.2))
    order = ["<1%", "1–5%", "5–10%", "10–25%", "≥25%", "no-answer\n(16k budget burned)"]
    vals = [counts[b] for b in order[:5]] + [counts["no-answer"]]
    colors = ["#2e7d32", "#7cb342", "#f9a825", "#ef6c00", "#c62828", "#37474f"]
    bars = ax.bar(order, vals, color=colors)
    for b, v in zip(bars, vals):
        ax.text(b.get_x() + b.get_width()/2, v + 0.5, str(v), ha="center", fontsize=10)
    ax.set_ylabel("cases")
    ax.set_title(f"Raw LLM ({stats['model']}) Black-Scholes prices vs oracle — n={stats['n']}\n"
                 "bimodal: near-oracle precision when it answers — or nothing at all")
    fig.tight_layout()
    fig.savefig(HERE / "error_buckets.png", dpi=150)

    fig, ax = plt.subplots(figsize=(7.2, 4.2))
    answered_tok = [r.get("reasoning_tokens") or 0 for r in rows if r["rel_err"] is not None]
    burn_tok = [r.get("reasoning_tokens") or 0 for r in rows
                if r["rel_err"] is None and r.get("finish_reason") == "length"]
    bins = range(4000, 18000, 1000)
    ax.hist(answered_tok, bins=bins, color="#1565c0", alpha=0.8,
            label=f"answered (n={len(answered_tok)})")
    ax.hist(burn_tok, bins=bins, color="#c62828", alpha=0.8,
            label=f"burned the 16,384 budget, no answer (n={len(burn_tok)})")
    ax.set_xlabel("reasoning tokens spent on ONE number")
    ax.set_ylabel("cases")
    ax.set_title("The cost of one option price from a raw LLM\n"
                 "(the oracle answers the same question in ~30 ms, manifest + replay included)")
    ax.legend()
    fig.tight_layout()
    fig.savefig(HERE / "reasoning_cost.png", dpi=150)

    print(json.dumps(stats, indent=1))
    return stats


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", action="store_true", help="call the API and refresh raw.json (default: analyze committed data)")
    args = ap.parse_args()
    mode_run() if args.run else mode_analyze()
