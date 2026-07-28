# U-tier working draft — Master phase table (W6-DS-U1)

All cells verified this wave (O3 log `.radc-pack/wave6/ec/o3/o3_spotcheck.out`) or cited PI.

## A. Single-demand standard candidate (5,0,4) — linked slice lambda = rho/2, s = h+q+c

| Class | rho* (T=8) | Source | Verify |
|---|---|---|---|
| n=2 Theta_2^down | impossible (identity (6,0,3) beats candidate L=4) | W4-Qn-3PLUS | PI |
| Q3-uniform | 16 | Fable W5-Q3U | EC (peer re-run) |
| Q3-down | 135/8 | W4 floor seams 8,15,135/8 | EC O3 |
| Q4-uniform | 64/5 | W4 | EC O3 |
| Q4-cap | 40/3 | W4 | EC O3 |
| Q4-down | 160/11 | W4 | EC O3 |
| ISC uniform (n, s) | -2n log2(2^{1-(1+s)/n} - 1) -> 4(1+s) | W5-SOL-ISC-PHASE | EC O3 (12 digits) |
| ISC Theta_n^down, s=2 | root of 2+psi(rho(n+4)/5n)+(n-1)psi(4rho/5n)=8 | W5 | EC O3 |
| ISC asymptotic | 10 log2 x1, x1^3=4(x1+1): 12.527642810712 | W5 = Kimi rho_cert limit | EC O3 |

Ordering: 64/5 < 40/3 < 160/11 (uniform < cap < down); 16 < 135/8 (Q3 same shape).

## B. Corridor rho*(s), Q4 cap (latency binds)

rho*_cap(s) = 4+4s (s<=3/2) | 20s/3 (<=12/5) | 80(s-1)/7 (<=3) | +inf (s>3).
Verified piecewise == phi_F(4+2s) at all seams (O3). Registered s=2 -> 40/3.
Landmarks: s=5/2 -> 120/7 (advertised margins (4,0,1/2)); s=3 -> 160/7 (identity saturation, margins (5,0,1)).

## C. Two-demand (dual-track, never merged)

| (n, timeline) | MDC-FABLE verdict | MDC-KIMI verdict | Threshold |
|---|---|---|---|
| (4, batch, (40,20)) | fails (L>5 everywhere on Theta_4^down) | PARITY-DUAL (5,0,4) dominates, margins (5,0,1) | rho* batch: down 150/17, cap 1200/137, uniform 96/11 [PI, peer re-run PASS + O3 re-verified] |
| (4, seq, (40,20)) | CLASS KILL (L>=127/25>5 vertex, >=41/8 unif) | (8,0,4) dominates, margins (7,0,1) | G2>=8 at 125/17, H2>=8 at 150/17 (binding) [PI, peer re-run PASS] |
| (5, seq ZE) | dominance forall theta, margins (46/5,0,4/5) | out of scope (n=4 only) | n_crit=5 (Fable island) |
| (general n, ZE) | iff p_c >= (9-2n)/3; kill n<=4; vacuous n>=5 | n/a | p_c vertex: n=3: 9/25, n=4: 7/25, n=5: 29/125 |
| Q3 two-demand parity feasibility | — | — | Q3-down 400/41, Q3-uniform 48/5 [PI, Fable w5b peer re-run PASS + O3 re-verified] |

Cont-2 parity spine (3m+2,0,4) at m=1,2 = (5,0,4),(8,0,4) = Kimi batch/seq ledgers (Tier-3 M9, EC).

## D. Sequential multi-demand full-prefix phases (Cont-2 family, rho=40, lambda=20)

| n | class | phase | margins | status |
|---|---|---|---|---|
| 3 | Theta_3^down | dominance iff m <= 16; m_crit(3)=16 | gamma_M >= 1 (m<=9: 3,4,5,137/24,5,4,3,2,1); gamma_L = 0 (WEAK, identity tie); gamma_D = 0 | W6-DS-G7 PROVED DR+EC (m<=3 strip + L-floor via W4 Q3-down breakpoints PI; m=4..16 barrier + no-message face DR+EC unconditional) (NEW this wave) |
| 4 | Theta_4^down, Theta_4^cap | dominance iff m <= 18; m_crit(4)=18 | gamma_M >= 1 (10..17), m=18 sharp: 277615146191/762939453125 (down), 20074685943080277/5e16 (cap); gamma_L = 1; gamma_D = 0 | Cont-2, re-attested EC this wave |
| 5 | Theta_5^down | fragments: dominance m<=3 and 11<=m<=18; fails m>=19; OPEN 4..10 | vertex gamma_0,18 = +887975035189461090631639/582076609134674072265625; gamma_0,19 < 0 | W6-DS-G8 PROVED fragment + OPEN strip |
| n general | — | crude obstruction onset m_fail(n,rho) = floor((rho(1-2^-n)-1)/2)+1 | — | W6-GROK-CONT2-NOMSG-MFAIL (endorsed DR) |

