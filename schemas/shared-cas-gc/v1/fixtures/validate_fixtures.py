#!/usr/bin/env python3
"""Validate shared-CAS GC v1 schemas and golden fixtures using only stdlib."""
from __future__ import annotations

import json; import re; import sys; from datetime import datetime, timedelta
from pathlib import Path; from typing import Any

HERE = Path(__file__).resolve().parent; SCHEMAS = HERE.parent; VERDICTS = {"retain", "collect", "retain-uncertain"}
UNCERTAIN_REASONS = {
    "unknown-version", "corrupt-metadata", "uncertain-metadata", "unpublished-temp"
}


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
    errors: list[str] = []; expected_type = schema.get("type")
    if expected_type and not type_matches(instance, expected_type):
        return [f"{where}: expected {expected_type}, got {type(instance).__name__}"]
    if "const" in schema and instance != schema["const"]:
        errors.append(f"{where}: expected constant {schema['const']!r}")
    if "enum" in schema and instance not in schema["enum"]:
        errors.append(f"{where}: value {instance!r} is not in enum")
    if isinstance(instance, dict):
        required = schema.get("required", []); errors.extend(f"{where}: missing required key {key!r}" for key in required if key not in instance); properties = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            errors.extend(f"{where}: unexpected key {key!r}" for key in instance if key not in properties)
        for key, value in instance.items():
            if key in properties:
                errors.extend(validate(value, properties[key], f"{where}.{key}"))
    if isinstance(instance, list):
        if len(instance) < schema.get("minItems", 0):
            errors.append(f"{where}: fewer than minItems")
        if "maxItems" in schema and len(instance) > schema["maxItems"]:
            errors.append(f"{where}: more than maxItems")
        if schema.get("uniqueItems"):
            encoded = [json.dumps(item, sort_keys=True, separators=(",", ":")) for item in instance]
            if len(encoded) != len(set(encoded)):
                errors.append(f"{where}: items are not unique")
        if "items" in schema:
            for index, value in enumerate(instance):
                errors.extend(validate(value, schema["items"], f"{where}[{index}]"))
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


def check_path(record: dict[str, Any], store_path: str, path_valid: bool) -> None:
    parts = Path(store_path).parts; actual = False
    if record.get("record_type") == "reachability-snapshot" and len(parts) == 5:
        actual = parts[:2] == ("gc", "roots") and parts[2:4] == (record.get("engine"), record.get("project_id")) and (parts[4] == "current.json" or (parts[4].startswith(".current.") and parts[4].endswith(".tmp")))
    elif record.get("record_type") == "pin" and len(parts) == 5:
        actual = parts[:2] == ("gc", "pins") and parts[2:4] == (record.get("engine"), record.get("project_id")) and parts[4] == record.get("pin_id", "") + ".json"
    elif record.get("record_type") == "lease" and len(parts) == 5:
        actual = parts[:2] == ("gc", "leases") and parts[2:4] == (record.get("engine"), record.get("project_id")) and parts[4] == record.get("operation_id", "") + ".json"
    if actual != path_valid:
        fail(f"path expectation mismatch for {store_path}: actual={actual}, expected={path_valid}")


def check_report_semantics(report: dict[str, Any], path: Path) -> None:
    for item in report.get("objects", []):
        verdict = item.get("verdict"); reasons = set(item.get("reason_codes", []))
        if verdict == "collect" and reasons != {"no-live-reference"}:
            fail(f"{path}: collect requires only no-live-reference")
        if reasons & UNCERTAIN_REASONS and verdict != "retain-uncertain":
            fail(f"{path}: uncertainty reason requires retain-uncertain")


