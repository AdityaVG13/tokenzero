#include <stdio.h>
#include <stdint.h>
typedef int64_t i64;
static int w[5]={9,4,4,4,4};
static uint32_t colm[5];
static uint16_t el[16], locc[5];
static int8_t ppc[1<<16];
static int16_t Ec[1<<16];
static i64 U[1<<16];
static void build_cell(uint32_t cell,int*csz){
    int k=0;
    for(int x=0;x<32;x++) if(cell&(1u<<x)) el[k++]=(uint16_t)x;
    *csz=k;
    for(int i=0;i<5;i++){uint16_t m=0;for(int j=0;j<k;j++)if((el[j]>>(4-i))&1)m|=(1u<<j);locc[i]=m;}
    int nm=1<<k;
    for(int m=0;m<nm;m++){ppc[m]=(int8_t)__builtin_popcount((unsigned)m);
        int e=0;for(int i=0;i<5;i++){int c1=__builtin_popcount(m&locc[i]);int c0=ppc[m]-c1;e+=w[i]*(c0<c1?c0:c1);}Ec[m]=(int16_t)e;}
}
static void udp(int S,int K,int csz){
    int nm=1<<csz;U[0]=0;
    for(int sz=1;sz<=csz;sz++)for(int m=1;m<nm;m++){if(ppc[m]!=sz)continue;
        i64 best=(i64)S*Ec[m];uint16_t low=(uint16_t)m&(uint16_t)(0u-(uint16_t)m);i64 ms=INT64_MAX;
        for(uint16_t B=(m-1)&m;B;B=(B-1)&m){if(!(B&low))continue;i64 v=U[B]+U[m^B];if(v<ms)ms=v;}
        if(ms!=INT64_MAX){i64 cand=(i64)K*sz+ms;if(cand<best)best=cand;}U[m]=best;}
}
int main(void){
    for(int i=0;i<5;i++){uint32_t m=0;for(int x=0;x<32;x++)if((x>>(4-i))&1)m|=(1u<<x);colm[i]=m;}
    uint32_t opts[16]={0x00017fff,0x0002bfff,0x0004dfff,0x0008efff,0x0010f7ff,0x0020fbff,
        0x0040fdff,0x0080feff,0x0100ff7f,0x0200ffbf,0x0400ffdf,0x0800ffef,0x1000fff7,0x2000fffb,
        0x4000fffd,0x7fff0001};
    int bad=0;
    for(int j=0;j<16;j++){
        for(int side=0;side<2;side++){
            uint32_t cell = side? (~opts[j]&0xFFFFFFFFu) : opts[j];
            int csz; build_cell(cell,&csz);
            int full=(1<<csz)-1; udp(32,158,csz);
            i64 base=32LL*Ec[full];
            if(U[full]!=base){printf("  *** R>0: M=%08x side=%d U=%lld base=%lld\n",opts[j],side,(long long)U[full],(long long)base);bad++;}
        }
    }
    printf("all 16 optimal bipartitions x both sides: %s\n", bad? "EXCESS FOUND":"R=0 everywhere (32/32 cells)");
    return 0;
}
