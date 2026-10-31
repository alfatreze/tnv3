#include <stdio.h>
#include <stdint.h>
#include "tvc.h"
int main(void){RC rc;rc_enc_init(&rc);uint16_t z[2]={16384,16384},m[6];for(int i=0;i<6;i++)m[i]=16384;uint32_t x=918823;for(int n=0;n<1500;n++){x=x*1664525u+1013904223u;int v=(x>>24)%5?((int)((x>>8)%4097)-2048):0;code_signed_eg(&rc,&z[n%2],m,6,v);}rc_enc_flush(&rc);fwrite(rc.out,1,rc.outlen,stdout);free(rc.out);}
