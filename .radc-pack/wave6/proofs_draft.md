# §6 Proofs draft — confirmed items (pre-O4)

Conventions per statement lock. Tags as given. EC files under `.radc-pack/wave6/ec/`.

## W6-DS-G1 (DR) Coverage-leaf transversality, general n

**Statement.** Fix n >= 2, N = 2^n. For every deterministic no-recovery prefix policy with r nonempty leaves and every demand law theta, P_T <= 1 - p_cov(theta,m)(1 - r/N), where p_cov(theta,m) = Pr_theta({S_1..S_m} = [n]). Same bound conditionally on encoder/decoder randomness.

**Proof.** Fix a transcript leaf A_j, a covering demand sequence s_{1:m} (every coordinate appears), and the decoder randomness. The answer map is then a function of the demand sequence only. On a covering sequence, the first answer to each coordinate pins one bit; at most one x in A_j agrees with all m answers (consistent answers pin a unique word; inconsistent repeats pin none). X uniform gives Pr[success | C_m, leaf A_j] <= 1/N; summing r leaves: Pr[success | C_m] <= r/N. On C_m^c, success <= 1. Total probability yields the claim. (Independently re-derived; equals W6-GROK-COV-LEAF-GEN.) Sharpening: for r=1 the bound is loose up to ~2x on C_m^c (exact no-message success is 2^{-k}, k = #distinct coordinates), so the no-message face must be treated exactly. ∎

## W6-DS-G2 (EC) Prefix-length spectra

C_N(1)=0, C_N(r) = min_{a+b=N, r1+r2=r} N + C_a(r1) + C_b(r2). Exact:
C_8 = (0,8,10,13,16,20,22,24); C_16 = (0,16,18,21,24,28,32,36,40,45,50,53,56,60,62,64);
C_32 = (0,32,34,37,40,44,48,52,56,61,66,71,76,81,86,91,96,102,108,114,120,124,128,132,136,141,146,149,152,156,158,160); C_64[1..12] = (0,64,66,69,72,76,80,84,88,93,98,103).
Structural certs: C_N(N) = N log2 N; nondecreasing; small-r matches N + U_{r-1}, U_k = k(a+2) - 2^{a+1}. Least r with C_N(r) >= 2N (the ell>=2 barrier case): r = 5 (N=8), 7 (N=16), 11 (N=32). (g2_spectra.py/.out.)

## W6-DS-G7 (DR+EC) n=3 full-prefix phase — HEADLINE

**Statement.** Let n=3, N=8, Theta_3^down = {theta_i >= 4/15}, gauge (rho,lambda)=(40,20), ledgers per statement lock (parity candidate (M,D,L) = (3m+2, 0, 4)). The parity policy dominates (weak in (M,D,L), strict in M) the complete randomized variable-length no-recovery prefix hull for every theta in Theta_3^down iff 1 <= m <= 16. m_crit(3) = 16.

**Proof.**
(a) *No-message face.* P_{0,m}(theta) = 2^{-3} sum_B theta(B)^m is symmetric convex, hence Schur-convex, maximized at the majorization-maximal vertex (7,4,4)/15. gamma_{0,m} = 39 - 2m - 40 P_{0,m} at the vertex is positive for m <= 16 (gamma_{0,16} = 845049722020265693/437893890380859375 ~= +1.929805) and negative at m=17 (gamma_{0,17} = -22519522704133297/437893890380859375 ~= -0.051427). Monotonicity on 10 <= m <= 16 via the integer certificates 20 m^m < (m+1)^{m+1} (same z^m(1-z) bound as Cont-2). At m=17 failure holds at EVERY theta: gamma is maximized where P is minimized (uniform law), gamma_{0,17}(unif) = -218455/14348907 < 0; for m >= 18 the universal P >= 1/8 gives gamma <= 34 - 2m <= -2. Hence a fixed-prototype no-message baseline (a legal r=1 prefix policy) strictly beats parity for all m >= 17 at every demand law.
(b) *Nontrivial-tree barrier.* Gamma_T = M_T - M_par = (m+1) ell - (2m+1) + rho(1 - P_T) >= (m+1) c_r/N - (2m+1) + rho p_m (1 - r/N) by G1 + G2, with p_m = 1 - (8/15)^m - 2(11/15)^m (union bound; miss-sum Schur-maximized at the vertex). For r >= 5: c_r >= 16 gives ell >= 2, so Gamma_T >= (m+1)*2 - (2m+1) = 1 for ALL m. For r in {2,3,4}: B_r(m) >= 1 on m in [4,28], [4,32], [4,40] respectively (exact scan; B_2(4) = 6998/1125; B_2(16) ~= 13.58); simultaneous onset m=4. Hence every r >= 2 tree has margin >= 1 for 4 <= m <= 16.
(c) *Small m.* m in {1,2,3}: the one-demand reduction M_T >= ((m+1)/2) F_{3,down}(80/(m+1)) with F_{3,down}(t) = 8 at t = 40, 80/3, 20 (t >= 135/8 = rho*_{3,down}; W4 breakpoints PI, independently DP-verified at all 10 denominator-15 grid laws) gives margin >= m+2 >= 3. F drops below 8 at t=16 (473/60), so the strip ends at m=3, contiguous with the barrier onset m=4.
(d) *L and D.* 2L_T >= 2 + 2 ell + 40 e_1 >= F_{3,down}(40) = 8 gives L_T >= 4 = L_par: gamma_L >= 0 with a tie attained by the zero-error identity (L_id = 1+n = 4); gamma_D = 0 (D_par = 0 <= e_T, tight). Dominance is weak with strict M-margin >= 1.
Supporting EC: full adaptive joint-m-demand subset-tree DP over all 10 denominator-15 grid laws x m=1..17 — DP margins equal the exact no-message margins; all laws positive at m <= 16, all beaten at m=17. (g7_n3_phase.py/.out.) ∎

## W6-DS-G8 (DR+EC) n=5 certified fragments + exact obstruction bracket

**Statement.** At n=5, Theta_5^down, (40,20): parity dominates the full hull for 1 <= m <= 3 (one-demand reduction with F_{5,down}(t)=12 for t >= 18, PI via Fable W5-Q5-SW cert rho_cert(5) <= 18; margin 3m+4) and for 11 <= m <= 18 (barrier B_r(m) >= 1 for all r >= 2 — simultaneous onset m=11 — plus Schur-max no-message vertex positive); dominance is impossible for m >= 19 (vertex gamma_{0,18} = +887975035189461090631639/582076609134674072265625 ~= +1.525529; gamma_{0,19} = -254541365995396231447867/582076609134674072265625 ~= -0.437299; crude m_fail(5,40)=19 tight here). OPEN strip 4 <= m <= 10: closing it needs the exact one-demand Q5-down class floor F_{5,down}(t) for t in (1600/121, 18) (at m=4, t=16), or a coverage floor stronger than the union bound. (g8_n5_partial.py/.out.) ∎ (fragment; strip OPEN/SB)

## W6-DS-G9 (DR+EC) rho-surface at n=4

M_0 - M_par = rho(1 - P_{0,m}) - 2m - 1. (a) Exact m_crit^nomsg(rho) (largest m with rho(1-P_m) > 2m+1 at the class-extreme law; identical at down and cap vertices for all ten gauges): rho: 20 24 28 32 36 40 48 56 64 80 -> m_crit: 8 10 12 14 16 18 21 25 29 36. Crossing brackets exact (e.g. rho=48 down: gamma(21) = 182793269680409/95367431640625 > 0, gamma(22) = -158706297446709/2384185791015625 < 0). The exact boundary sits below the crude m_fail = floor((rho(1-2^-n)-1)/2)+1 wherever occupancy beats the P >= 1/16 bound (rho=48: 21 < 22).
(b) Barrier survival: B_r(rho,m) >= 1 forall r in 2..6, m in [10,18] iff rho >= 72479248046875/3157132488062 ~= 22.957303 (binding at (r,m)=(2,18), machine-checked tight; r>=7 case rho-free).
(c) Full-phase survival: m=18 endpoint positive iff rho > 37/(1-P_18): down 141143798828125/3563296863977 ~= 39.610452, cap 74000000000000000000/1870074685943080277 ~= 39.570612. The entire Cont-2 phase m_crit=18 survives unchanged iff rho >= the down-class threshold; registered rho=40 sits ~0.39 above the cliff. (g9_rho_surface.py/.out.) ∎

## W6-DS-G10 (DR+EC) lambda decoupling

L_T = 1 + ell + c + lambda e_T >= 1 + ell + lambda e_1 >= G_Theta(lambda) := min_T (1+ell) + lambda e_1 = F_Theta(2 lambda)/2, with equality at m=1. Certified margin gamma_L >= F_Theta(2 lambda)/2 - 4. At the registered instance F_{4,down}(40) = 10 gives gamma_L = 1, lambda-free: lambda never binds at (40,20). lambda binds exactly when G_Theta(lambda) <= 4, i.e. lambda <= rho*_class/2 (Q4-down 80/11, Q4-uniform 32/5, Q3-down 135/16, Q3-uniform 8; strict gamma_L > 0 iff lambda > lambda*). Zero-error baselines are lambda-immune (L = 1+ell >= 1+n; strict vs L_par=4 for n >= 4, tie at n=3, fail at n=2). At n=3, G(lambda) <= 4 always (identity attains 4), so gamma_L = 0 is the best possible latency margin at any lambda: Cont-2's gamma_L = 1 is a Q4-or-higher phenomenon. EC: full Pareto-frontier DP at the Q3-down vertex recovers the 11-line lower envelope; lambda* = 135/16 exact, binding pair (L_ext,E) = (15,16). (g10_lambda.py/.out.) ∎

## W6-DS-M3 (EC; floors PI) Separating instance

At the single locked instance (n=4, Theta_4^down, theta = (2/5,1/5,1/5,1/5), (40,20), sequential 4-turn): Fable's pi_EDC^2 has p_c = 7/25, ledger (218/25, 0, 127/25), and 127/25 > 5 = L(identity seq): FAILS L-dominance. Kimi's PARITY-DUAL has ledger (8,0,4) and DOMINATES with margins (7,0,1) (given G2(40)=15, H2(40)=10 [PI]). Same polytope, gauge, demand law, baseline: opposite verdicts. Full 8-cell tabulation (2 theta x 2 timelines): at every cell Fable is M-better/L-worse than identity (incomparable); Kimi dominates identity; Kimi's ledger strictly dominates Fable's (8 < 218/25 <= M_fable, 4 < 127/25 <= L_fable). M-direction audit: min M_fable on Theta_4^down = 218/25 > 8 (max p_c = 7/25, grid certificate 455 pts den=60): Fable's M-advantage exists only against the identity baseline, never against anything Kimi doesn't already beat. (m3_separating_example.py/.out.) ∎

## W6-DS-M4 (DR+EC) No reduction Fable->Kimi

Any gauge-respecting reduction preserves the carried-token-weighted expand-count distribution (exactly the variable part of M under the shared accounting, m7). For pi_EDC^2, #exp in {1,2} with P(#exp=2) = 1 - p_c(theta) > 0 on every full-support theta (p_c < (sum theta)^2 = 1 when >= 2 entries positive); for PARITY-DUAL, #exp = 1 a.s. Distributions differ; no reduction exists on any full-support polytope. Mean-matching also fails: E[C_fable] = 3 - p_c equals 2 iff p_c = 1 (Dirac) and 1 iff p_c = 2 (impossible). ∎

## W6-DS-M5 (DR+EC) No reduction Kimi->Fable

PARITY-DUAL's ledgers have no representation in pi_EDC^2's family (9 - p_c, 0, 11/2 - (3/2) p_c): batch M=5 forces p_c = 4 > 1 (off-simplex); seq M=8 or L=4 forces p_c = 1 iff theta Dirac (p_c = 1 iff theta_i in {0,1}), excluded by full support. A batch-ified pi_EDC^2 (M = 6 - p_c) meets M=5 only at p_c = 1. ∎

## W6-DS-M6 (DR+EC) Interaction dichotomy

Fable's p_c = sum theta_i^2 = Pr[S1 = S2] (exact enumeration) is probabilistic (Renyi-2 collision mass, p_c = 2^{-H_2(theta)}); Kimi's r_A(Q) = dim pi_Q(span 1^4) = 1 for all 15 nonempty Q is algebraic (parity-fiber collapse x_j = parity(x) xor sum_{i != j} x_i over GF(2), verified on all 16 x). Fable's ledgers are affine nonconstant in theta (slopes -1, -3/2); Kimi's are theta-independent constants. A gauge-respecting reparameterization cannot identify a nonconstant affine law with a constant one: the structural root of non-reducibility. ∎

## W6-DS-M7 (EC lemma) Shared accounting

One carried-token rule generates both camps' ledgers: seq factors (capsule x3, R1 x2, R2 x1) + Convention A give Fable M = 3(1+h) + 2(1+q) + (1+q)(1-p_c) = 9 - p_c and Kimi seq M = 3(1+h) + 2(1+q) = 8 at (1,0,1/2,1/2); batch factors give Kimi batch M = 2(1+h) + (1+q) = 5. The seq M-gap decomposes as M_fable - M_kimi = 1 - p_c = the expected cost of Fable's conditional second expand, which PARITY-DUAL never pays. The difference is the candidate, not the accounting. ∎

## W6-DS-M10 (DR+EC) MDC resolution: PERMANENT SEPARATION

Certificates C1-C8: (C1) ledger mismatch at uniform n=4: (35/4, 0, 41/8) vs (5,0,4)/(8,0,4); (C2) M-coincidence only at p_c = 1 (Dirac); (C3) expand counts 7/4, 43/25 vs 1 a.s.; (C4) opposite n=4 L-verdicts 127/25 > 5 vs 4 <= 5; (C5) separating instance W6-DS-M3; (C6) theta-dependence dichotomy W6-DS-M6; (C7) expand-distribution invariant W6-DS-M4; (C8) p_c non-representation W6-DS-M5. C1-C4 independently re-verified (Grok W6); C5-C8 new this wave. Adjudication: scopes complementary, not nested (Kimi: n=4 positive with margins; Fable: n>=5 positive, n<=4 kill). The two kills overlap without contradiction: Fable's k-case quantifies over the EDC class (per-demand independent recovery); Kimi's NECESSITY kills all >=2-expand exact-ref policies; PARITY-DUAL escapes both (1-expand; parity recovery not per-demand-independent). pi_EDC^2 itself is killed by BOTH at n=4. Scope caveat: Fable's k-case must be read as quantifying over the EDC policy class (reading it as all exact-recovery policies would contradict W5-MDC-SEQ); freeze should state this scope explicitly. Cont-2's (3m+2,0,4) at m=1,2 equals Kimi's (5,0,4),(8,0,4): Cont-2 attaches to the parity/Kimi spine; Fable is the dedup-EDC island. ∎

## W6-DS-A1 (DR+EC) Agency RD converse audit + strengthening

The chain I(X;Z,R|S) = I(X;Z)+I(X;R|Z,S) >= I(X_S;Z,R|S) = 1 - H(X_S|Z,R,S) >= 1 - H2(P_e) >= 1 - H2(D) is valid under (H1) S independent of X; (H2) Z pre-demand (S independent of (X,Z)); (H3) deterministic decoder given randomness. Conditional Fano is applied correctly: given S=s, X_s -> (Z,R) -> A-hat is Markov, X_s binary, so the Fano RHS is exactly H2(P_{e,s}); average with Jensen. Strengthening: the bound uses only H(X_s)=1 and the theta-averaged distortion, so R_ag,theta(D) = 1-H2(D) holds for ALL theta including non-full-support (full support needed only for A2 strictness and the corridor). EC: 400 random schemes, margins all >= 0. ∎

## W6-DS-A4a (DR+EC) Sharp chord threshold — agency headline

Model: binary ISC; hybrid time-shares soft (rate f(D0)=1-H2(D0), distortion D0, fraction beta=D/D0) with exact expand (rate cost rho, distortion 0): R = beta f(D0) + (1-beta) rho.
**Statement.** Soft is rate-optimal at D iff rho >= rho*(D) := 1 + log2(1-D). For 0 <= rho < rho*(D), the optimal hybrid has D0* = 1 - 2^{rho-1} (unique root of nu(D0) = f'(D0) D0 - f(D0) + rho) and R_hyb(D;rho) = rho - D log2((1-D0*)/D0*).
**Proof.** Soft optimal iff rho >= Phi(D,D0) := (D0 f(D) - D f(D0))/(D0 - D) for all D0 in (D, 1/2]. dPhi/dD0 = -D[f(D) - f(D0) - f'(D0)(D - D0)]/(D0-D)^2 <= 0 by strict convexity of f (bracket >= 0), so sup_Phi is the limit D0 -> D+: Phi -> f(D) - D f'(D) = f(D) + D log2((1-D)/D) = 1 + log2(1-D) (telescoping via -H2(D) = D log2 D + (1-D) log2(1-D)). For rho below, the tangent condition nu(D0)=0 gives D0*, and the tangent line evaluates to the stated frontier. Grok's chord theorem is the boundary case rho=1 >= rho*(D) for all D (equality only at D=0). EC: sup Phi matches 1+log2(1-D) to 1.4e-7; closed form matches exhaustive grid to 1e-4 (tier4 a4); independent O3 check 16/16. ∎

## W6-DS-A4b (DR) Latency-charged expand collapses agency rate

rho = 0 < rho*(D) for all D < 1/2, so soft is never optimal; the optimum has D0* = 1/2 (empty capsule, coin-flip answer, expand w.p. 1-2D), giving rate R = 0 at every D. Hence R_ag = 1-H2(D) is NOT optimal among "soft + demand-conditioned expand" hybrids under pure latency charging; rate-optimality holds iff the expand is rate-charged >= 1+log2(1-D), in particular at its full information content rho = 1. ∎

## W6-DS-A4c (DR+EC) Ledger-consistent hybrid frontier (Model H)

Model H (m=1, carried-token accounting matching Cont-1 corridor margins and EDC endpoints): M_NR = 2+2R_NR(D), L_NR = 1+R_NR(D); M_RA = 2+f(D)+2h+q, L_RA = 1+f(D)+s; hybrid M_H = 2+2R_NR(D0)+alpha(1+2h+q), L_H = 1+R_NR(D0)+alpha(1+s), D = (1-alpha) D0. Coin-flip hybrid CF (D0=1/2, alpha=1-2D): exact margins DeltaM(CF-RA) = H2(D) - 2D(1+2h+q), DeltaL(CF-RA) = H2(D) - 2D(1+s). Lemma: H2(D)/D strictly decreases from +inf to 2 on (0,1/2] (concavity, H2(0)=0), so for coefficient c > 2 there is a UNIQUE crossover D-dagger with H2(D-dagger) = c D-dagger: RA dominates CF below, CF dominates above. At (1,0,1): D-dagger ~= 0.041587 (H2(D)=6D). Corridor-endpoint fragment (uniform theta, s < n-1, D* = H2^{-1}(1-s/(n-1))): pure soft at D* is (M,L)-dominated by CF iff 1 - s/(n-1) < 2 min(1+s, 1+2h+q) H2^{-1}(1-s/(n-1)); dominates CF iff the reverse with max; else split margins. EC frontier scan: hybrid dominating RA exists for D in {0.05..0.45}, not at D=0.02 (consistent with D-dagger). ∎

## W6-DS-A5b (DR+EC) Soft-decision TV RD = 1-H2(D)

Any channel X -> Q (Q = Bern(q)) induces the binary channel X -> Y, Y|Q ~ Bern(Q), with E[1{Y != X}] = E[d(X,Q)] <= D and I(X;Y) <= I(X;Q) (data processing); the minimum over binary channels is 1-H2(D) classically; endpoints achieve it. So R_TV(D) = 1-H2(D): randomized soft decisions buy nothing. Variants: observation-channel TV (distortion Delta 1{X-hat != X}): R(D) = 1-H2(D/Delta) on [0, Delta/2]; endpoint-free grid {1/4,1/2,3/4}: R(D) = 1-H2(2(D-1/4)) on [1/4,1/2], D < 1/4 infeasible. EC: BA matches 2.7e-6 / 2.5e-16 / 2e-5. (Also W6-DS-A5a: k-action 0-1 loss agency RD = 1-H2(D) for all k >= 2 — Fano runs on the binary X_S; extra actions useless by data processing.) ∎

## W6-DS-A6 (EC correction) AOT-6 interpolation

AOT-6's "dichotomy, no interpolation" holds only within the two canonical families: a mixed alias (w.p. beta content-hash, else opaque random, mode carried in A) gives I(X;A) = beta n exactly for every beta in [0,1] (EC: n=2, I = 1/2, 1, 3/2 at beta = 1/4, 1/2, 3/4); DLU-STRUCT's randomized-leak example I = (3/4) log2 3 already interpolates. AOT-1,2,3,5 and capacity K 2^r <= N endorsed (EC: n=2,K=2,N=4 independence exact; E[draws] bound verified with Fractions on 6 pairs). ∎
