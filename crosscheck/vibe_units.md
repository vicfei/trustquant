# Unit & Convention Cross-Reference: Vibe-Trading Options vs TrustQuant

> Status: working note (vibe-trading-ai 0.1.19 × trustquant 0.2.0, checked 2026-11-06).
> Purpose: agree on unit conventions **before** reading any numeric diff produced by
> [`vibe_options_diff.py`](vibe_options_diff.py). Findings below are convention checks,
> not error reports. Every claim cites the exact source location in the pinned version.

## Inputs

| Aspect | Vibe-Trading (`analyze_options` MCP tool) | TrustQuant (`price_european_option`) |
|---|---|---|
| Time to expiry | `expiry_days` → `T = expiry_days / 365.0` at `src/tools/options_pricing_tool.py:139` | `valuation_date` + `maturity_date`, `Actual365Fixed` (echoed in `result.conventions.day_counter`) |
| Day-count basis | actual days ÷ 365 (leap years included in numerator, 365 denominator — equivalent to Act/365F) | Act/365F (same) |
| Risk-free rate | annual, continuously compounded (`src/quantlib/options.py` docstring) | annual, continuously compounded (`result.conventions.rates`) |
| Volatility | annualized, decimal (0.25 = 25%) | annualized, decimal |
| Dividend yield | **not expressible** in the tool schema (core `bs_price`/`bs_greeks` accept `q`, the MCP tool hardcodes q=0) | `dividend_yield`, continuously compounded, default 0 |
| Option style | European call/put | European call/put (American → `unsupported_feature`) |

By construction the diff grid uses `maturity_date = valuation_date + expiry_days`, so both
sides compute the identical year fraction `expiry_days / 365` — day-count is **aligned, not
assumed**.

## Outputs

| Metric | Vibe-Trading native unit | Source (0.1.19) | TrustQuant native unit | Alignment to compare |
|---|---|---|---|---|
| price / npv | currency per unit spot | `bs_price`, rounded 6 dp by tool | currency per unit spot (`units.npv.scale = units_of_spot`) | as-is |
| delta | per 1.0 spot | `bs_greeks` | per unit spot (`units.delta`) | as-is |
| gamma | per 1.0 spot² | `bs_greeks` | per unit spot² (`units.gamma`) | as-is |
| theta | **per calendar day** | `src/quantlib/options.py:347` region: `theta / 365.0` | **per year** (`units.theta.convention`) | Vibe × 365 |
| vega | **per 1 percentage point of vol** | `src/quantlib/options.py`: `vega … / 100.0` | **per 1.00 volatility** (`units.vega.convention`) | Vibe × 100 |
| rho | per 1 percentage point of rate | `src/quantlib/options.py`: `rho = … / 100.0` | **not published** (result schema has no rho) | Vibe × 100, TrustQuant side N/A |

## Known precision & capability notes

1. **Display rounding**: the MCP tool rounds every metric to 6 decimals at its *native*
   scale (`src/tools/options_pricing_tool.py:72-73`). After alignment this becomes a
   granularity floor: 5e-7 for price/delta/gamma, 1.825e-4 for aligned theta, 5e-5 for
   aligned vega/rho. The diff harness therefore also calls the core `bs_price`/`bs_greeks`
   directly (full double precision) so engine-level agreement is measured independently of
   display rounding.
2. **rho asymmetry**: Vibe reports rho; TrustQuant's `bs_analytic.v1` schema omits it.
   Recorded as a coverage gap, not a numeric disagreement.
3. **Dividend asymmetry**: TrustQuant accepts `dividend_yield`; the Vibe tool does not.
   All grid cases therefore use q = 0. (A q ≠ 0 case would be a TrustQuant-only number and
   is out of scope for the diff.)
4. Vibe ships daily releases; every report pins `vibe-trading-ai` explicitly and diff
   conclusions are valid only for the pinned version.
