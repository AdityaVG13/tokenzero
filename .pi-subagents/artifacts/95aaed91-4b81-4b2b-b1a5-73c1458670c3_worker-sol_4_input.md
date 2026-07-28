# Task for worker-sol

Wave 7 xhigh P5 master phase synthesis, READ-ONLY. Source root: /Users/aditya/Downloads/RADC_WAVE7_THEORY_BUNDLE_EXTRACTED/wave7-attach-FLAT. Read all phase claims/checkers across Core/W4/Cont-1/Cont-2/peers. Build precise master table with parallel no-message/full-prefix, batch/sequential, ISC, rho*(n,s,Theta), m_crit, lambda, MDC-FABLE/KIMI. Include only certified survivors, domains/quantifiers/open cells. Propose W7-SOL-* theorem only if proved, with derivation/EC/tags/nonclaims. No edits, no subagents.

---
**Output:**
Write your findings to exactly this path: /Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/95aaed91-4b81-4b2b-b1a5-73c1458670c3/analysis-xhigh/74_p5_master_phase.md
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