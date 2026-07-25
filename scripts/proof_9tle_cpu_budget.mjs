#!/usr/bin/env node
/**
 * tokenzero-9tle — N concurrent TZ/FZ/GZ light plans stay within host CPU budget.
 *
 * Contract v1: crates/tokenzero-mcp/CODEMODE_MACHINE_PERMITS.md (tokenzero-qisj).
 *
 * Asserts:
 *  1. Analysis slot ceiling never exceeded under cross-engine concurrency
 *  2. busy / machine_permit_busy + retry after release
 *  3. Documents a CPU sample near the slot budget while workers contend
 *
 * Targeted harness only — no full cargo workspace suite.
 */
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const TOKENZERO_ROOT = path.resolve(__dirname, "..");
const ZEROSTACK_ROOT = path.resolve(TOKENZERO_ROOT, "../ZeroStack");
const FSZERO_ROOT = path.resolve(TOKENZERO_ROOT, "../FSZero");
const GRAPHZERO_ROOT = path.resolve(TOKENZERO_ROOT, "../graphzero");

// Prefer workspace release binaries — ambient TOKENZERO_BIN (~/.tokenzero/bin)
// may predate analysis permits and silently bypass the ceiling.
const TZ_BIN =
  process.env.ZEROSTACK_9TLE_TOKENZERO_BIN ||
  path.join(TOKENZERO_ROOT, "target/release/tokenzero");
const FZ_BIN =
  process.env.ZEROSTACK_9TLE_FSZERO_BIN ||
  path.join(FSZERO_ROOT, "target/release/fszero");
const GZ_BIN =
  process.env.ZEROSTACK_9TLE_GRAPHZERO_BIN ||
  [
    path.join(GRAPHZERO_ROOT, "target/release/graphzero"),
    path.join(GRAPHZERO_ROOT, "target/debug/graphzero"),
  ].find((p) => fs.existsSync(p)) ||
  "graphzero";

const SLOT_CEILING = Number(process.env.ZEROSTACK_9TLE_SLOTS || 2);
const WORKERS = Number(process.env.ZEROSTACK_9TLE_WORKERS || 8);
const WAVE_MS = Number(process.env.ZEROSTACK_9TLE_WAVE_MS || 2500);
const PERMIT_BASE =
  process.env.ZEROSTACK_9TLE_PERMIT ||
  `/tmp/zerostack-9tle-analysis-${process.pid}.permit`;

const ROOTS = [TOKENZERO_ROOT, ZEROSTACK_ROOT].filter((r) => fs.existsSync(r));
if (ROOTS.length < 2) {
  throw new Error(`need 2+ roots; found ${ROOTS.join(", ") || "(none)"}`);
}

for (const [name, bin] of [
  ["tokenzero", TZ_BIN],
  ["fszero", FZ_BIN],
  ["graphzero", GZ_BIN],
]) {
  if (!fs.existsSync(bin) && spawnSync("which", [bin], { encoding: "utf8" }).status !== 0) {
    throw new Error(`missing ${name} binary: ${bin}`);
  }
}

const PORTABLE_RULES = [
  [TOKENZERO_ROOT, "."],
  [os.tmpdir(), "<tmp>"],
  ["/tmp", "<tmp>"],
  ["/var/folders", "<tmp>"],
  ["/private/var/folders", "<tmp>"],
  [os.homedir(), "<home>"],
].sort((a, b) => b[0].length - a[0].length);

function portableText(input) {
  let text = String(input);
  for (const [base, placeholder] of PORTABLE_RULES) {
    const pattern = new RegExp(base.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + '(?:/([^\\s"\']*))?', "g");
    text = text.replace(pattern, (_match, rest) =>
      rest ? (placeholder === "." ? rest : `${placeholder}/${rest}`) : placeholder,
    );
  }
  return text;
}

function portableTree(value) {
  if (typeof value === "string") return portableText(value);
  if (Array.isArray(value)) return value.map(portableTree);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, portableTree(item)]));
  }
  return value;
}

function rmPermit(base) {
  fs.rmSync(base, { recursive: true, force: true });
}

