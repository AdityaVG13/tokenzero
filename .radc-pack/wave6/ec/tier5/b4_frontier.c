/* W6 Tier-5 jobs B4/B7: exact DP audits of the BP1 amortized tangent inequality.
 *
 * Model: X ~ Unif({0,1}^n), weights w_i (integer), d = sum w.
 * E(A) = sum_i w_i min(N_i^0(A), N_i^1(A));  e(T) = sum_leaves E(leaf)/(d 2^n);
 * ell(T) = L(T)/2^n, L = sum_leaves |leaf| depth = sum_internal |cell|.
 * J_t(T) = 2(1+ell) + t e  =>  d 2^n J_t = 2 d 2^n + 2 d L + t sum E(leaf).
 *
 * U-DP at scaled slope (K/S = s*d):  U(A) = min(S*E(A), K*|A| + min_splits U(B)+U(C))
 *   amortized tangent e(T) >= 1/2 - s ell(T) for ALL trees  <=>  U(Omega) = S*E(Omega).
 * V-DP at t = p/q:  V(A) = min(p*E(A), q*2d*|A| + min_splits V(B)+V(C));
 *   F(t) = (q*2d*2^n + V(Omega)) / (q*d*2^n).
 *
 * n<=4: full subset DP (all 2^(2^n) cells). n=5: one-bit brute force over all
 * 2^31 bipartitions + depth-2 structured/random families.
 */
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef int64_t i64;
#define MAXCELL (1<<16)

static int n, N; /* N = 2^n sources */
static int w[8], d;
static uint32_t colm[8];
static int popc[MAXCELL];
static int Ecell[MAXCELL];
static i64 U[MAXCELL];

static inline int E_of(uint32_t mask, int sz) {
    int e = 0;
    for (int i = 0; i < n; i++) {
        int c1 = __builtin_popcount(mask & colm[i]);
        int c0 = sz - c1;
        e += w[i] * (c0 < c1 ? c0 : c1);
    }
    return e;
}

static void setup(int nn, const int *ww) {
    n = nn; N = 1 << n; d = 0;
    for (int i = 0; i < n; i++) { w[i] = ww[i]; d += ww[i]; }
    for (int i = 0; i < n; i++) {
        uint32_t m = 0;
        for (int x = 0; x < N; x++) if ((x >> (n - 1 - i)) & 1) m |= (1u << x);
        colm[i] = m;
    }
    int ncells = 1 << N;
    for (int A = 0; A < ncells; A++) {
        popc[A] = __builtin_popcount((unsigned)A);
        Ecell[A] = E_of((uint32_t)A, popc[A]);
    }
}

/* generic cell DP: stop value stopMul*E(A), split cost splitMul*|A| */
static void cell_dp(int stopMul, i64 splitMul) {
    int ncells = 1 << N;
    U[0] = 0;
    for (int sz = 1; sz <= N; sz++) {
        for (int A = 1; A < ncells; A++) {
            if (popc[A] != sz) continue;
            i64 best = (i64)stopMul * Ecell[A];
            uint32_t low = (uint32_t)A & (0u - (uint32_t)A);
            i64 ms = INT64_MAX;
            for (uint32_t B = (A - 1) & A; B; B = (B - 1) & A) {
                if (!(B & low)) continue;
                uint32_t C = A ^ B;
                i64 v = U[B] + U[C];
                if (v < ms) ms = v;
            }
            if (ms != INT64_MAX) {
                i64 cand = splitMul * sz + ms;
                if (cand < best) best = cand;
            }
            U[A] = best;
        }
    }
}

static i64 gcd64(i64 a, i64 b) { while (b) { i64 t = a % b; a = b; b = t; } return a < 0 ? -a : a; }

