#!/usr/bin/env python3
"""Matched A/B performance gate for two already-built TokenZero binaries."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import resource
import selectors
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, Callable

SCHEMA = "tokenzero.matched-ab.v1"
TIMEOUT_SECONDS = 60
MCP_PROTOCOL_VERSION = "2024-11-05"
ECHO_SENTINEL = "tokenzero-ab-echo"
STORE_ENV_VARS = (
    "TOKENZERO_CACHE_PATH",
    "TOKENZERO_SHARED_STORE",
    "ZEROSTACK_SHARED_STORE",
    "ZEROSTACK_STORE_ROOT",
    "ZERO_STACK_STORE_ROOT",
    "TOKENZERO_ROOT",
)


class HarnessError(RuntimeError):
    pass


def nearest_rank(values: list[float], percentile: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(percentile * len(ordered)) - 1)]


def summary(values: list[float]) -> dict[str, float | int]:
    if not values:
        raise HarnessError("cannot summarize an empty sample set")
    return {
        "n": len(values),
        "p50": round(nearest_rank(values, 0.50), 6),
        "p95": round(nearest_rank(values, 0.95), 6),
        "min": round(min(values), 6),
        "max": round(max(values), 6),
    }


def percent_change(baseline: float, candidate: float) -> float | None:
    if baseline == 0:
        return 0.0 if candidate == 0 else None
    return round(100.0 * (candidate - baseline) / baseline, 6)


def describe_metric(baseline: list[float], candidate: list[float]) -> dict[str, Any]:
    before = summary(baseline)
    after = summary(candidate)
    return {
        "baseline": before,
        "candidate": after,
        "change_pct": {
            key: percent_change(float(before[key]), float(after[key]))
            for key in ("p50", "p95", "min", "max")
        },
    }


def isolated_env(home: Path, cache: Path, ref_index: Path) -> dict[str, str]:
    env = os.environ.copy()
    for name in STORE_ENV_VARS:
        env.pop(name, None)
    env.update(
        {
            "HOME": str(home),
            "XDG_CACHE_HOME": str(home / ".cache"),
            "XDG_CONFIG_HOME": str(home / ".config"),
            "TOKENZERO_CACHE_PATH": str(cache),
            "TOKENZERO_REF_INDEX_PATH": str(ref_index),
            "NO_COLOR": "1",
            "TERM": "dumb",
        }
    )
    return env


def decode_json(stdout: bytes, label: str) -> Any:
    try:
        return json.loads(stdout.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        preview = stdout[:500].decode("utf-8", errors="replace")
        raise HarnessError(f"{label} returned invalid JSON: {exc}; stdout={preview!r}") from exc


def run_cli(
    binary: Path,
    arguments: list[str],
    cwd: Path,
    env: dict[str, str],
    label: str,
    validator: Callable[[bytes], None],
) -> tuple[float, float]:
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    started = time.perf_counter_ns()
    try:
        proc = subprocess.run(
            [str(binary), *arguments],
            cwd=cwd,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        raise HarnessError(f"{label} timed out after {TIMEOUT_SECONDS}s") from exc
    wall_ms = (time.perf_counter_ns() - started) / 1_000_000
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpu_ms = 1000.0 * (
        (after.ru_utime - before.ru_utime) + (after.ru_stime - before.ru_stime)
    )
    if proc.returncode != 0:
        stderr = proc.stderr[-2000:].decode("utf-8", errors="replace")
        raise HarnessError(f"{label} exited {proc.returncode}: {stderr.strip()}")
    validator(proc.stdout)
    return wall_ms, cpu_ms


def find_ref(value: Any) -> str | None:
    if isinstance(value, dict):
        direct = value.get("ref")
        if isinstance(direct, str) and direct.startswith(("tz://", "fz://", "gz://")):
            return direct
        for child in value.values():
            found = find_ref(child)
            if found is not None:
                return found
    elif isinstance(value, list):
        for child in value:
            found = find_ref(child)
            if found is not None:
                return found
    return None


class McpClient:
    def __init__(self, binary: Path, cwd: Path, env: dict[str, str], cache: Path, name: str) -> None:
        self.name = name
        self.stderr = tempfile.TemporaryFile(mode="w+t", encoding="utf-8")
        self.child = subprocess.Popen(
            [str(binary), "mcp-server", "--allowed-root", str(cwd), "--cache-path", str(cache)],
            cwd=cwd,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.stderr,
            bufsize=0,
        )
        self.next_id = 1
        response, _ = self.request(
            "initialize",
            {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "tokenzero-matched-ab", "version": "1"},
            },
        )
        if not isinstance(response.get("result"), dict):
            raise HarnessError(f"{name} initialize response has no object result")
        self.notify("notifications/initialized", {})

    def _stderr_tail(self) -> str:
        self.stderr.flush()
        self.stderr.seek(0)
        return self.stderr.read()[-2000:]

    def _readline(self) -> bytes:
        assert self.child.stdout is not None
        selector = selectors.DefaultSelector()
        selector.register(self.child.stdout, selectors.EVENT_READ)
        try:
            if not selector.select(TIMEOUT_SECONDS):
                raise HarnessError(f"{self.name} MCP response timed out after {TIMEOUT_SECONDS}s")
            line = self.child.stdout.readline()
        finally:
            selector.close()
        if not line:
            raise HarnessError(
                f"{self.name} MCP server exited unexpectedly with {self.child.poll()}: "
                f"{self._stderr_tail().strip()}"
            )
        return line

    def request(self, method: str, params: dict[str, Any]) -> tuple[dict[str, Any], float]:
        assert self.child.stdin is not None
        request_id = self.next_id
        self.next_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        }
        started = time.perf_counter_ns()
        try:
            self.child.stdin.write(json.dumps(payload, separators=(",", ":")).encode() + b"\n")
            self.child.stdin.flush()
        except BrokenPipeError as exc:
            raise HarnessError(f"{self.name} MCP stdin closed: {self._stderr_tail().strip()}") from exc
        while True:
            raw = self._readline()
            try:
                response = json.loads(raw)
            except json.JSONDecodeError as exc:
                raise HarnessError(f"{self.name} MCP emitted invalid JSON: {raw[:500]!r}") from exc
            if not isinstance(response, dict):
                raise HarnessError(f"{self.name} MCP emitted a non-object response: {response!r}")
            if response.get("id") != request_id:
                if "id" not in response and "method" in response:
                    continue
                raise HarnessError(
                    f"{self.name} MCP response id {response.get('id')!r} did not match {request_id}"
                )
            if "error" in response:
                raise HarnessError(f"{self.name} MCP {method} error: {response['error']!r}")
            if "result" not in response:
                raise HarnessError(f"{self.name} MCP {method} response has no result")
            elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
            return response, elapsed_ms

    def notify(self, method: str, params: dict[str, Any]) -> None:
        assert self.child.stdin is not None
        payload = {"jsonrpc": "2.0", "method": method, "params": params}
        self.child.stdin.write(json.dumps(payload, separators=(",", ":")).encode() + b"\n")
        self.child.stdin.flush()

    def close(self) -> None:
        if self.child.stdin is not None and not self.child.stdin.closed:
            self.child.stdin.close()
        try:
            rc = self.child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.child.terminate()
            try:
                rc = self.child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.child.kill()
                rc = self.child.wait(timeout=5)
        stderr = self._stderr_tail()
        self.stderr.close()
        if rc != 0:
            raise HarnessError(f"{self.name} MCP server exited {rc}: {stderr.strip()}")


def validate_binary(path_text: str, option: str) -> Path:
    path = Path(path_text).expanduser().resolve()
    if not path.is_file():
        raise HarnessError(f"{option} is not a regular file: {path}")
    if not os.access(path, os.X_OK):
        raise HarnessError(f"{option} is not executable: {path}")
    return path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compare already-built TokenZero binaries with matched, alternating samples."
    )
    parser.add_argument("--baseline", required=True, help="path to the already-built baseline binary")
    parser.add_argument("--candidate", required=True, help="path to the already-built candidate binary")
    parser.add_argument("--fixture", required=True, help="UTF-8 regular file used by read/find/expand")
    parser.add_argument("--work-dir", required=True, help="existing allowed root and command working directory")
    parser.add_argument("--trials", type=int, default=20, help="samples per binary and scenario (default: 20)")
    parser.add_argument("--json-output", type=Path, help="also write the stdout JSON report to this path")
    parser.add_argument(
        "--noise-tolerance-pct",
        type=float,
        default=0.0,
        help="allowed candidate increase for gate metrics, in percent (default: 0)",
    )
    return parser.parse_args()


def run(args: argparse.Namespace) -> tuple[dict[str, Any], bool]:
    if args.trials < 1:
        raise HarnessError("--trials must be at least 1")
    if not math.isfinite(args.noise_tolerance_pct) or args.noise_tolerance_pct < 0:
        raise HarnessError("--noise-tolerance-pct must be a finite nonnegative number")
    baseline = validate_binary(args.baseline, "--baseline")
    candidate = validate_binary(args.candidate, "--candidate")
    if baseline == candidate:
        raise HarnessError("--baseline and --candidate must resolve to different files")
    fixture = Path(args.fixture).expanduser().resolve()
    work_dir = Path(args.work_dir).expanduser().resolve()
    if not work_dir.is_dir():
        raise HarnessError(f"--work-dir is not a directory: {work_dir}")
    if not fixture.is_file():
        raise HarnessError(f"--fixture is not a regular file: {fixture}")
    try:
        fixture.relative_to(work_dir)
    except ValueError as exc:
        raise HarnessError(f"--fixture must be inside --work-dir ({work_dir}): {fixture}") from exc
    fixture_bytes = fixture.read_bytes()
    try:
        fixture_text = fixture_bytes.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise HarnessError(f"--fixture must contain UTF-8 text: {fixture}") from exc
    words = [word for word in fixture_text.split() if any(char.isalnum() for char in word)]
    if not words:
        raise HarnessError("--fixture must contain at least one searchable word")
    query = words[0][:128]

    with tempfile.TemporaryDirectory(prefix="tokenzero-matched-ab-") as raw_tmp:
        temp_root = Path(raw_tmp)
        seed = temp_root / "expand-seed.txt"
        repeated = fixture_bytes or b"tokenzero\n"
        seed_payload = (repeated * (131072 // len(repeated) + 1))[:131072]
        seed.write_bytes(seed_payload)
        sides: dict[str, dict[str, Any]] = {}
        for name, binary in (("baseline", baseline), ("candidate", candidate)):
            home = temp_root / name / "home"
            home.mkdir(parents=True)
            cache = temp_root / name / "store.json"
            ref_index = temp_root / name / "ref-index"
            sides[name] = {
                "binary": binary,
                "cache": cache,
                "env": isolated_env(home, cache, ref_index),
            }

        def json_object(label: str) -> Callable[[bytes], None]:
            def validate(stdout: bytes) -> None:
                value = decode_json(stdout, label)
                if not isinstance(value, (dict, list)):
                    raise HarnessError(f"{label} JSON response must be an object or array")
            return validate

        refs: dict[str, str] = {}
        for name in ("baseline", "candidate"):
            side = sides[name]
            captured: dict[str, Any] = {}

            def capture(stdout: bytes, *, label: str = name) -> None:
                value = decode_json(stdout, f"{label} expand seed")
                ref = find_ref(value)
                if ref is None:
                    raise HarnessError(f"{label} expand seed response contained no recoverable ref")
                captured["ref"] = ref

            run_cli(
                side["binary"],
                ["read", str(seed), "--allowed-root", str(temp_root), "--cache-path", str(side["cache"]), "--json"],
                work_dir,
                side["env"],
                f"{name} expand seed",
                capture,
            )
            refs[name] = captured["ref"]
        if refs["baseline"] != refs["candidate"]:
            raise HarnessError(
                "equivalent expand seed produced different refs: "
                f"baseline={refs['baseline']} candidate={refs['candidate']}"
            )

        versions: dict[str, str] = {}

        def version_output(name: str) -> Callable[[bytes], None]:
            def validate(stdout: bytes) -> None:
                try:
                    value = stdout.decode("utf-8").strip()
                except UnicodeDecodeError as exc:
                    raise HarnessError(f"{name} version returned non-UTF-8 stdout") from exc
                if not value:
                    raise HarnessError(f"{name} version returned empty stdout")
                previous = versions.setdefault(name, value)
                if previous != value:
                    raise HarnessError(
                        f"{name} version output changed during the run: {previous!r} -> {value!r}"
                    )
            return validate

        def echo(stdout: bytes) -> None:
            if ECHO_SENTINEL.encode() not in stdout:
                raise HarnessError(f"run_echo response omitted {ECHO_SENTINEL!r}")

        def expanded(stdout: bytes) -> None:
            if stdout != seed_payload:
                raise HarnessError("expand --raw response did not exactly match the seeded bytes")

        operations: dict[str, Callable[[str], tuple[list[str], Callable[[bytes], None]]]] = {
            "version": lambda name: (["--version"], version_output(name)),
            "run_echo": lambda name: (
                ["run", "--allowed-root", str(work_dir), "--", "/bin/echo", ECHO_SENTINEL],
                echo,
            ),
            "read": lambda name: (
                ["read", str(fixture), "--allowed-root", str(work_dir), "--cache-path", str(sides[name]["cache"]), "--json"],
                json_object(f"{name} read"),
            ),
            "find": lambda name: (
                ["find", query, str(fixture), "--allowed-root", str(work_dir), "--cache-path", str(sides[name]["cache"]), "--json"],
                json_object(f"{name} find"),
            ),
            "tree": lambda name: (
                ["tree", str(work_dir), "--depth", "2", "--allowed-root", str(work_dir), "--cache-path", str(sides[name]["cache"]), "--json"],
                json_object(f"{name} tree"),
            ),
            "expand": lambda name: (
                ["expand", refs[name], "--cache-path", str(sides[name]["cache"]), "--raw"],
                expanded,
            ),
        }
        raw: dict[str, dict[str, dict[str, list[float]]]] = {}
        for operation, command_factory in operations.items():
            raw[operation] = {
                name: {"wall_ms": [], "cpu_ms": []} for name in ("baseline", "candidate")
            }
            for trial in range(args.trials):
                order = ("baseline", "candidate") if trial % 2 == 0 else ("candidate", "baseline")
                for name in order:
                    side = sides[name]
                    command, validator = command_factory(name)
                    wall_ms, cpu_ms = run_cli(
                        side["binary"], command, work_dir, side["env"], f"{name} {operation}", validator
                    )
                    raw[operation][name]["wall_ms"].append(wall_ms)
                    raw[operation][name]["cpu_ms"].append(cpu_ms)

        clients: dict[str, McpClient] = {}
        try:
            for name in ("baseline", "candidate"):
                side = sides[name]
                clients[name] = McpClient(
                    side["binary"], work_dir, side["env"], side["cache"], name
                )
            def validate_mcp(operation: str, name: str, response: dict[str, Any]) -> None:
                result = response.get("result")
                if not isinstance(result, dict):
                    raise HarnessError(f"{name} {operation} result is not an object")
                if result.get("isError") is True:
                    raise HarnessError(f"{name} {operation} returned isError=true: {result!r}")
                if operation == "mcp_tools_list_warm":
                    tools = result.get("tools")
                    if not isinstance(tools, list) or not tools:
                        raise HarnessError(f"{name} tools/list returned no tools")
                    tool_names = {
                        tool.get("name") for tool in tools if isinstance(tool, dict)
                    }
                    if "read" not in tool_names and "tz_read" not in tool_names:
                        raise HarnessError(f"{name} tools/list did not advertise a read tool")

            mcp_specs = {
                "mcp_tools_list_warm": ("tools/list", {}),
                "mcp_read_warm": (
                    "tools/call",
                    {"name": "read", "arguments": {"path": str(fixture), "fresh": True}},
                ),
            }
            for operation, (method, params) in mcp_specs.items():
                for name in ("baseline", "candidate"):
                    warm, _ = clients[name].request(method, params)
                    validate_mcp(operation, name, warm)
                raw[operation] = {
                    name: {"wall_ms": []} for name in ("baseline", "candidate")
                }
                for trial in range(args.trials):
                    order = ("baseline", "candidate") if trial % 2 == 0 else ("candidate", "baseline")
                    for name in order:
                        response, wall_ms = clients[name].request(method, params)
                        validate_mcp(operation, name, response)
                        raw[operation][name]["wall_ms"].append(wall_ms)
        finally:
            close_errors = []
            for client in clients.values():
                try:
                    client.close()
                except HarnessError as exc:
                    close_errors.append(str(exc))
            if close_errors and sys.exc_info()[0] is None:
                raise HarnessError("; ".join(close_errors))

        results: dict[str, Any] = {}
        regressions: list[dict[str, Any]] = []
        tolerance_factor = 1.0 + args.noise_tolerance_pct / 100.0
        for operation, samples in raw.items():
            metrics: dict[str, Any] = {}
            for metric in samples["baseline"]:
                metrics[metric] = describe_metric(
                    samples["baseline"][metric], samples["candidate"][metric]
                )
                for percentile in ("p50", "p95"):
                    before = metrics[metric]["baseline"][percentile]
                    after = metrics[metric]["candidate"][percentile]
                    if after > before * tolerance_factor:
                        regressions.append(
                            {
                                "operation": operation,
                                "metric": metric,
                                "percentile": percentile,
                                "baseline": before,
                                "candidate": after,
                                "change_pct": metrics[metric]["change_pct"][percentile],
                            }
                        )
            results[operation] = {"metrics": metrics}

        report = {
            "schema": SCHEMA,
            "environment": {
                "platform": platform.platform(),
                "machine": platform.machine(),
                "python": platform.python_version(),
                "baseline": {"path": str(baseline), "sha256": hashlib.sha256(baseline.read_bytes()).hexdigest(), "version": versions["baseline"]},
                "candidate": {"path": str(candidate), "sha256": hashlib.sha256(candidate.read_bytes()).hexdigest(), "version": versions["candidate"]},
                "fixture": {"path": str(fixture), "sha256": hashlib.sha256(fixture_bytes).hexdigest(), "bytes": len(fixture_bytes)},
                "work_dir": str(work_dir),
            },
            "methodology": {
                "trials_per_binary": args.trials,
                "sample_order": "AB on even trials, BA on odd trials, independently for every operation",
                "isolation": "separate temporary HOME, XDG cache/config, recovery cache, and ref index per binary; identical immutable inputs",
                "expand_seed": "identical deterministic 131072-byte repetition of fixture bytes; refs required to match",
                "mcp": "one long-lived stdio process per binary; initialize, notifications/initialized, one unmeasured operation warmup, then alternating measured requests",
                "percentile": "nearest rank",
                "units": {"wall_ms": "milliseconds", "cpu_ms": "milliseconds"},
            },
            "measurement_limitations": [
                "CLI CPU is the portable RUSAGE_CHILDREN user+system delta and may include descendant work; it excludes parent harness CPU.",
                "Per-request CPU is not reported for long-lived MCP processes because Python exposes no portable, sufficiently precise process-CPU snapshot API; MCP gating therefore uses wall time only.",
                "Wall time includes process startup for CLI operations and JSON-RPC framing/pipe scheduling for MCP operations.",
                "The harness does not pin CPU frequency, affinity, thermal state, or competing system load.",
            ],
            "gate": {
                "noise_tolerance_pct": args.noise_tolerance_pct,
                "rule": "fail when any available candidate p50 or p95 wall_ms/cpu_ms exceeds baseline by more than noise_tolerance_pct",
                "passed": not regressions,
                "regressions": regressions,
            },
            "results": results,
        }
        return report, not regressions


def main() -> int:
    args = parse_args()
    try:
        report, passed = run(args)
        payload = json.dumps(report, indent=2, sort_keys=True) + "\n"
        if args.json_output is not None:
            destination = args.json_output.expanduser()
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(payload, encoding="utf-8")
        sys.stdout.write(payload)
        return 0 if passed else 1
    except (HarnessError, OSError) as exc:
        print(f"compare_binaries.py: error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
