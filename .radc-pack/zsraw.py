#!/usr/bin/env python3
"""zsraw -- like zs but prints the full structuredContent.value (uncompressed)."""
import json
import os
import subprocess
import sys

ORIGIN_MAIN = os.path.expanduser("~/.pi/agent/zerostack-origin-main")
ENGINES = {
    "fs": (f"{ORIGIN_MAIN}/fszero/target/release/fszero-codemode", "fz_execute_code"),
    "graph": (f"{ORIGIN_MAIN}/graphzero/target/release/graphzero-codemode", "gz_execute_code"),
    "token": (f"{ORIGIN_MAIN}/TokenZero/target/release/tokenzero-codemode", "tz_execute_code"),
}
INIT = {"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "zs-cli", "version": "1.0.0"}}}
INITIALIZED = {"jsonrpc": "2.0", "method": "notifications/initialized"}


def main() -> None:
    engine_name = sys.argv[1]
    plan = sys.argv[2] if len(sys.argv) > 2 else sys.stdin.read()
    binary, method = ENGINES[engine_name]
    call = {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": method, "arguments": {"plan": plan, "form": "js"}}}
    payload = "\n".join(json.dumps(m) for m in (INIT, INITIALIZED, call)) + "\n"
    proc = subprocess.run([binary], input=payload, capture_output=True, text=True,
                          timeout=int(os.environ.get("ZS_TIMEOUT_MS", "120000")) / 1000)
    for line in proc.stdout.splitlines():
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("id") != 2:
            continue
        if "error" in msg:
            print(json.dumps(msg["error"]), file=sys.stderr)
            sys.exit(1)
        sc = msg["result"].get("structuredContent") or {}
        value = sc.get("value", sc)
        if isinstance(value, dict) and "result" in value:
            value = value["result"]
        if isinstance(value, str):
            print(value)
        else:
            print(json.dumps(value, indent=2))
        if msg["result"].get("isError"):
            sys.exit(1)
        return
    print("zsraw: no response", file=sys.stderr)
    sys.exit(1)


if __name__ == "__main__":
    main()