rho-surface at n=4 (W6-DS-G9): full phase m_crit=18 survives iff rho >= 141143798828125/3563296863977 ~= 39.6105 (down) / ~= 39.5706 (cap); barrier-only survival rho >= 72479248046875/3157132488062 ~= 22.9573. Exact m_crit^nomsg(rho): 20->8, 24->10, 28->12, 32->14, 36->16, 40->18, 48->21, 56->25, 64->29, 80->36 (both class-extreme laws).

lambda-phase (W6-DS-G10): lambda never binds at (40,20); gamma_L >= F_Theta(2 lambda)/2 - 4; binds iff lambda <= rho*_class/2 (Q4-down 80/11, Q4-uniform 32/5, Q3-down 135/16, Q3-uniform 8); n=3: gamma_L = 0 ceiling at every lambda.

## E. One-bit / BP1 corridor (Theta_n^down heavy vertex)

| n | e_anti | rho_kill = 4/e_anti (3<=n<=7), 12 (n>=8) | rho_cert | t1 conj = 2/(1/2-e_anti) |
|---|---|---|---|---|
| 3 | 1/4 | 16 | +inf | 8 |
| 4 | 11/40 | 160/11 | ~20.761270 | 80/9 |
| 5 | 121/400 | 1600/121 | ~17.577411 (<=18 cert) | 800/79 |
| 6 | 5/16 | 64/5 | ~16.203387 | 32/3 |
| 7 | 145/448 | 1792/145 | ~15.425000 | 896/79 |
| 8 | 43/128 | 12 | ~14.921276 | 256/21 |

rho_kill -> 12 (NOT 10); corridor [rho_kill, rho_cert) shrinks to [12, 10 log2 x1) ~= [12, 12.527643).
BP1: reduction PROVED; n<=4 PROVED (EC-complete universal amortized tangent over ALL trees, five classes, W6-DS-B4b); n=5 optimal-root fragment PROVED (W6-DS-B4d, O4-corrected cells); greedy route DEAD all n (density 1/2 > s1); general-n amortized OPEN (root-split sufficient condition W6-DS-B4c isolated).

## F. Agency RD (ISC formal class)

R_ag,theta(D) = 1 - H2(D) (all full-support theta); R_NR,theta(D) = water-filling min sum (1-H2(d_i)) s.t. sum theta_i d_i <= D (logistic KKT d_i(mu) = 1/(1+2^{mu theta_i})); strict R_NR > R_ag (n>1, D<1/2).
Corridor endpoint: G_theta(D) = R_NR - (1-H2(D)) strictly decreasing from n-1 to 0; unique D* with G(D*) = s; latency binding. Uniform: D_iso = H2^{-1}(1 - s/(n-1)); n=4, s=2: H2^{-1}(1/3) ~= 0.06149047008.
Decision-TV: conditional TV collapses to 0-1 loss (TV(delta_a,P) = 1-P(a)); marginal-TV RD degenerate (R=0 forall d>0). W6-DS-A5a: k-action 0-1 loss = 1-H2(D) all k>=2; W6-DS-A5b: soft-decision TV RD = 1-H2(D) (data-processing reduction); variants 1-H2(D/Delta) and 1-H2(2(D-1/4)).
Hybrid (W6-DS-A4a/b/c): soft rate-optimal iff rho_exp >= 1+log2(1-D); frontier R_hyb = rho - D log2((1-D0*)/D0*), D0* = 1-2^{rho-1}; latency-charged expand collapses rate to 0; Model-H CF margins H2(D)-2D(1+2h+q), H2(D)-2D(1+s), crossover D-dagger ~= 0.041587 at (1,0,1).

## G. Alignment note (U2)

