/* Compare two-frame propagation with the E-034 full-history algorithm. */
#define main encoder_main
#include "../tvc_enc.c"
#undef main
#include <assert.h>
static void reference(const Look *L, const char *isI, int n, double strength, double **off) {
 int nm=S.mbw*S.mbh; double *prop=calloc((size_t)n*nm,sizeof *prop);
 for(int t=n-1;t>=1;t--) {
  if(isI[t])continue;
  for(int i=0;i<nm;i++) {
   int x=i%S.mbw,y=i/S.mbw;size_t k=(size_t)t*nm+i;
   int mx=L->mvx[k],my=L->mvy[k];double in=L->intra[k],inter=L->inter[k];
   double amount=(in+prop[(size_t)t*nm+i])*(1.0-inter/in);if(amount<=0)continue;
   int sh=S.qpel?2:1,px=x*16+(mx>>sh),py=y*16+(my>>sh);
   int bx0=(int)floor(px/16.0),by0=(int)floor(py/16.0);
   for(int dy=0;dy<2;dy++)for(int dx=0;dx<2;dx++){
    int bx=bx0+dx,by=by0+dy;if(bx<0||by<0||bx>=S.mbw||by>=S.mbh)continue;
    int ox=16-abs(px-bx*16),oy=16-abs(py-by*16);if(ox<=0||oy<=0)continue;
    prop[(size_t)(t-1)*nm+by*S.mbw+bx]+=amount*ox*oy/256.0;
   }
  }
 }
 for(int t=0;t<n;t++)for(int i=0;i<nm;i++){
  double in=L->intra[(size_t)t*nm+i];off[t][i]=-strength*log2((in+prop[(size_t)t*nm+i])/in);
 }
 free(prop);
}
static unsigned seed=73;static unsigned rnd(void){seed=seed*1664525u+1013904223u;return seed;}
int main(void){
 S.mbw=7;S.mbh=5;int nm=S.mbw*S.mbh;
 for(int trial=0;trial<200;trial++){
  int n=1+(int)(rnd()%40);size_t len=(size_t)n*nm;S.qpel=trial%2;
  Look L={malloc(len*sizeof(double)),malloc(len*sizeof(double)),malloc(len*sizeof(int)),malloc(len*sizeof(int))};
  char *key=calloc(n,1);double **a=malloc((size_t)n*sizeof *a),**b=malloc((size_t)n*sizeof *b);
  for(int t=0;t<n;t++){key[t]=(rnd()%9)==0;a[t]=malloc((size_t)nm*sizeof **a);b[t]=malloc((size_t)nm*sizeof **b);}
  for(size_t k=0;k<len;k++){L.intra[k]=64+(rnd()%12000);L.inter[k]=L.intra[k]*(rnd()%1001)/1000.0;L.mvx[k]=(int)(rnd()%129)-64;L.mvy[k]=(int)(rnd()%129)-64;}
  reference(&L,key,n,2.0,a);tree_offsets(&L,key,n,2.0,b);
  for(int t=0;t<n;t++){assert(!memcmp(a[t],b[t],(size_t)nm*sizeof **a));free(a[t]);free(b[t]);}
  free(a);free(b);free(key);free(L.intra);free(L.inter);free(L.mvx);free(L.mvy);
 }
 puts("PASS 200 randomized look-ahead sequences; offsets bit-identical");return 0;
}
