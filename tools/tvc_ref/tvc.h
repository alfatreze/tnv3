/* TVC v0 reference: shared decoder-side definitions (range coder, contexts, syntax, reconstruction, deblocking).
 * The normative description is docs/TVC_BITSTREAM_V0.md; this header must match it.  The macroblock syntax is ONE
 * function (tvc_code_mb) used in three modes: decode, encode, and bit counting, so the encoder and decoder cannot
 * disagree on syntax.  An independent decoder (tools/tvc_ref/py/) checks the specification against this code. */
#ifndef TVC_H
#define TVC_H
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

/* ---------------------------------------------------------------- constants */
#define PROB_ONE 32768
#define PROB_INIT 16384
#define PROB_RATE 5

static const int TVC_T[8][8] = {           /* round(90.5 * c_k * cos((2n+1) k pi / 16)), c_0 = 1/sqrt(2) */
    {64, 64, 64, 64, 64, 64, 64, 64},
    {89, 75, 50, 18, -18, -50, -75, -89},
    {84, 35, -35, -84, -84, -35, 35, 84},
    {75, -18, -89, -50, 50, 89, 18, -75},
    {64, -64, -64, 64, 64, -64, -64, 64},
    {50, -89, 18, 75, -75, -18, 89, -50},
    {35, -84, 84, -35, -35, 84, -84, 35},
    {18, -50, 75, -89, 89, -75, 50, -18}};

static const int TVC_QS[52] = {            /* round(16 * 2^((qp - 4) / 6)): quantiser step in 1/16 units */
    10, 11, 13, 14, 16, 18, 20, 23, 25, 29, 32, 36, 40, 45, 51, 57, 64, 72, 81, 91, 102, 114, 128, 144, 161, 181,
    203, 228, 256, 287, 323, 362, 406, 456, 512, 575, 645, 724, 813, 912, 1024, 1149, 1290, 1448, 1625, 1825,
    2048, 2299, 2580, 2896, 3251, 3649};

static const uint8_t TVC_PCLASS[64] = {    /* scan position -> significance context class */
    0, 1, 2, 3, 4, 5, 6, 6, 7, 7, 8, 8, 8, 8, 9, 9, 9, 9, 9, 9, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10,
    11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12,
    12, 12, 12, 12, 12, 12, 12, 12, 12, 12};

static int TVC_ZZ[64];                     /* scan index -> row*8+col (filled by tvc_init) */
static int TVC_ZZ4[16];                    /* draft 4: 4x4 scan index -> row*4+col */
static const int TVC_T4[4][4] = {          /* round(128 * c_k * sqrt(2/4) * cos((2n+1) k pi / 8)): rows 0,2,4,6 of T */
    {64, 64, 64, 64}, {84, 35, -35, -84}, {64, -64, -64, 64}, {35, -84, 84, -35}};
static const uint8_t TVC_PCLASS4[16] = {0, 1, 2, 3, 4, 5, 6, 6, 7, 7, 7, 7, 8, 8, 8, 8};

enum { PIC_I = 0, PIC_P = 1, PIC_B = 2 };
enum { MB_INTRA = 0, MB_INTER = 1, MB_SKIP = 2 };
enum { IM_DC = 0, IM_V = 1, IM_H = 2, IM_PLANAR = 3 };

#ifdef TVC_PRED_HOOK   /* tools/tvc_ref/tvc_pred_trace.c: reports every intra prediction's inputs and every add-and-clip */
void tvc_pred_hook_intra(int bx, int by, int top, int mode, int ta, int la, const int *T, const int *L);
void tvc_pred_hook_add(const uint8_t *pl, int w, int bx, int by, const uint8_t pred[8][8], int res[8][8]);
#endif
static inline int tvc_clip(int v, int lo, int hi) { return v < lo ? lo : v > hi ? hi : v; }
static inline int tvc_min(int a, int b) { return a < b ? a : b; }

static void zz_fill(int *zz, int N) {
    int i = 0, M = N - 1;
    for (int s = 0; s < 2 * N - 1; s++) {
        if (s & 1) { for (int r = (s > M ? s - M : 0); r <= (s < M ? s : M); r++) zz[i++] = r * N + (s - r); }
        else { for (int r = (s < M ? s : M); r >= (s > M ? s - M : 0); r--) zz[i++] = r * N + (s - r); }
    }
}
static void tvc_init(void) { zz_fill(TVC_ZZ, 8); zz_fill(TVC_ZZ4, 4); }

/* ---------------------------------------------------------------- range coder */
/* TVC_RC_HOOK (tools/tvc_ref/tvc_rc_trace.c): the decoder reports every slice start and every decoded bin (the probability
 * BEFORE its update, the bit, range and code AFTER normalisation) so the range-decoder RTL can be replayed bit for bit. */
#ifdef TVC_RC_HOOK
void tvc_rc_hook_init(const uint8_t *buf, size_t len);
void tvc_rc_hook_bin(int bypass, const uint16_t *pp, unsigned p, int bit, uint32_t range, uint32_t code);
void tvc_rc_hook_ctx(const void *base);                                     /* ctx_init: the probability array of the slice */
void tvc_rc_hook_blk(int end, int cc, int n, const int16_t *lev);           /* a coefficient block starts (end = 0) / ends with its levels */
void tvc_rc_hook_slice(int ptype, int aq, int t4ok, int qp, int mbw, int y0, int y1);   /* a slice starts: picture parameters and its macroblock rows y0 .. y1 - 1 */
#endif
enum { RC_DEC, RC_ENC, RC_CNT };
typedef struct {
    int mode;
    const uint8_t *in; size_t inlen, inpos; uint32_t range, code;
    uint64_t low; uint8_t cache; uint64_t cache_size; uint8_t *out; size_t outlen, outcap;
    double bits;
} RC;

