# TrustQuant

> Open, local-first, agent-facing financial tool layer: **trusted numerical core → one tool contract → provider routing → calculation manifests and replay**.
> Status: **M1 public preview candidate** — three end-to-end demos, every tool oracle-pinned, every calculation recorded and replayable. License: **Apache-2.0**. (中文说明：README.zh-CN.md)

## Why

A general agent shouldn't need to know QuantLib, PyPortfolioOpt or day-count conventions to price an option, optimize a portfolio or run a stress test — but it MUST get unit-correct, verifiable, replayable answers, and honest refusals when something is out of scope. TrustQuant is that layer:

- **Oracle-pinned numerics**: 18 QuantLib 1.33 golden fixtures + a 90-point put-call-parity grid pin the option engine; a pure-Python independent oracle pins risk/stress metrics and analytic optimization references (global min-variance and interior tangency solutions).
- **Honest states, never silent fallbacks**: american options → `unsupported_feature` with no fabricated number; missing provider → `provider_unavailable` with an install hint; missing dataset/manifest → `artifact_unavailable` / `replay_unavailable`.
- **Everything is recorded**: every call gets a `calculation_id`; datasets are content-addressed; manifests record who computed, with which versions, on which inputs; replay (same version, same environment) verifies digests and reports diffs.
- **Units are machine-readable**: per-metric unit semantics (Vega per 1.00 vol, Theta per year, daily simple returns, 252-day year) travel with every result.

## Quickstart (no credentials, no cloud)

```bash
make test                 # fixtures + invariants + 3-entry conformance + sidecar + evals
make demo                 # Demo 1: option pricing -> manifest -> replay (REPLAY: PASS)
make provider             # external Python provider channel check
trustquant serve          # http://127.0.0.1:7860 - all three demos in one page

# Full profile (portfolio optimization demo):
trustquant provider install   # explicit, pinned venv (sidecar/requirements.txt) - the ONLY network step
```

Lightweight profile = the Rust binary alone (option pricing, risk, stress, manifests, replay, Web/MCP). One-stop profile adds the pinned Python provider (PyPortfolioOpt) for `optimize_portfolio`. The provider is **never** auto-installed and never silently substituted.

## The three demos

| Demo | Tools | Numerical evidence |
|---|---|---|
| Option pricing & Greeks | `price_european_option` | 18 QuantLib fixtures (1d-10y, vol 1e-6-0.9, zero/negative rates) + parity grid + zero-vol limit |
| Portfolio optimization | `load_market_dataset` -> `optimize_portfolio` | Analytic min-variance/tangency references from an independent pure-Python oracle + constraint invariants |
| Risk, stress & replay | `analyze_portfolio_risk`, `stress_test_portfolio`, `replay_calculation` | Oracle fixtures incl. hand-computed cases; contribution-sum invariant; replay digest checks |

MCP config sample: [examples/mcp-config.json](examples/mcp-config.json). Verification matrix: [docs/dod-matrix.md](docs/dod-matrix.md). Provider status: [docs/provider-status.md](docs/provider-status.md).

## Entries — one implementation

Library (`trustquant-runtime`), CLI (`trustquant`), MCP (`trustquant mcp`) and Web (`trustquant serve`, localhost-only) all route through the same Runtime; a conformance suite asserts identical results and identical error envelopes (normalized-text comparison; ADR-0001 §6 documents the serde_json 1-ulp parse caveat).

## Layout

| Path | Role |
|---|---|
| `crates/trustquant-core` | Numerical core: date/day-count, BS analytic engine, portfolio risk/stress, contract types (10-class error taxonomy), canonicalization v1 + BLAKE3, manifest assembly. No I/O, no async |
| `crates/trustquant-runtime` | Product core: tool registry/validation, dataset store (content-addressed), provider routing, manifests, replay, MCP handler, localhost Web |
| `crates/trustquant-sidecar` + `sidecar/` | External Python provider: JSON-Lines-over-stdio protocol v1, pinned env (PyPortfolioOpt 1.6.0 / cvxpy 1.6.7) |
| `crates/trustquant-cli` / `-eval` / `-fixtures` | Single binary; deterministic trace Agent-Eval; fixture consumption |
| `fixture-generator/` | QuantLib C++ generator + pure-Python portfolio oracle (byte-identical regeneration, weekly CI drift check) |
| `skills/` + `evals/` | Three official skills + 11 deterministic eval cases (incl. injection-as-data) |
| `adr/` | ADR-0001..0006 |

## Known limitations (honest list)

- Replay promise is **same version, same environment**; cross-version gets parsing + diff reports only.
- Optimization is a narrow slice: long-only `min_volatility`/`max_sharpe` with weight bounds; bounded solutions are verified by invariants, not analytic weights; no shorting/leverage/transaction costs; "solver succeeded" is never sold as "globally optimal".
- `load_market_dataset` accepts `date,ticker,close` CSVs (ADR-0005 rules); missing values are rejected, not imputed.
- serde_json's text parser shifts certain 17-digit decimals by one ulp **deterministically** (ADR-0001 §6); in-process digests and replay are unaffected.
- The sidecar `die` channel test can be flaky on a loaded 2-core machine during parallel compilation (retry-wrapped; the channel is verified manually and in CI).
- WASM is a M0-M2 differential-validation target, not a shipped interface.

## Lineage

[RELEASE-LINEAGE.md](RELEASE-LINEAGE.md) — QuantLib 1.33 (Ubuntu 24.04 apt, pinned), rustc 1.99.1, python3.12 provider env; v1 archive `vicfei/quantrust@9d40ae9` salvaged with re-acceptance.

## License

Apache-2.0 ([LICENSE](LICENSE)). GPL/AGPL code must not enter this core; third-party code copies are license-reviewed with NOTICE obligations.
