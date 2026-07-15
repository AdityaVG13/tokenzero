#!/usr/bin/env python3
"""Shared, stdlib-only benchmark measurement helpers."""
from __future__ import annotations
import argparse, hashlib, json; import os, platform, random, shutil, string; import subprocess, sys, tempfile, time; from contextlib import contextmanager
from datetime import datetime, timezone; from pathlib import Path; GUARD = Path('/tmp/zerostack-heavy-process.guard'); RECOVERY_CACHE = Path.home() / '.tokenzero' / 'recovery-cache.json'
REPO = Path(__file__).resolve().parents[1]

def bin_path(profile='release', env_var='TOKENZERO_BIN', required=True):
    candidates = []
    if os.environ.get(env_var):
        candidates.append(Path(os.environ[env_var]))
    candidates += [REPO / 'target' / p / 'tokenzero' for p in (profile, 'release', 'debug')]; candidates.append(Path.home() / '.tokenzero/bin/tokenzero')
    if shutil.which('tokenzero'):
        candidates.append(Path(shutil.which('tokenzero')))
    for path in candidates:
        if path.is_file() and os.access(path, os.X_OK):
            return path
    if required:
        raise SystemExit(f'tokenzero binary not found. Build or set {env_var}=/path/to/tokenzero')
    return REPO / 'target' / profile / 'tokenzero'

def now_ms():
    return int(time.time() * 1000)

def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b''):
            digest.update(chunk)
    return digest.hexdigest()

def git_commit(cwd=None, short=False):
    command = ['git', 'rev-parse', *(['--short'] if short else []), 'HEAD']
    try:
        return subprocess.run(command, cwd=cwd or REPO, capture_output=True, text=True, check=True).stdout.strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return 'unknown'

def token_estimate(data):
    size = data if isinstance(data, int) else len(data if isinstance(data, bytes) else data.encode())
    return (size + 3) // 4

def percentiles_ms(times, qs=(0.5, 0.9, 0.99)):
    if not times:
        return [0] * len(qs)
    values = sorted(times)
    return [round(values[min(len(values) - 1, int(q * (len(values) - 1)))] * 1000) for q in qs]

