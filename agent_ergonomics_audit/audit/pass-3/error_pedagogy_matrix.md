# Pass 3 -- Error Pedagogy Matrix

Generated: 2026-07-27T23:14:24Z
Mode: audit-only. Live `tokenzero` binary probes.

## Rubric (Error-Teaches)

| Part | Meaning |
|---|---|
| **(a)** What failed | Names the bad flag/arg/subcommand or domain failure |
| **(b)** Where / why | Context: which command surface, constraint, or root cause |
| **(c)** Exact corrected invocation | Copy-pasteable `tokenzero ...` the agent can re-run |

Scores: `Y` = present, `P` = partial, `N` = missing/wrong.

## Summary

| Verdict | Count | % |
|---|---:|---:|
| PASS (a+b+c all Y) | 0 | 0% |
| PARTIAL | 6 | 40% |
| FAIL | 9 | 60% |

**Zero of 15 errors fully satisfy Error-Teaches (a)(b)(c).** Best cluster: clap required-arg Usage lines (E03/E11) and edit domain ladder (E15). Worst: wrong did-you-mean (E09, E14), global typo silence (E08), bare `read` (E02).

## Matrix

| ID | Invocation | exit | (a) | (b) | (c) | Verdict | Notes |
|---|---|---:|:---:|:---:|:---:|---|---|
| E01 | `tokenzero foobarbaz` | 2 | Y | P | N | **FAIL** | unrecognized subcommand; Usage root only; no nearest real verbs; no copy-paste example. |
| E02 | `tokenzero read` | 1 | Y | P | N | **FAIL** | Error: read requires a path -- names failure but no `tokenzero read <path> --json` example. |
| E03 | `tokenzero find` | 2 | Y | Y | P | **PARTIAL** | clap required-args lists <QUERY> + Usage line; no worked example with --json. |
| E04 | `tokenzero edit --force README.md` | 2 | Y | N | N | **FAIL** | clap tip to pass --force as value via `-- --force` is actively misleading (no such option). |
| E05 | `tokenzero edit README.md --dry-run` | 1 | Y | Y | P | **PARTIAL** | names --edits-json or --stdin; still no full paste-ready command with hunk shape. |
| E06 | `tokenzero expand tz://blob/notarealref` | 1 | Y | P | N | **FAIL** | zeroref_malformed on stdout; no how-to mint a ref or example expand command. |
| E07 | `tokenzero read /no/such/path/zzz.txt --json` | 1 | Y | Y | N | **FAIL** | structured JSON path_not_allowed; good machine form; no corrected roots command. |
| E08 | `tokenzero --jsonn` | 2 | Y | N | N | **FAIL** | global typo of --json; NO did-you-mean (subcommand --jsno recovers). Classic pass1 gap. |
| E09 | `tokenzero read --exlpain README.md` | 2 | Y | N | N | **FAIL** | WRONG did-you-mean: suggests --help. Teaches false association. |
| E10 | `tokenzero run --json` | 1 | Y | Y | P | **PARTIAL** | run requires a command after -- ; almost exact but omits full paste line. |
| E11 | `tokenzero glob` | 2 | Y | Y | P | **PARTIAL** | required <PATTERN> + Usage; no --json example. |
| E12 | `tokenzero tree /no/such/dir` | 1 | Y | Y | N | **FAIL** | path_not_allowed with path named; no roots/doctor recovery command. |
| E13 | `tokenzero robot-docs` | 2 | Y | Y | P | **PARTIAL** | prints subcommand list; guide empty description; no default to guide. |
| E14 | `tokenzero ls .` | 2 | Y | N | N | **FAIL** | WRONG did-you-mean: suggests false-success-shell. |
| E15 | `tokenzero edit ... hunk miss` | 1 | Y | Y | P | **PARTIAL** | domain hunk_not_found + write recovery ladder; weak for exact hunk re-run. |

## Patterns

1. **Clap default envelope** dominates usage errors: `For more information, try '--help'` without a corrected full command.
2. **Did-you-mean quality is untrusted**: `--exlpain` → `--help`; `ls` → `false-success-shell`. Wrong tips are worse than no tip (R-013).
3. **Subcommand flag typos often tip correctly** (`--editsjson` → `--edits-json`, `--startline` → `--start-line`) but still lack the full re-run line with values filled in.
4. **Custom domain errors** (edit hunk, path_not_allowed) are richer than clap, but still omit exact re-invocation with fixed args.
5. **Stdout vs stderr split inconsistent**: edit ladder and expand errors land on **stdout**; clap on **stderr**. Agents parsing stderr miss domain ladders.
6. **Bare `read`** (E02) is worse than bare `find` (E03): no Usage line at all.

## Residual vs Pass 1 R-003

R-003 (Error-Teaches rewrite) remains open. Pass 3 evidence strengthens it: even "good" tips stop short of paste-ready corrected commands, and two wrong-hints actively regress pedagogy.

Raw transcripts: `pass-3/pedagogy_raw/`, `pass-3/error_transcripts/`.
