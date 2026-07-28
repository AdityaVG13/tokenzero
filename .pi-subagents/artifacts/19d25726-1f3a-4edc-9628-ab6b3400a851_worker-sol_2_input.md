# Task for worker-sol

Standard-tier GPT-5.6 Sol medium READ-ONLY agency/phase referee. Do not use subagents. Audit RADC_WAVE7_PACKAGE.md and w7_certificates.py against analysis/medium/87_agency_review.md and analysis/xhigh/86_phase_review.md. Ensure no complete-frontier overclaim, kappa_exp=1, D_H/D* interval and ledger signs are scoped, adaptive DTV/DeepSeek malformed claims rejected, exact-slice phase table uses unlinked lambda correctly, tree surface marked sufficient, Qwen not full-strip proof, MDC category scope. Return exact line findings and PASS/FAIL. No edits.

---
**Output:**
Write your findings to exactly this path: /Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/19d25726-1f3a-4edc-9628-ab6b3400a851/analysis-medium/92_agency_phase_review.md
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