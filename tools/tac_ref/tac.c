/* TAC v0 reference (TNV3 audio): MDCT transform codec, 48 kHz, mono or stereo, 16-bit PCM.
 *   tac enc IN.wav OUT.tac [-b KBPS]     (default 96; encoder: psychoacoustic allocation, block switching, M/S, noise filling)
 *   tac dec IN.tac OUT.wav               (reference decoder)
 * Normative description: docs/TAC_BITSTREAM_V0.md.  Frame = 1024 new samples; long MDCT of 2048, or 8 short MDCTs of
 * 256 (block switching with START/STOP transition windows); scale-factor bands; power-law quantiser (|x|^(3/4));
 * adaptive binary range coding (the TVC coder); per-band mid/side; noise filling of all-zero bands.
 * The decoder is floating point; conformance is by tolerance against this reference (see the spec). */
#include "../tvc_ref/tvc.h"

#define N 1024
#define NS 128
#define SF_OFF 100
static const int BL[] = {0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 52, 57, 62, 67, 73, 79, 86, 93, 101, 110, 120, 131,
                         143, 157, 172, 188, 206, 225, 246, 268, 292, 319, 349, 384, 426, 478, 546, 642, 793, 853};
#define NBL 42
static const int BS[] = {0, 4, 8, 12, 16, 20, 24, 29, 35, 42, 51, 65, 92, 106};
#define NBS 13
#define QMAX 8191      /* largest quantised magnitude allowed in a stream */
#define NF_START_L 8   /* first band (long) that may be noise filled */
#define NF_START_S 3
enum { W_LONG = 0, W_START = 1, W_SHORT = 2, W_STOP = 3 };
static int NFILL = 1;           /* encoder switch -n 0/1 */
static double SMR_DB = 0.0;     /* encoder switch -m: extra signal-to-mask offset (dB) on top of the tonality offsets */
static int MSFORCE = -1;        /* encoder switch -M 0/1/2: force the stereo mode (conformance vectors); -1 = per-band choice */
static int TONAL = 1;           /* encoder switch -T 0/1: tonality-dependent offsets (0 = fixed 13 dB, the draft-1 model) */
static double ATT_RATIO = 8.0;  /* encoder switch -a: attack = segment high-pass energy above this multiple of the recent average */

static double WL[2 * N], WS[2 * NS], CL[2 * N][N], CS[2 * NS][NS];
static void tables(void) {
    for (int n = 0; n < 2 * N; n++) WL[n] = sin(M_PI * (n + 0.5) / (2 * N));
    for (int n = 0; n < 2 * NS; n++) WS[n] = sin(M_PI * (n + 0.5) / (2 * NS));
    for (int n = 0; n < 2 * N; n++) for (int k = 0; k < N; k++) CL[n][k] = cos(M_PI / N * (n + 0.5 + N / 2.0) * (k + 0.5));
    for (int n = 0; n < 2 * NS; n++) for (int k = 0; k < NS; k++) CS[n][k] = cos(M_PI / NS * (n + 0.5 + NS / 2.0) * (k + 0.5));
}
/* long-transform window of a frame type (2048 samples); SHORT frames use 8 short windows instead */
static double win_long(int type, int n) {
    if (type == W_LONG) return WL[n];
    if (type == W_START) { if (n < N) return WL[n]; if (n < 1472) return 1.0; if (n < 1600) return WS[NS + (n - 1472)]; return 0.0; }
    /* STOP */ if (n < 448) return 0.0; if (n < 576) return WS[n - 448]; if (n < N) return 1.0; return WL[n];
}
/* forward transform of one 2048-sample frame into coef[1024]; SHORT: coef[w*128 + k] */
static void mdct_frame(const double *x, int type, double *coef) {
    if (type != W_SHORT) {
        double xw[2 * N]; for (int n = 0; n < 2 * N; n++) xw[n] = x[n] * win_long(type, n);
        for (int k = 0; k < N; k++) { double s = 0; for (int n = 0; n < 2 * N; n++) s += xw[n] * CL[n][k]; coef[k] = s; }
        return;
    }
    for (int w = 0; w < 8; w++) {
        const double *xs = x + 448 + NS * w;
        for (int k = 0; k < NS; k++) { double s = 0; for (int n = 0; n < 2 * NS; n++) s += xs[n] * WS[n] * CS[n][k]; coef[w * NS + k] = s; }
    }
}
/* inverse: adds the windowed 2048-sample output of one frame into out[] */
static void imdct_frame(const double *coef, int type, double *out) {
    if (type != W_SHORT) {
        for (int n = 0; n < 2 * N; n++) { double s = 0; for (int k = 0; k < N; k++) s += coef[k] * CL[n][k]; out[n] += s * (2.0 / N) * win_long(type, n); }
        return;
    }
    for (int w = 0; w < 8; w++)
        for (int n = 0; n < 2 * NS; n++) { double s = 0; for (int k = 0; k < NS; k++) s += coef[w * NS + k] * CS[n][k]; out[448 + NS * w + n] += s * (2.0 / NS) * WS[n]; }
}

