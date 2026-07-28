# RADC Wave-6 Tier-5 (jobs B1–B8) report — DeepSeek 64-swarm lane

Tags: PI=peer-imported, DR=proof this lane, EC=exact computation this lane, BE=numerics, SB=speculative.
EC code+outputs in this directory: `b1_b2_antiopt_audit.{py,out}`, `b4_frontier.{c,out}`,
`b4_n5_subcube.{py,out}`, `b4_n5_cells.{c,out}`, `b5_density.{py,out}`, `b6_rhokill.{py,out}`.
Model lock: X~Unif({0,1}^n); Θ_n^↓ heavy-vertex weights w=(n+4,4,…,4), d=5n;
E(A)=Σ_i w_i min_a N_i^a(A); e(T)=ΣE(leaf)/(d2^n); ℓ(T)=L(T)/2^n; J_t(T)=2(1+ℓ)+t·e; F(t)=min_T J_t(T).

## B1 | DONE | Fable ANTI-OPT audit
- EC: e_n^anti recomputed independently n=2..30 in BOTH forms; identical in all 29 cases.
  Claimed values n=3..8 (1/4, 11/40, 121/400, 5/16, 145/448, 43/128) all match. [EC]
- Bridge identity 2S(n−1)/(2^n·5n) = [2(n−1)−B(n)]/(5n): PROVED algebraically [DR]:
  min(u,v)=(u+v−|u−v|)/2 with u+v=5n gives Fable form e=(5n−E|8K−5n|)/(10n), K~Bin(n−1,½);
  E|Z|=2E[Z⁺]−E[Z] and E[8K−5n]=−(n+4) give Kimi form. EC confirms to n=30.
- Enumeration minima 2S(n−1) n=3..8 = 30, 88, 242, 600, 1450, 3440 ✓ recomputed.
- Δ_m formula VERIFIED [EC]: recovered scale = 1/(2^n·5n) (Fable's Δ_m is in enumerator
  units); Δ_m·(2^n5n) = 2^{n−m−1}·C(m,k₀)·(4−r) matched for every m at n=5, 8, 12
  (all rows, incl. Δ=0 cases; c=a+4m, unique k₀ with |8k₀−c|<4, else 0).
- Mod-8 tie law VERIFIED n=3..24 [EC]: e(1,n−1)=e(1,n−2) iff 8|n; e(0,m)>e(1,m) strict ∀m≥1.
  n=8 spot: e(1,7)=e(1,8)=43/128 (hand-checked: gval=E max(12,4|Y₇|)=105/8).

## B2 | DONE | Kimi LPP-OPT audit
- Max-identity PROVED [DR]: condition on Y=y: (|y+w|+|y−w|)/2 = max(|y|,|w|) since
  |y+w|+|y−w| = (|y|+|w|)+||y|−|w||. Hence E|Y+wR|=E max(|Y|,|w|) ≥ E|Y|, equality iff
  |Y|≥|w| a.s. ⟹ g_n monotone under inclusion ⟹ argmax S=[n] (antipodal). EC: 2000 random
  discrete Y, exact. [EC]
- NEW PRECISION (kill of a hidden gap) [EC+DR]: the support functional
  e({p,q})=(5n−g_n(supp(p⊕q)))/(10n) is the codebook error under WEIGHTED-majority
  (optimal) assignment, NOT under Hamming-nearest partition: for supports containing the
  heavy coordinate plus 1..n−2 lights (and at antipodal support for even n), both
  deterministic Hamming-nearest tie rules are STRICTLY worse (n=8 antipodal: 93/256 vs
  43/128; q=254: 23/64 vs 43/128 despite zero Hamming ties). Weighted-majority partitions
  attain the formula in every case tested (all q, n=2..8, both tie rules minimized) [EC].
  Achievability at the boundary: n=8 enumerator 3440 attained by weighted-majority codes
  for BOTH the antipodal and the 7 drop-one-light supports [EC] ⟹ mod-8 tie law and
  codebook counts stand as achievable exact values: 2^{n−1} iff 8∤n, n·2^{n−1} iff 8|n;
  n=8: 1024 = 8·2⁷ ✓ (p=0-fixed optimal q count = 8).
