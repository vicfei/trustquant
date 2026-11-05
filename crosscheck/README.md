# crosscheck/ — differential checks against third-party quant stacks

中文提示：本目录做"惯例核对"式差分——先钉单位口径（[vibe_units.md](vibe_units.md)），
再比数字；结论只对报告头部钉死的版本有效，不说"谁错了"。

Long-term intent: a surviving crosscheck here can be promoted into a third oracle
(next to QuantLib golden fixtures and the in-house second oracle) once it proves
stable across several upstream releases.

## Contents

| File | Role |
|---|---|
| [`vibe_units.md`](vibe_units.md) | Unit & convention cross-reference **written before** any diff conclusion (theta per day vs per year, vega per 1 vol point vs per 1.00 vol, rho coverage, day-count, dividend capability) |
| [`vibe_options_diff.py`](vibe_options_diff.py) | Black-Scholes options diff harness: Vibe-Trading (MCP tool path + core module at full precision) vs `trustquant run` |
| [`reports/`](reports/) | Generated, version-pinned reports (markdown + strict-JSON rows). Committed as evidence. |

## Running

```bash
# inside the Vibe venv (the harness imports vibe-trading-ai directly);
# TRUSTQUANT_BIN defaults to ~/.local/bin/trustquant
~/.venvs/vibe/bin/python crosscheck/vibe_options_diff.py          # full grid (150 cases)
~/.venvs/vibe/bin/python crosscheck/vibe_options_diff.py --quick  # 9-case smoke grid
```

Runtime artifacts (request files, TrustQuant manifests) land in `.run/` (gitignored);
`trustquant replay` of one sampled calculation_id is part of every report.

Exit code: 0 = all engine gates pass and the sample replay reproduces; 1 otherwise.

## Method rules

1. **Units before numbers** — read `vibe_units.md`; alignment factors live in `ALIGN`
   in the harness and mirror that table (theta × 365, vega × 100, rho × 100).
2. **Day-count aligned by construction** — `maturity = valuation + expiry_days`, so
   Vibe's `expiry_days/365` and TrustQuant's Act/365F see the identical year fraction.
3. **Two Vibe columns** — the agent-facing tool path (6-dp display rounding) and the
   core module (full precision). Engine agreement is judged on the core column; the
   tool column must additionally sit within the 6-dp rounding floor of the engine diff.
4. **Convention-check wording** — findings describe agreement levels, rounding floors
   and coverage gaps; never "errors" on either side.
5. **Version pinning** — Vibe ships daily releases; each report records
   `vibe-trading-ai` + `trustquant` versions, python/platform, and the repo rev.
   A conclusion is valid only for the pinned pair.