/* ------------------------------------------------------------------ bitstream contexts */
typedef struct {
    uint16_t sfz[2], sfm[6], ms[2], bz[3][2], m0[3][5], m1[3][5], m2[3][3], esc[6];
} ACtx;
static void actx_init(ACtx *c) { uint16_t *p = (uint16_t *)c; for (size_t i = 0; i < sizeof *c / 2; i++) p[i] = PROB_INIT; }
static int rc_bits(RC *r, int nbits, int v) { int o = 0; for (int b = nbits - 1; b >= 0; b--) o = (o << 1) | rc_bypass(r, (v >> b) & 1); return o; }

typedef struct {
    int reset, type, msmode, maxband, nfill;
    int ms[NBL];                 /* per band, when msmode == 2 */
    int sf[2][NBL];              /* scale factors per channel and band */
    int q[2][N];                 /* quantised lines: long k, or short w*128+k */
} Frame;

static int nbands(int type) { return type == W_SHORT ? NBS : NBL; }
static const int *edges(int type) { return type == W_SHORT ? BS : BL; }
static int bclass(int type, int b) { return type == W_SHORT ? (b < 4 ? 0 : b < 9 ? 1 : 2) : (b < 12 ? 0 : b < 28 ? 1 : 2); }
/* the lines of band b in coding order: long: k = e0..e1-1; short: window-major, w = 0..7, k = e0..e1-1 */
static int band_lines(int type, int b, int *idx) {
    const int *e = edges(type); int n = 0;
    if (type != W_SHORT) { for (int k = e[b]; k < e[b + 1]; k++) idx[n++] = k; }
    else for (int w = 0; w < 8; w++) for (int k = e[b]; k < e[b + 1]; k++) idx[n++] = w * NS + k;
    return n;
}