- One-bit optimum over ALL bipartitions (not just prototype codes) = e_anti enumerator:
  n=5 verified over all 2^31 canonical bipartitions: E1min=242, exactly 16=2^{n−1} optimal
  bipartitions [EC, b4_frontier.c]; n≤4 all five classes [EC].

## B3 | DONE | BP1 equivalence, clean restatement [DR restatement of PI reduction]
Root-leaf line: e(Ω)=E(Ω)/(d2^n)=1/2 ⟹ J₀(t)=2+t/2. For any tree, J_t(T)≥J₀(t) on
[0,t] ⟺ t ≤ 2ℓ(T)/(1/2−e(T)). Hence the first breakpoint t₁ = inf_T 2ℓ(T)/(1/2−e(T)).
THREE-WAY EQUIVALENCE [PI proof, restated]:
(i) F(t)=2+t/2 exactly on [0,t₁] with t₁=2/s₁ (s₁=1/2−e^(1));
(ii) EVERY full prefix tree T: e(T) ≥ 1/2 − s₁·ℓ(T) (amortized tangent);
(iii) the scalar DP at t=t₁ admits the root leaf as an optimum.
(ii)⟹t₁≥2/s₁; the one-bit antipodal code (ℓ=1, e=e^(1)) attains 2/s₁, giving (ii)⟺(i);
(iii)⟺F(t₁)=2+t₁/2⟺(i). ANTI-OPT supplies the attaining tree, so the entire content
of BP1 is the universal inequality (ii). [PI: Fable W5-BP1 reduction; restatement DR]

## B4 | DONE (largest certified fragment) | BP1 general-n attempt
Headline new EC: the universal inequality (ii) verified EXACTLY over ALL full prefix
trees for all five n≤4 classes, by the slope-parametric subset DP
U_s(A)=min(S·E(A), K·|A|+min_splits U(B)+U(C)), K/S=s₁d: in every class U_{s₁}(Ω)=S·E(Ω)
(inequality holds for every tree) and U at slope s₁−1/(d2^n) drops strictly below
(slope tight) [EC, b4_frontier.out]. This upgrades Fable's breakpoint-level EC to a
direct proof of (ii) for n≤4 (same finite content, stronger form).
Routes:
(a) Potential induction: the local superharmonicity condition
    Ψ(A) ≥ γ(A;B,C) − s₁d|A| + Ψ(B)+Ψ(C) forces Ψ(pair) ≥ d(1−2s₁)>0 on antipodal pairs
    while Ψ(Ω)≤0 is needed — any valid potential must charge antipodal content before it
    forms; no closed form found. The U-DP IS the minimal such potential; its vanishing
    at Ω is exactly BP1. [DR analysis]
(b) Deep-leaf lemma [DR, general n]: if every leaf of T has depth ≥ 2, then
    (1/2−e(T))/ℓ(T) ≤ 1/4. Proof: G(T)≤E(Ω)=d2^{n−1}, L(T)≥2·2^n. ∎
    So only trees with depth-1 leaves can threaten s₁<1/4 (n≥4).
(c) Root-split reduction [DR]: for first split (B,C), writing slack(B,C)=s₁d2^n−γ(Ω;B,C)
    ≥0 (ANTI-OPT, all n [PI+EC]) and R(C)=max over subtrees of [G−s₁dL] (excess):
    ratio(T)≤s₁ ⟸ R(B)+R(C) ≤ slack(B,C). Sufficient condition for BP1(n); the binding
    case is optimal splits (slack 0) requiring excess-free optimal sides.
