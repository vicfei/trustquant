#!/usr/bin/env python3
"""Synthetic demo data generator (ADR-0005 §3 / ADR-0006 gate 10).

Deterministic: fixed seed, no network, no real market data. Output is a
daily close price series for a small universe of synthetic tickers under
geometric Brownian motion. The CSV and its SHA256 are committed alongside
this generator; regeneration must reproduce them byte-for-byte.
"""

import hashlib
import csv
import math
import random

SEED = 20260101
TICKERS = {
    "SYNTH_A": (100.0, 0.08, 0.18),
    "SYNTH_B": (64.0, 0.05, 0.24),
    "SYNTH_C": (250.0, 0.10, 0.12),
}
DAYS = 500
OUT = "data/demo_prices.csv"


def business_dates(start_year=2023, start_month=1, count=DAYS):
    # Simple weekday sequence (Mon-Fri), no holiday calendar: synthetic data
    # deliberately avoids market-calendar semantics.
    import datetime

    d = datetime.date(start_year, start_month, 2)
    out = []
    while len(out) < count:
        if d.weekday() < 5:
            out.append(d.isoformat())
        d += datetime.timedelta(days=1)
    return out


def main() -> int:
    rng = random.Random(SEED)
    dates = business_dates()
    rows = []
    prices = {t: p0 for t, (p0, _, _) in TICKERS.items()}
    for d in dates:
        for ticker, (_, mu, vol) in TICKERS.items():
            z = rng.gauss(0.0, 1.0)
            dt = 1.0 / 252.0
            prices[ticker] *= math.exp((mu - 0.5 * vol * vol) * dt + vol * math.sqrt(dt) * z)
            rows.append({"date": d, "ticker": ticker, "close": f"{prices[ticker]:.6f}"})

    with open(OUT, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=["date", "ticker", "close"])
        writer.writeheader()
        writer.writerows(rows)

    with open(OUT, "rb") as f:
        digest = hashlib.sha256(f.read()).hexdigest()
    print(f"wrote {OUT}: {len(rows)} rows, seed={SEED}")
    print(f"sha256: {digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