/* one frame's syntax, shared by decoder, encoder and bit counting (rc mode) */
static void code_frame(RC *r, ACtx *c, Frame *f, int nch) {
    f->reset = rc_bypass(r, f->reset);
    if (f->reset) actx_init(c);
    f->type = rc_bits(r, 2, f->type);
    f->msmode = nch == 2 ? rc_bits(r, 2, f->msmode) : 0;
    f->maxband = rc_bits(r, 6, f->maxband);
    f->nfill = rc_bypass(r, f->nfill);
    int nb = nbands(f->type);
    if (f->maxband > nb) { fprintf(stderr, "bad max band\n"); exit(2); }
    if (f->msmode == 2) { int prev = 0; for (int b = 0; b < f->maxband; b++) prev = f->ms[b] = rc_bit(r, &c->ms[prev], f->ms[b]); }
    for (int ch = 0; ch < nch; ch++) {
        int *sf = f->sf[ch];
        sf[0] = rc_bits(r, 8, sf[0]);
        int pd = 0;
        for (int b = 1; b < f->maxband; b++) {
            int d = r->mode == RC_DEC ? 0 : sf[b] - sf[b - 1];
            int nz = rc_bit(r, &c->sfz[pd != 0], d != 0);
            if (nz) { int m = 1 + rc_eg(r, c->sfm, 6, 0, abs(d) - 1); d = rc_bypass(r, d < 0) ? -m : m; } else d = 0;
            sf[b] = sf[b - 1] + d; pd = d;
            if (sf[b] < 0 || sf[b] > 255) { fprintf(stderr, "scale factor out of range\n"); exit(2); }
        }
        int *q = f->q[ch], prevz = 0, idx[8 * N / 8 + 8 * 64], q1 = 0, q2 = 0;
        if (r->mode == RC_DEC) memset(q, 0, N * sizeof *q);
        for (int b = 0; b < f->maxband; b++) {
            int n = band_lines(f->type, b, idx), cl = bclass(f->type, b), any = 0;
            if (r->mode != RC_DEC) for (int i = 0; i < n; i++) any |= q[idx[i]] != 0;
            int zero = rc_bit(r, &c->bz[cl][prevz], !any);
            prevz = zero;
            if (zero) continue;
            for (int i = 0; i < n; i++) {
                int a = abs(q[idx[i]]), nc = tvc_min(q1, 2) + tvc_min(q2, 2);
                if (!rc_bit(r, &c->m0[cl][nc], a != 0)) a = 0;
                else if (!rc_bit(r, &c->m1[cl][nc], a > 1)) a = 1;
                else if (!rc_bit(r, &c->m2[cl][tvc_min(nc, 2)], a > 2)) a = 2;
                else a = 3 + rc_eg(r, c->esc, 6, 0, a - 3);
                if (a > QMAX) { fprintf(stderr, "quantised magnitude above the limit\n"); exit(2); }
                if (a) { int s = rc_bypass(r, q[idx[i]] < 0); q[idx[i]] = s ? -a : a; } else q[idx[i]] = 0;
                q2 = q1; q1 = a;
            }
        }
        if (r->mode == RC_DEC) for (int b = f->maxband; b < NBL; b++) sf[b] = 0;
    }
}

/* ------------------------------------------------------------------ reconstruction (decoder side) */
static double dq(int q, int sf) { return q == 0 ? 0.0 : (q < 0 ? -1 : 1) * pow(fabs((double)q), 4.0 / 3.0) * pow(2.0, (sf - SF_OFF) / 4.0); }
static uint32_t lcg(uint32_t *s) { *s = *s * 1664525u + 1013904223u; return *s; }
#define XCLAMP 67108864.0   /* draft 1.2: dequantised and mid/side values are limited to +-2^26 (section 6) */
static double xclamp(double v) { return v > XCLAMP ? XCLAMP : v < -XCLAMP ? -XCLAMP : v; }
static void reconstruct(const Frame *f, int nch, uint32_t frame_index, double coef[2][N]) {
    int idx[N];
    for (int ch = 0; ch < nch; ch++) {
        memset(coef[ch], 0, sizeof coef[ch]);
        uint32_t seed = frame_index * 2u + (uint32_t)ch + 1u;
        int nfs = f->type == W_SHORT ? NF_START_S : NF_START_L;
        for (int b = 0; b < f->maxband; b++) {
            int n = band_lines(f->type, b, idx), any = 0;
            for (int i = 0; i < n; i++) any |= f->q[ch][idx[i]] != 0;
            if (any || !f->nfill || b < nfs) { for (int i = 0; i < n; i++) coef[ch][idx[i]] = xclamp(dq(f->q[ch][idx[i]], f->sf[ch][b])); continue; }
            double amp = pow(2.0, (f->sf[ch][b] - SF_OFF) / 4.0);
            for (int i = 0; i < n; i++) coef[ch][idx[i]] = xclamp(amp * ((double)((lcg(&seed) >> 8) & 0xFFFF) / 32768.0 - 1.0));
        }
    }
    if (nch == 2 && f->msmode) {
        for (int b = 0; b < f->maxband; b++) {
            if (f->msmode == 2 && !f->ms[b]) continue;
            int n = band_lines(f->type, b, idx);
            for (int i = 0; i < n; i++) { double m = coef[0][idx[i]], s = coef[1][idx[i]]; coef[0][idx[i]] = xclamp(m + s); coef[1][idx[i]] = xclamp(m - s); }
        }
    }
}