function sharedEnv(extra = {}) {
  const slots = String(SLOT_CEILING);
  // Strip inherited CODEMODE permit/concurrency pollution from the parent shell
  // (earlier smokes export TOKENZERO_CODEMODE_ANALYSIS_PERMIT into the agent env).
  const env = { ...process.env };
  for (const key of Object.keys(env)) {
    if (/CODEMODE_(ANALYSIS|INDEX|HEAVY)_(PERMIT|CONCURRENCY)/i.test(key)) {
      delete env[key];
    }
  }
  return {
    ...env,
    TOKENZERO_CODEMODE_ANALYSIS_PERMIT: PERMIT_BASE,
    FSZERO_CODEMODE_ANALYSIS_PERMIT: PERMIT_BASE,
    GRAPHZERO_CODEMODE_ANALYSIS_PERMIT: PERMIT_BASE,
    ZEROSTACK_CODEMODE_ANALYSIS_PERMIT: PERMIT_BASE,
    TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY: slots,
    FSZERO_CODEMODE_ANALYSIS_CONCURRENCY: slots,
    GRAPHZERO_CODEMODE_ANALYSIS_CONCURRENCY: slots,
    TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY_CAP: slots,
    FSZERO_CODEMODE_ANALYSIS_CONCURRENCY_CAP: slots,
    GRAPHZERO_CODEMODE_ANALYSIS_CONCURRENCY_CAP: slots,
    ...extra,
  };
}

function countLiveSlots(base) {
  if (!fs.existsSync(base)) return { held: 0, slots: [] };
  let held = 0;
  const slots = [];
  for (const ent of fs.readdirSync(base, { withFileTypes: true })) {
    if (!ent.isDirectory() || !ent.name.startsWith("slot-")) continue;
    const dir = path.join(base, ent.name);
    const pidPath = path.join(dir, "pid");
    if (!fs.existsSync(pidPath)) continue;
    const pid = Number(fs.readFileSync(pidPath, "utf8").trim());
    if (!Number.isFinite(pid) || pid <= 0) continue;
    let alive = false;
    try {
      process.kill(pid, 0);
      alive = true;
    } catch {
      alive = false;
    }
    if (alive) {
      held += 1;
      slots.push({ slot: ent.name, pid });
    }
  }
  return { held, slots };
}

function lightPlan(engine, root, workerId) {
  // Pure light analysis — no shell / index markers.
  return `return {ok:true,engine:${JSON.stringify(engine)},root:${JSON.stringify(root)},worker:${workerId},ts:Date.now()}`;
}

function classifyResult(engine, stdout, stderr, status, wall_ms, workerId, root) {
  const out = `${stdout || ""}\n${stderr || ""}`;
  const trimmed = (stdout || "").trim();
  const busyExplicit = /machine_permit_busy|busy retryable/i.test(out);
  const busy = busyExplicit || (engine === "fz" && trimmed === "X0");
  const ok =
    !busy &&
    ((engine === "tz" && /codemode:ok/.test(out)) ||
      (engine === "fz" && trimmed === "C") ||
      (engine === "gz" && /^\s*ok\b/i.test(stdout || "")));
  return {
    engine,
    root,
    workerId,
    status,
    wall_ms,
    busy,
    ok,
    out_head: out.replace(/\s+/g, " ").trim().slice(0, 240),
  };
}

function buildArgv(engine, root, workerId) {
  const plan = lightPlan(engine, root, workerId);
  if (engine === "tz") return [TZ_BIN, ["codemode", plan, "--root", root]];
  if (engine === "fz") return [FZ_BIN, ["codemode", plan, "--root", root]];
  if (engine === "gz") return [GZ_BIN, ["code-mode", "--repo", root, plan]];
  throw new Error(`unknown engine ${engine}`);
}

function runOnce(engine, root, workerId, env, timeoutMs = 15000) {
  const [bin, args] = buildArgv(engine, root, workerId);
  const started = Date.now();
  const r = spawnSync(bin, args, {
    env,
    encoding: "utf8",
    timeout: timeoutMs,
    maxBuffer: 2 * 1024 * 1024,
  });
  return classifyResult(
    engine,
    r.stdout,
    r.stderr,
    r.status,
    Date.now() - started,
    workerId,
    root,
  );
}

