# Task for worker-sol

Wave 7 xhigh P1 theorem construction, READ-ONLY. Source root: /Users/aditya/Downloads/RADC_WAVE7_THEORY_BUNDLE_EXTRACTED/wave7-attach-FLAT. Read Core/Cont-2 and all relevant peer proofs/checkers fully. Reconstruct from first principles strongest correct general-n sequential full-prefix phase at (40,20), Theta_down_n: n=2; no-message; every nontrivial tree; randomized hull; exact m_crit; all-n tail. Resolve 16/18/19. Publish full W7-SOL-* proof, exact inequalities, checker code/spec/key integers, PI/DR/EC/BE/SB + [S/F/M], dependencies/nonclaims. No edits, no subagents.

---
**Output:**
Write your findings to exactly this path: /Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/95aaed91-4b81-4b2b-b1a5-73c1458670c3/analysis-xhigh/70_p1_general_n.md
This path is authoritative for this run.
Ignore any other output filename or output path mentioned elsewhere, including output destinations in the base agent prompt, system prompt, or task instructions.

## Acceptance Contract
Acceptance level: attested
Completion is not accepted from prose alone. End with a structured acceptance report.

Criteria:
- criterion-1: Return concrete findings with file paths and severity when applicable

Required evidence: review-findings, residual-risks

Finish with a fenced JSON block tagged `acceptance-report` in this shape:
Use empty arrays when no items apply; array fields contain strings unless object entries are shown.
`criteriaSatisfied[].status` must be exactly one of: satisfied, not-satisfied, not-applicable.
`commandsRun[].result` must be exactly one of: passed, failed, not-run.
`manualNotes` and `notes` are optional strings; an empty string means no note and does not satisfy `manual-notes` evidence.
```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "specific proof"
    }
  ],
  "changedFiles": [
    "src/file.ts"
  ],
  "testsAddedOrUpdated": [
    "test/file.test.ts"
  ],
  "commandsRun": [
    {
      "command": "command",
      "result": "passed",
      "summary": "short result"
    }
  ],
  "validationOutput": [
    "validation output or concise summary"
  ],
  "residualRisks": [
    "none"
  ],
  "noStagedFiles": true,
  "diffSummary": "short description of the diff",
  "reviewFindings": [
    "blocker: file.ts:12 - issue found, or no blockers"
  ],
  "manualNotes": "anything else the parent should know"
}
```