#!/usr/bin/env python3
"""Validate and summarize locked three-mode output benchmark trials.

The harness never invokes a model. It consumes raw, provider-produced trial
bundles so model/tool identity is frozen before any measurement and every raw
attempt remains inspectable.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable

LBI_SCHEMA = "tokenzero.three-mode.lbi.v1"
TASK_SCHEMA = "tokenzero.three-mode.tasks.v1"
TRIAL_SCHEMA = "tokenzero.three-mode.trial.v1"
VERIFIER_SCHEMA = "tokenzero.three-mode.verifier.v1"
REPORT_SCHEMA = "tokenzero.three-mode.report.v1"
MODES = ("full_file", "text_diff", "edit_protocol")
CACHE_STATES = ("cold", "retained")
OBSERVATION_CLASSES = ("exact", "estimated", "billed", "absent")
ATTEMPT_KINDS = ("primary", "repair", "fallback")
OUTCOMES = ("success", "failure")
USAGE_FIELDS = (
    "input_tokens",
    "cached_input_tokens",
    "output_tokens",
)
WORK_FIELDS = (
    "fresh_work_tokens",
    "replayed_tokens",
    "recovery_tokens",
    "overhead_tokens",
    "file_read_bytes",
    "index_query_units",
    "tool_executions",
    "verifier_runs",
    "latency_ms",
)
FRESH_FIELDS = (
    "fresh_work_tokens",
    "replayed_tokens",
    "recovery_tokens",
    "overhead_tokens",
)
ZEP_REQUIRED_FIELDS = {
    "READ": ("r",),
    "REPLACE": ("r", "text"),
    "INSERT": ("at", "text"),
    "DELETE": ("r",),
    "MOVE": ("from", "to"),
    "COPY": ("from", "to"),
    "RENAME": ("sym", "to"),
    "APPLY_PATCH": ("base", "patch"),
    "RUN": ("cmd",),
}


class HarnessError(ValueError):
    """A typed benchmark-bundle validation failure."""


def canonical_bytes(value: object) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def canonical_sha256(value: object) -> str:
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _load_json(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise HarnessError(f"{label}: cannot read JSON: {error}") from error
    if not isinstance(value, dict):
        raise HarnessError(f"{label}: expected a JSON object")
    return value


def _load_jsonl(path: Path) -> list[dict[str, Any]]:
    try:
        lines = path.read_text().splitlines()
    except OSError as error:
        raise HarnessError(f"trials: cannot read JSONL: {error}") from error
    trials: list[dict[str, Any]] = []
    for line_number, line in enumerate(lines, 1):
        if not line.strip():
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError as error:
            raise HarnessError(
                f"trials line {line_number}: invalid JSON: {error}"
            ) from error
        if not isinstance(value, dict):
            raise HarnessError(f"trials line {line_number}: expected a JSON object")
        trials.append(value)
    if not trials:
        raise HarnessError("trials: at least one raw trial is required")
    return trials


def _require_fields(value: dict[str, Any], fields: Iterable[str], context: str) -> None:
    missing = [field for field in fields if field not in value]
    if missing:
        raise HarnessError(f"{context}: missing fields: {', '.join(missing)}")


def _string(value: object, context: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise HarnessError(f"{context}: expected a nonempty string")
    return value


def _integer(value: object, context: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise HarnessError(f"{context}: expected a nonnegative integer")
    return value


def _digest(value: object, context: str) -> str:
    text = _string(value, context)
    if len(text) != 64 or any(char not in "0123456789abcdef" for char in text):
        raise HarnessError(f"{context}: expected a lowercase sha256 digest")
    return text


def _pin_object(
    value: object,
    fields: Iterable[str],
    context: str,
) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise HarnessError(f"{context}: expected an object")
    _require_fields(value, fields, context)
    for field in fields:
        item = value[field]
        if item is None or item == "" or item == [] or item == {}:
            raise HarnessError(f"{context}.{field}: pin must not be empty")
    return value


def _digest_fields(value: dict[str, Any], fields: Iterable[str], context: str) -> None:
    for field in fields:
        _digest(value[field], f"{context}.{field}")


def validate_lbi(lbi: dict[str, Any], task_manifest_sha256: str) -> None:
    _require_fields(
        lbi,
        (
            "schema_version",
            "identity_label",
            "phase",
            "model",
            "decoder",
            "tokenizer",
            "snapshots",
            "task_manifest_sha256",
            "tools",
            "verifier",
            "hardware",
            "setup",
            "fallback",
            "timeouts",
            "resources",
            "accounting",
            "statistics",
        ),
        "lbi",
    )
    if lbi["schema_version"] != LBI_SCHEMA:
        raise HarnessError(f"lbi.schema_version: expected {LBI_SCHEMA}")
    _string(lbi["identity_label"], "lbi.identity_label")
    if lbi["phase"] not in ("exploratory", "confirmatory"):
        raise HarnessError("lbi.phase: expected exploratory or confirmatory")
    _pin_object(
        lbi["model"],
        ("provider", "model_id", "weights_revision", "execution_identity"),
        "lbi.model",
    )
    _pin_object(
        lbi["decoder"],
        ("sampling_law", "random_stream"),
        "lbi.decoder",
    )
    _pin_object(
        lbi["tokenizer"],
        ("tokenizer_id", "revision", "rendering_schema"),
        "lbi.tokenizer",
    )
    snapshots = lbi["snapshots"]
    if not isinstance(snapshots, list) or not snapshots:
        raise HarnessError("lbi.snapshots: expected a nonempty list")
    for index, snapshot in enumerate(snapshots):
        pinned = _pin_object(
            snapshot,
            ("repository", "commit", "tree_sha256"),
            f"lbi.snapshots[{index}]",
        )
        _digest(pinned["tree_sha256"], f"lbi.snapshots[{index}].tree_sha256")
    actual_task_digest = _digest(
        lbi["task_manifest_sha256"],
        "lbi.task_manifest_sha256",
    )
    if actual_task_digest != task_manifest_sha256:
        raise HarnessError(
            "lbi.task_manifest_sha256: does not bind the supplied task manifest"
        )
    tools = _pin_object(
        lbi["tools"], ("interface_digest", "effect_digest"), "lbi.tools"
    )
    _digest_fields(tools, ("interface_digest", "effect_digest"), "lbi.tools")
    verifier = _pin_object(
        lbi["verifier"],
        ("verifier_id", "revision", "command_digest"),
        "lbi.verifier",
    )
    _digest_fields(verifier, ("command_digest",), "lbi.verifier")
    _pin_object(lbi["hardware"], ("host_id", "os", "arch"), "lbi.hardware")
    setup = _pin_object(
        lbi["setup"],
        ("setup_receipt_sha256", "index_receipt_sha256"),
        "lbi.setup",
    )
    _digest_fields(setup, ("setup_receipt_sha256", "index_receipt_sha256"), "lbi.setup")
    for name in ("fallback", "timeouts", "resources"):
        pin = _pin_object(lbi[name], ("policy_digest",), f"lbi.{name}")
        _digest_fields(pin, ("policy_digest",), f"lbi.{name}")
    accounting = _pin_object(
        lbi["accounting"],
        ("contract", "pricing", "pricing_digest", "cost_policy", "cache_states"),
        "lbi.accounting",
    )
    if accounting["contract"] != "fresh-work-vector-v1":
        raise HarnessError("lbi.accounting.contract: expected fresh-work-vector-v1")
    if accounting["cache_states"] != list(CACHE_STATES):
        raise HarnessError("lbi.accounting.cache_states: expected [cold, retained]")
    pricing = _pin_object(
        accounting["pricing"],
        ("currency", "unit", "source", "effective_revision", "rates"),
        "lbi.accounting.pricing",
    )
    if pricing["currency"] != "USD" or pricing["unit"] != "microusd":
        raise HarnessError("lbi.accounting.pricing: expected USD integer microusd")
    rates = pricing["rates"]
    if not isinstance(rates, dict) or not rates:
        raise HarnessError("lbi.accounting.pricing.rates: expected a nonempty object")
    for name, rate in rates.items():
        _string(name, "lbi.accounting.pricing.rates key")
        _integer(rate, f"lbi.accounting.pricing.rates.{name}")
    pricing_digest = _digest(
        accounting["pricing_digest"], "lbi.accounting.pricing_digest"
    )
    if pricing_digest != canonical_sha256(pricing):
        raise HarnessError(
            "lbi.accounting.pricing_digest: does not bind pricing assumptions"
        )
    statistics = _pin_object(
        lbi["statistics"],
        ("seeds", "rule_digest", "exclusions_digest"),
        "lbi.statistics",
    )
    _digest_fields(statistics, ("rule_digest", "exclusions_digest"), "lbi.statistics")
    seeds = statistics["seeds"]
    if not isinstance(seeds, list) or not seeds:
        raise HarnessError("lbi.statistics.seeds: expected a nonempty list")
    normalized = [
        _integer(seed, f"lbi.statistics.seeds[{index}]")
        for index, seed in enumerate(seeds)
    ]
    if len(set(normalized)) != len(normalized):
        raise HarnessError("lbi.statistics.seeds: duplicate seeds are forbidden")


def validate_tasks(
    manifest: dict[str, Any],
    snapshot_digests: set[str],
) -> dict[str, dict[str, Any]]:
    _require_fields(manifest, ("schema_version", "suite_id", "tasks"), "tasks")
    if manifest["schema_version"] != TASK_SCHEMA:
        raise HarnessError(f"tasks.schema_version: expected {TASK_SCHEMA}")
    _string(manifest["suite_id"], "tasks.suite_id")
    values = manifest["tasks"]
    if not isinstance(values, list) or not values:
        raise HarnessError("tasks.tasks: expected a nonempty list")
    tasks: dict[str, dict[str, Any]] = {}
    ranks: set[tuple[str, int]] = set()
    for index, task in enumerate(values):
        context = f"tasks.tasks[{index}]"
        if not isinstance(task, dict):
            raise HarnessError(f"{context}: expected an object")
        _require_fields(
            task,
            (
                "task_id",
                "scale_group",
                "scale_rank",
                "prompt_sha256",
                "snapshot_sha256",
                "expected_artifact_sha256",
            ),
            context,
        )
        task_id = _string(task["task_id"], f"{context}.task_id")
        if task_id in tasks:
            raise HarnessError(f"{context}.task_id: duplicate {task_id}")
        group = _string(task["scale_group"], f"{context}.scale_group")
        rank = _integer(task["scale_rank"], f"{context}.scale_rank")
        if (group, rank) in ranks:
            raise HarnessError(
                f"{context}.scale_rank: duplicate rank {rank} in {group}"
            )
        ranks.add((group, rank))
        _digest(task["prompt_sha256"], f"{context}.prompt_sha256")
        snapshot = _digest(task["snapshot_sha256"], f"{context}.snapshot_sha256")
        if snapshot not in snapshot_digests:
            raise HarnessError(f"{context}.snapshot_sha256: not pinned by the LBI")
        _digest(task["expected_artifact_sha256"], f"{context}.expected_artifact_sha256")
        tasks[task_id] = dict(task)
    return tasks


def validate_observation(value: object, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise HarnessError(f"{context}: expected an observation object")
    _require_fields(value, ("class",), context)
    observation_class = value["class"]
    if observation_class not in OBSERVATION_CLASSES:
        raise HarnessError(
            f"{context}.class: expected one of {', '.join(OBSERVATION_CLASSES)}"
        )
    if observation_class == "absent":
        if "value" in value:
            raise HarnessError(
                f"{context}: absent is not zero; omit value when usage was not reported"
            )
        return {"class": "absent"}
    _require_fields(value, ("value",), context)
    return {
        "class": observation_class,
        "value": _integer(value["value"], f"{context}.value"),
    }


def summarize_observations(values: Iterable[dict[str, Any]]) -> dict[str, Any]:
    observations = list(values)
    classes = Counter(value["class"] for value in observations)
    measured = [value["value"] for value in observations if value["class"] != "absent"]
    return {
        "observed_sum": sum(measured) if measured else None,
        "observed_count": len(measured),
        "absent_count": classes["absent"],
        "complete": classes["absent"] == 0,
        "class_counts": {
            observation_class: classes[observation_class]
            for observation_class in OBSERVATION_CLASSES
        },
        "observed_sum_by_class": {
            observation_class: (
                sum(
                    value["value"]
                    for value in observations
                    if value["class"] == observation_class
                )
                if classes[observation_class]
                else None
            )
            for observation_class in OBSERVATION_CLASSES
            if observation_class != "absent"
        },
    }


def _safe_file(root: Path, relative: object, context: str) -> tuple[str, Path]:
    text = _string(relative, context)
    path = Path(text)
    if path.is_absolute() or ".." in path.parts:
        raise HarnessError(f"{context}: path must stay relative to the trial bundle")
    root_resolved = root.resolve()
    resolved = (root / path).resolve()
    try:
        resolved.relative_to(root_resolved)
    except ValueError as error:
        raise HarnessError(f"{context}: path escapes the trial bundle") from error
    if not resolved.is_file():
        raise HarnessError(f"{context}: file does not exist: {path.as_posix()}")
    return path.as_posix(), resolved


def _validate_zep(raw: bytes, context: str) -> None:
    try:
        plan = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise HarnessError(
            f"{context}: successful edit_protocol output is not JSON"
        ) from error
    if not isinstance(plan, dict) or plan.get("p") != "zep/1":
        raise HarnessError(
            f"{context}: successful edit_protocol output must use p=zep/1"
        )
    ops = plan.get("ops")
    if not isinstance(ops, list) or not ops:
        raise HarnessError(
            f"{context}: successful edit_protocol output needs nonempty ops"
        )
    for index, operation in enumerate(ops):
        if (
            not isinstance(operation, dict)
            or operation.get("v") not in ZEP_REQUIRED_FIELDS
        ):
            raise HarnessError(f"{context}.ops[{index}]: unknown ZEP/1 verb")
        for field in ZEP_REQUIRED_FIELDS[operation["v"]]:
            if not isinstance(operation.get(field), str) or not operation[field]:
                raise HarnessError(
                    f"{context}.ops[{index}].{field}: required ZEP/1 field is empty"
                )
        if "side" in operation and operation["side"] not in ("before", "after"):
            raise HarnessError(f"{context}.ops[{index}].side: expected before or after")


def _validate_verifier(
    path: Path,
    expected_sha256: str,
    outcome: str,
    context: str,
) -> dict[str, Any]:
    receipt = _load_json(path, context)
    _require_fields(
        receipt,
        ("schema_version", "status", "expected_sha256", "actual_sha256"),
        context,
    )
    if receipt["schema_version"] != VERIFIER_SCHEMA:
        raise HarnessError(f"{context}.schema_version: expected {VERIFIER_SCHEMA}")
    if receipt["expected_sha256"] != expected_sha256:
        raise HarnessError(f"{context}: expected digest does not match the task")
    if outcome == "success":
        if receipt["status"] != "pass" or receipt["actual_sha256"] != expected_sha256:
            raise HarnessError(
                f"{context}: successful trial needs a passing exact verifier"
            )
    elif receipt["status"] not in ("fail", "not_run"):
        raise HarnessError(f"{context}: failed trial cannot carry a passing verifier")
    if receipt["actual_sha256"] is not None:
        _digest(receipt["actual_sha256"], f"{context}.actual_sha256")
    return receipt


def _eta_from_summaries(summaries: dict[str, dict[str, Any]]) -> dict[str, Any]:
    if any(not summaries[field]["complete"] for field in FRESH_FIELDS):
        return {"eta_action_ppm": None, "state": "absent_component"}
    components = [summaries[field]["observed_sum"] or 0 for field in FRESH_FIELDS]
    total = sum(components)
    if total == 0:
        return {"eta_action_ppm": None, "state": "undeclared_zero_total"}
    return {
        "eta_action_ppm": components[0] * 1_000_000 // total,
        "state": "declared",
    }


def _validate_attempt(
    attempt: object,
    root: Path,
    context: str,
) -> dict[str, Any]:
    if not isinstance(attempt, dict):
        raise HarnessError(f"{context}: expected an object")
    _require_fields(
        attempt,
        (
            "kind",
            "mode",
            "outcome",
            "raw_output_path",
            "usage",
            "backend_work",
            "total_cost_microusd",
        ),
        context,
    )
    kind = attempt["kind"]
    mode = attempt["mode"]
    outcome = attempt["outcome"]
    if kind not in ATTEMPT_KINDS:
        raise HarnessError(f"{context}.kind: invalid attempt kind")
    if mode not in MODES:
        raise HarnessError(f"{context}.mode: invalid mode")
    if outcome not in OUTCOMES:
        raise HarnessError(f"{context}.outcome: invalid outcome")
    relative, raw_path = _safe_file(
        root, attempt["raw_output_path"], f"{context}.raw_output_path"
    )
    raw = raw_path.read_bytes()
    if mode == "edit_protocol" and outcome == "success":
        _validate_zep(raw, context)
    usage = attempt["usage"]
    backend = attempt["backend_work"]
    if not isinstance(usage, dict) or not isinstance(backend, dict):
        raise HarnessError(f"{context}: usage and backend_work must be objects")
    _require_fields(usage, USAGE_FIELDS, f"{context}.usage")
    _require_fields(backend, WORK_FIELDS, f"{context}.backend_work")
    validated = {
        "kind": kind,
        "mode": mode,
        "outcome": outcome,
        "raw_output": {
            "path": relative,
            "sha256": hashlib.sha256(raw).hexdigest(),
            "bytes": len(raw),
        },
        "usage": {
            field: validate_observation(usage[field], f"{context}.usage.{field}")
            for field in USAGE_FIELDS
        },
        "backend_work": {
            field: validate_observation(
                backend[field], f"{context}.backend_work.{field}"
            )
            for field in WORK_FIELDS
        },
        "total_cost_microusd": validate_observation(
            attempt["total_cost_microusd"],
            f"{context}.total_cost_microusd",
        ),
    }
    input_tokens = validated["usage"]["input_tokens"]
    fresh = [validated["backend_work"][field] for field in FRESH_FIELDS]
    if input_tokens["class"] != "absent" and all(
        item["class"] != "absent" for item in fresh
    ):
        if sum(item["value"] for item in fresh) != input_tokens["value"]:
            raise HarnessError(
                f"{context}: fresh-work components must sum to input_tokens"
            )
    return validated


def _trial_metric_summaries(attempts: list[dict[str, Any]]) -> dict[str, Any]:
    usage = {
        field: summarize_observations(attempt["usage"][field] for attempt in attempts)
        for field in USAGE_FIELDS
    }
    repair_outputs = [
        attempt["usage"]["output_tokens"]
        for attempt in attempts
        if attempt["kind"] == "repair"
    ]
    if not repair_outputs:
        repair_outputs = [{"class": "exact", "value": 0}]
    usage["repair_round_output_tokens"] = summarize_observations(repair_outputs)
    backend = {
        field: summarize_observations(
            attempt["backend_work"][field] for attempt in attempts
        )
        for field in WORK_FIELDS
    }
    return {
        "usage": usage,
        "backend_work": backend,
        "total_cost_microusd": summarize_observations(
            attempt["total_cost_microusd"] for attempt in attempts
        ),
        **_eta_from_summaries(backend),
    }


def validate_trial(
    raw_trial: dict[str, Any],
    root: Path,
    tasks: dict[str, dict[str, Any]],
    lbi_sha256: str,
    seeds: set[int],
    line_number: int,
) -> dict[str, Any]:
    context = f"trials line {line_number}"
    _require_fields(
        raw_trial,
        (
            "schema_version",
            "trial_id",
            "lbi_sha256",
            "task_id",
            "requested_mode",
            "cache_state",
            "seed",
            "outcome",
            "attempts",
            "materialized_artifact_path",
            "verifier_receipt_path",
        ),
        context,
    )
    if raw_trial["schema_version"] != TRIAL_SCHEMA:
        raise HarnessError(f"{context}.schema_version: expected {TRIAL_SCHEMA}")
    trial_id = _string(raw_trial["trial_id"], f"{context}.trial_id")
    if raw_trial["lbi_sha256"] != lbi_sha256:
        raise HarnessError(f"{context}.lbi_sha256: benchmark identity drift")
    task_id = _string(raw_trial["task_id"], f"{context}.task_id")
    if task_id not in tasks:
        raise HarnessError(f"{context}.task_id: not in the locked task manifest")
    requested_mode = raw_trial["requested_mode"]
    if requested_mode not in MODES:
        raise HarnessError(f"{context}.requested_mode: invalid mode")
    cache_state = raw_trial["cache_state"]
    if cache_state not in CACHE_STATES:
        raise HarnessError(f"{context}.cache_state: expected cold or retained")
    seed = _integer(raw_trial["seed"], f"{context}.seed")
    if seed not in seeds:
        raise HarnessError(f"{context}.seed: not pinned by the LBI")
    outcome = raw_trial["outcome"]
    if outcome not in OUTCOMES:
        raise HarnessError(f"{context}.outcome: invalid outcome")
    raw_attempts = raw_trial["attempts"]
    if not isinstance(raw_attempts, list) or not raw_attempts:
        raise HarnessError(f"{context}.attempts: expected a nonempty list")
    attempts = [
        _validate_attempt(attempt, root, f"{context}.attempts[{index}]")
        for index, attempt in enumerate(raw_attempts)
    ]
    if attempts[0]["kind"] != "primary" or attempts[0]["mode"] != requested_mode:
        raise HarnessError(f"{context}.attempts[0]: must be the requested primary mode")
    if sum(attempt["kind"] == "primary" for attempt in attempts) != 1:
        raise HarnessError(
            f"{context}.attempts: exactly one primary attempt is required"
        )
    failed_before = False
    for index, attempt in enumerate(attempts):
        if attempt["kind"] == "fallback":
            if not failed_before or attempt["mode"] == requested_mode:
                raise HarnessError(
                    f"{context}.attempts[{index}]: fallback must follow a failed requested attempt"
                )
        failed_before = failed_before or attempt["outcome"] == "failure"
    if attempts[-1]["outcome"] != outcome:
        raise HarnessError(f"{context}.outcome: must match the final attempt")
    task = tasks[task_id]
    expected_sha256 = task["expected_artifact_sha256"]
    materialized = None
    if outcome == "success":
        relative, materialized_path = _safe_file(
            root,
            raw_trial["materialized_artifact_path"],
            f"{context}.materialized_artifact_path",
        )
        actual_sha256 = file_sha256(materialized_path)
        if actual_sha256 != expected_sha256:
            raise HarnessError(
                f"{context}: materialized artifact does not match the task"
            )
        materialized = {
            "path": relative,
            "sha256": actual_sha256,
            "bytes": materialized_path.stat().st_size,
        }
    elif raw_trial["materialized_artifact_path"] is not None:
        raise HarnessError(
            f"{context}: failed trial must not claim a materialized artifact"
        )
    verifier_relative, verifier_path = _safe_file(
        root,
        raw_trial["verifier_receipt_path"],
        f"{context}.verifier_receipt_path",
    )
    _validate_verifier(verifier_path, expected_sha256, outcome, f"{context}.verifier")
    action_bytes = sum(attempt["raw_output"]["bytes"] for attempt in attempts)
    repair_bytes = sum(
        attempt["raw_output"]["bytes"]
        for attempt in attempts
        if attempt["kind"] == "repair"
    )
    fallback_used = any(attempt["kind"] == "fallback" for attempt in attempts)
    metrics = _trial_metric_summaries(attempts)
    ratio = None
    if materialized is not None and materialized["bytes"] > 0:
        ratio = action_bytes * 1_000_000 // materialized["bytes"]
    return {
        "trial_id": trial_id,
        "task_id": task_id,
        "scale_group": task["scale_group"],
        "scale_rank": task["scale_rank"],
        "requested_mode": requested_mode,
        "cache_state": cache_state,
        "seed": seed,
        "outcome": outcome,
        "fallback_used": fallback_used,
        "attempt_count": len(attempts),
        "failed_attempt_count": sum(
            attempt["outcome"] == "failure" for attempt in attempts
        ),
        "action_description_bytes": action_bytes,
        "repair_round_output_bytes": repair_bytes,
        "action_to_artifact_ppm": ratio,
        "materialized_artifact": materialized,
        "verifier_receipt": {
            "path": verifier_relative,
            "sha256": file_sha256(verifier_path),
        },
        "attempts": attempts,
        "metrics": metrics,
    }


def _aggregate_mode(mode: str, trials: list[dict[str, Any]]) -> dict[str, Any]:
    selected = [trial for trial in trials if trial["requested_mode"] == mode]
    usage = {
        field: summarize_observations(
            attempt["usage"][field]
            for trial in selected
            for attempt in trial["attempts"]
        )
        for field in USAGE_FIELDS
    }
    repair_observations: list[dict[str, Any]] = []
    for trial in selected:
        repairs = [
            attempt["usage"]["output_tokens"]
            for attempt in trial["attempts"]
            if attempt["kind"] == "repair"
        ]
        repair_observations.extend(repairs or [{"class": "exact", "value": 0}])
    usage["repair_round_output_tokens"] = summarize_observations(repair_observations)
    backend = {
        field: summarize_observations(
            attempt["backend_work"][field]
            for trial in selected
            for attempt in trial["attempts"]
        )
        for field in WORK_FIELDS
    }
    successes = [trial for trial in selected if trial["outcome"] == "success"]
    artifact_bytes = sum(trial["materialized_artifact"]["bytes"] for trial in successes)
    action_bytes = sum(trial["action_description_bytes"] for trial in selected)
    successful_action_bytes = sum(
        trial["action_description_bytes"] for trial in successes
    )
    return {
        "mode": mode,
        "denominators": {
            "trials": len(selected),
            "successes": len(successes),
            "failures": len(selected) - len(successes),
            "fallbacks": sum(trial["fallback_used"] for trial in selected),
            "attempts": sum(trial["attempt_count"] for trial in selected),
            "failed_attempts": sum(trial["failed_attempt_count"] for trial in selected),
            "by_cache_state": {
                state: {
                    "trials": sum(trial["cache_state"] == state for trial in selected),
                    "successes": sum(
                        trial["cache_state"] == state and trial["outcome"] == "success"
                        for trial in selected
                    ),
                    "failures": sum(
                        trial["cache_state"] == state and trial["outcome"] == "failure"
                        for trial in selected
                    ),
                    "fallbacks": sum(
                        trial["cache_state"] == state and trial["fallback_used"]
                        for trial in selected
                    ),
                }
                for state in CACHE_STATES
            },
        },
        "success_rate_ppm": len(successes) * 1_000_000 // len(selected),
        "usage": usage,
        "backend_work": backend,
        "total_cost_microusd": summarize_observations(
            attempt["total_cost_microusd"]
            for trial in selected
            for attempt in trial["attempts"]
        ),
        "action_description_bytes": action_bytes,
        "successful_action_description_bytes": successful_action_bytes,
        "materialized_artifact_bytes": artifact_bytes,
        "action_to_artifact_ppm": (
            successful_action_bytes * 1_000_000 // artifact_bytes
            if artifact_bytes
            else None
        ),
        **_eta_from_summaries(backend),
    }


def _frontier_verdict(
    tasks: dict[str, dict[str, Any]],
    trials: list[dict[str, Any]],
) -> dict[str, Any]:
    groups: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for task in tasks.values():
        groups[task["scale_group"]].append(task)
    cells: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for trial in trials:
        cells[(trial["task_id"], trial["requested_mode"])].append(trial)
    results: list[dict[str, Any]] = []
    has_falsifier = False
    eligible_count = 0
    for group_name, group_tasks in sorted(groups.items()):
        ordered = sorted(group_tasks, key=lambda task: task["scale_rank"])
        if len(ordered) < 2:
            results.append(
                {
                    "scale_group": group_name,
                    "eligible": False,
                    "reason": "needs_two_scales",
                }
            )
            continue
        group_trials = [
            trial
            for task in ordered
            for mode in MODES
            for trial in cells[(task["task_id"], mode)]
        ]
        if any(
            trial["outcome"] != "success" or trial["fallback_used"]
            for trial in group_trials
        ):
            has_falsifier = True
            results.append(
                {
                    "scale_group": group_name,
                    "eligible": False,
                    "reason": "failure_or_fallback",
                }
            )
            continue
        eligible_count += 1
        small = ordered[0]
        large = ordered[-1]

        def totals(task: dict[str, Any], mode: str) -> tuple[int, int]:
            values = cells[(task["task_id"], mode)]
            return (
                sum(trial["action_description_bytes"] for trial in values),
                sum(trial["materialized_artifact"]["bytes"] for trial in values),
            )

        full_small, artifact_small = totals(small, "full_file")
        full_large, artifact_large = totals(large, "full_file")
        diff_small, diff_artifact_small = totals(small, "text_diff")
        diff_large, diff_artifact_large = totals(large, "text_diff")
        edit_small, edit_artifact_small = totals(small, "edit_protocol")
        edit_large, edit_artifact_large = totals(large, "edit_protocol")
        if (
            len(
                {
                    (artifact_small, artifact_large),
                    (diff_artifact_small, diff_artifact_large),
                    (edit_artifact_small, edit_artifact_large),
                }
            )
            != 1
        ):
            raise HarnessError(
                f"frontier {group_name}: modes materialized different artifact sizes"
            )
        artifact_growth = artifact_large - artifact_small
        full_growth = full_large - full_small
        diff_growth = diff_large - diff_small
        edit_growth = edit_large - edit_small
        diff_small_ratio = (
            diff_small * 1_000_000 // artifact_small if artifact_small else None
        )
        diff_large_ratio = (
            diff_large * 1_000_000 // artifact_large if artifact_large else None
        )
        edit_small_ratio = (
            edit_small * 1_000_000 // artifact_small if artifact_small else None
        )
        edit_large_ratio = (
            edit_large * 1_000_000 // artifact_large if artifact_large else None
        )
        conditions = {
            "artifact_grew": artifact_growth > 0,
            "full_file_tracked_artifact": full_growth >= artifact_growth,
            "text_diff_grew_slower_than_artifact": diff_growth < artifact_growth,
            "text_diff_ratio_declined": (
                diff_small_ratio is not None
                and diff_large_ratio is not None
                and diff_large_ratio < diff_small_ratio
            ),
            "edit_protocol_grew_slower_than_artifact": edit_growth < artifact_growth,
            "edit_protocol_ratio_declined": (
                edit_small_ratio is not None
                and edit_large_ratio is not None
                and edit_large_ratio < edit_small_ratio
            ),
        }
        supported = all(conditions.values())
        has_falsifier = has_falsifier or not supported
        results.append(
            {
                "scale_group": group_name,
                "eligible": True,
                "small_task": small["task_id"],
                "large_task": large["task_id"],
                "artifact_growth_bytes": artifact_growth,
                "full_file_growth_bytes": full_growth,
                "text_diff_growth_bytes": diff_growth,
                "text_diff_small_action_to_artifact_ppm": diff_small_ratio,
                "text_diff_large_action_to_artifact_ppm": diff_large_ratio,
                "edit_protocol_growth_bytes": edit_growth,
                "edit_protocol_small_action_to_artifact_ppm": edit_small_ratio,
                "edit_protocol_large_action_to_artifact_ppm": edit_large_ratio,
                "conditions": conditions,
                "supported": supported,
            }
        )
    if has_falsifier:
        verdict = "falsified_on_locked_suite"
    elif eligible_count:
        verdict = "supported_on_locked_suite"
    else:
        verdict = "insufficient_locked_scales"
    return {
        "verdict": verdict,
        "claim_scope": "locked_suite_only_not_release",
        "eligible_scale_groups": eligible_count,
        "groups": results,
    }


def build_report(lbi_path: Path, tasks_path: Path, trials_path: Path) -> dict[str, Any]:
    lbi = _load_json(lbi_path, "lbi")
    task_manifest = _load_json(tasks_path, "tasks")
    task_manifest_sha256 = canonical_sha256(task_manifest)
    validate_lbi(lbi, task_manifest_sha256)
    lbi_sha256 = canonical_sha256(lbi)
    snapshot_digests = {snapshot["tree_sha256"] for snapshot in lbi["snapshots"]}
    tasks = validate_tasks(task_manifest, snapshot_digests)
    raw_trials = _load_jsonl(trials_path)
    seeds = set(lbi["statistics"]["seeds"])
    root = trials_path.parent
    trials = [
        validate_trial(trial, root, tasks, lbi_sha256, seeds, index)
        for index, trial in enumerate(raw_trials, 1)
    ]
    trial_ids = [trial["trial_id"] for trial in trials]
    if len(set(trial_ids)) != len(trial_ids):
        raise HarnessError("trials: duplicate trial_id")
    expected_cells = {
        (task_id, mode, seed, cache_state)
        for task_id in tasks
        for mode in MODES
        for seed in seeds
        for cache_state in CACHE_STATES
    }
    actual_cells = Counter(
        (
            trial["task_id"],
            trial["requested_mode"],
            trial["seed"],
            trial["cache_state"],
        )
        for trial in trials
    )
    duplicates = [cell for cell, count in actual_cells.items() if count != 1]
    missing = sorted(expected_cells - set(actual_cells))
    extra = sorted(set(actual_cells) - expected_cells)
    if duplicates or missing or extra:
        raise HarnessError(
            "trials: paired coverage mismatch "
            f"duplicates={duplicates} missing={missing} extra={extra}"
        )
    mode_reports = [_aggregate_mode(mode, trials) for mode in MODES]
    return {
        "schema_version": REPORT_SCHEMA,
        "evidence_phase": lbi["phase"],
        "claim_scope": "locked_suite_only_not_release",
        "lbi_sha256": lbi_sha256,
        "locked_benchmark_identity": lbi,
        "task_manifest": {
            "suite_id": task_manifest["suite_id"],
            "sha256": task_manifest_sha256,
            "task_count": len(tasks),
        },
        "raw_trials": {
            "file": trials_path.name,
            "sha256": file_sha256(trials_path),
            "trial_count": len(trials),
            "retained": True,
        },
        "denominators": {
            "trials": len(trials),
            "successes": sum(trial["outcome"] == "success" for trial in trials),
            "failures": sum(trial["outcome"] == "failure" for trial in trials),
            "fallbacks": sum(trial["fallback_used"] for trial in trials),
            "attempts": sum(trial["attempt_count"] for trial in trials),
            "failed_attempts": sum(trial["failed_attempt_count"] for trial in trials),
            "by_cache_state": {
                state: sum(trial["cache_state"] == state for trial in trials)
                for state in CACHE_STATES
            },
        },
        "modes": mode_reports,
        "frontier": _frontier_verdict(tasks, trials),
        "trials": trials,
    }


def atomic_write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    rendered = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    with tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        dir=path.parent,
        prefix=f".{path.name}.",
        delete=False,
    ) as stream:
        temporary = Path(stream.name)
        stream.write(rendered)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Validate locked full-file/diff/ZEP benchmark trials"
    )
    parser.add_argument("--lbi", type=Path, required=True)
    parser.add_argument("--tasks", type=Path, required=True)
    parser.add_argument("--trials", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument(
        "--require-supported",
        action="store_true",
        help="exit 3 unless the locked-suite frontier verdict is supported",
    )
    args = parser.parse_args(argv)
    try:
        report = build_report(args.lbi, args.tasks, args.trials)
        atomic_write_json(args.out, report)
    except HarnessError as error:
        print(f"three-mode-output: {error}", file=sys.stderr)
        return 2
    verdict = report["frontier"]["verdict"]
    print(
        json.dumps(
            {
                "report": args.out.name,
                "lbi_sha256": report["lbi_sha256"],
                "verdict": verdict,
                "denominators": report["denominators"],
            },
            sort_keys=True,
        )
    )
    if args.require_supported and verdict != "supported_on_locked_suite":
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
