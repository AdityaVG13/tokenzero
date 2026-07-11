#!/usr/bin/env python3
"""Measure warm MCP stdio latency and equivalent CLI process latency.

The debug binary must already exist. One MCP server is reused for the complete
run. Raw per-call samples and attribution are written to mcp_latency/<label>.json.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
BIN = REPO / "target/debug/tokenzero"
EVIDENCE = Path(__file__).with_suffix("").with_name("mcp_latency")
GUARD = Path("/tmp/zerostack-heavy-process.guard")
WARMUPS = 10
SAMPLES = 50


def acquire_guard(command: str) -> None:
    try:
        GUARD.mkdir()
    except FileExistsError:
        try:
            pid = int((GUARD / "pid").read_text().strip())
            os.kill(pid, 0)
        except (FileNotFoundError, ValueError, ProcessLookupError):
            for child in GUARD.iterdir():
                if child.is_file():
                    child.unlink()
            GUARD.rmdir()
            GUARD.mkdir()
        except PermissionError as exc:
            raise SystemExit(f"heavy-process guard owner cannot be inspected: {exc}")
        else:
            raise SystemExit(f"heavy-process guard held by live pid {pid}")
    (GUARD / "pid").write_text(f"{os.getpid()}\n")
    (GUARD / "repository").write_text(f"{REPO}\n")
    (GUARD / "command").write_text(f"{command}\n")
    (GUARD / "started_at").write_text(time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()) + "\n")


def release_guard() -> None:
    if not GUARD.exists() or not (GUARD / "pid").exists():
        return
    if (GUARD / "pid").read_text().strip() != str(os.getpid()):
        return
    for child in GUARD.iterdir():
        if child.is_file():
            child.unlink()
    GUARD.rmdir()


def percentile(values: list[float], q: float) -> float:
    ordered = sorted(values)
    index = (len(ordered) - 1) * q
    lo = int(index)
    hi = min(lo + 1, len(ordered) - 1)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (index - lo)


def summary(values: list[float]) -> dict[str, float | int]:
    return {"n": len(values), "p50_ms": round(percentile(values, .50), 6), "p95_ms": round(percentile(values, .95), 6), "mean_ms": round(statistics.fmean(values), 6)}


def write_comparison() -> Path:
    baseline_path = EVIDENCE / "baseline.json"
    candidate_path = EVIDENCE / "candidate.json"
    baseline = json.loads(baseline_path.read_text())
    candidate = json.loads(candidate_path.read_text())
    operations = {}
    losses = []
    components = ("framing_residual_ms", "engine_ms", "persist_ms")
    for name in ("read", "find", "expand"):
        before = baseline["mcp"][name]
        after = candidate["mcp"][name]
        row = {
            "wall_p50_ms": {"baseline": before["wall"]["p50_ms"], "candidate": after["wall"]["p50_ms"]},
            "wall_p95_ms": {"baseline": before["wall"]["p95_ms"], "candidate": after["wall"]["p95_ms"]},
            "attribution_p50_ms": {component: {"baseline": before["attribution"][component]["p50_ms"], "candidate": after["attribution"][component]["p50_ms"]} for component in components},
        }
        for percentile_name in ("wall_p50_ms", "wall_p95_ms"):
            metric = row[percentile_name]
            metric["change_pct"] = round(100 * (metric["candidate"] - metric["baseline"]) / metric["baseline"], 3)
            if metric["candidate"] > metric["baseline"]:
                losses.append(f"{name}.{percentile_name} increased {metric['candidate'] - metric['baseline']:.6f} ms")
        for component, metric in row["attribution_p50_ms"].items():
            if metric["candidate"] > metric["baseline"]:
                losses.append(f"{name}.{component} p50 increased {metric['candidate'] - metric['baseline']:.6f} ms ({metric['baseline']} -> {metric['candidate']} ms)")
        operations[name] = row
    if not any("wall_" in loss for loss in losses):
        losses.append("No MCP wall p50 or p95 regression was observed; component movements are retained rather than cherry-picked.")
    result = {
        "schema": "tokenzero.mcp-latency-comparison.v1",
        "change": "remove per-acquisition durability barriers for Pulse advisory-lock diagnostic metadata; advisory locking and JSONL append remain",
        "proven_hotspot": "Pulse persistence exceeded 20% of baseline MCP p50 for read/find/expand",
        "operations": operations,
        "losses": losses,
        "candidate_mcp_vs_cli_p50_ratio": candidate["mcp_vs_cli_p50_ratio"],
    }
    destination = EVIDENCE / "comparison.json"
    destination.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    return destination


class McpClient:
    def __init__(self, root: Path, cache: Path) -> None:
        self.child = subprocess.Popen(
            [str(BIN), "mcp-server", "--allowed-root", str(root), "--cache-path", str(cache)],
            cwd=REPO, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1,
        )
        self.next_id = 1
        self.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "mcp-latency", "version": "1"}})
        self.notify("notifications/initialized", {})

    def request(self, method: str, params: dict) -> tuple[dict, float]:
        assert self.child.stdin is not None and self.child.stdout is not None
        request_id = self.next_id
        self.next_id += 1
        message = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
        started = time.perf_counter_ns()
        self.child.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
        self.child.stdin.flush()
        line = self.child.stdout.readline()
        elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
        if not line:
            raise RuntimeError(f"MCP server exited unexpectedly: {self.child.poll()}")
        response = json.loads(line)
        if response.get("id") != request_id:
            raise RuntimeError(f"unexpected response id: {response}")
        if "error" in response:
            raise RuntimeError(f"MCP error: {response['error']}")
        return response, elapsed_ms

    def notify(self, method: str, params: dict) -> None:
        assert self.child.stdin is not None
        self.child.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method, "params": params}, separators=(",", ":")) + "\n")
        self.child.stdin.flush()

    def tool(self, name: str, arguments: dict) -> tuple[dict, float]:
        return self.request("tools/call", {"name": name, "arguments": arguments})

    def attribution(self, canonical: str) -> dict[str, float]:
        response, _ = self.request("resources/read", {"uri": "resource://tokenzero/metrics"})
        result = response["result"]
        text = result["contents"][0]["text"]
        metric = json.loads(text)["last_attribution_us"][canonical]
        return {"engine_ms": metric["engine_us"] / 1000, "persist_ms": metric["persist_us"] / 1000}

    def close(self) -> None:
        if self.child.stdin is not None:
            self.child.stdin.close()
        try:
            rc = self.child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.child.kill()
            rc = self.child.wait(timeout=5)
        if rc != 0:
            raise RuntimeError(f"MCP server exited with {rc}")


def find_ref(value: object) -> str | None:
    if isinstance(value, str):
        match = re.search(r"(?:tz|fz)://[^\s]+", value)
        if match:
            return match.group(0)
    if isinstance(value, dict):
        for item in value.values():
            found = find_ref(item)
            if found:
                return found
    if isinstance(value, list):
        for item in value:
            found = find_ref(item)
            if found:
                return found
    return None


def cli_sample(command: list[str]) -> float:
    started = time.perf_counter_ns()
    proc = subprocess.run(command, cwd=REPO, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, timeout=30)
    elapsed = (time.perf_counter_ns() - started) / 1_000_000
    if proc.returncode != 0:
        raise RuntimeError(f"CLI failed ({proc.returncode}): {command!r}\n{proc.stderr[-2000:]}")
    return elapsed


def run(label: str) -> Path:
    if not BIN.is_file():
        raise SystemExit("target/debug/tokenzero missing; build it once before running this harness")
    acquire_guard(f"mcp_latency.py --label {label}")
    client = None
    try:
        with tempfile.TemporaryDirectory(prefix="tokenzero-mcp-latency-") as raw_tmp:
            root = Path(raw_tmp)
            corpus = root / "corpus.txt"
            corpus.write_text("needle TokenZero latency corpus\n" * 256)
            cache = root / "cache.json"
            client = McpClient(root, cache)
            read_args = {"path": str(corpus), "fresh": True, "raw": True}
            read_response, _ = client.tool("tz_read", read_args)
            ref = find_ref(read_response)
            if ref is None:
                raise RuntimeError(f"read response contained no ref: {read_response}")
            operations = {
                "read": ("tz_read", read_args),
                "find": ("tz_find", {"query": "needle", "path": str(corpus), "fresh": True}),
                "expand": ("tz_expand", {"ref": ref}),
            }
            mcp = {}
            for canonical, (tool, arguments) in operations.items():
                for _ in range(WARMUPS):
                    client.tool(tool, arguments)
                rows = []
                for _ in range(SAMPLES):
                    _, wall_ms = client.tool(tool, arguments)
                    split = client.attribution(canonical)
                    split["wall_ms"] = wall_ms
                    split["framing_residual_ms"] = max(0.0, wall_ms - split["engine_ms"] - split["persist_ms"])
                    rows.append({key: round(value, 6) for key, value in split.items()})
                mcp[canonical] = {
                    "wall": summary([row["wall_ms"] for row in rows]),
                    "attribution": {key: summary([row[key] for row in rows]) for key in ("framing_residual_ms", "engine_ms", "persist_ms")},
                    "samples": rows,
                }
            client.close()
            client = None

            noop = [cli_sample([str(BIN), "--version"]) for _ in range(SAMPLES)]
            noop_p50 = percentile(noop, .50)
            cli_commands = {
                "read": [str(BIN), "read", str(corpus), "--allowed-root", str(root), "--cache-path", str(cache), "--raw"],
                "find": [str(BIN), "find", "needle", str(corpus), "--allowed-root", str(root), "--cache-path", str(cache), "--json"],
                "expand": [str(BIN), "expand", ref, "--cache-path", str(cache), "--raw"],
            }
            cli = {}
            ratios = {}
            for canonical, command in cli_commands.items():
                raw = [cli_sample(command) for _ in range(SAMPLES)]
                adjusted = [max(0.0, sample - noop_p50) for sample in raw]
                cli[canonical] = {"raw": summary(raw), "start_subtracted": summary(adjusted), "samples_ms": [round(x, 6) for x in raw]}
                mcp_p50 = mcp[canonical]["wall"]["p50_ms"]
                raw_p50 = cli[canonical]["raw"]["p50_ms"]
                adjusted_p50 = cli[canonical]["start_subtracted"]["p50_ms"]
                ratios[canonical] = {
                    "mcp_over_cli_raw": round(mcp_p50 / raw_p50, 6),
                    "mcp_over_cli_start_subtracted": None if adjusted_p50 == 0 else round(mcp_p50 / adjusted_p50, 6),
                }

            result = {
                "schema": "tokenzero.mcp-latency.v1", "label": label,
                "environment": {
                    "os": platform.platform(), "machine": platform.machine(), "python": platform.python_version(),
                    "commit": subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True, text=True, check=True).stdout.strip(),
                    "binary": "target/debug/tokenzero", "binary_mtime_ns": BIN.stat().st_mtime_ns, "profile": "debug",
                    "source_diff_sha256": hashlib.sha256(subprocess.run(["git", "diff", "--binary"], cwd=REPO, capture_output=True, check=True).stdout).hexdigest(),
                    "profile_reason": "relative request-path attribution; matches existing repository performance harness convention and avoids an unnecessary release build",
                    "warmups_per_op": WARMUPS, "samples_per_op": SAMPLES,
                },
                "commands": {
                    "mcp": "one tokenzero mcp-server process; NDJSON JSON-RPC; initialize once; serial tools/call",
                    "cli": {key: " ".join(command) for key, command in cli_commands.items()},
                    "cli_start_proxy": "tokenzero --version",
                },
                "methodology": {
                    "framing_residual": "client wall minus server dispatch_tool engine timer minus ToolMetrics persistence timer; includes JSON encode/decode, queues, response formatting, pipe scheduling, and client overhead",
                    "cli_start_subtracted": "per-call CLI wall minus the p50 of 50 tokenzero --version calls, floored at zero",
                    "ratio_direction": "MCP p50 divided by CLI p50; values below 1 mean MCP is faster",
                },
                "mcp": mcp, "cli_start_proxy": {"summary": summary(noop), "samples_ms": [round(x, 6) for x in noop]},
                "cli": cli, "mcp_vs_cli_p50_ratio": ratios,
            }
            EVIDENCE.mkdir(parents=True, exist_ok=True)
            destination = EVIDENCE / f"{label}.json"
            destination.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
            if label == "candidate" and (EVIDENCE / "baseline.json").is_file():
                write_comparison()
            return destination
    finally:
        if client is not None:
            try:
                client.close()
            except Exception:
                pass
        release_guard()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=("baseline", "candidate"), required=True)
    args = parser.parse_args()
    print(run(args.label).relative_to(REPO))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