static void F_at(int p, int q, const char *label) {
    /* V(A) = min(p*E(A), q*2d*|A| + min splits) */
    cell_dp(p, (i64)q * 2 * d);
    int full = (1 << N) - 1;
    i64 num = (i64)q * 2 * d * N + U[full];
    i64 den = (i64)q * d * N;
    i64 g = gcd64(num, den);
    printf("    F(%d/%d) [%s] = %lld/%lld = %.6f\n", p, q, label,
           (long long)(num / g), (long long)(den / g), (double)num / (double)den);
}

static void one_bit_small(void) {
    /* brute force one-bit codes for n<=4 */
    int full = (1 << N) - 1;
    int best = 1 << 30; i64 cnt = 0;
    for (uint32_t M = 1; M < (uint32_t)full; M += 2) { /* source 0 in M canonical */
        int pc = __builtin_popcount(M);
        int e = Ecell[M] + Ecell[full ^ M];
        (void)pc;
        if (e < best) { best = e; cnt = 1; }
        else if (e == best) cnt++;
    }
    printf("    one-bit brute force: min enumerator E1 = %d  (e1 = %d/(%d*%d)), #optimal bipartitions = %lld\n",
           best, best, d, N, (long long)cnt);
}

static void bp1_check(const char *name) {
    int full = (1 << N) - 1;
    int EOm = Ecell[full];
    /* one-bit optimum E1 */
    int E1 = 1 << 30;
    for (uint32_t M = 1; M < (uint32_t)full; M += 2) {
        int e = Ecell[M] + Ecell[full ^ M];
        if (e < E1) E1 = e;
    }
    /* s1*d*2^n = d*2^{n-1} - E1 =: K, scale S = 2^n */
    i64 K = (i64)d * (N / 2) - E1;
    i64 S = N;
    printf("  [%s] d=%d N=%d E(Omega)=%d  E1(one-bit min)=%d  e1=%d/%d  s1=%lld/%lld  t1=%lld/%lld\n",
           name, d, N, EOm, E1, E1, d * N,
           (long long)(K), (long long)(d * N / 2 * 0 + S * d / 1), 0LL, 0LL);
    /* print s1 and t1 as reduced fractions */
    i64 sn = K, sd = (i64)d * N; /* s1 = K/(d 2^n) */
    i64 g = gcd64(sn, sd); sn /= g; sd /= g;
    i64 tn = 2 * sd, td = sn; g = gcd64(tn, td); tn /= g; td /= g;
    printf("        s1 = %lld/%lld ; t1 = 2/s1 = %lld/%lld = %.6f\n",
           (long long)sn, (long long)sd, (long long)tn, (long long)td, (double)tn / td);
    /* U-DP at slope s1: U(A) = min(S*E(A), K*|A| + splits) */
    cell_dp((int)S, K);
    printf("        U_{s1}(Omega) = %lld  vs  S*E(Omega) = %lld  -> amortized tangent at s1: %s\n",
           (long long)U[full], (long long)(S * EOm),
           U[full] == (i64)S * EOm ? "HOLDS (all trees)" : "*** FAILS ***");
    /* tightness: slope one unit below (s1*d - 1/2^n): K-1 */
    cell_dp((int)S, K - 1);
    printf("        U_{s1 - 1/(d2^n)}(Omega) = %lld  (< %lld => slope s1 tight)  %s\n",
           (long long)U[full], (long long)(S * EOm),
           U[full] < (i64)S * EOm ? "TIGHT" : "NOT TIGHT (s1 not optimal!)");
    one_bit_small();
}

/* ---------- n=5 structured audits ---------- */
static uint32_t rng_state = 0x12345678u;
static uint32_t xr(void) { rng_state ^= rng_state << 13; rng_state ^= rng_state >> 17; rng_state ^= rng_state << 5; return rng_state; }

