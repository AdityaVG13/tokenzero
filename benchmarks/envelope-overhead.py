#!/usr/bin/env python3
"""Measure the exact default CLI JSON envelope overhead for sub-KiB reads."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import tempfile
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
SCHEMA = "tokenzero.envelope_overhead.v1"
SIZES = (128, 512, 900)
GATE_SOURCE_BYTES = 900
MAX_OVERHEAD_PER_VISIBLE_PAYLOAD_PPM = 200_000


def resolve_binary() -> Path:
    configured = os.environ.get("TOKENZERO_BIN")
    candidates = [Path(configured)] if configured else []
    candidates.extend([ROOT / "target/release/tokenzero", ROOT / "target/debug/tokenzero"])
    for candidate in candidates:
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate.resolve()
    raise SystemExit("tokenzero binary not found; set TOKENZERO_BIN=/path/to/tokenzero")


def run(binary: Path, args: list[str], cwd: Path) -> subprocess.CompletedProcess[bytes]:
    env = os.environ.copy()
    env.pop("TOKENZERO_SLIM_ENVELOPE", None)
    return subprocess.run(
        [str(binary), *args],
        cwd=cwd,
        env=env,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def decode_success(result: subprocess.CompletedProcess[bytes], label: str) -> Any:
    if result.returncode != 0:
        raise RuntimeError(
            f"{label} failed with {result.returncode}: "
            f"{result.stderr.decode('utf-8', errors='replace')}"
        )
    return json.loads(result.stdout)


def measure(binary: Path) -> dict[str, Any]:
    rows: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="tokenzero-envelope-") as raw_dir:
        root = Path(raw_dir)
        cache = root / "recovery.json"
        for source_bytes in SIZES:
            source = b"x" * source_bytes
            path = root / f"payload-{source_bytes}.txt"
            path.write_bytes(source)
            result = run(
                binary,
                [
                    "read",
                    str(path),
                    "--cache-path",
                    str(cache),
                    "--allowed-root",
                    str(root),
                    "--json",
                ],
                root,
            )
            envelope = decode_success(result, f"default read {source_bytes}")
            if envelope.get("schema_version") != "tokenzero.cli.v1":
                raise RuntimeError(f"default slim schema drift: {envelope}")
            visible = envelope.get("visible")
            if not isinstance(visible, str):
                raise RuntimeError(f"default envelope visible payload is not a string: {envelope}")
            refs = envelope.get("refs")
            if not isinstance(refs, list) or not refs or not all(isinstance(ref, str) for ref in refs):
                raise RuntimeError(f"default envelope refs are not flat strings: {envelope}")
            if not refs[0].startswith("tz://o/"):
                raise RuntimeError(f"primary blob ref is not a durable ordinal: {refs[0]}")
            for reference in refs:
                expanded = run(
                    binary,
                    ["expand", reference, "--cache-path", str(cache), "--raw"],
                    root,
                )
                if expanded.returncode != 0 or expanded.stdout != source:
                    raise RuntimeError(
                        f"exact expansion failed for {reference}: rc={expanded.returncode} "
                        f"stderr={expanded.stderr.decode('utf-8', errors='replace')}"
                    )

            emitted_envelope_bytes = len(result.stdout)
            exact_visible_payload_bytes = len(visible.encode("utf-8"))
            exact_envelope_overhead_bytes = emitted_envelope_bytes - exact_visible_payload_bytes
            overhead_per_visible_payload_ppm = (
                exact_envelope_overhead_bytes * 1_000_000 // max(1, exact_visible_payload_bytes)
            )
            rows.append(
                {
                    "source_payload_bytes": source_bytes,
                    "emitted_envelope_bytes": emitted_envelope_bytes,
                    "exact_visible_payload_bytes": exact_visible_payload_bytes,
                    "exact_envelope_overhead_bytes": exact_envelope_overhead_bytes,
                    "overhead_per_visible_payload_ppm": overhead_per_visible_payload_ppm,
                    "within_20_percent": overhead_per_visible_payload_ppm
                    <= MAX_OVERHEAD_PER_VISIBLE_PAYLOAD_PPM,
                    "all_refs_expand_exact": True,
                    "ref_count": len(refs),
                }
            )

        gate_row = next(row for row in rows if row["source_payload_bytes"] == GATE_SOURCE_BYTES)
        full = run(
            binary,
            [
                "read",
                str(root / f"payload-{GATE_SOURCE_BYTES}.txt"),
                "--cache-path",
                str(cache),
                "--allowed-root",
                str(root),
                "--json=full",
            ],
            root,
        )
        full_envelope = decode_success(full, "full compatibility read")
        full_schema_preserved = (
            full_envelope.get("schema_version") == "tokenzero.cli.v1"
            and isinstance(full_envelope.get("telemetry"), dict)
            and isinstance(full_envelope.get("accounting"), dict)
            and isinstance(full_envelope.get("visible"), dict)
        )

        claim = run(
            binary,
            [
                "claim-audit",
                "--output-json",
                str(root / "claim-audit.json"),
                "--json",
            ],
            root,
        )
        claim_envelope = decode_success(claim, "claim audit schema probe")
        claim_audit_full_schema_preserved = (
            claim_envelope.get("schema_version") == "tokenzero.claim_audit.v1"
            and isinstance(claim_envelope.get("evidence_gates"), list)
        )

    gate_pass = (
        gate_row["overhead_per_visible_payload_ppm"]
        <= MAX_OVERHEAD_PER_VISIBLE_PAYLOAD_PPM
    )
    full_larger_than_default = len(full.stdout) > gate_row["emitted_envelope_bytes"]
    ok = (
        gate_pass
        and full_schema_preserved
        and full_larger_than_default
        and claim_audit_full_schema_preserved
    )
    return {
        "schema_version": SCHEMA,
        "status": "ok" if ok else "failed",
        "ok": ok,
        "binary": str(binary),
        "measurement_kind": "exact",
        "numerator": "exact_envelope_overhead_bytes",
        "denominator": "exact_visible_payload_bytes",
        "line_terminator_included_in_emitted_envelope_bytes": True,
        "rows": rows,
        "all_measured_rows_within_limit": all(
            row["within_20_percent"] for row in rows
        ),
        "gate": {
            "scope": "single_900_byte_sub_kib_fixture",
            "source_payload_bytes": GATE_SOURCE_BYTES,
            "maximum_overhead_per_visible_payload_ppm": MAX_OVERHEAD_PER_VISIBLE_PAYLOAD_PPM,
            "observed_overhead_per_visible_payload_ppm": gate_row[
                "overhead_per_visible_payload_ppm"
            ],
            "pass": gate_pass,
        },
        "full_envelope": {
            "emitted_envelope_bytes": len(full.stdout),
            "schema_preserved": full_schema_preserved,
            "larger_than_default": full_larger_than_default,
        },
        "claim_audit_full_schema_preserved": claim_audit_full_schema_preserved,
    }


def markdown(report: dict[str, Any]) -> str:
    lines = [
        "| source payload bytes | emitted envelope bytes | exact visible payload bytes | exact overhead bytes | overhead / visible payload | refs exact |",
        "|---:|---:|---:|---:|---:|:---:|",
    ]
    for row in report["rows"]:
        ratio = row["overhead_per_visible_payload_ppm"] / 10_000
        lines.append(
            f"| {row['source_payload_bytes']} | {row['emitted_envelope_bytes']} | "
            f"{row['exact_visible_payload_bytes']} | {row['exact_envelope_overhead_bytes']} | "
            f"{ratio:.2f}% | {'yes' if row['all_refs_expand_exact'] else 'no'} |"
        )
    gate = report["gate"]
    lines.extend(
        [
            "",
            f"Gate fixture: {gate['source_payload_bytes']} source bytes. Numerator is exact envelope overhead bytes; denominator is exact visible payload bytes. "
            f"Observed {gate['observed_overhead_per_visible_payload_ppm'] / 10_000:.2f}% against the 20.00% limit: "
            f"**{'PASS' if gate['pass'] else 'FAIL'}**.",
            f"`--json=full` emitted {report['full_envelope']['emitted_envelope_bytes']} bytes and preserved the forensic schema. "
            f"Claim-audit full artifact schema preserved: {'yes' if report['claim_audit_full_schema_preserved'] else 'no'}.",
        ]
    )
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--json-out", type=Path)
    parser.add_argument("--format", choices=("markdown", "json"), default="markdown")
    args = parser.parse_args()
    report = measure(resolve_binary())
    if args.json_out:
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        temporary = args.json_out.with_suffix(args.json_out.suffix + ".tmp")
        temporary.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
        temporary.replace(args.json_out)
    if args.format == "json":
        print(json.dumps(report, sort_keys=True, indent=2))
    else:
        print(markdown(report))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
