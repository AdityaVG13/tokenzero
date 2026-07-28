# Implementation distill brief (for bead author model)

## What you are building toward

TokenZero already has RACC language (capsules, exact refs, recovery-adjusted cost).  
The multi-wave math campaign produced **formal** dominance results for recovery-aware policies vs no-recovery baselines in locked models.

Your job: turn the **freeze** into a **buildable** graph so coding agents implement:

1. Honest **ledger** (visible + expand + fail)  
2. **Opaque exact-ref** path (EDC-shaped)  
3. **Pilot A/B** that shows recovery-adjusted token wins  
4. Optional formal checker regression  

## What “return” looks like

Not “prove best compressor on earth.”  
**Return** = on a fixed pilot suite, exact-ref policy has lower recovery-adjusted tokens per successful task than paste-full baseline, with success/anchors not worse.

## Theory citations allowed in beads

| Cite as | Meaning in impl |
|---------|-----------------|
| Cont-2 m≤18 phase | Motivation for multi-demand / multi-expand cost; **not** a hard product limit of 18 tools |
| \(R_{\mathrm{ag}}=1-H_2(D)\) | Motivation: expand after demand is the right shape for sparse decisions |
| Opacity / alias→CAS | **Do implement**: visible id must not be raw content hash if claiming opaque |
| Corridor \(\rho^\star(s)\) | Config + measurement; advisory until pilot validates |
| Dual-track MDC | **Do not** pick one peer story in code; implement generic multi-expand accounting |

## Out of scope for v1 beads

- General-n Cont-2 proof  
- Merging Fable/Kimi MDC theories  
- Predictive cache v1 (P2+)  
- Rewriting stack in C  