def main() -> int:
    schema_paths = sorted(SCHEMAS.glob("*.schema.json"))
    if len(schema_paths) != 4:
        fail(f"expected 4 schemas, found {len(schema_paths)}")
    schemas = {path.name: load_json(path) for path in schema_paths}
    for name, schema in schemas.items():
        if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
            fail(f"{name}: wrong meta-schema")
        if not schema.get("$id"):
            fail(f"{name}: missing $id")

    contract_dirs = sorted(path for path in HERE.iterdir() if path.is_dir())
    if len(contract_dirs) != 8:
        fail(f"expected 8 contract directories, found {len(contract_dirs)}")

    fixture_count = 0; case_count = 0; all_contracts: list[str] = []; loaded_by_dir: dict[Path, dict[str, Any]] = {}
    for directory in contract_dirs:
        expectations_path = directory / "expectations.json"; expectations = load_json(expectations_path); all_contracts.append(expectations["contract_point"]); cases = expectations.get("cases")
        if not isinstance(cases, list) or not cases:
            fail(f"{expectations_path}: cases must be non-empty")
        case_by_file = {case["file"]: case for case in cases}
        if len(case_by_file) != len(cases):
            fail(f"{expectations_path}: duplicate case file")
        fixture_names = {path.name for path in directory.iterdir() if path.is_file() and path.name != "expectations.json"}
        if fixture_names != set(case_by_file):
            fail(f"{directory}: fixture/expectation mismatch: {fixture_names ^ set(case_by_file)}")
        loaded: dict[str, Any] = {}
        for name, case in case_by_file.items():
            case_count += 1; fixture_count += 1
            if case.get("verdict") not in VERDICTS:
                fail(f"{expectations_path}: invalid or missing verdict for {name}")
            reasons = case.get("reason_codes")
            if not isinstance(reasons, list) or not reasons:
                fail(f"{expectations_path}: missing reason_codes for {name}")
            fixture = directory / name
            if fixture.suffix == ".corrupt":
                try:
                    load_json(fixture)
                except (json.JSONDecodeError, UnicodeDecodeError):
                    pass
                else:
                    fail(f"{fixture}: expected corrupt JSON")
                if case.get("parse_valid") is not False or case.get("verdict") != "retain-uncertain":
                    fail(f"{fixture}: corrupt fixture must explicitly retain-uncertain")
                continue
            record = load_json(fixture); loaded[name] = record; schema_name = case.get("schema")
            if schema_name not in schemas:
                fail(f"{expectations_path}: unknown schema {schema_name!r}")
            errors = validate(record, schemas[schema_name]); expected_valid = case.get("schema_valid")
            if not isinstance(expected_valid, bool):
                fail(f"{expectations_path}: schema_valid missing for {name}")
            if (not errors) != expected_valid:
                fail(f"{fixture}: schema validity mismatch; errors={errors}")
            if expected_valid and record.get("record_type") == "dry-run-report":
                check_report_semantics(record, fixture)
            if "store_path" in case:
                check_path(record, case["store_path"], case.get("path_valid", True))
        loaded_by_dir[directory] = loaded

    namespace = loaded_by_dir[HERE / "01-namespace-isolation"]; namespace_pairs = {(r["engine"], r["project_id"]) for n, r in namespace.items() if not n.endswith(".invalid.json")}
    if len(namespace_pairs) < 2:
        fail("namespace fixture does not contain independent projects")

    collision_dir = HERE / "07-collision-resistance"; collision = loaded_by_dir[collision_dir]; collision_records = [r for n, r in collision.items() if "project-" in n]; namespaces = {(r["engine"], r["project_id"]) for r in collision_records}
    hashes = {h for r in collision_records for h in r["blob_hashes"]}
    if len({r["engine"] for r in collision_records}) != 2 or len({r["project_id"] for r in collision_records}) != 2 or len(namespaces) != 4 or len(hashes) != 1:
        fail("collision fixture must be a 2-engine x 2-project matrix sharing one hash")

    grace_expectations = load_json(HERE / "04-stale-lease-grace" / "expectations.json"); evaluated = datetime.fromisoformat(grace_expectations["evaluated_at"].replace("Z", "+00:00"))
    for case in grace_expectations["cases"]:
        record = loaded_by_dir[HERE / "04-stale-lease-grace"][case["file"]]; expires = datetime.fromisoformat(record["expires_at"].replace("Z", "+00:00")); inside = evaluated < expires + timedelta(seconds=record["grace_seconds"])
        if inside and case["verdict"] != "retain":
            fail(f"{case['file']}: inside grace must retain")
        if not inside and case["owner_status"] == "unknown" and case["verdict"] != "retain-uncertain":
            fail(f"{case['file']}: unknown owner after grace must retain-uncertain")

    unknown = load_json(HERE / "05-unknown-newer-corrupt" / "expectations.json")
    for case in unknown["cases"]:
        if case["file"] != "known-v1.json" and (case.get("scope") != "whole-store" or case["verdict"] != "retain-uncertain"):
            fail("unknown/newer/corrupt metadata must retain whole store uncertain")

    print(f"validated {len(schemas)} schemas"); print(f"validated {len(contract_dirs)} contract fixture directories"); print(f"validated {fixture_count} fixtures and {case_count} verdict expectations"); print("validated namespace path grammar and 2-engine x 2-project shared-hash isolation")
    print("validated stale-lease grace and whole-store retain-on-uncertainty semantics"); print("shared-CAS GC v1 fixtures: OK")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, KeyError, OSError, json.JSONDecodeError) as error:
        print(f"validation failed: {error}", file=sys.stderr)
        raise SystemExit(1)
