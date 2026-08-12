#!/usr/bin/env python3
"""Build the public binaries and run only the bounded top-level ship suite."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def run(command: list[str], *, env: dict[str, str] | None = None) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def target_directory() -> Path:
    output = subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
    )
    return Path(json.loads(output)["target_directory"])


def binary_environment() -> dict[str, str]:
    suffix = ".exe" if os.name == "nt" else ""
    target = target_directory() / "debug"
    env = os.environ.copy()
    env["TOKENZERO_SHIP_BIN"] = os.fspath(target / f"tokenzero{suffix}")
    env["TOKENZERO_SHIP_WORKER_BIN"] = os.fspath(target / f"tokenzero-codemode{suffix}")
    return env


def build(target: str) -> None:
    if target in {"all", "ship"}:
        run(
            [
                "cargo",
                "build",
                "--locked",
                "-p",
                "tokenzero-cli",
                "--bin",
                "tokenzero",
                "--no-default-features",
            ]
        )
    if target in {"all", "ship-worker"}:
        run(
            [
                "cargo",
                "build",
                "--locked",
                "-p",
                "tokenzero-worker",
                "--bin",
                "tokenzero-codemode",
                "--no-default-features",
            ]
        )


def test(target: str, exact: str | None) -> None:
    targets = ["ship", "ship-worker"] if target == "all" else [target]
    env = binary_environment()
    for test_target in targets:
        command = [
            "cargo",
            "test",
            "--locked",
            "-p",
            "tokenzero-ship-tests",
            "--test",
            test_target,
        ]
        if exact:
            command.append(exact)
        command.extend(["--", "--test-threads=1"])
        if exact:
            command.append("--exact")
        run(command, env=env)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", choices=("all", "ship", "ship-worker"), default="all")
    parser.add_argument("--exact")
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    if args.exact and args.target == "all":
        parser.error("--exact requires --target ship or ship-worker")
    if not args.skip_build:
        build(args.target)
    test(args.target, args.exact)
    return 0


if __name__ == "__main__":
    sys.exit(main())
