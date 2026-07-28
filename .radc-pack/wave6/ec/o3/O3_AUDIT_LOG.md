PASS p_c(vertex)=7/25
PASS Fable M=9-p_c=218/25
PASS Fable L=11/2-3/2 p_c=127/25
PASS Fable L>5 (n=4 kill)
PASS ZE iff p_c>=(9-2n)/3 at n=4: 7/25<1/3 kill
PASS uniform p_c=1/4; M=35/4; L=41/8
PASS E[#exp] Fable uniform = 7/4
PASS E[#exp] Fable vertex = 43/25 > 1
PASS F2_batch,down(150/17)=6+27/100*150/17 >= 8 ?
PASS F2down(40)=10
PASS F2cap(40)=10
PASS G2down(40)=15
PASS F2down crosses 8 exactly at 150/17
PASS F2cap crosses 8 exactly at 1200/137
PASS G2down crosses 8 at 125/17
PASS Kimi batch margins (5,0,1): (10-5,0,5-4)
PASS Kimi seq margins (7,0,1): (15-8,0,5-4)
PASS necessity 11/2>5
t1(3) = 8 ~= 8.0000
t1(4) = 80/9 ~= 8.8889
t1(5) = 800/79 ~= 10.1266
t1(6) = 32/3 ~= 10.6667
t1(7) = 896/79 ~= 11.3418
t1(8) = 256/21 ~= 12.1905
PASS e_anti(7)<1/3<e_anti(8)
PASS rho_kill(3)=4/e_anti
PASS rho_kill(4)=4/e_anti
PASS rho_kill(5)=4/e_anti
PASS rho_kill(6)=4/e_anti
PASS rho_kill(7)=4/e_anti
m_fail(n,40): {2: 15, 3: 18, 4: 19, 5: 19, 6: 20, 7: 20, 8: 20}
PASS m_fail(4,40)=19
PASS m_fail(3,40)=18
PASS 160/11 > 64/5 (Q4d>Q4u)
PASS 135/8 > 16 (Q3d>Q3u)
TOTAL: 28 / 28 PASS

== O3 two-demand parity batch-threshold verification (against Fable w5b re-run alpha=2 lines) ==
Q4-uniform: F2_2 reaches 8 at t=96/11 via line 2+11t/16  [PASS, matches W5-spine]
Q4-down:    Kimi binding seq threshold 150/17 (H2>=8), G2>=8 at 125/17  [PASS previously]
Q3-down:    envelope = min(2+33t/50, 4+41t/100, 8); crosses 8 at 400/41  [PASS]
Q3-uniform: envelope = min(2+2t/3, 4+5t/12, 8); crosses 8 at 48/5  [PASS]
Fable-side F2_3 thresholds re-confirmed from re-run: 92/11, 143/17, 94/11, 17/2 (M-side); 80/11, 125/17, 250/33, 15/2 (target 8)
n_crit collision check (w5b tail): p_c>=(9-2n)/3 at vertices: n=3 False, n=4 False, n>=5 True  [PASS]

== O3 ISC phase verification (U1 pre-draft; numeric 1e-12 bisection) ==
rho*_ISC uniform closed form -2n log2(2^{1-(1+s)/n}-1): n=4,s=2 -> 19.215694029125 [PASS]
F_down(n,rho)=2+psi(rho(n+4)/5n)+(n-1)psi(4rho/5n)=8 roots: n=4: 20.761269757482; n=5: 17.577410966179; n=8: 14.921275962574; n=20: 13.307915419377; n=100: 12.670054406604 [ALL PASS vs W5-spine]
asymptotic x1^3=4(x1+1): x1=2.382975767906373, 10 log2 x1 = 12.527642810712 [PASS]
uniform limit n->inf: 4(1+s)=12 confirmed (12.0000125 at n=1e6) [PASS]

== O3 multi-demand structure verification (U-tier pre-draft) ==
n_crit^opaque(m) m=1..12 recomputed exactly from A_{n,m},B_{n,m}: [3,5,6,7,8,9,10,11,12,12,13,14] [PASS vs W5-spine]
asymptotic n_crit/m -> 1/ln 3 = 0.9102392266268373 [PASS]
ZE batch margins (2n-3, n-3) at n=3: (3,0) weak dominance boundary [PASS]

== O3 corridor rho*_cap(s) verification ==
phi_F(4+2s) == rho*_cap(s) piecewise (4+4s | 20s/3 | 80(s-1)/7 | +inf) at 8 sample points incl. all seams [PASS]
registered s=2 -> 40/3 [PASS]; landmark thresholds recovered: s=3/2->10 (rho_M... no: F=10 sat), s=5/2->120/7 (advertised), s=3->160/7 (identity saturation)

== O3 DLU ledger arithmetic (U4 pre-draft) ==
inverse h=M-L, q=2L-M-3 at (5,4),(6,5),(7,5): h,q in cone u/2<=v<=u [PASS x3]
integer-token radii (r2,r_inf,r1)=(sqrt2,1,2) from (dM,dL)=(2a+b,a+b), nearest (0,1)->(1,1) [PASS]

== O3 n=3 no-message face (audit reference for Tier-2 G7) ==
vertex (7,4,4)/15, rho=40, gamma_{0,m}=39-2m-40*P_{0,m}, P_{0,m}=2^-3 sum_B theta(B)^m:
m=14: 1269145731735089/216243896484375 ~= 5.869048 POS
m=15: 168849719449271/43248779296875 ~= 3.904150 POS
m=16: 845049722020265693/437893890380859375 ~= 1.929805 POS
m=17: -22519522704133297/437893890380859375 ~= -0.051427 NEG  (margin dies at m=17)
m=18: -200765409863563655039/98526125335693359375 ~= -2.037687 NEG
m=19: -396826139021214462733/98526125335693359375 ~= -4.027624 NEG
Matches Grok W6-CONT2-N3-EXACT decimals (3.904/1.930/-0.0514/-2.038); independent exact fractions now on record.

== O3 INDEPENDENT n=3 FULL-PREFIX PHASE DERIVATION (audit reference for W6-DS-G7 candidate) ==
Setup: n=3, Theta_3^down (vertex (7,4,4)/15), (rho,lambda)=(40,20), parity (3m+2,0,4).
(a) m=1..9 one-demand-floor reduction M_T >= ((m+1)/2) F_3down(80/(m+1)):
    margins m=1..9 = 3, 4, 5, 137/24, 5, 4, 3, 2, 1 -- ALL >= 1 [PASS]
    NOTE: W4/W5 extraction log Q3-down pieces "2+2t / 4+4t" are TYPOS; correct pieces from
    supported pairs (0,60),(8,30),(15,16),(24,0) are 2+t/2 (t<=8), 4+t/4 (8..15), 23/4+2t/15 (15..135/8), 8.
    Seams 8, 15, 135/8 verified consistent with corrected pieces; F(40)=8 landmark unaffected.
(b) m=10..16 nontrivial trees: r>=5 => ell>=2 (C_8(5)=16) => Gamma >= 1 exactly;
    r=2,3,4: B_r(m) decreasing in m (slope c_r/8-2 < 0); B_r(16) = 13.5790, 12.8992, 14.3443 -- ALL > 1 [PASS]
    coverage floor p_m = 1 - (8/15)^m - 2(11/15)^m (vertex max miss-sum, convexity).
(c) no-message face: P_{0,m} Schur-max at vertex; gap 39-2m-40P positive for m<=16 (m=16: +1.929805),
    negative at m=17 (-0.051427) -- vertex witness kills m=17; universal bound gap <= 34-2m kills m>=18 for ALL theta.
(d) latency: F_3down(40)=8 => L_T >= 4 = L_par (gamma_L=0 WEAK; gamma_M >= 1 strict; gamma_D=0).
CANDIDATE THEOREM: parity weakly dominates (M-strict) the full randomized variable-length no-recovery
prefix hull on Theta_3^down at (40,20) iff 1 <= m <= 16; m_crit(3) = 16 < 18 = m_crit(4).
Obstruction witness at m=17: theta=(7,4,4)/15 no-message baseline, gap -22519522704133297/437893890380859375.
This CLOSES the n=3 full-prefix question Grok W6 left OPEN (W6-GROK-CONT2-FULL-N).

== O3 G9 audit reference: exact no-message phase boundary m_fail(rho) at n=4 (both class-extreme laws coincide) ==
rho:   20 24 28 32 36 40 48 56 64 80
m_fail: 9 11 13 15 17 19 22 26 30 37   (parity dominates no-message face for m <= m_fail-1)
rho=40 -> 19 [matches Cont-2 obstruction onset]
NOTE: exact boundary dips BELOW Grok's crude formula floor((rho(1-2^-n)-1)/2)+1 at rho=48 (exact 22 vs crude 23):
the crude bound uses only P >= 1/16; the exact occupancy P_{0,m}(vertex) is strictly larger at moderate m.

== O3 B1 audit reference: ANTI-OPT bridge + tie law ==
Fable 2S(n-1)/(2^n 5n) == Kimi (2(n-1)-B(n))/(5n) for n=2..30 [ALL EQUAL]
proof: min(4k,5n-4k)=(5n-|5n-8k|)/2; E|8K-5n|=2E[(8K-5n)+]+(n+4); hence E[min]=2(n-1)-B(n). QED.
e_anti n=3..8: 1/4, 11/40, 121/400, 5/16, 145/448, 43/128 [MATCH both camps]
mod-8 tie law (drop-one-light ties iff 8|n) verified by direct Rademacher sums n=3..24 [CONSISTENT]

== O3 A5 audit reference: decision-TV model ==
NEGATIVE: marginal-TV RD is degenerate -- R=0 for any d>0 (independent reproduction Q~Bern(1/2-d) has I=0);
so marginal TV between source/reproduction marginals is NOT a viable agency distortion. (numeric grid, 400x400)
POSITIVE: for the conditional decision distortion D_dec = E[TV(delta_{X_S}, P_{A-hat|Z,R,S})]:
TV(delta_a, P) = 1 - P(a) for any finite action set, so D_dec = E[1 - P(correct)] = action-error probability.
Hence the k-action finite decision-TV model collapses EXACTLY to 0-1 loss for point-mass truth,
and R_ag(D) = 1-H2(D) (binary action/coordinate) carries over verbatim; multi-coordinate joint demands
are the only non-collapsed extension (joint success != product of marginals -- see Cont-2 occupancy).

== O3 G9 rho* verification ==
rho*_endpoint = 37/(1-P_{0,18}): down = 141143798828125/3563296863977 ~= 39.61045184 [MATCH Tier-2]
cap = 74000000000000000000/1870074685943080277 ~= 39.57061210 [MATCH Tier-2]
registered rho=40 slack: down 1388075730955/3563296863977 ~= 0.3895; cap ~= 0.4294 -- phase [1..18] survives with positive slack.

== O3 G8 n=5 vertex margins verification ==
vertex (9,4,4,4,4)/25, gap = 39-2m-40 P_{0,m}: m=18: +887975035189461090631639/582076609134674072265625 ~= +1.525529;
m=19: -254541365995396231447867/582076609134674072265625 ~= -0.437299; m=20: -875473201896514136958568509/363797880709171295166015625 ~= -2.406482 [ALL MATCH Tier-2]
n=5 no-message vertex crossing at m=18/19 (same boundary as n=4's class-extreme, later than n=3's 16/17).

== O3 AUDIT FLAG A3: tier4/a3_corridor_ec.py AssertionError is a HARNESS BUG, not a substrate violation ==
Script computes D* from outer s in (0.5,1.0,2.0) but tests the gamma_M chain with hardcoded (h,q,c)=(1,0,1) (s=2).
Cont-1 chain gamma_M = 2G(D)+f(D)-2h-q > q+2c+f(D) is a tautology given G(D) > s with MATCHING s=h+q+c.
Verified: at s_out=0.5, D near D*: G(D) ~= 0.56 < 2, so the s=2 premise is false and the assertion fails legitimately.
With matching (h,q,c)=(0.5,0,0): gM=0.3079 > q+2c+f=0.1868 [PASS]. Identity gamma_M == 2G+f-2h-q exact (diff 0.0).
Verdict: Cont-1 corridor theorem STANDS; script needs the chain tested with s-consistent (h,q,c).
