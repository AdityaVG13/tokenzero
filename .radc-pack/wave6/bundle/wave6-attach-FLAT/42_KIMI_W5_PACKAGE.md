# RADC Wave-5 Package (Invention / Prove-First)

**Model/run:** Kimi K3 swarm (Wave-5 orchestrator + 6 parallel invention/computation agents +
independent orchestrator exact-arithmetic cross-checks). **Date:** 2026-07-27.
**Mode:** prove-first invention over the locked Wave-4 Sol Pro base (canonical survivors treated
as proved axioms; nothing from Wave 4 is re-litigated). Wave-3 Kimi/Claude are historical only.
**Scope:** academic formal research on recovery-aware multi-objective context compression
(RACC/RADC). No security/exploit content. No production-dominance claims without corridor
inequalities — none are made anywhere below.

**Proof-status tags:** PI published input | DR derived result | EC exact computation |
BE bounded experiment | SB speculative bridge.

**Companion proof/EC appendix files (full-length versions, same run):**
`W5_DLU.md`, `W5_LPP.md`, `W5_AOT.md`, `W5_MDC.md`, `W5_SMC_RACE.md`,
`W5_COMP_CERTIFICATES.md` (harness + raw checker logs + code).

---

## 0. Executive verdict

1. **Six new affirmative theorem families proved (campaign bar: ≥3).** Every concrete splice
   target in the mission was hit, and one target (LPP) returned strictly more than asked —
   an *all-n* optimality theorem where a conjecture was scoped:

   | Family | Headline affirmative result | Splice of |
   |--------|-----------------------------|-----------|
   | **W5-DLU** | Inside the exact-recovery zero-determination class, the minimal ledger point `(3+2h+q, 0, 2+h+q+c)` is **unique** and the class Pareto front is a singleton; C5.1 forced accounting at (40,20) proved; extremizer set characterized explicitly (forced 1-bit sufficient recovery × Δ=0 handles), incl. a new converse: the parity/complement family is the *unique* max-leak deterministic handle | W4-AFF-Q4-40, W4-DA-RATE, W4-DETERMINATION-FLOOR, W4-EXTREMAL-NONUNIQUE |
   | **W5-LPP** | Closed-form phase algebra: `e_anti(n) = [2(n−1) − B(n)]/(5n)` with exponential slack bound; **antipodal one-bit code optimal for ALL n** (Rademacher max-identity), optimal codebooks `= 2^{n−1}` iff `8∤n`, `n·2^{n−1}` iff `8|n`; exact kill law `ρ_kill(n) = 4/e_anti (3≤n≤7), 12 (n≥8)`; certified `ρ_cert(n)` brackets n=4..12 by integer comparisons; `ρ⋆(n,Θ,h,q,c) = F⁻¹(T) = max_j (T−2−2ℓ_j)/e_j` | W4-FLOOR-Q4, W4-PHASE-MASTER, W4-Q5-ANTIPODAL, W4-Qn-SEPARABLE, W4-PHASE-Q4-H |
   | **W5-AOT** | Two-level alias→CAS opacity algebra: `I(X;A)=0` exactly (joint K entries); post-expand leak `= H(X_S) = 1` bit exactly, batch leak `= dim π_Q(ker A)`; rate 2 optimal among `I=0` handles; converse `H(Π) ≥ n` with the two-level construction the canonical minimal factorization; ε-birthday bound `I ≤ K(K−1)H(X)/(2N)` | W4-OPAQUE-CAS-ALIAS, W4-DA-RATE, W4-DIRECT-HASH-KILL, W4-LINEAR-ALIAS-RANK, W4-ALIAS-CAPACITY |
   | **W5-MDC** | Two-demand cycle dominated by PARITY-DUAL with the **same** `(5,0,4)` ledger as single demand (**the second demand is free**), margins `(5,0,1)` batch / `(7,0,1)` seq at (40,20); exact two-demand floors computed (`F2_batch(40)=10`, `G2(40)=15`); non-reduction proved: residual rank 1 is a **necessity certificate**, any ≥2-expand policy pays `L ≥ 11/2 > 5` for all ρ | W4-FLOOR/DP, W4-PHASE-MASTER, W4-DA-RATE, W4-LINEAR-ALIAS-RANK, W4-BATCH-PARITY-KILL |
   | **W5-SMC** | Master parametric corridor: margins as explicit affine functions of measured `(h_τ,q_τ,c_τ,Δ,p_miss)`; threshold map `ρ⋆(h,q,c) = 4+4s | 20s/3 | 80(s−1)/7 | +∞`, `s = h+q+c` (exactly extends W4-PHASE-Q4-H); C5.5 constant corrected with certificate | W4-CORRIDOR-Q4, W4-PROD-CORRIDOR-DELTA, W4-PHASE-MASTER, W4-PHASE-Q4-H |
   | **W5-RACE** | At collapsed gauges π_EDC is the **unique** `J_w`-minimizer for the **full open positive orthant** of weights (weighted-sum scalarizations complete); general-gauge exact equilibrium cone `C*` with closed-form defeat distortions `δ*(ρ)`, `ê(ρ)` and maximal repaired inner cones | W4-GEO-Q4, W4-PHASE-MASTER, W4-FLOOR-Q4-CAP |

2. **Closed forms delivered (campaign bar: ≥1 uniqueness/closed-form/recurrence).**
   Uniqueness: W5-DLU-1 (ledger). Closed forms: `e_anti(n)`; `ρ⋆ = max_j (T−2−2ℓ_j)/e_j`;
   `ρ_cert(∞,1) = 10·log₂x₁`, `x₁³ = 4(x₁+1)`; two-demand floors `F2_batch`/`G2` (4 exact
   piecewise families); `δ*(ρ)`, `ê(ρ)` cone distortions; `ρ⋆(h,q,c)` in `s = h+q+c`.

3. **Phase picture after Wave 5.** On `Θ_n↓` with the standard candidate: theorem false below
   `ρ_kill(n)` (exact, with `ρ_kill = 12` for all `n ≥ 8` — **not** ↓10; the zero-message
   L-witness floors it), certified above `ρ_cert(n)` (integer-comparison brackets), exact closures
   at n=3 (`135/8`), n=4 (`160/11`, where the kill is **tight**: the floor's binding ℓ=1 code IS
   the antipodal code). The open corridor shrinks to `[12, 10·log₂x₁) ≈ [12, 12.527643)` —
   non-vacuous since `x₁ > 2` gives `10·log₂x₁ > 12`.

4. **Targeted disproofs (all instrumental, each with an exact replacement):**
   (i) starter C5.5's L-inequality has a spurious `+1` (would over-certify `γ_L ≤ 2`; true sharp
   value 1 — identity-baseline certificate); corrected set proved exact.
   (ii) The corner-`δ*` scalarization cone is invalid above `ρ₀ = 9−√129/3` (exact weight
   counterexample `w·(b−a) = −201/500`); repaired two-sided cones proved valid at every gauge.
   (iii) In the two-demand cycle the OPAQUE-DUAL policy fails latency dominance for **all** ρ
   (`L = 11/2 > 5`); the parity (residual-rank-1) witness is the unique dominating dual shape.
   (iv) Literal EDC uniqueness stays dead (W4), but is now mapped: the minimizer set is an
   explicit infinite family; ledger uniqueness is the maximal true statement.
   (v) `ρ_kill(n) ↓ 10` is false (limit is 12); `ρ_cert(3) = +∞` (Ψ vacuous at n=3).

5. **Certificate strengthening (74/74 checker assertions PASS, zero Wave-4 discrepancies).**
   All W4 supported pairs/floors/breakpoints reproduced exactly; `21,457,825` split comparisons
   reproduced and explained (`= Σ_k C(16,k)(2^{k−1}−1)`); envelope completeness at all 13
   intersections; Q5 496-codebook enumeration re-run (min 242); antipodal optimality enumeration
   extended from W4's n=5 to n=3..8 (all optimal); new n=8 tie phenomenon classified; all new
   Wave-5 numbers cross-validated by ≥2 independent implementations (agent C++ + orchestrator
   C++ + pure-Python Fraction DPs): **zero three-way discrepancies**.

6. **What is NOT claimed.** No exact general-n no-recovery frontier for `n ≥ 5` inside the
   corridor (explicitly open, finite-block); no production TokenZero/real-tokenizer dominance
   (corridor theorems are formal and correctly gated); no monotonicity proof of `e_anti` for
   all n (EC to n=101); no Lean artifact (algebraic cores are Lean-ready specifications only).

---

## 1. Effort budget log (prove % / strengthen % / disprove %)

| Budget line | Share | Content |
|-------------|-------|---------|
| Affirmative invention + proof of new positive theorems | **≈ 66%** | W5-DLU (class, floor, uniqueness, extremizers, radius), W5-LPP (closed forms, all-n optimality, kill/cert algebra, product theorem), W5-AOT (opacity algebra 6 theorems), W5-MDC (floors, rate, batch/seq dominance, interaction, necessity), W5-SMC (master corridor, threshold map, table), W5-RACE (orthant cone, exact cone + repair) |
| Certificate strengthening of Wave-4 survivors | **≈ 24%** | 74-assertion exact harness: full W4 reproduction (floors, pairs, 21,457,825 count, envelope completeness, Q5 enumeration, Ψ check) + new exact computations (two-demand frontiers/floors at both vertices, antipodal enumeration n=3..8, e_anti table n=3..20, ρ_cert brackets, n=8 tie classification) + orchestrator's independent second implementation of every load-bearing number |
| Targeted disproof (only clearing obstacles to positive claims) | **≈ 10%** | C5.5 constant correction (→ SMC-2 exact replacement); corner-cone obstruction (→ RACE-2 repaired cones); OPAQUE-DUAL latency obstruction (→ MDC-NECESSITY); literal-uniqueness boundary mapping (→ DLU-STRUCT maximal replacement); `ρ_kill ↓ 10` correction (→ LPP exact law); mission EDC-twice arithmetic slip (→ stronger strawman, conclusions a fortiori) |

Prove-first ratio **≈ 66/24/10** — satisfies the hard bias (≥60% / ≥20% / ≤20%).
No REFUTED-only rows exist in this package: every obstruction carries an exact true replacement.

---

## 2. Statement lock (inherit Wave-4; exact corrections with certificates)

**Inherited, unchanged:** the full Wave-4 survivor set (W4-DP-Q4, FLOOR-Q4-CAP/DOWN/UNIFORM,
FLOOR-Q3-DOWN, PHASE-MASTER, PHASE-Q4-H, AFF-Q4-40/EXPANDED, ZE-GORDIAN, Qn-FANO/SEPARABLE/3PLUS,
Q5-ANTIPODAL, DETERMINATION-FLOOR, EXTREMAL-NONUNIQUE/KILL, LINEAR-ALIAS-RANK, BATCH-PARITY-KILL,
ALIAS-CAPACITY, PROD-CORRIDOR-DELTA, NO-PENALTY-ROBUST, NEG-NR-n, DA-RATE, DIRECT-HASH-KILL,
OPAQUE-CAS-ALIAS, GEO-Q4, PEER-REPAIRS). No exact error was found in any of them; the
74-assertion reproduction returned zero discrepancies (§7).

**Corrections to Wave-5 *mission documents* (not to Wave-4 math), each with a certificate:**

