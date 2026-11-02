#!/usr/bin/env python3
"""TrustQuant external Python Provider sidecar (ADR-0002, protocol v1).

JSON Lines over stdio: stdout carries protocol messages ONLY, stderr carries
logs. M1 capabilities:
  - echo_add                : channel test
  - meta.version            : environment/package version report
  - optimize_portfolio      : narrow PyPortfolioOpt slice
    (objectives: min_volatility | max_sharpe; long-only with weight bounds)

The provider NEVER reads files or the network: prices arrive inside the
request payload (working-directory boundary per ADR-0002). If PyPortfolioOpt
is not installed, the capability is reported as absent and requests for it
return a structured unavailable error — never a silent fallback.
"""

import json
import sys
import time
from importlib import metadata

PROTOCOL = 1
PROVIDER = "python-sidecar"

try:
    import pandas as pd
    from pypfopt import EfficientFrontier, expected_returns, risk_models

    PYPFOPT_AVAILABLE = True
    _IMPORT_ERROR = ""
except Exception as exc:  # pragma: no cover - environment-dependent
    PYPFOPT_AVAILABLE = False
    _IMPORT_ERROR = str(exc)


def solver_backend() -> str:
    try:
        import cvxpy  # noqa: PLC0415

        return f"pypfopt/EfficientFrontier (cvxpy {version('cvxpy')})"
    except Exception:
        return "pypfopt/EfficientFrontier (scipy fallback)"


def version(pkg: str) -> str:
    try:
        return metadata.version(pkg)
    except Exception:
        return "absent"


def capabilities() -> list:
    caps = ["echo_add", "meta.version"]
    if PYPFOPT_AVAILABLE:
        caps.append("portfolio_optimization")
    return caps


def environment() -> dict:
    env = {
        "python": sys.version.split()[0],
        "packages": {
            "numpy": version("numpy"),
            "scipy": version("scipy"),
            "pandas": version("pandas"),
            "pypfopt": version("PyPortfolioOpt"),
        },
    }
    if not PYPFOPT_AVAILABLE:
        env["pypfopt_import_error"] = _IMPORT_ERROR
    return env


def log(msg: str) -> None:
    print(f"[sidecar] {msg}", file=sys.stderr, flush=True)


def send(msg: dict) -> None:
    sys.stdout.write(json.dumps(msg, sort_keys=True) + "\n")
    sys.stdout.flush()


def err(req_id, error_type: str, message: str, **extra):
    body = {"error_type": error_type, "message": message}
    body.update(extra)
    return {"type": "response", "request_id": req_id, "status": "error", "error": body}


def optimize(req_id: str, params: dict) -> dict:
    if not PYPFOPT_AVAILABLE:
        return err(
            req_id,
            "provider_unavailable",
            "portfolio optimization requires PyPortfolioOpt in the provider environment",
            install_hint="trustquant provider install  (explicit; never auto-installed)",
        )
    try:
        tickers = [str(t) for t in params["tickers"]]
        rows = params["prices"]  # {ticker: [close...]} aligned by date index
        objective = params.get("objective", "min_volatility")
        risk_free = float(params.get("risk_free_rate", 0.0))
        max_weight = float(params.get("max_weight", 1.0))
        if not (0.0 < max_weight <= 1.0):
            return err(req_id, "invalid_input", "max_weight must be in (0, 1]", field="max_weight")
        if objective not in ("min_volatility", "max_sharpe"):
            return err(req_id, "invalid_input", "objective must be min_volatility|max_sharpe", field="objective")
        if len(tickers) < 2:
            return err(req_id, "invalid_input", "need at least 2 tickers", field="tickers")
        lengths = {len(rows[t]) for t in tickers}
        if len(lengths) != 1 or next(iter(lengths)) < 3:
            return err(req_id, "invalid_market_data", "price series must share one length >= 3")

        frame = pd.DataFrame({t: [float(x) for x in rows[t]] for t in tickers})
        if (frame.to_numpy() <= 0).any():
            return err(req_id, "invalid_market_data", "prices must be strictly positive")
        mu = expected_returns.mean_historical_return(frame, compounding=False)
        s_cov = risk_models.sample_cov(frame)
        ef = EfficientFrontier(mu, s_cov, weight_bounds=(0.0, max_weight))
        if objective == "min_volatility":
            raw = ef.min_volatility()
        else:
            if not (-0.5 <= risk_free <= 0.5):
                return err(req_id, "invalid_input", "risk_free_rate must be in [-0.5, 0.5]", field="risk_free_rate")
            ef = EfficientFrontier(mu, s_cov, weight_bounds=(0.0, max_weight))
            raw = ef.max_sharpe(risk_free_rate=risk_free)
        weights = {t: float(raw[t]) for t in tickers}
        cleaned = ef.clean_weights()
        perf = ef.portfolio_performance()
        # Recorded values are the raw solver weights; 'clean' (tiny-weight
        # zeroed) variants are display-only and never replayed.
        return {
            "type": "response",
            "request_id": req_id,
            "status": "ok",
            "result": {
                "weights": weights,
                "weights_display": {t: float(cleaned[t]) for t in tickers},
                "objective": objective,
                "expected_return": float(perf[0]),
                "volatility": float(perf[1]),
                "sharpe": float(perf[2]),
                "diagnostics": {
                    "solver": solver_backend(),
                    "mu_estimator": "mean_historical_return(compounding=False): arithmetic mean of daily simple returns, x252",
                    "cov_estimator": "sample_cov (ddof=1)",
                    "long_only": True,
                    "weight_bounds": [0.0, max_weight],
                },
            },
        }
    except KeyError as exc:
        return err(req_id, "invalid_input", f"missing field {exc}", field=str(exc))
    except Exception as exc:  # noqa: BLE001 - channel boundary must not crash
        log(f"optimize failed: {type(exc).__name__}: {exc}")
        return err(req_id, "internal_error", f"optimizer failed: {type(exc).__name__}: {exc}")


def handle_request(req: dict) -> dict:
    tool = req.get("tool")
    params = req.get("params", {})
    req_id = req.get("request_id")
    if tool == "echo_add":
        try:
            a = float(params["a"])
            b = float(params["b"])
        except (KeyError, TypeError, ValueError):
            return err(req_id, "invalid_input", "echo_add requires numeric a and b", field="a")
        return {"type": "response", "request_id": req_id, "status": "ok", "result": {"sum": a + b}}
    if tool == "meta.version":
        return {"type": "response", "request_id": req_id, "status": "ok", "result": environment()}
    if tool == "optimize_portfolio":
        return optimize(req_id, params)
    if tool == "sleep_long":
        time.sleep(float(params.get("ms", 5000)) / 1000.0)
        return {"type": "response", "request_id": req_id, "status": "ok", "result": {"slept_ms": params.get("ms")}}
    if tool == "die":
        log("die requested: exiting deliberately")
        sys.exit(70)
    return err(req_id, "unsupported_feature", f"unknown tool {tool!r}")


def main() -> int:
    log(f"starting {PROVIDER} protocol={PROTOCOL} pypfopt={PYPFOPT_AVAILABLE}")
    send({
        "type": "hello",
        "protocol": PROTOCOL,
        "provider": PROVIDER,
        "version": "0.1.0",
        "capabilities": capabilities(),
        "environment": environment(),
    })
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError as exc:
            log(f"bad json: {exc}")
            continue
        if msg.get("type") == "request":
            send(handle_request(msg))
        elif msg.get("type") == "shutdown":
            log("shutdown requested")
            return 0
        else:
            log(f"ignoring message type {msg.get('type')!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
