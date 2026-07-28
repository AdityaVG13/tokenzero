# RADC Wave-5 Continuation 1 (Sol Pro merge — append-only)

## 0. Executive verdict

This continuation closes the arbitrary-demand-law agency rate-distortion problem on the Information-Separable Capsule substrate and sharpens the Q4 multi-demand obstruction from three point laws to the full cap/down demand classes for the entire no-message face.

New frozen results:

1. **W5-SOL-AGRD-THETA [M] (DR):** for every full-support singleton demand law \(\theta\),
   \[
   R_{\rm ag,\theta}(D)=1-H_2(D),
   \]
   while the exact no-recovery curve has a logistic water-filling parameterization.
2. **W5-SOL-AGRD-THETA-CORRIDOR [M] (DR):** the general-demand same-distortion dominance interval has a unique exact endpoint determined by
   \[
   R_{\rm NR,\theta}(D)-[1-H_2(D)]=h+q+c.
   \]
3. **W5-SOL-OCCUPANCY-SCHUR [F] (DR):** the no-message joint-success probability is a symmetric convex, hence Schur-convex, occupancy polynomial.
4. **W5-SOL-MDC-NOMSG-18/19 [F] (DR+EC):** over the entire Q4 cap and lower-bound polytopes, parity beats every no-message sequential baseline for \(m\le18\), and a no-message baseline beats parity for every \(m\ge19\). This does **not** yet eliminate nontrivial prefix trees for \(10\le m\le18\).

## 1. Effort budget log

- Affirmative theorem construction/proof: 72%
- Independent exact computation: 22%
- Targeted obstruction work: 6%

## 2. Statement lock

Retain all Wave-5 locks. Let
\[
f(d)=1-H_2(d),\qquad 0\le d\le\tfrac12.
\]
For no recovery, pre-demand rate is \(I(X;Z)\). For recovery-aware agency, rate is
\[
I(X;Z)+I(X;R\mid Z,S).
\]

For the Q4 no-message theorem, joint distortion means failure of at least one of \(m\) demanded answers, and the sequential parity ledger remains
\[
M_{\rm par}=3m+2,\qquad L_{\rm par}=4.
\]

## 3. New theorem index

| ID | Statement | Status | Tag |
|---|---|---|---|
| W5-SOL-AGRD-THETA | Exact agency RD for arbitrary full-support demand law | DR | [M] |
| W5-SOL-AGRD-WATERFILL | Exact parametric no-recovery RD curve | DR+EC | [M] |
| W5-SOL-AGRD-THETA-CORRIDOR | Unique general-demand lossy dominance endpoint | DR | [M] |
| W5-SOL-OCCUPANCY-SCHUR | Schur-convex no-message success polynomial | DR | [F] |
| W5-SOL-MDC-NOMSG-18/19 | Full-polytope no-message phase boundary | DR+EC | [F] |

## 4. W5-SOL-AGRD-THETA — exact arbitrary-demand agency curve

Let \(S\sim\theta\), with every \(\theta_i>0\).

### Theorem

For every \(0\le D\le1/2\),
\[
\boxed{R_{\rm ag,\theta}(D)=1-H_2(D).}
\]

### Proof

For any protocol,
\[
I(X;Z)+I(X;R\mid Z,S)=I(X;Z,R\mid S).
\]
By data processing and conditional binary Fano,
\[
I(X;Z,R\mid S)\ge I(X_S;\widehat A\mid S)\ge1-H_2(D).
\]

Achievability takes \(Z\) constant and sends after observing \(S\)
\[
R=X_S\oplus N,\qquad N\sim\operatorname{Bernoulli}(D).
\]
Then the decision error is \(D\), and
\[
I(X;R\mid S)=1-H_2(D).
\]
Thus the curve is independent of \(\theta\). ∎

## 5. W5-SOL-AGRD-WATERFILL — exact no-recovery curve

The exact no-recovery problem is
\[
R_{\rm NR,\theta}(D)
=
\min_{\substack{0\le d_i\le1/2\\\sum_i\theta_i d_i\le D}}
\sum_i f(d_i).
\]

For \(0<D<1/2\), there is a unique multiplier \(\mu>0\) such that
\[
\boxed{d_i(\mu)=\frac1{1+2^{\mu\theta_i}}}
\]
and
\[
\boxed{D(\mu)=\sum_i\theta_i d_i(\mu).}
\]
The exact curve is
\[
\boxed{R_{\rm NR,\theta}(D(\mu))=\sum_i f(d_i(\mu)).}
\]