/* ------------------------------------------------------------------ WAV I/O (16-bit PCM) */
static int16_t *wav_read(const char *path, int *nch, int *rate, long *nsamp) {
    FILE *f = fopen(path, "rb"); if (!f) return NULL;
    uint8_t h[12]; if (fread(h, 1, 12, f) != 12 || memcmp(h, "RIFF", 4) || memcmp(h + 8, "WAVE", 4)) return NULL;
    int bits = 16; *nch = 0;
    for (;;) {
        uint8_t ch[8]; if (fread(ch, 1, 8, f) != 8) return NULL;
        uint32_t len = ch[4] | ch[5] << 8 | ch[6] << 16 | (uint32_t)ch[7] << 24;
        if (!memcmp(ch, "fmt ", 4)) { uint8_t b[64] = {0}; if (fread(b, 1, len < 64 ? len : 64, f) == 0) return NULL; if (len > 64) fseek(f, len - 64, SEEK_CUR);
            *nch = b[2] | b[3] << 8; *rate = b[4] | b[5] << 8 | b[6] << 16 | b[7] << 24; bits = b[14] | b[15] << 8; }
        else if (!memcmp(ch, "data", 4)) {
            if (bits != 16 || *nch < 1 || *nch > 2) return NULL;
            *nsamp = len / (2 * *nch); int16_t *d = malloc(len); if (fread(d, 1, len, f) != len) { *nsamp = 0; }
            fclose(f); return d;
        } else fseek(f, len + (len & 1), SEEK_CUR);
    }
}
static void wav_write(const char *path, const int16_t *d, int nch, int rate, long nsamp) {
    FILE *f = fopen(path, "wb"); uint32_t len = (uint32_t)(nsamp * nch * 2);
    fwrite("RIFF", 1, 4, f); put32(f, 36 + len); fwrite("WAVEfmt ", 1, 8, f); put32(f, 16); put16(f, 1); put16(f, nch);
    put32(f, rate); put32(f, rate * nch * 2); put16(f, nch * 2); put16(f, 16); fwrite("data", 1, 4, f); put32(f, len);
    fwrite(d, 2, nsamp * nch, f); fclose(f);
}

/* ------------------------------------------------------------------ decoder */
static int decode(const char *in, const char *out) {
    FILE *f = fopen(in, "rb"); char m[4];
    if (!f || fread(m, 1, 4, f) != 4 || memcmp(m, "TAC0", 4)) { fprintf(stderr, "not a TAC0 file\n"); return 1; }
    int rate = (int)get32(f), nch = fgetc(f); fgetc(f); fgetc(f); fgetc(f);
    uint32_t nfr = get32(f), nsamp = get32(f);
    if (rate != 48000 || nch < 1 || nch > 2) { fprintf(stderr, "unsupported stream\n"); return 1; }
    double *acc = calloc((size_t)(nfr + 2) * N, sizeof *acc * 2);    /* interleaved per channel: acc[ch][...] */
    double *accc[2] = {acc, acc + (size_t)(nfr + 2) * N};
    ACtx c; actx_init(&c); Frame fr;
    for (uint32_t t = 0; t < nfr; t++) {
        int len = get16(f); uint8_t *buf = malloc(len ? len : 1);
        if (fread(buf, 1, len, f) != (size_t)len) { fprintf(stderr, "truncated frame %u\n", t); return 1; }
        RC r; rc_dec_init(&r, buf, len); memset(&fr, 0, sizeof fr);
        code_frame(&r, &c, &fr, nch);
        if (t == 0 && !fr.reset) { fprintf(stderr, "first frame must reset the contexts\n"); return 1; }
        double coef[2][N]; reconstruct(&fr, nch, t, coef);
        for (int ch = 0; ch < nch; ch++) imdct_frame(coef[ch], fr.type, accc[ch] + (size_t)t * N);   /* frame t starts at sample (t-1)*N */
        free(buf);
    }
    int16_t *pcm = malloc((size_t)nsamp * nch * 2);
    for (uint32_t s = 0; s < nsamp; s++) for (int ch = 0; ch < nch; ch++) {
        double v = accc[ch][s + N]; long iv = lround(v);
        pcm[(size_t)s * nch + ch] = (int16_t)(iv > 32767 ? 32767 : iv < -32768 ? -32768 : iv);
    }
    wav_write(out, pcm, nch, rate, nsamp);
    return 0;
}

