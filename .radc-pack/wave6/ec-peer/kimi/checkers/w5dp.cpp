// w5dp.cpp — exact prefix-tree subset DP for RADC Wave-5 certificates.
// G_{θ,t}(A) = min{ t·Leaf(A), c·|A| + min_{∅≠B⊊A, min(A)∈B} [G(B)+G(A∖B)] }
// mode 1 (leaf1): Leaf(A) = E_θ(A) = (1/W)·Σ_i w_i·min{N_i0,N_i1}         [Wave-4]
// mode 2 (leaf2): Leaf(A) = E2_θ(A) = min_p Σ_{x∈A} [1 − (Σ_s θ_s·1[p_s=x_s])²]  [Wave-5 two-demand]
// All arithmetic exact integers in units of 1/scale, scale = Q·W^k / gcd(P, Q·W^k), t=P/Q.
// Tie-break: among equal primary values, prefer smaller total leaf error E (lexicographic),
// which makes the witness at t=0 the envelope's first segment and witnesses at breakpoints canonical.
// Usage: w5dp mode n c P Q w1 ... wn
// Output: V D Eint splitCount scale   (G(Ω)=V/scale; policy line: b=a+c·D/2^n, slope=Eint/(W^k·2^n))
#include <bits/stdc++.h>
using namespace std;
typedef __int128 i128;

static i128 parse128(const string& s){ i128 v=0; size_t i=0; bool neg=false; if(s[0]=='-'){neg=true;i=1;} for(;i<s.size();i++) v=v*10+(s[i]-'0'); return neg?-v:v; }
static string str128(i128 v){ if(v==0) return "0"; bool neg=v<0; if(neg) v=-v; string s; while(v>0){s.push_back(char('0'+int(v%10))); v/=10;} if(neg) s.push_back('-'); reverse(s.begin(),s.end()); return s; }
static long long gcdll(long long a,long long b){ while(b){ long long t=a%b; a=b; b=t;} return a; }

int main(int argc, char** argv){
    int mode = atoi(argv[1]);
    int n = atoi(argv[2]);
    long long ccoef = atoll(argv[3]);
    i128 P = parse128(argv[4]);
    long long Q = atoll(argv[5]);
    vector<long long> w(n);
    long long W = 0;
    for(int i=0;i<n;i++){ w[i]=atoll(argv[6+i]); W+=w[i]; }
    // NOTE: W here is the *denominator* of θ (weights given as integers summing to W).
    // For θ given as w_i/W with W = sum w_i. We require sum w_i = W by construction.
    long long Wk = (mode==1)? W : W*W; // leaf denominator
    int N = 1<<n;

    // scale: units of 1/scale where scale = Q*Wk/gcd(P mod stuff)
    // reduce P/(Q*Wk): g = gcd(P, Q*Wk) using long long if possible; fall back to i128 gcd.
    // Q, Wk are long long; P may be i128 but in practice fits long long.
    long long Pl = (long long)P;
    assert((i128)Pl == P && "P must fit long long");
    long long QWk = Q * Wk;
    long long g = gcdll( (Pl%QWk+QWk)%QWk, QWk);
    long long scale = QWk / g;           // unit denominator
    long long Pq = Pl / g;               // leaf multiplier: t*Leaf = Pq * M(A) / scale
    // split cost in units: ccoef*|A| * scale

    // ---- precompute leaf numerators M(A) ----
    // mode1: M(A) = Σ_i w_i min(N_i1(A), |A|-N_i1(A))
    // mode2: M2(A) = |A|*W^2 - max_p Σ_{x∈A} m(p,x)^2, m(p,x)=Σ_i w_i·[bit_i(p)==bit_i(x)]
    unsigned int SZpre = 1u<<N;
    vector<long long> M(SZpre, 0);
    vector<int> popc(SZpre,0);
    for(unsigned int A=1;A<SZpre;A++) popc[A]=popc[A>>1]+(A&1);
    if(mode==1){
        // A indexes subsets of an n-element source set: A < 2^N. For n=4, A < 2^16 (fits uint32).
        for(int i=0;i<n;i++){
            unsigned int maski = 0;
            for(int x=0;x<N;x++) if((x>>i)&1) maski |= (1u<<x);
            for(unsigned int A=0;A<(1u<<N);A++){
                int c1 = __builtin_popcount(A & maski);
                int c0 = popc[A] - c1;
                M[A] += w[i] * min(c0,c1);
            }
        }
    } else {
        // m2[p][x]
        vector<vector<long long>> m2(N, vector<long long>(N,0));
        for(int p=0;p<N;p++) for(int x=0;x<N;x++){
            long long m=0; for(int i=0;i<n;i++) if(((p>>i)&1)==((x>>i)&1)) m+=w[i];
            m2[p][x]=m*m;
        }
        // S(p,A)=Σ_{x∈A} m2[p][x]; iterate members
        for(unsigned int A=0;A<(1u<<N);A++){
            long long best=0;
            for(int p=0;p<N;p++){
                long long s=0;
                unsigned int B=A;
                while(B){ int x=__builtin_ctz(B); B&=B-1; s+=m2[p][x]; }
                if(s>best) best=s;
            }
            M[A] = (long long)popc[A]*W*W - best;
        }
    }

    // ---- DP over subsets by increasing popcount ----
    unsigned int SZ = 1u<<N;
    vector<i128> bV(SZ); vector<long long> bE(SZ); vector<unsigned int> bB(SZ); // bB: 0 = leaf, else split mask B
    long long splitCount = 0;
    // order subsets by popcount
    vector<vector<unsigned int>> bypop(N+1);
    for(unsigned int A=0;A<SZ;A++) bypop[__builtin_popcount(A)].push_back(A);
    for(int k=0;k<=N;k++){
        for(unsigned int A : bypop[k]){
            // leaf option
            i128 v = (i128)Pq * M[A];
            long long e = M[A];
            unsigned int bb = 0;
            if(k>=2){
                unsigned int low = A & (-(int)A); // least element (lowest set bit)
                unsigned int R = A ^ low;
                i128 splitBase = (i128)ccoef * popc[A] * scale;
                for(unsigned int S = R; ; S = (S-1)&R){
                    if(S==R) { if(S==0) break; else continue; } // skip B==A
                    unsigned int B = S | low;
                    unsigned int AminusB = A ^ B;
                    splitCount++;
                    i128 vv = splitBase + bV[B] + bV[AminusB];
                    long long ee = bE[B] + bE[AminusB];
                    if(vv < v || (vv==v && ee < e)){ v=vv; e=ee; bb=B; }
                    if(S==0) break;
                }
            }
            bV[A]=v; bE[A]=e; bB[A]=bb;
        }
    }
    // ---- witness walk ----
    long long D=0, Eint=0;
    vector<unsigned int> st; st.push_back(SZ-1);
    while(!st.empty()){
        unsigned int A=st.back(); st.pop_back();
        if(bB[A]==0){ Eint += M[A]; }
        else { D += popc[A]; st.push_back(bB[A]); st.push_back(A ^ bB[A]); }
    }
    // sanity: Eint must equal bE[SZ-1]
    assert(Eint == bE[SZ-1]);
    printf("%s %lld %lld %lld %lld\n", str128(bV[SZ-1]).c_str(), D, Eint, splitCount, scale);
    return 0;
}