### Proof

The objective is strictly convex on \((0,1/2)^n\). The KKT equation is
\[
f'(d_i)+\mu\theta_i=0.
\]
Since
\[
f'(d)=\log_2\frac d{1-d},
\]
we obtain the logistic formula. Strict monotonicity of \(D(\mu)\) gives uniqueness. ∎

### Strict recovery advantage

For \(n>1\), full support, and \(D<1/2\),
\[
\boxed{R_{\rm NR,\theta}(D)>R_{\rm ag,\theta}(D).}
\]
Indeed,
\[
\sum_i f(d_i)>\sum_i\theta_i f(d_i)\ge f\!\left(\sum_i\theta_i d_i\right)=f(D).
\]
The first inequality is strict because every optimizer has \(d_i<1/2\), hence \(f(d_i)>0\), and every \(\theta_i<1\).

## 6. W5-SOL-AGRD-THETA-CORRIDOR — unique lossy endpoint

Let
\[
G_\theta(D)=R_{\rm NR,\theta}(D)-f(D).
\]
Then
\[
G_\theta(0)=n-1,\qquad G_\theta(1/2)=0.
\]
Moreover, \(G_\theta\) is strictly decreasing.

### Proof of monotonicity

Along the KKT curve,
\[
R_{\rm NR,\theta}'(D)=-\mu.
\]
Also
\[
f'(D)=-\log_2\frac{1-D}{D}.
\]
Because \(0<\theta_i<1\),
\[
d_i(\mu)=\frac1{1+2^{\mu\theta_i}}>rac1{1+2^\mu}.
\]
Therefore
\[
D=\sum_i\theta_i d_i(\mu)>\frac1{1+2^\mu},
\]
which implies
\[
\log_2\frac{1-D}{D}<\mu.
\]
Hence
\[
G_\theta'(D)=-\mu+\log_2\frac{1-D}{D}<0.
\]
∎

Let \(s=h+q+c\). If \(s<n-1\), there is a unique
\[
\boxed{D_\theta^\star\in(0,1/2)}
\]
satisfying
\[
\boxed{G_\theta(D_\theta^\star)=s.}
\]
For every \(0\le D<D_\theta^\star\), the recovery-aware same-distortion policy strictly improves latency and memory:
\[
\gamma_L(D)=G_\theta(D)-s>0,
\]
\[
\gamma_M(D)=2R_{\rm NR,\theta}(D)-f(D)-2h-q>0.
\]
Latency is again the binding condition, since \(G_\theta(D)>s\) implies
\[
\gamma_M(D)>q+2c+f(D)\ge0.
\]

This strictly generalizes the uniform endpoint
\[
H_2^{-1}\!\left(1-\frac{s}{n-1}\right).
\]

## 7. W5-SOL-OCCUPANCY-SCHUR — exact no-message polynomial

Fix a prototype \(p\). With no pre-demand message, all \(m\) answers are correct exactly when every demanded coordinate lies in the random agreement set
\[
B(X,p)=\{i:X_i=p_i\}.
\]
For uniform \(X\), every subset \(B\subseteq[n]\) occurs with probability \(2^{-n}\). Therefore
\[
\boxed{
P_{0,m}(\theta)
=
2^{-n}\sum_{B\subseteq[n]}\theta(B)^m,
\qquad
\theta(B)=\sum_{i\in B}\theta_i.
}
\]
This value is prototype-independent.

### Theorem

For every integer \(m\ge1\), \(P_{0,m}\) is symmetric and convex on the simplex, hence Schur-convex.

### Proof

Each map
\[
\theta\mapsto\theta(B)^m
\]
is convex on the nonnegative simplex because it is the composition of a linear form with the convex function \(x\mapsto x^m\). Their positive sum is convex. Summation over all subsets makes the function permutation-invariant. Every symmetric convex function is Schur-convex. ∎

Consequently, the maximum no-message success over a majorization-defined demand class occurs at its majorization-maximal point.

For Q4:

\[
P_{\rm unif}(m)
=
\frac1{16}\sum_{k=0}^4\binom4k\left(\frac k4\right)^m,
\]

\[
P_{\rm down}(m)
=
\frac1{16}\sum_{k=0}^3\binom3k
\left[
\left(\frac k5\right)^m+
\left(\frac{k+2}{5}\right)^m
\right],
\]

