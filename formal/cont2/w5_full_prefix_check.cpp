#include <algorithm>
#include <array>
#include <cassert>
#include <cstdint>
#include <iostream>
#include <limits>
#include <string>
#include <vector>

using i128 = __int128_t;

static i128 abs128(i128 x) { return x < 0 ? -x : x; }
static i128 gcd128(i128 a, i128 b) {
    a = abs128(a); b = abs128(b);
    while (b != 0) { i128 t = a % b; a = b; b = t; }
    return a;
}
static i128 pow128(i128 a, int e) {
    i128 r = 1;
    while (e-- > 0) r *= a;
    return r;
}
static std::string s128(i128 x) {
    if (x == 0) return "0";
    bool neg = x < 0;
    if (neg) x = -x;
    std::string s;
    while (x > 0) {
        s.push_back(static_cast<char>('0' + static_cast<int>(x % 10)));
        x /= 10;
    }
    if (neg) s.push_back('-');
    std::reverse(s.begin(), s.end());
    return s;
}

struct Rat {
    i128 n{0}, d{1};
    Rat() = default;
    Rat(i128 nn) : n(nn), d(1) {}
    Rat(i128 nn, i128 dd) : n(nn), d(dd) { norm(); }
    void norm() {
        assert(d != 0);
        if (d < 0) { n = -n; d = -d; }
        i128 g = gcd128(n, d);
        if (g != 0) { n /= g; d /= g; }
    }
};
static Rat operator+(const Rat& a, const Rat& b) { return Rat(a.n*b.d+b.n*a.d, a.d*b.d); }
static Rat operator-(const Rat& a, const Rat& b) { return Rat(a.n*b.d-b.n*a.d, a.d*b.d); }
static Rat operator*(const Rat& a, const Rat& b) { return Rat(a.n*b.n, a.d*b.d); }
static bool operator==(const Rat& a, const Rat& b) { return a.n == b.n && a.d == b.d; }
static bool operator<(const Rat& a, const Rat& b) { return a.n*b.d < b.n*a.d; }
static bool operator>(const Rat& a, const Rat& b) { return b < a; }
static bool operator>=(const Rat& a, const Rat& b) { return !(a < b); }
static std::string sr(const Rat& x) { return s128(x.n) + "/" + s128(x.d); }

static int C[17][17];
static void build_min_path_dp() {
    const int INF = 1'000'000;
    for (int N=0; N<=16; ++N) for (int r=0; r<=16; ++r) C[N][r]=INF;
    for (int N=1; N<=16; ++N) C[N][1]=0;
    for (int N=1; N<=16; ++N) {
        for (int r=2; r<=N; ++r) {
            for (int a=1; a<N; ++a) {
                int b=N-a;
                for (int r1=1; r1<r; ++r1) {
                    int r2=r-r1;
                    if (r1<=a && r2<=b) {
                        C[N][r]=std::min(C[N][r], N+C[a][r1]+C[b][r2]);
                    }
                }
            }
        }
    }
}

static Rat subset_moment(const std::array<int,4>& w, int m) {
    int W=0; for (int x:w) W+=x;
    i128 num=0;
    for (int mask=0; mask<16; ++mask) {
        int z=0;
        for (int i=0;i<4;++i) if ((mask>>i)&1) z+=w[i];
        num += pow128(z,m);
    }
    return Rat(num, static_cast<i128>(16)*pow128(W,m));
}
static Rat gap0(const std::array<int,4>& w, int m) {
    return Rat(39-2*m) - Rat(40)*subset_moment(w,m);
}

int main() {
    build_min_path_dp();
    const std::array<int,16> expected = {0,16,18,21,24,28,32,36,40,45,50,53,56,60,62,64};
    for (int r=1;r<=16;++r) assert(C[16][r]==expected[r-1]);

    Rat p10 = Rat(1) - Rat(pow128(3,10),pow128(5,10)) - Rat(3)*Rat(pow128(4,10),pow128(5,10));
    assert(p10 == Rat(6560848,9765625));

    const std::array<int,7> cr = {0,0,16,18,21,24,28};
    const std::array<Rat,7> expB = {
        Rat(), Rat(),
        Rat(10769686,1953125),
        Rat(97023471,15625000),
        Rat(252888283,31250000),
        Rat(38966203,3906250),
        Rat(20384017,1562500)
    };
    std::array<Rat,7> B;
    for (int r=2;r<=6;++r) {
        B[r]=Rat(19*cr[r],16)-Rat(37)+Rat(40)*p10*Rat(16-r,16);
        assert(B[r]==expB[r]);
        assert(B[r]>Rat(1));
    }

    const std::array<int,4> down={2,1,1,1};
    const std::array<int,4> cap={3,3,2,2};
    assert(gap0(down,17)==Rat(71088276063LL,30517578125LL));
    assert(gap0(down,18)==Rat(static_cast<i128>(277615146191LL),static_cast<i128>(762939453125LL)));
    assert(gap0(cap,17)==Rat(static_cast<i128>(475055717444931LL),static_cast<i128>(200000000000000LL)));
    assert(gap0(cap,18)==Rat(static_cast<i128>(20074685943080277LL),static_cast<i128>(50000000000000000LL)));
    assert(gap0(down,17)>Rat(1) && gap0(down,18)>Rat(0) && gap0(down,18)<Rat(1));
    assert(gap0(cap,17)>Rat(1) && gap0(cap,18)>Rat(0) && gap0(cap,18)<Rat(1));

    for (int m=10;m<=17;++m) {
        assert(static_cast<i128>(20)*pow128(m,m) < pow128(m+1,m+1));
        assert(gap0(down,m)>gap0(down,m+1));
        assert(gap0(cap,m)>gap0(cap,m+1));
    }

    for (int m=10;m<=18;++m) {
        Rat pm = Rat(1)-Rat(pow128(3,m),pow128(5,m))-Rat(3)*Rat(pow128(4,m),pow128(5,m));
        assert(pm>=p10);
        for (int r=2;r<=6;++r) {
            Rat g = Rat((m+1)*cr[r],16)-Rat(2*m+1)+Rat(40)*pm*Rat(16-r,16);
            assert(g>=B[r]);
        }
    }

    assert(Rat(73,2)-Rat(2*19)==Rat(-3,2));

    std::cout << "PASS independent C++ exact certificate\n";
    std::cout << "C_16(r):";
    for (int r=1;r<=16;++r) std::cout << ' ' << C[16][r];
    std::cout << "\np10 " << sr(p10) << "\n";
    for (int r=2;r<=6;++r) std::cout << "B_" << r << ' ' << sr(B[r]) << "\n";
    std::cout << "down17 " << sr(gap0(down,17)) << "\n";
    std::cout << "down18 " << sr(gap0(down,18)) << "\n";
    std::cout << "cap17 " << sr(gap0(cap,17)) << "\n";
    std::cout << "cap18 " << sr(gap0(cap,18)) << "\n";
    std::cout << "m>=19 obstruction at m=19: -3/2\n";
    return 0;
}