/* ------------------------------------------------------------------ encoder */
static double ath_db(double f) {   /* absolute threshold of hearing (Terhardt), dB SPL */
    f = fmax(f, 20.0) / 1000.0;
    return 3.64 * pow(f, -0.8) - 6.5 * exp(-0.6 * (f - 3.3) * (f - 3.3)) + 1e-3 * pow(f, 4);
}
static double bark(double f) { return 13 * atan(0.00076 * f) + 3.5 * atan((f / 7500) * (f / 7500)); }

/* Tonality of each band from the spectral flatness of its line energies (geometric over arithmetic mean): about -5 dB
 * for noise-like bands in an MDCT, -30 to -60 dB for a few dominant lines.  tonality = min(-SFM_dB / 60, 1). */
static void tonality(int type, const double *coef, double *ton) {
    int nb = nbands(type), idx[N];
    for (int b = 0; b < nb; b++) {
        int n = band_lines(type, b, idx); double lg = 0, ar = 0;
        for (int i = 0; i < n; i++) { double p = coef[idx[i]] * coef[idx[i]] + 1.0; lg += log(p); ar += p; }
        double sfm_db = 10.0 / log(10.0) * (lg / n - log(ar / n));
        ton[b] = fmin(fmax(-sfm_db / 60.0, 0.0), 1.0);
    }
}

/* masking threshold (energy) per band: each band j masks with its energy spread in Bark and lowered by a
 * tonality-dependent offset (tonal maskers mask less: 14.5 + z_j dB; noise-like maskers 5.5 dB, interpolated),
 * then the absolute threshold */
static void psy(int type, const double *coef, const double *E, double *T) {
    int nb = nbands(type); const int *e = edges(type);
    double lw = type == W_SHORT ? 48000.0 / 2 / NS : 48000.0 / 2 / N, z[NBL], ton[NBL], off[NBL];
    for (int b = 0; b < nb; b++) z[b] = bark(lw * (e[b] + e[b + 1]) / 2.0);
    tonality(type, coef, ton);
    for (int j = 0; j < nb; j++)
        off[j] = TONAL ? pow(10.0, -(ton[j] * (14.5 + z[j]) + (1.0 - ton[j]) * 5.5 + SMR_DB) / 10.0) : pow(10.0, -(13.0 + SMR_DB) / 10.0);
    for (int b = 0; b < nb; b++) {
        double s = 0;
        for (int j = 0; j < nb; j++) {
            double dz = z[b] - z[j];   /* masker j, maskee b: masks upward (dz > 0) with -10 dB/Bark, downward -25 dB/Bark */
            double att = dz >= 0 ? -10.0 * dz : 25.0 * dz;
            s += E[j] * off[j] * pow(10.0, att / 10.0);
        }
        double thr = s;
        /* absolute threshold, conservative for loud headphone playback: 0 dBFS sine = 105 dB SPL, and the curve is capped
         * at 40 dB SPL so that quiet high-frequency content is not discarded; per line energy of a full-scale sine is about
         * (32768 * N/2)^2 */
        double full = 32768.0 * (type == W_SHORT ? NS : N) / 2.0, ath = 0;
        for (int k = e[b]; k < e[b + 1]; k++) ath += full * full * pow(10.0, (fmin(ath_db(lw * (k + 0.5)), 40.0) - 105.0) / 10.0);
        if (type == W_SHORT) ath *= 8;
        T[b] = fmax(thr, ath);
    }
}

