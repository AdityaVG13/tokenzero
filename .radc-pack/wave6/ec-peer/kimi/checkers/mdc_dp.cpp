// W5-MDC exact two-demand prefix-tree DP (exact integers, no floats)
// X ~ Unif({0,1}^4), two iid demands S1,S2 ~ theta.
// Leaf prototype error per source: e2(p,x) = 1 - (sum_s w_s 1[p_s=x_s])^2 / S
//   down-vertex: w=(2,1,1,1), S=25  (theta in fifths)
//   cap-vertex:  w=(2,2,3,3), S=100 (theta in tenths)
// Leaf cost E2[A] = min_p sum_{x in A} (S - m(p,x)^2)   [integer]
// Scaled DP:  val[A] = min( tn*E2[A], td*k*S*|A| + min_splits val[B]+val[A\B] )
//   t = tn/td ; floor = base + val[full]/(16*S*td)
//   batch: base=2, k=2 (F2_batch, H2) ; seq M-front: base=3, k=3 (G2)
#include <bits/stdc++.h>
using namespace std;
typedef long long ll;
int w[4]; ll S;
int E2[1<<16];
ll val[1<<16];
int choiceB[1<<16]; // 0 = leaf, else split partner B (contains lowbit)

int mval(int p,int x){
    int m=0;
    for(int i=0;i<4;i++) if( ((p>>i)&1) == ((x>>i)&1) ) m += w[i];
    return m;
}

int main(int argc, char** argv){
    // args: vertex(0=down,1=cap) k tn td
    int vertex = atoi(argv[1]);
    int k = atoi(argv[2]);
    int tn = atoi(argv[3]);
    int td = atoi(argv[4]);
    if(vertex==0){ w[0]=2; w[1]=1; w[2]=1; w[3]=1; S=25; }
    else { w[0]=2; w[1]=2; w[2]=3; w[3]=3; S=100; }
    // leaf costs
    for(int A=1; A<(1<<16); A++){
        int best = INT_MAX;
        for(int p=0;p<16;p++){
            int s=0;
            for(int x=0;x<16;x++) if(A>>x & 1){
                int m=mval(p,x); s += (int)(S - (ll)m*m);
            }
            if(s<best) best=s;
        }
        E2[A]=best;
    }
    // DP over increasing A
    for(int A=1; A<(1<<16); A++){
        ll leaf = (ll)tn * E2[A];
        ll best = leaf; int bB=0;
        ll internal = (ll)td * k * S * __builtin_popcount(A);
        int low = A & (-A);
        for(int B=(A-1)&A; B>0; B=(B-1)&A){
            if(!(B & low)) continue;
            ll v = internal + val[B] + val[A^B];
            if(v < best){ best=v; bB=B; }
        }
        val[A]=best; choiceB[A]=bB;
    }
    // backtrack on full set
    ll ell_num=0, e2_num=0;
    // stack traversal
    int st[20], sp=0; st[sp++]=(1<<16)-1;
    while(sp){
        int A=st[--sp];
        if(choiceB[A]==0){ e2_num += E2[A]; }
        else { ell_num += __builtin_popcount(A); st[sp++]=choiceB[A]; st[sp++]=A^choiceB[A]; }
    }
    // floor numerator: base + val/(16*S*td) ; also ell=ell_num/16, e2=e2_num/(16*S)
    int base = (k==2)?2:3;
    // print: t=tn/td, val, ell_num, e2_num, denom checks
    printf("t=%d/%d k=%d vtx=%d val=%lld ellnum=%lld e2num=%lld S=%lld base=%d\n",
           tn,td,k,vertex,val[(1<<16)-1],ell_num,e2_num,S,base);
    return 0;
}
