#!/usr/bin/env python3
"""florin external Python Provider sidecar (ADR-0002, protocol v1).

JSON Lines over stdio: stdout carries protocol messages ONLY, stderr carries
logs. M0 scope is the engineering channel prototype: hello handshake,
version/capability reporting, one deterministic calculation (echo_add), plus
deliberate slow/crash behaviors used by the channel tests. No portfolio
optimization logic here yet, and no network access of any kind.
"""

import json
import sys
import time

PROTOCOL = 1
PROVIDER = "python-sidecar"
VERSION = "0.0.1"
CAPABILITIES = ["echo_add"]

BEHAVIOR_NOTES = (
    "diagnostic behaviors: die (process exit), sleep_long (slow response). "
    "These exist to exercise channel semantics, not to model computation."
)


def log(msg: str) -> None:
    print(f"[sidecar] {msg}", file=sys.stderr, flush=True)


def send(msg: dict) -> None:
    sys.stdout.write(json.dumps(msg, sort_keys=True) + "\n")
    sys.stdout.flush()


def handle_request(req: dict) -> dict:
    tool = req.get("tool")
    params = req.get("params", {})
    if tool == "echo_add":
        try:
            a = float(params["a"])
            b = float(params["b"])
        except (KeyError, TypeError, ValueError):
            return {"type": "response", "request_id": req.get("request_id"),
                    "status": "error", "error": {"error_type": "invalid_input",
                    "message": "echo_add requires numeric a and b"}}
        return {"type": "response", "request_id": req.get("request_id"),
                "status": "ok", "result": {"sum": a + b}}
    if tool == "sleep_long":
        time.sleep(float(params.get("ms", 5000)) / 1000.0)
        return {"type": "response", "request_id": req.get("request_id"),
                "status": "ok", "result": {"slept_ms": params.get("ms")}}
    if tool == "die":
        log("die requested: exiting deliberately")
        sys.exit(70)
    return {"type": "response", "request_id": req.get("request_id"),
            "status": "error", "error": {"error_type": "unsupported_feature",
            "message": f"unknown tool {tool!r}"}}


def main() -> int:
    log(f"starting {PROVIDER} v{VERSION} protocol={PROTOCOL}")
    send({"type": "hello", "protocol": PROTOCOL, "provider": PROVIDER,
          "version": VERSION, "capabilities": CAPABILITIES,
          "environment": {"python": sys.version.split()[0]},
          "notes": BEHAVIOR_NOTES})
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