\[
P_{\rm cap}(m)
=
\frac1{16}\sum_{a=0}^2\sum_{b=0}^2
\binom2a\binom2b
\left(\frac{3a+2b}{10}\right)^m.
\]

## 8. W5-SOL-MDC-NOMSG-18/19 — full-polytope no-message boundary

The no-message sequential cost is
\[
M_0(\theta,m)=m+1+40[1-P_{0,m}(\theta)].
\]
Against parity,
\[
M_0-M_{\rm par}
=
\boxed{39-2m-40P_{0,m}(\theta).}
\]

Because \(P_{0,m}\) is Schur-convex, its maximum over Q4 down occurs at a permutation of \((2,1,1,1)/5\), and over Q4 cap at a permutation of \((3,3,2,2)/10\).

At \(m=18\), the exact worst-case margins are
\[
\gamma_{\rm unif}
=
\frac{7620400327}{17179869184}>0,
\]
\[
\gamma_{\rm down}
=
\frac{277615146191}{762939453125}>0,
\]
\[
\gamma_{\rm cap}
=
\frac{20074685943080277}{50000000000000000}>0.
\]
Since \(P_{0,m}(\theta)\) decreases with \(m\), these margins are positive for all \(m\le18\).

For every \(\theta\),
\[
P_{0,m}(\theta)\ge2^{-4}=\frac1{16},
\]
because the event \(X=p\) always yields complete success. Thus
\[
M_0-M_{\rm par}
\le39-2m-\frac{40}{16}
=
\frac{73}{2}-2m<0
\]
for every \(m\ge19\).

Therefore:

\[
\boxed{
\begin{array}{l}
\text{Parity beats every no-message baseline over the full Q4 cap/down classes for }m\le18,\\
\text{and some no-message baseline beats parity for every demand law when }m\ge19.
\end{array}
}
\]

This closes the entire no-message face. It does not yet prove that no nontrivial prefix tree beats parity for \(10\le m\le18\).

## 9. Conflict-resolution update

The previous point-law \(18/19\) boundary is strengthened as follows:

- **Full demand-polytope closure:** achieved for the no-message face by Schur convexity.
- **Complete prefix-hull closure:** still achieved through \(m=9\).
- **Remaining strip:** nontrivial trees only, \(10\le m\le18\).

The remaining problem is therefore no longer “optimize over all demand laws and all trees.” It is:

> Prove or refute that every nontrivial prefix tree has scalar cost at least the no-message root over Q4 cap/down for \(10\le m\le18\).

This is a substantially smaller obstruction.

## 10. Independent EC replication

`W5_CONTINUATION_CHECKS.py` verifies:

- the exact Q4 occupancy formulas;
- the exact margins at \(m=18\) and negative margins at \(m=19\);
- positivity of the arbitrary-demand RD gap at representative KKT points.

The exact outputs reproduce all fractions frozen above.

## 11. Failure log and next route

| Open route | Current obstruction | Next exact method |
|---|---|---|
| Full Q4 prefix hull, \(10\le m\le18\) | Nontrivial tree policies may have nonsymmetric piecewise-polynomial success | Enumerate active tree policies at rational cells, then certify each cell with Bernstein coefficients or CAD |
| Arbitrary-prefix all-\(n\) BP1 | Local high-density splits prevent edgewise charging | Search for a global entropy/external-path potential with leaf-size correction |
| Production corridor | Formal information rate is not measured token cost | Empirical tokenizer/store map with tail bounds |

## 12. North Star distance update

The agency RD spine is now demand-law complete for singleton queries. The Q4 multi-demand gap has been reduced from a full demand-polytope optimization to a finite family of nontrivial tree-policy inequalities on \(m=10,\ldots,18\).

## 13. Confidence

- Arbitrary-demand agency RD and water-filling: 0.98
- Unique lossy endpoint theorem: 0.96
- Occupancy Schur-convexity: 0.99
- Full-polytope no-message \(18/19\) boundary: 0.99
- Complete prefix-hull closure for \(10\le m\le18\): not claimed

## 14. Timestamp + model identity

```text
Timestamp: 2026-07-27 America/Los_Angeles
Model identity: GPT-5.6 Thinking
Continuation identity: RADC Wave-5 Sol Pro merge — append-only continuation 1
```
