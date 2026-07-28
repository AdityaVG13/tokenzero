# RADC Wave-5 Continuation 2 (Sol Pro merge — full-prefix claim closure)

## 0. Executive verdict

The remaining Q4 multi-demand claim is now closed.

Let

\[
\Theta_4^{\downarrow}
=
\left\{\theta\in\Delta_3:\theta_i\ge\frac15\right\},
\qquad
\Theta_4^{\mathrm{cap}}
=
\left\{\theta\in\Delta_3:\frac15\le\theta_i\le\frac3{10}\right\}.
\]

At the registered sequential gauge

\[
(\rho_{\mathrm{fail}},\lambda_{\mathrm{fail}})=(40,20),
\]

the parity/complement recovery policy has

\[
(M,D,L)=(3m+2,0,4)
\]

for \(m\) sequential demands.

### Closed theorem

\[
\boxed{
\begin{array}{l}
\text{For every }\theta\in\Theta_4^{\downarrow}
\text{ or }\Theta_4^{\mathrm{cap}},\\[0.2em]
\text{the parity policy strictly dominates the complete randomized}\\
\text{variable-length no-recovery prefix hull iff }1\le m\le18.
\end{array}}
\]

- For \(1\le m\le9\), this is the previously frozen one-demand-floor reduction.
- For \(10\le m\le18\), the new proof covers **every nontrivial prefix tree over the full demand polytopes**, not only the three point laws and not only the no-message face.
- For every \(m\ge19\), a no-message baseline has strictly smaller \(M\), so dominance is impossible.

The exact critical demand count is therefore

\[
\boxed{m_{\mathrm{crit}}=18.}
\]

The proof does **not** require the stronger statement that the no-message root is the exact baseline optimizer for every \(10\le m\le18\). That route was unnecessary. A coverage-versus-leaf-count inequality places every nontrivial tree directly above the parity candidate.

At \(m=18\), the sharp uniform \(M\)-margins are

\[
\boxed{
\gamma_M(\Theta_4^{\downarrow})
=
\frac{277615146191}{762939453125}
\approx0.363875724415,
}
\]

\[
\boxed{
\gamma_M(\Theta_4^{\mathrm{cap}})
=
\frac{20074685943080277}{50000000000000000}
\approx0.401493718862.
}
\]

For both classes and every \(1\le m\le18\),

\[
\boxed{\gamma_L=1,\qquad \gamma_D=0.}
\]

The latency margin is exact because the no-recovery identity has \(L=5\).

---

## 1. Effort budget log

- Affirmative theorem construction and proof: **76%**
- Independent exact computation: **20%**
- Targeted obstruction/correction: **4%**

The only negative step was used to identify the exact \(m=19\) obstruction and to avoid proving the stronger, unnecessary root-optimality statement.

---

## 2. Statement lock

Let

\[
X\sim\operatorname{Unif}(\{0,1\}^4),
\qquad
S_1,\ldots,S_m\stackrel{\mathrm{i.i.d.}}{\sim}\theta,
\qquad
S_{1:m}\perp X.
\]

A no-recovery policy emits a source-dependent prefix-free transcript before observing any demand. Conditioned on all policy randomness, the transcript partitions the sixteen source words into \(r\) nonempty leaves

\[
A_1,\ldots,A_r,
\]

with codeword depths \(d_1,\ldots,d_r\). Its expected pre-demand length is

\[
\ell
=
\frac1{16}\sum_{j=1}^r |A_j|d_j.
\]

The decoder answers the demands online. Between demands it receives no correctness oracle and no other source-dependent feedback; its state contains only the pre-demand transcript, the demand history, its own outputs, and policy randomness. Its joint success and failure probabilities are

\[
P_T
=
\Pr[\widehat X_{S_t}=X_{S_t}\text{ for every }t\le m],
\qquad
 e_T=1-P_T.
\]

The sequential no-recovery ledger is

\[
M_T=(m+1)(1+\ell)+40e_T,
\]

