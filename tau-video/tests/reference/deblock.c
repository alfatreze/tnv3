#include <stdio.h>
#include <stdint.h>
#include "tvc.h"
int main(void) {uint32_t state=29132;for(int n=0;n<104;n++) {MB mb[4]={0};uint8_t y[1024],u[256],v[256];uint8_t *planes[3]={y,u,v};int sizes[3]={1024,256,256};
 for(int j=0;j<4;j++){mb[j].type=(n+j)%3;mb[j].uses=(n+j)%4;mb[j].coded=(n*7+j*13)%64;mb[j].qp=(n+j)%52;for(int l=0;l<2;l++)for(int k=0;k<2;k++)mb[j].mv[l][k]=(n*3+j*11+l*7+k*5)%25-12;}
 for(int p=0;p<3;p++)for(int i=0;i<sizes[p];i++){state=state*1664525u+1013904223u;planes[p][i]=112+((state>>24)%32);}
 Pic pic={0};pic.w=32;pic.h=32;pic.mbw=2;pic.mbh=2;pic.mb=mb;pic.qpel=n%2;Plane3 image={32,32,{y,u,v}};tvc_deblock(&pic,&image);
 for(int p=0;p<3;p++)fwrite(planes[p],1,sizes[p],stdout);
}}
