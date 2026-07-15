#!/usr/bin/env python3
"""Measure manifest+delta boot cost on this repo and a 100k-file corpus."""
from __future__ import annotations
import argparse; import json; import tempfile; from pathlib import Path
import os
import harness as H; REPO = H.REPO; BIN = Path(os.environ.get('TOKENZERO_BOOT_BENCH_BIN', REPO / 'target/debug/tokenzero')); EVIDENCE = Path(__file__).with_suffix('')
SYNTHETIC_FILES = 100000; COUNT_EXCLUDES = {'.git', 'target', '.zerostack'}

def masked_argv(arguments: list[str], root: Path, cache: Path) -> list[str]:
    return ['<root>' if value == str(root) else '<temp-cache>' if value == str(cache) else value for value in arguments]

def measure(label: str, root: Path, cache_dir: Path) -> dict[str, object]:
    cache = cache_dir / 'recovery-cache.json'; command = [str(BIN), 'session-open', '--root', str(root), '--cache-path', str(cache), '--json']; initialized = H.run_json(command); recorded = H.run_json(command)
    boot = recorded['raw_json']; components = boot['telemetry']; component_sum = sum((int(components[name]) for name in ('manifest', 'delta', 'toc_working_set', 'other'))); total = int(components['total'])
    if component_sum != total:
        raise RuntimeError(f'{label} components {component_sum} != total {total}')
    if total >= 100:
        raise RuntimeError(f'{label} boot cost {total} is not below 100: {boot}')
    if boot.get('mode') != 'manifest_delta':
        raise RuntimeError(f'{label} did not use manifest+delta boot: {boot}')
    if boot.get('demand_paging', {}).get('working_set_loaded') is not False:
        raise RuntimeError(f'{label} eagerly loaded the working set: {boot}')
    initialized['argv'] = masked_argv(command, root, cache); recorded['argv'] = masked_argv(command, root, cache)
    for blob in (initialized, recorded):
        blob.pop('returncode', None); blob.pop('stdout', None)
    return {'label': label, 'root': str(root), 'file_count': H.file_count(root, COUNT_EXCLUDES), 'file_count_excludes': sorted(COUNT_EXCLUDES), 'metadata_initialization': initialized, 'recorded_manifest_delta_boot': recorded, 'boot_tokens': total, 'components': components, 'component_sum': component_sum}

def run(label: str) -> Path:
    if not BIN.is_file():
        raise SystemExit(f'benchmark binary missing: {BIN}; build or select it with TOKENZERO_BOOT_BENCH_BIN')
    with H.heavy_guard(f'python3 benchmarks/boot-cost.py --label {label}'):
        with tempfile.TemporaryDirectory(prefix='tokenzero-boot-cost-') as raw:
            tmp = Path(raw); synthetic = tmp / 'synthetic-100k'; synthetic.mkdir(); H.synthetic_tree(synthetic, SYNTHETIC_FILES)
            result = {'schema': 'tokenzero.boot-cost.v1', 'environment': H.capture_environment(BIN, f'python3 benchmarks/boot-cost.py --label {label}', extra={'worktree_note': 'Measurements intentionally include the uncommitted bead implementation.', 'synthetic_generator': '100 directories x 1000 deterministic 8-byte text files'}), 'corpora': [measure('repository', REPO, tmp / 'repository-cache'), measure('synthetic-100k', synthetic, tmp / 'synthetic-cache')], 'assertions': {'max_boot_tokens_exclusive': 100, 'component_totals_match': True, 'working_set_demand_paged': True, 'all_passed': True}}; EVIDENCE.mkdir(parents=True, exist_ok=True); destination = EVIDENCE / f'{label}.json'; destination.write_text(json.dumps(result, indent=2) + '\n')
            return destination

def main() -> int:
    parser = argparse.ArgumentParser(); parser.add_argument('--label', default='candidate'); args = parser.parse_args(); print(run(args.label).relative_to(REPO))
    return 0
if __name__ == '__main__':
    raise SystemExit(main())