\[
L_T=1+\ell+c_{\mathrm{comp}}+20e_T,
\qquad c_{\mathrm{comp}}\ge0,
\]

and \(D_T=e_T\).

The parity candidate has

\[
M_{\mathrm{par}}=3m+2,
\qquad
L_{\mathrm{par}}=4,
\qquad
D_{\mathrm{par}}=0.
\]

All deterministic statements below extend to randomized prefix policies by conditioning on all encoder/decoder randomness and averaging, because \(M\), \(L\), and success probability are affine under mixtures.

---

## 3. New theorem index

| ID | Statement | Status | Ambition |
|---|---|---|---|
| W5-SOL-COVERAGE-LEAF | Full-coordinate coverage limits a prefix leaf to one successful source | DR | [F] |
| W5-SOL-Q4-LENGTH-SPECTRUM | Exact minimum external path sum by leaf count for sixteen equiprobable sources | DR + EC | [S] |
| W5-SOL-Q4-NONTRIVIAL-BARRIER | Every nontrivial Q4 prefix tree has \(M-M_{\rm par}\ge1\) for \(10\le m\le18\) | DR + EC | [F] |
| W5-SOL-Q4-NOMSG-REPAIR | Exact no-message monotonicity and endpoint margins | DR + EC | [S] |
| W5-SOL-MDC-Q4-FULL-18/19 | Complete full-polytope, full-prefix demand-count phase \(m\le18\) versus \(m\ge19\) | DR + EC | [M] |

---

## 4. W5-SOL-COVERAGE-LEAF — coverage–leaf transversality

Let

\[
\mathcal C_m
=
\{\{S_1,\ldots,S_m\}=[4]\}
\]

be the event that all four coordinates have appeared, and let

\[
p_{\mathrm{cov}}(\theta,m)=\Pr_\theta(\mathcal C_m).
\]

### Theorem

For every deterministic no-recovery prefix policy with \(r\) nonempty transcript leaves,

\[
\boxed{
P_T
\le
1-p_{\mathrm{cov}}(\theta,m)
\left(1-\frac r{16}\right).
}
\]

The same inequality holds conditionally on every realization of policy randomness.

### Proof

Fix a transcript leaf \(A_j\), a complete demand sequence \(s_{1:m}\), and all decoder randomness. The online decoder's answer sequence is then fixed.

On \(\mathcal C_m\), every source coordinate occurs at least once. Therefore at most one source word \(x\in A_j\) can agree with all emitted answers. If repeated demands receive inconsistent answers, no source word agrees; otherwise the first emitted answer for each coordinate specifies a unique four-bit word.

Since \(X\) is uniform, the total source-averaged success contribution of leaf \(A_j\), conditional on \(\mathcal C_m\), is at most \(1/16\). Summing over \(r\) leaves gives

\[
\Pr[\text{success}\mid\mathcal C_m]\le\frac r{16}.
\]

On \(\mathcal C_m^c\), success is at most one. Hence

\[
P_T
\le
(1-p_{\mathrm{cov}})\cdot1
+p_{\mathrm{cov}}\frac r{16},
\]

which is the claimed inequality. ∎

### Interpretation

This is not a product bound. It couples:

1. the occupancy geometry of the demand process;
2. the number of source fibers retained by the prefix transcript;
3. the joint-decision success event.

A leaf can conceal many source words while only a strict subset of coordinates is demanded. Once every coordinate is visited, that multiplicity collapses to at most one successful source per leaf.

---

## 5. W5-SOL-Q4-LENGTH-SPECTRUM — exact leaf-count cost

Let

\[
L_{\mathrm{ext}}
=
16\ell
=
\sum_{j=1}^r |A_j|d_j
\]

be the unnormalized external path length.

### Lemma

For sixteen equiprobable source states,

\[
\boxed{
L_{\mathrm{ext}}
\ge
c_r,
}
\]

where

\[
\begin{array}{c|ccccc}
r&2&3&4&5&6\\ \hline
c_r&16&18&21&24&28
\end{array}
\]

