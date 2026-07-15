#!/usr/bin/env python3
"""Measure MCP ingest and expand latency across deterministic payload sizes.

Uses an existing debug binary only. The server is initialized once, each payload
is ingested with tz_read, then its returned ref is expanded repeatedly. Evidence
includes raw samples, percentiles, and server-metrics attribution.
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
from typing import Any

REPO = Path(__file__).resolve().parents[3]
import sys
sys.path.insert(0, str(REPO / "benchmarks"))
from bench_common import find_ref as _find_ref, percentile, summary as _summary
summary = lambda values: _summary(values, include_p99=True)
find_ref = lambda value: _find_ref(value, pattern=REF_RE.pattern)
BIN = Path(os.environ.get("TOKENZERO_EXPAND_BENCH_BIN", REPO / "target/debug/tokenzero"))
EVIDENCE = Path(__file__).with_suffix("") / "evidence.json"
SIZE_CLASSES = (
    ("1KB", 1 * 1024, 50),
    ("100KB", 100 * 1024, 50),
    ("1MB", 1 * 1024 * 1024, 30),
    ("10MB", 10 * 1024 * 1024, 30),
    ("100MB", 100 * 1024 * 1024, 3),
)
REF_RE = re.compile(r"(?:tz|fz)://[A-Za-z0-9._/-]+")








def write_payload(path: Path, size: int, label: str) -> str:
    line = (hashlib.sha256(("tokenzero-expand-latency:" + label).encode()).hexdigest() + "\n").encode()
    digest = hashlib.sha256()
    remaining = size
    with path.open("wb") as handle:
        while remaining:
            piece = line[: min(len(line), remaining)]
            handle.write(piece)
            digest.update(piece)
            remaining -= len(piece)
    return digest.hexdigest()


class McpClient:
    def __init__(self, root: Path, cache: Path) -> None:
        child_env = os.environ.copy()
        for name in ("TOKENZERO_CACHE_PATH", "TOKENZERO_ROOT", "ZEROSTACK_STORE_ROOT", "TOKENZERO_ALLOWED_ROOTS"):
            child_env.pop(name, None)
        self.child = subprocess.Popen(
            [str(BIN), "mcp-server", "--allowed-root", str(root), "--cache-path", str(cache)],
            cwd=REPO,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1,
            env=child_env,
        )
        self.next_id = 1
        self.peak_rss_bytes = 0
        self.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "expand-latency", "version": "1"}})
        self.notify("notifications/initialized", {})

    def request(self, method: str, params: dict[str, Any]) -> tuple[dict[str, Any], float, int]:
        assert self.child.stdin is not None and self.child.stdout is not None
        request_id = self.next_id
        self.next_id += 1
        message = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
        started = time.perf_counter_ns()
        self.child.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
        self.child.stdin.flush()
        line = self.child.stdout.readline()
        wall_ms = (time.perf_counter_ns() - started) / 1_000_000
        self.sample_rss()
        if not line:
            raise RuntimeError(f"MCP server exited unexpectedly: {self.child.poll()}")
        response = json.loads(line)
        if response.get("id") != request_id:
            raise RuntimeError(f"unexpected response id: {response}")
        if "error" in response:
            raise RuntimeError(f"MCP error: {response['error']}")
        return response, wall_ms, len(line.encode())

    def sample_rss(self) -> None:
        measured = subprocess.run(
            ["ps", "-o", "rss=", "-p", str(self.child.pid)],
            capture_output=True,
            text=True,
            check=False,
        )
        if measured.returncode == 0 and measured.stdout.strip().isdigit():
            self.peak_rss_bytes = max(self.peak_rss_bytes, int(measured.stdout.strip()) * 1024)

    def notify(self, method: str, params: dict[str, Any]) -> None:
        assert self.child.stdin is not None
        self.child.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method, "params": params}, separators=(",", ":")) + "\n")
        self.child.stdin.flush()

    def tool(self, name: str, arguments: dict[str, Any]) -> tuple[dict[str, Any], float, int]:
        return self.request("tools/call", {"name": name, "arguments": arguments})

    def attribution(self, canonical: str) -> dict[str, float]:
        response, _, _ = self.request("resources/read", {"uri": "resource://tokenzero/metrics"})
        text = response["result"]["contents"][0]["text"]
        metric = json.loads(text)["last_attribution_us"][canonical]
        return {"materialization_ms": metric["engine_us"] / 1000, "persist_ms": metric["persist_us"] / 1000}

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


def attributed_row(client: McpClient, tool: str, canonical: str, arguments: dict[str, Any]) -> tuple[dict[str, Any], dict[str, float | int]]:
    response, wall_ms, wire_bytes = client.tool(tool, arguments)
    split = client.attribution(canonical)
    split["wall_ms"] = wall_ms
    split["framing_ms"] = max(0.0, wall_ms - split["materialization_ms"] - split["persist_ms"])
    row: dict[str, float | int] = {key: round(value, 6) for key, value in split.items()}
    row["response_wire_bytes"] = wire_bytes
    return response, row


def operation_summary(rows: list[dict[str, float | int]]) -> dict[str, Any]:
    keys = ("wall_ms", "materialization_ms", "framing_ms", "persist_ms")
    return {
        "wall": summary([float(row["wall_ms"]) for row in rows]),
        "attribution": {key: summary([float(row[key]) for row in rows]) for key in keys[1:]},
        "attribution_p50_pct_of_wall": {
            key: round(100 * percentile([float(row[key]) for row in rows], 0.50) / percentile([float(row["wall_ms"]) for row in rows], 0.50), 3)
            for key in keys[1:]
        },
        "samples": rows,
    }


def run(
    size_classes: tuple[tuple[str, int, int], ...] = SIZE_CLASSES,
    merge_existing: bool = False,
    output_path: Path = EVIDENCE,
) -> Path:
    if not BIN.is_file():
        raise SystemExit("target/debug/tokenzero missing; refusing to build under this measurement harness")
    if not os.access(BIN, os.X_OK):
        raise SystemExit("target/debug/tokenzero is not executable")

    started_utc = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    results: list[dict[str, Any]] = []
    client: McpClient | None = None
    with tempfile.TemporaryDirectory(prefix="tokenzero-expand-latency-") as raw_tmp:
        root = Path(raw_tmp)
        cache = root / "cache.json"
        client = McpClient(root, cache)
        try:
            for label, size_bytes, samples in size_classes:
                payload_path = root / f"payload-{label}.txt"
                payload_sha256 = write_payload(payload_path, size_bytes, label)
                ingest_response, ingest_row = attributed_row(client, "tz_read", "read", {"path": str(payload_path), "fresh": True, "raw": True})
                ref = find_ref(ingest_response)
                if ref is None:
                    raise RuntimeError(f"{label} ingest response contained no ref")

                # One unmeasured expansion faults any lazy store state in before sampling.
                client.tool("tz_expand", {"ref": ref})
                expand_rows: list[dict[str, float | int]] = []
                client.peak_rss_bytes = 0
                for _ in range(samples):
                    _, row = attributed_row(client, "tz_expand", "expand", {"ref": ref})
                    if int(row["response_wire_bytes"]) < size_bytes:
                        raise RuntimeError(f"{label} expand response shorter than payload: {row['response_wire_bytes']} < {size_bytes}")
                    expand_rows.append(row)

                results.append({
                    "size_class": label,
                    "payload_bytes": size_bytes,
                    "payload_sha256": payload_sha256,
                    "ref": ref,
                    "samples_n": samples,
                    "ingest_to_ref": operation_summary([ingest_row]),
                    "expand_round_trip": operation_summary(expand_rows),
                    "server_peak_rss_bytes": client.peak_rss_bytes,
                })
        finally:
            client.close()

    if merge_existing and output_path.is_file():
        previous = json.loads(output_path.read_text()).get("results", [])
        measured_labels = {item["size_class"] for item in results}
        results = [item for item in previous if item["size_class"] not in measured_labels] + results
        order = {label: index for index, (label, _, _) in enumerate(SIZE_CLASSES)}
        results.sort(key=lambda item: order[item["size_class"]])

    table = []
    crossover: str | None = None
    for item in results:
        expand = item["expand_round_trip"]
        wall = expand["wall"]
        attribution = expand["attribution"]
        p50_wall = float(wall["p50_ms"])
        p50_materialization = float(attribution["materialization_ms"]["p50_ms"])
        p50_framing = float(attribution["framing_ms"]["p50_ms"])
        p50_persist = float(attribution["persist_ms"]["p50_ms"])
        dominant = max(
            (("materialization", p50_materialization), ("framing", p50_framing), ("persist", p50_persist)),
            key=lambda pair: pair[1],
        )[0]
        if crossover is None and dominant == "materialization" and p50_materialization > p50_wall / 2:
            crossover = item["size_class"]
        table.append({
            "size_class": item["size_class"],
            "n": item["samples_n"],
            "expand_p50_ms": wall["p50_ms"],
            "expand_p95_ms": wall["p95_ms"],
            "expand_p99_ms": wall["p99_ms"],
            "materialization_p50_ms": round(p50_materialization, 6),
            "framing_p50_ms": round(p50_framing, 6),
            "persist_p50_ms": round(p50_persist, 6),
            "dominant_p50_component": dominant,
        })

    conclusion = (
        f"Materialization starts to dominate at the {crossover} size class (p50 engine time is both the largest component and more than 50% of p50 wall time)."
        if crossover else
        "Materialization did not exceed both other components and 50% of p50 wall time in any measured size class."
    )
    commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True, text=True, check=True).stdout.strip()
    evidence = {
        "schema": "tokenzero.expand-latency.v1",
        "environment": {
            "started_utc": started_utc,
            "os": platform.platform(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "commit": commit,
            "binary_path": str(BIN),
            "binary_mtime_ns": BIN.stat().st_mtime_ns,
            "binary_sha256": hashlib.sha256(BIN.read_bytes()).hexdigest(),
            "samples_by_size": {item["size_class"]: item["samples_n"] for item in results},
        },
        "methodology": {
            "transport": "one warm tokenzero mcp-server process per invocation over newline-delimited stdio JSON-RPC; serial tools/call requests; --only runs accumulate size classes in the evidence file",
            "ingest": "tz_read(path, fresh=true, raw=true), then recursively extract returned tz:// or fz:// ref",
            "expand": "one unmeasured warm-up followed by N tz_expand(ref) round trips; wall timer surrounds request write, flush, response readline, before JSON parsing",
            "attribution": "resource://tokenzero/metrics last_attribution_us after every operation; engine_us is labeled materialization, persist_us is persistence, framing is non-negative client wall residual",
            "framing_scope": "JSON serialization, response formatting, pipe scheduling, response bytes, and client overhead; JSON parsing occurs after the wall timer",
            "percentile": "linear interpolation at (n-1)*q",
            "payload": "deterministic repeated SHA-256 hex line, exact byte length; temporary directory removed automatically",
            "rss": "server RSS sampled with ps immediately after each expand response; peak is the maximum observed sample for the size class and may understate a transient serialization peak",
            "limitations": "engine_us is the existing server engine timer and may include store lookup in addition to byte materialization; RSS is sampled rather than an OS high-water mark; small-n p95/p99 are descriptive interpolations",
        },
        "results": results,
        "table": table,
        "crossover_size_class": crossover,
        "conclusion": conclusion,
        "files_changed": [
            "crates/tokenzero-mcp/benches/expand_latency.py",
            "crates/tokenzero-mcp/benches/expand_latency/evidence.json",
        ],
        "close_note_draft": "Measured ingest-to-ref and warm expand round-trip latency at 1KB, 100KB, 1MB, 10MB, and 100MB using the existing debug binary and MCP per-op telemetry. See table and conclusion in crates/tokenzero-mcp/benches/expand_latency/evidence.json; no product code or cargo invocation.",
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(evidence, indent=2, sort_keys=True) + '\n')
    print(output_path)
    return output_path

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--only", choices=[label for label, _, _ in SIZE_CLASSES])
    parser.add_argument("--samples", type=int, help="override samples for the selected size class")
    parser.add_argument("--output", type=Path, default=EVIDENCE)
    args = parser.parse_args()
    selected = SIZE_CLASSES if args.only is None else tuple(row for row in SIZE_CLASSES if row[0] == args.only)
    if args.samples is not None:
        if args.only is None or args.samples < 1:
            parser.error("--samples requires --only and a positive count")
        selected = tuple((label, size, args.samples) for label, size, _ in selected)
    run(selected, merge_existing=args.only is not None, output_path=args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
