# RADC Wave-6 Tier-4 (DeepSeek jobs A1–A10) — consolidated report

EC code: `.radc-pack/wave6/ec/tier4/a{1..8}_*.py` with `.out` captures (all PASS).
Tags: PI | DR | EC | BE | SB. RD curves use math.log2 floats (EC-numeric); combinatorial
quantities use exact Fractions.

## New theorem candidates
- **W6-DS-A1** agency-RD converse audit: verified; strengthened to all theta (full support unnecessary).
- **W6-DS-A2** water-filling audit: verified (KKT logistic, uniqueness, envelope -mu, strict advantage, G decreasing).
- **W6-DS-A3** corridor audit: verified; addendum — dominance region is the CLOSED interval D <= D* (M strict at boundary).
- **W6-DS-A4a (headline, DR)** chord threshold: soft optimal iff rho_exp >= 1+log2(1-D);
  R_hyb(D;rho) = rho - D log2((1-D0*)/D0*), D0* = 1-2^{rho-1}, for 0 <= rho < rho*(D).
- **W6-DS-A4b (DR)** latency-charged expand collapses agency rate to 0 (coin-flip hybrid).
- **W6-DS-A4c (DR+EC)** Model-H ledger frontier: CF-vs-RA margins H2(D)-2D(1+2h+q), H2(D)-2D(1+s);
  unique crossover D-dagger; corridor-endpoint domination iff 1-s/(n-1) < 2 min(1+s,1+2h+q) H2^{-1}(1-s/(n-1)).
- **W6-DS-A5a (DR)** k-action 0-1 loss agency RD = 1-H2(D) for all k>=2.
- **W6-DS-A5b (DR)** soft-decision TV RD = 1-H2(D) (data-processing reduction); kills 'TV differs' suspicion;
  variants: observation-channel TV = 1-H2(D/Delta); endpoint-free grid = 1-H2(2(D-1/4)).
- **W6-DS-A6** opacity audit: AOT-1,2,3,5 + capacity endorsed; AOT-6 dichotomy corrected (mixed alias I = beta n).
- **W6-DS-A7** rho*(s) map verified; landmarks at s in {1/2, 2, 5/2, 3}; class thresholds are map evaluations.
- **W6-DS-A8** five class thresholds consistent at T=8; Q3d pair attribution flagged PI-gap.
- **W6-DS-A9** certified (M,D,L) region: s<n-1, D<=D*_theta (single demand) + m<=18 strip at D=0.
- **W6-DS-A10** obstruction map (4 one-liners, PI).