(d) n=5 certified fragment [EC]:
    - ANTI-OPT at n=5 over ALL 2^31 one-bit splits: E1min=242, slack law s₁d2^n=158. ✓
    - R(C)=0 (with strict margin ≥1/800 of slope) for ALL 32 sides of the 16 TRUE optimal
      one-bit bipartitions (E=242, slack 0; shifted half-cubes, e.g. 0x00017fff — NONE is a
      radius-≤2 ball: O4 supplementary audit n5_optclass.c, 2^31 enumeration) [n5_all16.c].
      ⟹ BP1(n=5) HOLDS for every tree rooted at an optimal one-bit split, arbitrary
      subtrees below. [EC-cert]
      (Earlier draft of this cell audit used Ball(p,≤2) sides, which have E=250/side-pair —
      slack 8, not optimal; O4 audit corrected the cells and re-certified on the true
      optimal class. The slack-8 ball-root fragment remains valid as a weaker statement.)
    - Subcube-tree family (all 3⁵ cells, exact Pareto frontier): max ratio 9/50 < 79/400;
      family t₁=100/9. [EC]
    - Depth-2 families (all 62 ball outers + 5 coordinate outers + 200 random outers,
      exact inner one-bit): max ratios 13/80=0.1625 and 1/7≈0.1429 < 79/400=0.1975. [EC]
    - Obstruction quantified: antipodal-closed cells carry excess ({x₀=x₁}: R=25/... 
      U=5600 vs base 6400; {x₀=x₁=x₂} and a built 8-pair cell likewise R>0) [EC].
    - One-bit slack histogram (full 2^31): E=242:16, 243:128, 244:416, 245:896, … [EC].
    Remaining open case at n=5: suboptimal root splits with R-heavy children
    (slack 1..~100 vs excess up to 25 for 16-cells: plausible but unproved).
Mission correction [EC]: t₁(5)=2/(1/2−121/400)=2/(79/400)=800/79≈10.1266; the brief's
"800/159≈5.03" miscomputes 1/2−121/400 as 159/400 (correct: 79/400).

## B5 | DONE | Obstruction family [EC]
Max split-gain mass-density = 1/2 in ALL five W5 classes AND Q5-down (sizes ≤4 exhaustive:
C(32,2)+C(32,3)+C(32,4)=41,416 sets), attained at EXACTLY the 2^{n−1} antipodal pairs and
nowhere else (set-identity check True for all six classes). Size-2 closed form [DR]:
E({x,y})=Σ_{i:x_i≠y_i} w_i ⟹ density = diff-weight/(2d) ≤ 1/2, equality iff antipodal.
Second-highest densities (density gap): Q3-down 11/30, Q3-uniform 1/3, Q4-down 2/5,
Q4-cap 2/5, Q4-uniform 3/8, Q5-down 21/50 (histogram in b5_density.out: 21/50 occurs 96×,
all size-3 sets with heavy+all-light-spanning difference patterns). Independent
re-verification of w5e_rest.py claims: CONFIRMED, extended to n=5.

## B6 | DONE | ρ_kill reconciliation [EC+DR]
Reconciliation [DR]: ρ_kill(n) = max(12, 4/e_anti(n)). Fable's 4/e_n^anti is the one-bit
COMPONENT (no one-bit policy kills for ρ≥4/e_anti, now exact-among-one-bit ∀n via
ANTI-OPT); Kimi's is the full law: for n≥8, e_anti>1/3 ⟹ 4/e_anti<12 and the binding
constraint is the zero-message L-witness L_b=1+ρ/4<4=L(candidate) iff ρ<12.
EC: crossings e_anti(7)=145/448<1/3 (435<448) ✓, e_anti(8)=43/128>1/3 (129>128) ✓,
margin exactly 1/384 ✓; e_anti>1/3 for ALL n=8..101, min margin 1/384 at n=8 ✓;
strictly increasing n=8..20 ✓ (exact fractions in b6_rhokill.out). Kill table
16, 160/11, 1600/121, 64/5, 1792/145 (n=3..7) ✓; =12 for n≥8; lim ρ_kill=12 NOT 10
(4/e_anti→10 is only the antipodal branch) ✓. L_b arithmetic: 1+11/4=15/4<4 ✓, =4 at ρ=12.

