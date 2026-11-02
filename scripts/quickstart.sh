#!/usr/bin/env bash
# Clean-environment quickstart (ADR/v2 M1 gate: README-only, no credentials).
# Usage: scripts/quickstart.sh [--full]
#   --full also exercises the Python-provider optimization demo (requires
#   `trustquant provider install` first — the explicit, only-network step).
set -euo pipefail
cd "$(dirname "$0")/.."

DATA="$(mktemp -d /tmp/trustquant-quickstart-XXXX)"
export TRUSTQUANT_DATA_DIR="$DATA"
trap 'rm -rf "$DATA"' EXIT

echo "== build =="
cargo build -p trustquant-cli -q
BIN=target/debug/trustquant

echo "== demo 1: option pricing -> manifest -> replay =="
"$BIN" run examples/price_call.json >/dev/null
CID=$(ls "$DATA/calculations" | head -1 | sed 's/\.json$//')
"$BIN" replay "$CID" | tail -1

echo "== demo 3 (core part): dataset -> risk -> stress =="
echo '{"schema_version":"0.1","tool":"load_market_dataset","params":{"builtin":"demo_prices"}}' >/tmp/tq-qs-load.json
DS=$("$BIN" run /tmp/tq-qs-load.json | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["dataset_id"])')
echo "{\"schema_version\":\"0.1\",\"tool\":\"analyze_portfolio_risk\",\"params\":{\"dataset\":\"$DS\",\"weights\":{\"SYNTH_A\":0.34,\"SYNTH_B\":0.33,\"SYNTH_C\":0.33}}}" >/tmp/tq-qs-risk.json
"$BIN" run /tmp/tq-qs-risk.json | python3 -c 'import json,sys; r=json.load(sys.stdin)["result"]; v=r["annualized_volatility"]; d=r["max_drawdown"]; print("risk: ann_vol=%.4f max_dd=%.4f" % (v, d))'

if [[ "${1:-}" == "--full" ]]; then
  echo "== demo 2: optimization via external Python provider =="
  echo "{\"schema_version\":\"0.1\",\"tool\":\"optimize_portfolio\",\"params\":{\"dataset\":\"$DS\",\"objective\":\"min_volatility\",\"max_weight\":0.6}}" >/tmp/tq-qs-opt.json
  "$BIN" run /tmp/tq-qs-opt.json | python3 -c 'import json,sys; r=json.load(sys.stdin)["result"]; print("weights:", {k: round(v,4) for k,v in r["weights"].items()})'
else
  echo "== demo 2 skipped (pass --full after 'trustquant provider install') =="
fi

echo "== evals =="
cargo run -q -p trustquant-eval -- run | python3 -c 'import json,sys; r=json.load(sys.stdin); print("eval: %d/%d passed" % (r["passed"], r["total"]))'
echo "QUICKSTART: OK"
