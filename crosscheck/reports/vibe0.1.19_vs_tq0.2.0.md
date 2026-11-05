# Vibe-Trading × TrustQuant — options crosscheck (convention check)

Ran 2026-11-06T02:10:48+08:00 · 150 cases · verdict: **agree**

- vibe-trading-ai **0.1.19** (tool path `options_pricing` + core `src.quantlib.options`)
- trustquant **0.2.0** (`bs_analytic.v1` via `trustquant run`; repo rev a45bf84)
- day-count aligned by construction: maturity = valuation + expiry_days, so Act/365F gives exactly expiry_days/365 on both sides
- unit alignment before comparison (see [../vibe_units.md](../vibe_units.md)): Vibe theta × 365, vega × 100, rho × 100; price/delta/gamma as-is

## Engine-level agreement (vibe_core full precision vs trustquant)

| metric | max abs diff | max rel diff | gate (rel ≤ 1e-9 + floor) |
|---|---|---|---|
| price | 3.553e-14 | 3.045e-09 | PASS |
| delta | 1.110e-16 | 4.624e-13 | PASS |
| gamma | 6.939e-18 | 1.971e-16 | PASS |
| theta | 3.553e-15 | 5.194e-13 | PASS |
| vega | 7.105e-15 | 2.018e-16 | PASS |

(max rel diff is taken over cases where the metric dominates the absolute floor; 56 metric-cases with near-zero values — deep OTM at days-to-expiry — are gated by the absolute floor instead.)

## Tool-level diffs vs rounding floor

The MCP tool rounds to 6 decimals at native scale; after alignment that is a granularity floor per metric. Every tool-level diff must sit within that floor of the engine-level diff.

| metric | rounding floor (aligned) | all cases within floor |
|---|---|---|
| price | 5.000e-07 | yes |
| delta | 5.000e-07 | yes |
| gamma | 5.000e-07 | yes |
| theta | 1.825e-04 | yes |
| vega | 5.000e-05 | yes |

## Coverage notes

- rho: Vibe-only coverage; TrustQuant result schema omits rho (coverage gap, not a disagreement)
- Vibe tool layer expresses no dividend yield (q = 0 hardcoded); all cases use q = 0.
- Audit trail: sample `trustquant replay 1461c1a7-af6c-4459-ae0f-62a041cc0adc` → REPLAY PASS (same version, same environment).

Machine-readable rows: vibe0.1.19_vs_tq0.2.0.json
