#!/usr/bin/env python3
"""Verify byte-stable prompt-prefix surfaces without rebuilding TokenZero.

The suite runs every generated surface in a fresh process, exercises cache-pack
against cold, warm, post-GC, and fresh stores, and writes reproducible evidence.
It measures byte reuse as a provider-independent prefix proxy; it does not claim
an upstream model provider's cache-hit telemetry.
"""
from __future__ import annotations

import json
import os
import subprocess
import tempfile
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[3]
import sys
sys.path.insert(0, str(REPO / "benchmarks"))
from bench_common import environment as _environment, sha256_bytes as sha256, write_json
environment = lambda: _environment(REPO, BIN)
BIN = REPO / "target/debug/tokenzero"
EASY_START = REPO / "docs/easy-start.tokenzero.md"
SESSION_DELTA = Path(__file__).with_name("session_delta") / "evidence.json"
OUT = Path(__file__).with_suffix("") / "evidence.json"


def run_bytes(args: list[str], *, env_overrides: dict[str, str] | None = None) -> bytes:
    env = os.environ.copy()
    for name in (
        "TOKENZERO_CACHE_PATH",
        "TOKENZERO_ROOT",
        "ZEROSTACK_STORE_ROOT",
        "TOKENZERO_ALLOWED_ROOTS",
    ):
        env.pop(name, None)
    env.update({"CI": "1", "NO_COLOR": "1"})
    if env_overrides:
        env.update(env_overrides)
    completed = subprocess.run(
        [str(BIN), *args],
        cwd=REPO,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"command failed ({completed.returncode}): {args!r}\n"
            + completed.stderr.decode("utf-8", errors="replace")[-2000:]
        )
    return completed.stdout


def run_json(args: list[str], *, env_overrides: dict[str, str] | None = None) -> dict[str, Any]:
    return json.loads(run_bytes(args, env_overrides=env_overrides))


def stable_ref(response: dict[str, Any]) -> str:
    for record in response.get("refs", []):
        if record.get("kind") == "stable_prefix":
            return str(record["ref"])
    raise AssertionError(f"cache-pack response has no stable_prefix ref: {response.get('refs')!r}")


def expanded_bytes(ref_id: str, cache: Path) -> bytes:
    response = run_json(["expand", ref_id, "--cache-path", str(cache), "--json"])
    assert response["status"] == "ok", response
    return response["visible"]["text"].encode("utf-8")


def cache_pack(root: Path, cache: Path) -> tuple[dict[str, Any], bytes, str]:
    response = run_json(
        [
            "cache-pack",
            "--scope",
            "agent",
            "--root",
            str(root),
            "--cache-path",
            str(cache),
            "--json",
        ]
    )
    assert response["status"] == "ok", response
    ref_id = stable_ref(response)
    return response, expanded_bytes(ref_id, cache), ref_id


def changed_paths(left: Any, right: Any, prefix: str = "") -> list[str]:
    if type(left) is not type(right):
        return [prefix or "$"]
    if isinstance(left, dict):
        paths: list[str] = []
        for key in sorted(set(left) | set(right)):
            path = f"{prefix}.{key}" if prefix else key
            if key not in left or key not in right:
                paths.append(path)
            else:
                paths.extend(changed_paths(left[key], right[key], path))
        return paths
    if isinstance(left, list):
        if len(left) != len(right):
            return [prefix]
        paths = []
        for index, (a, b) in enumerate(zip(left, right)):
            paths.extend(changed_paths(a, b, f"{prefix}[{index}]"))
        return paths
    return [] if left == right else [prefix or "$"]


def longest_common_prefix(values: list[bytes]) -> int:
    if not values:
        return 0
    limit = min(map(len, values))
    first = values[0]
    for index in range(limit):
        if any(value[index] != first[index] for value in values[1:]):
            return index
    return limit


def surface_result(samples: list[bytes]) -> dict[str, Any]:
    lcp = longest_common_prefix(samples)
    denominator = len(samples[0]) if samples else 0
    identical = all(sample == samples[0] for sample in samples[1:])
    return {
        "runs": len(samples),
        "bytes": [len(sample) for sample in samples],
        "sha256": [sha256(sample) for sample in samples],
        "byte_identical": identical,
        "longest_common_prefix_bytes": lcp,
        "prefix_reuse_pct": round(100 * lcp / denominator, 3) if denominator else 100.0,
    }


