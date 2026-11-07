# Verification entry

> 每一条 TrustQuant 的主张，配一条任何人都能跑的验证命令。从干净克隆出发；"通过信号"是正常运行会打印的内容。
>
> Every claim this project makes, with the command that checks it. Start from a clean clone.

| # | Claim | Check (command) | Passing signal |
|---|---|---|---|
| 1 | Option engine matches **QuantLib 1.33** golden fixtures (options, calendars, rates) | `make test` (fmt + clippy + wasm build + full workspace tests) | all fixture tests green — 18 option-fixture groups, 90-point put-call parity grid, 92 rates fixtures, 80 calendar fixtures |
| 2 | CLI / MCP / library are **one implementation** | included in the workspace tests (3-entry conformance suite) | conformance tests pass; identical results + normalized-identical error envelopes |
| 3 | Every calculation **replays** (same version, same environment) | `TRUSTQUANT_DATA_DIR=/tmp/tq make demo` | runs `price_european_option` → manifest → `trustquant replay` and prints **REPLAY: PASS** |
| 4 | The external provider is **explicit, never silent** | `make provider` | sidecar channel check (cold start / version echo / round-trip); no provider → `provider_unavailable`, never a fallback |
| 5 | An **independent second oracle** agrees | included in CI/tests: `fixture-generator/rates_oracle.py` + `portfolio_oracle.py` re-derive curves/swaps/risk by *different* methods (bisection vs closed-form; cashflow summation vs telescoping) | agreement tests pass (curve DFs 1e-11, telescoping gap ~1e-16) |
| 6 | A **third-party stack** diffs clean against us | `~/.venvs/vibe/bin/python crosscheck/vibe_options_diff.py --quick` (needs `vibe-trading-ai` in a venv; full grid without `--quick`) | verdict `agree`, sample `trustquant replay` PASS; committed pinned report: [crosscheck/reports/](../crosscheck/reports/) (vibe 0.1.19 × tq 0.2.0, 150 cases, engine-level max rel 3e-9) |
| 7 | The raw-LLM-vs-oracle essay's numbers are **reproducible from committed data** | `python3 essays/llm-numbers-vs-oracle/run_experiment.py` | regenerates `stats.json` + both charts from the committed `raw.json` (no API calls) |
| 8 | CI runs 1–5 on every push | the CI badge on the README ([ci.yml](../.github/workflows/ci.yml)) | green |
| 9 | Version/oracle/toolchain lineage | [RELEASE-LINEAGE.md](../RELEASE-LINEAGE.md), [docs/dod-matrix.md](dod-matrix.md), [docs/provider-status.md](provider-status.md) | — |

Notes on what verification does **not** claim: replay is same-version/same-environment (cross-version gets diff reports, not equality promises); WASM is a differential-validation target, not a shipped interface. The full honest list lives in the README's *Known limitations*.