static double TVC_COST[PROB_ONE / 8 + 1];  /* -log2(p) for p = i*8/32768 */
static void tvc_cost_init(void) {
    for (int i = 0; i <= PROB_ONE / 8; i++) TVC_COST[i] = -log2((i ? i * 8.0 : 4.0) / PROB_ONE);
}

static inline uint8_t rc_byte(RC *r) { return r->inpos < r->inlen ? r->in[r->inpos++] : 0; }
static void rc_dec_init(RC *r, const uint8_t *buf, size_t len) {
    memset(r, 0, sizeof *r); r->mode = RC_DEC; r->in = buf; r->inlen = len; r->range = 0xFFFFFFFFu;
    for (int i = 0; i < 5; i++) r->code = (r->code << 8) | rc_byte(r);
#ifdef TVC_RC_HOOK
    tvc_rc_hook_init(buf, len);
#endif
}
static void rc_enc_init(RC *r) {
    memset(r, 0, sizeof *r); r->mode = RC_ENC; r->range = 0xFFFFFFFFu; r->cache_size = 1;
    r->outcap = 1 << 16; r->out = malloc(r->outcap);
}
static void rc_cnt_init(RC *r) { memset(r, 0, sizeof *r); r->mode = RC_CNT; }
static void rc_put(RC *r, uint8_t b) {
    if (r->outlen == r->outcap) { r->outcap *= 2; r->out = realloc(r->out, r->outcap); }
    r->out[r->outlen++] = b;
}
static void rc_shift_low(RC *r) {
    if ((uint32_t)r->low < 0xFF000000u || (r->low >> 32) != 0) {
        uint8_t carry = (uint8_t)(r->low >> 32), t = r->cache;
        do { rc_put(r, (uint8_t)(t + carry)); t = 0xFF; } while (--r->cache_size != 0);
        r->cache = (uint8_t)(r->low >> 24);
    }
    r->cache_size++;
    r->low = (r->low & 0x00FFFFFFu) << 8;
}
static void rc_enc_flush(RC *r) { for (int i = 0; i < 5; i++) rc_shift_low(r); }

/* p = probability (x32768) that the bit is 0.  Returns the bit (decoded, or the one given). */
static inline int rc_bit(RC *r, uint16_t *p, int bit) {
    if (r->mode == RC_DEC) {
        uint32_t bound = (r->range >> 15) * *p;
        if (r->code < bound) { r->range = bound; bit = 0; } else { r->code -= bound; r->range -= bound; bit = 1; }
        while (r->range < (1u << 24)) { r->range <<= 8; r->code = (r->code << 8) | rc_byte(r); }
#ifdef TVC_RC_HOOK
        tvc_rc_hook_bin(0, p, *p, bit, r->range, r->code);
#endif
    } else if (r->mode == RC_ENC) {
        uint32_t bound = (r->range >> 15) * *p;
        if (!bit) r->range = bound; else { r->low += bound; r->range -= bound; }
        while (r->range < (1u << 24)) { r->range <<= 8; rc_shift_low(r); }
    } else {
        r->bits += TVC_COST[(bit ? PROB_ONE - *p : *p) >> 3];
    }
    if (bit) *p -= *p >> PROB_RATE; else *p += (PROB_ONE - *p) >> PROB_RATE;
    return bit;
}
static inline int rc_bypass(RC *r, int bit) {
    if (r->mode == RC_DEC) {
        r->range >>= 1;
        if (r->code >= r->range) { r->code -= r->range; bit = 1; } else bit = 0;
        while (r->range < (1u << 24)) { r->range <<= 8; r->code = (r->code << 8) | rc_byte(r); }
#ifdef TVC_RC_HOOK
        tvc_rc_hook_bin(1, 0, 0, bit, r->range, r->code);
#endif
    } else if (r->mode == RC_ENC) {
        r->range >>= 1;
        if (bit) r->low += r->range;
        while (r->range < (1u << 24)) { r->range <<= 8; rc_shift_low(r); }
    } else r->bits += 1.0;
    return bit;
}
/* adaptive Exp-Golomb of order k: prefix bins with contexts ctx[min(i, n-1)], suffix bypass, MSB first */
static int rc_eg(RC *r, uint16_t *ctx, int n, int k, int v) {
    int i = 0;
    if (r->mode == RC_DEC) {
        int base = 0;
        while (rc_bit(r, &ctx[tvc_min(i, n - 1)], 0)) { base += 1 << k; k++; i++; if (k > 24) { fprintf(stderr, "eg overflow\n"); exit(2); } }
        int s = 0; for (int b = k - 1; b >= 0; b--) s = (s << 1) | rc_bypass(r, 0);
        return base + s;
    }
    int vv = v;
    while (vv >= (1 << k)) { rc_bit(r, &ctx[tvc_min(i, n - 1)], 1); vv -= 1 << k; k++; i++; }
    rc_bit(r, &ctx[tvc_min(i, n - 1)], 0);
    for (int b = k - 1; b >= 0; b--) rc_bypass(r, (vv >> b) & 1);
    return v;
}

/* ---------------------------------------------------------------- contexts */
typedef struct {
    uint16_t skip[3], intra[3], bdir[2], mpm[1], rem[2], cmode[3], cbp[3][3], dqz[2], dqm[3];
    uint16_t mvz[2][3], mvm[2][6], last[3][64], sig[3][13][3], gt1[3][3][3], lrem[3][6];   /* class 2 = luma 4x4 (draft 4) */
    uint16_t t4[3], cbf4[4];
} Ctx;
static void ctx_init(Ctx *c) {
    uint16_t *p = (uint16_t *)c; for (size_t i = 0; i < sizeof *c / 2; i++) p[i] = PROB_INIT;
#ifdef TVC_RC_HOOK
    tvc_rc_hook_ctx(c);
#endif
}

