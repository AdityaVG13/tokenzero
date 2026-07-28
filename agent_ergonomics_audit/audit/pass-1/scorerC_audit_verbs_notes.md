# Scorer C — thin help / audit / eval verb family (Pass 1)

- **Scorer:** C (independent deep runtime probes)
- **Pass:** 1
- **Rubric:** 1.0.0
- **Scored at:** 2026-07-27T23:10:46Z
- **Count:** 28 verbs
- **Hard rule:** no cargo test --workspace

## Summary metrics

| Metric | Value |
|--------|-------|
| Scored count | 28 |
| Mean `self_documentation` | 101.8 |
| Mean `agent_ease_of_use` | 167.9 |
| Mean `agent_intuitiveness` | 444.6 |
| Mean `error_pedagogy` | 355.4 |
| Mean weighted | 356.0 |
| Self-doc min / max | 50 / 250 |
| Scores >700 | **none** (no evidence bar triggered) |

## Capabilities parity (self-doc gap)

`tokenzero capabilities --json` currently lists **17** primary commands  
(`capabilities`, `codemode`, `doctor`, `edit`, `expand`, `fetch`, `find`, `glob`, `hook`, `install`, `mem`, `pulse`, `read`, `recall`, `robot-docs`, `run`, `tree`).

**All 28 family verbs are missing from capabilities:**

`adapter-approval-audit`, `adapter-approval-template`, `artifact-handoff`, `bench`, `cache-pack`, `claim-audit`, `completion-audit`, `exact-recovery-audit`, `exact-recovery-shell`, `false-success-shell`, `harm-eval`, `install-smoke`, `mcp-server`, `mcp-smoke`, `mcp-soak`, `one-shot-eval`, `os-reach-audit`, `os-release-artifact`, `package-audit`, `prompt-cache-pack`, `protected-anchor-audit`, `quote`, `reach`, `repo-inventory`, `security-privacy-audit`, `shell-matrix`, `source-currency-audit`, `ws-skeleton`

`tokenzero robot-docs guide` also does not document this family (probe: no hits for claim-audit/harm-eval/mcp-smoke/cache-pack/package-audit/etc.).

Top-level `tokenzero --help` lists every verb with an **empty description column** (clap about missing).

## Probe method

1. `tokenzero <cmd> --help` → `audit/partial/help_probes/<cmd>.help.txt` (exit 0 all)
2. Safe bare / `--json` (tmpdir roots; skip bare `mcp-server`; alarm timeouts) → `audit/partial/bare_probes/`
3. Unknown flag `--not-a-real-flag` → `audit/partial/error_probes/scorerC/`
4. Global subcommand typos (`claim-auditt`, `harmeval`, `mcp-smok`, `cachepack`, `packge-audit`)
5. Flag typos (`--outptu-json`, `--josn`) on claim-audit / package-audit
6. Caps dump → `audit/partial/tz_caps_full.json`

## Cross-cutting findings

### Critical self-doc failures
- Empty top-level help blurbs for entire family.
- Subcommand `--help` is almost pure flag lists (no about, no examples, no when-to-use, no related verbs).
- Zero representation in `capabilities --json` / `robot-docs`.
- **Default path collision:** `harm-eval`, `mcp-smoke`, `mcp-soak`, `prompt-cache-pack`, `repo-inventory` all default `--output-json` to `results/current/rust_mcp_smoke.json` — agents will clobber unrelated audits.

### What works (runtime > help)
- Most audit/eval verbs **succeed on bare/`--json`** with stable-looking `schema_version` + `ok`/`status` fields.
- Strong **global subcommand typo recovery** (clap similar-subcommands tips).
- Flag edit-distance tips for `--json` / `--output-json` misspellings.
- Domain JSON often carries pedagogical fields (`blocked_reasons`, hazard rows) even when CLI help does not.

### Safety notes
- `install-smoke` bare **applies** install (dry_run false) with rollback metadata; help shows no dry-run gate.
- `ws-skeleton` writes many `results/current/tokenzero_ws_001_*.json` artifacts; `--json --output-json` timed out in probe.
- `mcp-server` is long-running stdio; not a first-try JSON tool.

