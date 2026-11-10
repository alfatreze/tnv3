/* TAC v0 fixed-point decoder model (docs/TAC_BITSTREAM_V0.md section 10): integer-only reconstruction for the hardware.
 *   tac_fx dec IN.tac OUT.wav [-f FX] [-t TW]     fixed-point decode (FX fraction bits of coefficients and accumulators,
 *                                                   TW fraction bits of the twiddle and window tables; defaults 4, 20)
 *   tac_fx cmp IN.tac [-f FX] [-t TW]             fixed vs the double reference: max / RMS sample difference, widths
 *   tac_fx test                                    FFT-based IMDCT (double) equals the direct reference transform
 * Syntax parsing and the double reference are tac.c's (included); only reconstruction and the transform are new here.
 * Dequantiser: |q|^(4/3) table (0..1024, PB fraction bits) with linear interpolation above 1024, 2^(b/4) table, shifts.
 * Transform: DCT-IV through an N/2-point complex FFT (pre-twiddle, radix-2 stages each halving with rounding,
 * post-twiddle), so the 2/N scale of the IMDCT is exactly the FFT's 1/2 per stage; then window, overlap-add, rounding. */
#define TAC_NO_MAIN
#include "tac.c"

static int FX = 4, TW = 20;   /* chosen from the sweep in the spec, section 10 */
#define PB 16                              /* fraction bits of the |q|^(4/3) table (8 gives 4e-4 error at q = 3) */
#define P2B 22                             /* fraction bits of the 2^(b/4) table (14 gives 3e-5 error: 0.5 at full scale) */
#define XLIM (((int64_t)1) << (26 + FX))       /* coefficient clamp, 2^26 in real units (section 6): above any 16-bit
                                                   signal's MDCT (2^25.4); every FFT value then fits 28 + FX bits with sign */
static int64_t POW43[1025], P2[4];
static int64_t TWL[4][N / 2], TWS[4][N / 2];          /* cos, sin of pi (4j+1) / (4n) (pre-twiddle), of pi j / n (post) */
static int64_t FFTL[2][N / 4], FFTS[2][NS / 4];        /* FFT twiddles cos, sin of 2 pi j / (n/2) */
static int64_t WLQ[2 * N], WSQ[2 * NS];
static int64_t maxdata;                                /* largest magnitude seen in the transform (width report) */

static int64_t qround(double v) { return (int64_t)llround(v); }
static int64_t rshift(int64_t v, int s) { return s <= 0 ? v * ((int64_t)1 << -s) : (v + ((int64_t)1 << (s - 1))) >> s; }   /* round half up */
static int64_t clampx(int64_t v) { return v > XLIM ? XLIM : v < -XLIM ? -XLIM : v; }

static void fx_tables(void) {
    for (int i = 0; i <= 1024; i++) POW43[i] = qround(pow(i, 4.0 / 3.0) * (1 << PB));
    for (int b = 0; b < 4; b++) P2[b] = qround(pow(2.0, b / 4.0) * (1 << P2B));
    double one = (double)((int64_t)1 << TW);
    for (int j = 0; j < N / 2; j++) {
        TWL[0][j] = qround(cos(M_PI * (4 * j + 1) / (4.0 * N)) * one); TWL[1][j] = qround(sin(M_PI * (4 * j + 1) / (4.0 * N)) * one);
        TWL[2][j] = qround(cos(M_PI * j / N) * one); TWL[3][j] = qround(sin(M_PI * j / N) * one);
    }
    for (int j = 0; j < NS / 2; j++) {
        TWS[0][j] = qround(cos(M_PI * (4 * j + 1) / (4.0 * NS)) * one); TWS[1][j] = qround(sin(M_PI * (4 * j + 1) / (4.0 * NS)) * one);
        TWS[2][j] = qround(cos(M_PI * j / NS) * one); TWS[3][j] = qround(sin(M_PI * j / NS) * one);
    }
    for (int j = 0; j < N / 4; j++) { FFTL[0][j] = qround(cos(2 * M_PI * j / (N / 2)) * one); FFTL[1][j] = qround(sin(2 * M_PI * j / (N / 2)) * one); }
    for (int j = 0; j < NS / 4; j++) { FFTS[0][j] = qround(cos(2 * M_PI * j / (NS / 2)) * one); FFTS[1][j] = qround(sin(2 * M_PI * j / (NS / 2)) * one); }
    for (int n = 0; n < 2 * N; n++) WLQ[n] = qround(WL[n] * one);
    for (int n = 0; n < 2 * NS; n++) WSQ[n] = qround(WS[n] * one);
}