1. **C5.5's L-inequality constant is wrong by +1.** Starter conjecture C5.5 (file `01`) proposes
   `2+h+q+c+γ_L ≤ 1 + F4(ρ)/2`. The exact RHS is `F(2λ)/2` (= `F(ρ)/2` on the linked slice) —
   the turn-1 base cost `1` is already inside `F(2λ)/2`. **Certificate:** at (40,20) cap with
   ideal `(h,q,c) = (1,0,1)`, C5.5 certifies `γ_L ≤ 2`, but the identity baseline sits at `L = 5`
   with candidate `L = 4`, so the sharp uniform margin is exactly `1`; C5.5 as written would
   assert `L_b ≥ 6` for all baselines — false at identity. Corrected set proved exact and sharp
   (W5-SMC-2, §5.5).
2. **`ρ_kill(n) ↓ 10` (mission sketch/splice-brief suggestion) is false.** The zero-message
   L-witness (`L_b = 1 + ρ/4 < 4` for `ρ < 12`) floors the kill at 12 for every n; exact law:
   `ρ_kill(n) = 4/e_anti(n)` for `3 ≤ n ≤ 7` and `= 12` for `n ≥ 8` (W5-LPP-KILL). Only the
   antipodal branch `4/e_anti(n)` tends to 10. Largest true replacement corridor:
   `[12, 10·log₂x₁)`.
3. **Mission EDC-TWICE sequential ledger slip.** The sketch's `M = 3(1+2h)+2(1+q)+(1+q) = 7+6h+3q = 13`
   misexpands: the correct value is `6+6h+3q = 12` at `(h,q) = (1,0)`. All interaction conclusions
   are used against the *stronger* (cheaper) strawman and hold a fortiori (W5-MDC §5 note).
4. **Structural note (not an error):** `ρ_cert(3) = +∞`. The separable certificate
   `Ψ↓_{3,t} = 8 − log₂[(1+2^{−7t/30})²(1+2^{−2t/15})⁴] < 8` for every finite `t`; the n=3 phase
   is closed by the exact DP floor (`135/8`), never by Ψ (W5-LPP-CERT(0)).

**Locked Wave-5 additions (new locked objects):** see §3. Demand classes, timelines (3-turn
batch ×2/×1/×0; 4-turn sequential ×3/×2/×1/×0), baselines, gauges, and the linked slice
`λ = ρ/2` are as in the substrate card. The two-demand task locks `D = 0` ⟺ both answers exact,
product demand `(S1,S2) ~ θ⊗θ`, leaf task error `e2(p,x) = 1 − (Σ_s θ_s·1[p_s=x_s])²`.
Latency convention (locked Convention A, the unique decomposition consistent with W4's
`L_EDC = 2+h+q+c`): `L = 1 + h + c0 + Σ_{expands}(1+q+c1)`.

---

## 3. New formal objects (names, definitions, splices used)

1. **Exact-recovery zero-determination class** `Π_ER(Θ;h,q,c)` — exact-recovery
   contract-respecting exact-ref policies, envelope cost `h ≥ 1`, costs `(q,c0,c1)`, and
   `Δ_Θ(H) = 0` (θ-free per-handle property: `Δ_Θ = 0 ⇔ δ(H) = 0` on compact full-support Θ).
   Splice: W4-DA-RATE ⋉ W4-DETERMINATION-FLOOR ⋉ W4-OPAQUE-CAS-ALIAS. *(DLU)*
2. **Ledger slack decomposition** `M − m* = 2ℓ + (r−1) + q(e−1)`,
   `L − ℓ* = ℓ + (r−1) + (q+c1)(e−1)` with floors `r,e ≥ 1−Δ` — the uniqueness engine. *(DLU)*
3. **Antipodal error closed form** `e_anti(n) = [2(n−1) − B(n)]/(5n)`,
   `B(n) = E[(8K−5n)⁺]`, `K ~ Bin(n−1,1/2)`; and the **Rademacher support functional**
   `g_n(S) = E|Σ_{i∈S} w_i R_i|` with `e({p,q}) = (5n − g_n(supp(p⊕q)))/(10n)`.
   Splice: W4-Q5-ANTIPODAL ⋉ W4-FLOOR-Q4. *(LPP)*
4. **Kill/cert phase algebra** `ρ_kill(n) = max{12, 4/e_anti(n)}` (exact piecewise law),
   `ρ_cert(n) = inf{ρ : Ψ↓_{n,ρ} ≥ 8}` with integer-comparison certification scheme
   (dyadic grid + big-integer powers), limit `10·log₂x₁`, `x₁³ = 4(x₁+1)`. *(LPP)*
5. **Threshold inverse formula** `ρ⋆(n,Θ,h,q,c) = F⁻¹_{n,Θ}(T(h,q,c)) = max_{j: e_j>0} (T−2−2ℓ_j)/e_j`,
   `T(h,q,c) = max(3+2h+q, 4+2h+2q+2c)` — floor family × candidate target → threshold. *(LPP/SMC)*
6. **Alias opacity probability space** — uniform injective alias K-tuple `A ↪ 𝒜` independent of
   payloads; private table `Π`; visible transcript `τ₁`; post-expand information equations
   `I = H(X_S)` / `I = dim π_Q(ker A)`. Splice: W4-OPAQUE-CAS-ALIAS ⋉ W4-LINEAR-ALIAS-RANK. *(AOT)*
7. **Two-demand product game** — product demand `θ⊗θ`, leaf error `e2(p,x) = 1 − a(p,x)²`,
   exact floors `F2_batch` (3-turn), `G2`/`H2` (4-turn); candidate family
   `{OPAQUE-DUAL, PARITY-DUAL, EDC-TWICE}`; latency Convention A.
   Splice: W4-FLOOR/DP ⋉ W4-LINEAR-ALIAS-RANK ⋉ W4-BATCH-PARITY-KILL. *(MDC)*
8. **Measured-cost corridor tuple** `(h_τ, q_τ, c_τ, Δ, p_miss, ΔM_fb, ΔL_fb)` with
   `A_M, A_L, T_H = max(A_M, 2A_L)` and the **latency-bound lemma** `2A_L − A_M = 1+q+2c > 0`
   (⇒ `T_H = 4+2s`, `s = h+q+c`, the costs are exactly exchangeable). *(SMC)*
9. **Scalarization equilibrium cone** `C* = {w > 0 : min_{b∈hull} w·(b−a) > 0}` with two-sided
   corner distortion `δ*` and one-sided defeats `ê_M, ê_L` (exact per-edge LP), repaired inner
   cones `C_suff ⊆ C_suff⁺ ⊆ C*`. Splice: W4-GEO-Q4 ⋉ W4-PHASE-MASTER ⋉ W4-FLOOR-Q4-CAP. *(RACE)*
10. **n=8 mask-tie phenomenon** — at `8 | n` the optimal one-bit family grows from `2^{n−1}`
    complement codebooks to `n·2^{n−1}` (dropping one light coordinate ties: the weighted
    Rademacher sum avoids `(−4,4)` by parity). *(LPP)*

---

## 4. Theorem index

All IDs below are **new in Wave 5**; none appeared in Wave 4. Every row is affirmative;
targeted obstructions live in §6 (each with its exact replacement).

