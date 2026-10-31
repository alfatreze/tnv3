#include <stdint.h>
#include <stdio.h>
#include "tvc.h"
static void emit32(int x) { uint32_t u=(uint32_t)x; putchar(u&255); putchar((u>>8)&255); putchar((u>>16)&255); putchar((u>>24)&255); }
int main(void) {
 tvc_init(); uint32_t x=891203u;
 for(int n=0;n<104;n++) { int16_t l[64]; int out[8][8]={0}; int qp=n%52;
  for(int i=0;i<64;i++){x=x*1664525u+1013904223u;l[i]=(int16_t)((x>>16)%601)-300;}
  inv_transform(l,qp,out); for(int y=0;y<8;y++) for(int z=0;z<8;z++) emit32(out[y][z]);
  for(int y=0;y<8;y++) for(int z=0;z<8;z++) out[y][z]=0;
  inv_transform4(l,qp,out,0,0); for(int y=0;y<4;y++) for(int z=0;z<4;z++) emit32(out[y][z]);
 }
}
