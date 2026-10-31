#define main encoder_main
#include "../tvc_enc.c"
#undef main
#include <assert.h>
int main(void) {
 uint8_t aa[16*37+16],bb[16*41+16];
 for(int offset=0;offset<16;offset++) {
  memset(aa,0,sizeof aa);memset(bb,255,sizeof bb);
  assert(enc_sad16_pixels(aa+offset,37,bb+offset,41)==65280);
  assert(enc_sad16_pixels(bb+offset,41,aa+offset,37)==65280);
  assert(enc_sad16_pixels(aa+offset,37,aa+offset,37)==0);
 }
 S.w=80; S.h=64;
 Plane3 a=plane_alloc(S.w,S.h), b=plane_alloc(S.w,S.h);
 srand(7);
 for(int i=0;i<S.w*S.h;i++){a.p[0][i]=rand()%256;b.p[0][i]=rand()%256;}
 for(int q=0;q<2;q++) {S.qpel=q;
  for(int i=0;i<10000;i++) {
   int x=rand()%5,y=rand()%4,mx=rand()%257-128,my=rand()%257-128, expected=0;
   if(i%2==0){int u=q?4:2;mx=mx/u*u;my=my/u*u;}
   uint8_t pred[256];
   if(q) inter_pred_q(a.p[0],S.w,S.h,x*16,y*16,16,16,mx,my,pred,16);
   else inter_pred(a.p[0],S.w,S.h,x*16,y*16,16,16,mx,my,pred,16);
   for(int yy=0;yy<16;yy++)for(int xx=0;xx<16;xx++)expected+=abs(pred[yy*16+xx]-b.p[0][(y*16+yy)*S.w+x*16+xx]);
   assert(expected==sad16(&a,&b,x,y,mx,my));
  }
 }
 plane_free(&a);plane_free(&b);puts("PASS 20000 SAD comparisons, integer/fractional, borders, negative vectors, half/qpel");
}