| ID | Status | Statement | Splice of | Tag |
|----|--------|-----------|-----------|-----|
| W5-DLU-1 | PROVED | In `Π_ER(Θ;h,q,c)`: `M ≥ 3+2h+q`, `L ≥ 2+h+q+c`; minimal ledger point unique; `argmin_M = argmin_L`; class Pareto front = singleton | W4-DA-RATE ⋉ W4-DETERMINATION-FLOOR ⋉ W4-AFF-Q4-40 | DR+EC |
| W5-DLU-0 | PROVED | C5.1: `(5,0,4)` at (40,20)/Θ4cap forces `h=1, q=0, ℓ_extra=0, c=1`, expand = 1 token a.s. | W5-DLU-1, W4-DA-RATE | DR+EC |
| W5-DLU-STRUCT | PROVED | Minimizer set = explicit infinite family; recovery forced to 1-bit sufficient statistic (unique up to relabel/OTP); deterministic leak cap `n−1` with parity family the UNIQUE maximizer up to bijection; randomized leak `→ n` unattained | W4-EXTREMAL-NONUNIQUE ⋉ W4-DETERMINATION-FLOOR | DR+EC |
| W5-DLU-RADIUS | PROVED | `Δ_Θ = 0` exactly tight: any `Δ > 0` strictly improves `(M,L)` by `(Δ(1+q), Δ(1+q+c1))`; contract and D=0 restrictions also tight | W4-EXTREMAL-KILL ⋉ W4-DA-RATE | DR+EC |
| W5-LPP-ANTI | PROVED | `e_anti(n) = [2(n−1) − B(n)]/(5n)`, `B(n) ≤ (3n−8)e^{−(n+4)²/(32(n−1))}`; `e_anti → 2/5` at rate `Θ(1/n)`; exact table n=3..20 | W4-Q5-ANTIPODAL ⋉ W4-FLOOR-Q4 | DR+EC |
| W5-LPP-SYM | PROVED | Every complement pair `{p,p̄}` attains `e_anti(n)`: explicit `2^{n−1}` positive family | W4-FLOOR-Q4 | DR |
| W5-LPP-OPT | PROVED | One-bit optimum `= e_anti(n)` for **ALL n**; optimal codebooks `= 2^{n−1}` iff `8∤n`, `n·2^{n−1}` iff `8|n` | W4-Q5-ANTIPODAL ⋉ W4-FLOOR-Q4 | DR+EC |
| W5-LPP-KILL | PROVED | Exact law `ρ_kill(n) = 4/e_anti(n)` (3≤n≤7), `12` (n≥8); `e_anti(n) > 1/3 ∀n≥8`; `lim ρ_kill = 12` | W4-Q5-ANTIPODAL | DR+EC |
| W5-LPP-CERT | PROVED | `ρ_cert(3) = +∞`; certified integer-comparison brackets n=4..12; `ρ_cert(n) → 10·log₂x₁`, `x₁³ = 4(x₁+1)` | W4-Qn-SEPARABLE ⋉ W4-PHASE-MASTER | DR+EC+BE |
| W5-LPP-PRODUCT | PROVED | `ρ⋆(n,Θ,h,q,c) = F⁻¹(T) = max_j (T−2−2ℓ_j)/e_j`; reproduces 135/8, 160/11, 40/3, 64/5 and both h-curves; corridor `[12, 10·log₂x₁)` | W4-PHASE-MASTER ⋉ W4-FLOOR-Q4 ⋉ W4-PHASE-Q4-H | DR+EC |
| W5-AOT-1 | PROVED | `I(X₁..K ; A₁..K) = I(X; τ₁) = 0` exactly (uniform injective aliases) | W4-OPAQUE-CAS-ALIAS | DR+EC |
| W5-AOT-2 | PROVED | Post-expand `I(X; τ₁,S,R) = H(X_S) = 1` bit; batch `I = dim π_Q(ker A)` exactly | W4-LINEAR-ALIAS-RANK | DR+EC |
| W5-AOT-3 | PROVED | `R_DA,opaque = 2` attained and optimal among `I=0` handles; singleton rate 2 exact in Δ=0 class | W4-DA-RATE ⋉ W4-DETERMINATION-FLOOR | DR |
| W5-AOT-4 | PROVED | Opaque exact recovery ⇒ `H(Π) ≥ n`; minimal ⇒ `Π = X` up to relabeling: two-level alias→CAS is the canonical minimal factorization | W4-DETERMINATION-FLOOR ⋉ W4-OPAQUE-CAS-ALIAS | DR |
| W5-AOT-5 | PROVED | Re-issue keeps `I = 0` exactly (`E[draws] ≤ KN/(N−K+1)`); content tie-break `I ≤ K(K−1)H(X)/(2N)`; capacity `K·2^r ≤ N` | W4-ALIAS-CAPACITY | DR+EC |
| W5-AOT-6 | PROVED | Injective hash: `I = n`, `Δ = 1`, repriced leaky policy — never opaque; dichotomy with no interpolation | W4-DIRECT-HASH-KILL ⋉ W4-PROD-CORRIDOR-DELTA | DR |
| W5-MDC-FLOOR | PROVED | Exact two-demand floors: `F2_batch,↓` (breakpoints 80/9, 400/37, 400/27), `F2_batch,cap` (10, 800/71, 1600/123), `G2_↓` (40/3, 600/37, 200/9); `F2(40)=10`, `G2(40)=15` | W4-FLOOR/DP | DR+EC |
| W5-MDC-MONO | PROVED | `e2 ≥ e` pointwise ⇒ `F2 ≥ F4`; t=40 collapse both vertices without DP | W4-FLOOR-Q4 | DR |
| W5-MDC-RATE | PROVED | Rates: no-rec 4; batch opaque 3 / parity 2; seq opaque 3 / parity 2; EDC-twice 4; parity 2 optimal in Δ=0 exact-ref class; batch-m parity rate = 2 ∀m | W4-DA-RATE ⋉ W4-LINEAR-ALIAS-RANK | DR |
| W5-MDC-BATCH | PROVED | PARITY-DUAL `(5,0,4)` dominates two-demand hull at (40,20) on both vertices, sharp margins `(5,0,1)`; `ρ⋆ = 150/17` (↓), `1200/137` (cap) | W4-PHASE-MASTER ⋉ W4-BATCH-PARITY-KILL | DR+EC |
| W5-MDC-SEQ | PROVED | PARITY-DUAL `(8,0,4)` dominates sequential hull at (40,20), margins `(7,0,1)`; `ρ⋆ = 150/17` | W4-PHASE-MASTER | DR+EC |
| W5-MDC-INTERACTION | PROVED | Non-reduction: handle amortization `2h`/`3h` + residual rank 1 vs `|Q|`; diffs `1+2h+q` (batch), `1+3h+q` (seq) > 0 always | W4-LINEAR-ALIAS-RANK ⋉ W4-DETERMINATION-FLOOR | DR |
| W5-MDC-NECESSITY | PROVED | Any exact-ref policy with ≥2 expands pays `L ≥ 11/2 > 5 = max_ρ F2/2` ⇒ fails latency dominance for ALL ρ; PARITY-DUAL is the unique natural dominating dual shape | W4-DETERMINATION-FLOOR ⋉ W4-LINEAR-ALIAS-RANK | DR |
| W5-SMC-1 | PROVED | Master corridor: dominance ⇔ `F(ρ) ≥ T_H`; margins affine in all measured parameters; D-gate necessary | W4-PHASE-MASTER ⋉ W4-PROD-CORRIDOR-DELTA | DR |
| W5-SMC-2 | PROVED | C5.5 corrected: L-RHS `= F(2λ)/2`; over-certification exactly 1; corrected set sharp | W5-SMC-1 ⋉ W4-FLOOR-Q4-CAP | DR+EC |
| W5-SMC-3 | PROVED | `ρ⋆(h,q,c) = 4+4s \| 20s/3 \| 80(s−1)/7 \| +∞`, `s = h+q+c`; obstruction boundary s=3; reduces exactly to W4-PHASE-Q4-H | W4-PHASE-Q4-H ⋉ W4-PHASE-MASTER | DR+EC |
| W5-SMC-4 | PROVED | Exact 5-row measured table at (40,20): 4 PASS (2 boundary) / 1 FAIL | W5-SMC-1/3 | EC |
| W5-RACE-1 | PROVED | Collapsed gauge ⇒ unique `J_w`-minimizer for **all** `w ∈ ℝ³_{>0}`; weighted sums complete | W4-GEO-Q4 ⋉ W4-PHASE-MASTER | DR |
| W5-RACE-2 | PROVED | `C*` open convex cone = exact equilibrium region (≤4 vertex half-spaces); `δ*(ρ) = 19/(5(16−ρ)) \| 2/(10−ρ) \| +∞`; `ê(ρ)` 2-piece; repaired cones `C_suff ⊆ C_suff⁺ ⊆ C*`; maximality via vertex LP | W4-PHASE-MASTER ⋉ W4-FLOOR-Q4-CAP ⋉ W4-GEO-Q4 | DR+EC |

---

## 5. Proofs (affirmative first)

Compact but complete core proofs. Full-length versions with exhaustive EC logs are in the
six appendix files; every load-bearing number was independently recomputed by the orchestrator
(second C++ DP + pure-Python Fraction DP).

### 5.1 W5-DLU — Double-Ledger Uniqueness (Recipe A: Bound ⋉ Extremal ⋉ Uniqueness)

**Class (W5-DLU-CLASS).** `Π_ER(Θ;h,q,c)`: exact-recovery (`D=0`, recall 1, pin or exact
charged fallback), contract-respecting exact-ref envelope cost `h ≥ 1` a.s. (W4-DA-RATE: ≥1
reference token for omission), costs `q, c0, c1 ≥ 0`, `c = c0+c1`, and `Δ_Θ(H) = 0`.
On compact full-support Θ, `θ ↦ Δ_θ(H) = Σθ_iδ_i(H)` is linear, so the infimum is attained:
`Δ_Θ = 0 ⇔ δ(H) = 0` coordinatewise — a θ-free per-handle property. In class, every demand
is unresolved a.s., so exactness forces expansion `e_π = 1` and `|RS| ≥ 1` a.s. (prefix-free:
an empty codeword cannot coexist with another codeword).

**Accounting lemma (W5-DLU-ACCT).** With `ℓ := ℓ_extra`, `r := E|RS|`, `e := Pr[E]`, `Δ := Δ_θ`:
`M = 2(1+h+ℓ) + r + qe`, `L = 1+h+ℓ+c0 + r + (q+c1)e`, floors `h ≥ 1`, `r,e ≥ 1−Δ`
(W4-DETERMINATION-FLOOR). Hence the slack decomposition, relative to
`m* := 3+2h+q`, `ℓ* := 2+h+q+c`:
`M − m* = 2ℓ + (r−1) + q(e−1)`,  `L − ℓ* = ℓ + (r−1) + (q+c1)(e−1)`.  ∎

**Theorem W5-DLU-1 (ledger uniqueness).**
(A) *Floor:* in class, `Δ = 0` ⇒ `e = 1`, `r ≥ 1`, so both slacks are sums of nonnegative
terms: `M ≥ m*`, `L ≥ ℓ*`, `D = 0`.
(B) *Attainment:* `π_EDC` (opaque alias, `ℓ_extra = 0`, expand = exactly `X_S` = one token)
lies in class and attains `(m*, 0, ℓ*)`.
(C) *Equality characterization:* `(a) M = m* ⇔ (b) L = ℓ* ⇔ (c) ℓ_extra = 0 AND |RS| = 1`
a.s. on every `s ∈ supp θ ⇔ (d) ledger = (m*,0,ℓ*)`. Forward: plug in. Back: the slack is a
sum of two nonnegative terms, so `ℓ = 0` and `E|RS| = 1`; with `|RS| ≥ 1` a.s. (integer-valued
variable `Y ≥ 1` a.s., `E[Y−1] = 0` ⇒ `Y = 1` a.s.) — the a.s. prefix-free floor converts mean
equality into a.s. equality, on every demanded coordinate by full support.
Hence **all minimizers share the same (M,D,L) point** (`argmin_M = argmin_L`) and the class
achievable set at `D = 0` is contained in `[m*,∞)×[ℓ*,∞)` and contains its corner: **the
class Pareto front is the singleton `{(m*,ℓ*)}`** — double-ledger uniqueness. ∎
(Gauge-free: penalty terms vanish at `D = 0`; (40,20) enters only the cross-class comparison
via W4-AFF-Q4-40.)

**Theorem W5-DLU-0 (C5.1, PROVED).** `M = 5` and the floor give `3+2h+q = 5`, so `2h+q = 2`
with `h ≥ 1` (a.s. integer count) and `q ≥ 0`; then `2h ≤ 2` so `h = 1`, `q = 0`; and
`E[hcnt] = 1` with `hcnt ≥ 1` a.s. forces `hcnt = 1` a.s. `L = 4 = 3 + c` forces `c = 1`.
W5-DLU-1(C) gives `ℓ_extra = 0`, `|RS| = 1` a.s. on every `s ∈ [4]`; recall 1 forces the
eviction discipline. So every policy attaining `(5,0,4)` matches the EDC accounting up to
measure-zero events. (EC: unique rational solution `(h,q) = (1,0)` verified exhaustively.) ∎

**Theorem W5-DLU-STRUCT (extremizer characterization).**
(a) *Forced recovery content:* for every minimizer, a.e. handle value `v` and demand `s`:
`X_s` is non-constant on the fiber, the one-token message `M(v,s)` separates the two
half-fibers, and the decode map is a **bijection** on used symbols — the message is a 1-bit
sufficient statistic of `X_s`, unique up to per-`(v,s)` relabeling and decoder-invertible
private randomization (e.g. OTP `M = X_s ⊕ R`).
(b) *Deterministic-handle leak cap with parity uniqueness:* deterministic `H = h(X)` in class
has fibers spanning every coordinate, so `|F(v)| ≥ 2` a.s. and
`I(X;H) = n − E[log₂|F(H)|] ≤ n−1`, equality iff `|F| = 2` a.s.; a 2-point fiber spanning all
coordinates is a complement pair `{x, x⊕1ⁿ}` (EC: exhaustive n=2..5), and the complement
partition is canonical — **the maximal-leak deterministic handle is unique up to bijection:
the parity/complement-pair alias of W4-EXTREMAL-NONUNIQUE**.
(c) *Randomized handles leak strictly more:* the ε-complement channel (`H = X` w.p. `1−ε`,
`X⊕1ⁿ` w.p. `ε`) has `Δ_Θ = 0` and `I(X;H) = n − H_2(ε) → n` unattained (`I = n ⇔ Δ = 1`);
exact member `n=2, ε=1/4`: `I = (3/4)log₂3 > 1` by integer certificate `27 > 16`. Since
`I(X;H)` is bijection-invariant, no handle-level "uniqueness up to bijection" exists.
(d) *Parameterization:* the minimizer set is the explicit product
`{Δ=0 handle distributions} × {recovery relabelings/OTP}` — an infinite family, all sharing
the point `(m*,0,ℓ*)`. Ledger uniqueness is the maximal true uniqueness statement. ∎