def main() -> None:
    if not BIN.is_file():
        raise SystemExit("target/debug/tokenzero missing; use the prebuilt binary")
    if not SESSION_DELTA.is_file():
        raise SystemExit(f"session-delta evidence missing: {SESSION_DELTA}")

    easy_before = EASY_START.read_bytes()
    capabilities = [run_bytes(["capabilities", "--json"]) for _ in range(2)]
    robot_guide = [run_bytes(["robot-docs", "guide"]) for _ in range(2)]

    with tempfile.TemporaryDirectory(prefix="tokenzero-prefix-conformance-") as raw_tmp:
        temp = Path(raw_tmp)
        root = temp / "repo"
        fresh_root = temp / "repo-after-restart"
        for fixture_root in (root, fresh_root):
            fixture_root.mkdir()
            (fixture_root / "AGENTS.md").write_text("stable instructions\n")
            (fixture_root / "Cargo.toml").write_text("[workspace]\n")
        cache = temp / "cache.json"
        fresh_cache = temp / "fresh-cache.json"

        cold_response, cold_prefix, cold_ref = cache_pack(root, cache)
        warm_response, warm_prefix, warm_ref = cache_pack(root, cache)

        run_json(
            [
                "cache",
                "prune",
                "--root",
                str(root),
                "--cache-path",
                str(cache),
                "--apply",
                "--json",
            ]
        )
        post_gc_response, post_gc_prefix, post_gc_ref = cache_pack(root, cache)
        fresh_response, fresh_prefix, fresh_ref = cache_pack(fresh_root, fresh_cache)

        capabilities.append(
            run_bytes(
                ["capabilities", "--json"],
                env_overrides={"TOKENZERO_CACHE_PATH": str(cache)},
            )
        )
        robot_guide.append(
            run_bytes(
                ["robot-docs", "guide"],
                env_overrides={"TOKENZERO_CACHE_PATH": str(fresh_cache)},
            )
        )

        stable_prefixes = [cold_prefix, warm_prefix, post_gc_prefix, fresh_prefix]
        cache_refs = [cold_ref, warm_ref, post_gc_ref, fresh_ref]
        cache_state_labels = ["cold", "warm", "post_gc", "fresh_store"]
        for prefix in stable_prefixes:
            assert str(temp).encode() not in prefix, "absolute fixture path leaked into stable prefix"
        cache_manifest_states = [
            json.loads(response["visible"]["text"])
            for response in (cold_response, warm_response, post_gc_response, fresh_response)
        ]
        whole_output_changes = {
            "cold_to_warm": changed_paths(cold_response, warm_response),
            "warm_to_post_gc": changed_paths(warm_response, post_gc_response),
            "cold_to_fresh_store": changed_paths(cold_response, fresh_response),
        }
        manifest_changes = {
            "cold_to_warm": changed_paths(cache_manifest_states[0], cache_manifest_states[1]),
            "warm_to_post_gc": changed_paths(cache_manifest_states[1], cache_manifest_states[2]),
            "cold_to_fresh_store": changed_paths(cache_manifest_states[0], cache_manifest_states[3]),
        }

    easy_after = EASY_START.read_bytes()
    surfaces = {
        "easy_start_section": surface_result([easy_before, easy_after]),
        "capabilities_json": surface_result(capabilities),
        "robot_docs_guide": surface_result(robot_guide),
        "cache_pack_stable_prefix": surface_result(stable_prefixes),
    }
    failures = [name for name, row in surfaces.items() if not row["byte_identical"]]
    assert not failures, f"non-byte-stable prefix surfaces: {failures}"
    assert len(set(cache_refs)) == 1, f"stable cache-pack ref changed: {cache_refs}"

    session_delta = json.loads(SESSION_DELTA.read_text())
    turn_2_plus = session_delta["turn_2_plus"]
    provider_independent_ratio = min(row["prefix_reuse_pct"] for row in surfaces.values())
    evidence = {
        "schema": "tokenzero.prefix-conformance.v1",
        "environment": environment(),
        "methodology": {
            "process_model": "Each generated sample is a new tokenzero process; static easy-start bytes are read before and after cache mutation.",
            "cache_states": cache_state_labels,
            "gc": "cache prune --apply runs between warm and post_gc cache-pack samples",
            "comparison": "Raw stdout bytes for capabilities/robot docs; exact expanded stable_prefix bytes and ref identity for cache-pack.",
            "integrity": "All surfaces and cache states are retained; no mismatches or losses are filtered.",
            "serializer": "TokenZero's emitted bytes are compared without JSON reserialization.",
        },
        "surfaces": surfaces,
        "cache_pack": {
            "states": cache_state_labels,
            "stable_refs": cache_refs,
            "stable_ref_identical": len(set(cache_refs)) == 1,
            "whole_output_changed_paths": whole_output_changes,
            "manifest_changed_paths": manifest_changes,
            "layout_invariant": "Stable prefix content/ref first; root, manifest path, and cache invalidation state remain outside expanded stable prefix in the volatile tail/manifest.",
        },
        "capabilities_diff": {
            "changed_paths": changed_paths(
                json.loads(capabilities[0]), json.loads(capabilities[-1])
            ),
            "finding": "No cache-state, timestamp, counter, map-order, or absolute-path fields occur in capabilities output.",
        },
        "session_delta_telemetry": {
            "source": str(SESSION_DELTA.relative_to(REPO)),
            "turn_2_plus": turn_2_plus,
            "interpretation": "Session-delta measures visible-byte suppression after turn 1; it is complementary to, not a substitute for, provider prefix-cache telemetry.",
        },
        "target_95": {
            "provider_independent_byte_stability_proxy_pct": provider_independent_ratio,
            "proxy_exceeds_95_pct": provider_independent_ratio > 95.0,
            "provider_cache_hit_rate_pct": None,
            "measurable": False,
            "reason": "No real model-provider cache-hit counter or billing trace is available to this local Python harness; claiming a provider hit rate would be fabricated.",
        },
        "volatile_field_findings": [
            "cache-pack manifest invalidation_reason/invalidation_count legitimately change cold-to-warm",
            "cache-pack root and manifest paths are confined to volatile_tail; changing cache path changes only volatile ref/manifest fields",
            "no volatile fields were found in easy-start, capabilities, robot-docs guide, or cache-pack stable_prefix bytes",
        ],
        "product_changes": [],
        "losses": [
            "The complete cache-pack response is intentionally not byte-identical across cold/warm stores because invalidation telemetry reports real state; only its advertised stable_prefix is prompt-prefix material.",
            "The >95% real provider cache-hit target is not measurable without provider telemetry; the suite proves only the byte-stability prerequisite.",
        ],
        "status": "ok",
    }
    write_json(OUT, evidence, emit=True)


if __name__ == "__main__":
    main()