static int inner_gain(uint32_t B) { /* E(B) - min over one-bit splits of B */
    int sz = __builtin_popcount(B);
    if (sz <= 1) return 0;
    int EB = E_of(B, sz);
    uint32_t low = B & (0u - B);
    int best = 1 << 30;
    for (uint32_t M = (B - 1) & B; M; M = (M - 1) & B) {
        if (!(M & low)) continue;
        uint32_t C = B ^ M;
        int v = E_of(M, __builtin_popcount(M)) + E_of(C, sz - __builtin_popcount(M));
        if (v < best) best = v;
    }
    return EB - best;
}

static void n5_depth2(uint32_t B, i64 *bestG, i64 *bestL, const char *tag, int verbose) {
    uint32_t full = 0xFFFFFFFFu;
    uint32_t C = full ^ B;
    int szB = __builtin_popcount(B), szC = 32 - szB;
    if (szB == 0 || szC == 0) return;
    int EOm = E_of(full, 32);
    int gam0 = EOm - E_of(B, szB) - E_of(C, szC);
    int gB = inner_gain(B), gC = inner_gain(C);
    /* full depth-2: L = 2*32 = 64 */
    i64 G2 = gam0 + gB + gC, L2 = 64;
    /* split only B: L = 32 + szB ; only C: L = 32 + szC */
    i64 Gb = gam0 + gB, Lb = 32 + szB;
    i64 Gc = gam0 + gC, Lc = 32 + szC;
    i64 Gs[3] = {G2, Gb, Gc}, Ls[3] = {L2, Lb, Lc};
    for (int k = 0; k < 3; k++) {
        /* ratio = G/(d*L); compare with current best */
        if (*bestL == 0 || (__int128)Gs[k] * (*bestL) > (__int128)(*bestG) * Ls[k]) {
            *bestG = Gs[k]; *bestL = Ls[k];
            if (verbose)
                printf("    new max ratio %lld/(%d*%lld) = %.6f  (%s, mode %d, |B|=%d)\n",
                       (long long)Gs[k], d, (long long)Ls[k], (double)Gs[k] / (d * Ls[k]), tag, k, szB);
        }
    }
}

static void n5_audit(void) {
    int ww[5] = {9, 4, 4, 4, 4};
    setup(5, ww);
    printf("  [Q5-down] d=%d  E(Omega)=%d  s1=79/400, t1=800/79 (conjectured)\n", d, E_of(0xFFFFFFFFu, 32));
    /* (a) one-bit brute force over all 2^31 canonical bipartitions */
    uint32_t full = 0xFFFFFFFFu;
    int best = 1 << 30; i64 cnt = 0;
    for (uint32_t M = 1; M < full; M += 2) {
        int pc = __builtin_popcount(M);
        int e = E_of(M, pc) + E_of(full ^ M, 32 - pc);
        if (e < best) { best = e; cnt = 1; }
        else if (e == best) cnt++;
    }
    printf("    one-bit brute (2^31): E1min = %d (expect 242 -> e=242/800=121/400); #optimal bipartitions = %lld (expect 16)\n",
           best, (long long)cnt);
    /* (b) depth-2 structured: outer = prototype-ball splits {0,q}, ties both ways; coordinate splits */
    i64 bestG = 0, bestL = 0;
    for (uint32_t q = 1; q < 32; q++) {
        for (int tierule = 0; tierule < 2; tierule++) {
            uint32_t B = 0;
            for (uint32_t x = 0; x < 32; x++) {
                int d0 = __builtin_popcount(x), d1 = __builtin_popcount(x ^ q);
                int inB = (d0 < d1) || (tierule && d0 == d1);
                if (inB) B |= (1u << x);
            }
            n5_depth2(B, &bestG, &bestL, "ball", 0);
        }
    }
    for (int j = 0; j < 5; j++) n5_depth2(colm[j], &bestG, &bestL, "coord", 0);
    printf("    depth-2 (all 62 ball outers + 5 coord outers, exact inner 1-bit): max ratio = %lld/(%d*%lld)\n",
           (long long)bestG, d, (long long)bestL);
    printf("      compare s1: 400*%lld vs 79*%d*%lld -> %s\n", (long long)bestG, d, (long long)bestL,
           (__int128)400 * bestG <= (__int128)79 * d * bestL ? "<= s1 OK" : "*** EXCEEDS s1: BP1 CONJECTURE REFUTED ***");
    /* (c) random outers, exact inner one-bit */
    i64 bG2 = 0, bL2 = 0;
    int samples = 0;
    while (samples < 200) {
        uint32_t B = xr();
        int pc = __builtin_popcount(B);
        if (pc < 6 || pc > 18) continue;
        n5_depth2(B, &bG2, &bL2, "random", 0);
        samples++;
    }
    printf("    depth-2 (200 random outers, |B| in 6..18, exact inner 1-bit): max ratio = %lld/(%d*%lld)\n",
           (long long)bG2, d, (long long)bL2);
    printf("      compare s1: %s\n",
           (__int128)400 * bG2 <= (__int128)79 * d * bL2 ? "<= s1 OK" : "*** EXCEEDS s1 ***");
}

