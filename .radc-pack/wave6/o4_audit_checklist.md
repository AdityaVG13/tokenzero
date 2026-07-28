# O4 adversarial audit checklist (final pass before package sign-off)

## Identity / scope
- [ ] Single package document, sections 0-12 per spec
- [ ] Dual-track MDC names everywhere (no merged MDC-* label)
- [ ] No production TokenZero claims; no 99.9% marketing; no beads
- [ ] Tags PI|DR|EC|BE|SB on every theorem row
- [ ] Roster >= 64 rows with status per agent id; fan-out count stated

## Substrate re-check (already PASS, record)
- [x] Cont-2 sha256 6/6; Python 8/8; C++ __int128 PASS; grid 45/45
- [x] Cont-1 checker PASS
- [x] Fable w5a-f PASS (flag F1 = mod-8 tie law, value always optimal); manifest 7/7
- [x] Kimi drive 72/0; C++ floors F2=10, G2=15, H2=10; manifest 16/16
- [x] Grok W6 3 checkers 22/0 byte-identical

## Headline theorem audits (two-lane: analytic + DP)
- [x] W6-DS-G7 n=3: O3 independent derivation == Tier-2 full DP, bit-identical fractions (m=15/16/17); m=1..9 margins >= 1; B_r(16) > 1 r=2,3,4; r>=5 ell>=2; vertex kills m=17; universal kills m>=18; gamma_L=0 weak-dominance wording correct (M strict)
- [x] W6-DS-G8 n=5: vertex margins m=18/19/20 independently recomputed == Tier-2; OPEN strip [4,10] stated with missing input
- [x] W6-DS-G9: rho* = 37/(1-P_18) fractions recomputed == Tier-2 (down/cap); slack at rho=40 positive; crude-vs-exact m_fail distinction stated
- [x] W6-DS-G10: lambda never binds; lambda* = rho*/2; n=3 gamma_L=0 ceiling (identity attains 4)
- [x] W6-DS-M3..M10: 67 PASS lines spot-checked; 8 certificates; scope caveat (Fable k-case = EDC class) recorded
- [x] W6-DS-A4a: O3 independent 16/16 PASS (analytic tangent + grid)
- [ ] W6-DS-B* (Tier-5): check B4 deliverable against conjecture table t1(5)=800/79 (O3 correction); greedy-kill all-n; verdict wording (PROVED fragment / OPEN / obstruction)

## Flags resolved during wave (record in audit section)
- [x] F1: Fable w5c "***FAIL***" rows = mod-8 tie law, not errors
- [x] A3: tier4 corridor script AssertionError = harness bug (s-mismatch); Cont-1 corridor stands (identity gamma_M == 2G+f-2h-q exact)
- [x] Q3-down floor pieces typo in S2/S6 extraction ("2+2t","4+4t" -> correct "2+t/2","4+t/4"); landmarks unaffected
- [x] t1(5) = 800/79 (not 800/159): s1 = 79/400
- [x] crude vs exact m_fail(rho) divergence (rho=48: 23 vs 22) — both reported with distinct names
- [x] AOT-6 dichotomy overclaim killed (mixed alias I = beta n) — Tier-4 A6
- [x] Q3d 135/8 pair attribution PI-gap (A8) — record as open PI-gap, not contradiction

## Overclaim sweep
- [ ] Every "PROVED" has either DR proof in package or EC file reference; every peer floor used is tagged PI
- [ ] n=3 dominance stated as WEAK (gamma_L=0, gamma_D=0, M strict) — same convention as Cont-2
- [ ] m_crit(3)=16 not confused with no-message-only results; barrier + one-demand strip + DP all cited
- [ ] Non-claims section complete (production, merge, n>=5, BP1 general-n, real-agent RD)
- [ ] v1/v2 latency conventions not mixed
- [ ] Kimi/Fable DP floors: PI with checker re-runs, NOT "re-proved"
