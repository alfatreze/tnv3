#include "../tvc.h"
#include "qpel.h"
#include <assert.h>
static uint32_t seed = 9;
static uint32_t rnd(void) { seed = seed * 1664525u + 1013904223u; return seed; }
int main(void) {
    const int dims[3][2] = {{80,64},{37,41},{16,16}};
    unsigned hits[4][4] = {{0}}, cases = 0, fallback = 0;
    for (int d = 0; d < 3; d++) for (int pattern = 0; pattern < 5; pattern++) {
        int w=dims[d][0], h=dims[d][1];
        uint8_t *allocation=malloc((size_t)w*h+16);
        assert(allocation);
        for (int offset=0; offset<16; offset+=5) {
            uint8_t *ref=allocation+offset;
            for(int y=0;y<h;y++) for(int x=0;x<w;x++)
                ref[y*w+x] = pattern==0 ? 0 : pattern==1 ? 255 : pattern==2 ? ((x+y)&1)*255 : pattern==3 ? rnd()>>24 : (x==w/2&&y==h/2?255:0);
            for(int fx=0;fx<4;fx++) for(int fy=0;fy<4;fy++) for(int i=0;i<32;i++) {
                int bx=(int)(rnd()%(w+32))-16, by=(int)(rnd()%(h+32))-16;
                int mx=((int)(rnd()%33)-16)*4+fx, my=((int)(rnd()%33)-16)*4+fy;
                uint8_t expected[16*23+32], actual[16*23+32];
                memset(expected,0xcd,sizeof expected); memset(actual,0xcd,sizeof actual);
                inter_pred_q(ref,w,h,bx,by,16,16,mx,my,expected+7,23);
                if(enc_pred_q16(ref,w,h,bx,by,mx,my,actual+7,23)) hits[fx][fy]++;
                else {
                    /* Rejected footprints must leave all output and guards untouched. */
                    for(size_t k=0;k<sizeof actual;k++) assert(actual[k]==0xcd);
                    inter_pred_q(ref,w,h,bx,by,16,16,mx,my,actual+7,23); fallback++;
                }
                assert(!memcmp(expected,actual,sizeof actual)); cases++;
            }
        }
        free(allocation);
    }
    for(int fx=0;fx<4;fx++) for(int fy=0;fy<4;fy++) assert(hits[fx][fy]);
    printf("PASS %u whole-pixel qpel cases, %u border fallbacks, all phases and guarded output\n",cases,fallback);
    return 0;
}