/* ---------------------------------------------------------------- picture and macroblock state */
/* uses: bit 0 = list 0 (P: the latest anchor; B: the previous anchor), bit 1 = list 1 (B: the latest anchor) */
typedef struct {
    int8_t type; uint8_t uses; int16_t mv[2][2], mvd[2][2]; uint8_t coded; uint8_t imode[4]; uint8_t cmode; uint8_t qp;
    uint8_t t4; uint16_t s4;       /* draft 4: bit b = luma block b uses 4x4 transforms; bit 4b+k = its sub-block k coded */
} MB;
typedef struct { int w, h; uint8_t *p[3]; } Plane3;   /* coded size (multiple of 16); chroma w/2 x h/2 */
typedef struct {
    int w, h, mbw, mbh;            /* coded size and macroblock grid */
    int ptype, qp, aq, qpel, t4ok; /* picture header (qpel: quarter-sample vectors, draft 3; t4ok: 4x4 option, draft 4) */
    int prev_qp, prev_dq;          /* running state of the dqp syntax */
    int slice_y0;                  /* draft 5: first macroblock row of the current slice */
    MB *mb;                        /* mbw * mbh */
    Ctx ctx;                       /* reset at every picture (carrying contexts across pictures was measured at about
                                      -1% and rejected: pictures stay independent for seeking, V-191) */
} Pic;

static Plane3 plane_alloc(int w, int h) {
    Plane3 p = {w, h, {malloc(w * h), malloc(w * h / 4), malloc(w * h / 4)}};
    return p;
}
static void plane_free(Plane3 *p) { for (int i = 0; i < 3; i++) free(p->p[i]); }

/* draft 5: a neighbour in another slice (rows before slice_y0) is unavailable, like one outside the picture */
static inline MB *mb_at(Pic *s, int x, int y) { return (x < 0 || y < s->slice_y0 || x >= s->mbw || y >= s->mbh) ? NULL : &s->mb[y * s->mbw + x]; }

/* intra mode of luma 8x8 block b of MB (x,y) as seen by a neighbour: DC if unavailable or not intra */
static int nb_imode(Pic *s, int x, int y, int b) { MB *m = mb_at(s, x, y); return (m && m->type == MB_INTRA) ? m->imode[b] : IM_DC; }
static int nb_coded(Pic *s, int x, int y, int bit) { MB *m = mb_at(s, x, y); return m ? (m->coded >> bit) & 1 : 0; }

/* a neighbour's vector for list L: its mv[L] if it uses L, else (0,0) */
static inline int nmv(const MB *m, int L, int c) { return (m && ((m->uses >> L) & 1)) ? m->mv[L][c] : 0; }
static void mv_pred(Pic *s, int x, int y, int L, int *px, int *py) {
    MB *A = mb_at(s, x - 1, y), *B = mb_at(s, x, y - 1), *C = mb_at(s, x + 1, y - 1), *D = mb_at(s, x - 1, y - 1);
    if (!B) { *px = nmv(A, L, 0); *py = nmv(A, L, 1); return; }
    if (!C) C = D;
    int ax = nmv(A, L, 0), ay = nmv(A, L, 1), bx = nmv(B, L, 0), by = nmv(B, L, 1), cx = nmv(C, L, 0), cy = nmv(C, L, 1);
#define MED(a, b, c) ((a) > (b) ? ((b) > (c) ? (b) : ((a) > (c) ? (c) : (a))) : ((a) > (c) ? (a) : ((b) > (c) ? (c) : (b))))
    *px = MED(ax, bx, cx); *py = MED(ay, by, cy);
#undef MED
}

/* ---------------------------------------------------------------- syntax */
/* coefficient block of N = 64 (8x8, classes 0 luma / 1 chroma) or N = 16 (4x4, class 2) levels in scan order */
static void code_block_n(RC *r, Ctx *c, int cc, int16_t *lev, int N) {
    int last = 0, nb = N == 64 ? 6 : 4;
    const uint8_t *pcl = N == 64 ? TVC_PCLASS : TVC_PCLASS4;
    if (r->mode != RC_DEC) { for (int i = N - 1; i >= 0; i--) if (lev[i]) { last = i; break; } }
    else memset(lev, 0, N * sizeof *lev);
#ifdef TVC_RC_HOOK
    if (r->mode == RC_DEC) tvc_rc_hook_blk(0, cc, N, 0);
#endif
    int node = 1;
    for (int b = nb - 1; b >= 0; b--) { int bit = rc_bit(r, &c->last[cc][node], (last >> b) & 1); node = node * 2 + bit; }
    last = node - N;
    int ngt1 = 0;
    for (int i = last; i >= 0; i--) {
        int nz = 1;
        if (i < last) {
            int n = (lev[i + 1] != 0) + (i + 2 <= last && lev[i + 2] != 0);
            nz = rc_bit(r, &c->sig[cc][pcl[i]][n], lev[i] != 0);
        }
        if (!nz) continue;
        int a = abs(lev[i]);
        int gc = N == 64 ? (i < 6 ? 0 : i < 20 ? 1 : 2) : (i < 3 ? 0 : i < 8 ? 1 : 2);
        int g = rc_bit(r, &c->gt1[cc][gc][tvc_min(ngt1, 2)], a > 1);
        if (g) { a = 2 + rc_eg(r, c->lrem[cc], 6, 0, a - 2); ngt1++; } else a = 1;
        int sg = rc_bypass(r, lev[i] < 0);
        lev[i] = (int16_t)(sg ? -a : a);
    }
#ifdef TVC_RC_HOOK
    if (r->mode == RC_DEC) tvc_rc_hook_blk(1, cc, N, lev);
#endif
}
static void code_block(RC *r, Ctx *c, int cc, int16_t *lev) { code_block_n(r, c, cc, lev, 64); }

static int code_signed_eg(RC *r, uint16_t *zctx, uint16_t *mctx, int n, int v) {
    int nz = rc_bit(r, zctx, v != 0);
    if (!nz) return 0;
    int m = 1 + rc_eg(r, mctx, n, 0, abs(v) - 1);
    int sg = rc_bypass(r, v < 0);
    return sg ? -m : m;
}

