#!/usr/bin/env python3
"""Shard package_audit/tests.rs into tests/ mirroring tar/zip production split."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "crates/tokenzero-install/src/package_audit"
SRC = ROOT / "tests.rs"
lines = SRC.read_text().splitlines(keepends=True)

# Line ranges are 1-indexed, inclusive (from manual audit of tests.rs).
FIND_ZIP_EOCD = lines[0:4]  # #[cfg(test)] fn find_zip_eocd ...
GENERAL = lines[9:218]  # first 9 integration tests
TAR = lines[218:1352]  # tar corpus through last tar-only test
ZIP = lines[1352:3042]  # zip corpus through malformed_zip_corpus test
FIXTURES_TAR = lines[3043:3173]  # TarTestEntry .. end of tar helpers
FIXTURES_ZIP = lines[3173:]  # ZipTestEntry .. deflate_bytes

MOD_RS = """use super::*;
use tempfile::tempdir;

mod fixtures;
mod general;
mod tar;
mod zip;
"""

CHILD_USES = "use super::fixtures::*;\nuse super::*;\nuse std::io::Write;\n\n"

FIXTURES_MOD = """mod tar;
mod zip;

pub(crate) use tar::*;
pub(crate) use zip::*;
"""

def pub_crate_helpers(body: str) -> str:
    body = re.sub(r"^fn ", "pub(crate) fn ", body, flags=re.M)
    body = re.sub(r"^struct ", "pub(crate) struct ", body, flags=re.M)
    body = re.sub(r"^    fn ", "    pub(crate) fn ", body, flags=re.M)
    return body

tests_dir = ROOT / "tests"
fixtures_dir = tests_dir / "fixtures"
tests_dir.mkdir(exist_ok=True)
fixtures_dir.mkdir(exist_ok=True)

(tests_dir / "mod.rs").write_text(MOD_RS)
(tests_dir / "general.rs").write_text(CHILD_USES + "".join(GENERAL))
(tests_dir / "tar.rs").write_text(CHILD_USES + "".join(TAR))
(tests_dir / "zip.rs").write_text(CHILD_USES + "".join(ZIP))

tar_fixture = "use super::super::*;\nuse std::io::Write;\nuse std::path::Path;\n\n"
(fixtures_dir / "tar.rs").write_text(tar_fixture + pub_crate_helpers("".join(FIXTURES_TAR)))

zip_fixture = "use super::super::*;\nuse std::io::Write;\nuse std::path::Path;\n\n"
find_zip = pub_crate_helpers("".join(FIND_ZIP_EOCD).replace("#[cfg(test)]\n", ""))
(fixtures_dir / "zip.rs").write_text(zip_fixture + find_zip + pub_crate_helpers("".join(FIXTURES_ZIP)))
(fixtures_dir / "mod.rs").write_text(FIXTURES_MOD)

SRC.unlink()

for path in sorted(tests_dir.rglob("*.rs")):
    rel = path.relative_to(ROOT)
    print(f"wrote {rel}: {sum(1 for _ in path.open())} lines")