static int quant1(double x, int sf) { double st = pow(2.0, (sf - SF_OFF) / 4.0); int q = (int)floor(pow(fabs(x) / st, 0.75) + 0.4054); return x < 0 ? -q : q; }


static int encode(const char *in, const char *out, int kbps) {
    int nch, rate; long ns; int16_t *pcm = wav_read(in, &nch, &rate, &ns);
    if (!pcm || rate != 48000) { fprintf(stderr, "need 16-bit 48 kHz mono/stereo WAV\n"); return 1; }
    long nfr = (ns + N - 1) / N + 1;
    double *x[2];
    for (int ch = 0; ch < nch; ch++) {   /* x[ch][i] = sample i - N (N zeros of lead-in), padded after the end */
        x[ch] = calloc((size_t)(nfr + 2) * N, sizeof(double));
        for (long s = 0; s < ns; s++) x[ch][s + N] = pcm[s * nch + ch];
    }
    /* transient detection on the new half of each frame window: high-pass energy of 8 segments of 128 samples */
    int *att = calloc(nfr + 1, sizeof *att), *type = calloc(nfr + 1, sizeof *type);
    double hist = 0;
    for (long t = 0; t < nfr; t++) {
        for (int seg = 0; seg < 8; seg++) {
            double e = 0; long base = t * N + N + seg * 128;   /* frame t covers x[t*N .. t*N + 2N) */
            for (int i = 0; i < 128; i++) for (int ch = 0; ch < nch; ch++) { double d = x[ch][base + i] - x[ch][base + i - 1]; e += d * d; }
            if (e > ATT_RATIO * hist && e > 1e4 * 128) att[t] = 1;
            hist = 0.7 * hist + 0.3 * e;
        }
    }
    /* window sequence: LONG -> LONG | START, START -> SHORT, SHORT -> SHORT | STOP, STOP -> LONG | START.
     * An attack in frame t+1 turns a LONG/STOP frame t into START so that t+1 can be SHORT. */
    int nshort = 0;
    for (long t = 0; t < nfr; t++) {
        int prev = t ? type[t - 1] : W_LONG;
        if (prev == W_START) type[t] = W_SHORT;
        else if (prev == W_SHORT) type[t] = (att[t] || att[t + 1]) ? W_SHORT : W_STOP;
        else type[t] = att[t + 1] ? W_START : W_LONG;
        nshort += type[t] == W_SHORT;
    }
    FILE *o = fopen(out, "wb");
    fwrite("TAC0", 1, 4, o); put32(o, 48000); fputc(nch, o); fputc(0, o); fputc(0, o); fputc(0, o); put32(o, (uint32_t)nfr); put32(o, (uint32_t)ns);
    double mean_bits = kbps * 1000.0 * N / 48000.0, reservoir = 0, rmax = 6 * mean_bits;
    /* encoder bandwidth, rounded to band edges: 128 kbit/s up to 18.6 kHz, 96 kbit/s up to 15.05 kHz */
    double cut = kbps >= 128 ? 18700 : kbps >= 96 ? 15100 : 12900;
    ACtx c; actx_init(&c);
    long total = 0;
    for (long t = 0; t < nfr; t++) {
        Frame f; memset(&f, 0, sizeof f);
        f.type = type[t]; f.reset = t % 16 == 0; f.nfill = NFILL;
        int nb = nbands(f.type); const int *e = edges(f.type); int idx[N];
        double lw = f.type == W_SHORT ? 48000.0 / 2 / NS : 48000.0 / 2 / N;
        int maxb = 0; while (maxb < nb && lw * e[maxb + 1] <= cut) maxb++;
        if (maxb < 1) maxb = 1;
        double coef[2][N], E[2][NBL], T[2][NBL];
        for (int ch = 0; ch < nch; ch++) mdct_frame(x[ch] + t * N, f.type, coef[ch]);
        /* mid/side per band */
        if (nch == 2) {
            f.msmode = 2;
            for (int b = 0; b < maxb; b++) {
                int n = band_lines(f.type, b, idx); double el = 0, er = 0, em = 0, es = 0;
                for (int i = 0; i < n; i++) { double l = coef[0][idx[i]], rr = coef[1][idx[i]], m = (l + rr) / 2, s = (l - rr) / 2; el += l * l; er += rr * rr; em += m * m; es += s * s; }
                f.ms[b] = em * es < 0.5 * el * er;
            }
            if (MSFORCE >= 0) { f.msmode = MSFORCE; for (int b = 0; b < maxb; b++) f.ms[b] = MSFORCE == 1; }   /* conformance streams */
        }
        double cc[2][N]; memcpy(cc, coef, sizeof cc);
        for (int ch = 0; ch < nch; ch++) {
            for (int b = 0; b < nb; b++) { int n = band_lines(f.type, b, idx); double s = 0; for (int i = 0; i < n; i++) s += coef[ch][idx[i]] * coef[ch][idx[i]]; E[ch][b] = s + 1e-9; }
            psy(f.type, coef[ch], E[ch], T[ch]);
        }
        if (nch == 2) for (int b = 0; b < maxb; b++) if (f.ms[b]) {
            int n = band_lines(f.type, b, idx); double tm = fmin(T[0][b], T[1][b]);
            for (int i = 0; i < n; i++) { double l = coef[0][idx[i]], rr = coef[1][idx[i]]; cc[0][idx[i]] = (l + rr) / 2; cc[1][idx[i]] = (l - rr) / 2; }
            T[0][b] = T[1][b] = tm / 2;      /* conservative: the stricter threshold, halved for the M/S split */
        }
        double allowed = mean_bits + (f.type == W_SHORT ? 0.6 : 0.15) * reservoir;
        /* rate loop: bisection on a global noise offset (dB) applied to every band's masking threshold */
        double lo = -30, hi = 40, best_bits = 0; Frame best = f; int have = 0;
        for (int it = 0; it < 12; it++) {
            double alpha = (lo + hi) / 2;
            Frame g = f;
            for (int ch = 0; ch < nch; ch++) {
                for (int b = 0; b < maxb; b++) {
                    int n = band_lines(f.type, b, idx);
                    double allow = T[ch][b] * pow(10.0, alpha / 10.0), en = 0;
                    for (int i = 0; i < n; i++) en += cc[ch][idx[i]] * cc[ch][idx[i]];
                    /* largest scale factor whose quantisation error stays within the allowed noise, but never so small that a
                     * quantised magnitude exceeds QMAX (the bitstream limit) */
                    double amax = 0; for (int i = 0; i < n; i++) amax = fmax(amax, fabs(cc[ch][idx[i]]));
                    int sfmin = amax > 0 ? (int)ceil(4.0 * log2(amax / pow(QMAX - 0.5, 4.0 / 3.0)) + SF_OFF) : 0;
                    sfmin = tvc_clip(sfmin, 0, 255);
                    int sfl = sfmin, sfh = 255;
                    while (sfl < sfh) {
                        int mid = (sfl + sfh + 1) / 2; double err = 0;
                        for (int i = 0; i < n; i++) { double v = cc[ch][idx[i]]; double d = v - dq(quant1(v, mid), mid); err += d * d; }
                        if (err <= allow) sfl = mid; else sfh = mid - 1;
                    }
                    int any = 0;
                    for (int i = 0; i < n; i++) { g.q[ch][idx[i]] = quant1(cc[ch][idx[i]], sfl); any |= g.q[ch][idx[i]] != 0; }
                    if (!any && f.nfill && b >= (f.type == W_SHORT ? NF_START_S : NF_START_L)) {
                        double rms = sqrt(en / n), amp = rms * sqrt(3.0);   /* uniform noise of the band's energy */
                        sfl = tvc_clip((int)lround(4 * log2(fmax(amp, 1e-9)) + SF_OFF), 0, 255);
                    }
                    g.sf[ch][b] = sfl;
                }
                /* the decoder codes scale-factor deltas up to maxband; keep zero-band values close to neighbours */
                for (int b = 1; b < maxb; b++) {
                    int n = band_lines(f.type, b, idx), any = 0; for (int i = 0; i < n; i++) any |= g.q[ch][idx[i]] != 0;
                    if (!any && !(f.nfill && b >= (f.type == W_SHORT ? NF_START_S : NF_START_L))) g.sf[ch][b] = g.sf[ch][b - 1];
                }
            }
            g.maxband = maxb;
            RC r; rc_cnt_init(&r); ACtx tc = c; code_frame(&r, &tc, &g, nch);
            if (r.bits <= allowed || !have) { best = g; best_bits = r.bits; have = 1; }
            if (r.bits > allowed) lo = alpha; else hi = alpha;
        }
        if (best_bits > allowed) { /* even the coarsest tried setting is over budget: accept, the reservoir absorbs it */ }
        RC r; rc_enc_init(&r); code_frame(&r, &c, &best, nch); rc_enc_flush(&r);
        put16(o, (int)r.outlen); fwrite(r.out, 1, r.outlen, o); total += r.outlen + 2; free(r.out);
        reservoir += mean_bits - 8.0 * r.outlen; if (reservoir > rmax) reservoir = rmax;
    }
    fclose(o);
    fprintf(stderr, "%ld frames (%d short), %ld bytes, %.1f kbit/s\n", nfr, nshort, total, total * 8.0 / (ns / 48000.0) / 1000.0);
    return 0;
}