/* One macroblock.  Encode/count: *m and lev[6][64] (scan order) are inputs.  Decode: outputs.
 * pv[L][c] = motion vector prediction for list L.  Updates s->prev_qp / s->prev_dq. */
static void tvc_code_mb(Pic *s, RC *r, Ctx *c, int x, int y, MB *m, int16_t lev[6][64], int pv[2][2]) {
    MB *L = mb_at(s, x - 1, y), *T = mb_at(s, x, y - 1);
    if (r->mode == RC_DEC) memset(m, 0, sizeof *m);
    if (s->ptype != PIC_I) {
        int ctx = (L && L->type == MB_SKIP) + (T && T->type == MB_SKIP);
        if (rc_bit(r, &c->skip[ctx], m->type == MB_SKIP)) {
            m->type = MB_SKIP; m->uses = s->ptype == PIC_B ? 3 : 1;
            for (int l = 0; l < 2; l++) for (int k = 0; k < 2; k++) { m->mv[l][k] = ((m->uses >> l) & 1) ? (int16_t)pv[l][k] : 0; m->mvd[l][k] = 0; }
            m->coded = 0; m->qp = s->prev_qp;
            return;
        }
        int ci = (L && L->type == MB_INTRA) + (T && T->type == MB_INTRA);
        m->type = rc_bit(r, &c->intra[ci], m->type == MB_INTRA) ? MB_INTRA : MB_INTER;
    } else m->type = MB_INTRA;

    if (s->aq) {
        int d = r->mode == RC_DEC ? 0 : m->qp - s->prev_qp;
        d = code_signed_eg(r, &c->dqz[s->prev_dq != 0], c->dqm, 3, d);
        m->qp = (uint8_t)tvc_clip(s->prev_qp + d, 0, 51);
        s->prev_qp = m->qp; s->prev_dq = d;
    } else m->qp = (uint8_t)s->qp;

    if (m->type == MB_INTRA) {
        m->uses = 0; memset(m->mv, 0, sizeof m->mv); memset(m->mvd, 0, sizeof m->mvd);
        for (int b = 0; b < 4; b++) {
            int ml = (b & 1) ? m->imode[b - 1] : nb_imode(s, x - 1, y, b + 1);
            int mt = (b & 2) ? m->imode[b - 2] : nb_imode(s, x, y - 1, b + 2);
            int mpm = tvc_min(ml, mt), mode = m->imode[b];
            if (rc_bit(r, c->mpm, mode == mpm)) mode = mpm;
            else {
                int others[3], k = 0, rem = 0;
                for (int q = 0; q < 4; q++) if (q != mpm) { if (q == mode) rem = k; others[k++] = q; }
                int a = rc_bit(r, &c->rem[0], rem != 0);
                rem = a ? 1 + rc_bit(r, &c->rem[1], rem == 2) : 0;
                mode = others[rem];
            }
            m->imode[b] = (uint8_t)mode;
        }
        int cm = m->cmode;
        if (!rc_bit(r, &c->cmode[0], cm != 0)) cm = 0;
        else if (!rc_bit(r, &c->cmode[1], cm != 1)) cm = 1;
        else cm = 2 + rc_bit(r, &c->cmode[2], cm == 3);
        m->cmode = (uint8_t)cm;
    } else {
        int uses = 1;
        if (s->ptype == PIC_B) {                     /* 0 -> both lists; 1 then 0 -> list 0; 1 then 1 -> list 1 */
            if (!rc_bit(r, &c->bdir[0], m->uses != 3)) uses = 3;
            else uses = rc_bit(r, &c->bdir[1], m->uses == 2) ? 2 : 1;
        }
        m->uses = (uint8_t)uses;
        for (int l = 0; l < 2; l++) {
            if (!((uses >> l) & 1)) { m->mv[l][0] = m->mv[l][1] = m->mvd[l][0] = m->mvd[l][1] = 0; continue; }
            for (int k = 0; k < 2; k++) {
                int pd = 0;
                if (L && L->type == MB_INTER && ((L->uses >> l) & 1)) pd += abs(L->mvd[l][k]);
                if (T && T->type == MB_INTER && ((T->uses >> l) & 1)) pd += abs(T->mvd[l][k]);
                int cl = pd < 2 ? 0 : pd < 16 ? 1 : 2;
                int d = r->mode == RC_DEC ? 0 : m->mv[l][k] - pv[l][k];
                d = code_signed_eg(r, &c->mvz[k][cl], c->mvm[k], 6, d);
                m->mvd[l][k] = (int16_t)d; m->mv[l][k] = (int16_t)(pv[l][k] + d);
            }
        }
    }

    int coded = r->mode == RC_DEC ? 0 : m->coded;
    for (int b = 0; b < 4; b++) {
        int lc = (b & 1) ? (coded >> (b - 1)) & 1 : nb_coded(s, x - 1, y, b + 1);
        int tc = (b & 2) ? (coded >> (b - 2)) & 1 : nb_coded(s, x, y - 1, b + 2);
        int bit = rc_bit(r, &c->cbp[0][lc + tc], (coded >> b) & 1);
        coded = (coded & ~(1 << b)) | (bit << b);
    }
    for (int k = 4; k < 6; k++) {
        int lc = nb_coded(s, x - 1, y, k), tc = nb_coded(s, x, y - 1, k);
        int bit = rc_bit(r, &c->cbp[k - 3][lc + tc], (coded >> k) & 1);
        coded = (coded & ~(1 << k)) | (bit << k);
    }
    m->coded = (uint8_t)coded;
    int t4 = r->mode == RC_DEC ? 0 : (m->t4 & coded & 15), s4 = r->mode == RC_DEC ? 0 : m->s4;
    for (int b = 0; b < 4; b++) if (!((t4 >> b) & 1)) s4 &= ~(15 << (4 * b));   /* only 4x4 blocks carry sub flags */
    for (int b = 0; b < 6; b++) {
        if (!((coded >> b) & 1)) continue;
        if (b < 4 && s->t4ok) {                      /* draft 4: transform size of a coded luma block */
            int lt = (b & 1) ? (t4 >> (b - 1)) & 1 : (L ? (L->t4 >> (b + 1)) & 1 : 0);
            int tt = (b & 2) ? (t4 >> (b - 2)) & 1 : (T ? (T->t4 >> (b + 2)) & 1 : 0);
            int bit = rc_bit(r, &c->t4[lt + tt], (t4 >> b) & 1);
            t4 = (t4 & ~(1 << b)) | (bit << b);
            if (bit) {
                for (int k = 0; k < 4; k++) {
                    int sb = rc_bit(r, &c->cbf4[k], (s4 >> (4 * b + k)) & 1);
                    s4 = (s4 & ~(1 << (4 * b + k))) | (sb << (4 * b + k));
                    if (sb) code_block_n(r, c, 2, lev[b] + 16 * k, 16);
                    else if (r->mode == RC_DEC) memset(lev[b] + 16 * k, 0, 16 * sizeof(int16_t));
                }
                continue;
            }
        }
        code_block(r, c, b < 4 ? 0 : 1, lev[b]);
    }
    if (!s->t4ok) { t4 = 0; s4 = 0; }
    m->t4 = (uint8_t)t4; m->s4 = (uint16_t)s4;
}