def median_ms(times):
    values = sorted(times)
    return int(values[len(values) // 2] * 1000) if values else 0

def run_json(argv, cwd=None, check=True):
    started = time.perf_counter(); proc = subprocess.run(argv, cwd=cwd or REPO, capture_output=True, text=True, check=check)
    try:
        raw = json.loads(proc.stdout) if proc.stdout.strip() else {}
    except json.JSONDecodeError:
        raw = {}
    return {'argv': list(map(str, argv)), 'elapsed_ms': round((time.perf_counter() - started) * 1000, 3), 'stdout_bytes': len(proc.stdout.encode()), 'raw_json': raw, 'stderr': proc.stderr}

def capture_environment(binary, harness_command, extra=None):
    try:
        binary_name = str(binary.relative_to(REPO))
    except ValueError:
        binary_name = str(binary)
    exists = binary.is_file(); result = {'generated_at_utc': datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z'), 'harness_command': harness_command, 'cwd': str(REPO), 'os': platform.platform(), 'machine': platform.machine(), 'python': platform.python_version(), 'commit': git_commit(), 'binary': binary_name, 'binary_sha256': sha256(binary) if exists else '', 'binary_mtime_ns': binary.stat().st_mtime_ns if exists else 0, 'cargo_build_jobs': os.environ.get('CARGO_BUILD_JOBS'), 'cargo_incremental': os.environ.get('CARGO_INCREMENTAL')}; result.update(extra or {})
    return result

def _clear_guard():
    for child in GUARD.iterdir():
        if child.is_file():
            child.unlink()
    try:
        GUARD.rmdir()
    except OSError:
        pass

@contextmanager
def heavy_guard(command, repo=None):
    deadline = time.monotonic() + 600
    while True:
        try:
            GUARD.mkdir()
            break
        except FileExistsError:
            try:
                pid = int((GUARD / 'pid').read_text().strip()); os.kill(pid, 0)
            except (FileNotFoundError, ValueError, ProcessLookupError):
                _clear_guard()
                continue
            except PermissionError as err:
                raise SystemExit(f'cannot inspect heavy-process guard owner: {err}') from err
            if time.monotonic() >= deadline:
                raise SystemExit(f'heavy-process guard still held by live pid {pid}')
            time.sleep(2)
    values = {'pid': os.getpid(), 'repository': repo or REPO, 'command': command, 'started_at': datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z')}
    for name, value in values.items():
        (GUARD / name).write_text(f'{value}\n')
    try:
        yield
    finally:
        try:
            owned = (GUARD / 'pid').read_text().strip() == str(os.getpid())
        except FileNotFoundError:
            owned = False
        if owned:
            _clear_guard()

def synthetic_tree(root, count):
    for number in range(count):
        shard, index = divmod(number, 1000); directory = root / f'd{shard:03d}'; directory.mkdir(parents=True, exist_ok=True); (directory / f'f{index:04d}.txt').write_text(f'{shard:03d}:{index:04d}\n')

def million_line_repo(root, n_dirs, n_files, n_lines, needle, seed=42):
    random.seed(seed); chars = string.ascii_letters + string.digits
    for i in range(n_dirs):
        directory = root / f'mod_{i:04d}'; directory.mkdir(parents=True, exist_ok=True)
        for j in range(n_files):
            with (directory / f'file_{i:04d}_{j:03d}.rs').open('w') as stream:
                for k in range(n_lines):
                    body = f'pub fn {needle}(x: usize) -> bool {{ true }}' if k == 499 and (i * n_files + j) % 20 == 0 else ''.join(random.choices(chars, k=36)); stream.write(f'// line {k:04d} {body}\n')

def file_count(root, excludes=('.git', 'target', '.zerostack')):
    total = 0
    for _, dirs, files in os.walk(root):
        dirs[:] = [name for name in dirs if name not in excludes]; total += len(files)
    return total

def _times(command, runs, warmup, prepare, name, cold_warmup=False):
    if shutil.which('hyperfine'):
        with tempfile.TemporaryDirectory(prefix='tz-hf-') as tmp:
            artifact = Path(tmp) / 'hf.json'; subprocess.run(['hyperfine', '--warmup', str(warmup), '--runs', str(runs), '--style', 'basic', '--export-json', str(artifact), '--prepare', prepare, '--command-name', name, command], capture_output=True, text=True)
            try:
                return json.loads(artifact.read_text()).get('results', [{}])[0].get('times', []) or []
            except (json.JSONDecodeError, OSError, IndexError, KeyError, TypeError):
                pass
    if cold_warmup:
        subprocess.run(['bash', '-c', f'{prepare}; {command}'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    probe = subprocess.run(['/usr/bin/time', '-f', '%e', 'true'], capture_output=True, text=True)
    try:
        float((probe.stderr or probe.stdout).strip().splitlines()[-1]); gnu_time = probe.returncode == 0
    except (ValueError, IndexError):
        gnu_time = False
    times = []
    for _ in range(runs):
        subprocess.run(['bash', '-c', prepare], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if not gnu_time:
            started = time.perf_counter(); subprocess.run(['bash', '-c', command], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); times.append(time.perf_counter() - started)
            continue
        with tempfile.NamedTemporaryFile('w+', delete=False) as stream:
            output = Path(stream.name)
        script = f"/usr/bin/time -f '%e' bash -c {json.dumps(command)} >/dev/null 2>>{json.dumps(str(output))}"; subprocess.run(['bash', '-c', script])
        try:
            times.extend((float(line) for line in output.read_text().splitlines() if line.strip()))
        except (OSError, ValueError):
            pass
        finally:
            output.unlink(missing_ok=True)
    return times

def measure_cell(label, command, cold=False, runs=50, warmup=3):
    prepare = f'rm -f {RECOVERY_CACHE}' if cold else 'true'
    return tuple(percentiles_ms(_times(command, runs, warmup, prepare, label, cold)))

def measure_median(label, command, runs=5, warmup=1):
    wall = median_ms(_times(command, runs, warmup, 'true', label))
    try:
        output = subprocess.run(['bash', '-c', command], capture_output=True).stdout
    except OSError:
        output = b''
    return (wall, len(output), token_estimate(output))

def _json(data):
    if not isinstance(data, str):
        return data
    try:
        return json.loads(data)
    except json.JSONDecodeError:
        return {}

def mcp_schema_tokens(cap_file, tools_csv):
    cap = json.loads(Path(cap_file).read_text()); by_name = cap.get('commands_by_name', {}); commands = cap.get('commands', []); selected = {}
    for name in filter(None, map(str.strip, tools_csv.split(','))):
        found = by_name.get(name) or next((item for item in commands if item.get('name') == name), None)
        if found:
            selected[name] = found
    return token_estimate(json.dumps(selected, separators=(',', ':'), ensure_ascii=False).encode())

def quality_check(task, payload):
    data = _json(payload)
    if isinstance(data, dict):
        value, visible = (data.get('value', {}), data.get('visible', {})); text = str(value['text'] if isinstance(value, dict) and 'text' in value else visible['text'] if isinstance(visible, dict) and 'text' in visible else json.dumps(data))
    else:
        text = str(data)
    low = text.lower()
    passed = {'read_file': '[workspace]' in text, 'search_filter': 'TokenZero' in text and text.count('\n') >= 1, 'edit_verify': 'BETA' in text and 'beta' not in text, 'multi_step_nav': 'workspace' in low, 'shell_expand': 'Cargo.toml' in text}.get(task, False)
    return 'PASS' if passed else 'FAIL'

def accounting_tokens(payload, key='raw_tokens'):
    data = _json(payload)
    if not isinstance(data, dict):
        return 0
    bags = (data.get('accounting', {}), data.get('telemetry', {}), data.get('value', {}))
    return next((int(bag[key]) for bag in bags if isinstance(bag, dict) and key in bag), 0)

def first_blob_ref(data):
    data = _json(data)
    return next((str(item.get('ref', '')) for item in data.get('refs', []) if item.get('kind') == 'blob'), '') if isinstance(data, dict) else ''

def glob_root_and_first(data):
    data = _json(data); text = str(data.get('visible', {}).get('text', '')) if isinstance(data, dict) else ''
    root = next((line.split(':', 1)[1].strip() for line in text.splitlines() if line.startswith('# root:')), '')
    rel = next((line.strip() for line in text.splitlines() if line.strip() and (not line.strip().startswith('#'))), '')
    return (root, rel)

def main(argv=None):
    parser = argparse.ArgumentParser(prog='harness.py'); sub = parser.add_subparsers(dest='action', required=True)

    def add(name, *args):
        command = sub.add_parser(name)
        for flags, options in args:
            command.add_argument(*flags, **options)
    add('resolve_bin', (('--profile',), {'default': 'release'})); add('now_ms'); add('tok', (('--bytes',), {'type': int})); percentile = sub.add_parser('percentiles')
    group = percentile.add_mutually_exclusive_group(required=True); group.add_argument('--json'); group.add_argument('--times'); add('measure_cell', (('label',), {}), (('cmd',), {}), (('--cold',), {'action': 'store_true'}), (('--runs',), {'type': int, 'default': 50}), (('--warmup',), {'type': int, 'default': 3}))
    add('measure_median', (('label',), {}), (('cmd',), {}), (('--runs',), {'type': int, 'default': 5}), (('--warmup',), {'type': int, 'default': 1})); add('mcp_schema_tokens', (('cap_file',), {}), (('tools',), {})); add('quality', (('task',), {})); add('clear_cache')
    add('git_commit', (('--short',), {'action': 'store_true'})); add('accounting', (('--file',), {}), (('--key',), {'default': 'raw_tokens'})); add('first_blob_ref', (('file',), {})); add('glob_pick', (('file',), {}))
    add('generate_million', (('root',), {}), (('--dirs',), {'type': int, 'default': 100}), (('--files',), {'type': int, 'default': 10}), (('--lines',), {'type': int, 'default': 1000}), (('--needle',), {'default': 'BENCH_NEEDLE_FN'})); add('tz_metrics', (('file',), {}), (('wall',), {})); args = parser.parse_args(argv); action = args.action
    if action == 'resolve_bin':
        result = bin_path(args.profile)
    elif action == 'now_ms':
        result = now_ms()
    elif action == 'tok':
        result = token_estimate(args.bytes if args.bytes is not None else sys.stdin.buffer.read())
    elif action == 'percentiles':
        values = json.loads(Path(args.json).read_text()).get('results', [{}])[0].get('times', []) if args.json else [float(x) for x in Path(args.times).read_text().splitlines() if x.strip()]; result = '\t'.join(map(str, percentiles_ms(values)))
    elif action == 'measure_cell':
        result = '\t'.join(map(str, measure_cell(args.label, args.cmd, args.cold, args.runs, args.warmup)))
    elif action == 'measure_median':
        result = '\t'.join(map(str, measure_median(args.label, args.cmd, args.runs, args.warmup)))
    elif action == 'mcp_schema_tokens':
        result = mcp_schema_tokens(args.cap_file, args.tools)
    elif action == 'quality':
        result = quality_check(args.task, sys.stdin.read())
    elif action == 'clear_cache':
        RECOVERY_CACHE.unlink(missing_ok=True)
        return 0
    elif action == 'git_commit':
        result = git_commit(short=args.short)
    elif action == 'accounting':
        result = accounting_tokens(Path(args.file).read_text() if args.file else sys.stdin.read(), args.key)
    elif action == 'first_blob_ref':
        result = first_blob_ref(Path(args.file).read_text())
    elif action == 'glob_pick':
        result = '\t'.join(glob_root_and_first(Path(args.file).read_text()))
    elif action == 'generate_million':
        million_line_repo(Path(args.root), args.dirs, args.files, args.lines, args.needle); result = 'done'
    else:
        try:
            accounting = json.loads(Path(args.file).read_text()).get('accounting', {}); result = f"{accounting.get('visible_tokens', 0)}\t{accounting.get('raw_tokens', 0)}\t{args.wall}"
        except (json.JSONDecodeError, OSError):
            result = f'0\t0\t{args.wall}'
    print(result)
    return 0
if __name__ == '__main__':
    raise SystemExit(main())
