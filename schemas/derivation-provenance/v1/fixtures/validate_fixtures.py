#!/usr/bin/env python3
"""Validate derivation-provenance v1 schema and golden fixtures using only stdlib."""
from __future__ import annotations

import json
import re
import sys
from datetime import datetime
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
SCHEMAS = HERE.parent
SCHEMA_NAME = "derivation-provenance.schema.json"
PHASE_A_ENGINE = {"tokenzero", "fszero", "graphzero"}


def fail(message: str) -> None:
    raise AssertionError(message)


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as stream:
        return json.load(stream)


def type_matches(value: Any, expected: str) -> bool:
    if expected == "object":
        return isinstance(value, dict)
    if expected == "array":
        return isinstance(value, list)
    if expected == "string":
        return isinstance(value, str)
    if expected == "integer":
        return isinstance(value, int) and not isinstance(value, bool)
    if expected == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    if expected == "boolean":
        return isinstance(value, bool)
    if expected == "null":
        return value is None
    fail(f"unsupported schema type: {expected}")


def validate(instance: Any, schema: dict[str, Any], where: str = "$") -> list[str]:
    errors: list[str] = []
    expected_type = schema.get("type")
    if expected_type and not type_matches(instance, expected_type):
        return [f"{where}: expected {expected_type}, got {type(instance).__name__}"]
    if "const" in schema and instance != schema["const"]:
        errors.append(f"{where}: expected constant {schema['const']!r}")
    if "enum" in schema and instance not in schema["enum"]:
        errors.append(f"{where}: value {instance!r} is not in enum")
    if isinstance(instance, dict):
        required = schema.get("required", [])
        errors.extend(
            f"{where}: missing required key {key!r}" for key in required if key not in instance
        )
        properties = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            errors.extend(
                f"{where}: unexpected key {key!r}" for key in instance if key not in properties
            )
        for key, value in instance.items():
            if key in properties:
                errors.extend(validate(value, properties[key], f"{where}.{key}"))
    if isinstance(instance, str):
        if len(instance) < schema.get("minLength", 0):
            errors.append(f"{where}: shorter than minLength")
        if "maxLength" in schema and len(instance) > schema["maxLength"]:
            errors.append(f"{where}: longer than maxLength")
        if "pattern" in schema and re.fullmatch(schema["pattern"], instance) is None:
            errors.append(f"{where}: does not match {schema['pattern']!r}")
        if schema.get("format") == "date-time":
            try:
                datetime.fromisoformat(instance.replace("Z", "+00:00"))
            except ValueError:
                errors.append(f"{where}: invalid date-time")
    if isinstance(instance, (int, float)) and not isinstance(instance, bool):
        if "minimum" in schema and instance < schema["minimum"]:
            errors.append(f"{where}: below minimum")
        if "maximum" in schema and instance > schema["maximum"]:
            errors.append(f"{where}: above maximum")
    return errors


def phase_a_path_valid(record: dict[str, Any], store_path: str) -> bool:
    parts = Path(store_path).parts
    if len(parts) != 3:
        return False
    engine, folder, filename = parts
    if engine not in PHASE_A_ENGINE or folder != "provenance":
        return False
    if filename != f"{record.get('row_id', '')}.json":
        return False
    if record.get("producing_engine") != engine:
        return False
    return True


def main() -> int:
    schema_path = SCHEMAS / SCHEMA_NAME
    if not schema_path.is_file():
        fail(f"missing schema {schema_path}")
    schema = load_json(schema_path)
    if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
        fail("wrong meta-schema")
    if not schema.get("$id"):
        fail("missing $id")
    version_schema = schema["properties"]["schema_version"]
    if version_schema.get("const") != "zerostack.derivation-provenance.v1":
        fail("schema_version const must be zerostack.derivation-provenance.v1")
    # Must not accept cas-gc ids as schema_version values (orthogonal freeze).
    allowed = version_schema.get("const") or version_schema.get("enum")
    if isinstance(allowed, list) and any(
        isinstance(v, str) and v.startswith("zerostack.cas-gc.") for v in allowed
    ):
        fail("schema_version must not accept cas-gc.* freeze ids")
    if isinstance(allowed, str) and allowed.startswith("zerostack.cas-gc."):
        fail("schema_version must not be a cas-gc.* freeze id")

    expectations = load_json(HERE / "expectations.json")
    cases = expectations.get("cases")
    if not isinstance(cases, list) or not cases:
        fail("cases must be non-empty")
    case_by_file = {case["file"]: case for case in cases}
    if len(case_by_file) != len(cases):
        fail("duplicate case file")
    fixture_names = {
        path.name
        for path in HERE.iterdir()
        if path.is_file() and path.name not in {"expectations.json", "validate_fixtures.py"}
    }
    if fixture_names != set(case_by_file):
        fail(f"fixture/expectation mismatch: {fixture_names ^ set(case_by_file)}")

    valid_count = 0
    invalid_count = 0
    for name, case in case_by_file.items():
        fixture = HERE / name
        record = load_json(fixture)
        errors = validate(record, schema)
        expected_valid = case.get("schema_valid")
        if not isinstance(expected_valid, bool):
            fail(f"{name}: schema_valid missing")
        if (not errors) != expected_valid:
            fail(f"{fixture}: schema validity mismatch; errors={errors}")
        if expected_valid:
            valid_count += 1
            if record.get("schema_version") != "zerostack.derivation-provenance.v1":
                fail(f"{fixture}: valid fixture must use frozen schema id")
        else:
            invalid_count += 1
        if "store_path" in case:
            actual = phase_a_path_valid(record, case["store_path"])
            expected_path = case.get("path_valid", True)
            if actual != expected_path:
                fail(
                    f"{name}: path expectation mismatch for {case['store_path']}: "
                    f"actual={actual}, expected={expected_path}"
                )

    print(f"validated 1 schema ({SCHEMA_NAME})")
    print(f"validated {len(cases)} fixtures ({valid_count} valid, {invalid_count} invalid)")
    print("validated Phase A engine-private path grammar; Phase B gc/provenance deferred")
    print("derivation-provenance v1 fixtures: OK")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, KeyError, OSError, json.JSONDecodeError) as error:
        print(f"validation failed: {error}", file=sys.stderr)
        raise SystemExit(1)