and

\[
\boxed{L_{\mathrm{ext}}\ge32\quad\text{for every }r\ge7.}
\]

The complete exact spectrum is

\[
\boxed{
C_{16}(r)
=
(0,16,18,21,24,28,32,36,40,45,50,53,56,60,62,64)
}
\]

for \(r=1,\ldots,16\).

### Proof of the values needed for the theorem

Let \(k_j=|A_j|\ge1\), so \(\sum_jk_j=16\), and let

\[
d_{\min}=\min_jd_j.
\]

For fixed codeword depths,

\[
\begin{aligned}
L_{\mathrm{ext}}
&=
\sum_jd_j+
\sum_j(k_j-1)d_j\\
&\ge
\sum_jd_j+(16-r)d_{\min}.
\end{aligned}
\]

If \(d_{\min}\ge2\), then every source has depth at least two and

\[
L_{\mathrm{ext}}\ge32.
\]

Suppose \(d_{\min}=1\). For \(r\ge3\), exactly one codeword can have depth one. Removing the common first bit from the remaining \(r-1\) codewords leaves a binary prefix code with relative depths \(e_1,\ldots,e_{r-1}\). Therefore

\[
L_{\mathrm{ext}}
\ge
16+\sum_{j=1}^{r-1}e_j.
\]

For \(k\) equally weighted binary codewords, the minimum unweighted external path sum is

\[
U_k
=
 k(a+2)-2^{a+1},
\qquad
 a=\lfloor\log_2k\rfloor.
\]

In particular,

\[
U_2=2,
\quad
U_3=5,
\quad
U_4=8,
\quad
U_5=12,
\quad
U_6=16.
\]

Thus

\[
16+U_{r-1}
=
18,21,24,28,32
\]

for \(r=3,4,5,6,7\), while \(r=2\) gives \(L_{\mathrm{ext}}\ge16\) directly. Since \(U_k\) is nondecreasing, every \(r\ge7\) has \(L_{\mathrm{ext}}\ge32\). ∎

An independent subset-split dynamic program reproduced the complete sixteen-entry spectrum above.

---

## 6. Uniform demand-coverage floor

Every demand law in \(\Theta_4^{\downarrow}\), and hence every law in \(\Theta_4^{\mathrm{cap}}\), satisfies \(\theta_i\ge1/5\).

By the union bound,

\[
\begin{aligned}
p_{\mathrm{cov}}(\theta,m)
&\ge
1-\sum_{i=1}^4(1-\theta_i)^m.
\end{aligned}
\]

For \(m\ge2\), the function \(x\mapsto(1-x)^m\) is convex. Therefore its sum is maximized over \(\Theta_4^{\downarrow}\) at a vertex, namely a permutation of

\[
\left(\frac25,\frac15,\frac15,\frac15\right).
\]

Consequently,

\[
\boxed{
p_{\mathrm{cov}}(\theta,m)
\ge
p_m
:=
1-
\left(\frac35\right)^m
-3\left(\frac45\right)^m.
}
\]

For \(m\ge10\),

\[
p_m\ge p_{10}
=
\boxed{
\frac{6560848}{9765625}
}
\approx0.6718308352.
\]

---

## 7. W5-SOL-Q4-NONTRIVIAL-BARRIER — every nontrivial tree is above parity

For a deterministic prefix tree \(T\), define its memory gap over parity by

\[
\Gamma_T
=
M_T-M_{\mathrm{par}}.
\]

Since \(e_T=1-P_T\),

\[
\Gamma_T
=
39-2m+(m+1)\ell-40P_T.
\]

Using coverage–leaf transversality,

\[
\boxed{
\Gamma_T
\ge
(m+1)\ell-(2m+1)
+40p_{\mathrm{cov}}(\theta,m)
\left(1-\frac r{16}\right).
}
\]

### Trees with \(r\ge7\)

Here \(\ell\ge2\). The trivial bound \(P_T\le1\) already gives