int main(int argc, char **argv) {
    (void)argc; (void)argv;
    int q3d[3] = {7, 4, 4}, q3u[3] = {5, 5, 5};
    int q4d[4] = {8, 4, 4, 4}, q4c[4] = {6, 6, 4, 4}, q4u[4] = {5, 5, 5, 5};

    printf("=== BP1 amortized-tangent exact verification (full subset DP), five classes ===\n");
    setup(3, q3d); bp1_check("Q3-down");
    setup(3, q3u); bp1_check("Q3-uniform");
    setup(4, q4d); bp1_check("Q4-down");
    setup(4, q4c); bp1_check("Q4-cap");
    setup(4, q4u); bp1_check("Q4-uniform");

    printf("\n=== Floor F(t) spot values at claimed breakpoints (cross-check vs W4) ===\n");
    setup(3, q3d);
    printf("  Q3-down (claimed bps 8, 15, 135/8; F(40)=8):\n");
    F_at(4, 1, "seg1"); F_at(8, 1, "bp1"); F_at(23, 2, "mid"); F_at(15, 1, "bp2");
    F_at(255, 16, "mid"); F_at(135, 8, "bp3"); F_at(40, 1, "plateau");
    setup(3, q3u);
    printf("  Q3-uniform (claimed floor min(2+t/2, 4+t/4, 8); bps 8, 16):\n");
    F_at(4, 1, "seg1"); F_at(8, 1, "bp1"); F_at(12, 1, "mid"); F_at(16, 1, "bp2"); F_at(40, 1, "plateau");
    setup(4, q4c);
    printf("  Q4-cap (claimed bps 10, 16, 160/7; F(40)=10):\n");
    F_at(5, 1, "seg1"); F_at(10, 1, "bp1"); F_at(13, 1, "mid"); F_at(16, 1, "bp2");
    F_at(136, 7, "mid"); F_at(160, 7, "bp3"); F_at(40, 1, "plateau");
    setup(4, q4d);
    printf("  Q4-down (claimed bps 80/9, 16, 80/3; F(40)=10):\n");
    F_at(40, 9, "seg1"); F_at(80, 9, "bp1"); F_at(112, 9, "mid"); F_at(16, 1, "bp2");
    F_at(64, 3, "mid"); F_at(80, 3, "bp3"); F_at(40, 1, "plateau");
    setup(4, q4u);
    printf("  Q4-uniform (claimed bps 32/3, 16, 20, 22; F(40)=10):\n");
    F_at(16, 3, "seg1"); F_at(32, 3, "bp1"); F_at(40, 3, "mid"); F_at(16, 1, "bp2");
    F_at(18, 1, "mid"); F_at(20, 1, "bp3"); F_at(21, 1, "mid"); F_at(22, 1, "bp4"); F_at(40, 1, "plateau");

    printf("\n=== n=5 (Q5-down) structured audits ===\n");
    n5_audit();
    return 0;
}
