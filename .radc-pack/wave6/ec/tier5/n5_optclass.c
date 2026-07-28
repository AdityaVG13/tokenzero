#include <stdio.h>
#include <stdint.h>
static int w[5] = {9,4,4,4,4};
static uint32_t colm[5];
static inline int E_of(uint32_t mask, int sz){
    int e=0;
    for(int i=0;i<5;i++){
        int c1=__builtin_popcount(mask&colm[i]);
        int c0=sz-c1;
        e+=w[i]*(c0<c1?c0:c1);
    }
    return e;
}
int main(void){
    for(int i=0;i<5;i++){uint32_t m=0;for(int x=0;x<32;x++)if((x>>(4-i))&1)m|=(1u<<x);colm[i]=m;}
    uint32_t full=0xFFFFFFFFu;
    /* precompute the 16 ball masks (radius<=2 around each center, canonical: source0 in M) */
    uint32_t balls[32]; int nb=0;
    for(int c=0;c<32;c++){
        uint32_t B=0;
        for(int x=0;x<32;x++)if(__builtin_popcount(x^c)<=2)B|=(1u<<x);
        if(B&1u){ balls[nb++]=B; } /* canonical rep contains source 0 */
    }
    int opt_total=0, opt_ball=0;
    for(uint32_t M=1;M<full;M+=2){
        int pc=__builtin_popcount(M);
        int e=E_of(M,pc)+E_of(full^M,32-pc);
        if(e==242){
            opt_total++;
            int isb=0;
            for(int j=0;j<nb;j++) if(balls[j]==M){isb=1;break;}
            if(isb)opt_ball++;
            else {
                int wc=-1;
                printf("  NON-BALL optimal M=%08x |M|=%d\n", M, pc);
            }
        }
    }
    printf("optimal bipartitions=%d, of which balls=%d, distinct canonical balls=%d\n", opt_total, opt_ball, nb);
    return 0;
}
