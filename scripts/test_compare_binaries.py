#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import os
import time
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("compare_binaries.py")
SPEC = importlib.util.spec_from_file_location("compare_binaries", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
compare_binaries = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(compare_binaries)


class CompareBinariesTests(unittest.TestCase):
    def test_nearest_rank_summary_keeps_all_samples(self) -> None:
        values = [9.0, 1.0, 7.0, 3.0, 5.0]
        self.assertEqual(
            compare_binaries.summary(values),
            {"n": 5, "p50": 5.0, "p95": 9.0, "min": 1.0, "max": 9.0},
        )

    def test_live_process_cpu_is_monotonic(self) -> None:
        before = compare_binaries.process_cpu_ms(os.getpid())
        deadline = time.perf_counter() + 0.02
        while time.perf_counter() < deadline:
            pass
        after = compare_binaries.process_cpu_ms(os.getpid())
        self.assertGreaterEqual(after, before)
        self.assertGreater(after, 0.0)


if __name__ == "__main__":
    unittest.main()
