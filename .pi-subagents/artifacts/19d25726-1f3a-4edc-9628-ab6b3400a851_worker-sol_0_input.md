# Task for worker-sol

Standard-tier GPT-5.6 Sol medium READ-ONLY formal referee. Do not use subagents. Audit RADC_WAVE7_PACKAGE.md and checkers/w7_certificates.py against corrected reviews 80,83,84,85 and exact source/logs. Focus P1/P2/P3 proof correctness, all-n quantifiers, randomized hull, P2 category scope, 149 inventory verdicts, ROOT37, and checker-to-claim traceability. Return only severity-ranked blockers/fixes with exact package lines and PASS/FAIL verdict. No edits.

---
**Output:**
Write your findings to exactly this path: /Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/19d25726-1f3a-4edc-9628-ab6b3400a851/analysis-medium/90_formal_review.md
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