/* ---------------------------------------------------------------- reconstruction */
static int dequant(int l, int qp) {
    int a = (abs(l) * TVC_QS[qp] + 8) >> 4;
    if (a > 32767) a = 32767;
    return l < 0 ? -a : a;
}
/* inverse transform of levels in scan order -> residual res[8][8] */
static void inv_transform(const int16_t *lev, int qp, int res[8][8]) {
    int C[8][8] = {{0}}, e[8][8];
    for (int i = 0; i < 64; i++) if (lev[i]) C[TVC_ZZ[i] >> 3][TVC_ZZ[i] & 7] = dequant(lev[i], qp);
    for (int y = 0; y < 8; y++) for (int v = 0; v < 8; v++) {
        long s = 0; for (int u = 0; u < 8; u++) s += (long)TVC_T[u][y] * C[u][v];
        e[y][v] = tvc_clip((int)((s + 64) >> 7), -32768, 32767);
    }
    for (int y = 0; y < 8; y++) for (int xx = 0; xx < 8; xx++) {
        long s = 0; for (int v = 0; v < 8; v++) s += (long)e[y][v] * TVC_T[v][xx];
        res[y][xx] = (int)((s + 128) >> 8);
    }
}

/* intra prediction of the 8x8 block at (bx,by) of a plane (stride w) from the current reconstruction */
/* top = first sample row of the current slice in this plane (draft 5); the row above it is not available */
static void intra_pred(const uint8_t *pl, int w, int bx, int by, int top, int mode, uint8_t out[8][8]) {
    int T[8], L[8], ta = by > top, la = bx > 0;
    for (int i = 0; i < 8; i++) { T[i] = ta ? pl[(by - 1) * w + bx + i] : 0; L[i] = la ? pl[(by + i) * w + bx - 1] : 0; }
#ifdef TVC_PRED_HOOK
    tvc_pred_hook_intra(bx, by, top, mode, ta, la, T, L);
#endif
    if (mode == IM_DC) {
        int s = 0, dc;
        for (int i = 0; i < 8; i++) s += (ta ? T[i] : 0) + (la ? L[i] : 0);
        dc = ta && la ? (s + 8) >> 4 : (ta || la) ? (s + 4) >> 3 : 128;
        for (int y = 0; y < 8; y++) for (int x = 0; x < 8; x++) out[y][x] = (uint8_t)dc;
        return;
    }
    if (!ta) for (int i = 0; i < 8; i++) T[i] = la ? L[0] : 128;
    if (!la) for (int i = 0; i < 8; i++) L[i] = ta ? T[0] : 128;
    for (int y = 0; y < 8; y++) for (int x = 0; x < 8; x++) {
        int v;
        if (mode == IM_V) v = T[x];
        else if (mode == IM_H) v = L[y];
        else v = ((7 - x) * L[y] + (x + 1) * T[7] + (7 - y) * T[x] + (y + 1) * L[7] + 8) >> 4;
        out[y][x] = (uint8_t)v;
    }
}

/* half-pel bilinear prediction of a bw x bh block at (bx,by) with vector (mvx,mvy) in half-sample units */
static void inter_pred(const uint8_t *ref, int w, int h, int bx, int by, int bw, int bh, int mvx, int mvy, uint8_t *out, int ostride) {
    int ix = mvx >> 1, iy = mvy >> 1, fx = mvx & 1, fy = mvy & 1;
    for (int y = 0; y < bh; y++) for (int x = 0; x < bw; x++) {
        int x0 = tvc_clip(bx + x + ix, 0, w - 1), x1 = tvc_clip(bx + x + ix + 1, 0, w - 1);
        int y0 = tvc_clip(by + y + iy, 0, h - 1), y1 = tvc_clip(by + y + iy + 1, 0, h - 1);
        int a = ref[y0 * w + x0], v;
        if (fx && fy) v = (a + ref[y0 * w + x1] + ref[y1 * w + x0] + ref[y1 * w + x1] + 2) >> 2;
        else if (fx) v = (a + ref[y0 * w + x1] + 1) >> 1;
        else if (fy) v = (a + ref[y1 * w + x0] + 1) >> 1;
        else v = a;
        out[y * ostride + x] = (uint8_t)v;
    }
}
static int chroma_mv(int v) { return v / 2; }  /* C division: truncates toward zero */