/* |q|^(4/3) * 2^((sf-100)/4) * 2^FX, rounded, with the sign of q */
static int64_t fx_dq(int q, int sf) {
    if (!q) return 0;
    int a = abs(q);
    int64_t v = a <= 1024 ? POW43[a] : 16 * (POW43[a >> 3] + rshift((POW43[(a >> 3) + 1] - POW43[a >> 3]) * (a & 7), 3));
    int e = sf - SF_OFF, ex = e >> 2, b = e & 3;           /* floor and non-negative remainder */
    int64_t m = clampx(rshift(v * P2[b], PB + P2B - FX - ex));
    return q < 0 ? -m : m;
}
/* noise line: u * 2^((sf-100)/4) * 2^FX with u = (r - 32768) / 32768 */
static int64_t fx_noise(uint32_t r16, int sf) {
    int64_t u = (int64_t)r16 - 32768;
    int e = sf - SF_OFF, ex = e >> 2, b = e & 3;
    int64_t m = clampx(rshift((u < 0 ? -u : u) * P2[b], 15 + P2B - FX - ex));
    return u < 0 ? -m : m;
}

static void fx_reconstruct(const Frame *f, int nch, uint32_t t, int64_t coef[2][N]) {
    int idx[N];
    for (int ch = 0; ch < nch; ch++) {
        memset(coef[ch], 0, sizeof coef[ch]);
        uint32_t seed = t * 2u + (uint32_t)ch + 1u;
        int nfs = f->type == W_SHORT ? NF_START_S : NF_START_L;
        for (int b = 0; b < f->maxband; b++) {
            int n = band_lines(f->type, b, idx), any = 0;
            for (int i = 0; i < n; i++) any |= f->q[ch][idx[i]] != 0;
            if (any || !f->nfill || b < nfs) { for (int i = 0; i < n; i++) coef[ch][idx[i]] = fx_dq(f->q[ch][idx[i]], f->sf[ch][b]); continue; }
            for (int i = 0; i < n; i++) coef[ch][idx[i]] = fx_noise((lcg(&seed) >> 8) & 0xFFFF, f->sf[ch][b]);
        }
    }
    if (nch == 2 && f->msmode)
        for (int b = 0; b < f->maxband; b++) {
            if (f->msmode == 2 && !f->ms[b]) continue;
            int n = band_lines(f->type, b, idx);
            for (int i = 0; i < n; i++) { int64_t m = coef[0][idx[i]], s = coef[1][idx[i]]; coef[0][idx[i]] = clampx(m + s); coef[1][idx[i]] = clampx(m - s); }
        }
}

