#!/usr/bin/env python3
"""Replay a deterministic MCP corpus and gate ledger-visible token mass."""
from __future__ import annotations
import argparse, json, os, re, select, subprocess, tempfile; from pathlib import Path; from typing import Any; HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]; ALIASES = {'read': ('read', 'tokenzero_read', 'zero_read', 'tz_read'), 'search': ('search', 'find', 'tokenzero_search', 'tokenzero_find', 'zero_search', 'tz_find'), 'expand': ('expand', 'tokenzero_expand', 'zero_expand', 'tz_expand'), 'shell': ('shell', 'run', 'tokenzero_shell', 'tokenzero_run', 'zero_shell', 'tz_shell'), 'codemode': ('execute', 'codemode', 'zero_execute', 'tokenzero_codemode', 'tz_execute_code')}

class Client:

    def __init__(self, binary: Path, mode: str, root: Path, cache: Path):
        self.p = subprocess.Popen([str(binary), 'mcp-server', '--mode', mode, '--allowed-root', str(root), '--cache-path', str(cache)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1, env={**os.environ, 'NO_COLOR': '1', 'CI': '1'}); self.ident = 1; self.request('initialize', {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 'ledger-gate', 'version': '1'}}); self.send({'jsonrpc': '2.0', 'method': 'notifications/initialized', 'params': {}})
        listed = self.request('tools/list', {})['result']['tools']; self.tools = {tool['name']: tool for tool in listed}

    def send(self, value: dict[str, Any]):
        assert self.p.stdin; self.p.stdin.write(json.dumps(value, separators=(',', ':')) + '\n'); self.p.stdin.flush()

    def request(self, method: str, params: dict[str, Any]):
        ident = self.ident; self.ident += 1; self.send({'jsonrpc': '2.0', 'id': ident, 'method': method, 'params': params}); assert self.p.stdout
        while True:
            ready, _, _ = select.select([self.p.stdout], [], [], 30)
            if not ready:
                raise RuntimeError(f'MCP timeout during {method}')
            line = self.p.stdout.readline()
            if not line:
                err = self.p.stderr.read() if self.p.stderr else ''
                raise RuntimeError(f'MCP server exited during {method}: {err.strip()}')
            reply = json.loads(line)
            if reply.get('id') != ident:
                continue
            if 'error' in reply:
                raise RuntimeError(f"MCP {method}: {reply['error']}")
            return reply

    def name(self, logical: str) -> str:
        aliases = ALIASES[logical]
        for alias in aliases:
            if alias in self.tools:
                return alias
        for actual in self.tools:
            normalized = actual.lower().replace('-', '_')
            if any((normalized.endswith(alias) for alias in aliases)):
                return actual
        raise RuntimeError(f'no {logical} tool; advertised: {sorted(self.tools)}')

    def call(self, logical: str, arguments: dict[str, Any]):
        return self.request('tools/call', {'name': self.name(logical), 'arguments': arguments})

    def close(self):
        if self.p.poll() is None:
            if self.p.stdin:
                self.p.stdin.close()
            try:
                self.p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.p.terminate()
                try:
                    self.p.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    self.p.kill(); self.p.wait(timeout=5)

def replace(value: Any, root: Path, read_ref: str | None):
    if isinstance(value, str):
        value = value.replace('${ROOT}', str(root))
        if '${READ_REF}' in value:
            if not read_ref:
                raise RuntimeError('READ_REF requested before read emitted one')
            value = value.replace('${READ_REF}', read_ref)
        return value
    if isinstance(value, list):
        return [replace(v, root, read_ref) for v in value]
    if isinstance(value, dict):
        return {k: replace(v, root, read_ref) for k, v in value.items()}
    return value

def find_ref(value: Any):
    if isinstance(value, str):
        match = re.search('(?:tz|fz|gz)://[^\\s]+', value)
        if match:
            return match.group(0)
    values = value.values() if isinstance(value, dict) else value if isinstance(value, list) else ()
    for child in values:
        found = find_ref(child)
        if found:
            return found
    return None

def export(binary: Path, cache: Path, ledger: Path):
    if not ledger.is_file() or not ledger.stat().st_size:
        raise RuntimeError(f'ledger smoke failed: missing or empty {ledger}')
    rows = []
    for number, line in enumerate(ledger.read_text().splitlines(), 1):
        if line.strip():
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError as exc:
                raise RuntimeError(f'invalid ledger row {number}: {exc}') from exc
    done = subprocess.run([str(binary), 'session-ledger', 'export', '--json'], text=True, capture_output=True, env={**os.environ, 'TOKENZERO_CACHE_PATH': str(cache), 'NO_COLOR': '1', 'CI': '1'})
    if done.returncode:
        raise RuntimeError(f'session-ledger export failed: {done.stderr.strip()}')
    report = json.loads(done.stdout); report['jsonl_records'] = len(rows)
    return report

