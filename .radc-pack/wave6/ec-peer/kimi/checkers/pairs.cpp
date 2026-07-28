// pairs.cpp — exact one-bit (two-prototype) codebook enumeration at Θ_n↓ heavy vertex.
// Weights w = (n+4, 4, ..., 4), denominator 5n. Distortion per source x: min(d_w(x,p), d_w(x,q)).
// Enumerates all unordered multisets {p,q}: p <= q, i.e. C(2^n,2)+2^n codebooks.
// Usage: pairs n
// Output: n Npairs Emin Eanti cntMin cntAntipodalType cntNonComplementTie argminExamples...
#include <bits/stdc++.h>
using namespace std;
int main(int argc, char** argv){
    int n = atoi(argv[1]);
    int N = 1<<n;
    vector<long long> w(n); w[0]=n+4; for(int i=1;i<n;i++) w[i]=4;
    // d[x][p]
    vector<vector<int>> d(N, vector<int>(N,0));
    for(int x=0;x<N;x++) for(int p=0;p<N;p++){
        int s=0; for(int i=0;i<n;i++) if(((x>>i)&1)!=((p>>i)&1)) s+=w[i];
        d[x][p]=s;
    }
    long long Emin = LLONG_MAX, Eanti = -1, EminDiag = LLONG_MAX;
    long long cntMin=0, cntAnti=0, cntNonComp=0;
    vector<pair<int,int>> examples;
    int full = N-1;
    for(int p=0;p<N;p++) for(int q=p;q<N;q++){
        long long E=0;
        for(int x=0;x<N;x++) E += min(d[x][p], d[x][q]);
        if(p==0 && q==full) Eanti = E;
        if(p==q){ if(E<EminDiag) EminDiag=E; continue; }
        bool antiType = (q == (p^full));
        if(E < Emin){ Emin=E; cntMin=1; cntAnti=antiType?1:0; cntNonComp=antiType?0:1; examples.clear(); examples.push_back({p,q}); }
        else if(E==Emin){ cntMin++; if(antiType) cntAnti++; else { cntNonComp++; if(examples.size()<8) examples.push_back({p,q}); } }
    }
    printf("n=%d Nstrict=%lld Nmulti=%lld Emin=%lld Eanti=%lld EminDiag=%lld cntMin=%lld cntAntipodalType=%lld cntNonComplementTie=%lld",
           n, (long long)N*(N-1)/2, (long long)N*(N+1)/2, Emin, Eanti, EminDiag, cntMin, cntAnti, cntNonComp);
    for(auto&e:examples) printf(" (%d,%d)", e.first, e.second);
    printf("\n");
    return 0;
}