\[
\Gamma_T
\ge
39-2m+2(m+1)-40
=
\boxed{1}.
\]

### Trees with \(2\le r\le6\)

Use \(\ell\ge c_r/16\), \(p_{\mathrm{cov}}\ge p_m\ge p_{10}\), and \(10\le m\le18\). Since \(c_r/16<2\),

\[
(m+1)\frac{c_r}{16}-(2m+1)
\]

is decreasing in \(m\), so its minimum in this interval occurs at \(m=18\). Therefore

\[
\Gamma_T
\ge
B_r
:=
\frac{19c_r}{16}-37
+40p_{10}\left(1-\frac r{16}\right).
\]

The exact values are

\[
\begin{array}{c|c|c}
r&B_r&\text{decimal}\\ \hline
2&\dfrac{10769686}{1953125}&5.514079232000\\[0.8em]
3&\dfrac{97023471}{15625000}&6.209502144000\\[0.8em]
4&\dfrac{252888283}{31250000}&8.092425056000\\[0.8em]
5&\dfrac{38966203}{3906250}&9.975347968000\\[0.8em]
6&\dfrac{20384017}{1562500}&13.045770880000
\end{array}
\]

Every value is greater than one.

### Conclusion

For every deterministic nontrivial tree, every

\[
\theta\in\Theta_4^{\downarrow}
\quad\text{or}\quad
\Theta_4^{\mathrm{cap}},
\]

and every

\[
10\le m\le18,
\]

\[
\boxed{
M_T-M_{\mathrm{par}}\ge1.
}
\]

Conditioning and averaging proves the same statement for every randomized no-recovery policy whose realized transcript is nontrivial.

---

## 8. W5-SOL-Q4-NOMSG-REPAIR — exact no-message face

Continuation 1 correctly identified the no-message occupancy polynomial, but the sentence “\(P_{0,m}\) decreases, hence the margin is positive for all \(m\le18\)” was not by itself sufficient because the parity ledger also changes with \(m\). The following closes that logical gap.

### Exact optimal no-message success

With no source-dependent transcript, fix a demand sequence containing \(k\) distinct coordinates. Any internally consistent answer strategy specifies one bit for each visited coordinate and succeeds with probability exactly \(2^{-k}\); an inconsistent strategy succeeds with probability zero. Hence a fixed prototype is optimal and

\[
P_{0,m}(\theta)
=
\mathbb E_\theta[2^{-|Q_m|}]
=
2^{-4}\sum_{B\subseteq[4]}\theta(B)^m.
\]

This function is symmetric and convex, hence Schur-convex. Its maximum over \(\Theta_4^{\downarrow}\) occurs at

\[
\theta^{\downarrow}
=
\left(\frac25,\frac15,\frac15,\frac15\right),
\]

and its maximum over \(\Theta_4^{\mathrm{cap}}\) occurs at

\[
\theta^{\mathrm{cap}}
=
\left(\frac3{10},\frac3{10},\frac15,\frac15\right),
\]

up to permutation.

The no-message gap is

\[
\gamma_{0,m}(\theta)
=
39-2m-40P_{0,m}(\theta).
\]

### Monotonicity on the remaining strip

Let \(B\subseteq[4]\) be uniformly random and put \(Z=\theta(B)\in[0,1]\). Then

\[
P_{0,m}-P_{0,m+1}
=
\mathbb E[Z^m(1-Z)].
\]

Calculus gives

\[
\max_{0\le z\le1}z^m(1-z)
=
\frac{m^m}{(m+1)^{m+1}}.
\]

For each integer \(m=10,\ldots,17\), the exact integer certificate

\[
20m^m<(m+1)^{m+1}
\]

holds. Therefore

\[
40(P_{0,m}-P_{0,m+1})<2
\]

and

\[
\boxed{
\gamma_{0,m}>\gamma_{0,m+1}
\qquad(10\le m\le17).
}
\]

Thus the smallest no-message margin on \(10\le m\le18\) occurs at \(m=18\).

### Exact endpoint margins

For the lower-capped class,

