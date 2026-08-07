from __future__ import annotations

import unittest

from benchmarks.harness import first_blob_ref

BLOB = "tz://blob/" + "a" * 64
ORDINAL = "tz://o/2/1"


class FirstBlobRefTests(unittest.TestCase):
    def test_full_envelope_selects_blob_kind(self) -> None:
        self.assertEqual(
            first_blob_ref(
                {
                    "refs": [
                        {"kind": "file", "ref": "tz://file/f123"},
                        {"kind": "blob", "ref": BLOB},
                    ]
                }
            ),
            BLOB,
        )

    def test_slim_envelope_accepts_only_first_durable_primary_ref(self) -> None:
        self.assertEqual(
            first_blob_ref({"refs": [ORDINAL, "tz://file/f123", "tz://search/h123"]}),
            ORDINAL,
        )
        self.assertEqual(first_blob_ref({"refs": ["tz://file/f123", ORDINAL]}), "")
        self.assertEqual(first_blob_ref({"refs": ["https://invalid", ORDINAL]}), "")
        self.assertEqual(first_blob_ref({"refs": ["tz://o/0/1", ORDINAL]}), "")

    def test_invalid_or_mixed_shapes_fail_closed(self) -> None:
        self.assertEqual(
            first_blob_ref({"refs": [ORDINAL, {"kind": "blob", "ref": BLOB}]}),
            "",
        )
        self.assertEqual(first_blob_ref({"refs": "not-a-list"}), "")
        self.assertEqual(first_blob_ref({"refs": [17]}), "")

    def test_legacy_detail_ref_requires_a_durable_primary_shape(self) -> None:
        self.assertEqual(first_blob_ref({"detail_ref": BLOB}), BLOB)
        self.assertEqual(first_blob_ref({"detail_ref": "tz://file/f123"}), "")

    def test_glob_parser_accepts_slim_and_full_visible_shapes(self) -> None:
        from benchmarks.harness import glob_root_and_first

        text = "# root: /work\nsrc/lib.rs\nsrc/main.rs"
        self.assertEqual(
            glob_root_and_first({"visible": text}), ("/work", "src/lib.rs")
        )
        self.assertEqual(
            glob_root_and_first({"visible": {"text": text}}),
            ("/work", "src/lib.rs"),
        )
        self.assertEqual(glob_root_and_first({"visible": 7}), ("", ""))


if __name__ == "__main__":
    unittest.main()
