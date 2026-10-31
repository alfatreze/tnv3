#include <stdio.h>
#include <stdint.h>
#include "tvc.h"
int main(void) { RC rc; Ctx c; ctx_init(&c); rc_enc_init(&rc); uint32_t x=163239u; for(int n=0;n<300;n++) {int cc=n%3, N=cc==2?16:64; int16_t lev[64]={0};for(int i=0;i<N;i++){x=x*1664525u+1013904223u; lev[i]=((x>>24)%7==0)?(int16_t)(((x>>8)%601)-300):0;}lev[n%N]=n%2?-2:1;code_block_n(&rc,&c,cc,lev,N);} rc_enc_flush(&rc);fwrite(rc.out,1,rc.outlen,stdout);free(rc.out); }