\[
\boxed{
\gamma_{0,18}^{\downarrow}
=
\frac{277615146191}{762939453125}
>0.
}
\]

For the original capped class,

\[
\boxed{
\gamma_{0,18}^{\mathrm{cap}}
=
\frac{20074685943080277}{50000000000000000}
>0.
}
\]

At \(m=17\),

\[
\gamma_{0,17}^{\downarrow}
=
\frac{71088276063}{30517578125}
>1,
\]

\[
\gamma_{0,17}^{\mathrm{cap}}
=
\frac{475055717444931}{200000000000000}
>1.
\]

Hence:

- every no-message policy has \(M\)-gap greater than one for \(10\le m\le17\);
- at \(m=18\), the class-extreme no-message policies are the unique tight objective face below the nontrivial-tree barrier of one.

---

## 9. W5-SOL-MDC-Q4-FULL-18/19 — exact complete phase

### Theorem

For either

\[
\Theta=\Theta_4^{\downarrow}
\quad\text{or}\quad
\Theta_4^{\mathrm{cap}},
\]

at \((\rho,\lambda)=(40,20)\), the sequential parity policy

\[
(3m+2,0,4)
\]

strictly multi-objective-dominates every randomized variable-length no-recovery prefix policy uniformly over \(\Theta\) iff

\[
\boxed{1\le m\le18.}
\]

### Proof: \(1\le m\le9\)

For any baseline, joint failure contains first-answer failure. Therefore

\[
M_T
\ge
\frac{m+1}{2}
F_\Theta\!\left(\frac{80}{m+1}\right),
\]

where \(F_\Theta\) is the exact one-demand Q4 floor. The previously frozen exact floors give the following certified \(M\)-margins:

\[
\begin{array}{c|ccccccccc}
m&1&2&3&4&5&6&7&8&9\\ \hline
\Theta_4^{\downarrow}&5&7&7&7&6&5&4&3&1\\
\Theta_4^{\mathrm{cap}}&5&7&8&8&7&6&5&3&1
\end{array}
\]

All are positive.

### Proof: \(10\le m\le18\)

Condition on all policy randomness.

- If the realized transcript has one leaf, §8 gives a positive no-message gap. It is greater than one through \(m=17\), and at \(m=18\) equals the exact class margin displayed above.
- If it has at least two leaves, §7 gives
  \[
  M_T-M_{\mathrm{par}}\ge1.
  \]

Averaging preserves the inequality. Hence every randomized prefix policy has strictly larger \(M\).

For latency, let \(e_1\) be the first-answer error induced by the same transcript and first-step decoder. Since joint failure contains first-answer failure,

\[
e_T\ge e_1.
\]

The exact one-demand Q4 floor at \(t=40\) is \(F_\Theta(40)=10\). Therefore

\[
2+2\ell+40e_1\ge10,
\]

and

\[
L_T
=
1+\ell+c_{\mathrm{comp}}+20e_T
\ge5.
\]

Thus

\[
L_T-L_{\mathrm{par}}\ge1.
\]

The distortion inequality is immediate because \(D_{\mathrm{par}}=0\le e_T\).

### Proof: failure for every \(m\ge19\)

A no-message fixed-prototype policy succeeds whenever \(X\) equals that prototype, an event of probability \(1/16\). Hence

\[
e_0\le\frac{15}{16}.
\]

Its memory cost satisfies

\[
M_0
\le
m+1+40\frac{15}{16}
=
m+\frac{77}{2}.
\]

Therefore

\[
M_0-M_{\mathrm{par}}
\le
\frac{73}{2}-2m.
\]

At \(m=19\), this is \(-3/2\), and it decreases thereafter. Thus a valid no-recovery baseline has smaller \(M\) for every \(m\ge19\), so parity cannot dominate the full hull. ∎

---

## 10. Exact margins and phase table

### Uniform margins over the complete hull

