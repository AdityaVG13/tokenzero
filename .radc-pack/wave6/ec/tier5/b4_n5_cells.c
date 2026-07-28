/* W6 Tier-5 B4: n=5 Q5-down cell-excess audit.
 * R(C) = max over full subtrees of C of [G - s1*d*L]  (the "excess").
 * R(C) = 0  <=>  U(C) = S*E(C) in the U-DP at slope s1 (scaled K/S, K=158, S=32).
 * Cells checked:
 *  - the 3 symmetry types of OPTIMAL one-bit code sides (balls radius<=2):
 *    centers 0^5 (type I), e_heavy (type II), e_light (type III) [complements
 *    are balls around antipodes, same types];
 *  - antipodal-rich non-subcube cells {x0=x1} (16 elts), {x0=x1=x2} (8 elts),
 *    a complement-closed "paired" cell, and a half-cube subcube (control).
 * Also: histogram of one-bit split enumerators E(B)+E(C) near the optimum
 * (slack distribution), full 2^31 enumeration.
 */
#include <stdio.h>
#include <stdint.h>
#include <string.h>

typedef int64_t i64;
static int w[5] = {9, 4, 4, 4, 4};
#define D 25
static uint32_t colm[5];

static inline int E_of(uint32_t mask, int sz) {
    int e = 0;
    for (int i = 0; i < 5; i++) {
        int c1 = __builtin_popcount(mask & colm[i]);
        int c0 = sz - c1;
        e += w[i] * (c0 < c1 ? c0 : c1);
    }
    return e;
}

/* U-DP over submasks of a 16-bit-local cell */
static uint16_t el[16];         /* element source indices */
static uint16_t locc[5];        /* local coord masks */
static int8_t ppc[1 << 16];
static int16_t Ec[1 << 16];
static i64 U[1 << 16];

static void build_cell(uint32_t cell, int *csz) {
    int k = 0;
    for (int x = 0; x < 32; x++) if (cell & (1u << x)) el[k++] = (uint16_t)x;
    *csz = k;
    for (int i = 0; i < 5; i++) {
        uint16_t m = 0;
        for (int j = 0; j < k; j++) if ((el[j] >> (4 - i)) & 1) m |= (1u << j);
        locc[i] = m;
    }
    int nm = 1 << k;
    for (int m = 0; m < nm; m++) {
        ppc[m] = (int8_t)__builtin_popcount((unsigned)m);
        int e = 0;
        for (int i = 0; i < 5; i++) {
            int c1 = __builtin_popcount(m & locc[i]);
            int c0 = ppc[m] - c1;
            e += w[i] * (c0 < c1 ? c0 : c1);
        }
        Ec[m] = (int16_t)e;
    }
}

static void udp(int S, int K, int csz) {
    int nm = 1 << csz;
    U[0] = 0;
    for (int sz = 1; sz <= csz; sz++)
        for (int m = 1; m < nm; m++) {
            if (ppc[m] != sz) continue;
            i64 best = (i64)S * Ec[m];
            uint16_t low = (uint16_t)m & (uint16_t)(0u - (uint16_t)m);
            i64 ms = INT64_MAX;
            for (uint16_t B = (m - 1) & m; B; B = (B - 1) & m) {
                if (!(B & low)) continue;
                i64 v = U[B] + U[m ^ B];
                if (v < ms) ms = v;
            }
            if (ms != INT64_MAX) {
                i64 cand = (i64)K * sz + ms;
                if (cand < best) best = cand;
            }
            U[m] = best;
        }
}

static void check_cell(const char *name, uint32_t cell) {
    int csz;
    build_cell(cell, &csz);
    if (csz > 16) { printf("  %-28s |C|=%d > 16, skipped\n", name, csz); return; }
    int full = (1 << csz) - 1;
    udp(32, 158, csz);
    i64 base = 32LL * Ec[full];
    printf("  %-28s |C|=%2d E=%3d : U_{s1}(C)=%lld vs 32*E=%lld -> R(C)%s ; ",
           name, csz, Ec[full], (long long)U[full], (long long)base,
           U[full] == base ? "=0" : ">0 ***");
    udp(32, 157, csz);
    printf("tightness: U_{s1-1/800}(C)=%lld %s\n", (long long)U[full],
           U[full] < base ? "(excess appears below s1: consistent)" : "(no excess even below s1?!)");
}

int main(void) {
    for (int i = 0; i < 5; i++) {
        uint32_t m = 0;
        for (int x = 0; x < 32; x++) if ((x >> (4 - i)) & 1) m |= (1u << x);
        colm[i] = m;
    }
    printf("=== n=5 cell-excess audit at slope s1=79/400 (K=158,S=32) ===\n");
    /* optimal one-bit sides: balls radius<=2 */
    int centers[3] = {0, 16, 8}; /* 0^5, e_heavy, e_light */
    const char *tn[3] = {"Ball(0^5,<=2) typeI", "Ball(e_heavy,<=2) typeII", "Ball(e_light,<=2) typeIII"};
    for (int t = 0; t < 3; t++) {
        uint32_t cell = 0;
        for (int x = 0; x < 32; x++) if (__builtin_popcount((unsigned)(x ^ centers[t])) <= 2) cell |= (1u << x);
        check_cell(tn[t], cell);
        check_cell("  complement (coball)", ~cell & 0xFFFFFFFFu);
    }
    /* antipodal-rich cells */
    uint32_t c1 = 0, c2 = 0, c3 = 0, c4 = 0;
    for (int x = 0; x < 32; x++) {
        if (((x >> 4) & 1) == ((x >> 3) & 1)) c1 |= (1u << x);            /* x0=x1 */
        if ((((x >> 4) & 1) == ((x >> 3) & 1)) && (((x >> 3) & 1) == ((x >> 2) & 1))) c2 |= (1u << x); /* x0=x1=x2 */
        if ((x & 1) == 0) c3 |= (1u << x);                                /* subcube control x4=0 */
    }
    /* complement-closed paired cell: {0,31} U {x: x2=0, x not in previous}... build 8 antipodal pairs */
    uint32_t used = 0;
    for (int x = 0; x < 32 && __builtin_popcount(c4) < 16; x++) {
        if (used & (1u << x)) continue;
        c4 |= (1u << x) | (1u << (x ^ 31));
        used |= (1u << x) | (1u << (x ^ 31));
    }
    check_cell("{x0=x1} (anti-closed)", c1);
    check_cell("{x0=x1=x2} (anti-closed)", c2);
    check_cell("subcube x4=0 (control)", c3);
    check_cell("8 antipodal pairs (built)", c4);

    printf("\n=== one-bit split enumerator histogram near optimum (full 2^31) ===\n");
    uint32_t full = 0xFFFFFFFFu;
    static i64 hist[401]; /* E ranges 0..400 */
    memset(hist, 0, sizeof hist);
    int best = 1 << 30;
    for (uint32_t M = 1; M < full; M += 2) {
        int pc = __builtin_popcount(M);
        int e = E_of(M, pc) + E_of(full ^ M, 32 - pc);
        if (e < best) best = e;
        if (e <= 400) hist[e]++;
    }
    printf("  E1min = %d (s1*d*2^n = 400-242 = 158 slack=0)\n", best);
    for (int e = best; e <= best + 20 && e <= 400; e++)
        if (hist[e]) printf("    E=%3d (slack %3d): %lld bipartitions\n", e, e - best, (long long)hist[e]);
    return 0;
}