**Theorem W5-DLU-RADIUS (class restriction exactly tight).** Any exact-recovery
contract-respecting policy with `Δ := Δ_θ(H) > 0` (existence: W4-EXTREMAL-KILL,
`Δ_θ = θ(B) > 0`) has attainable ledger `M = m* − Δ(1+q) < m*`,
`L = ℓ* − Δ(1+q+c1) < ℓ*` at the same `D = 0` — strict componentwise improvement. So the
conclusions of W5-DLU-1 hold at `Δ = 0` and fail at every `Δ > 0`: the uniqueness radius is
the single point `{Δ = 0}`. On Θ4cap: `θ(B) ≥ |B|/5`; at `|B| = 1`, ideal costs,
`M_B = 24/5`, `L_B = 37/10` (improvements `1/5`, `3/10`). The contract restriction is tight
(pure retrieval at `M = 3+q`, W4-DA-RATE) and the `D=0` restriction is tight (below-threshold
gauges, W4 phase algebra). ∎

### 5.2 W5-LPP — Linked-Phase Product (Recipe B: Floor ⋉ Phase ⋉ Dimension)

Setup: heavy vertex of `Θ_n↓`, weights `w_1 = n+4`, `w_i = 4` (`i ≥ 2`), total `W = 5n`;
one-bit codebook `{p,q}` with weighted-nearest encoding, error `e({p,q}) = E_X min(d_θ(X,p), d_θ(X,q))`.

**Theorem W5-LPP-ANTI (closed form).** With `K ~ Bin(n−1,1/2)`, `B(n) := E[(8K−5n)⁺] ≥ 0`:
(a) `e_anti(n) = Σ_k C(n−1,k)min{4k,5n−4k}/(5n·2^{n−1}) = [2(n−1) − B(n)]/(5n)`
— first equality by pairing `x ↔ x̄` (`min{n+4+4k, 4(n−1−k)} = min{4j, 5n−4j}` under
`j = n−1−k`); second by `min{4k,5n−4k} = 4k − (8k−5n)⁺` and `E[4K] = 2(n−1)`.
(b) `e_anti(n) ≤ 2(n−1)/(5n) < 2/5`.
(c) `8k−5n ≤ 3n−8` on the tail, so `B(n) ≤ (3n−8)·Pr[K > 5n/8] ≤ (3n−8)·e^{−(n+4)²/(32(n−1))}`
(Hoeffding); hence `2/5 − e_anti(n) = 2/(5n) + B(n)/(5n)` with `B(n)/(5n) = o(1/n)`
exponentially: `e_anti(n) → 2/5` at rate `Θ(1/n)`, constant `2/5`.
(d) Exact table n=3..20 (W4 row n=3..8 reproduced bit-exactly). ∎

**Theorem W5-LPP-SYM (complement invariance).** `d_θ(x,p̄) = 1 − d_θ(x,p)`, so
`e({p,p̄}) = E[min(D, 5n−D)]/(5n)` with `D = (n+4)B_1 + 4Σ_{i≥2}B_i`, `B_i` iid Bernoulli(1/2)
**for every p** (sign invariance): the value is independent of `p`, and equals `e_anti(n)` at
`p = 0ⁿ`. The exact one-bit value is attained by `2^{n−1}` codebooks. ∎

