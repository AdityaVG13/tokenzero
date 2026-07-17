#!/usr/bin/env python3
"""Expand committed ``*.rs.gz`` corpus members into ``expanded/``.

Enforces decompressed-size and expansion-ratio ceilings so a hostile
gzip member cannot force unbounded live memory / disk during materialize.
Members are streamed into a staging directory and the live ``expanded/``
tree is replaced only after every member validates, so a late failure
cannot leave a partially refreshed corpus.
"""

from __future__ import annotations

import gzip
import os
import shutil
import sys
import tempfile
from pathlib import Path

# Legitimate members top out near ~92 KiB expanded / ~5x ratio. Keep headroom
# for growth, but reject classic gzip bombs (16 MiB @ ~1000x).
MAX_EXPANDED_BYTES = 512 * 1024
MAX_EXPANSION_RATIO = 32.0
READ_CHUNK = 64 * 1024


def materialize_member(source: Path, destination: Path) -> None:
    compressed_bytes = source.stat().st_size
    if compressed_bytes == 0:
        raise ValueError(f"{source.name}: empty gzip member")

    destination.parent.mkdir(parents=True, exist_ok=True)
    expanded = 0
    with source.open("rb") as raw, gzip.GzipFile(fileobj=raw, mode="rb") as gz:
        with tempfile.NamedTemporaryFile(
            dir=destination.parent,
            prefix=f".{destination.name}.",
            suffix=".tmp",
            delete=False,
        ) as tmp:
            tmp_path = Path(tmp.name)
            try:
                while True:
                    chunk = gz.read(READ_CHUNK)
                    if not chunk:
                        break
                    expanded += len(chunk)
                    if expanded > MAX_EXPANDED_BYTES:
                        raise ValueError(
                            f"{source.name}: expanded size {expanded} exceeds "
                            f"max {MAX_EXPANDED_BYTES} bytes"
                        )
                    ratio = expanded / compressed_bytes
                    if ratio > MAX_EXPANSION_RATIO:
                        raise ValueError(
                            f"{source.name}: expansion ratio {ratio:.1f} exceeds "
                            f"max {MAX_EXPANSION_RATIO}"
                        )
                    tmp.write(chunk)
                tmp.flush()
            except Exception:
                tmp_path.unlink(missing_ok=True)
                raise
    tmp_path.replace(destination)


def main() -> int:
    root = Path(__file__).parent
    out = root / "expanded"
    sources = sorted(root.glob("*.rs.gz"))
    staging = root / f".expanded.staging.{os.getpid()}"
    backup = root / f".expanded.bak.{os.getpid()}"
    try:
        if staging.exists():
            shutil.rmtree(staging)
        staging.mkdir()
        for source in sources:
            materialize_member(source, staging / source.name.removesuffix(".gz"))
        # All members validated — publish staging as the live expanded tree.
        if backup.exists():
            shutil.rmtree(backup)
        if out.exists():
            out.rename(backup)
        staging.rename(out)
        if backup.exists():
            shutil.rmtree(backup)
    except (OSError, EOFError, gzip.BadGzipFile, ValueError) as exc:
        if staging.exists():
            shutil.rmtree(staging, ignore_errors=True)
        print(f"materialize failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
