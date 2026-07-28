# §9 Obstruction map draft (pre-O4)

| Route | Progress | Missing input | Assessment |
|---|---|---|---|
| Naive Cont-2 lift n -> n' by substitution | DEAD (Grok barrier, endorsed; quantified by W6-DS-G11 inventory: spectra, coverage floors, saturation thresholds, phase boundary, latency margin ALL n-dependent) | none | Closed as impossible |
| Cont-2 full-prefix general n | n=3 CLOSED (W6-DS-G7, m_crit=16); n=5 fragments [1,3],[11,18] (W6-DS-G8) | exact one-demand Q5-down floor F_{5,down}(t) for t in (1600/121, 18) — equivalently sharpen the Q5 sandwich to an exact value; or exact vertex inclusion-exclusion p_cov | n=3 done; n=5 strip [4,10] OPEN with isolated missing input; n >= 6 untouched (spectrum+coverage+floor computation each) |
| Cont-2 general (rho,lambda) full surface | no-message face exact (W6-DS-G9a); barrier survival rho >= 72479248046875/3157132488062 (G9b); full phase survival rho >= 141143798828125/3563296863977 (G9c); lambda decoupled (G10) | full-prefix B_r re-proof below barrier threshold for small rho | Largely closed at n=4; other n inherit per-n recomputation |
| MDC merge Fable = Kimi | DEAD — permanent separation, 8 certificates (W6-DS-M10) | none | Closed; dual-track freeze law stands; Cont-2 attaches to parity spine |
| BP1 general-n by greedy/per-split induction | DEAD all n >= 2 (antipodal density 1/2 > s1; size-2 closed form this wave: density = diff-weight/(2d), equality iff antipodal) | none | Greedy route permanently closed |
| BP1 general-n by amortized potential | n<=4 PROVED EC-complete over ALL trees (W6-DS-B4b); n=5 optimal-root fragment PROVED (EC; O4-corrected cells: 16 true optimal bipartitions = shifted half-cubes, 32/32 sides excess-free) | global potential / extremal-family theorem; suboptimal-root case at n=5 (slack 1..~100 vs excess <=25 for 16-cells) | OPEN; conjecture t1(n) = 2/(1/2 - e_anti(n)); corrected table t1(5)=800/79 |
| Polytope-uniform two-demand floor | Still blocked (Fable Veronese obstruction: leaf law quadratic in theta, no finite vertex reduction) | new idea needed | Not reopened; retained |
| Agency RD lift to production decision-TV | formal binary/ISC fragments only (Tier-4 pending); marginal-TV RD shown degenerate (O3) | non-ISC side information models; production corridors (4 unproved mappings) | Production blocked as freeze non-goal |
| Q5 sandwich close (rho_cert(5) exact) | bracket (1600/121, 18] retained; Fable cert <= 18 PI | exact >=5-leaf killer analysis | OPEN; gates the n=5 strip |
| rho_kill limit | CLOSED: lim = 12, not 10 (Kimi law; mission-sketch "-> 10" refuted; zero-message L-witness binds n>=8) | none | Resolved in W5; re-verified this wave (B6) |