### Standouts (relative within family)
| Verb | Why higher/lower |
|------|------------------|
| `package-audit`, `reach`, `cache-pack` | Cleanest bare JSON; slightly higher parseability/intuitiveness |
| `claim-audit`, `source-currency-audit`, `false-success-shell` | Runtime JSON teaches; help still empty |
| `mcp-server` | Only member with useful option prose (`--mode`) |
| `harm-eval` / `mcp-smoke` / `mcp-soak` / `repo-inventory` / `prompt-cache-pack` | Lowest self-doc (path collision) |
| `install-smoke` | Lowest safety |
| `ws-skeleton`, `bench` | Weak bare first-try |
| `quote` | Required `--platform` with no platform enumeration |

## Per-verb score snapshot (emphasized dims)

| Verb | self_doc | ease | intuit | err_ped | weighted | in_caps |
|------|---------:|-----:|-------:|-------:|---------:|:-------:|
| `harm-eval` | 50 | 150 | 500 | 350 | 354.5 | no |
| `mcp-smoke` | 50 | 150 | 450 | 350 | 340.9 | no |
| `mcp-soak` | 50 | 150 | 450 | 350 | 340.9 | no |
| `prompt-cache-pack` | 50 | 150 | 450 | 350 | 354.5 | no |
| `repo-inventory` | 50 | 150 | 400 | 350 | 340.9 | no |
| `adapter-approval-audit` | 100 | 200 | 450 | 350 | 363.6 | no |
| `adapter-approval-template` | 100 | 150 | 450 | 350 | 359.1 | no |
| `artifact-handoff` | 100 | 150 | 450 | 350 | 359.1 | no |
| `bench` | 100 | 150 | 250 | 300 | 286.4 | no |
| `claim-audit` | 100 | 200 | 500 | 450 | 386.4 | no |
| `completion-audit` | 100 | 150 | 450 | 350 | 359.1 | no |
| `exact-recovery-audit` | 100 | 150 | 450 | 350 | 354.5 | no |
| `exact-recovery-shell` | 100 | 150 | 500 | 350 | 368.2 | no |
| `false-success-shell` | 100 | 150 | 500 | 400 | 372.7 | no |
| `install-smoke` | 100 | 150 | 350 | 300 | 309.1 | no |
| `one-shot-eval` | 100 | 150 | 450 | 400 | 363.6 | no |
| `os-reach-audit` | 100 | 200 | 500 | 350 | 377.3 | no |
| `os-release-artifact` | 100 | 200 | 500 | 350 | 372.7 | no |
| `protected-anchor-audit` | 100 | 150 | 450 | 350 | 359.1 | no |
| `quote` | 100 | 150 | 350 | 400 | 309.1 | no |
| `security-privacy-audit` | 100 | 150 | 500 | 350 | 372.7 | no |
| `shell-matrix` | 100 | 150 | 450 | 350 | 359.1 | no |
| `source-currency-audit` | 100 | 200 | 500 | 450 | 381.8 | no |
| `ws-skeleton` | 100 | 100 | 250 | 250 | 277.3 | no |
| `cache-pack` | 150 | 200 | 500 | 350 | 395.5 | no |
| `package-audit` | 150 | 200 | 550 | 350 | 409.1 | no |
| `reach` | 150 | 200 | 550 | 350 | 404.5 | no |
| `mcp-server` | 250 | 300 | 300 | 350 | 336.4 | no |

## Artifact paths

- Scores: `agent_ergonomics_audit/audit/partial/scores_pass1_scorerC_audit_verbs.jsonl`
- Notes: `agent_ergonomics_audit/audit/pass-1/scorerC_audit_verbs_notes.md`
- Help probes: `agent_ergonomics_audit/audit/partial/help_probes/*.help.txt`
- Bare probes: `agent_ergonomics_audit/audit/partial/bare_probes/*.bare.txt`
- Error probes: `agent_ergonomics_audit/audit/partial/error_probes/scorerC/`

## Independence

Scorer C did not use scorer A/B numeric outputs for calibration. Shared inventory/help dumps under `audit/partial/` were used as fixture inputs; scores derive from scorerC-run bare/error probes and rubric anchors.