## B7 | DONE | One-bit vs full floor [EC+DR]
F(t)=min over tree lines {2(1+ℓ)+t·e}. Root leaf: 2+t/2 (E(Ω)=d2^{n−1}). One-bit
optimum line: 4+t·e^(1). Their crossing: 2+t/2=4+te^(1) ⟺ t=2/s₁=t₁. EC (b4_frontier.out,
exact rationals): in ALL five classes the second segment of F is EXACTLY the one-bit line
— collinear-triple checks: Q3-down 4+t/4 through (8,6),(23/2,55/8),(15,31/4) ✓; Q3-uniform
4+t/4 (claimed floor min(2+t/2,4+t/4,8) reproduced exactly) ✓; Q4-cap 4+3t/10 ✓;
Q4-down 4+11t/40 ✓; Q4-uniform 4+5t/16 ✓. All W4 breakpoint lists reproduced
(cap 10,16,160/7; down 80/9,16,80/3; uniform 32/3,16,20,22; Q3-down 8,15,135/8;
Q3-uniform 8,16) and F(40)=10 (n=4), =8 (n=3) ✓. t₁=2/(1/2−e^(1)) for each class:
10=2/(1/5), 80/9=2/(9/40), 32/3=2/(3/16), 8=2/(1/4), 8 ✓. So e^(1) enters F as the slope
of the second segment; BP1 = "no deeper tree's line undercuts the crossing of these two".
Class note [EC]: Q4-cap identified as weights (6,6,4,4) (θ=(3/10,3/10,1/5,1/5)) — the only
small-weight class reproducing e^(1)=3/10, t₁=10 and the full W4 cap breakpoint list.

## B8 | DONE | BP1 VERDICT
- Reduction (three-way equivalence): PROVED [PI, restated B3].
- n≤4: PROVED (EC-complete): universal amortized tangent verified over ALL trees in all
  five classes, slope tight [EC upgrades Fable's breakpoint EC to statement (ii) itself].
- Greedy route: permanently DEAD, all n [DR+EC]: max per-split density 1/2 > s₁(n)
  (antipodal pairs; infinite family), re-verified independently and extended to n=5.
- General n: OPEN, with the largest certified fragment at n=5 (B4d): optimal-root-split
  trees fully certified; sufficient-condition reduction R(B)+R(C)≤slack(B,C) isolated;
  boundary case (slack 0) proved; suboptimal-root case open.
- Conjectured target t₁(n)=4/(1−2e_anti(n))=2/s₁(n), exact table [EC]:
  n=2: 20/3; n=3: 8; n=4: 80/9; n=5: 800/79; n=6: 32/3; n=7: 896/79; n=8: 256/21;
  n=9: 4608/371; n=10: 12800/987; n=11: 1408/105; n=12: 2560/187; n=13: 532480/37653;
  n=14: 573440/39897; n=15: 819200/55913. (Trend: t₁→20 as n→∞ since e_anti→2/5.)

## New theorem candidates
- W6-DS-B1 (DR+EC): Bridge identity Fable⟺Kimi e_anti forms (proof §B1; EC n≤30).
- W6-DS-B2 (DR+EC): Support functional = weighted-majority codebook error; Hamming-nearest
  strictly suboptimal (new precision on LPP-OPT); counts 2^{n−1}/n·2^{n−1} achievable.
- W6-DS-B3 (DR): Clean three-way BP1 equivalence with J₀(t)=2+t/2 and t₁=inf_T 2ℓ/(1/2−e).
- W6-DS-B4a (DR): Deep-leaf lemma (ratio ≤1/4 if min leaf depth ≥2, general n).
- W6-DS-B4b (EC, proof-grade for n≤4): universal amortized tangent for all trees, five
  classes, exact + tight (slope-parametric subset DP certificates).
- W6-DS-B4c (DR): Root-split sufficient condition BP1(n) ⟸ R(B)+R(C)≤slack(B,C) ∀ splits.
- W6-DS-B4d (EC): n=5: optimal one-bit sides are excess-free (all 3 ball types, strict
  margin) ⟹ BP1(5) for all optimal-root trees; plus subcube/depth-2 family maxima below s₁.
- W6-DS-B5 (EC+DR): density-1/2 obstruction attained exactly at antipodal pairs (six
  classes); second-highest density table; n=5 histogram.
- W6-DS-B6 (DR+EC): ρ_kill(n)=max(12,4/e_anti(n)) reconciliation; monotonicity n=8..20;
  >1/3 to n=101 with min margin 1/384.
- W6-DS-B7 (EC): second segment of F = one-bit-optimal line in all five classes (exact);
  Q4-cap class pinned to weights (6,6,4,4).
- W6-DS-B8 (EC): corrected t₁ target table n=2..15 (incl. t₁(5)=800/79, killing the
  800/159 misprint).
