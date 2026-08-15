#!/usr/bin/env python3
# cargo bench invocation (keep-gate path):
#   rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_tokenzero cargo bench -p tokenzero-core --bench hotpaths --profile release-perf
"""TokenZero performance keep-gate: quarantine, history ratchet, host-native bin."""

from __future__ import annotations

import argparse
import json
import math
import os
import shutil
import statistics
import subprocess
import sys
from pathlib import Path
from typing import Any

SCHEMA = "tokenzero.bench-history/v1"

# Single source for persist + keep compare bands (CC2-R5 / F-010).
KEEP_GATE_GEOMEAN_PCT = 3.0  # also persist-gate
KEEP_GATE_PASS_PCT = 5.0
CV_PCT_QUARANTINE = 5.0

ELF_MAGIC = b"\x7fELF"
MACHO_MAGICS = {
    b"\xfe\xed\xfa\xce",  # MH_MAGIC
    b"\xfe\xed\xfa\xcf",  # MH_MAGIC_64
    b"\xce\xfa\xed\xfe",  # MH_CIGAM
    b"\xcf\xfa\xed\xfe",  # MH_CIGAM_64
    b"\xca\xfe\xba\xbe",  # FAT_MAGIC / CAFEBABE
    b"\xbe\xba\xfe\xca",  # FAT_CIGAM
}


class KeepGateError(ValueError):
    """Fail-closed keep-gate / persist / resolve error."""


def cv_pct(samples: list[float]) -> float:
    """Population coefficient of variation in percent."""
    if not samples:
        raise KeepGateError("cv_pct requires a non-empty samples list")
    mean = statistics.fmean(samples)
    if mean == 0.0:
        return 0.0
    return (statistics.pstdev(samples) / abs(mean)) * 100.0


def _group_samples(group: dict[str, Any]) -> list[float] | None:
    raw = group.get("samples")
    if raw is None:
        return None
    if not isinstance(raw, list) or not raw:
        raise KeepGateError(f"group {group.get('name')!r}: samples must be a non-empty list")
    try:
        return [float(v) for v in raw]
    except (TypeError, ValueError) as error:
        raise KeepGateError(f"group {group.get('name')!r}: samples must be numeric") from error


def group_mean(group: dict[str, Any]) -> float:
    samples = _group_samples(group)
    if samples is not None:
        return statistics.fmean(samples)
    for key in ("mean", "mean_ns"):
        if key in group:
            try:
                return float(group[key])
            except (TypeError, ValueError) as error:
                raise KeepGateError(
                    f"group {group.get('name')!r}: {key} must be numeric"
                ) from error
    raise KeepGateError(
        f"group {group.get('name')!r}: need samples or mean/mean_ns"
    )


def group_cv_pct(group: dict[str, Any]) -> float:
    samples = _group_samples(group)
    if samples is not None:
        return cv_pct(samples)
    if "cv_pct" in group:
        try:
            return float(group["cv_pct"])
        except (TypeError, ValueError) as error:
            raise KeepGateError(
                f"group {group.get('name')!r}: cv_pct must be numeric"
            ) from error
    raise KeepGateError(
        f"group {group.get('name')!r}: need samples or cv_pct to quarantine"
    )


def geomean(values: list[float]) -> float:
    if not values:
        raise KeepGateError("geomean requires at least one positive value")
    if any(v <= 0 for v in values):
        raise KeepGateError("geomean requires strictly positive values")
    return math.exp(statistics.fmean(math.log(v) for v in values))