/* self test: perfect reconstruction of the transform chain for a window sequence with every transition */
static int selftest(void) {
    int seq[] = {W_LONG, W_START, W_SHORT, W_SHORT, W_STOP, W_LONG, W_START, W_SHORT, W_STOP, W_START, W_SHORT, W_STOP, W_LONG, W_LONG};
    int nf = sizeof seq / sizeof *seq; long len = (long)(nf + 2) * N;
    double *x = calloc(len, sizeof *x), *y = calloc(len, sizeof *y), coef[N];
    srand(3); for (long i = N; i < (long)nf * N; i++) x[i] = rand() % 65536 - 32768;
    for (int t = 0; t < nf; t++) { mdct_frame(x + (long)t * N, seq[t], coef); imdct_frame(coef, seq[t], y + (long)t * N); }
    double maxe = 0; for (long i = 2 * N; i < (long)(nf - 1) * N; i++) maxe = fmax(maxe, fabs(x[i] - y[i]));
    printf("transform chain (all window transitions): max reconstruction error %.2e (expect < 1e-6)\n", maxe);
    return maxe > 1e-6;
}

#ifndef TAC_NO_MAIN   /* tac_fx.c reuses the syntax and reconstruction code */
int main(int argc, char **argv) {
    tables(); tvc_cost_init();
    if (argc >= 2 && !strcmp(argv[1], "test")) return selftest();
    if (argc >= 4 && !strcmp(argv[1], "dec")) return decode(argv[2], argv[3]);
    if (argc >= 4 && !strcmp(argv[1], "enc")) {
        int kb = 96;      /* default locked at 96 kbit/s: transparent for the owner in the blind test (V-195) */
        for (int i = 4; i + 1 < argc; i += 2) {
            if (!strcmp(argv[i], "-b")) kb = atoi(argv[i + 1]); else if (!strcmp(argv[i], "-n")) NFILL = atoi(argv[i + 1]);
            else if (!strcmp(argv[i], "-m")) SMR_DB = atof(argv[i + 1]); else if (!strcmp(argv[i], "-a")) ATT_RATIO = atof(argv[i + 1]);
            else if (!strcmp(argv[i], "-T")) TONAL = atoi(argv[i + 1]); else if (!strcmp(argv[i], "-M")) MSFORCE = atoi(argv[i + 1]);
        }
        return encode(argv[2], argv[3], kb);
    }
    fprintf(stderr, "usage: tac enc in.wav out.tac [-b kbps] | tac dec in.tac out.wav\n");
    return 1;
}
#endif
