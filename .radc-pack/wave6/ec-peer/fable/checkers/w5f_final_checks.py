#!/usr/bin/env python3
"""W5 final checks: ANTI-OPT exact difference formula + tie law; big-int certificates."""
from math import comb

def S(n, m):
    a = n + 4
    return sum(comb(m, k) * min(4*k, a + 4*m - 4*k) for k in range(m+1))

def g1(n, m):
    return (1 << (n-1)) * 4 * (n-1-m) + (1 << (n-m)) * S(n, m)

# claimed: Delta_m = g1(n,m) - g1(n,m+1) = 2^{n-m-1} * C(m,k0) * (4 - r)  if k0 <= m else 0
# where c = n+4+4m, k0 = nearest multiple of 8 to c divided by 8, r = |8 k0 - c|.
ok = True
for n in range(3, 41):
    for m in range(0, n-1):
        c = n + 4 + 4*m
        k0 = (c + 4) // 8            # nearest multiple (ties -> r=4 -> zero anyway)
        r = abs(8*k0 - c)
        if r > 4:
            k0 = c // 8; r = abs(8*k0 - c)
        pred = 0
        if k0 <= m and r < 4:
            pred = (1 << (n-m-1)) * comb(m, k0) * (4 - r)
        actual = g1(n, m) - g1(n, m+1)
        if pred != actual:
            ok = False
            print(f"MISMATCH n={n} m={m}: pred {pred} actual {actual} (c={c}, k0={k0}, r={r})")
print("Delta formula exact for all n in 3..40, all m:", ok)

# tie law: g1(n, n-2) == g1(n, n-1) iff 8 | n; and no deeper ties
for n in range(3, 41):
    tie2 = g1(n, n-2) == g1(n, n-1)
    tie3 = (n >= 4) and g1(n, n-3) == g1(n, n-1)
    assert tie2 == (n % 8 == 0), n
    assert not tie3 or n == 3, (n, tie3)
print("tie law: final-step tie iff 8|n; never a third tied class: verified n in 3..40")

# b=0 strictly worse: g0(n,m) - g1(n,m) > 0
def g0(n, m):
    T = sum(comb(m, k) * min(k, m-k) for k in range(m+1))
    return (1 << (n-1)) * (5*n - 4*m) + (1 << (n-m)) * 4 * T
bad = [(n, m) for n in range(3, 31) for m in range(1, n) if g0(n, m) <= g1(n, m)]
print("b=0 always strictly worse (n<=30):", bad == [])

# big-integer certificates for Psi_down(n,40) >= 11, n=6,7; and chord lemma pieces
print("\ncertificates:")
print("  n=6: 65^2 * 463^10 <= 8 * 64^2 * 400^10 :", 65**2 * 463**10 <= 8 * 64**2 * 400**10)
print("  n=7: 2075^2 * 309^12 <= 32 * 2048^2 * 256^12 :", 2075**2 * 309**12 <= 32 * 2048**2 * 256**12)
print("  n=7 aux: 27^7 >= 2^33 :", 27**7 >= 2**33, "; 53^7 >= 2^40 :", 53**7 >= 2**40)
print("  chord: 125 <= 128 (log2(5/4) <= 1/3):", 125 <= 128)
print("  chord: 17^11 <= 2^45 (log2(17/16) <= 1/11):", 17**11 <= 2**45, 17**11, 2**45)
print("  n>=8 tail: need 20/11 + (16(n-1)/n)(2/3) >= 9 at n=8: 20/11 + 28/3 =",
      20/11 + 28/3, ">= 9:", 20/11 + 28/3 >= 9)
print("  MDC-5: 16641 * 43046721 =", 16641*43046721, "< 3*16384*16777216 =", 3*16384*16777216)
print("  MDC-5: 3^5 =", 3**5, "< 2^8 =", 2**8)
print("  Psi(5,18) cert: 7^25 > 2^69:", 7**25 > 2**69, "; 3^25 > 2^39:", 3**25 > 2**39,
      "; 71*11^4 = ", 71*11**4, "< 2^20 =", 2**20)

# p_c extremes on Theta_n^down: vertex value ((n+4)^2+16(n-1))/(25 n^2), min = 1/n
from fractions import Fraction as Fr
for n in (3, 4, 5, 6):
    pcv = Fr((n+4)**2 + 16*(n-1), 25*n*n)
    print(f"  p_c at Theta_{n}^down vertex = {pcv}; (9-2n)/3 = {Fr(9-2*n,3)}; "
          f"vertex >= threshold? {pcv >= Fr(9-2*n,3)}")
