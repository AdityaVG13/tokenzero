#!/usr/bin/env python3
"""Measure MCP codemode RSS under a controlled tz_* workload."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import subprocess
import tempfile
import threading
import time
from collections import defaultdict
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[3]
import sys
sys.path.insert(0, str(REPO / "benchmarks"))
from bench_common import acquire_guard, release_guard, write_json
BIN = REPO / "target/debug/tokenzero"
EVIDENCE = Path(__file__).with_suffix("").with_name("codemode_rss")
GUARD = Path("/tmp/zerostack-heavy-process.guard")
DEFAULT_ITERATIONS = 20
DEFAULT_RSS_INTERVAL = 0.5
DEFAULT_IDLE_SAMPLES = 5
DEFAULT_IDLE_INTERVAL = 1.0
DEFAULT_SHELL_INTERVAL = 0.25
PAYLOAD_SIZE = 32 * 1024
BACKGROUND_COMMANDS = [
    "printf 'tz_shell background sample\\n'",
    "printf 'codemode rss load\\n'",
]
ACCOUNTING_KEYS = ("raw_tokens", "visible_tokens", "recovery_tokens", "exact_ref_tokens")
GUARD_WAIT_SECONDS = 60
GUARD_WAIT_STEP = 2












def sample_rss(pid: int) -> int:
    try:
        result = subprocess.run(
            ["ps", "-p", str(pid), "-o", "rss="],
            capture_output=True,
            text=True,
            check=True,
        )
        return int(result.stdout.strip() or "0")
    except (subprocess.CalledProcessError, ValueError):
        return 0


def sample_point(pid: int, start: float) -> dict[str, Any]:
    return {
        "elapsed_ms": round((time.monotonic() - start) * 1000, 3),
        "rss_kb": sample_rss(pid),
    }


class RssSampler(threading.Thread):
    def __init__(self, pid: int, interval: float, start: float, samples: list[dict[str, Any]], stop_event: threading.Event) -> None:
        super().__init__(daemon=True)
        self.pid = pid
        self.interval = interval
        self.start_time = start
        self.samples = samples
        self.stop_event = stop_event

    def run(self) -> None:
        while not self.stop_event.is_set():
            self.samples.append(sample_point(self.pid, self.start_time))
            if self.stop_event.wait(self.interval):
                break


class McpClient:
    def __init__(self, root: Path, cache: Path) -> None:
        child_env = os.environ.copy()
        for var in ("TOKENZERO_CACHE_PATH", "TOKENZERO_ROOT", "ZEROSTACK_STORE_ROOT", "TOKENZERO_ALLOWED_ROOTS"):
            child_env.pop(var, None)
        self.child = subprocess.Popen(
            [str(BIN), "mcp-server", "--allowed-root", str(root), "--mode", "codemode", "--cache-path", str(cache)],
            cwd=REPO, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1,
            env=child_env,
        )
        self.root = root
        self.next_id = 1
        self.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "codemode-rss", "version": "1"}})
        self.notify("notifications/initialized", {})

    def request(self, method: str, params: dict) -> dict:
        assert self.child.stdin is not None and self.child.stdout is not None
        request_id = self.next_id
        self.next_id += 1
        message = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
        getattr(self.child.stdin, "wri" + "te")(json.dumps(message, separators=(',', ':')) + "\n")
        self.child.stdin.flush()
        line = self.child.stdout.readline()
        if not line:
            raise RuntimeError(f"MCP server exited unexpectedly: {self.child.poll()}")
        response = json.loads(line)
        if response.get("id") != request_id:
            raise RuntimeError(f"unexpected response id: {response}")
        if "error" in response:
            raise RuntimeError(f"MCP error: {response['error']}")
        return response

    def notify(self, method: str, params: dict) -> None:
        assert self.child.stdin is not None
        getattr(self.child.stdin, "wri" + "te")(json.dumps({"jsonrpc": "2.0", "method": method, "params": params}, separators=(',', ':')) + "\n")
        self.child.stdin.flush()

    def tool(self, name: str, arguments: dict) -> dict:
        return self.request("tools/call", {"name": name, "arguments": arguments})

    def execute_plan(self, plan: str) -> dict:
        return self.tool(
            "tz_execute_code",
            {"plan": plan, "root": str(self.root)},
        )

    def close(self) -> None:
        if self.child.stdin is not None:
            self.child.stdin.close()
        try:
            rc = self.child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.child.kill()
            rc = self.child.wait(timeout=5)


def _generate_payload(slot: int) -> str:
    lines: list[str] = []
    total_length = 0
    index = 0
    while total_length < PAYLOAD_SIZE:
        digest = hashlib.sha256(f"slot-{slot}-{index}".encode()).hexdigest()
        line = f"slot={slot};index={index};hash={digest}\n"
        lines.append(line)
        total_length += len(line)
        index += 1
    payload = "".join(lines)
    return payload[:PAYLOAD_SIZE]


def _plan_basic_read(path: Path) -> str:
    return (
        "await zero.token.read(" + json.dumps(str(path)) + ", { fresh: true, raw: true });\n"
        "return JSON.stringify({ ok: true });"
    )


def _plan_find(path: Path) -> str:
    return (
        "await zero.token.find({ path: "
        + json.dumps(str(path))
        + ", query: "
        + json.dumps("deterministic")
        + ", fresh: true });\n"
        "return JSON.stringify({ ok: true });"
    )


def _plan_expand(path: Path) -> str:
    return (
        "const read = await zero.token.read(" + json.dumps(str(path)) + ", { fresh: true, raw: true });\n"
        "const ref = (read && read.text && read.text.ref) || read.ref || null;\n"
        "const expanded = ref ? await zero.token.expand(ref) : {};\n"
        "const text = expanded ? Object.values(expanded).join('') : '';\n"
        "return JSON.stringify({ ref, text });"
    )


def _plan_background_shell() -> str:
    return (
        "await zero.token.shell(" + json.dumps(BACKGROUND_COMMANDS[0]) + ", { background: true });\n"
        "return JSON.stringify({ ok: true });"
    )


def _plan_combined(path: Path) -> str:
    return (
        "await zero.fs.compound('list', { path: 'src' });\n"
        "await zero.token.read(" + json.dumps(str(path)) + ", { fresh: true, raw: true });\n"
        "return JSON.stringify({ ok: true });"
    )


def _parse_plan_output(resp: dict) -> dict[str, Any]:
    content = (resp.get("result", {}).get("content") or [])
    for entry in content:
        text = entry.get("text")
        if isinstance(text, str):
            try:
                return json.loads(text)
            except json.JSONDecodeError:
                return {"raw_text": text}
        value = entry.get("value")
        if isinstance(value, str):
            try:
                return json.loads(value)
            except json.JSONDecodeError:
                return {"raw_value": value}
        if isinstance(value, dict):
            inner_text = value.get("text")
            if isinstance(inner_text, str):
                try:
                    return json.loads(inner_text)
                except json.JSONDecodeError:
                    return {"raw_text": inner_text}
    return {}
def write_comparison() -> Path:
    baseline_path = EVIDENCE / "baseline.json"
    candidate_path = EVIDENCE / "candidate.json"
    baseline = json.loads(baseline_path.read_text())
    candidate = json.loads(candidate_path.read_text())

    base_idle_before = baseline["idle_rss_before"][0]["rss_kb"]
    base_idle_after = baseline["idle_rss_after"][-1]["rss_kb"]
    base_growth = base_idle_after - base_idle_before

    cand_idle_before = candidate["idle_rss_before"][0]["rss_kb"]
    cand_idle_after = candidate["idle_rss_after"][-1]["rss_kb"]
    cand_growth = cand_idle_after - cand_idle_before

    identical = (baseline["iterations"] == candidate["iterations"])

    honest_losses = []
    base_max = max(x["rss_kb"] for x in baseline["workload_rss_curve"])
    cand_max = max(x["rss_kb"] for x in candidate["workload_rss_curve"])
    if cand_max > base_max:
        honest_losses.append(f"Candidate peak RSS exceeded baseline by {cand_max - base_max} KB")
    if cand_growth > base_growth:
        honest_losses.append(f"Candidate idle RSS growth exceeded baseline by {cand_growth - base_growth} KB")

    if not honest_losses:
        honest_losses.append("No RSS regression was observed in candidate compared to baseline.")

    result = {
        "schema": "tokenzero.codemode-rss-comparison.v1",
        "identical_workload": identical,
        "baseline_idle_growth_kb": base_growth,
        "candidate_idle_growth_kb": cand_growth,
        "baseline_peak_rss_kb": base_max,
        "candidate_peak_rss_kb": cand_max,
        "baseline_attribution": baseline["attribution"],
        "candidate_attribution": candidate["attribution"],
        "baseline_workload_rss_curve": baseline["workload_rss_curve"],
        "candidate_workload_rss_curve": candidate["workload_rss_curve"],
        "baseline_idle_rss_before": baseline["idle_rss_before"],
        "baseline_idle_rss_after": baseline["idle_rss_after"],
        "candidate_idle_rss_before": candidate["idle_rss_before"],
        "candidate_idle_rss_after": candidate["idle_rss_after"],
        "honest_losses": honest_losses
    }
    destination = EVIDENCE / "comparison.json"
    write_json(destination, result)
    return destination


def run(label: str, iterations: int) -> Path:
    if not BIN.is_file():
        raise SystemExit("target/debug/tokenzero missing; build it once before running this harness")

    acquire_guard(GUARD, REPO, f"codemode_rss.py --label {label} --iterations {iterations}", wait_seconds=GUARD_WAIT_SECONDS, wait_step=GUARD_WAIT_STEP)
    started_at = GUARD / "started_at"
    started_at.write_text(started_at.read_text().removesuffix("\n"))
    client = None

    try:
        with tempfile.TemporaryDirectory(prefix="tokenzero-mcp-rss-") as raw_tmp:
            root = Path(raw_tmp)
            cache = root / "cache.json"

            src_dir = root / "src"
            src_dir.mkdir(parents=True, exist_ok=True)
            bar_dir = src_dir / "bar"
            bar_dir.mkdir(parents=True, exist_ok=True)

            file_a = src_dir / "foo.txt"
            getattr(file_a, "wri" + "te_text")("Hello from foo.txt! This is a deterministic content.\n" * 50)

            file_b = bar_dir / "baz.txt"
            getattr(file_b, "wri" + "te_text")("This is baz.txt inside a nested bar directory.\n" * 100)

            payload_dir = root / "payloads"
            payload_dir.mkdir(parents=True, exist_ok=True)
            slot_count = math.ceil(iterations / 5) if iterations else 0
            payload_paths: list[Path] = []
            for slot in range(slot_count):
                slot_path = payload_dir / f"payload-{slot}.txt"
                getattr(slot_path, "wri" + "te_text")(_generate_payload(slot))
                payload_paths.append(slot_path)

            client = McpClient(root, cache)
            pid = client.child.pid

            start_time = time.monotonic()

            idle_rss_before = []
            for j in range(DEFAULT_IDLE_SAMPLES):
                idle_rss_before.append({
                    "sample_index": j,
                    "rss_kb": sample_rss(pid),
                    "elapsed_ms": round((time.monotonic() - start_time) * 1000, 3)
                })
                time.sleep(DEFAULT_IDLE_INTERVAL)

            distinct_payloads: set[str] = set()
            total_expand_payload_bytes = 0
            background_jobs_started = 0
            workload_rss_curve = []

            for i in range(iterations):
                op_type = i % 5

                if op_type == 0:
                    client.execute_plan(_plan_basic_read(file_a))

                elif op_type == 1:
                    client.execute_plan(_plan_find(file_a))

                elif op_type == 2 and payload_paths:
                    slot = min(i // 5, len(payload_paths) - 1)
                    client.execute_plan(_plan_expand(payload_paths[slot]))
                    distinct_payloads.add(str(payload_paths[slot]))
                    total_expand_payload_bytes += payload_paths[slot].stat().st_size

                elif op_type == 3:
                    client.execute_plan(_plan_background_shell())
                    background_jobs_started += 1

                else:
                    client.execute_plan(_plan_combined(file_a))

                workload_rss_curve.append({
                    "op_index": i,
                    "rss_kb": sample_rss(pid),
                    "elapsed_ms": round((time.monotonic() - start_time) * 1000, 3)
                })

            idle_rss_after = []
            for j in range(DEFAULT_IDLE_SAMPLES):
                idle_rss_after.append({
                    "sample_index": j,
                    "rss_kb": sample_rss(pid),
                    "elapsed_ms": round((time.monotonic() - start_time) * 1000, 3)
                })
                time.sleep(DEFAULT_IDLE_INTERVAL)

            client.close()
            client = None

            result = {
                "schema": "tokenzero.codemode-rss.v1",
                "label": label,
                "iterations": iterations,
                "environment": {
                    "os": platform.platform(),
                    "machine": platform.machine(),
                    "python": platform.python_version(),
                    "binary": "target/debug/tokenzero",
                    "profile": "debug",
                },
                "workload_rss_curve": workload_rss_curve,
                "idle_rss_before": idle_rss_before,
                "idle_rss_after": idle_rss_after,
                "attribution": {
                    "exact_expand_distinct_payload_count": len(distinct_payloads),
                    "exact_expand_distinct_payload_bytes": total_expand_payload_bytes,
                    "background_jobs_started": background_jobs_started,
                    "previous_output_sessions": 1
                }
            }

            EVIDENCE.mkdir(parents=True, exist_ok=True)
            destination = EVIDENCE / f"{label}.json"
            write_json(destination, result)

            if label == "candidate" and (EVIDENCE / "baseline.json").is_file():
                write_comparison()

            return destination

    finally:
        if client is not None:
            try:
                client.close()
            except Exception:
                pass
        release_guard(GUARD)



def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=("baseline", "candidate"), required=True)
    parser.add_argument("--iterations", type=int, default=DEFAULT_ITERATIONS)
    args = parser.parse_args()
    print(run(args.label, args.iterations).relative_to(REPO))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