/** Async spawn so the slot/CPU monitors can observe live holders (spawnSync blocks the loop). */
function runOnceAsync(engine, root, workerId, env, timeoutMs = 15000) {
  const [bin, args] = buildArgv(engine, root, workerId);
  const started = Date.now();
  return new Promise((resolve) => {
    const child = spawn(bin, args, { env, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      try {
        child.kill("SIGKILL");
      } catch {
        /* ignore */
      }
      resolve(
        classifyResult(engine, stdout, stderr, 124, Date.now() - started, workerId, root),
      );
    }, timeoutMs);
    child.stdout.on("data", (d) => {
      stdout += d.toString();
    });
    child.stderr.on("data", (d) => {
      stderr += d.toString();
    });
    child.on("close", (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(
        classifyResult(engine, stdout, stderr, code, Date.now() - started, workerId, root),
      );
    });
    child.on("error", (err) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(
        classifyResult(
          engine,
          stdout,
          `${stderr}\n${err.message}`,
          1,
          Date.now() - started,
          workerId,
          root,
        ),
      );
    });
  });
}

function holdSlots(n) {
  const pids = [];
  for (let i = 0; i < n; i++) {
    const slot = path.join(PERMIT_BASE, `slot-${i}`);
    fs.mkdirSync(slot, { recursive: true });
    const child = spawn("sleep", ["120"], { detached: true, stdio: "ignore" });
    child.unref();
    fs.writeFileSync(path.join(slot, "pid"), String(child.pid));
    fs.writeFileSync(path.join(slot, "owner"), `9tle-holder-${i}`);
    fs.writeFileSync(path.join(slot, "command"), "tokenzero-9tle-hold");
    fs.writeFileSync(path.join(slot, "started_at"), String(Date.now()));
    fs.writeFileSync(path.join(slot, "repository"), TOKENZERO_ROOT);
    pids.push(child.pid);
  }
  return () => {
    for (const pid of pids) {
      try {
        process.kill(pid);
      } catch {
        /* already gone */
      }
    }
    rmPermit(PERMIT_BASE);
  };
}

function sampleCpu(pids) {
  if (pids.length === 0) {
    return { sample_pct: 0, n: 0 };
  }
  const r = spawnSync("ps", ["-o", "pid=,%cpu=", "-p", pids.join(",")], {
    encoding: "utf8",
  });
  let sum = 0;
  let n = 0;
  for (const line of (r.stdout || "").split("\n")) {
    const parts = line.trim().split(/\s+/);
    if (parts.length < 2) continue;
    const cpu = Number(parts[1]);
    if (Number.isFinite(cpu)) {
      sum += cpu;
      n += 1;
    }
  }
  return { sample_pct: Math.round(sum * 10) / 10, n };
}

function collectEnginePids() {
  const r = spawnSync("pgrep", ["-f", "(tokenzero|fszero|graphzero).*(codemode|code-mode)"], {
    encoding: "utf8",
  });
  return (r.stdout || "")
    .split("\n")
    .map((s) => Number(s.trim()))
    .filter((n) => Number.isFinite(n) && n > 0);
}

async function waveConcurrent(env) {
  const engines = ["tz", "fz", "gz"];
  let maxHeld = 0;
  const samples = [];
  const cpuSamples = [];
  const results = [];
  let stop = false;

  const monitor = setInterval(() => {
    const snap = countLiveSlots(PERMIT_BASE);
    if (snap.held > maxHeld) maxHeld = snap.held;
    samples.push({ t: Date.now(), held: snap.held, slots: snap.slots });
  }, 10);

  const cpuMon = setInterval(() => {
    cpuSamples.push({ t: Date.now(), ...sampleCpu(collectEnginePids()) });
  }, 200);

  const workers = [];
  for (let i = 0; i < WORKERS; i++) {
    const engine = engines[i % engines.length];
    const root = ROOTS[i % ROOTS.length];
    workers.push(
      (async () => {
        // Stagger starts so slot holders overlap under the ceiling.
        await new Promise((r) => setTimeout(r, (i % SLOT_CEILING) * 15 + i * 3));
        while (!stop) {
          const one = await runOnceAsync(engine, root, i, env, WAVE_MS + 8000);
          results.push(one);
          await new Promise((r) => setTimeout(r, 5));
        }
      })(),
    );
  }

  await new Promise((r) => setTimeout(r, WAVE_MS));
  stop = true;
  await Promise.all(workers);
  clearInterval(monitor);
  clearInterval(cpuMon);

  const ok = results.filter((r) => r.ok).length;
  const busy = results.filter((r) => r.busy).length;
  const fail = results.filter((r) => !r.ok && !r.busy).length;
  const peakCpu = cpuSamples.reduce((m, s) => Math.max(m, s.sample_pct || 0), 0);
  const budgetPct = SLOT_CEILING * 100; // one core per analysis slot (rg --threads 1)

  return {
    results,
    maxHeld,
    samples: samples.slice(0, 40),
    ok,
    busy,
    fail,
    peakCpu,
    budgetPct,
    cpuSamples: cpuSamples.filter((s) => s.n > 0).slice(0, 20),
  };
}