| Demand count | \(\Theta_4^{\downarrow}\): certified \(\gamma_M\) | \(\Theta_4^{\mathrm{cap}}\): certified \(\gamma_M\) | \(\gamma_L\) | \(\gamma_D\) |
|---:|---:|---:|---:|---:|
| \(1\le m\le9\) | exact-floor lower bounds in §9 | exact-floor lower bounds in §9 | 1 | 0 |
| \(10\le m\le17\) | \(\ge1\) | \(\ge1\) | 1 | 0 |
| \(m=18\) | \(277615146191/762939453125\) | \(20074685943080277/50000000000000000\) | 1 | 0 |
| \(m\ge19\) | dominance fails | dominance fails | — | — |

At \(m=18\), the displayed \(M\)-margins are sharp because the corresponding class-extreme no-message baselines attain them, while every nontrivial transcript has gap at least one.

### Exact demand-count phase

\[
\boxed{
\mathcal D_{\mathrm{parity}}^{\mathrm{seq}}
=
\{m\in\mathbb N:m\le18\}.
}
\]

This is now a full-demand-polytope and full-prefix-hull statement, not a vertex-only or no-message-only statement.

---

## 11. Independent EC replications

Two independent exact implementations certify the arithmetic.

### Python exact-rational checker

`W5_FULL_PREFIX_CHECKS.py` verifies:

1. the complete external-path spectrum \(C_{16}(r)\);
2. \(p_{10}=6560848/9765625\);
3. all five exact \(B_r\) lower bounds;
4. exact no-message margins at \(m=17,18\);
5. all eight integer monotonicity certificates;
6. the direct finite-range inequalities for every \((m,r)\in\{10,\ldots,18\}\times\{2,\ldots,6\}\);
7. the \(m=19\) obstruction.

### Portable C++20 checker

`w5_full_prefix_check.cpp` independently reproduces the same quantities using:

- a separate subset-split external-path DP;
- signed 128-bit exact rational arithmetic;
- direct subset-moment enumeration;
- exact integer cross-multiplication.

Both checkers pass without floating-point dependence.

### Exact denominator-20 grid replication

`sol_m_demand_grid.cpp` also reruns the complete adaptive subset-tree DP on all five permutation-orbit representatives of the denominator-20 lower-capped grid,

\[
(8,4,4,4),\ (7,5,4,4),\ (6,6,4,4),\ (6,5,5,4),\ (5,5,5,5),
\]

for every integer \(m=10,\ldots,18\). In all forty-five exact runs, the selected optimum has

\[
L_{\mathrm{ext}}=0,\qquad r=1.
\]

Thus the no-message root is exact on this full rational grid. This grid result is an additional EC replication; the continuum theorem does not depend on it.

---

## 12. Bead blueprints

