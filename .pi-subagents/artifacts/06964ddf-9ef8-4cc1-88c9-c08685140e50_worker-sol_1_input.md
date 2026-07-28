# Task for worker-sol

Wave 7 xhigh independent checker designer, READ-ONLY. Read analysis/xhigh/70_p1_general_n.md, 71_p2_mdc.md, 72_p3_bp1.md, analysis/EC_STATUS.md, PARENT_SYNTHESIS_NOTES.md, and all cited checker source/logs. Design a single portable stdlib Python checker for final W7 headline results. Return complete paste-ready typed source code, exact expected output, and assertion-to-theorem traceability. It must independently cover P1 endpoints/all-n certificates/nontrivial-tree barrier enough to support scope, P2 dual-ledger/separation/critical-dimension arithmetic, BP1 certified fragment/obstruction integers, and agency hooks clearly marked numeric if included. Do not import peer checkers or trust stored output. No edits, no subagents.

---
**Output:**
Write your findings to exactly this path: /Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/06964ddf-9ef8-4cc1-88c9-c08685140e50/analysis-xhigh/82_checker_design.md
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