def quarantine_groups(
    groups: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Split groups by CV_PCT_QUARANTINE. Noisy groups are never averaged in."""
    kept: list[dict[str, Any]] = []
    quarantined: list[dict[str, Any]] = []
    for group in groups:
        if not isinstance(group, dict) or "name" not in group:
            raise KeepGateError("each group must be an object with a name")
        cv = group_cv_pct(group)
        if cv > CV_PCT_QUARANTINE:
            quarantined.append(group)
        else:
            kept.append(group)
    if not kept:
        names = [str(g.get("name")) for g in quarantined]
        raise KeepGateError(
            "refuse: all primary groups quarantined "
            f"(cv_pct > {CV_PCT_QUARANTINE}): {names}"
        )
    return kept, quarantined


def load_history(path: Path) -> dict[str, Any]:
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except OSError as error:
        raise KeepGateError(f"cannot read {path}: {error}") from error
    except json.JSONDecodeError as error:
        raise KeepGateError(f"invalid JSON in {path}: {error}") from error
    if not isinstance(document, dict):
        raise KeepGateError(f"{path}: root must be a JSON object")
    if document.get("schema") != SCHEMA:
        raise KeepGateError(
            f"{path}: schema must be {SCHEMA!r}, got {document.get('schema')!r}"
        )
    groups = document.get("groups")
    if not isinstance(groups, list) or not groups:
        raise KeepGateError(f"{path}: groups must be a non-empty list")
    return document


def _index_by_name(groups: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    indexed: dict[str, dict[str, Any]] = {}
    for group in groups:
        name = str(group["name"])
        if name in indexed:
            raise KeepGateError(f"duplicate group name {name!r}")
        indexed[name] = group
    return indexed


def _regression_pct(current: float, baseline: float) -> float:
    """Positive when current is slower (worse) than baseline."""
    if baseline <= 0:
        raise KeepGateError("baseline mean must be positive")
    return ((current - baseline) / baseline) * 100.0


def compare_to_history(
    current: dict[str, Any],
    history: dict[str, Any],
    *,
    geomean_band_pct: float = KEEP_GATE_GEOMEAN_PCT,
    pass_band_pct: float = KEEP_GATE_PASS_PCT,
) -> tuple[bool, list[str]]:
    """Keep-gate: quarantine, then geomean / per-pass bands. Lower latency wins."""
    messages: list[str] = []
    current_groups = current.get("groups")
    history_groups = history.get("groups")
    if not isinstance(current_groups, list) or not current_groups:
        raise KeepGateError("current: groups must be a non-empty list")
    if not isinstance(history_groups, list) or not history_groups:
        raise KeepGateError("history: groups must be a non-empty list")

    kept_current, quarantined = quarantine_groups(current_groups)
    if quarantined:
        names = [str(g["name"]) for g in quarantined]
        messages.append(
            f"quarantined cv_pct>{CV_PCT_QUARANTINE}: {', '.join(names)}"
        )

    # History noisy groups are also excluded from the compare denominator.
    kept_history, _ = quarantine_groups(list(history_groups))
    hist_kept = _index_by_name(kept_history)
    cur_kept = _index_by_name(kept_current)

    shared = sorted(set(cur_kept) & set(hist_kept))
    if not shared:
        raise KeepGateError(
            "refuse: no shared non-quarantined groups between current and history"
        )

    cur_means = [group_mean(cur_kept[name]) for name in shared]
    hist_means = [group_mean(hist_kept[name]) for name in shared]
    passed = True

    for name, cur_m, hist_m in zip(shared, cur_means, hist_means, strict=True):
        reg = _regression_pct(cur_m, hist_m)
        if reg > pass_band_pct:
            passed = False
            messages.append(
                f"FAIL pass {name}: +{reg:.4f}% vs history "
                f"(band {pass_band_pct}%, current={cur_m}, history={hist_m})"
            )
        else:
            messages.append(
                f"PASS pass {name}: {reg:+.4f}% vs history "
                f"(band {pass_band_pct}%)"
            )

    cur_geo = geomean(cur_means)
    hist_geo = geomean(hist_means)
    geo_reg = _regression_pct(cur_geo, hist_geo)
    if geo_reg > geomean_band_pct:
        passed = False
        messages.append(
            f"FAIL geomean: +{geo_reg:.4f}% vs history "
            f"(band {geomean_band_pct}%, current={cur_geo}, history={hist_geo})"
        )
    else:
        messages.append(
            f"PASS geomean: {geo_reg:+.4f}% vs history "
            f"(band {geomean_band_pct}%)"
        )

    return passed, messages


def persist_gate(
    current: dict[str, Any],
    history: dict[str, Any],
    *,
    geomean_band_pct: float = KEEP_GATE_GEOMEAN_PCT,
) -> tuple[bool, list[str]]:
    """Persist uses the same 3% geomean constant as keep-gate (not 25%)."""
    return compare_to_history(
        current,
        history,
        geomean_band_pct=geomean_band_pct,
        pass_band_pct=geomean_band_pct,
    )


def detect_binary_os(path: Path) -> str:
    """Return 'linux' or 'darwin' from magic bytes (not file(1) alone)."""
    try:
        with path.open("rb") as handle:
            magic = handle.read(4)
    except OSError as error:
        raise KeepGateError(f"cannot read binary {path}: {error}") from error
    if len(magic) < 4:
        raise KeepGateError(f"{path}: file too short to detect binary OS")
    if magic == ELF_MAGIC:
        return "linux"
    if magic in MACHO_MAGICS:
        return "darwin"
    raise KeepGateError(
        f"{path}: unrecognized binary magic {magic.hex()} "
        "(expected ELF or Mach-O)"
    )


def host_os() -> str:
    """Host OS from rustc -vV host when available, else sys.platform."""
    try:
        completed = subprocess.run(
            ["rustc", "-vV"],
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError:
        completed = None
    if completed is not None and completed.returncode == 0:
        for line in completed.stdout.splitlines():
            if line.startswith("host:"):
                triple = line.split(":", 1)[1].strip()
                if "apple-darwin" in triple or triple.endswith("-darwin"):
                    return "darwin"
                if "linux" in triple:
                    return "linux"
                break
    platform = sys.platform
    if platform == "darwin":
        return "darwin"
    if platform.startswith("linux"):
        return "linux"
    raise KeepGateError(f"unsupported host platform {platform!r}")


def resolve_tokenzero_bin() -> Path:
    """TOKENZERO_BIN wins; otherwise host install / PATH. Refuse OS mismatch."""
    env = os.environ.get("TOKENZERO_BIN")
    if env:
        path = Path(env).expanduser()
    else:
        candidates = [
            Path.home() / ".tokenzero" / "bin" / "tokenzero",
        ]
        which = shutil.which("tokenzero")
        if which:
            candidates.append(Path(which))
        path = next((c for c in candidates if c.is_file()), None)
        if path is None:
            raise KeepGateError(
                "TOKENZERO_BIN unset and no host tokenzero binary found "
                "(tried ~/.tokenzero/bin/tokenzero and PATH)"
            )

    if not path.is_file():
        raise KeepGateError(f"tokenzero binary not found: {path}")

    binary = detect_binary_os(path)
    host = host_os()
    if binary != host:
        raise KeepGateError(
            f"refuse: host OS is {host} but binary {path} is {binary} "
            "(ELF vs Mach-O mixup; set TOKENZERO_BIN to a host-native binary)"
        )
    return path


def _cmd_compare(args: argparse.Namespace) -> int:
    current = load_history(args.current)
    history = load_history(args.history)
    passed, messages = compare_to_history(current, history)
    for line in messages:
        print(line)
    print("Result: PASS" if passed else "Result: FAIL")
    return 0 if passed else 1


def _cmd_persist(args: argparse.Namespace) -> int:
    current = load_history(args.current)
    history = load_history(args.history)
    passed, messages = persist_gate(current, history)
    for line in messages:
        print(line)
    print(
        f"persist-gate band={KEEP_GATE_GEOMEAN_PCT}% "
        f"(KEEP_GATE_GEOMEAN_PCT)"
    )
    print("Result: PASS" if passed else "Result: FAIL")
    return 0 if passed else 1


def _cmd_resolve_bin(_args: argparse.Namespace) -> int:
    path = resolve_tokenzero_bin()
    print(path)
    return 0


def _cmd_dry_run(_args: argparse.Namespace) -> int:
    print("keep_gate dry-run")
    print(f"schema={SCHEMA}")
    print(f"KEEP_GATE_GEOMEAN_PCT={KEEP_GATE_GEOMEAN_PCT}")
    print(f"KEEP_GATE_PASS_PCT={KEEP_GATE_PASS_PCT}")
    print(f"CV_PCT_QUARANTINE={CV_PCT_QUARANTINE}")
    print(
        "cargo bench: rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_tokenzero "
        "cargo bench -p tokenzero-core --bench hotpaths --profile release-perf"
    )
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="keep_gate.py",
        description=(
            "TokenZero keep-gate: cv_pct quarantine, .bench-history ratchet, "
            "host-native binary resolve."
        ),
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="print named constants and cargo bench invocation, then exit",
    )
    sub = parser.add_subparsers(dest="command")

    compare = sub.add_parser(
        "compare",
        help=(
            f"keep-gate vs history (geomean {KEEP_GATE_GEOMEAN_PCT}%% / "
            f"pass {KEEP_GATE_PASS_PCT}%%)"
        ),
    )
    compare.add_argument("--current", type=Path, required=True)
    compare.add_argument("--history", type=Path, required=True)
    compare.set_defaults(func=_cmd_compare)

    persist = sub.add_parser(
        "persist",
        help=f"persist-gate vs history (geomean band {KEEP_GATE_GEOMEAN_PCT}%%)",
    )
    persist.add_argument("--current", type=Path, required=True)
    persist.add_argument("--history", type=Path, required=True)
    persist.set_defaults(func=_cmd_persist)

    resolve = sub.add_parser(
        "resolve-bin",
        help="print host-native TOKENZERO_BIN path or refuse OS mismatch",
    )
    resolve.set_defaults(func=_cmd_resolve_bin)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.dry_run:
        return _cmd_dry_run(args)
    if not getattr(args, "command", None):
        parser.print_help()
        return 0
    try:
        return int(args.func(args))
    except KeepGateError as error:
        print(f"keep_gate: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
