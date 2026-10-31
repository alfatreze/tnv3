#include <stdint.h>
#include <stdio.h>
#include "tvc.h"
int main(void) { uint32_t s=912325u; uint8_t p[32*24],o[16*16];
 for(int n=0;n<128;n++){for(int i=0;i<32*24;i++){s=s*1664525u+1013904223u;p[i]=s>>24;} int bx=(n*7)%32,by=(n*11)%24,mx=(n%33)-16,my=((n*13)%33)-16;
 for(int mode=0;mode<3;mode++){if(mode==0)inter_pred(p,32,24,bx,by,16,16,mx,my,o,16);else if(mode==1)inter_pred_q(p,32,24,bx,by,16,16,mx,my,o,16);else inter_pred_c8(p,32,24,bx,by,16,16,mx,my,o,16); fwrite(o,1,256,stdout);}}
}
