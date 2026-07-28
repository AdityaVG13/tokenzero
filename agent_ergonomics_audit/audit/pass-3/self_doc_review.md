# Pass 3 -- Self-Doc Review (robot-docs vs live behavior)

Generated: 2026-07-27T23:14:24Z
Sources: `tokenzero robot-docs guide|commands|examples`, live CLI probes.

## Scope

10 documented primary commands / recovery claims from the robot guide **First Commands / Context / Shell / Safe Mutation** sections, plus related recovery claims.

## Results

| # | Documented claim | Live probe | Result |
|---:|---|---|---|
| 1 | `tokenzero capabilities --json` discovers contract | exit 0; schema_version present; stderr empty | **MATCH** |
| 2 | `tokenzero search <q> <path> --json` alias for find | exit 0 | **MATCH** |
| 3 | `tokenzero run true --json` recovers to canonical run | exit 0 | **MATCH** |
| 4 | `tokenzero rn true --json` typo for run | exit 0 | **MATCH** |
| 5 | `tokenzero doctor status --json` read-side recovery | exit 0 | **MATCH** |
| 6 | `tokenzero pulse stats --json` recovery | exit 0; pulse schema | **MATCH** |
| 7 | `tokenzero cache statuz --json` recovery | exit 0 | **MATCH** |
| 8 | `tokenzero install status --json` → clients detect | exit 0; command=clients detect | **MATCH** |
| 9 | `tokenzero install --plan --json` safe plan | exit 0 | **MATCH** |
| 10 | `tokenzero run --jsno` / `--timout` normalized | both exit 0 | **MATCH** |

**Bonus checks:**

| Claim | Result | Notes |
|---|---|---|
| `tokenzero --robot-help` / `robot-help` | **MATCH** | Both emit guide text (exit 0) |
| `capability` / `capabilites` typos | **MATCH** | Recover to capabilities JSON |
| `tokenzero cache prune --json` dry-run unless `--apply` | **MATCH** | dry_run=true without --apply |
| `tokenzero robot-docs manual` | **MATCH** | Treated as guide alias |
| `tokenzero codemode 'search:read'` | **MISMATCH / env** | exit 1: surface-codemode feature not in this artifact |
| Root `tokenzero --robot-triage` | **ABSENT** | exit 2 unexpected argument |
| Root `tokenzero robot-triage` | **ABSENT** | exit 2 unrecognized subcommand |
| `tokenzero doctor --robot-triage` | **EXISTS (undocumented in guide First Commands)** | exit 0; schema tokenzero.doctor.robot_triage.v1 |

## Documentation quality gaps

1. **Guide omits the only real mega-command**: `doctor --robot-triage` works, but First Commands lists other doctor forms instead.
2. **capabilities.commands incomplete**: 17 rows vs ~60 help verbs; 28 empty one-line help descriptions.
3. **CodeMode section oversells** relative to this binary (missing surface-codemode feature).
4. **Recoveries list is accurate** for wired aliases; does not warn that global flag typos are unforgiving.
5. **Stdout contract** claim is violated by domain errors on stdout (edit ladder, expand malformed).

## Score impact

Self-documentation remains a relative strength for the **primary agent path** (10/10 guide claims matched). Residual gap is **coverage and discoverability**, not false happy-path claims.

## Recommendation hooks

- R-001b: advertise `doctor --robot-triage` in guide First Commands + capabilities + root aliases.
- R-004: fill empty help blurbs; expand capabilities.commands or mark experimental.
- R-016: CodeMode guide gated on feature_flags.codemode_surface.
