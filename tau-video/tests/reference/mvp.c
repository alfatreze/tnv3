#include <stdio.h>
#include <stdint.h>
#include "tvc.h"
static void emit(int x){uint16_t u=(uint16_t)x;putchar(u&255);putchar(u>>8);}
int main(void){for(int n=0;n<512;n++){MB mb[9]={0};for(int j=0;j<9;j++){mb[j].uses=(n+j*3)%4;for(int l=0;l<2;l++)for(int k=0;k<2;k++)mb[j].mv[l][k]=(n*19+j*17+l*37+k*53)%401-200;}Pic p={0};p.mbw=3;p.mbh=3;p.mb=mb;int x=n%3,y=(n/3)%3;p.slice_y0=n%2?y:0;for(int l=0;l<2;l++){int a,b;mv_pred(&p,x,y,l,&a,&b);emit(a);emit(b);}}}