/* draft 3, quarter-sample mode: 4-tap luma filters (Catmull-Rom cubic at t = 0, 1/4, 1/2, 3/4, x64) for taps -1..2 */
static const int TVC_QF[4][4] = {{0, 64, 0, 0}, {-4, 56, 14, -2}, {-4, 36, 36, -4}, {-2, 14, 56, -4}};
static void inter_pred_q(const uint8_t *ref, int w, int h, int bx, int by, int bw, int bh, int mvx, int mvy, uint8_t *out, int ostride) {
    int ix = mvx >> 2, iy = mvy >> 2, fx = mvx & 3, fy = mvy & 3;
#define R(X, Y) ((int)ref[tvc_clip((Y), 0, h - 1) * w + tvc_clip((X), 0, w - 1)])
    for (int y = 0; y < bh; y++) for (int x = 0; x < bw; x++) {
        int X = bx + x + ix, Y = by + y + iy, v;
        if (!fx && !fy) v = R(X, Y);
        else if (!fy) { int s = 0; for (int k = 0; k < 4; k++) s += TVC_QF[fx][k] * R(X + k - 1, Y); v = (s + 32) >> 6; }
        else if (!fx) { int s = 0; for (int k = 0; k < 4; k++) s += TVC_QF[fy][k] * R(X, Y + k - 1); v = (s + 32) >> 6; }
        else {
            int s = 0;
            for (int j = 0; j < 4; j++) { int t = 0; for (int k = 0; k < 4; k++) t += TVC_QF[fx][k] * R(X + k - 1, Y + j - 1); s += TVC_QF[fy][j] * t; }
            v = (s + 2048) >> 12;
        }
        out[y * ostride + x] = (uint8_t)tvc_clip(v, 0, 255);
    }
}
/* quarter-sample mode chroma: the luma vector is in 1/8 chroma samples; bilinear with 1/8 weights */
static void inter_pred_c8(const uint8_t *ref, int w, int h, int bx, int by, int bw, int bh, int mvx, int mvy, uint8_t *out, int ostride) {
    int ix = mvx >> 3, iy = mvy >> 3, fx = mvx & 7, fy = mvy & 7;
    for (int y = 0; y < bh; y++) for (int x = 0; x < bw; x++) {
        int X = bx + x + ix, Y = by + y + iy;
        int v = ((8 - fx) * (8 - fy) * R(X, Y) + fx * (8 - fy) * R(X + 1, Y) + (8 - fx) * fy * R(X, Y + 1) + fx * fy * R(X + 1, Y + 1) + 32) >> 6;
        out[y * ostride + x] = (uint8_t)v;
    }
#undef R
}

/* draft 4: inverse 4x4 transform of 16 levels in 4x4 scan order, added into res at (ox, oy) */
static void inv_transform4(const int16_t *lev, int qp, int res[8][8], int ox, int oy) {
    int C[4][4] = {{0}}, e[4][4];
    for (int i = 0; i < 16; i++) if (lev[i]) C[TVC_ZZ4[i] >> 2][TVC_ZZ4[i] & 3] = dequant(lev[i], qp);
    for (int y = 0; y < 4; y++) for (int v = 0; v < 4; v++) {
        long s = 0; for (int u = 0; u < 4; u++) s += (long)TVC_T4[u][y] * C[u][v];
        e[y][v] = tvc_clip((int)((s + 64) >> 7), -32768, 32767);
    }
    for (int y = 0; y < 4; y++) for (int x = 0; x < 4; x++) {
        long s = 0; for (int v = 0; v < 4; v++) s += (long)e[y][v] * TVC_T4[v][x];
        res[oy + y][ox + x] = (int)((s + 64) >> 7);
    }
}
/* luma residual of block b of an MB: 8x8 transform, or (t4) four 4x4 transforms with their own coded flags */
static void luma_residual(const MB *m, int b, const int16_t *lev, int res[8][8]) {
    memset(res, 0, 64 * sizeof(int));
    if (!((m->coded >> b) & 1)) return;
    if ((m->t4 >> b) & 1) {
        for (int k = 0; k < 4; k++) if ((m->s4 >> (4 * b + k)) & 1) inv_transform4(lev + 16 * k, m->qp, res, (k & 1) * 4, (k >> 1) * 4);
    } else inv_transform(lev, m->qp, res);
}
static void add_res(uint8_t *pl, int w, int bx, int by, const uint8_t pred[8][8], int res[8][8]) {
    for (int y = 0; y < 8; y++) for (int x = 0; x < 8; x++) pl[(by + y) * w + bx + x] = (uint8_t)tvc_clip(pred[y][x] + res[y][x], 0, 255);
#ifdef TVC_PRED_HOOK
    tvc_pred_hook_add(pl, w, bx, by, pred, res);
#endif
}
static void add_residual(uint8_t *pl, int w, int bx, int by, const uint8_t pred[8][8], const int16_t *lev, int qp, int coded) {
    int res[8][8] = {{0}};
    if (coded) inv_transform(lev, qp, res);
    for (int y = 0; y < 8; y++) for (int x = 0; x < 8; x++) pl[(by + y) * w + bx + x] = (uint8_t)tvc_clip(pred[y][x] + res[y][x], 0, 255);
#ifdef TVC_PRED_HOOK
    tvc_pred_hook_add(pl, w, bx, by, pred, res);
#endif
}