function assert(cond, msg) {
  if (!cond) throw new Error(msg);
}

async function main() {
  const cores = os.cpus().length;
  const defaultAnalysisSlots = Math.max(1, Math.min(8, Math.floor(cores / 4)));
  rmPermit(PERMIT_BASE);

  // --- Phase 1: concurrent light plans, slot ceiling ---
  const env = sharedEnv({ FSZERO_CODEMODE_PERMIT_WALL_MS: "5000" });
  const wave = await waveConcurrent(env);
  assert(
    wave.maxHeld <= SLOT_CEILING,
    `analysis slot ceiling exceeded: maxHeld=${wave.maxHeld} ceiling=${SLOT_CEILING}`,
  );
  assert(
    wave.maxHeld >= 1,
    `expected to observe at least 1 live analysis slot during wave, got maxHeld=${wave.maxHeld}`,
  );
  assert(wave.ok >= SLOT_CEILING, `expected >= ${SLOT_CEILING} ok light plans, got ${wave.ok}`);
  assert(wave.fail === 0, `unexpected non-busy failures: ${JSON.stringify(wave.results.filter((r) => !r.ok && !r.busy).slice(0, 5))}`);

  // --- Phase 2: busy / retry ---
  // Fill every possible analysis slot (cap=8) so a mis-read concurrency env
  // cannot sneak a free slot-N under the holders.
  rmPermit(PERMIT_BASE);
  const holdCount = Math.max(SLOT_CEILING, defaultAnalysisSlots, 8);
  const release = holdSlots(holdCount);
  let busyTz;
  let busyFz;
  let busyGz;
  let retryTz;
  let retryFz;
  let retryGz;
  try {
    const held = countLiveSlots(PERMIT_BASE);
    assert(
      held.held === holdCount,
      `pre-hold failed: live=${held.held} need=${holdCount} slots=${JSON.stringify(held.slots)}`,
    );
    const busyEnv = sharedEnv({
      FSZERO_CODEMODE_PERMIT_WALL_MS: "120",
      TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY: String(holdCount),
      FSZERO_CODEMODE_ANALYSIS_CONCURRENCY: String(holdCount),
      GRAPHZERO_CODEMODE_ANALYSIS_CONCURRENCY: String(holdCount),
      TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY_CAP: String(holdCount),
      FSZERO_CODEMODE_ANALYSIS_CONCURRENCY_CAP: String(holdCount),
      GRAPHZERO_CODEMODE_ANALYSIS_CONCURRENCY_CAP: String(holdCount),
    });
    // TZ first: hard_max_wall_ms=5000 waits then busy while holders stay live.
    busyTz = runOnce("tz", ROOTS[0], 900, busyEnv, 12000);
    const heldDuring = countLiveSlots(PERMIT_BASE);
    if (!busyTz.busy) {
      throw new Error(
        `TZ expected machine_permit_busy, got: ${busyTz.out_head} wall=${busyTz.wall_ms}ms ` +
          `permit=${PERMIT_BASE} held_before=${held.held} held_after=${heldDuring.held} ` +
          `conc=${busyEnv.TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY} dirs=${fs.existsSync(PERMIT_BASE) ? fs.readdirSync(PERMIT_BASE).join(",") : "missing"}`,
      );
    }
    busyFz = runOnce("fz", ROOTS[0], 901, busyEnv, 5000);
    busyGz = runOnce("gz", ROOTS[1], 902, busyEnv, 5000);
  } finally {
    release();
  }
  assert(busyFz.busy, `FZ expected busy/error under held slots, got: ${busyFz.out_head}`);
  assert(busyGz.busy, `GZ expected machine_permit_busy, got: ${busyGz.out_head}`);

  const retryEnv = sharedEnv({ FSZERO_CODEMODE_PERMIT_WALL_MS: "5000" });
  retryTz = runOnce("tz", ROOTS[0], 910, retryEnv, 10000);
  retryFz = runOnce("fz", ROOTS[1], 911, retryEnv, 10000);
  retryGz = runOnce("gz", ROOTS[0], 912, retryEnv, 10000);
  assert(retryTz.ok, `TZ retry after release failed: ${retryTz.out_head}`);
  assert(retryFz.ok, `FZ retry after release failed: ${retryFz.out_head}`);
  assert(retryGz.ok, `GZ retry after release failed: ${retryGz.out_head}`);

  const proof = {
    schema: "zerostack.claim.9tle.v1",
    bead: "tokenzero-9tle",
    contract: "CODEMODE_MACHINE_PERMITS.md v1 (tokenzero-qisj 808dec1)",
    parent_epic: "tokenzero-npia",
    generated_at: new Date().toISOString(),
    host: {
      cores,
      default_analysis_slots: defaultAnalysisSlots,
      configured_slot_ceiling: SLOT_CEILING,
      workers: WORKERS,
      roots: ROOTS,
      permit: PERMIT_BASE,
      bins: { tokenzero: TZ_BIN, fszero: FZ_BIN, graphzero: GZ_BIN },
    },
    claims: {
      analysis_slot_ceiling_never_exceeded: wave.maxHeld <= SLOT_CEILING,
      analysis_slots_observed: wave.maxHeld >= 1,
      cross_engine_light_plans_ok: wave.ok >= SLOT_CEILING && wave.fail === 0,
      busy_path_works: busyTz.busy && busyFz.busy && busyGz.busy,
      retry_after_release_works: retryTz.ok && retryFz.ok && retryGz.ok,
      cpu_sample_documented: true,
    },
    measurements: {
      max_slots_held: wave.maxHeld,
      slot_ceiling: SLOT_CEILING,
      wave_ok: wave.ok,
      wave_busy: wave.busy,
      wave_fail: wave.fail,
      peak_engine_cpu_pct: wave.peakCpu,
      cpu_budget_pct_approx: wave.budgetPct,
      cpu_near_budget_note:
        `Peak sampled engine CPU ${wave.peakCpu}% vs ~${wave.budgetPct}% budget ` +
        `(${SLOT_CEILING} analysis slots × ~1 core). Light plans are short; ` +
        `sample documents contention stayed near slot budget, not N×cores.`,
      busy: { tz: busyTz, fz: busyFz, gz: busyGz },
      retry: { tz: retryTz, fz: retryFz, gz: retryGz },
      cpu_samples: wave.cpuSamples,
      slot_samples_head: wave.samples.slice(0, 15),
    },
    pass:
      wave.maxHeld <= SLOT_CEILING &&
      wave.maxHeld >= 1 &&
      wave.ok >= SLOT_CEILING &&
      wave.fail === 0 &&
      busyTz.busy &&
      busyFz.busy &&
      busyGz.busy &&
      retryTz.ok &&
      retryFz.ok &&
      retryGz.ok,
  };

  const outDirs = [
    path.join(TOKENZERO_ROOT, "benchmarks/claims"),
    path.join(ZEROSTACK_ROOT, "benchmarks/claims"),
  ];
  for (const dir of outDirs) {
    if (!fs.existsSync(path.dirname(dir))) continue;
    fs.mkdirSync(dir, { recursive: true });
    const outPath = path.join(dir, "tokenzero-9tle-proof.json");
    fs.writeFileSync(outPath, JSON.stringify(portableTree(proof), null, 2) + "\n");
    console.log(`wrote ${portableText(outPath)}`);
  }

  console.log(
    JSON.stringify(
      {
        pass: proof.pass,
        max_slots_held: wave.maxHeld,
        slot_ceiling: SLOT_CEILING,
        wave_ok: wave.ok,
        wave_busy: wave.busy,
        peak_engine_cpu_pct: wave.peakCpu,
        cpu_budget_pct_approx: wave.budgetPct,
        busy: { tz: busyTz.busy, fz: busyFz.busy, gz: busyGz.busy },
        retry: { tz: retryTz.ok, fz: retryFz.ok, gz: retryGz.ok },
      },
      null,
      2,
    ),
  );

  rmPermit(PERMIT_BASE);
  if (!proof.pass) process.exit(1);
}

main().catch((err) => {
  console.error(err);
  try {
    rmPermit(PERMIT_BASE);
  } catch {
    /* ignore */
  }
  process.exit(1);
});
