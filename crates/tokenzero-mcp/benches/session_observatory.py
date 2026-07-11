#!/usr/bin/env python3
"""Decompose tokenzero.ledger.v1 sessions into tokenzero.observatory.v1 turns."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
BIN = REPO / "target/debug/tokenzero"
EVIDENCE = Path(__file__).with_suffix("").with_name("session_observatory")
LEDGER_SCHEMA = "tokenzero.ledger.v1"
OBSERVATORY_SCHEMA = "tokenzero.observatory.v1"


def count_tokens(text: str) -> int:
    """Match tokenzero_core::count_tokens, including its ASCII-only word rule."""
    tokens = 0
    in_token = False
    for char in text:
        if char.isascii() and (char.isalnum() or char == "_"):
            if not in_token:
                tokens += 1
                in_token = True
        elif char.isspace():
            in_token = False
        else:
            tokens += 1
            in_token = False
    return tokens


def canonical_tokens(value: object) -> int:
    return count_tokens(json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False))


def load_records(path: Path) -> tuple[list[dict], int, int]:
    records = []
    malformed = 0
    ignored_schema = 0
    for raw_line in path.read_bytes().splitlines():
        if not raw_line.strip():
            continue
        try:
            record = json.loads(raw_line)
        except (json.JSONDecodeError, UnicodeDecodeError):
            malformed += 1
            continue
        if record.get("schema") != LEDGER_SCHEMA:
            ignored_schema += 1
            continue
        try:
            record["session_id"]
            record["timestamp_ms"]
            record["tool"]
            record["token_mass"]["visible_tokens"]
        except (KeyError, TypeError):
            malformed += 1
            continue
        records.append(record)
    return records, malformed, ignored_schema


def select_session(records: list[dict], session_id: str | None) -> list[dict]:
    if not records:
        return []
    selected = session_id or max(records, key=lambda row: int(row["timestamp_ms"]))["session_id"]
    return sorted(
        (row for row in records if row["session_id"] == selected),
        key=lambda row: int(row["timestamp_ms"]),
    )


def observe(
    ledger_path: Path,
    session_id: str | None,
    boot_envelope_tokens: int,
    tool_schema_tokens: int,
    capture: dict | None = None,
) -> dict:
    records, malformed, ignored_schema = load_records(ledger_path)
    rows = select_session(records, session_id)
    turns = []
    for index, record in enumerate(rows, start=1):
        visible = int(record["token_mass"]["visible_tokens"])
        is_expand = str(record["tool"]).removeprefix("tz_") == "expand"
        expand = visible if is_expand else 0
        ordinary_result = 0 if is_expand else visible
        boot = boot_envelope_tokens if index == 1 else 0
        schemas = tool_schema_tokens if index == 1 else 0
        buckets = {
            "boot_envelope_tokens": boot,
            "tool_schema_tokens": schemas,
            "tool_result_tokens": ordinary_result,
            "expand_materialization_tokens": expand,
        }
        turns.append({
            "turn_index": index,
            "timestamp_ms": int(record["timestamp_ms"]),
            "tool": record["tool"],
            "accounting_source": LEDGER_SCHEMA,
            "cost_buckets": buckets,
            "tool_result_visible_tokens": visible,
            "model_facing_total_tokens": sum(buckets.values()),
            "ledger_token_mass": record["token_mass"],
            "cumulative_session_cost_tokens": int(record.get("cumulative_session_cost_tokens", 0)),
            "optimization_tags": record.get("optimization_tags", []),
        })
    for event in (capture or {}).get("unledgered_tool_results", []):
        canonical = str(event["tool"]).removeprefix("tz_")
        if any(str(record["tool"]).removeprefix("tz_") == canonical for record in rows):
            continue
        visible = int(event["visible_tokens"])
        buckets = {
            "boot_envelope_tokens": 0,
            "tool_schema_tokens": 0,
            "tool_result_tokens": 0 if canonical == "expand" else visible,
            "expand_materialization_tokens": visible if canonical == "expand" else 0,
        }
        turns.append({
            "turn_index": len(turns) + 1,
            "timestamp_ms": int(rows[-1]["timestamp_ms"]) + len(turns) if rows else 0,
            "tool": canonical,
            "accounting_source": "mcp_capture",
            "cost_buckets": buckets,
            "tool_result_visible_tokens": visible,
            "model_facing_total_tokens": sum(buckets.values()),
            "ledger_token_mass": None,
            "cumulative_session_cost_tokens": int(rows[-1].get("cumulative_session_cost_tokens", 0)) if rows else 0,
            "optimization_tags": [],
        })
    totals = {
        key: sum(turn["cost_buckets"][key] for turn in turns)
        for key in (
            "boot_envelope_tokens",
            "tool_schema_tokens",
            "tool_result_tokens",
            "expand_materialization_tokens",
        )
    }
    totals["tool_result_visible_tokens"] = sum(turn["tool_result_visible_tokens"] for turn in turns)
    totals["model_facing_total_tokens"] = sum(turn["model_facing_total_tokens"] for turn in turns)
    losses = []
    if len(turns) < 5:
        losses.append(f"Only {len(turns)} tool turn(s) were observed; this is a schema example, not a workload distribution.")
    if any(turn["accounting_source"] == "mcp_capture" for turn in turns):
        losses.append("The ledger emitted no expand record; expand materialization cost is measured from the captured MCP result with the same tokenizer.")
    if malformed:
        losses.append(f"Ignored {malformed} malformed ledger record(s).")
    if ignored_schema:
        losses.append(f"Ignored {ignored_schema} foreign-schema ledger record(s).")
    if not boot_envelope_tokens:
        losses.append("Boot-envelope cost was unavailable and is reported as zero.")
    if not tool_schema_tokens:
        losses.append("Tool-schema cost was unavailable and is reported as zero.")
    result = {
        "schema": OBSERVATORY_SCHEMA,
        "session": {
            "session_id": rows[0]["session_id"] if rows else session_id,
            "repo": rows[0]["repo"] if rows else None,
            "agent": rows[0].get("agent") if rows else None,
            "version": rows[0]["version"] if rows else None,
        },
        "sample": {
            "turns_n": len(turns),
            "malformed_records_n": malformed,
            "ignored_schema_records_n": ignored_schema,
        },
        "methodology": {
            "tokenizer": "exact Python port of tokenzero_core::count_tokens",
            "boot_envelope": "canonical JSON of the MCP initialize result; charged once on turn 1",
            "tool_schema": "canonical JSON of the MCP tools/list result.tools array; charged once on turn 1",
            "tool_result": "ledger token_mass.visible_tokens; an unledgered captured MCP result is tokenized identically and labeled mcp_capture",
            "model_facing_total": "sum of the four disjoint cost_buckets; prevented tokens are not counted as cost",
            "prevented_read_metric": "ledger token_mass.prevented_tokens is retained per turn and never presented as observed cost",
        },
        "source": {
            "ledger": str(ledger_path),
            "ledger_sha256": hashlib.sha256(ledger_path.read_bytes()).hexdigest(),
            "capture": capture,
        },
        "totals": totals,
        "turns": turns,
        "losses_disclosed": losses or ["No data-quality or missing-envelope loss was detected by the pipeline."],
    }
    return result


def human_report(result: dict) -> str:
    totals = result["totals"]
    session = result["session"]
    lines = [
        "TokenZero session observatory",
        "=============================",
        f"schema: {result['schema']}",
        f"session: {session['session_id']}",
        f"version: {(session.get('version') or {}).get('crate')}",
        f"turns n: {result['sample']['turns_n']}",
        "",
        "Model-facing token cost",
        f"  boot envelope:          {totals['boot_envelope_tokens']}",
        f"  tool schemas:           {totals['tool_schema_tokens']}",
        f"  ordinary tool results:  {totals['tool_result_tokens']}",
        f"  expand materialization: {totals['expand_materialization_tokens']}",
        f"  total:                  {totals['model_facing_total_tokens']}",
        "",
        "Per turn",
    ]
    for turn in result["turns"]:
        buckets = turn["cost_buckets"]
        lines.append(
            f"  {turn['turn_index']:>2} {turn['tool']:<12} total={turn['model_facing_total_tokens']} "
            f"boot={buckets['boot_envelope_tokens']} schemas={buckets['tool_schema_tokens']} "
            f"result={buckets['tool_result_tokens']} expand={buckets['expand_materialization_tokens']}"
        )
    lines.extend(["", "Losses / limitations"])
    lines.extend(f"  - {loss}" for loss in result["losses_disclosed"])
    return "\n".join(lines) + "\n"


class McpClient:
    def __init__(self, root: Path, cache: Path) -> None:
        env = os.environ.copy()
        for key in ("TOKENZERO_CACHE_PATH", "TOKENZERO_ROOT", "ZEROSTACK_STORE_ROOT", "TOKENZERO_ALLOWED_ROOTS"):
            env.pop(key, None)
        env["TOKENZERO_AGENT"] = "session-observatory-replay"
        self.child = subprocess.Popen(
            [str(BIN), "mcp-server", "--allowed-root", str(root), "--cache-path", str(cache)],
            cwd=REPO,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
            env=env,
        )
        self.next_id = 1

    def request(self, method: str, params: dict) -> dict:
        assert self.child.stdin is not None and self.child.stdout is not None
        request_id = self.next_id
        self.next_id += 1
        message = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
        self.child.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
        self.child.stdin.flush()
        line = self.child.stdout.readline()
        if not line:
            stderr = self.child.stderr.read() if self.child.stderr else ""
            raise RuntimeError(f"MCP server exited unexpectedly ({self.child.poll()}): {stderr[-2000:]}")
        response = json.loads(line)
        if response.get("id") != request_id or "error" in response:
            raise RuntimeError(f"MCP request failed: {response}")
        return response["result"]

    def notify(self, method: str, params: dict) -> None:
        assert self.child.stdin is not None
        self.child.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method, "params": params}, separators=(",", ":")) + "\n")
        self.child.stdin.flush()

    def close(self) -> None:
        if self.child.stdin is not None:
            self.child.stdin.close()
        try:
            code = self.child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.child.kill()
            code = self.child.wait(timeout=5)
        if code != 0:
            stderr = self.child.stderr.read() if self.child.stderr else ""
            raise RuntimeError(f"MCP server exited with {code}: {stderr[-2000:]}")


def find_ref(value: object) -> str | None:
    if isinstance(value, str):
        match = re.search(r"(?:tz|fz)://[^\\s]+", value)
        if match:
            return match.group(0).rstrip(".,;)")
    if isinstance(value, dict):
        for child in value.values():
            found = find_ref(child)
            if found:
                return found
    if isinstance(value, list):
        for child in value:
            found = find_ref(child)
            if found:
                return found
    return None


def run_replay(output: Path, human: Path, ledger_copy: Path) -> dict:
    if not BIN.is_file():
        raise SystemExit("target/debug/tokenzero missing; use the existing debug binary or build it before running this harness")
    with tempfile.TemporaryDirectory(prefix="tokenzero-observatory-") as raw_tmp:
        root = Path(raw_tmp)
        corpus = root / "corpus.txt"
        corpus.write_text("needle TokenZero observatory replay\n" * 64)
        cache = root / "recovery-cache.json"
        client = McpClient(root, cache)
        try:
            initialize = client.request("initialize", {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "session-observatory", "version": "1"},
            })
            client.notify("notifications/initialized", {})
            tools = client.request("tools/list", {})
            read_result = client.request("tools/call", {"name": "tz_read", "arguments": {"path": str(corpus), "raw": True, "fresh": True}})
            client.request("tools/call", {"name": "tz_find", "arguments": {"query": "needle", "path": str(corpus), "fresh": True}})
            ref = find_ref(read_result)
            if ref is None:
                raise RuntimeError(f"tz_read produced no recovery ref: {read_result}")
            expand_result = client.request("tools/call", {"name": "tz_expand", "arguments": {"ref": ref}})
        finally:
            client.close()
        ledger = cache.with_name("ledger.jsonl")
        if not ledger.is_file():
            raise RuntimeError(f"replay did not produce {ledger}")
        ledger_copy.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ledger, ledger_copy)
        capture = {
            "kind": "prebuilt-mcp-stdio-replay",
            "binary": "target/debug/tokenzero",
            "binary_version": subprocess.run([str(BIN), "--version"], cwd=REPO, capture_output=True, text=True, check=True).stdout.strip(),
            "binary_mtime_ns": BIN.stat().st_mtime_ns,
            "environment": {"os": platform.platform(), "machine": platform.machine(), "python": platform.python_version()},
            "protocol_version": "2024-11-05",
            "tool_calls": ["tz_read", "tz_find", "tz_expand"],
            "boot_envelope_tokens": canonical_tokens(initialize),
            "tool_schema_tokens": canonical_tokens(tools.get("tools", [])),
            "tool_schemas_n": len(tools.get("tools", [])),
            "unledgered_tool_results": [{"tool": "tz_expand", "visible_tokens": canonical_tokens(expand_result)}],
        }
        result = observe(ledger_copy, None, capture["boot_envelope_tokens"], capture["tool_schema_tokens"], capture)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
        human.parent.mkdir(parents=True, exist_ok=True)
        human.write_text(human_report(result))
        return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    analyze = subparsers.add_parser("analyze", help="analyze an existing ledger")
    analyze.add_argument("ledger", type=Path)
    analyze.add_argument("--session-id")
    analyze.add_argument("--boot-envelope-tokens", type=int, default=0)
    analyze.add_argument("--tool-schema-tokens", type=int, default=0)
    analyze.add_argument("--output", type=Path, required=True)
    analyze.add_argument("--human", type=Path)
    replay = subparsers.add_parser("replay", help="capture a real three-turn session through the prebuilt MCP binary")
    replay.add_argument("--output", type=Path, default=EVIDENCE / "replayed.json")
    replay.add_argument("--human", type=Path, default=EVIDENCE / "replayed.txt")
    replay.add_argument("--ledger-copy", type=Path, default=EVIDENCE / "replayed.ledger.jsonl")
    args = parser.parse_args()

    if args.command == "replay":
        result = run_replay(args.output, args.human, args.ledger_copy)
        output = args.output
    else:
        result = observe(args.ledger, args.session_id, args.boot_envelope_tokens, args.tool_schema_tokens)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
        if args.human:
            args.human.parent.mkdir(parents=True, exist_ok=True)
            args.human.write_text(human_report(result))
        output = args.output
    print(json.dumps({"output": str(output), **result["sample"], "totals": result["totals"], "losses_disclosed": result["losses_disclosed"]}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