/* inter prediction of a whole macroblock (luma 16x16 + two chroma 8x8) from the lists in m->uses; averaged if both */
static void mb_inter_pred(Pic *s, const Plane3 *const ref[2], int x, int y, const MB *m, uint8_t py[256], uint8_t pc[2][64]) {
    int W = s->w, H = s->h, n = 0;
    uint8_t ty[2][256], tc[2][2][64];
    for (int l = 0; l < 2; l++) {
        if (!((m->uses >> l) & 1)) continue;
        if (s->qpel) {
            inter_pred_q(ref[l]->p[0], W, H, x * 16, y * 16, 16, 16, m->mv[l][0], m->mv[l][1], ty[n], 16);
            for (int k = 0; k < 2; k++)
                inter_pred_c8(ref[l]->p[k + 1], W / 2, H / 2, x * 8, y * 8, 8, 8, m->mv[l][0], m->mv[l][1], tc[n][k], 8);
        } else {
            inter_pred(ref[l]->p[0], W, H, x * 16, y * 16, 16, 16, m->mv[l][0], m->mv[l][1], ty[n], 16);
            for (int k = 0; k < 2; k++)
                inter_pred(ref[l]->p[k + 1], W / 2, H / 2, x * 8, y * 8, 8, 8, chroma_mv(m->mv[l][0]), chroma_mv(m->mv[l][1]), tc[n][k], 8);
        }
        n++;
    }
    for (int i = 0; i < 256; i++) py[i] = n == 2 ? (uint8_t)((ty[0][i] + ty[1][i] + 1) >> 1) : ty[0][i];
    for (int k = 0; k < 2; k++) for (int i = 0; i < 64; i++) pc[k][i] = n == 2 ? (uint8_t)((tc[0][k][i] + tc[1][k][i] + 1) >> 1) : tc[0][k][i];
}

/* reconstruct macroblock (x,y) into cur, given its syntax and the reference pictures of lists 0 and 1 */
#ifdef TVC_MB_HOOK     /* tools/tvc_ref/tvc_mb_trace.c: whole-macroblock reconstruction trace (RTL macroblock unit) */
void tvc_mb_hook(int end, Pic *s, Plane3 *cur, const Plane3 *const ref[2], int x, int y, MB *m, int16_t lev[6][64]);
#define TVC_MB_HOOK_CALL(end) tvc_mb_hook(end, s, cur, ref, x, y, m, lev)
#else
#define TVC_MB_HOOK_CALL(end) ((void)0)
#endif
static void tvc_recon_mb(Pic *s, Plane3 *cur, const Plane3 *const ref[2], int x, int y, MB *m, int16_t lev[6][64]) {
    int W = s->w, H = s->h;
    uint8_t pred[8][8];
    TVC_MB_HOOK_CALL(0);
#ifdef TVC_TRACE_MB
    TVC_TRACE_MB(s, x, y, m);    /* memory-traffic study (tools/mem_study/tvc_trace.c) */
#endif
    if (m->type == MB_INTRA) {
        for (int b = 0; b < 4; b++) {
            int bx = x * 16 + (b & 1) * 8, by = y * 16 + (b >> 1) * 8;
            intra_pred(cur->p[0], W, bx, by, s->slice_y0 * 16, m->imode[b], pred);
            int res[8][8]; luma_residual(m, b, lev[b], res); add_res(cur->p[0], W, bx, by, pred, res);
        }
        for (int k = 1; k < 3; k++) {
            intra_pred(cur->p[k], W / 2, x * 8, y * 8, s->slice_y0 * 8, m->cmode, pred);
            add_residual(cur->p[k], W / 2, x * 8, y * 8, pred, lev[3 + k], m->qp, (m->coded >> (3 + k)) & 1);
        }
        TVC_MB_HOOK_CALL(1);
        return;
    }
    (void)H;
    uint8_t p16[16 * 16], pc[2][64];
    mb_inter_pred(s, ref, x, y, m, p16, pc);
    for (int b = 0; b < 4; b++) {
        int ox = (b & 1) * 8, oy = (b >> 1) * 8;
        for (int yy = 0; yy < 8; yy++) for (int xx = 0; xx < 8; xx++) pred[yy][xx] = p16[(oy + yy) * 16 + ox + xx];
        int res[8][8]; luma_residual(m, b, lev[b], res); add_res(cur->p[0], W, x * 16 + ox, y * 16 + oy, pred, res);
    }
    for (int k = 1; k < 3; k++) {
        memcpy(pred, pc[k - 1], 64);
        add_residual(cur->p[k], W / 2, x * 8, y * 8, pred, lev[3 + k], m->qp, (m->coded >> (3 + k)) & 1);
    }
    TVC_MB_HOOK_CALL(1);
}

/* ---------------------------------------------------------------- deblocking */
static void filt_line(uint8_t *p1, uint8_t *p0, uint8_t *q0, uint8_t *q1, int bs, int qp) {
    int qs = TVC_QS[qp], alpha = ((5 * qs) >> 5) + 1, beta = (qs >> 4) + 1, tc = (bs * qs) >> 6;
    if (tc < 1) tc = 1;
    int P1 = *p1, P0 = *p0, Q0 = *q0, Q1 = *q1;
    if (abs(P0 - Q0) >= alpha || abs(P1 - P0) >= beta || abs(Q1 - Q0) >= beta) return;
    int d = tvc_clip(((Q0 - P0) * 4 + (P1 - Q1) + 4) >> 3, -tc, tc);
    *p0 = (uint8_t)tvc_clip(P0 + d, 0, 255); *q0 = (uint8_t)tvc_clip(Q0 - d, 0, 255);
}
/* non-intra macroblocks: different list sets, or a used list whose vectors differ by one luma sample or more
 * (>= 2 in half-sample units, >= 4 in quarter-sample units) */
