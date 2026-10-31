/* Differential oracle: greedy RDOQ snapshot from 568ecb9a, using the unchanged
 * full coefficient counter. New prefix costs are checked against full replay. */
#define TVC_VERIFY_RDOQ 1
#define main encoder_main
#include "../tvc_enc.c"
#undef main
#include <assert.h>
static int rdoq_reference(const double *cs, int16_t *lev, int N, int qp, int cc, double lam) {
    double cur = N == 64 ? block_bits(cc, lev) : block4_bits(lev);
    int nz = 0;
    for (int i = N - 1; i >= 0; i--) {
        if (!lev[i]) continue;
        int a = abs(lev[i]), sg = lev[i] < 0;
        double c = fabs(cs[i]), d_old = c - dequant(a, qp), d_new = c - (a > 1 ? dequant(a - 1, qp) : 0);
        int16_t keep = lev[i];
        lev[i] = (int16_t)(sg ? -(a - 1) : a - 1);
        int any = 0; for (int k = 0; k < N && !any; k++) any = lev[k] != 0;
        if (!any) { lev[i] = keep; continue; }       /* the caller decides about an empty block */
        double nb = N == 64 ? block_bits(cc, lev) : block4_bits(lev);
        if (d_new * d_new - d_old * d_old + lam * (nb - cur) < 0) cur = nb; else lev[i] = keep;
    }
    for (int k = 0; k < N; k++) nz += lev[k] != 0;
    return nz;
}

static uint32_t seed=19;
static uint32_t rnd(void){seed=1664525u*seed+1013904223u;return seed;}
int main(void){
 tvc_init();tvc_cost_init();
 unsigned blocks=0;
 for(int cc=0;cc<3;cc++)for(int test=0;test<4000;test++){
  int n=cc==2?16:64,qp=test%52,pattern=(test/52)%8;
  uint16_t *p=(uint16_t*)&S.ctx;
  for(size_t j=0;j<sizeof S.ctx/sizeof *p;j++)p[j]=(uint16_t)(1+rnd()%32767);
  Ctx before=S.ctx;
  int16_t a[64],b[64];double cs[64];int any=0;
  for(int i=0;i<64;i++){
   a[i]=b[i]=(int16_t)0x2345;cs[i]=0;
   if(i>=n)continue;
   int v=pattern==0?((i<n/2)?1:0):pattern==1?(i==n-1?1:0):pattern==2?((rnd()%5)?0:1):pattern==3?(1+rnd()%32):pattern==4?((i&1)?0:1+rnd()%3):pattern==5?((rnd()%4)?0:30000):pattern==6?1:((rnd()%2)?0:1+rnd()%100);
   if(rnd()&1)v=-v;a[i]=b[i]=(int16_t)v;any|=v!=0;
   double mag=abs(dequant(abs(v),qp));
   cs[i]=pattern==0||pattern==6?0:mag+((int)(rnd()%65)-32)*(TVC_QS[qp]/16.0)/32;
   if(v<0)cs[i]=-cs[i];
  }
  if(!any)a[0]=b[0]=1;
  int old=rdoq_reference(cs,a,n,qp,cc,lambda_of(qp));
  assert(!memcmp(&before,&S.ctx,sizeof before));
  int fresh=rdoq(cs,b,n,qp,cc,lambda_of(qp));
  assert(old==fresh);assert(!memcmp(a,b,sizeof a));assert(!memcmp(&before,&S.ctx,sizeof before));blocks++;
 }
 puts("PASS 12000 differential RDOQ blocks; every trial bit cost equals full replay; contexts unchanged");
 assert(blocks==12000);return 0;
}
