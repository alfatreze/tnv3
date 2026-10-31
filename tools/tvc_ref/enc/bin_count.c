/* Count syntax bins while decoding with the unchanged reference decoder. */
#include <stdint.h>
#include <stdio.h>

static unsigned long long bins;
void tvc_rc_hook_init(const uint8_t *buf, size_t len) { (void)buf; (void)len; }
void tvc_rc_hook_bin(int bypass, const uint16_t *pp, unsigned p, int bit, uint32_t range, uint32_t code) {
    (void)bypass; (void)pp; (void)p; (void)bit; (void)range; (void)code;
    bins++;
}
void tvc_rc_hook_ctx(const void *base) { (void)base; }
void tvc_rc_hook_blk(int end, int cc, int n, const int16_t *lev) { (void)end; (void)cc; (void)n; (void)lev; }
void tvc_rc_hook_slice(int ptype, int aq, int t4ok, int qp, int mbw, int y0, int y1) {
    (void)ptype; (void)aq; (void)t4ok; (void)qp; (void)mbw; (void)y0; (void)y1;
}
void tvc_rc_hook_mb(int x, int y, const void *mb, const int16_t lev[6][64]) {
    (void)x; (void)y; (void)mb; (void)lev;
}

#define TVC_RC_HOOK 1
#define main tvc_dec_main
#include "../tvc_dec.c"
#undef main

int main(int argc, char **argv) {
    int rc = tvc_dec_main(argc, argv);
    fprintf(stderr, "TVC_SYNTAX_BINS=%llu\n", bins);
    return rc;
}
