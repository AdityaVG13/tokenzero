# Formal referee verdict: FAIL

## HIGH -- 149-inventory EC verdict is not checker-traceable

- **Package:** `RADC_WAVE7_PACKAGE.md:39-43` claims `[EC][M] 149/149 IDs`, zero missing/extra/duplicate, and retention of every verdict category.
- **Failure:** `checkers/w7_certificates.py` contains no inventory, ID, source-path, or verdict comparison. The shipped structural runner only searches for the literal token `149/149` before printing `peer_ids=149/149`; it does not count or reconcile rows. Corrected review 85 independently supports the 149 canonical-ID count, but not checker-to-package attestation of the delivered inventory and verdicts.
- **Fix:** add canonical expected ID sets by peer (5+5+28+10+16+46+10+19+10), parse the package inventory, normalize the documented Cont-2 alias, and assert exact ID, peer, path, and own-verdict equality. Do not label the package assertion EC until that check is durable.

## MEDIUM -- published ROOT37 K=157 margin is omitted from the final checker

- **Package:** `RADC_WAVE7_PACKAGE.md:2957-2960` publishes the stronger `K=157` side reruns and normalized margin. Its own minimum contract at `RADC_WAVE7_PACKAGE.md:2994-3000` requires K=158 checks on all 32 optimal sides plus representatives, and the same K=157 equalities whenever the margin is published.
- **Failure:** `checkers/w7_certificates.py:752` defaults the cell DP to K=158; `:868-878` invokes it only at that default and only for four representatives. K=157 is never asserted. A manual exact call at K=157 reproduced the four representative equalities, so this is an attestation blocker, not a discovered mathematical counterexample.
- **Fix:** assert K=157 and K=158 for all 32 optimal sides and the ball/heavy/light representatives, plus explicit symmetry invariance; otherwise delete the K=157 margin sentence.

## Scope verdicts

- **P1 proof, all-n quantifiers, randomized hull:** PASS. `RADC_WAVE7_PACKAGE.md:2290-2380` supplies every-leaf-count coverage, Schur extremizers, finite-strip reduction, strict realization-independent memory gaps, and seed conditioning.
- **P2 proof and category scope:** PASS. `RADC_WAVE7_PACKAGE.md:2403-2480` confines the theorem to two iid sequential demands and declared morphisms; `:2464-2478` explicitly makes non-reduction category-relative and rejects “permanent separation” as a theorem.
- **P3 theorem scope and ROOT37 mathematics:** PASS. `RADC_WAVE7_PACKAGE.md:2964-2985` limits uniform BP1 to n=1..12 and ROOT37 to 37 first splits with arbitrary subtrees, expressly excluding full Q5-down BP1.
- **Checker-to-claim traceability / 149 verdict attestation:** FAIL. These two gaps force the overall **FAIL** verdict.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Severity-ranked findings cite RADC_WAVE7_PACKAGE.md:39-43, 2957-2960, 2994-3000 and checker lines 752, 868-878."
    }
  ],
  "changedFiles": [],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "python3 checkers/w7_certificates.py",
      "result": "passed",
      "summary": "Printed PASS W7 FINAL HEADLINE CHECKER."
    },
    {
      "command": "./RUN_ALL.sh",
      "result": "passed",
      "summary": "Printed WAVE7_RUN_ALL_PASS; structural 149 result is token-presence only."
    },
    {
      "command": "python3 checkers/audit_package.py RADC_WAVE7_PACKAGE.md",
      "result": "failed",
      "summary": "Stale checker expected missing heading '## 1. Effort budget log'."
    },
    {
      "command": "manual exact calls to cell_dp_value(..., 157) for four ROOT37 representatives",
      "result": "passed",
      "summary": "Returned (3872,3872), (4000,4000), (4096,4096), (5376,5376)."
    }
  ],
  "validationOutput": [
    "P1 formal scope PASS; P2 formal/category scope PASS; P3 mathematical scope PASS.",
    "Overall FAIL because inventory and K=157 claims are not fully checker-traceable."
  ],
  "residualRisks": [
    "The 149 canonical-ID total is supported by corrected review 85 but is not mechanically revalidated against the delivered package rows and verdicts.",
    "ROOT37 symmetry and K=157 representative values were manually confirmed, but the shipped checker does not enforce the package's stated minimum assertion set."
  ],
  "noStagedFiles": true,
  "diffSummary": "Read-only repository audit; no repository files changed.",
  "reviewFindings": [
    "high: RADC_WAVE7_PACKAGE.md:39-43 - 149/149 inventory and verdict-retention EC claim has no exact checker comparison.",
    "medium: RADC_WAVE7_PACKAGE.md:2957-2960,2994-3000 - published ROOT37 K=157 margin is not asserted by checkers/w7_certificates.py."
  ],
  "manualNotes": "Overall FAIL is attestation/traceability-driven; no P1/P2/P3 mathematical counterexample was found."
}
```
