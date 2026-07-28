#include <algorithm>
#include <array>
#include <cstdint>
#include <iostream>
#include <string>
#include <vector>
using namespace std;
using i128=__int128_t;
static string s128(i128 x){if(x==0)return"0";bool neg=x<0;if(neg)x=-x;string s;while(x){s.push_back(char('0'+x%10));x/=10;}if(neg)s.push_back('-');reverse(s.begin(),s.end());return s;}
static i128 ipow(i128 a,int e){i128 r=1;while(e--){r*=a;}return r;}
int main(int argc,char**argv){
  // n, m demands, alpha length coefficient, rho_num, rho_den, weights...
  if(argc<7){cerr<<"usage n m alpha P Q weights...\n";return 2;}
  int n=stoi(argv[1]),m=stoi(argv[2]),alpha=stoi(argv[3]); long long P=stoll(argv[4]),Q=stoll(argv[5]);
  vector<long long>w(n);long long W=0;for(int i=0;i<n;i++){w[i]=stoll(argv[6+i]);W+=w[i];}
  int N=1<<n; unsigned SZ=1u<<N; if(N>20)return 3;
  vector<int>pc(SZ);for(unsigned A=1;A<SZ;A++)pc[A]=pc[A>>1]+(A&1);
  vector<array<unsigned,2>> cmask(n);for(int i=0;i<n;i++)for(int a=0;a<2;a++){unsigned z=0;for(int x=0;x<N;x++)if(((x>>i)&1)==a)z|=1u<<x;cmask[i][a]=z;}
  vector<i128> prev(SZ),cur(SZ);for(unsigned A=0;A<SZ;A++)prev[A]=pc[A];
  for(int t=1;t<=m;t++){
    cur[0]=0;
    for(unsigned A=1;A<SZ;A++){
      i128 z=0;for(int i=0;i<n;i++)z+=(i128)w[i]*max(prev[A&cmask[i][0]],prev[A&cmask[i][1]]);cur[A]=z;
    }
    swap(prev,cur);
  }
  i128 Wm=ipow(W,m);vector<i128>E(SZ);for(unsigned A=0;A<SZ;A++)E[A]=Wm*pc[A]-prev[A];
  // Objective alpha*L/N + (P/Q)*E/(N W^m); scale by Q W^m.
  i128 qwm=(i128)Q*Wm;
  vector<i128>V(SZ),Et(SZ);vector<unsigned>Bchoice(SZ);long long splits=0;
  vector<vector<unsigned>>by(N+1);for(unsigned A=0;A<SZ;A++)by[pc[A]].push_back(A);
  for(int k=0;k<=N;k++)for(unsigned A:by[k]){
    i128 best=(i128)P*E[A],beste=E[A];unsigned bc=0;
    if(k>=2){unsigned low=A&-A,R=A^low;i128 base=(i128)alpha*pc[A]*qwm;
      for(unsigned s=R;;s=(s-1)&R){if(s!=R){unsigned B=s|low,C=A^B;splits++;i128 z=base+V[B]+V[C],ee=Et[B]+Et[C];if(z<best||(z==best&&ee<beste)){best=z;beste=ee;bc=B;}}if(s==0)break;}
    }
    V[A]=best;Et[A]=beste;Bchoice[A]=bc;
  }
  unsigned full=SZ-1; long long Ltot=0; i128 EE=0;vector<unsigned>st{full};int leaves=0;
  while(!st.empty()){unsigned A=st.back();st.pop_back();unsigned B=Bchoice[A];if(!B){EE+=E[A];leaves++;}else{Ltot+=pc[A];st.push_back(B);st.push_back(A^B);}}
  // full scalar value including constant alpha: alpha + alpha L/N + rho E/(N W^m)
  // numerator over N*Q*W^m = alpha*N*QWm + V(full)
  i128 num=(i128)alpha*N*qwm+V[full], den=(i128)N*qwm;
  cout<<"num "<<s128(num)<<" den "<<s128(den)<<" Ltot "<<Ltot<<" E "<<s128(EE)<<" leaves "<<leaves<<" splits "<<splits<<"\n";
}