```yaml
beads:
  - bead_id: W5-SOL-COVERAGE-LEAF
    title: Coverage-leaf transversality
    status: PROVED
    proof_status: [DR]
    ambition: F
    assumptions:
      - "X is uniform on {0,1}^4"
      - "all demands are independent of X"
      - "the no-recovery transcript is fixed before demands"
      - "joint success requires every demanded answer to be correct"
    statement: >-
      If a deterministic transcript has r nonempty source leaves, then
      P_success <= 1 - p_cover*(1-r/16), where p_cover is the probability
      that all four coordinates appear.
    acceptance_tests:
      - "Condition on a covering demand sequence and prove at most one successful source per leaf."
      - "Average over covering and noncovering demand sequences."
    forbidden_promotions:
      - "Do not apply one-success-per-leaf when the demanded coordinates do not separate the source class."

  - bead_id: W5-SOL-Q4-NONTRIVIAL-BARRIER
    title: Nontrivial prefix-tree barrier for m=10..18
    status: PROVED
    proof_status: [DR, EC]
    ambition: F
    assumptions:
      - "theta_i >= 1/5"
      - "10 <= m <= 18"
      - "rho_fail = 40"
      - "sequential parity ledger M=3m+2"
    statement: >-
      Every no-recovery prefix tree with at least two realized leaves satisfies
      M_tree - M_parity >= 1.
    acceptance_tests:
      - "Reproduce C_16(r) for r=1..16."
      - "Reproduce p_10=6560848/9765625."
      - "Verify every B_r for r=2..6 is greater than one."
      - "Use ell>=2 for r>=7."
    forbidden_promotions:
      - "Do not infer that the no-message tree is the exact optimizer for every interior demand law."

  - bead_id: W5-SOL-MDC-Q4-FULL-18-19
    title: Exact full-prefix Q4 sequential demand-count phase
    status: PROVED
    proof_status: [DR, EC]
    ambition: M
    assumptions:
      - "theta lies in Theta_4_down or Theta_4_cap"
      - "randomized variable-length prefix-free no-recovery class"
      - "joint failure distortion"
      - "registered gauge (rho,lambda)=(40,20)"
    statement: >-
      The sequential parity policy (3m+2,0,4) strictly dominates the complete
      no-recovery hull iff 1<=m<=18.  At m=18 the exact uniform M margins are
      277615146191/762939453125 for Theta_4_down and
      20074685943080277/50000000000000000 for Theta_4_cap.
    acceptance_tests:
      - "Use the frozen one-demand-floor reduction for m<=9."
      - "Use the no-message and nontrivial-tree split for 10<=m<=18."
      - "Verify L>=5 from F_Theta(40)=10."
      - "At m=19 exhibit the no-message M-gap upper bound -3/2."
    forbidden_promotions:
      - "Do not extend the critical count to other n, gauges, or demand lower bounds without re-running the phase inequalities."
```

---

## 13. Failure-log and obstruction-map update

| Previous open route | Resolution | Remaining issue |
|---|---|---|
| Full Q4 prefix hull for \(10\le m\le18\) | **Closed** by coverage–leaf transversality plus exact prefix-length spectrum | None at the registered Q4 gauge |
| Prove every nontrivial tree is above the no-message root | Bypassed as unnecessarily strong | Root optimality may remain open for interior laws, but is irrelevant to parity dominance |
| Full-polytope \(m=18/19\) boundary | **Closed for the complete randomized prefix class** | None for \(\Theta_4^{\downarrow}\) or \(\Theta_4^{\mathrm{cap}}\) |
| Arbitrary \(n\), arbitrary \(m\) full-prefix phase | Not addressed by this finite Q4 closure | Generalize the coverage–leaf spectrum to \(2^n\) sources and optimize the resulting phase |
| Other failure gauges \(\rho,\lambda\) | Registered point only | Derive the full \((m,\rho,\lambda)\) surface |
| Production/tokenizer corridor | Still formal only | Requires measured token, lookup, miss, and cache distributions |

The prior remaining strip

\[
10\le m\le18
\]

has been removed from the North Star distance report.

---

## 14. North Star distance update

Closed in this continuation:

- full demand-polytope Q4 sequential dominance;
- full randomized variable-length prefix hull;
- exact critical demand count \(m_{\mathrm{crit}}=18\);
- sharp \(m=18\) uniform margins;
- exact \(m=19\) obstruction.

Still open beyond this claim:

1. the corresponding complete phase surface in \((n,m,\rho,\lambda,h,q,c)\);
2. arbitrary-\(n\) finite-prefix floors;
3. production corridor mapping.

None of those open items weakens the Q4 theorem proved here.

---

## 15. Confidence

- Coverage–leaf theorem: **0.99**
- Prefix-length spectrum and exact arithmetic: **0.99**
- Nontrivial-tree barrier: **0.98**
- Full \(m\le18\) versus \(m\ge19\) phase: **0.98**

The residual uncertainty is limited to interpretation drift in the campaign's ledger conventions. Under the statement lock in §2, the proof is complete.

---

## 16. Timestamp + model identity

```text
Timestamp: 2026-07-27 08:41:48 PDT (America/Los_Angeles)
Model identity: GPT-5.6 Pro
Continuation identity: RADC Wave-5 Sol Pro merge — append-only continuation 2
```