def replay(binary: Path, corpus: dict[str, Any]):
    with tempfile.TemporaryDirectory(prefix='tokenzero-ledger-gate-') as tmp:
        root = Path(tmp) / 'fixture'; cache = Path(tmp) / 'state' / 'recovery-cache.json'
        for relative, content in corpus['fixture'].items():
            target = root / relative; target.parent.mkdir(parents=True, exist_ok=True); target.write_text(content)
        clients = {}; read_ref = None; tools = {}
        try:
            for call in corpus['calls']:
                mode = call.get('mode', 'mcp')
                if mode not in clients:
                    clients[mode] = Client(binary, mode, root, cache)
                logical = call['tool']; response = clients[mode].call(logical, replace(call['arguments'], root, read_ref))
                if response['result'].get('isError'):
                    raise RuntimeError(f"{call['id']} failed: {response['result']}")
                tools[logical] = tools.get(logical, 0) + 1
                if logical == 'read':
                    read_ref = find_ref(response)
                    if not read_ref:
                        raise RuntimeError(f'read emitted no recovery ref: {response}')
        finally:
            for client in clients.values():
                client.close()
        report = export(binary, cache, cache.with_name('ledger.jsonl')); visible = int(report['total_visible_tokens']); raw = int(report['total_raw_tokens'])
        return {'schema_version': 'tokenzero.ledger-gate.evidence.v1', 'corpus_id': corpus['corpus_id'], 'mass': {'visible_tokens': visible, 'raw_tokens': raw, 'prevented_tokens': max(raw - visible, 0)}, 'turns': int(report['total_turns']), 'sessions': int(report['total_sessions']), 'jsonl_records': int(report['jsonl_records']), 'tools': dict(sorted(tools.items()))}

def pct(new: int, old: int):
    return 0.0 if old == new == 0 else float('inf') if old == 0 else (new - old) * 100.0 / old

def main():
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument('--binary', type=Path, default=REPO / 'target/debug/tokenzero'); parser.add_argument('--baseline', type=Path, default=HERE / 'baseline.json'); parser.add_argument('--threshold', type=float, default=5.0)
    parser.add_argument('--update-baseline', action='store_true'); args = parser.parse_args()
    if args.threshold < 0:
        parser.error('--threshold must be non-negative')
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f'binary not found: {binary}')
    corpus = json.loads((HERE / 'corpus.json').read_text()); evidence = replay(binary, corpus)
    if args.update_baseline:
        args.baseline.write_text(json.dumps({**evidence, 'threshold_percent': args.threshold}, indent=2, sort_keys=True) + '\n'); m = evidence['mass']; print(f"updated {args.baseline}: visible={m['visible_tokens']} raw={m['raw_tokens']} prevented={m['prevented_tokens']}")
        return 0
    baseline = json.loads(args.baseline.read_text())
    if baseline.get('corpus_id') != evidence['corpus_id']:
        raise RuntimeError('baseline corpus_id does not match corpus')
    print(f"ledger gate corpus={evidence['corpus_id']} threshold={args.threshold:.2f}%"); print('metric                   baseline   candidate       delta')
    for metric in ('visible_tokens', 'raw_tokens', 'prevented_tokens'):
        old = int(baseline['mass'][metric]); new = int(evidence['mass'][metric]); print(f'{metric:24} {old:10d} {new:11d} {pct(new, old):+10.2f}%')
    print('per-tool calls:')
    for tool in sorted(set(baseline.get('tools', {})) | set(evidence['tools'])):
        old = int(baseline.get('tools', {}).get(tool, 0)); new = int(evidence['tools'].get(tool, 0)); print(f'  {tool:18} {old:4d} -> {new:4d} ({new - old:+d})')
    delta = pct(evidence['mass']['visible_tokens'], baseline['mass']['visible_tokens'])
    if delta > args.threshold:
        print(f'FAIL: visible token cost regressed by {delta:.2f}% (limit {args.threshold:.2f}%)')
        return 1
    print(f'PASS: visible token cost delta {delta:+.2f}% is within {args.threshold:.2f}%')
    return 0
if __name__ == '__main__':
    raise SystemExit(main())
