#!/usr/bin/env python3
from pathlib import Path
import gzip

root = Path(__file__).parent
out = root / "expanded"
out.mkdir(exist_ok=True)
for source in sorted(root.glob("*.rs.gz")):
    (out / source.name.removesuffix(".gz")).write_bytes(gzip.decompress(source.read_bytes()))