static void track(int64_t v) { if (v < 0) v = -v; if (v > maxdata) maxdata = v; }
/* complex multiply by (c + i s) in Q(TW), rounded */
static void cmul(int64_t *re, int64_t *im, int64_t c, int64_t s) {
    int64_t r = rshift(*re * c - *im * s, TW), i = rshift(*re * s + *im * c, TW);
    *re = r; *im = i;
}
/* DCT-IV of length n (input x[0..n-1] in Q(FX)) scaled by 2/n: u[m] = (2/n) sum_k x[k] cos(pi/n (m+1/2)(k+1/2)) */
static void fx_dct4(const int64_t *x, int64_t *u, int n, int64_t tw[4][N / 2], int64_t ft[2][N / 4]) {
    int h = n / 2;
    int64_t re[N / 2], im[N / 2];
    for (int j = 0; j < h; j++) {                     /* pre-twiddle by exp(-i pi (4j+1) / (4n)), bit-reversed order */
        int r = 0; for (int b = 1, k = j; b < h; b <<= 1, k >>= 1) r = (r << 1) | (k & 1);
        int64_t a = x[2 * j], c = x[n - 1 - 2 * j];
        cmul(&a, &c, tw[0][j], -tw[1][j]);
        re[r] = a; im[r] = c; track(a); track(c);
    }
    for (int len = 2; len <= h; len <<= 1)            /* radix-2 DIT, every stage halves with rounding */
        for (int s = 0; s < h; s += len)
            for (int k = 0; k < len / 2; k++) {
                int64_t c = ft[0][k * (h / len)], sn = -ft[1][k * (h / len)];   /* exp(-2 pi i k / len) */
                int64_t br = re[s + k + len / 2], bi = im[s + k + len / 2];
                cmul(&br, &bi, c, sn);
                int64_t ar = re[s + k], ai = im[s + k];
                re[s + k] = (ar + br + 1) >> 1; im[s + k] = (ai + bi + 1) >> 1;
                re[s + k + len / 2] = (ar - br + 1) >> 1; im[s + k + len / 2] = (ai - bi + 1) >> 1;
                track(re[s + k]); track(im[s + k]); track(re[s + k + len / 2]); track(im[s + k + len / 2]);
            }
    for (int k = 0; k < h; k++) {                     /* post-twiddle by exp(-i pi k / n) */
        int64_t a = re[k], c = im[k];
        cmul(&a, &c, tw[2][k], -tw[3][k]);
        u[2 * k] = a; u[n - 1 - 2 * k] = -c;
    }
}
/* IMDCT output sample n (0..2n-1) from the DCT-IV values (section 6 phase n + 1/2 + n/2) */
static int64_t imdct_at(const int64_t *u, int n, int i) {
    if (i < n / 2) return u[i + n / 2];
    if (i < 3 * n / 2) return -u[3 * n / 2 - 1 - i];
    return -u[i - 3 * n / 2];
}
static int64_t fx_win_long(int type, int n) {
    if (type == W_LONG) return WLQ[n];
    if (type == W_START) { if (n < N) return WLQ[n]; if (n < 1472) return (int64_t)1 << TW; if (n < 1600) return WSQ[NS + (n - 1472)]; return 0; }
    if (n < 448) return 0; if (n < 576) return WSQ[n - 448]; if (n < N) return (int64_t)1 << TW; return WLQ[n];
}
static void fx_imdct_frame(const int64_t *coef, int type, int64_t *out) {
    int64_t u[N];
    if (type != W_SHORT) {
        fx_dct4(coef, u, N, TWL, FFTL);
        for (int n = 0; n < 2 * N; n++) out[n] += rshift(imdct_at(u, N, n) * fx_win_long(type, n), TW);
        return;
    }
    static int64_t fts[2][N / 4];
    for (int j = 0; j < NS / 4; j++) { fts[0][j] = FFTS[0][j]; fts[1][j] = FFTS[1][j]; }
    for (int w = 0; w < 8; w++) {
        fx_dct4(coef + w * NS, u, NS, TWS, fts);
        for (int n = 0; n < 2 * NS; n++) out[448 + NS * w + n] += rshift(imdct_at(u, NS, n) * WSQ[n], TW);
    }
}
static int16_t fx_sample(int64_t acc) {            /* round half away from zero, then clip */
    int64_t h = (int64_t)1 << (FX - 1), m = acc < 0 ? -acc : acc;
    int64_t v = (m + h) >> FX;
    v = acc < 0 ? -v : v;
    return (int16_t)(v > 32767 ? 32767 : v < -32768 ? -32768 : v);
}

/* decode a .tac file; fixed = 1: this model, 0: the double reference of tac.c */
static double *unrounded;   /* last run's output before rounding (samples, interleaved) */
static int16_t *run(const char *in, int fixed, int *nch_out, uint32_t *ns_out) {
    FILE *f = fopen(in, "rb"); char m[4];
    if (!f || fread(m, 1, 4, f) != 4 || (memcmp(m, "TAC0", 4) && memcmp(m, "TAC1", 4))) { fprintf(stderr, "not a TAC0 or TAC1 file\n"); exit(1); }
    int rate = (int)get32(f), nch = fgetc(f), feat = fgetc(f); fgetc(f); fgetc(f);
    if (memcmp(m, "TAC1", 4)) feat = 0;
    if (feat & ~1) { fprintf(stderr, "unsupported TAC1 feature bits\n"); exit(1); }
    FEAT = feat;
    uint32_t nfr = get32(f), nsamp = get32(f);
    if (rate != 48000 || nch < 1 || nch > 2) { fprintf(stderr, "unsupported stream\n"); exit(1); }
    size_t len_acc = (size_t)(nfr + 2) * N;
    int64_t *ia = fixed ? calloc(len_acc * 2, sizeof *ia) : NULL;
    double *da = fixed ? NULL : calloc(len_acc * 2, sizeof *da);
    ACtx c; actx_init(&c); Frame fr;
    for (uint32_t t = 0; t < nfr; t++) {
        int len = get16(f); uint8_t *buf = malloc(len ? len : 1);
        if (fread(buf, 1, len, f) != (size_t)len) { fprintf(stderr, "truncated frame %u\n", t); exit(1); }
        RC r; rc_dec_init(&r, buf, len); memset(&fr, 0, sizeof fr);
        code_frame(&r, &c, &fr, nch);
        if (fixed) {
            int64_t coef[2][N]; fx_reconstruct(&fr, nch, t, coef);
            for (int ch = 0; ch < nch; ch++) fx_imdct_frame(coef[ch], fr.type, ia + ch * len_acc + (size_t)t * N);
        } else {
            double coef[2][N]; reconstruct(&fr, nch, t, coef);
            for (int ch = 0; ch < nch; ch++) imdct_frame(coef[ch], fr.type, da + ch * len_acc + (size_t)t * N);
        }
        free(buf);
    }
    fclose(f);
    int16_t *pcm = malloc((size_t)nsamp * nch * 2 + 2);
    unrounded = malloc(((size_t)nsamp * nch + 1) * sizeof *unrounded);
    for (uint32_t s = 0; s < nsamp; s++) for (int ch = 0; ch < nch; ch++) {
        unrounded[(size_t)s * nch + ch] = fixed ? ia[ch * len_acc + s + N] / (double)((int64_t)1 << FX) : da[ch * len_acc + s + N];
        if (fixed) pcm[(size_t)s * nch + ch] = fx_sample(ia[ch * len_acc + s + N]);
        else { long iv = lround(da[ch * len_acc + s + N]); pcm[(size_t)s * nch + ch] = (int16_t)(iv > 32767 ? 32767 : iv < -32768 ? -32768 : iv); }
    }
    *nch_out = nch; *ns_out = nsamp;
    return pcm;
}

