#!/usr/bin/env python3
"""Shared measurement plumbing for TokenZero benchmarks (no runtime TokenZero dep)."""
from __future__ import annotations

import argparse, hashlib, json, os, platform, random, shutil, string, subprocess, sys, tempfile, time
from contextlib import contextmanager
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Iterator, Sequence

GUARD = Path("/tmp/zerostack-heavy-process.guard")
RECOVERY_CACHE = Path.home() / ".tokenzero" / "recovery-cache.json"
DEFAULT_EXCLUDES = frozenset({".git", "target", ".zerostack"})
REPO = Path(__file__).resolve().parents[1]


def bin_path(profile: str = "release", env_var: str = "TOKENZERO_BIN", required: bool = True) -> Path:
    env = os.environ.get(env_var)
    if env and Path(env).is_file() and os.access(env, os.X_OK):
        return Path(env)
    for prof in (profile, "release", "debug"):
        cand = REPO / "target" / prof / "tokenzero"
        if cand.is_file() and os.access(cand, os.X_OK):
            return cand
    home = Path.home() / ".tokenzero" / "bin" / "tokenzero"
    if home.is_file() and os.access(home, os.X_OK):
        return home
    which = shutil.which("tokenzero")
    if which:
        return Path(which)
    if required:
        raise SystemExit(f"tokenzero binary not found. Build or set {env_var}=/path/to/tokenzero")
    return REPO / "target" / profile / "tokenzero"


def now_ms() -> int:
    return int(time.time() * 1000)


def sha256(path: Path) -> str:
    d = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            d.update(chunk)
    return d.hexdigest()


def git_commit(cwd: Path | None = None, short: bool = False) -> str:
    args = ["git", "rev-parse"] + (["--short"] if short else []) + ["HEAD"]
    try:
        return subprocess.run(args, cwd=cwd or REPO, capture_output=True, text=True, check=True).stdout.strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return "unknown"


def token_estimate(data: str | bytes | int) -> int:
    n = data if isinstance(data, int) else len(data if isinstance(data, bytes) else data.encode("utf-8"))
    return (n + 3) // 4


def percentiles_ms(times: Sequence[float], qs: Sequence[float] = (0.5, 0.9, 0.99)) -> list[int]:
    if not times:
        return [0 for _ in qs]
    xs = sorted(times)
    n = len(xs)
    return [int(round(xs[min(n - 1, int(q * (n - 1)))] * 1000)) for q in qs]