Cont-2's m_crit and Fable's n_crit are different axes of one table, not one scalar:
m_crit counts DEMANDS at fixed n (sequential parity spine); n_crit counts DIMENSIONS at fixed m=2 (dedup-EDC island).
The parity spine (Kimi MDC + Cont-2) has m-phases; the dedup-EDC island (Fable MDC) has n-phases. Master table records them as parallel rows (endorsing Grok's alignment remark, now with the n=3 row filled).

## H. U3 scalarization / RACE cone comparison

| Camp | Claim | Status this wave |
|---|---|---|
| Fable W5-RACE | K(rho) = finite half-space intersection over supported pairs; closed form at saturation | PI statement; Q4 (40,20) cone verified below |
| Kimi W5-RACE-1/2 | full positive orthant at collapsed gauge; C* open convex cone, delta*(rho), e-hat(rho); corner-delta* cone invalid above rho0 = 9 - sqrt(129)/3 ~= 5.2141 | PI; Grok tension note: check gauge hypotheses before orthant claim |
| W5-SOL-RACE-CONE (merge) | at Q4 (40,20): exact cone C* = {w >= 0 : w_M + w_L > 0} (pure-D weight ties zero-error identity); inf_b J_w = (A/2) F(2B/A), A=2w_M+w_L, B=rho w_M + w_D + lambda w_L | endorsed DR (W5 spine); consistent with both peers at the registered gauge |

Resolution: both peer shapes AGREE at the collapsed gauge (40,20) where the baseline front is the singleton (10,0,5): the uniqueness cone is the full positive orthant in (w_M, w_L). Fable's "finite half-space intersection" is the general-gauge form; Kimi's "full orthant" is the collapsed-gauge specialization. No tension remains once the gauge hypothesis is made explicit. (U3 verdict: unify under W5-SOL-RACE-CONE wording.)

## I. U4 DLU ledger uniqueness vs path uniqueness

Verified (O3): inverse map h = M - L, q = 2L - M - 3; ledger cone u/2 <= v <= u in relative coords (u,v) = (M-5, L-4); integer-token radii (r2, r_inf, r1) = (sqrt 2, 1, 2); M-spectrum gap 1 (M < 6 forces EDC ledger (5,4)).
Wording law (GroK merge policy + Kimi W5-DLU-STRUCT): "LEDGER unique, PATHS not" -- the minimizer policy set is an explicit infinite family ({Delta=0 handles} x {relabelings/OTP}); deterministic maximal-leak Delta=0 linear handle is unique (parity/complement up to invertible row ops).
Continuous isolation radius 0 (q = epsilon gives (5+epsilon, 4+epsilon)).

## J. U7 Core v1.1 freeze delta (proposal, pending O4)

ACCEPT (all PROVED/DR+EC this wave):
1. W6-DS-G7: n=3 full-prefix phase, m_crit(3)=16 (weak dominance, M-strict; gamma_L=0 ceiling).
2. W6-DS-G8: n=5 certified fragments [1,3] and [11,18], obstruction m>=19; OPEN strip [4,10] with missing input F_{5,down} on (1600/121, 18].
3. W6-DS-G9: exact rho-surface; full n=4 phase survives iff rho >= 141143798828125/3563296863977 (down) / 74000000000000000000/1870074685943080277 (cap); barrier survival rho >= 72479248046875/3157132488062.
4. W6-DS-G10: lambda decoupling; gamma_L >= F_Theta(2 lambda)/2 - 4; lambda* = rho*/2; n=3 gamma_L=0 ceiling.
5. W6-DS-G2: spectra C_32 (full), C_64[1..12]; ell>=2 thresholds r=5/7/11 (N=8/16/32).
6. W6-DS-M10: MDC permanent separation, certificates C1-C8; dual-track freeze law confirmed; Cont-2 attaches to parity/Kimi spine.
7. W6-DS-M3: separating instance certificate (single locked instance, opposite verdicts).
8. W6-DS-M4/M5/M6: non-reduction structural theorems (expand-count invariant; p_c non-representation; theta-dependence dichotomy).
9. W6-DS-G4/5/6 + E1..E10: substrate re-attestation (Cont-2, Cont-1, Fable, Kimi, Grok checkers all PASS; sha256 manifests 6/6, 7/7, 16/16).
10. Master phase table (this file, sections A-G).
11. W6-DS-A4a/A4b/A4c (agency hybrid family: sharp chord threshold + frontier, latency-collapse, Model-H crossover); W6-DS-A5a/A5b (decision-TV = 1-H2(D)); W6-DS-A6 (AOT-6 interpolation correction).
12. W6-DS-B4b (universal amortized tangent, n<=4, five classes, EC proof-grade); W6-DS-B4d (n=5 optimal-root fragment, O4-corrected cells); W6-DS-B4a/B4c (deep-leaf lemma, root-split reduction); W6-DS-B1/B2/B5/B6/B7/B8 (audits + corrections: bridge identity, weighted-majority precision, density obstruction, rho_kill reconciliation, second-segment identity, corrected t1 table).

DO NOT FREEZE: full-prefix n>=5 phase; BP1 general-n; merged MDC; production dominance; Kimi/Fable DP floors as re-proved (they remain PI, re-run as peer checkers only); rho_cert(5) exact value (sandwich (1600/121, 18] stands).

## K. U8 non-claims (standing, pending O4 sign-off)

1. No production TokenZero / real-tokenizer dominance; four production mappings unproved.
2. No MDC merge by label; dual-track IDs mandatory.
3. No full-prefix Cont-2 phase for n >= 5 (n=5 strip [4,10] OPEN; n >= 6 untouched).
4. No BP1 general-n close (unless Tier-5 B4 delivers; reduction + n<=4 + greedy-kill only otherwise).
5. No agency RD claims beyond formal ISC/binary/finite-action models.
6. No claim that peer DP floors (Kimi F2/G2/H2, Fable W5-MDC-4/5 hull floors, Q5 cert) were re-derived; they are PI inputs with peer checkers re-run PASS.
7. No "99.9% compression"-style marketing; no real-agent policy claims.
8. v1/v2 latency conventions not mixed (identical numerically at registration only).
