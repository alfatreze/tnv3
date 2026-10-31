#include <stdio.h>
#include <stdint.h>
#include "tvc.h"
int main(void){RC r;Ctx c;rc_enc_init(&r);ctx_init(&c);MB mb[9]={0};Pic s={0};s.mb=mb;s.mbw=3;s.mbh=3;s.ptype=PIC_I;s.qp=25;s.prev_qp=25;s.aq=1;s.t4ok=1;for(int n=0;n<9;n++){MB *m=&mb[n];m->type=MB_INTRA;m->qp=20+n*3;m->coded=63;m->t4=n%2?5:10;m->s4=0xffff;for(int b=0;b<4;b++)m->imode[b]=(n+b)%4;m->cmode=n%4;int16_t lev[6][64]={{0}};for(int b=0;b<6;b++){if(b<4&&((m->t4>>b)&1)){for(int k=0;k<4;k++)lev[b][16*k]=(n+b+k)%2?-3:2;}else lev[b][0]=(n+b)%2?-3:2;}int pv[2][2]={{0}};tvc_code_mb(&s,&r,&c,n%3,n/3,m,lev,pv);}rc_enc_flush(&r);fwrite(r.out,1,r.outlen,stdout);free(r.out);}
