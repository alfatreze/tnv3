#include <stdint.h>
#include <stdio.h>
#include "tvc.h"
int main(void) { uint32_t x=78123u; for(int n=0;n<64;n++){uint8_t p[256],o[8][8]; for(int i=0;i<256;i++){x=x*1664525u+1013904223u;p[i]=x>>24;} for(int pos=0;pos<4;pos++)for(int mode=0;mode<4;mode++){intra_pred(p,16,(pos&1)*8,(pos>>1)*8,0,mode,o);fwrite(o,1,64,stdout);}} }
