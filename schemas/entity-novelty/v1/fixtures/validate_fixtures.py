#!/usr/bin/env python3
"""Validate entity-novelty v1 schema and fixtures (stdlib only)."""
from __future__ import annotations

import json
import re
import sys
from datetime import datetime
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
SCHEMA = HERE.parent / "entity-novelty.schema.json"
HEX64 = re.compile(r"^[0-9a-f]{64}$")
SCOPE = re.compile(r"^(session|repo|workspace):.+|^global$")
ENGINES = {"tokenzero", "fszero", "graphzero"}


def fail(msg: str) -> None:
    raise AssertionError(msg)


def load(path: Path) -> Any:
    with path.open(encoding="utf-8") as fh:
        return json.load(fh)


def validate_record(obj: Any) -> list[str]:
    errors: list[str] = []
    if not isinstance(obj, dict):
        return ["$: expected object"]
    required = {
        "schema_version",
        "record_type",
        "scope_key",
        "entity_ids",
        "producing_engine",
        "updated_at",
    }
    for key in required:
        if key not in obj:
            errors.append(f"$: missing {key}")
    if obj.get("schema_version") != "zerostack.entity-novelty.v1":
        errors.append("schema_version mismatch")
    if obj.get("record_type") != "entity-novelty":
        errors.append("record_type mismatch")
    scope = obj.get("scope_key")
    if not isinstance(scope, str) or not SCOPE.match(scope):
        errors.append("bad scope_key")
    ids = obj.get("entity_ids")
    if not isinstance(ids, list):
        errors.append("entity_ids must be array")
    else:
        seen = set()
        for i, eid in enumerate(ids):
            if not isinstance(eid, str) or not HEX64.match(eid):
                errors.append(f"entity_ids[{i}] must be 64-hex (no scheme prefix)")
            elif eid in seen:
                errors.append(f"entity_ids[{i}] duplicate")
            else:
                seen.add(eid)
            if isinstance(eid, str) and "://" in eid:
                errors.append(f"entity_ids[{i}] must not include a URI scheme")
    if obj.get("producing_engine") not in ENGINES:
        errors.append("bad producing_engine")
    updated = obj.get("updated_at")
    if isinstance(updated, str):
        try:
            datetime.fromisoformat(updated.replace("Z", "+00:00"))
        except ValueError:
            errors.append("bad updated_at")
    else:
        errors.append("updated_at required")
    cas = obj.get("cas_digest")
    if cas is not None and (not isinstance(cas, str) or not HEX64.match(cas)):
        errors.append("bad cas_digest")
    allowed = required | {"cas_digest"}
    for key in obj:
        if key not in allowed:
            errors.append(f"unexpected key {key}")
    return errors


def main() -> int:
    if not SCHEMA.is_file():
        fail(f"missing schema {SCHEMA}")
    load(SCHEMA)
    valid = HERE / "valid-minimal.json"
    invalid = HERE / "scheme-prefix.invalid.json"
    verr = validate_record(load(valid))
    if verr:
        fail(f"valid fixture failed: {verr}")
    ierr = validate_record(load(invalid))
    if not ierr:
        fail("scheme-prefix fixture should be invalid")
    print("entity-novelty fixtures: ok")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AssertionError as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
