/* Crafted TAC stream for the decoder limits (vector t08): values no encoder produces but the syntax allows.
 *   tac_gen OUT.tac
 * Frames (stereo, LONG, mid/side in every band): 0 large M and S in one line that both exceed the +-2^26 limit (the
 * limit makes R = 0; without it R would be a loud tone); 1-3 ordinary small values; 4 scale factor 255 with magnitude
 * 8191 (word-width guard); 5 scale factor 0 with magnitude 1 (values that round to zero); 6-9 empty bands with noise
 * filling at scale factor 200; 10-11 silence. */
#define TAC_NO_MAIN
#include "tac.c"

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: tac_gen OUT.tac\n"); return 1; }
    tables(); tvc_cost_init();
    const int nfr = 12, nch = 2;
    FILE *o = fopen(argv[1], "wb");
    fwrite("TAC0", 1, 4, o); put32(o, 48000); fputc(nch, o); fputc(0, o); fputc(0, o); fputc(0, o);
    put32(o, (uint32_t)nfr); put32(o, (uint32_t)((nfr - 1) * N));
    ACtx c; actx_init(&c);
    for (int t = 0; t < nfr; t++) {
        Frame f; memset(&f, 0, sizeof f);
        f.reset = t == 0 || t == 6; f.type = W_LONG; f.msmode = 1; f.maxband = 42; f.nfill = t >= 6 && t <= 9;
        for (int ch = 0; ch < nch; ch++) for (int b = 0; b < NBL; b++) f.sf[ch][b] = t == 4 ? 255 : t == 5 ? 0 : t >= 6 && t <= 9 ? 200 : 100;
        if (t == 0) { f.sf[0][10] = f.sf[1][10] = 160; f.q[0][BL[10]] = 900; f.q[1][BL[10]] = 880; }
        if (t >= 1 && t <= 3) for (int k = 0; k < 400; k += 7) { f.q[0][k] = (k % 3) - 1 + 5 * (k % 2); f.q[1][k] = -(k % 4); }
        if (t == 4) { f.q[0][3] = 8191; f.q[1][3] = -8191; }
        if (t == 5) for (int k = 0; k < N; k += 3) { f.q[0][k] = 1; f.q[1][k] = -1; }
        RC r; rc_enc_init(&r); code_frame(&r, &c, &f, nch); rc_enc_flush(&r);
        put16(o, (int)r.outlen); fwrite(r.out, 1, r.outlen, o); free(r.out);
    }
    fclose(o);
    return 0;
}