static int motion_differs(const Pic *s, const MB *P, const MB *Q) {
    int thr = s->qpel ? 4 : 2;
    if (P->uses != Q->uses) return 1;
    for (int l = 0; l < 2; l++)
        if (((P->uses >> l) & 1) && (abs(P->mv[l][0] - Q->mv[l][0]) >= thr || abs(P->mv[l][1] - Q->mv[l][1]) >= thr)) return 1;
    return 0;
}
/* boundary strength between luma 8x8 blocks (bxP,byP) and (bxQ,byQ) (in 8x8 units) */
static int bs_luma(Pic *s, int bxP, int byP, int bxQ, int byQ) {
    MB *P = &s->mb[(byP >> 1) * s->mbw + (bxP >> 1)], *Q = &s->mb[(byQ >> 1) * s->mbw + (bxQ >> 1)];
    if (P->type == MB_INTRA || Q->type == MB_INTRA) return 2;
    int cp = (P->coded >> ((byP & 1) * 2 + (bxP & 1))) & 1, cq = (Q->coded >> ((byQ & 1) * 2 + (bxQ & 1))) & 1;
    if (cp || cq) return 1;
    if (P != Q && motion_differs(s, P, Q)) return 1;
    return 0;
}
static int bs_chroma(const Pic *s, MB *P, MB *Q) {
    if (P->type == MB_INTRA || Q->type == MB_INTRA) return 2;
    if (P->coded || Q->coded) return 1;
    if (motion_differs(s, P, Q)) return 1;
    return 0;
}
static void tvc_deblock(Pic *s, Plane3 *pic) {
    int W = s->w, H = s->h;
    uint8_t *Y = pic->p[0];
    for (int x = 8; x < W; x += 8) for (int y = 0; y < H; y++) {
        int bs = bs_luma(s, x / 8 - 1, y / 8, x / 8, y / 8);
        if (!bs) continue;
        int qp = (s->mb[(y / 16) * s->mbw + (x - 8) / 16].qp + s->mb[(y / 16) * s->mbw + x / 16].qp + 1) >> 1;
        uint8_t *r = Y + y * W;
        filt_line(&r[x - 2], &r[x - 1], &r[x], &r[x + 1], bs, qp);
    }
    for (int y = 8; y < H; y += 8) for (int x = 0; x < W; x++) {
        int bs = bs_luma(s, x / 8, y / 8 - 1, x / 8, y / 8);
        if (!bs) continue;
        int qp = (s->mb[((y - 8) / 16) * s->mbw + x / 16].qp + s->mb[(y / 16) * s->mbw + x / 16].qp + 1) >> 1;
        filt_line(&Y[(y - 2) * W + x], &Y[(y - 1) * W + x], &Y[y * W + x], &Y[(y + 1) * W + x], bs, qp);
    }
    int cw = W / 2, ch = H / 2;
    for (int k = 1; k < 3; k++) {
        uint8_t *C = pic->p[k];
        for (int x = 8; x < cw; x += 8) for (int y = 0; y < ch; y++) {
            MB *P = &s->mb[(y / 8) * s->mbw + x / 8 - 1], *Q = &s->mb[(y / 8) * s->mbw + x / 8];
            int bs = bs_chroma(s, P, Q); if (!bs) continue;
            uint8_t *r = C + y * cw;
            filt_line(&r[x - 2], &r[x - 1], &r[x], &r[x + 1], bs, (P->qp + Q->qp + 1) >> 1);
        }
        for (int y = 8; y < ch; y += 8) for (int x = 0; x < cw; x++) {
            MB *P = &s->mb[(y / 8 - 1) * s->mbw + x / 8], *Q = &s->mb[(y / 8) * s->mbw + x / 8];
            int bs = bs_chroma(s, P, Q); if (!bs) continue;
            filt_line(&C[(y - 2) * cw + x], &C[(y - 1) * cw + x], &C[y * cw + x], &C[(y + 1) * cw + x], bs, (P->qp + Q->qp + 1) >> 1);
        }
    }
}

/* ---------------------------------------------------------------- levels (draft 5) */
typedef struct { int maxw, maxh, maxmbs, maxmbps, maxpic, maxrate, minslices, mvx, mvy; } Level;
/* index = level (1..3); mvx/mvy = largest vector component magnitude in luma samples */
static const Level TVC_LEVEL[4] = {
    {0, 0, 0, 0, 0, 0, 0, 0, 0},
    {400, 240, 375, 11250, 96000, 200000, 1, 256, 128},
    {800, 464, 1450, 43500, 256000, 400000, 1, 256, 128},
    {800, 720, 2250, 67500, 384000, 600000, 2, 256, 128}};

/* ---------------------------------------------------------------- container (.tvc v0, reference only) */
/* "TVC0" u16 width u16 height u16 fps_num u16 fps_den u32 nframes, then per picture u32 length + bytes (little endian)
 * picture bytes: u8 type, u8 qp, u8 flags (bit0 = aq), then range-coded data */
static void put16(FILE *f, int v) { fputc(v & 255, f); fputc((v >> 8) & 255, f); }
static void put32(FILE *f, uint32_t v) { for (int i = 0; i < 4; i++) fputc((v >> (8 * i)) & 255, f); }
static int get16(FILE *f) { int a = fgetc(f); int b = fgetc(f); return a | (b << 8); }
static uint32_t get32(FILE *f) { uint32_t v = 0; for (int i = 0; i < 4; i++) v |= (uint32_t)(fgetc(f) & 255) << (8 * i); return v; }

/* ---------------------------------------------------------------- y4m */
static int y4m_read_header(FILE *f, int *w, int *h, int *fn, int *fd) {
    char line[512]; if (!fgets(line, sizeof line, f)) return 0;
    if (strncmp(line, "YUV4MPEG2", 9)) return 0;
    *fn = 24; *fd = 1;
    for (char *t = strtok(line, " \n"); t; t = strtok(NULL, " \n")) {
        if (t[0] == 'W') *w = atoi(t + 1); else if (t[0] == 'H') *h = atoi(t + 1);
        else if (t[0] == 'F') sscanf(t + 1, "%d:%d", fn, fd);
    }
    return 1;
}
static int y4m_read_frame(FILE *f, uint8_t *buf, size_t n) {
    char line[128]; if (!fgets(line, sizeof line, f)) return 0;
    return fread(buf, 1, n, f) == n;
}
#endif