def median_ms(times: Sequence[float]) -> int:
    if not times:
        return 0
    xs = sorted(times)
    return int(xs[len(xs) // 2] * 1000)


def run_json(arguments: Sequence[str], *, cwd: Path | None = None, check: bool = True) -> dict[str, Any]:
    started = time.perf_counter()
    proc = subprocess.run(list(arguments), cwd=cwd or REPO, capture_output=True, text=True, check=check)
    try:
        raw = json.loads(proc.stdout) if proc.stdout.strip() else {}
    except json.JSONDecodeError:
        raw = {}
    return {
        "argv": [str(v) for v in arguments],
        "elapsed_ms": round((time.perf_counter() - started) * 1000, 3),
        "stdout_bytes": len(proc.stdout.encode("utf-8")),
        "raw_json": raw,
        "stderr": proc.stderr,
    }


def capture_environment(binary: Path, harness_command: str, extra: dict[str, Any] | None = None) -> dict[str, Any]:
    try:
        rel = str(binary.relative_to(REPO))
    except ValueError:
        rel = str(binary)
    env: dict[str, Any] = {
        "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "harness_command": harness_command,
        "cwd": str(REPO),
        "os": platform.platform(),
        "machine": platform.machine(),
        "python": platform.python_version(),
        "commit": git_commit(),
        "binary": rel,
        "binary_sha256": sha256(binary) if binary.is_file() else "",
        "binary_mtime_ns": binary.stat().st_mtime_ns if binary.is_file() else 0,
        "cargo_build_jobs": os.environ.get("CARGO_BUILD_JOBS"),
        "cargo_incremental": os.environ.get("CARGO_INCREMENTAL"),
    }
    if extra:
        env.update(extra)
    return env


@contextmanager
def heavy_guard(command: str, repo: Path | None = None) -> Iterator[None]:
    root = repo or REPO
    deadline = time.monotonic() + 600
    while True:
        try:
            GUARD.mkdir()
            break
        except FileExistsError:
            try:
                pid = int((GUARD / "pid").read_text().strip())
                os.kill(pid, 0)
            except (FileNotFoundError, ValueError, ProcessLookupError):
                for child in GUARD.iterdir():
                    if child.is_file():
                        child.unlink()
                try:
                    GUARD.rmdir()
                except OSError:
                    pass
                continue
            except PermissionError as err:
                raise SystemExit(f"cannot inspect heavy-process guard owner: {err}") from err
            if time.monotonic() >= deadline:
                raise SystemExit(f"heavy-process guard still held by live pid {pid}")
            time.sleep(2)
    (GUARD / "pid").write_text(f"{os.getpid()}\n")
    (GUARD / "repository").write_text(f"{root}\n")
    (GUARD / "command").write_text(f"{command}\n")
    (GUARD / "started_at").write_text(datetime.now(timezone.utc).isoformat().replace("+00:00", "Z") + "\n")
    try:
        yield
    finally:
        owned = False
        try:
            owned = (GUARD / "pid").read_text().strip() == str(os.getpid())
        except FileNotFoundError:
            owned = False
        if owned:
            for child in GUARD.iterdir():
                if child.is_file():
                    child.unlink()
            try:
                GUARD.rmdir()
            except OSError:
                pass


def synthetic_tree(root: Path, count: int) -> None:
    remaining = count
    for shard in range((count + 999) // 1000):
        directory = root / f"d{shard:03d}"
        directory.mkdir(parents=True, exist_ok=True)
        batch = min(remaining, 1000)
        for index in range(batch):
            (directory / f"f{index:04d}.txt").write_text(f"{shard:03d}:{index:04d}\n")
        remaining -= batch


def million_line_repo(root: Path, n_dirs: int, n_files: int, n_lines: int, needle: str, seed: int = 42) -> None:
    random.seed(seed)
    chars = string.ascii_letters + string.digits
    for i in range(n_dirs):
        d = root / f"mod_{i:04d}"
        d.mkdir(parents=True, exist_ok=True)
        for j in range(n_files):
            with (d / f"file_{i:04d}_{j:03d}.rs").open("w") as fh:
                for k in range(n_lines):
                    if k == 499 and (i * n_files + j) % 20 == 0:
                        fh.write(f"// line {k:04d} pub fn {needle}(x: usize) -> bool {{ true }}\n")
                    else:
                        fh.write(f"// line {k:04d} {''.join(random.choices(chars, k=36))}\n")


def file_count(root: Path, excludes: Iterable[str] | None = None) -> int:
    skip = set(excludes) if excludes is not None else set(DEFAULT_EXCLUDES)
    total = 0
    for _, dirs, files in os.walk(root):
        dirs[:] = [n for n in dirs if n not in skip]
        total += len(files)
    return total


def _times(cmd: str, runs: int, warmup: int, prepare: str, name: str, cold_warmup: bool = False) -> list[float]:
    if shutil.which("hyperfine"):
        with tempfile.TemporaryDirectory(prefix="tz-hf-") as tmp:
            artifact = Path(tmp) / "hf.json"
            subprocess.run(
                ["hyperfine", "--warmup", str(warmup), "--runs", str(runs), "--style", "basic",
                 "--export-json", str(artifact), "--prepare", prepare, "--command-name", name, cmd],
                capture_output=True, text=True,
            )
            if artifact.is_file():
                try:
                    return list(json.loads(artifact.read_text()).get("results", [{}])[0].get("times", []) or [])
                except (json.JSONDecodeError, OSError, IndexError, KeyError, TypeError):
                    pass
    times: list[float] = []
    if cold_warmup:
        subprocess.run(["bash", "-c", f"{prepare}; {cmd}"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    # Prefer GNU /usr/bin/time -f; fall back to python wall clock (macOS BSD time has no -f).
    use_gnu_time = False
    probe = subprocess.run(["/usr/bin/time", "-f", "%e", "true"], capture_output=True, text=True)
    if probe.returncode == 0:
        try:
            float((probe.stderr or probe.stdout).strip().splitlines()[-1])
            use_gnu_time = True
        except (ValueError, IndexError):
            use_gnu_time = False
    for _ in range(runs):
        subprocess.run(["bash", "-c", prepare], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if use_gnu_time:
            with tempfile.NamedTemporaryFile("w+", delete=False) as tf:
                path = tf.name
            try:
                script = f"/usr/bin/time -f '%e' bash -c {json.dumps(cmd)} >/dev/null 2>>{json.dumps(path)}"
                subprocess.run(["bash", "-c", script], check=False)
                for line in Path(path).read_text().splitlines():
                    try:
                        times.append(float(line.strip()))
                    except ValueError:
                        pass
            finally:
                try:
                    os.unlink(path)
                except OSError:
                    pass
        else:
            start = time.perf_counter()
            subprocess.run(["bash", "-c", cmd], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            times.append(time.perf_counter() - start)
    return times


def measure_cell(label: str, cmd: str, *, cold: bool = False, runs: int = 50, warmup: int = 3) -> tuple[int, int, int]:
    prepare = f"rm -f {RECOVERY_CACHE}" if cold else "true"
    p50, p90, p99 = percentiles_ms(_times(cmd, runs, warmup, prepare, label, cold_warmup=cold))
    return p50, p90, p99


def measure_median(label: str, cmd: str, *, runs: int = 5, warmup: int = 1) -> tuple[int, int, int]:
    wall = median_ms(_times(cmd, runs, warmup, "true", label))
    try:
        raw = subprocess.run(["bash", "-c", cmd], capture_output=True, check=False).stdout
    except OSError:
        raw = b""
    return wall, len(raw), token_estimate(len(raw))


def mcp_schema_tokens(cap_file: Path | str, tools_csv: str) -> int:
    tools = [t.strip() for t in tools_csv.split(",") if t.strip()]
    cap = json.loads(Path(cap_file).read_text())
    by_name = cap.get("commands_by_name", {})
    out: dict[str, Any] = {}
    for t in tools:
        if t in by_name:
            out[t] = by_name[t]
            continue
        for c in cap.get("commands", []):
            if c.get("name") == t:
                out[t] = c
                break
    return token_estimate(json.dumps(out, separators=(",", ":"), ensure_ascii=False).encode("utf-8"))


def quality_check(task: str, payload: str | dict[str, Any]) -> str:
    if isinstance(payload, str):
        try:
            d = json.loads(payload)
        except json.JSONDecodeError:
            return "FAIL"
    else:
        d = payload
    if isinstance(d, dict):
        val = d.get("value", {})
        vis = d.get("visible", {})
        if isinstance(val, dict) and "text" in val:
            text = str(val["text"])
        elif isinstance(vis, dict) and "text" in vis:
            text = str(vis["text"])
        else:
            text = json.dumps(d)
    else:
        text = str(d)
    low = text.lower()
    ok = {
        "read_file": "[workspace]" in text,
        "search_filter": "TokenZero" in text and text.count("\n") >= 1,
        "edit_verify": "BETA" in text and "beta" not in low,
        "multi_step_nav": "workspace" in low,
        "shell_expand": "Cargo.toml" in text,
    }.get(task, False)
    return "PASS" if ok else "FAIL"


def accounting_tokens(payload: str | dict[str, Any], key: str = "raw_tokens") -> int:
    if isinstance(payload, str):
        try:
            d = json.loads(payload)
        except json.JSONDecodeError:
            return 0
    else:
        d = payload
    if not isinstance(d, dict):
        return 0
    for bag in (d.get("accounting", {}), d.get("telemetry", {}), d.get("value", {}) if isinstance(d.get("value"), dict) else {}):
        if key in bag:
            return int(bag[key])
    return 0


def first_blob_ref(data: dict[str, Any] | str) -> str:
    if isinstance(data, str):
        try:
            data = json.loads(data)
        except json.JSONDecodeError:
            return ""
    for r in (data.get("refs", []) if isinstance(data, dict) else []):
        if isinstance(r, dict) and r.get("kind") == "blob":
            return str(r.get("ref", ""))
    return ""


def glob_root_and_first(data: dict[str, Any] | str) -> tuple[str, str]:
    if isinstance(data, str):
        try:
            data = json.loads(data)
        except json.JSONDecodeError:
            return "", ""
    text = str((data or {}).get("visible", {}).get("text", "") if isinstance(data, dict) else "")
    root = rel = ""
    for line in text.splitlines():
        if line.startswith("# root:"):
            root = line.split(":", 1)[1].strip()
        elif line.strip() and not line.strip().startswith("#") and not rel:
            rel = line.strip()
    return root, rel


def main(argv: Sequence[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="benchmarks.harness")
    sub = p.add_subparsers(dest="action", required=True)
    def sp(n: str) -> argparse.ArgumentParser: return sub.add_parser(n)
    s = sp("resolve_bin"); s.add_argument("--profile", default="release")
    sp("now_ms")
    s = sp("tok"); s.add_argument("--bytes", type=int, default=None)
    s = sp("percentiles"); g = s.add_mutually_exclusive_group(required=True); g.add_argument("--json"); g.add_argument("--times")
    s = sp("measure_cell"); s.add_argument("label"); s.add_argument("cmd"); s.add_argument("--cold", action="store_true"); s.add_argument("--runs", type=int, default=50); s.add_argument("--warmup", type=int, default=3)
    s = sp("measure_median"); s.add_argument("label"); s.add_argument("cmd"); s.add_argument("--runs", type=int, default=5); s.add_argument("--warmup", type=int, default=1)
    s = sp("mcp_schema_tokens"); s.add_argument("cap_file"); s.add_argument("tools")
    s = sp("quality"); s.add_argument("task")
    sp("clear_cache")
    s = sp("git_commit"); s.add_argument("--short", action="store_true")
    s = sp("accounting"); s.add_argument("--file"); s.add_argument("--key", default="raw_tokens")
    s = sp("first_blob_ref"); s.add_argument("file")
    s = sp("glob_pick"); s.add_argument("file")
    s = sp("generate_million"); s.add_argument("root"); s.add_argument("--dirs", type=int, default=100); s.add_argument("--files", type=int, default=10); s.add_argument("--lines", type=int, default=1000); s.add_argument("--needle", default="BENCH_NEEDLE_FN")
    s = sp("tz_metrics"); s.add_argument("file"); s.add_argument("wall")
    a = p.parse_args(argv)
    if a.action == "resolve_bin":
        print(bin_path(profile=a.profile)); return 0
    if a.action == "now_ms":
        print(now_ms()); return 0
    if a.action == "tok":
        print(token_estimate(a.bytes if a.bytes is not None else sys.stdin.buffer.read())); return 0
    if a.action == "percentiles":
        times = (json.loads(Path(a.json).read_text()).get("results", [{}])[0].get("times", [])
                 if a.json else [float(x) for x in Path(a.times).read_text().splitlines() if x.strip()])
        print("\t".join(map(str, percentiles_ms(times)))); return 0
    if a.action == "measure_cell":
        print("\t".join(map(str, measure_cell(a.label, a.cmd, cold=a.cold, runs=a.runs, warmup=a.warmup)))); return 0
    if a.action == "measure_median":
        print("\t".join(map(str, measure_median(a.label, a.cmd, runs=a.runs, warmup=a.warmup)))); return 0
    if a.action == "mcp_schema_tokens":
        print(mcp_schema_tokens(a.cap_file, a.tools)); return 0
    if a.action == "quality":
        print(quality_check(a.task, sys.stdin.read())); return 0
    if a.action == "clear_cache":
        try: RECOVERY_CACHE.unlink()
        except FileNotFoundError: pass
        return 0
    if a.action == "git_commit":
        print(git_commit(short=a.short)); return 0
    if a.action == "accounting":
        print(accounting_tokens(Path(a.file).read_text() if a.file else sys.stdin.read(), key=a.key)); return 0
    if a.action == "first_blob_ref":
        print(first_blob_ref(Path(a.file).read_text())); return 0
    if a.action == "glob_pick":
        r, rel = glob_root_and_first(Path(a.file).read_text()); print(f"{r}\t{rel}"); return 0
    if a.action == "generate_million":
        root = Path(a.root); root.mkdir(parents=True, exist_ok=True)
        million_line_repo(root, a.dirs, a.files, a.lines, a.needle); print("done"); return 0
    if a.action == "tz_metrics":
        try:
            acc = json.loads(Path(a.file).read_text()).get("accounting", {})
            print(f'{acc.get("visible_tokens", 0)}\t{acc.get("raw_tokens", 0)}\t{a.wall}')
        except (json.JSONDecodeError, OSError):
            print(f"0\t0\t{a.wall}")
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