**Theorem W5-LPP-OPT (all-n optimality + classification).**
*Lemma 1 (support reduction):* `min(a,b) = (a+b−|a−b|)/2` and `E d_θ(X,c) = 1/2` give
`e({p,q}) = 1/2 − (1/2)E|Σ_{i∈S}θ_iR_i| = (5n − g_n(S))/(10n)`, where `S = supp(p⊕q)` and
`R_i` iid Rademacher: the error depends only on the difference support.
*Lemma 2 (max-identity):* `E|Y + wR| = (|Y+w|+|Y−w|)/2 = E max(|Y|,|w|) ≥ E|Y|`.
(i) By Lemma 2, `g_n` is monotone under inclusion, so it is maximized at `S = [n]` —
complement pairs are one-bit optimal for **all n**, with
(ii) closed forms `g_n([n]) = (n+4) + 2B(n)` and `e_anti(n) = (5n − g_n([n]))/(10n)`.
(iii) *Classification:* dropping the heavy coordinate is always strict
(`Pr(|4U| < n+4) > 0`). Dropping one light coordinate `j`: `g_n([n]) − g_n([n]∖{j}) = E[(4−|Z|)⁺]`
with `Z = (n+4)R_1 + 4U`, `U ≡ n (mod 2)`: `|Z| < 4` is achievable iff the open interval
`(n/4, n/4+2)` contains an integer `≡ n (mod 2)`, which fails **iff `8 | n`** (if `4∤n` the
interval has ≥2 integers hence both parities; if `n = 4m` it has only `m+1`, and
`m+1 ≢ 4m (mod 2)` iff `m` even iff `8|n`). Any support missing ≥2 coordinates is strictly
suboptimal (chain argument). Hence optimal supports `= {[n]}` always, plus `{[n]∖{light}}`
iff `8|n`; codebook counts `2^{n−1}` resp. `n·2^{n−1}`.
EC: pair enumeration n=3..6 (28/120/496/2016 pairs; minima `1/4, 11/40, 121/400, 5/16`; only
complements optimal); support classification n≤60 matches the dichotomy; n=8: **1024** optimal
pairs = `8·2^7` (orchestrator's independent n≤8 enumeration identical). ∎

**Theorem W5-LPP-KILL (exact kill law).**
(a) `4/e_anti(n) ≥ 10n/(n−1) > 10` (from (b) above), and `4/e_anti(n) → 10`.
(b,c) `e_anti(7) = 145/448 < 1/3` (`435 < 448`), `e_anti(8) = 43/128 > 1/3` (`129 > 128`), and
`e_anti(n) > 1/3` for all `n ≥ 8`: EC for `8 ≤ n ≤ 101` (min margin `1/384` at n=8); for
`n ≥ 102`, `3B(n) ≤ 3(3n−8)e^{−(n+2)/32} < n−6`, certified at n=102 by the exact rational
series bound `e^{13/4} > Σ_{j≤40}(13/4)^j/j! ≈ 25.790 > 149/16 ≥ 3(3n−8)/(n−6)`.
(d) Exact law: `ρ_kill(n) = 4/e_anti(n)` for `3 ≤ n ≤ 7` (values `16, 160/11, 1600/121, 64/5,
1792/145`) and `ρ_kill(n) = 12` for `n ≥ 8` (zero-message L-witness `L_b = 1+ρ/4 < 4`).
So `lim ρ_kill = 12` — **not** 10; only the antipodal branch tends to 10. ∎

**Theorem W5-LPP-CERT (certified certificate thresholds).**
(0) `ρ_cert(3) = +∞`: `Ψ↓_{3,t} = 8 − log₂[(1+2^{−7t/30})²(1+2^{−2t/15})⁴] < 8` for all finite
`t` — the certificate is vacuous at n=3; the phase is closed by the exact DP floor `135/8`.
(1) For n=4..12, brackets `ρ_lo(n) < ρ_cert(n) ≤ ρ_hi(n)` each certified by EXACT INTEGER
COMPARISON on a dyadic grid `t = 10n·j/2^14`: both exponents dyadic, `2^{−A/2^s}` bracketed by
`m/2^40` via big-integer powers, then `(2^40+m_a)²(2^40+m_b)^{2(n−1)}` vs `2^{2n−6+80n}` —
the scheme of W4's `257·17³ < 2²¹` (which is exactly its `n=4, t=40` instance). Sample (n=8,
`ρ_hi = 955/64`): a 196-digit integer inequality `(2^40+233029032246)²(2^40+655546085238)^14 < 2^650`.
Roots (BE, matching the orchestrator's independent bisection to 6 dp): n=4: 20.761270;
n=5: 17.577411; n=6: 16.203387; n=7: 15.425000; n=8: 14.921276; …; n=12: 13.943051.
(2) Limit: `Ψ↓_{n,t} → Ψ↓_{∞,t} = 2 + ψ(t/5) + 2t/5` pointwise (`ψ'(0) = 1/2` gives
`(n−1)ψ(4t/5n) → 2t/5`); the limit root solves `1+2^{−s} = 2^{2s−2}` (`s = t/10`), i.e.
`x³ = 4(x+1)` with `x = 2^s`, so `ρ_cert(n) → 10·log₂x₁`,
`x₁ = ∛(2+2√33/9) + ∛(2−2√33/9) = 2.382975767906…` (Cardano; discriminant `−176 < 0`, one real
root), `10·log₂x₁ = 12.527642810… = 4 + 4log₂(x₁+2) > 12` (via `x₁⁵ = 4(x₁+2)²`).
(3) The open phase `[ρ_kill(n), ρ_cert(n))` shrinks to `[12, 10·log₂x₁)`, width `≈ 0.527643`,
non-vacuous by (2). ∎

**Theorem W5-LPP-PRODUCT (closed-form phase algebra).** With `F_{n,Θ}(t) = min_j(2+2ℓ_j + t e_j)`
the exact floor (supported pairs `(ℓ_j,e_j)`) and `T(h,q,c) = max(3+2h+q, 4+2h+2q+2c)`:
`F(t) ≥ T ⟺ t ≥ (T−2−2ℓ_j)/e_j ∀j`, so
`ρ⋆(n,Θ,h,q,c) = max_{j: e_j>0} (T(h,q,c) − 2 − 2ℓ_j)/e_j`
(`+∞` iff a supported `e_j = 0` line has `2+2ℓ_j < T`; at `n=2`, `T=8 > 6 = 2n+2` recovers the
W4 impossibility). Exact closures verified: `135/8` (pair `ℓ = 15/8`, e = 2/15), `160/11`
(pair `ℓ=1` = **the antipodal code** — the n=4 kill is tight), `40/3`, `64/5`; both
W4-PHASE-Q4-H curves with all breakpoints (max-formula `max{8+4h, (20/3)(1+h), 80h/7}` etc.);
M-only `ρ⋆_M = 6` at `h=1` on all four exact classes; asymptotics 7.5 / 15 / 17.5 (Kimi–Jensen
limit floor, PI). The full general-n statement: false below `ρ_kill(n)`, certified above
`ρ_cert(n)`, exact at n=3,4, corridor `[12, 10·log₂x₁)`. ∎

### 5.3 W5-AOT — Alias-Opacity Theorem (Recipe C: Kill ⋉ Repair ⋉ Rate)

Probability space: K live entries `X = (X_1,…,X_K)` iid `Unif({0,1}ⁿ)` (post-mask bytes,
Option A); alias vector `A = (A_1,…,A_K)` uniform over injective K-tuples of `𝒜`
(`|𝒜| = N ≥ K`), independent of payloads; private table `Π = T[A] = (H(X_e), g_e, sel_e, meta_e)`
never visible; visible transcript `τ₁ = (c₀, σ, A)` (control + source-independent schema).

**Theorem W5-AOT-1 (exact joint opacity).** By construction `p(x,a) = p(x)p(a)` with
`p(a) = 1/(N)_K` on injective tuples for every `x`, so `I(X;A) = 0` exactly; chain rule with
`c₀, σ` constants gives `I(X; τ₁) = 0 + 0 + 0 = 0`. EC: all 24 cells factorize at
`(n=1,N=3,K=2)`. ∎

**Theorem W5-AOT-2 (post-expand information equation).** Chain rule:
`I(X; τ₁,S,R) = I(X;τ₁) + I(X;S|τ₁) + I(X;X_S|τ₁,S) = 0 + 0 + H(X_S) = 1` bit
(`S ⊥ (X,τ₁)` jointly; `X_s ⊥ (τ₁,S)` given `S=s`). Batch with visible syndrome `Z = 𝖠X`:
`I(X; τ₁,Q,R_Q | Z) = dim π_Q(ker 𝖠)` exactly — `τ₁ ⊥ (X,Z)` jointly survives conditioning on
`Z` (joint independence `p(x,t,z) = p(x,z)p(t)`), and given `Z = z` the source is uniform on
the coset `x* + ker 𝖠`, so `X_Q` is uniform on a `2^{dim π_Q(ker 𝖠})`-point affine set.
Total: `I(X; τ₁,Q,R_Q,Z) = rank 𝖠 + dim π_Q(ker 𝖠)`. **Opacity loss equals charged recovery,
exactly — nothing leaks that was not charged.** EC: kernel dims and coset uniformity
enumerated. ∎

**Theorem W5-AOT-3 (rate optimality).** The construction attains `R_DA,opaque = 2` (1 ref +
1 recovered token; W4-DA-RATE). Optimality: `I(X;V) = 0` ⇒ every fiber is full-support ⇒
`Δ_θ = 0` ∀θ ⇒ ≥1 recovery token a.s. per demand (W4-DETERMINATION-FLOOR) and ≥1 token for
the nonempty prefix handle (omission contract), so rate ≥ 2; attainment by the two-level
construction. In the wider Δ=0 class the singleton rate is still exactly 2; batch re-prices
via residual rank (parity pays 1 for every nonempty batch). ∎

**Theorem W5-AOT-4 (converse/factorization).** Any exact-recovery mechanism with visible `V`
and private state `Π` has `X = ℰ(V,Π)` a.s. (run the expander over all full-support demands),
so `H(X | V,Π) = 0` and `n = H(X) = I(X;V) + I(X;Π|V) ≤ H(Π)`: **`H(Π) ≥ n` whenever
`I(X;V) = 0`**, and `Π` cannot be a function of `V` (else `I(X;V) = n > 0`). Minimality
(`H(Π) = n`) forces equality throughout: `Π ⊥ V`, `Π` uniform on `2ⁿ` points, `ℰ(v,·)` a
bijection, and `Π` a.s. a function of `(V,X)` — i.e. up to V-measurable relabeling, `Π = X`:
**every minimal opaque exact-recovery mechanism is a private copy of the source behind an
independent visible handle** — the two-level alias→CAS is the canonical minimal factorization,
and no mechanism with `Π = ∅` can be both opaque and exact-recovering. ∎

**Theorem W5-AOT-5 (ε-birthday robustness).** With-replacement aliases:
`Pr(E) = 1 − (N)_K/N^K ≤ K(K−1)/(2N)` (EC exact at three `(N,K)`). (a) Re-issue until
injective ⇒ the issued law is uniform over injective tuples ⇒ `I = 0` exactly;
`E[draws] = Σ_{i<K} N/(N−i) = N(H_N − H_{N−K}) ≤ KN/(N−K+1)` (EC: `6061/1365 ≤ 64/13` at
`(16,4)`). (b) Content-dependent public tie-break: `1_E ⊥ X`, so
`I(X;Â) ≤ Pr(E)·I(X;Â|E) ≤ K(K−1)H(X)/(2N)` — strict witness `I = 8/3 − (5log₂5+7log₂7)/12 ≈ 0.0616`
bits at `(N=3,K=2,n=1)` (integer cross-multiplication); content-free rules keep `I = 0`
(bound prices the admissible rule class). (c) Capacity splice: rank-r visible syndromes need
`K·2^r ≤ N` — opacity (`r=0`) is free; each leaked bit halves live-entry capacity. ∎

**Theorem W5-AOT-6 (opacity gate).** Injective visible hash: `I(X;H(X)) = n`, `Δ = 1` —
repriced as a rate-`(1+h_actual)` leaky corridor policy (W4-PROD-CORRIDOR-DELTA), never
opaque. Dichotomy: pay visible `h_actual` and `Δ = 1`, or pay 1 visible + 1 recovery token at
`I = 0`; AOT-3 fixes the latter at exactly 2; nothing interpolates without leaving the class. ∎

### 5.4 W5-MDC — Multi-Demand Cycle (Recipe D: single demand ⋉ two demands)

Locked: `X ~ Unif({0,1}⁴)`, `(S1,S2) ~ θ⊗θ`, both answers required (`D=0`); batch 3-turn
(`M = 2(1+ℓ)+ρe2`) and sequential 4-turn (`M = 3(1+ℓ)+ρe2`) ledgers; leaf task error
`e2(p,x) = 1 − (Σ_s θ_s 1[p_s=x_s])²`; Convention-A latency `L = 1+h+c0+Σ_expands(1+q+c1)`.

**Theorem W5-MDC-FLOOR (exact two-demand floors, EC+DR).** The prefix-tree DP with leaf
`E2_θ(A) = min_p Σ_{x∈A} e2(p,x)` (exact integer scaling) gives:
```
F2_batch,↓:  2+17t/25 (t≤80/9) | 4+91t/200 (80/9≤t≤400/37) | 6+27t/100 (400/37≤t≤400/27) | 10 (t≥400/27)
F2_batch,cap: 2+137t/200 (t≤10) | 4+97t/200 (10≤t≤800/71) | 6+123t/400 (800/71≤t≤1600/123) | 10 (t≥1600/123)
G2_↓:        3+17t/25 (t≤40/3) | 6+91t/200 (40/3≤t≤600/37) | 9+27t/100 (600/37≤t≤200/9) | 15 (t≥200/9)
```
`F2_batch(40) = 10` (both vertices), `G2(40) = 15`, `H2(40) = 10` — both fronts collapse to
identity `(10,0,5)` / `(15,0,5)` at the declared gauge. Envelope correctness: vertex reduction
(`e2` concave in `θ` — negative square of a linear form), DP exactness as in W4, breakpoint
tie-equalities verified at all 9 breakpoints, and concavity/interpolation (true floor concave,
equal to the envelope at all grid points ⇒ equal everywhere). Two independent C++
implementations + one pure-Python Fraction DP: zero discrepancies.
**Lemma W5-MDC-MONO:** `e2(p,x) = 1−q² ≥ 1−q = e(p,x)` pointwise, equality iff `q ∈ {0,1}` ⇒
`F2 ≥ F4` ⇒ the `t=40` collapse needs no DP. ∎

**Theorem W5-MDC-RATE.** No-recovery zero-error rate `= 4` (transcript determines X, W4-ZE-GORDIAN
argument). Batch: opaque `= 1 + r_A(Q) = 3` (`r_A(Q) = |Q| = 2` when `S1≠S2`), parity
`= 1 + r_A(Q) = 2` (`dim π_Q(span 1⁴) = 1` ∀ nonempty Q; recover `X_{S1}` — the antipodal
fiber `{x0, x̄0}` collapses to a singleton, so `X_{S2}` is free). Seq: opaque 3, parity 2
(expand2 = 0 tokens). EDC-twice `= 4`. Parity rate 2 is optimal in the Δ=0 exact-ref class
(≥1 ref + ≥1 recovery token, W4-DETERMINATION-FLOOR). **General batch-m:** parity rate `= 2`
for ALL `m ≥ 1` (residual rank 1), opaque `= 1+m`, EDC-m `= 2m`. ∎

**Theorem W5-MDC-BATCH (dominance).** Candidate ledgers (registered instance, Convention A):
PARITY-DUAL `(M,D,L) = (5,0,4)` — **identical to the single-demand EDC ledger**;
OPAQUE-DUAL `(6,0,11/2)`; EDC-TWICE `(8,0,13/2)`. Phase reduction (W4-PHASE-MASTER splice):
dominance ⟺ `F2_batch(ρ) ≥ T(a) = max(M(a), 2L(a))`. At (40,20), `F2_batch(40) = 10`
(collapsed): **PARITY-DUAL dominates the full two-demand no-recovery hull on both Θ4↓ and
Θ4cap with sharp margins `(γ_M,γ_D,γ_L) = (10−5, 0, 5−4) = (5,0,1)` — the second demand is
free.** Exact thresholds: `F2_batch,↓(ρ) ≥ 8 ⟺ ρ ≥ 150/17` (segment `2+17ρ/25`, `< 80/9`);
`F2_batch,cap(ρ) ≥ 8 ⟺ ρ ≥ 1200/137` — **strictly below** the single-demand thresholds
`160/11`, `40/3`: product demand inflates lossy error (W5-MDC-MONO), so dominance begins
sooner. ∎

**Theorem W5-MDC-NECESSITY (residual rank 1 is load-bearing).** Convention-A latency
`L = 1+h+c0+(#expands)(1+q+c1)` (the unique decomposition consistent with
`L_EDC = 2+h+q+c`): any exact-ref policy needing `m_exp ≥ 2` expands (opaque pair, EDC-twice,
or any Δ=0 alias with `dim π_Q(ker A) ≥ 2`, W4-LINEAR-ALIAS-RANK) pays
`L ≥ 1+1+1/2+2(3/2) = 11/2 > 5 = max_ρ F2_batch(ρ)/2`, hence fails latency dominance of the
two-demand hull **for all ρ**, while its M-dominance can hold (`γ_M = 4` batch at (40,20)).
**PARITY-DUAL — residual rank 1, exactly one expand for any batch — is the unique natural dual
policy shape that dominates**; the interaction term is a necessity certificate. (Alternate
conventions — fused round-trip `L = 5` giving weak dominance `(4,0,0)` at the collapsed front
only, mission-sketch B `L = 9/2` — are quarantined in the appendix; every PARITY-DUAL statement
is identical under all conventions.) ∎

**Theorem W5-MDC-SEQ (sequential dominance).** Ledgers: PARITY-DUAL `(8,0,4)`
(`M = 3(1+h)+2(1+q) = 5+3h+2q`), OPAQUE-DUAL `(9,0,11/2)`, EDC-TWICE `(12,0,13/2)`
(corrected strawman `6+6h+3q`, stronger than the sketch's slip), identity `(15,0,5)`.
At (40,20), `G2(40) = 15`, `H2(40) = 10`: **PARITY-DUAL dominates the sequential hull with
sharp margins `(γ_M,γ_D,γ_L) = (15−8, 0, 5−4) = (7,0,1)`**; thresholds `G2 ≥ 8`
(`ρ ≥ 125/17`), `H2 ≥ 8` (`ρ ≥ 150/17`, binding). OPAQUE-DUAL: `γ_M = 6`, `γ_L = −1/2`
(M-dominant, L-incomparable); EDC-TWICE: `(3,0,−3/2)`, fails. ∎

**Theorem W5-MDC-INTERACTION (non-reduction).** (i) *Handle amortization:* one handle serves
both demands — saves `2h` (batch) / `3h` (seq) vs EDC-TWICE. (ii) *Residual-rank term:*
`r_A({S1,S2}) = 1 ≠ |Q| = 2` — the second demand costs ZERO recovered tokens, impossible in
any independent product (each demand is an unresolved singleton needing its own ≥1-token
recovery, W4-DETERMINATION-FLOOR). (iii) Exact diffs: batch `(4+4h+2q) − (3+2h+q) = 1+2h+q = 3`;
seq `(6+6h+3q) − (5+3h+2q) = 1+3h+q = 4` — strictly positive for every `h ≥ 1, q ≥ 0`
(literal two-capsule strawman: 7, a fortiori). (iv) Marginal rate cost of demands `2,…,m`
under PARITY-DUAL is exactly 0; under any independent product ≥ 1 per demand. ∎

### 5.5 W5-SMC — Spliced Margin Corridor (Recipe: margins as explicit functions of measured costs)

**Theorem W5-SMC-1 (master parametric corridor).** Candidate: measured-cost exact-ref policy
with `(h_τ,q_τ,c_τ = c0+c1)`, worst-case determination `Δ = inf_Θ Δ_θ`, miss probability
`p_miss` with exact fallback `(ΔM_fb, ΔL_fb)` (**D-gate:** miss without exact fallback ⇒
`D > 0` ⇒ theorem void — necessary, not decorative). Then
`A_M = 2(1+h_τ) + (1−Δ)(1+q_τ) + p_missΔM_fb`,
`A_L = 1+h_τ+c0 + (1−Δ)(1+q_τ+c1) + p_missΔL_fb`, `T_H = max(A_M, 2A_L)`, and on the linked
slice: **three-objective dominance with `D = 0` ⟺ `F(ρ) ≥ T_H` with ≥1 strict margin**, with
sharp uniform margins affine in every measured parameter:
`γ_M = [F(ρ)−3+Δ] − 2h_τ − (1−Δ)q_τ − p_missΔM_fb`,
`γ_L = [F(ρ)/2−2+Δ] − h_τ − (1−Δ)q_τ − c0 − (1−Δ)c1 − p_missΔL_fb`, `γ_D ≡ 0`.
Proof: ledger exactness (W4-DETERMINATION-FLOOR + PROD-CORRIDOR-DELTA + eviction adjustment);
sufficiency via W4-PHASE-MASTER; necessity via floor attainment (`c_b = 0` free); sharpness by
definition of the inf. Two-parameter form: `F(ρ) ≥ A_M ∧ F(2λ) ≥ 2A_L`. ∎

**Theorem W5-SMC-2 (C5.5 corrected, with certificate).** The exact corridor is
`3+2h+q+γ_M ≤ F(ρ)` and `2+h+q+c+γ_L ≤ F(2λ)/2` — C5.5's L-RHS `1+F4(ρ)/2` double-counts the
turn-1 base cost. Over-certification certificate: at (40,20) cap ideal, C5.5 certifies
`γ_L ≤ 2` but the identity baseline has `L = 5` with candidate `L = 4`, so the sharp margin is
exactly `1`; C5.5 as written asserts `L_b ≥ 6` for all baselines — false at identity. The
corrected set is exact and sharp (identity attains both floors at collapsed gauges). ∎

**Theorem W5-SMC-3 (explicit threshold map).** *Latency-bound lemma:*
`2A_L − A_M = 1+q+2c > 0` (for general `Δ < 1`: `2c0 + (1−Δ)(1+q+2c1) > 0`) — the L-branch
always binds, so `T(h,q,c) = 4+2s` with **`s = h+q+c`** (handle, selector, latency costs
exactly exchangeable). Then on Θ4cap, linked slice, `Δ = p_miss = 0`:
```
ρ⋆(h,q,c) = 4+4s  (s ≤ 3/2) | 20s/3  (s ≤ 12/5) | 80(s−1)/7  (s ≤ 3) | +∞  (s > 3, INFEASIBLE)
```
with latency-obstruction boundary `s = 3` (`ρ⋆ = 160/7`); branch joints continuous
(`10, 16, 160/7`). At `q=0, c=1` this reduces **exactly** to W4-PHASE-Q4-H
(`8+4h | (20/3)(1+h) | 80h/7 | +∞`, EC-verified at 200 rational h); at `(1,0,1)`: `40/3`.
The Qn corridor `3+2h+q ≤ Φ*_{n,ρ} ∧ 2+h+q+c ≤ Φ*_{n,2λ}/2` is sufficient (correctly labeled;
`Φ*_{4,40} ≈ 9.3003 < 10 = F4,cap(40)` shows the gap). ∎

**Theorem W5-SMC-4 (worked measured table, EC).** At (40,20) cap: `(1,0,1)` →
`(A_M,A_L,T_H,γ_M,γ_L) = (5,4,8,5,1)` PASS (matches W4-GEO-Q4); `(3/2,0,1)` → `(6,9/2,9,4,1/2)`
PASS; `(2,0,1)` → `(7,5,10,3,0)` PASS (L-tight, strict via `γ_M`); `(1,1/2,3/2)` →
`(11/2,5,10,9/2,0)` PASS (boundary); `(2,1,2)` → `(8,7,14,2,−2)` FAIL (infeasible). ∎

### 5.6 W5-RACE — Recovery-Aware Competitive Equilibrium (Recipe E: Geometry ⋉ Phase)

**Theorem W5-RACE-1 (full-orthant cone at collapsed gauges).** At any gauge with
`F(ρ) = F(2λ) = 2+2n` (front collapsed to identity), for a `D=0` candidate with margins
`(γ_M, 0, γ_L)`, `γ_M+γ_L > 0`: every baseline satisfies `b − a ≥ (γ_M, 0, γ_L)` componentwise,
so for **every** `w ∈ ℝ³_{>0}`,
`w·(b−a) ≥ w_Mγ_M + w_Lγ_L > 0` — **π_EDC is the unique minimizer of `J_w` over
`hull(B_NR) ∪ {a}` for the full open positive orthant**. At (40,20): `w·(b−a) ≥ 5w_M + w_L > 0`.
Weighted sums are complete here (no unsupported-point obstruction — the candidate componentwise
dominates the collapsed front), strictly strengthening W4-GEO-Q4's single Tchebycheff witness. ∎

**Theorem W5-RACE-2 (exact cone at general gauges + maximality).**
*Cone structure:* `C* = {w > 0 : min_{b∈hull} w·(b−a) > 0}` is an open convex cone
(`φ(w) = min_b w·(b−a)` concave, positively homogeneous); `w·b` is affine in `(ℓ_b, e_b)`, so
the min is attained at a supported vertex — for Θ4cap the four vertices
`(0,1/2),(1,3/10),(2,7/40),(4,0)` suffice: `C*` is an explicit intersection of ≤4 open
half-spaces.
*Exact defeat distortions (per-edge LP, linked slice):* two-sided corner
`δ*(ρ) = 19/(5(16−ρ))` (`ρ ≤ 10/3`) `| 2/(10−ρ)` (`≤ 6`) `| +∞`; one-sided
`ê(ρ) = ê_L(ρ/2) = 14/(160−7ρ)` (`ρ ≤ 80/7`) `| 3/10 − (40−3ρ)/(10(16−ρ))` (`≤ 40/3`) `| +∞`;
both attained (public-coin mixtures realize edge points).
*Repaired sufficient cones:* with `ΔM = (M_a−F(ρ))⁺`, `ΔL = (L_a−F(2λ)/2)⁺`:
`C_suff = {w_D ê > w_MΔM + w_LΔL}` and
`C_suff⁺ = {w_D δ* > w_MΔM + w_LΔL, w_D ê_M > w_MΔM, w_D ê_L > w_LΔL}` satisfy
`C_suff ⊆ C_suff⁺ ⊆ C*` at **every** gauge (case split: two-sided/M-only/L-only defeaters).
*Maximality:* for `w ∉ C*`, the vertex LP minimizer `b* ∈ argmin_k w·(v_k − a)` is an exhibited
defeating baseline — `C*` is the exact equilibrium region; `(π_EDC, w)` is a saddle of the
scalarized game for all `w ∈ C*`. Worked example (gauge (3,3/2), EC):
`C* = {w_D > max(3w_M + 9w_L/2, w_M/3 + 31w_L/6, −61w_M/7 + 59w_L/14)}`,
`C_suff = {w_D > (417/28)w_M + (1251/56)w_L}`, strict chain `C_suff ⊊ C_suff⁺ ⊊ K ⊊ C*`
(20 000-weight exact sweep). ∎

---

## 6. Targeted obstruction map (only obstacles to positive claims; each with exact replacement)

| # | Obstacle to | What fails (exact certificate) | Exact true replacement (proved) |
|---|-------------|--------------------------------|---------------------------------|
| 1 | Literal EDC uniqueness (RQ1) | Parity/complement aliases tie the EDC ledger (`I = n−1`, `Δ = 0` — W4-EXTREMAL-NONUNIQUE); ε-complement channels leak `n − H_2(ε) → n` with `Δ = 0` (`27 > 16` certificate); `I(X;H)` is a bijection-invariant separating minimizers | **W5-DLU-1** ledger uniqueness + Pareto singleton; **W5-DLU-STRUCT** extremizer family; parity family is the UNIQUE max-leak deterministic handle |
| 2 | Dream "π_EDC optimal outside Δ=0" | Any `Δ > 0` strictly improves `(M,L)` (`Δ(1+q)`, `Δ(1+q+c1)`; Θ4cap margins `(1/5, 3/10)` at `|B|=1`) | **W5-DLU-RADIUS**: uniqueness radius is exactly `{Δ = 0}`; class hypothesis is tight, not incidental |
| 3 | Starter C5.5 (SMC mission) | `1+F4(ρ)/2` L-RHS over-certifies by exactly 1 (would give `γ_L = 2`; identity baseline has `L = 5`, true sharp `γ_L = 1`) | **W5-SMC-2** corrected set `γ_L ≤ F(2λ)/2 − (2+h+q+c)`, proved exact and sharp |
| 4 | Corner-`δ*` scalarization cone (RACE mission sketch) | Invalid above `ρ₀ = 9−√129/3 ≈ 5.2141`: at `(6,3)`, `w = (1/100,10,31/10)` inside the corner cone yet vertex `(1,3/10)` gives `w·(b−a) = −201/500 < 0` (one-sided defeaters carry distortion `ê_L < δ*`) | **W5-RACE-2** two-sided `δ*` + one-sided `ê` closed forms; repaired cones `C_suff ⊆ C_suff⁺ ⊆ C*` valid at every gauge; maximality via vertex LP |
| 5 | OPAQUE-DUAL two-demand dominance | `L = 11/2 > 5 = max_ρ F2(ρ)/2` — fails latency dominance for **all** ρ (second expand's `1/2` latency; `γ_L = −1/2` at (40,20)) | **W5-MDC-NECESSITY**: residual rank 1 is necessary; **PARITY-DUAL** (one expand) is the unique dominating dual shape; alternate fused-round-trip convention gives weak dominance `(4,0,0)` only at the collapsed front (quarantined) |
| 6 | `ρ_kill(n) ↓ 10` (mission sketch) | Zero-message L-witness `L_b = 1+ρ/4 < 4` for `ρ < 12`, every n | **W5-LPP-KILL**: exact law `ρ_kill = 4/e_anti (3≤n≤7)`, `12 (n≥8)`; corridor `[12, 10·log₂x₁)` non-vacuous (`x₁ > 2`) |
| 7 | `ρ_cert(3)` finiteness | `Ψ↓_{3,t} < 8` for every finite `t` (log term strictly positive) | **W5-LPP-CERT(0)**: certificate vacuous at n=3; exact DP floor closes `ρ⋆(3) = 135/8` |
| 8 | "Run Q4 twice" reduction (RQ4) | EDC-TWICE `(8,0,13/2)` batch / `(12,0,13/2)` seq fails dominance at (40,20); sketch arithmetic slip `13` vs true `12` corrected (stronger strawman) | **W5-MDC-INTERACTION**: handle amortization `2h`/`3h` + residual-rank `1+q`, diffs `1+2h+q`, `1+3h+q > 0` always; batch-m parity rate `= 2 ∀m` |
| 9 | HV-only promotion gates | Gemini counterexample (W4-PEER-REPAIRS): `ΔHV > 0` without Pareto dominance | All Wave-5 gates componentwise (margins or explicit cones); **W5-RACE-1** completeness holds only via componentwise collapse |

**Open fragments (honest, tagged):** exact general-n no-recovery frontier for `n ≥ 5` inside
`[ρ_kill, ρ_cert)` (finite-block DP — BE territory); `e_anti` all-n monotonicity (EC to 101);
`ρ_cert(n)` monotonicity (certified 4..12, limit proved); adaptive-demand AOT extension and
adaptive private state (SB); policy-level uniqueness under a leakage functional `I ≤ κ` (SB,
natural W6 splice DLU × AOT); parity-family namespace feasibility `2^{n−1} ≤ N_τ(h)` (open
combinatorics); classification of binding codes along `F_{n,↓}` for `n ≥ 5` (n=3 binds at the
variable-length `ℓ = 15/8` code, n=4 at the antipodal code).

---

## 7. Strengthened certificates for selected Wave-4 IDs

Exact-arithmetic harness (`W5_COMP_CERTIFICATES.md`: C++ `__int128` DP + pure-Python Fraction
DP + enumerator; 74/74 assertions PASS, 0 FAIL). Orchestrator independently re-derived every
load-bearing number in a second implementation (§9).

| W4 ID | Strengthening delivered |
|-------|-------------------------|
| W4-DP-Q4 | Split-comparison count **21,457,825 reproduced exactly** and explained in closed form `Σ_{k=1}^{16} C(16,k)(2^{k−1}−1)`; dual implementation (C++ + pure Python) agreement on all runs |
| W4-FLOOR-Q4-CAP | All supported pairs `(0,80),(16,48),(32,28),(64,0)` and floors `F(10)=7, F(16)=44/5, F(40/3)=8, F(120/7)=9, F(160/7)=10, F(40)=10` reproduced; envelope completeness at all intersections (concavity + endpoint-equality argument) |
| W4-FLOOR-Q4-DOWN | Pairs `(0,40),(16,22),(32,12),(64,0)` reproduced; `F(40)=10`; the lopsided one-bit realization (weighted-Hamming `2x1+x2+x3+x4 ≷ 2/3`) independently confirmed optimal among all 120 one-bit codebooks |
| W4-FLOOR-Q4-UNIFORM | Pairs `(0,32),(16,20),(32,12),(42,8),(64,0)` reproduced; the `(42,8)` code certified **variable-length** (`ℓ = 21/8`) — not a fixed-depth code; `F(20) = 39/4`, `F(22) = 10` reproduced |
| W4-FLOOR-Q3-DOWN | Pairs `(0,60),(8,30),(15,16),(24,0)` reproduced; `(15,16)` certified variable-length (`ℓ = 15/8`); `F(40)=8` |
| W4-Q5-ANTIPODAL | All 496 two-prototype codebooks re-enumerated; exact min weighted distortion 242 (`e = 121/400`) confirmed; **extended: antipodal optimality now EC-certified for n = 3..8** (minima 30, 88, 242, 600, 1450, 3440) and **proved for all n** (W5-LPP-OPT) |
| W4-Qn-SEPARABLE | `Ψ↓_{4,40} > 8 ⟺ 257·17³ = 1,262,641 < 2²¹ = 2,097,152` reproduced; certification scheme generalized to dyadic-grid big-integer brackets (W5-LPP-CERT) |
| W4-PHASE-Q4-H | Both h-curves reproduced at 200 rational h each via the W5 max-formula `ρ⋆ = max_j(T−2−2ℓ_j)/e_j` (W5-LPP-PRODUCT), including breakpoints and `+∞` regions |
| W4-GEO-Q4 | Identity-collapse margins `(5,0,1)` re-verified against all four supported vertices; strengthened to the full positive orthant (W5-RACE-1) |
| W4-ALIAS-CAPACITY | n=8 mask-tie phenomenon: optimal one-bit family grows at `8|n` (1024 = 8·2⁷ pairs; masks of weight ≥ n−1) — classified exactly (W5-LPP-OPT(iii), EC at n=8) |

**Zero Wave-4 discrepancies were found.** Wave-4's frozen core and append-only revision stand
as the working base in full.

---

## 8. Bead blueprints (YAML, PROVED-only new + still-valid W4)

```yaml
bead_freeze:
  freeze_id: "RADC-W5-KIMI-20260727-v1"
  supersedes: "RADC-W4-SOLPRO-20260727-v2"
  mode: "APPEND_ONLY"
  admission_rule: "PROVED_ONLY"
  excluded_statuses:
    - "CONJECTURE"
    - "BOUNDED_EXPERIMENT_AS_THEOREM"
    - "SPECULATIVE_BRIDGE"
    - "PRODUCTION_UNMAPPED"
    - "RELAXED_FLOOR_MISLABELED_EXACT"
  forbidden_promotions:
    - "two-demand parity dominance presented as production TokenZero claim"
    - "rho_kill limit presented as 10 (true: 12 for n>=8)"
    - "C5.5 L-inequality with the spurious +1"
    - "corner-delta* scalarization cone presented as valid at all gauges"
    - "OPAQUE-DUAL presented as two-demand dominator (fails latency for all rho)"
    - "literal EDC uniqueness presented as true (ledger uniqueness is the theorem)"
    - "antipodal optimality cited as enumeration-only (it is now proved for all n)"
  frozen_theorems_new:
    - "W5-DLU-1"
    - "W5-DLU-0"
    - "W5-DLU-STRUCT"
    - "W5-DLU-RADIUS"
    - "W5-LPP-ANTI"
    - "W5-LPP-SYM"
    - "W5-LPP-OPT"
    - "W5-LPP-KILL"
    - "W5-LPP-CERT"
    - "W5-LPP-PRODUCT"
    - "W5-AOT-1"
    - "W5-AOT-2"
    - "W5-AOT-3"
    - "W5-AOT-4"
    - "W5-AOT-5"
    - "W5-AOT-6"
    - "W5-MDC-FLOOR"
    - "W5-MDC-MONO"
    - "W5-MDC-RATE"
    - "W5-MDC-BATCH"
    - "W5-MDC-SEQ"
    - "W5-MDC-INTERACTION"
    - "W5-MDC-NECESSITY"
    - "W5-SMC-1"
    - "W5-SMC-2"
    - "W5-SMC-3"
    - "W5-SMC-4"
    - "W5-RACE-1"
    - "W5-RACE-2"
  frozen_theorems_still_valid_w4:
    - "W4-DP-Q4"
    - "W4-FLOOR-Q4-CAP"
    - "W4-FLOOR-Q4-DOWN"
    - "W4-FLOOR-Q4-UNIFORM"
    - "W4-FLOOR-Q3-DOWN"
    - "W4-PHASE-MASTER"
    - "W4-PHASE-Q4-H"
    - "W4-AFF-Q4-40"
    - "W4-AFF-Q4-EXPANDED"
    - "W4-ZE-GORDIAN"
    - "W4-Qn-FANO"
    - "W4-Qn-SEPARABLE"
    - "W4-Qn-3PLUS"
    - "W4-Q5-ANTIPODAL"
    - "W4-DETERMINATION-FLOOR"
    - "W4-EXTREMAL-NONUNIQUE"
    - "W4-EXTREMAL-KILL"
    - "W4-LINEAR-ALIAS-RANK"
    - "W4-BATCH-PARITY-KILL"
    - "W4-ALIAS-CAPACITY"
    - "W4-PROD-CORRIDOR-DELTA"
    - "W4-NO-PENALTY-ROBUST"
    - "W4-NEG-NR-n"
    - "W4-DA-RATE"
    - "W4-DIRECT-HASH-KILL"
    - "W4-OPAQUE-CAS-ALIAS"
    - "W4-GEO-Q4"
    - "W4-PEER-REPAIRS"
  beads:
    - bead_title: "certificate: double-ledger uniqueness checker"
      type: "task"
      priority: 0
      status: "FROZEN"
      theorem_ref: ["W5-DLU-1", "W5-DLU-0", "W5-DLU-STRUCT", "W5-DLU-RADIUS"]
      proof_artifact: "W5_DLU.md §§1-7"
      acceptance:
        - "verify slack decomposition M-m*=2l+(r-1)+q(e-1), L-l*=l+(r-1)+(q+c1)(e-1) in exact rationals"
        - "verify 3+2h+q=5 with h>=1,q>=0 has unique solution (1,0)"
        - "verify a.s. forcing: Y>=1 a.s. and EY=1 implies Y=1 a.s."
        - "verify deterministic leak cap n-1 with complement-pair uniqueness by exhaustive fiber check n=2..5"
        - "verify eps-complement channel I=(3/4)log2(3)>1 via integer comparison 27>16"
        - "verify Delta>0 repricing margins Delta(1+q), Delta(1+q+c1); Theta4cap values 24/5, 37/10"
      non_goals:
        - "claiming literal policy uniqueness"
        - "claiming uniqueness up to handle bijection"
    - bead_title: "certificate: closed-form phase algebra engine"
      type: "task"
      priority: 0
      status: "FROZEN"
      theorem_ref: ["W5-LPP-ANTI", "W5-LPP-SYM", "W5-LPP-OPT", "W5-LPP-KILL", "W5-LPP-CERT", "W5-LPP-PRODUCT"]
      proof_artifact: "W5_LPP.md"
      acceptance:
        - "reproduce e_anti(n) exact rationals n=3..20 from the binomial formula"
        - "verify B(n) <= (3n-8)*exp(-(n+4)^2/(32(n-1))) numerically to n=1000 (BE tag)"
        - "verify the max-identity E|Y+wR| = E max(|Y|,|w|) and the support reduction on 1000 random pairs per n in {3..7}"
        - "enumerate all one-bit codebooks n=3..6 (28/120/496/2016 pairs): minima 1/4, 11/40, 121/400, 5/16; complements only"
        - "verify the 8|n dichotomy on support classification n<=60; n=8 has 1024 optimal pairs"
        - "verify rho_kill law: e_anti(7)=145/448<1/3, e_anti(8)=43/128>1/3, e_anti>1/3 for n=8..101; series certificate e^(13/4)>149/16 at n=102"
        - "verify rho_cert brackets n=4..12 by big-integer comparisons on the dyadic grid; rho_cert(3)=+inf by the log-positivity argument"
        - "verify rho* = max_j (T-2-2l_j)/e_j reproduces 135/8, 160/11, 40/3, 64/5 and both h-curves at 200 rational h"
      non_goals:
        - "claiming exact general-n floors for n>=5 inside the corridor"
        - "floating-point threshold inference"
    - bead_title: "certificate: opacity algebra of the two-level alias"
      type: "feature"
      priority: 0
      status: "FROZEN"
      theorem_ref: ["W5-AOT-1", "W5-AOT-2", "W5-AOT-3", "W5-AOT-4", "W5-AOT-5", "W5-AOT-6"]
      proof_artifact: "W5_AOT.md"
      acceptance:
        - "verify I(X;A)=0 by exact cell factorization (n=1,N=3,K=2: 24 cells)"
        - "verify post-expand I=H(X_S)=1 and batch I=dim pi_Q(ker A) by kernel enumeration"
        - "verify H(Pi)>=n floor: build any opaque exact mechanism without private state and show failure"
        - "verify birthday bound K(K-1)/(2N) against exact 1-(N)_K/N^K at three (N,K)"
        - "verify re-issue uniformity over injective tuples by enumeration (N=4,K=3)"
        - "verify content tie-break strict witness I = 8/3-(5log2 5+7log2 7)/12 <= 2/3 by integer cross-multiplication"
      non_goals:
        - "declaring a visible content hash opaque"
        - "production tokenizer promotion"
    - bead_title: "certificate: two-demand exact floors and parity dominance"
      type: "task"
      priority: 0
      status: "FROZEN"
      theorem_ref: ["W5-MDC-FLOOR", "W5-MDC-MONO", "W5-MDC-RATE", "W5-MDC-BATCH", "W5-MDC-SEQ", "W5-MDC-INTERACTION", "W5-MDC-NECESSITY"]
      proof_artifact: "W5_MDC.md"
      acceptance:
        - "implement the two-demand leaf E2(A)=min_p sum_x [1-(sum_s theta_s 1[p_s=x_s])^2] in scaled integers"
        - "reproduce F2_batch down breakpoints 80/9, 400/37, 400/27 and cap breakpoints 10, 800/71, 1600/123 with breakpoint tie-equalities"
        - "reproduce F2_batch(40)=10 (both vertices), G2(40)=15, H2(40)=10"
        - "verify the mono lemma e2>=e pointwise implies the t=40 collapse without DP"
        - "verify PARITY-DUAL ledgers (5,0,4) batch and (8,0,4) seq; margins (5,0,1) and (7,0,1)"
        - "verify thresholds 150/17 and 1200/137 against the exact envelopes"
        - "verify any 2-expand policy pays L>=11/2>5 at the registered instance"
        - "verify interaction diffs 1+2h+q (batch), 1+3h+q (seq)"
      non_goals:
        - "presenting OPAQUE-DUAL as a full dominator (M-dominant only, Convention A)"
        - "transferring singleton rates to batch without the residual-rank correction"
    - bead_title: "promotion gate: measured-cost corridor with exact margins"
      type: "feature"
      priority: 0
      status: "FROZEN"
      theorem_ref: ["W5-SMC-1", "W5-SMC-2", "W5-SMC-3", "W5-SMC-4"]
      proof_artifact: "W5_SMC_RACE.md §§SMC"
      acceptance:
        - "require the D-gate: exact costed fallback or p_miss=0 before any D=0 claim"
        - "use L-RHS F(2*lambda)/2, never 1+F(rho)/2"
        - "compute T_H=max(A_M,2*A_L); verify the latency-bound lemma 2*A_L-A_M=1+q+2c>0"
        - "reproduce rho*(h,q,c) piecewise in s=h+q+c and the s=3 infeasibility boundary"
        - "reproduce the 5-row measured table verdicts at (40,20) cap"
      non_goals:
        - "production dominance without measured h_tau, q_tau, c_tau, Delta, p_miss"
    - bead_title: "frontier witness: scalarization equilibrium cones"
      type: "feature"
      priority: 1
      status: "FROZEN"
      theorem_ref: ["W5-RACE-1", "W5-RACE-2"]
      proof_artifact: "W5_SMC_RACE.md §§RACE"
      acceptance:
        - "at collapsed gauges verify w.(b-a) >= w_M*gamma_M + w_L*gamma_L > 0 at all supported vertices"
        - "compute C* as the <=4 vertex half-space intersection"
        - "reproduce delta* and e-hat closed forms on the linked slice via the per-edge LP"
        - "verify the repaired cones on a 135-gauge grid plus 300 just-inside weight points"
        - "reject the corner-delta* single-inequality cone above rho_0 = 9-sqrt(129)/3"
      non_goals:
        - "HV-only promotion gates"
        - "weighted-sum-only frontier certificates at non-collapsed gauges"
```

---

## 9. Replication brief (exact checkers / finite DP / rank args)

**What was run, where, and how to re-run it.** All computation was exact integer/rational
arithmetic. Three independent implementation lines agree on every shared number
(agent C++ `__int128` DP; orchestrator C++ DP (built independently from the substrate card,
before any agent result arrived); pure-Python `fractions.Fraction` DPs). Build/run in a
scratch executable directory (`/tmp`), e.g. `g++ -O3 -o dp dp.cpp`.

1. **Wave-4 base reproduction (74/74 PASS).**
   - Exact prefix-tree DP over `2^16` subsets: leaf `E_θ(A) = Σ_i θ_i min{N_i0,N_i1}`, split
     cost `2|A|`, least-element-in-left convention. **Comparison count = 21,457,825** per
     scalar run `= Σ_k C(16,k)(2^{k−1}−1)` (closed form now proved).
   - Reproduce floors: cap `F(10)=7, F(16)=44/5, F(40/3)=8, F(120/7)=9, F(160/7)=10, F(40)=10`;
     down `F(40)=10`; uniform `F(20)=39/4, F(22)=10`; n3-down `F(40)=8`.
   - Supported pairs (all four classes) + envelope completeness at all 13 adjacent-line
     intersections (concavity argument: a concave function equal to a linear function at both
     endpoints of an interval coincides on it).
   - `(42,8)` and `(15,16)` certified variable-length codes.
   - Q5: all 496 codebooks, min weighted distortion 242.
   - Antipodal optimality enumeration n=3..8: minima `30, 88, 242, 600, 1450, 3440`;
     optimal-pair counts `4, 8, 16, 32, 64, 1024`.
   - `257·17³ = 1,262,641 < 2²¹ = 2,097,152`.
2. **New Wave-5 computations.**
   - Two-demand DP: leaf `E2_θ(A) = min_p Σ_{x∈A}[1 − (Σ_s θ_s 1[p_s=x_s])²]` scaled to
     integers (`DEM²` with `DEM = Σ w_s`); supported pairs ↓ `(0,272),(16,182),(32,108),(64,0)`
     at scale `(16ℓ, 400·e2)` and cap `(0,1096),(16,776),(32,492),(64,0)` at `(16ℓ, 1600·e2)`;
     envelopes of §5.4; breakpoint ties at all 9 breakpoints; `F2_batch(40)=10` (both),
     `G2(40)=15`, `H2(40)=10`.
   - `e_anti(n)` exact rationals n=3..20 from the binomial formula (identity (a) verified
     symbolically per n).
   - One-bit optimality: identity `e({p,q}) = (5n − g_n(supp(p⊕q)))/(10n)` on 1000 random
     pairs per n (`n=3..7`); support classification `n ≤ 60` matches the `8|n` dichotomy;
     `E|full| = E|drop-light| = 105/8` at n=8 (orchestrator check).
   - `ρ_cert` brackets: dyadic grid `t = 10n·j/2^14`, big-integer `2^s`-th power comparisons;
     sample n=8 inequality printed in full (196 digits) in `W5_LPP.md`.
   - Cone engine: per-edge exact LP for `δ*`, `ê_M`, `ê_L`; 135-gauge grid + 300 random
     just-inside weights; counterexample `w·(b−a) = −201/500` at `(6,3)`; onset quadratic
     `3ρ² − 54ρ + 200 = 0` ⟹ `ρ₀ = 9 − √129/3`.
3. **Lean-ready cores (specifications, not artifacts):**
   `ledger_slack_decomposition`; `as_forcing (Y≥1 a.s., EY=1 ⟹ Y=1 a.s.)`;
   `rademacher_max_identity (E|Y+wR| = E max(|Y|,|w|))`; `support_reduction`;
   `two_demand_leaf_concavity`; `joint_independence_conditioning (AOT-2 key lemma)`;
   `private_state_entropy_floor (H(Π) ≥ n)`; `latency_bound_lemma (2A_L − A_M = 1+q+2c)`;
   `threshold_max_formula (ρ⋆ = max_j (T−2−2ℓ_j)/e_j)`; exact rational phase identities
   (`135/8, 160/11, 40/3, 64/5, 150/17, 1200/137`).
4. **Promotion rejection rule.** Reject any package that: calls `Φ*`/`Ψ↓` the exact frontier;
   calls a source-dependent hash opaque; ignores `Δ_θ` or `N_τ(h)`; transfers singleton rates
   to batch demand without residual rank; presents the corner-`δ*` cone as gauge-universal;
   quotes `ρ_kill → 10`; uses C5.5's `+1`; presents PARITY/OPAQUE two-demand results as
   production claims; treats bounded enumeration (any n-range) as an unbounded theorem
   (W5-LPP-OPT is exempt — it is proved, not enumerated).

---

## 10. Timestamp + model identity

**Date:** 2026-07-27.
**Model:** Kimi K3, acting as Wave-5 orchestrator; invention/computation sub-agents
(math_inventor × 5: DLU, LPP, AOT, MDC, SMC-RACE; coder × 1: certificate harness);
independent orchestrator exact-arithmetic cross-checks (second C++ DP + Python Fraction DPs)
built from the substrate card before agent results arrived.
**Run type:** prove-first splice campaign; parallel background invention agents with file
deliverables; stage-gated orchestrator refereeing of every proof; three-way cross-validation
of every load-bearing number; PROVED-only bead freeze.
**Files covered:** 00 campaign + 01 splice brief + 01b readme + 02 Wave-4 Sol Pro full package
(frozen merge + append-only revision, read in full) + 03 Kimi W3 + 04 Claude W3 (historical) +
05 adjacent-methods brief (method transfer only) + racc-public + RACC_RESEARCH_DISTILL +
99 user messages; the concat file adds nothing beyond these.
**Fable note:** no Fable Wave-3 content exists (blocked); nothing is inferred from it.
**Method-transfer note:** the adjacent-math brief was used as workflow only (bound → extremal
→ counterexample → infinite family/phase → obstruction map; proof-status tags; stronger
quantifier, shorter proof). No exterior domain math was imported.
**Wall-clock statement:** no unverifiable elapsed-duration claim is made; the ≥8h preference is
addressed by depth (six theorem families, 29 new PROVED IDs, 74 exact harness assertions plus
   dozens of theorem-level EC certificates), not by
an attestation.
**Final classification:** six new affirmative theorem families with proofs and exact
certificates — ledger uniqueness (DLU), closed-form phase algebra incl. all-n antipodal
optimality (LPP), opacity algebra with factorization converse (AOT), two-demand dominance with
free second demand and necessity certificate (MDC), parametric margin corridor with corrected
constants (SMC), and scalarization equilibrium cones (RACE) — over an untouched,
74-assertion-reverified Wave-4 base. Effort ≈ 66% prove / 24% strengthen / 10% targeted
obstruction. Campaign acceptance met: ≥3 new affirmative theorem IDs (29 delivered),
≥1 closed form/uniqueness (both delivered), bead freeze PROVED-only, no production claims,
no security content.
