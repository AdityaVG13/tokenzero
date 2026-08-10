#!/usr/bin/env python3
"""TokenZero thin adapter for canonical ZeroStack test-policy scripts."""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
from pathlib import Path
from types import ModuleType

ENGINE_ROOT = Path(__file__).resolve().parent.parent
EXPECTED_HUB_REV = "fa253840910ab4051635e2de95f04ddf6043a000"


def hub_root() -> Path:
    """Resolve the immutable ZeroStack source already pinned by Cargo.lock."""
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1"],
        cwd=ENGINE_ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    packages = json.loads(result.stdout)["packages"]
    for package in packages:
        source = package.get("source") or ""
        if package["name"] != "zero-abi" or EXPECTED_HUB_REV not in source:
            continue
        root = Path(package["manifest_path"]).resolve().parents[2]
        if (root / "scripts" / "check-portability.sh").is_file():
            return root
    raise RuntimeError(
        f"Cargo metadata does not contain ZeroStack revision {EXPECTED_HUB_REV}"
    )


def load_hub_script(script_name: str, module_name: str) -> ModuleType:
    """Load one canonical script without copying its policy implementation."""
    path = hub_root() / "scripts" / script_name
    spec = importlib.util.spec_from_file_location(module_name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load canonical ZeroStack script: {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module