/* the FFT-based DCT-IV in double precision equals the direct sum (checks the algorithm, not the rounding) */
static int test(void) {
    double maxe = 0;
    for (int n = NS; n <= N; n += N - NS) {
        int64_t x[N], u[N];
        srand(7);
        for (int k = 0; k < n; k++) x[k] = ((rand() % 2000001) - 1000000) * 1024LL;   /* 10 extra fraction bits */
        int saved = TW; TW = 30; fx_tables();
        int64_t ft[2][N / 4];
        for (int j = 0; j < n / 4; j++) { ft[0][j] = n == N ? FFTL[0][j] : FFTS[0][j]; ft[1][j] = n == N ? FFTL[1][j] : FFTS[1][j]; }
        fx_dct4(x, u, n, n == N ? TWL : TWS, ft);
        TW = saved; fx_tables();
        for (int mi = 0; mi < n; mi++) {
            double s = 0; for (int k = 0; k < n; k++) s += x[k] * cos(M_PI / n * (mi + 0.5) * (k + 0.5));
            maxe = fmax(maxe, fabs(s / 1024.0 * 2.0 / n - u[mi] / 1024.0));
        }
    }
    printf("FFT DCT-IV vs direct sum (inputs up to 1e6): max error %.5f (expect < 0.01)\n", maxe);
    return maxe > 0.01;
}

int main(int argc, char **argv) {
    tables(); tvc_cost_init();
    for (int i = 2; i + 1 < argc; i++) {
        if (!strcmp(argv[i], "-f")) FX = atoi(argv[i + 1]);
        else if (!strcmp(argv[i], "-t")) TW = atoi(argv[i + 1]);
    }
    fx_tables();
    if (argc >= 2 && !strcmp(argv[1], "test")) return test();
    if (argc >= 4 && !strcmp(argv[1], "dec")) {
        int nch; uint32_t ns;
        int16_t *p = run(argv[2], 1, &nch, &ns);
        wav_write(argv[3], p, nch, 48000, ns);
        return 0;
    }
    if (argc >= 3 && !strcmp(argv[1], "cmp")) {
        int nch, nch2; uint32_t ns, ns2;
        int16_t *a = run(argv[2], 1, &nch, &ns);
        double *ua = unrounded;
        int16_t *b = run(argv[2], 0, &nch2, &ns2);
        long maxd = 0; double se = 0, ue = 0, umax = 0; long ndiff = 0;
        for (size_t i = 0; i < (size_t)ns * nch; i++) {
            double e = fabs(ua[i] - unrounded[i]); ue += e * e; if (e > umax) umax = e;
            long d = labs((long)a[i] - b[i]);
            if (d > maxd) maxd = d;
            se += (double)d * d; ndiff += d != 0;
        }
        int bits = 0; while (((int64_t)1 << bits) <= maxdata) bits++;
        printf("FX=%d TW=%d  max|d|=%ld  rms=%.4f  differing=%.3f%%  unrounded err max %.4f rms %.5f  data %d bits + sign\n", FX, TW, maxd,
               sqrt(se / ((double)ns * nch)), 100.0 * ndiff / ((double)ns * nch), umax, sqrt(ue / ((double)ns * nch)), bits);
        return 0;
    }
    fprintf(stderr, "usage: tac_fx dec IN.tac OUT.wav [-f FX] [-t TW] | tac_fx cmp IN.tac [-f FX] [-t TW] | tac_fx test\n");
    return 1;
}
