#!/usr/bin/env python3
"""W5 Part E: (1,2,3,3) caterpillar scan at Q5; rho_cert(5) bracket; split-gain audit."""
from fractions import Fraction as Fr
import math

# ---- Q5 caterpillar (1,2,3,3): leaves at depths 1,2,3,3
n = 5; w = (9, 4, 4, 4, 4); d = 25; N = 1 << n
Dst = [[sum(w[i] for i in range(n) if ((x ^ p) >> i) & 1) for p in range(N)] for x in range(N)]
def cat_best(t):
    tn, td = t.numerator, t.denominator
    best = None
    c1 = [[2*1*d*td + tn*Dst[x][p] for x in range(N)] for p in range(N)]
    c2 = [[2*2*d*td + tn*Dst[x][p] for x in range(N)] for p in range(N)]
    c3 = [[2*3*d*td + tn*Dst[x][p] for x in range(N)] for p in range(N)]
    for p1 in range(N):
        a1 = c1[p1]
        for p2 in range(N):
            a2 = c2[p2]
            for p3 in range(N):
                a3 = c3[p3]
                for p4 in range(p3, N):
                    a4 = c3[p4]
                    tot = 0
                    for x in range(N):
                        v = a1[x]
                        if a2[x] < v: v = a2[x]
                        if a3[x] < v: v = a3[x]
                        if a4[x] < v: v = a4[x]
                        tot += v
                    if best is None or tot < best: best = tot
    return best

for t in (Fr(1600,121), Fr(14)):
    b = cat_best(t)
    val = Fr(2) + Fr(b, N*d*t.denominator)
    print(f"caterpillar(1,2,3,3) at t={t}: min value = {val} = {float(val):.5f}  "
          f"{'KILLS' if val < 8 else '>=8 (no kill)'}")

# ---- rho_cert(5): Psi_5(t) = 12 - 2[log2(1+2^(-9t/50)) + 4 log2(1+2^(-2t/25))]
def Psi5(t):
    return 12 - 2*(math.log2(1+2**(-9*t/50)) + 4*math.log2(1+2**(-2*t/25)))
lo, hi = 13.0, 20.0
for _ in range(60):
    mid = (lo+hi)/2
    if Psi5(mid) >= 8: hi = mid
    else: lo = mid
print(f"\nrho_cert(5) numeric = {hi:.6f}   Psi5(17.55)={Psi5(17.55):.5f}  Psi5(17.6)={Psi5(17.6):.5f}  Psi5(18)={Psi5(18):.5f}")
# exact certificate ingredients
print("cert: 7^25 > 2^69:", 7**25 > 2**69, 7**25, 2**69)
print("cert: 3^25 > 2^39:", 3**25 > 2**39)
print("cert: 71*11^4 < 4*64*8^4:", 71*11**4, "<", 4*64*8**4, ":", 71*11**4 < 4*64*8**4)
print("W4 n=4 cert: 257*17^3 =", 257*17**3, "< 2^21 =", 2**21, ":", 257*17**3 < 2**21)
print("MDC-5 cert: 129^2 * 9^8 < 3*128^2*8^8:", 129**2*9**8, "<", 3*128**2*8**8, ":", 129**2*9**8 < 3*128**2*8**8)
print("MDC-5 cert: 3^5 < 2^8:", 3**5 < 2**8)
print("Phi6 cert: 463^4 <= 2*400^4:", 463**4, "<=", 2*400**4, ":", 463**4 <= 2*400**4)
print("Phi6 cert: (63/400)^3 > 1/256 i.e. 63^3*256 > 400^3:", 63**3*256, ">", 400**3, ":", 63**3*256 > 400**3)

# ---- split-gain density audit for BP1 (n=3,4 vertices)
def audit(n, w, d, name):
    N2 = 1 << n; size = 1 << N2
    pc = [0]*size
    for A in range(1, size): pc[A] = pc[A & (A-1)] + 1
    ones = [[0]*size for _ in range(n)]
    for i in range(n):
        oi = ones[i]
        for A in range(1, size):
            lb = A & (-A); x = lb.bit_length()-1
            oi[A] = oi[A ^ lb] + ((x >> i) & 1)
    E = [0]*size
    for A in range(size):
        k = pc[A]; tot = 0
        for i in range(n):
            c1 = ones[i][A]
            tot += w[i]*(c1 if 2*c1 <= k else k-c1)
        E[A] = tot
    best = Fr(-10); arg = None
    for A in range(1, size):
        k = pc[A]
        if k < 2: continue
        lb = A & (-A); rest = A ^ lb; s = rest; loc = None
        while True:
            s = (s-1) & rest
            Bm = s | lb
            if Bm != A:
                gain = E[A] - E[Bm] - E[A ^ Bm]
                if loc is None or gain > loc: loc = gain
            if s == 0: break
        gden = Fr(loc, k)
        if gden > best: best = gden; arg = (A, k)
    s1 = Fr(E[size-1] - 0, 1)  # not used
    print(f"{name}: max split-gain density = {best} (den d={d}: = {best}/{d} of prob mass)"
          f" at |A|={arg[1]}, full set? {arg[0] == size-1}")
    return best

print()
b1 = audit(4, (3,3,2,2), 10, "Q4 cap")
print("   s1 = d*(1/2 - e1) =", 10*(Fr(1,2)-Fr(3,10)), "-> density bound equals?", b1)
b2 = audit(4, (2,1,1,1), 5, "Q4 down")
print("   s1 =", 5*(Fr(1,2)-Fr(11,40)), "matches max density?", b2 == 5*(Fr(1,2)-Fr(11,40)))
b3 = audit(4, (1,1,1,1), 4, "Q4 uniform")
print("   s1 =", 4*(Fr(1,2)-Fr(5,16)), "matches?", b3 == 4*(Fr(1,2)-Fr(5,16)))
b4 = audit(3, (7,4,4), 15, "Q3 down")
print("   s1 =", 15*(Fr(1,2)-Fr(1,4)), "matches?", b4 == 15*(Fr(1,2)-Fr(1,4)))
b5 = audit(3, (1,1,1), 3, "Q3 uniform")
print("   s1 =", 3*(Fr(1,2)-Fr(1,4)), "matches?", b5 == 3*(Fr(1,2)-Fr(1,4)))
