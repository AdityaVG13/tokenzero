# Requirements audit: RADC Wave-7 Sol Pro return

## Verdict: FAIL

The mathematical/EC payload is substantial and RUN_ALL is caller-CWD portable, but the final package violates exact-format gates and contains proof-corrupting control characters.

## Blockers and exact fixes

1. **BLOCKER -- exact headings:** `/Users/aditya/Downloads/RADC_WAVE7_SOLPRO_RETURN/RADC_WAVE7_PACKAGE.md:12,20,41,2268,2276,2292,3797,4123,4129,4149` uses shortened/changed wording. Only sections 0, 3, 9, and 10 match the goal exactly. Replace sections 1, 2, 4, 5, 6, 7, 8, 11, 12, and 13 with the exact strings in `pi-sol-goal-wave7.md`, preserving order. Title is exact.

2. **BLOCKER -- corrupted proofs/statements:** `RADC_WAVE7_PACKAGE.md` contains 87 forbidden control characters: TAB x61, FF x17, BS x7, VT x2. Backslashes were interpreted while assembling Markdown, corrupting formulas, including `\theta`, `\boxed`, `\begin`, `\frac`, and `\vartheta` (examples around logical lines 65, 192, 2297-2310). Regenerate from byte-safe/raw source, restore LaTeX, then gate on zero C0 controls except LF/CR. Until fixed, “full proofs” and exact theorem statements are not valid deliverables.

3. **BLOCKER -- shipped audit fails:** `python3 checkers/audit_package.py` exits 1 at `missing heading: ## 1. Effort budget log`. `RUN_ALL.sh` exits 0 only because it does not run that auditor and its inline check verifies section numbers plus seven tokens, not exact heading wording, 149 inventory rows, tags, EC exits, checksums, or ZIP contents. Fix headings, strengthen/integrate `audit_package.py`, and make RUN_ALL invoke it.

4. **BLOCKER -- incomplete peer log shape:** `RADC_WAVE7_PACKAGE.md:27-39` has seven data rows, combining `methods, SOLPRO_W5_CONT2, SOLPRO_W5_CONT1, WAVE4` into one row. The contract requires one row for each of ten exact labels. Split that row into four, yielding ten independently auditable rows.

5. **BLOCKER -- claim-tag coverage:** theorem-index rows are tagged, but affirmative theorem/proof statements are not uniformly marked with both proof-status and ambition tags. Example: `RADC_WAVE7_PACKAGE.md` section 7, `#### W7-SOL-SEQ-DOWN-STAIRCASE` and its statement carry no adjacent PI/DR/EC/BE/SB plus [S/F/M] declaration. Add explicit tags to every final claim/new object, not only the index.

6. **BLOCKER -- model identity:** required section 13 text is `Timestamp + model identity (GPT-5.6 Sol Pro)`; package says `## 13. Timestamp/model` and `GPT-5.6 Sol medium, standard tier`. Resolve truthfully with the acceptance owner. Do not relabel a medium run as Pro merely to satisfy a string gate.

7. **MAJOR -- README/audit contradiction:** `README.txt:5` and section 11 say `audit_package.py` is absent, but `checkers/audit_package.py` exists. README also says the structural audit is integrated while RUN_ALL uses a weaker replacement. Correct both statements after integrating the real gate.

## Requirements matrix

- **PASS:** exact title; section number/order; effort 58/27/15 satisfies >=55/>=25/<=20; 149/149 canonical peer-ID coverage is mechanically attested in `analysis/xhigh/85_citation_audit.md` (expected/observed counts by peer, zero missing/extra/duplicate); conflict topics, theorem index, EC logs/key integers, master phase table, obstruction map, freeze delta, nonclaims, NOT_IN_ZIP log are present.
- **PASS:** MDC tracks remain explicitly inequivalent; production TokenZero/tokenizer dominance is explicitly disclaimed; no forbidden brand merge found.
- **PASS WITH CAVEAT:** `RUN_ALL.sh` is executable and caller-CWD independent; from `/tmp` it exited 0 and printed certificate/BP1/structural pass output. Its structural acceptance coverage is insufficient, as above.
- **FAIL:** exact 14-heading wording; per-peer log rows; universal claim/ambition tags; full readable proofs; timestamp/model contract; authoritative final audit.
- **Residual risk:** I did not independently re-prove every theorem. Existing exact checkers validate headline finite arithmetic, not every prose step. The 149 inventory result relies on the package's included mechanical citation audit and its dedicated 00-60 audits.

## Validation evidence

- `python3 checkers/audit_package.py` -> exit 1, missing exact section-1 heading.
- `cd /tmp && /Users/aditya/Downloads/RADC_WAVE7_SOLPRO_RETURN/RUN_ALL.sh` -> exit 0, `WAVE7_RUN_ALL_PASS`.
- Control-character scan -> `Counter({9: 61, 12: 17, 8: 7, 11: 2})`.
- No package files were edited or staged.