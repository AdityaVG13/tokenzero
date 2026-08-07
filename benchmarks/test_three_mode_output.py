from __future__ import annotations

import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from benchmarks.three_mode_output import (
    CACHE_STATES,
    HarnessError,
    LBI_SCHEMA,
    TASK_SCHEMA,
    TRIAL_SCHEMA,
    VERIFIER_SCHEMA,
    build_report,
    canonical_sha256,
    main,
)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def observation(value: int = 1, classification: str = "exact") -> dict[str, object]:
    return {"class": classification, "value": value}


def attempt(kind: str, mode: str, outcome: str, raw_path: str) -> dict[str, object]:
    return {
        "kind": kind,
        "mode": mode,
        "outcome": outcome,
        "raw_output_path": raw_path,
        "usage": {
            "input_tokens": observation(10),
            "cached_input_tokens": observation(0, "billed"),
            "output_tokens": observation(5, "billed"),
        },
        "backend_work": {
            "fresh_work_tokens": observation(2),
            "replayed_tokens": observation(6),
            "recovery_tokens": observation(1),
            "overhead_tokens": observation(1),
            "file_read_bytes": observation(32),
            "index_query_units": observation(1),
            "tool_executions": observation(1),
            "verifier_runs": observation(1),
            "latency_ms": observation(4),
        },
        "total_cost_microusd": observation(100, "billed"),
    }


class Fixture:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.tasks_path = root / "tasks.json"
        self.lbi_path = root / "lbi.json"
        self.trials_path = root / "trials.jsonl"
        snapshot = "1" * 64
        tasks = []
        self.targets: dict[str, Path] = {}
        for rank, size in ((1, 100), (2, 1000)):
            task_id = f"rename-{rank}"
            target = root / f"target-{rank}.txt"
            target.write_bytes(b"A" * size)
            self.targets[task_id] = target
            tasks.append(
                {
                    "task_id": task_id,
                    "scale_group": "same-rename",
                    "scale_rank": rank,
                    "prompt_sha256": digest(f"prompt-{rank}".encode()),
                    "snapshot_sha256": snapshot,
                    "expected_artifact_sha256": digest(target.read_bytes()),
                }
            )
        self.tasks = {
            "schema_version": TASK_SCHEMA,
            "suite_id": "fixture-suite-not-a-product-claim",
            "tasks": tasks,
        }
        self.tasks_path.write_text(json.dumps(self.tasks))
        pricing = {
            "currency": "USD",
            "unit": "microusd",
            "source": "fixture-price-card",
            "effective_revision": "fixture-v1",
            "rates": {
                "input_per_million_tokens": 1_000_000,
                "cached_input_per_million_tokens": 100_000,
                "output_per_million_tokens": 5_000_000,
            },
        }
        self.lbi = {
            "schema_version": LBI_SCHEMA,
            "identity_label": "fixture-only",
            "phase": "exploratory",
            "model": {
                "provider": "fixture",
                "model_id": "fixture-model",
                "weights_revision": "fixture-revision",
                "execution_identity": "deterministic-test",
            },
            "decoder": {
                "sampling_law": "deterministic",
                "random_stream": "seed-7",
            },
            "tokenizer": {
                "tokenizer_id": "fixture-tokenizer",
                "revision": "v1",
                "rendering_schema": "fixture",
            },
            "snapshots": [
                {"repository": "fixture", "commit": "fixture", "tree_sha256": snapshot}
            ],
            "task_manifest_sha256": canonical_sha256(self.tasks),
            "tools": {"interface_digest": "2" * 64, "effect_digest": "3" * 64},
            "verifier": {
                "verifier_id": "sha256",
                "revision": "v1",
                "command_digest": "4" * 64,
            },
            "hardware": {"host_id": "fixture", "os": "fixture", "arch": "fixture"},
            "setup": {
                "setup_receipt_sha256": "5" * 64,
                "index_receipt_sha256": "6" * 64,
            },
            "fallback": {"policy_digest": "7" * 64},
            "timeouts": {"policy_digest": "8" * 64},
            "resources": {"policy_digest": "9" * 64},
            "accounting": {
                "contract": "fresh-work-vector-v1",
                "pricing": pricing,
                "pricing_digest": canonical_sha256(pricing),
                "cost_policy": "provider-billed-microusd",
                "cache_states": list(CACHE_STATES),
            },
            "statistics": {
                "seeds": [7],
                "rule_digest": "b" * 64,
                "exclusions_digest": "c" * 64,
            },
        }
        self.lbi_path.write_text(json.dumps(self.lbi))
        self.lbi_sha256 = canonical_sha256(self.lbi)
        self.trials: list[dict[str, object]] = []
        for task in tasks:
            task_id = task["task_id"]
            rank = task["scale_rank"]
            verifier = root / f"verifier-{rank}.json"
            verifier.write_text(
                json.dumps(
                    {
                        "schema_version": VERIFIER_SCHEMA,
                        "status": "pass",
                        "expected_sha256": task["expected_artifact_sha256"],
                        "actual_sha256": task["expected_artifact_sha256"],
                    }
                )
            )
            raw_by_mode = {
                "full_file": self.targets[task_id].read_bytes(),
                "text_diff": b"@@ -1 +1 @@\n-old\n+new\n",
                "edit_protocol": json.dumps(
                    {
                        "p": "zep/1",
                        "ops": [
                            {
                                "v": "REPLACE",
                                "r": "src/lib.rs#L1-L1",
                                "text": "new\n",
                            }
                        ],
                    },
                    separators=(",", ":"),
                ).encode(),
            }
            for mode, raw in raw_by_mode.items():
                raw_path = root / f"raw-{rank}-{mode}.txt"
                raw_path.write_bytes(raw)
                for cache_state in CACHE_STATES:
                    self.trials.append(
                        {
                            "schema_version": TRIAL_SCHEMA,
                            "trial_id": f"{task_id}-{mode}-7-{cache_state}",
                            "lbi_sha256": self.lbi_sha256,
                            "task_id": task_id,
                            "requested_mode": mode,
                            "cache_state": cache_state,
                            "seed": 7,
                            "outcome": "success",
                            "attempts": [
                                attempt("primary", mode, "success", raw_path.name)
                            ],
                            "materialized_artifact_path": self.targets[task_id].name,
                            "verifier_receipt_path": verifier.name,
                        }
                    )
        self.write_trials()

    def write_trials(self) -> None:
        self.trials_path.write_text(
            "".join(
                json.dumps(trial, separators=(",", ":")) + "\n" for trial in self.trials
            )
        )


class ThreeModeOutputTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.fixture = Fixture(self.root)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def report(self) -> dict[str, object]:
        return build_report(
            self.fixture.lbi_path,
            self.fixture.tasks_path,
            self.fixture.trials_path,
        )

    def test_locked_paired_suite_reports_all_three_modes(self) -> None:
        report = self.report()
        self.assertEqual(report["frontier"]["verdict"], "supported_on_locked_suite")
        self.assertEqual(
            report["denominators"],
            {
                "trials": 12,
                "successes": 12,
                "failures": 0,
                "fallbacks": 0,
                "attempts": 12,
                "failed_attempts": 0,
                "by_cache_state": {"cold": 6, "retained": 6},
            },
        )
        modes = {mode["mode"]: mode for mode in report["modes"]}
        self.assertEqual(set(modes), {"full_file", "text_diff", "edit_protocol"})
        self.assertEqual(
            modes["edit_protocol"]["denominators"]["by_cache_state"],
            {
                "cold": {"trials": 2, "successes": 2, "failures": 0, "fallbacks": 0},
                "retained": {
                    "trials": 2,
                    "successes": 2,
                    "failures": 0,
                    "fallbacks": 0,
                },
            },
        )
        self.assertEqual(modes["edit_protocol"]["eta_action_ppm"], 200_000)
        self.assertEqual(
            modes["full_file"]["total_cost_microusd"]["class_counts"]["billed"], 4
        )
        self.assertEqual(
            modes["full_file"]["total_cost_microusd"]["observed_sum_by_class"][
                "billed"
            ],
            400,
        )
        self.assertTrue(report["raw_trials"]["retained"])
        self.assertNotIn(str(self.root), json.dumps(report))

    def test_absent_usage_is_not_coerced_to_zero(self) -> None:
        for trial in self.fixture.trials:
            trial["attempts"][0]["usage"]["cached_input_tokens"] = {"class": "absent"}
        self.fixture.write_trials()
        report = self.report()
        cached = report["modes"][0]["usage"]["cached_input_tokens"]
        self.assertIsNone(cached["observed_sum"])
        self.assertEqual(cached["observed_count"], 0)
        self.assertEqual(cached["absent_count"], 4)

    def test_fallback_and_failed_attempt_remain_in_denominators(self) -> None:
        trial = next(
            trial
            for trial in self.fixture.trials
            if trial["task_id"] == "rename-1"
            and trial["requested_mode"] == "edit_protocol"
        )
        trial["attempts"][0]["outcome"] = "failure"
        fallback_path = self.root / "fallback.diff"
        fallback_path.write_text("@@ -1 +1 @@\n-old\n+new\n")
        trial["attempts"].append(
            attempt("fallback", "text_diff", "success", fallback_path.name)
        )
        self.fixture.write_trials()
        report = self.report()
        self.assertEqual(report["denominators"]["fallbacks"], 1)
        self.assertEqual(report["denominators"]["failed_attempts"], 1)
        self.assertEqual(report["denominators"]["attempts"], 13)
        self.assertEqual(report["frontier"]["verdict"], "falsified_on_locked_suite")
        receipt = next(
            item for item in report["trials"] if item["trial_id"] == trial["trial_id"]
        )
        self.assertEqual(len(receipt["attempts"]), 2)
        self.assertRegex(
            receipt["attempts"][0]["raw_output"]["sha256"], r"^[0-9a-f]{64}$"
        )

    def test_cli_writes_a_bound_report(self) -> None:
        report_path = self.root / "report.json"
        return_code = main(
            [
                "--lbi",
                str(self.fixture.lbi_path),
                "--tasks",
                str(self.fixture.tasks_path),
                "--trials",
                str(self.fixture.trials_path),
                "--out",
                str(report_path),
                "--require-supported",
            ]
        )
        self.assertEqual(return_code, 0)
        report = json.loads(report_path.read_text())
        self.assertEqual(report["lbi_sha256"], self.fixture.lbi_sha256)
        self.assertEqual(report["raw_trials"]["trial_count"], 12)

    def test_successful_zep_attempt_requires_verb_payload(self) -> None:
        trial = next(
            trial
            for trial in self.fixture.trials
            if trial["requested_mode"] == "edit_protocol"
        )
        raw_path = self.root / trial["attempts"][0]["raw_output_path"]
        raw_path.write_text(
            json.dumps(
                {"p": "zep/1", "ops": [{"v": "REPLACE", "r": "src/lib.rs#L1-L1"}]}
            )
        )
        with self.assertRaisesRegex(HarnessError, "required ZEP/1 field is empty"):
            self.report()

    def test_pricing_assumptions_are_digest_bound(self) -> None:
        self.fixture.lbi["accounting"]["pricing"]["rates"][
            "output_per_million_tokens"
        ] += 1
        self.fixture.lbi_path.write_text(json.dumps(self.fixture.lbi))
        with self.assertRaisesRegex(HarnessError, "does not bind pricing assumptions"):
            self.report()

    def test_identity_drift_fails_closed(self) -> None:
        self.fixture.trials[0]["lbi_sha256"] = "0" * 64
        self.fixture.write_trials()
        with self.assertRaisesRegex(HarnessError, "benchmark identity drift"):
            self.report()

    def test_fresh_work_vector_must_match_input_tokens(self) -> None:
        self.fixture.trials[0]["attempts"][0]["backend_work"]["fresh_work_tokens"] = (
            observation(3)
        )
        self.fixture.write_trials()
        with self.assertRaisesRegex(HarnessError, "must sum to input_tokens"):
            self.report()

    def test_wrong_materialized_bytes_fail_closed(self) -> None:
        self.fixture.targets["rename-1"].write_bytes(b"tampered")
        with self.assertRaisesRegex(HarnessError, "materialized artifact"):
            self.report()

    def test_absent_observation_cannot_smuggle_a_zero(self) -> None:
        trial = copy.deepcopy(self.fixture.trials[0])
        trial["attempts"][0]["usage"]["input_tokens"] = {
            "class": "absent",
            "value": 0,
        }
        self.fixture.trials[0] = trial
        self.fixture.write_trials()
        with self.assertRaisesRegex(HarnessError, "absent is not zero"):
            self.report()


if __name__ == "__main__":
    unittest.main()
