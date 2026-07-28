#!/usr/bin/env python3
"""G2: prefix-length spectra C_N(r) by subset-split DP, exact integers.

Model (Cont-2 lock): the transcript is a source-dependent variable-length
partition code. A full binary tree whose leaves partition the N equiprobable
source words; leaf A_j sits at depth d_j; external path sum
L_ext = sum_j |A_j| d_j = N * ell. Splitting a set of size n into nonempty
parts adds 1 to the depth of every word in it, contributing n to L_ext.

C_N(1) = 0;  C_N(r) = min over a in 1..N-1, r1 in 1..r-1 of
    N + C_a(r1) + C_{N-a}(r-r1).

Independently re-derived (Tier-2, not copied from Cont-2 checker).
Checks Grok's C_8, C_16, C_32[1..8], C_32(32); extends to C_64 (r <= 12).
"""

def spectrum(N, rmax):
    INF = None
    # C[n][r], n = 1..N, r = 1..min(rmax, n); None = unreachable/unset
    C = [[None] * (rmax + 1) for _ in range(N + 1)]
    for n in range(1, N + 1):
        C[n][1] = 0
    for n in range(2, N + 1):
        rtop = min(rmax, n)
        for r in range(2, rtop + 1):
            best = None
            for a in range(1, n // 2 + 1):
                b = n - a
                Ca, Cb = C[a], C[b]
                r1lo = max(1, r - min(rmax, b))
                r1hi = min(a, r - 1, rmax - 1)
                for r1 in range(r1lo, r1hi + 1):
                    r2 = r - r1
                    if r2 < 1 or r2 > b or r2 > rmax:
                        continue
                    va, vb = Ca[r1], Cb[r2]
                    if va is None or vb is None:
                        continue
                    v = n + va + vb
                    if best is None or v < best:
                        best = v
            C[n][r] = best
    return C


def main():
    # N = 8, 16, 32 full spectra; N = 64 up to r = 12
    C8 = spectrum(8, 8)
    C16 = spectrum(16, 16)
    C32 = spectrum(32, 32)
    C64 = spectrum(64, 12)

    s8 = [C8[8][r] for r in range(1, 9)]
    s16 = [C16[16][r] for r in range(1, 17)]
    s32 = [C32[32][r] for r in range(1, 33)]
    s64 = [C64[64][r] for r in range(1, 13)]

    print("C_8  (r=1..8)  =", tuple(s8))
    print("C_16 (r=1..16) =", tuple(s16))
    print("C_32 (r=1..32) =", tuple(s32))
    print("C_64 (r=1..12) =", tuple(s64))

    # Grok's claimed values (W6-GROK-LENGTH-SPECTRUM-N)
    assert s8 == [0, 8, 10, 13, 16, 20, 22, 24], "C_8 mismatch vs Grok"
    assert s16 == [0, 16, 18, 21, 24, 28, 32, 36, 40, 45, 50, 53, 56, 60, 62, 64], "C_16 mismatch"
    assert s32[:8] == [0, 32, 34, 37, 40, 44, 48, 52], "C_32[1..8] mismatch vs Grok"
    assert s32[31] == 160, "C_32(32) != 160"
    print("PASS Grok C_8, C_16, C_32[1..8], C_32(32)=160 all reproduced exactly")

    # Structural checks: identity tree gives C_N(N) = N*log2(N)
    for N, s in ((8, s8), (16, s16), (32, s32)):
        lg = N.bit_length() - 1
        assert s[N - 1] == N * lg, f"C_{N}({N}) != {N*lg}"
    print("PASS C_N(N) = N log2 N for N=8,16,32 (identity tree extremal at r=N)")

    # Monotonicity and Kraft sanity: C_N(r) nondecreasing in r
    for name, s in (("C_8", s8), ("C_16", s16), ("C_32", s32), ("C_64[1..12]", s64)):
        assert all(s[i] <= s[i + 1] for i in range(len(s) - 1)), name
    print("PASS spectra nondecreasing in r")

    # Cross-check small-r against the d_min=1 strip formula C_N(r) = N + U_{r-1},
    # U_k = k(a+2) - 2^(a+1), a = floor(log2 k), valid while U_{r-1} <= N (r small)
    def U(k):
        a = k.bit_length() - 1
        return k * (a + 2) - (1 << (a + 1))
    for N, s in ((8, s8), (16, s16), (32, s32)):
        for r in range(2, len(s) + 1):
            if U(r - 1) <= N:
                assert s[r - 1] == N + U(r - 1), (N, r, s[r - 1], N + U(r - 1))
    print("PASS small-r entries match N + U_{r-1} closed form where U_{r-1} <= N")

    # Values needed downstream (barrier thresholds ell >= 2)
    for N, s in ((8, s8), (16, s16), (32, s32)):
        r0 = next(r for r in range(1, N + 1) if s[r - 1] >= 2 * N)
        print(f"N={N}: least r with C_N(r) >= 2N (ell>=2 barrier case) is r = {r0}")


if __name__ == "__main__":
    main()
