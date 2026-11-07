/* TVC v0 reference encoder v2.
 * tvc_enc IN.y4m OUT.tvc [-q QP] [-g GOP] [-b NB] [-i IQP_OFFSET] [-o BQP_OFFSET] [-t TREE] [-a AQ] [-n FRAMES] [-r RECON.y4m]
 *         [-l LEVEL] [-S ROWS] [-P L1,L2,...]   level: per-picture and per-second byte limits enforced by raising the QP;
 *         -P gives the picture lengths just before this input (segmented encodes) so the rate window spans the joint
 * Tools: I/P anchors with NB non-reference B pictures between them (display order I B B P B B P ...), per-MB
 * skip/inter/intra (B: list 0, list 1 or both) decided by rate-distortion cost with bits counted on the live
 * context models, integer + quarter-pel motion search, 4 intra modes per 8x8, 8x8/4x4 transform choice, dead-zone
 * quantiser with RDOQ, scene-cut keys, look-ahead QP propagation ("tree", strength TREE, 0 = off), optional variance AQ.
 * Not yet: full trellis quantisation. The whole source input is held in memory; -r also retains all reconstructions. */
#include "tvc.h"
#include "enc/pixel_ops.h"
#include "enc/bit_ops.h"
#include "enc/qpel.h"
#include "enc/rdoq.h"

static double D8[8][8];
typedef struct { int qp, gop, nb, iqoff, boff, nmax; double aq, tree; const char *recon; int qpel; double cut; int t4, level; } Opt;
#define MIN_KEY 8

static double D4[4][4];
static void dct_init(void) {
    for (int k = 0; k < 8; k++) for (int n = 0; n < 8; n++)
        D8[k][n] = (k ? 1.0 : 1.0 / sqrt(2.0)) * 0.5 * cos((2 * n + 1) * k * M_PI / 16.0);
    for (int k = 0; k < 4; k++) for (int n = 0; n < 4; n++)
        D4[k][n] = (k ? 1.0 : 1.0 / sqrt(2.0)) * sqrt(0.5) * cos((2 * n + 1) * k * M_PI / 8.0);
}
static void fdct(const int r[8][8], double C[8][8]) {
    double t[8][8];
    for (int k = 0; k < 8; k++) for (int x = 0; x < 8; x++) { double s = 0; for (int y = 0; y < 8; y++) s += D8[k][y] * r[y][x]; t[k][x] = s; }
    for (int k = 0; k < 8; k++) for (int l = 0; l < 8; l++) { double s = 0; for (int x = 0; x < 8; x++) s += t[k][x] * D8[l][x]; C[k][l] = s; }
}
static int quant(double C[8][8], int qp, double f, int16_t *lev) {
    double step = TVC_QS[qp] / 16.0; int nz = 0;
    for (int i = 0; i < 64; i++) {
        double c = C[TVC_ZZ[i] >> 3][TVC_ZZ[i] & 7];
        int a = (int)floor(fabs(c) / step + f);
        if (a > 30000) a = 30000;
        lev[i] = (int16_t)(c < 0 ? -a : a); nz += a != 0;
    }
    return nz;
}
static double lambda_of(int qp) { return 0.85 * pow(2.0, (qp - 12) / 3.0); }

static int frame_w, frame_h;
static Pic S; static Plane3 SRC, CUR;
static const Plane3 *REFS[2];

static long sse8(const uint8_t *a, int aw, int ax, int ay, const uint8_t pred[8][8], const int res[8][8]) {
    long s = 0;
    for (int y = 0; y < 8; y++) for (int x = 0; x < 8; x++) {
        int v = tvc_clip(pred[y][x] + (res ? res[y][x] : 0), 0, 255) - a[(ay + y) * aw + ax + x]; s += v * v;
    }
    return s;
}
/* Coefficient coding touches only the selected class; avoid copying unrelated models on every RDOQ trial. */
static double coefficient_bits(int cc, int16_t *lev, int n) {
    RC r; rc_cnt_init(&r); Ctx c;
    memcpy(c.last[cc], S.ctx.last[cc], sizeof c.last[cc]);
    memcpy(c.sig[cc], S.ctx.sig[cc], sizeof c.sig[cc]);
    memcpy(c.gt1[cc], S.ctx.gt1[cc], sizeof c.gt1[cc]);
    memcpy(c.lrem[cc], S.ctx.lrem[cc], sizeof c.lrem[cc]);
    code_block_n(&r, &c, cc, lev, n); return r.bits;
}
static double block_bits(int cc, int16_t *lev) { return coefficient_bits(cc, lev, 64); }
static double block4_bits(int16_t *lev) { return coefficient_bits(2, lev, 16); }

/* Rate-distortion quantisation (encoder only): after the dead-zone quantiser, walk the levels from the highest scan
 * position down and lower each magnitude by one (possibly to zero) when distortion + lambda * bits decreases; bits are
 * counted with the live context models.  cs = coefficient values (orthonormal units) in scan order.  Returns the
 * number of non-zero levels left. */
static int RDOQ = 1;
static int rdoq(const double *cs, int16_t *lev, int N, int qp, int cc, double lam) {
    return enc_rdoq(cs, lev, N, qp, cc, lam, &S.ctx);
}

/* code one 8x8 residual: fills lev, returns 1 if coded; *dist = SSE of the result, *bits = coefficient bits */
static int code_residual(int plane, int bx, int by, const uint8_t pred[8][8], int qp, double f, double lam, int cc,
                         int16_t *lev, long *dist, double *bits, int res_out[8][8]) {
    int pw = plane ? S.w / 2 : S.w, r[8][8], res[8][8];
    const uint8_t *src = SRC.p[plane];
    for (int y = 0; y < 8; y++) for (int x = 0; x < 8; x++) r[y][x] = src[(by + y) * pw + bx + x] - pred[y][x];
    double C[8][8]; fdct(r, C);
    long d0 = sse8(src, pw, bx, by, pred, NULL);
    if (!quant(C, qp, f, lev)) { *dist = d0; *bits = 0; return 0; }
    if (RDOQ) { double cs[64]; for (int i = 0; i < 64; i++) cs[i] = C[TVC_ZZ[i] >> 3][TVC_ZZ[i] & 7]; rdoq(cs, lev, 64, qp, cc, lam); }
    inv_transform(lev, qp, res);
    long d1 = sse8(src, pw, bx, by, pred, res);
    double b1 = block_bits(cc, lev) + 1.0;
    if (d0 <= d1 + lam * b1) { memset(lev, 0, 64 * sizeof *lev); *dist = d0; *bits = 0; return 0; }
    if (res_out) memcpy(res_out, res, sizeof res);
    *dist = d1; *bits = b1; return 1;
}

/* luma 8x8 residual with the draft-4 choice between one 8x8 transform and four 4x4 transforms (when the picture
 * allows it).  Fills lev (64 levels, or 4 x 16 for 4x4), *t4, *s4 (4 sub-block coded bits), res (reconstructed
 * residual), returns 1 if the block is coded. */
static int code_luma_residual(int bx, int by, const uint8_t pred[8][8], int qp, double f, double lam,
                              int16_t *lev, int *t4, int *s4, int res[8][8], long *dist, double *bits) {
    *t4 = 0; *s4 = 0;
    memset(res, 0, 64 * sizeof(int));
    int c8 = code_residual(0, bx, by, pred, qp, f, lam, 0, lev, dist, bits, res);
    if (!S.t4ok) return c8;
    double J8 = *dist + lam * (*bits + (c8 ? 1.0 : 0.0));
    const uint8_t *src = SRC.p[0]; int w = S.w;
    int16_t l4[64]; int r4[8][8] = {{0}}, sub = 0; long d4 = 0; double b4 = 1.0 + 4.0;
    double step = TVC_QS[qp] / 16.0;
    for (int k = 0; k < 4; k++) {
        int ox = (k & 1) * 4, oy = (k >> 1) * 4, rr[4][4]; double C[4][4], t[4][4];
        for (int y = 0; y < 4; y++) for (int x = 0; x < 4; x++) rr[y][x] = src[(by + oy + y) * w + bx + ox + x] - pred[oy + y][ox + x];
        for (int a = 0; a < 4; a++) for (int x = 0; x < 4; x++) { double s = 0; for (int y = 0; y < 4; y++) s += D4[a][y] * rr[y][x]; t[a][x] = s; }
        for (int a = 0; a < 4; a++) for (int b = 0; b < 4; b++) { double s = 0; for (int x = 0; x < 4; x++) s += t[a][x] * D4[b][x]; C[a][b] = s; }
        int16_t *L = l4 + 16 * k; int nz = 0;
        for (int i = 0; i < 16; i++) {
            double cc = C[TVC_ZZ4[i] >> 2][TVC_ZZ4[i] & 3]; int a = (int)floor(fabs(cc) / step + f);
            if (a > 30000) a = 30000;
            L[i] = (int16_t)(cc < 0 ? -a : a); nz += a != 0;
        }
        if (nz && RDOQ) { double cs[16]; for (int i = 0; i < 16; i++) cs[i] = C[TVC_ZZ4[i] >> 2][TVC_ZZ4[i] & 3]; nz = rdoq(cs, L, 16, qp, 2, lam); }
        long dz = 0, dc = 0;
        for (int y = 0; y < 4; y++) for (int x = 0; x < 4; x++) { int v = rr[y][x]; dz += v * v; }
        if (nz) {
            int tmp[8][8] = {{0}}; inv_transform4(L, qp, tmp, ox, oy);
            for (int y = 0; y < 4; y++) for (int x = 0; x < 4; x++) {
                int v = tvc_clip(pred[oy + y][ox + x] + tmp[oy + y][ox + x], 0, 255) - src[(by + oy + y) * w + bx + ox + x]; dc += v * v; }
            double bb = block4_bits(L);
            if (dc + lam * bb < dz) { sub |= 1 << k; d4 += dc; b4 += bb; for (int y = 0; y < 4; y++) for (int x = 0; x < 4; x++) r4[oy + y][ox + x] = tmp[oy + y][ox + x]; continue; }
        }
        memset(L, 0, 16 * sizeof *L); d4 += dz;
    }
    if (!sub || d4 + lam * b4 >= J8) return c8;
    memcpy(lev, l4, sizeof l4); memcpy(res, r4, sizeof r4);
    *t4 = 1; *s4 = sub; *dist = d4; *bits = b4;
    return 1;
}

static double mb_bits(int x, int y, MB *m, int16_t lev[6][64], int pv[2][2]) {
    RC r; rc_cnt_init(&r); Ctx c = S.ctx; int pq = S.prev_qp, pd = S.prev_dq;
    MB tmp = *m; tvc_code_mb(&S, &r, &c, x, y, &tmp, lev, pv);
    S.prev_qp = pq; S.prev_dq = pd;
    return r.bits;
}

/* ---- intra candidate: writes its luma reconstruction into CUR (final recon is redone by tvc_recon_mb) */
static void eval_intra(int x, int y, int qp, MB *m, int16_t lev[6][64], long *dist_out) {
    double lam = lambda_of(qp); long dist = 0;
    memset(m, 0, sizeof *m); m->type = MB_INTRA; m->qp = (uint8_t)qp;
    for (int b = 0; b < 4; b++) {
        int bx = x * 16 + (b & 1) * 8, by = y * 16 + (b >> 1) * 8;
        int ml = (b & 1) ? m->imode[b - 1] : nb_imode(&S, x - 1, y, b + 1);
        int mt = (b & 2) ? m->imode[b - 2] : nb_imode(&S, x, y - 1, b + 2);
        int mpm = tvc_min(ml, mt);
        double best = 1e30; int bm = 0, bcoded = 0, bt4 = 0, bs4 = 0; int16_t blev[64]; int bres[8][8];
        for (int mode = 0; mode < 4; mode++) {
            uint8_t pred[8][8]; int16_t l[64]; long d; double bits; int t4, s4, res[8][8];
            intra_pred(CUR.p[0], S.w, bx, by, S.slice_y0 * 16, mode, pred);
            int cd = code_luma_residual(bx, by, pred, qp, 1.0 / 3, lam, l, &t4, &s4, res, &d, &bits);
            double J = d + lam * (bits + (mode == mpm ? 1.0 : 2.6));
            if (J < best) { best = J; bm = mode; bcoded = cd; bt4 = t4; bs4 = s4; memcpy(blev, l, sizeof blev); memcpy(bres, res, sizeof bres); }
        }
        m->imode[b] = (uint8_t)bm; memcpy(lev[b], blev, sizeof blev);
        if (bcoded) m->coded |= 1 << b;
        if (bt4) { m->t4 |= 1 << b; m->s4 |= (uint16_t)(bs4 << (4 * b)); }
        uint8_t pred[8][8]; intra_pred(CUR.p[0], S.w, bx, by, S.slice_y0 * 16, bm, pred);
        add_res(CUR.p[0], S.w, bx, by, pred, bres);
        dist += sse8(SRC.p[0], S.w, bx, by, pred, bres);
    }
    long bestc = -1; int cm = 0;
    for (int mode = 0; mode < 4; mode++) {
        long d = 0; uint8_t pred[8][8];
        for (int k = 1; k < 3; k++) { intra_pred(CUR.p[k], S.w / 2, x * 8, y * 8, S.slice_y0 * 8, mode, pred); d += sse8(SRC.p[k], S.w / 2, x * 8, y * 8, pred, NULL); }
        if (bestc < 0 || d < bestc) { bestc = d; cm = mode; }
    }
    m->cmode = (uint8_t)cm;
    for (int k = 1; k < 3; k++) {
        uint8_t pred[8][8]; long d; double bits;
        intra_pred(CUR.p[k], S.w / 2, x * 8, y * 8, S.slice_y0 * 8, cm, pred);
        if (code_residual(k, x * 8, y * 8, pred, qp, 1.0 / 3, lam, 1, lev[3 + k], &d, &bits, NULL)) m->coded |= 1 << (3 + k);
        dist += d;
    }
    *dist_out = dist;
}

/* ---- inter candidate with m->uses / m->mv set; residual optional (off for skip) */
static long inter_mb(int x, int y, int qp, double f, int allow_res, MB *m, int16_t lev[6][64]) {
    double lam = lambda_of(qp); long dist = 0;
    uint8_t p16[256], pc[2][64], pred[8][8];
    mb_inter_pred(&S, REFS, x, y, m, p16, pc);
    m->coded = 0; m->t4 = 0; m->s4 = 0;
    for (int b = 0; b < 6; b++) {
        int plane = b < 4 ? 0 : b - 3, bx, by, pw = plane ? S.w / 2 : S.w; long d; double bits;
        if (b < 4) { int ox = (b & 1) * 8, oy = (b >> 1) * 8; bx = x * 16 + ox; by = y * 16 + oy;
            for (int yy = 0; yy < 8; yy++) for (int xx = 0; xx < 8; xx++) pred[yy][xx] = p16[(oy + yy) * 16 + ox + xx]; }
        else { bx = x * 8; by = y * 8; memcpy(pred, pc[b - 4], 64); }
        if (allow_res && b < 4) {
            int t4, s4, res[8][8];
            if (code_luma_residual(bx, by, pred, qp, f, lam, lev[b], &t4, &s4, res, &d, &bits)) m->coded |= 1 << b;
            if (t4) { m->t4 |= 1 << b; m->s4 |= (uint16_t)(s4 << (4 * b)); }
        }
        else if (allow_res) { if (code_residual(plane, bx, by, pred, qp, f, lam, 1, lev[b], &d, &bits, NULL)) m->coded |= 1 << b; }
        else { memset(lev[b], 0, 128); d = sse8(SRC.p[plane], pw, bx, by, pred, NULL); }
        dist += d;
    }
    return dist;
}

static int sad16(const Plane3 *ref, const Plane3 *src, int x, int y, int mvx, int mvy) {
    int unit = S.qpel ? 4 : 2;
    if (!(mvx & (unit - 1)) && !(mvy & (unit - 1))) {
        int rx = x * 16 + mvx / unit, ry = y * 16 + mvy / unit;
        if (rx >= 0 && ry >= 0 && rx + 16 <= S.w && ry + 16 <= S.h) {
            const uint8_t *a = src->p[0] + y * 16 * S.w + x * 16;
            const uint8_t *b = ref->p[0] + ry * S.w + rx;
            return enc_sad16_pixels(a, S.w, b, S.w);
        }
    }
    uint8_t p[256];
    if (S.qpel) {
        if (!enc_pred_q16(ref->p[0], S.w, S.h, x * 16, y * 16, mvx, mvy, p, 16))
            inter_pred_q(ref->p[0], S.w, S.h, x * 16, y * 16, 16, 16, mvx, mvy, p, 16);
    }
    else inter_pred(ref->p[0], S.w, S.h, x * 16, y * 16, 16, 16, mvx, mvy, p, 16);
    const uint8_t *sp = src->p[0] + y * 16 * S.w + x * 16;
    return enc_sad16_pixels(p, 16, sp, S.w);
}
static double mvbits(int d) { return enc_mvbits(d); }
#define MVX_PIX 64   /* search range in luma samples */
#define MVY_PIX 32
static const int DX8[8] = {-1, 0, 1, -1, 1, -1, 0, 1}, DY8[8] = {-1, -1, -1, 0, 0, 1, 1, 1};

/* search list L (reference ref) around the prediction; subpel = 0 for an integer-only search; returns the cost.
 * Vectors are in the picture's units: U = 2 per sample (half-sample mode) or 4 (quarter-sample mode). */
static double motion_search(const Plane3 *ref, const Plane3 *src, int x, int y, int L, int px, int py, double ls, int subpel, int *bx_, int *by_) {
    int U = S.qpel ? 4 : 2, XM = MVX_PIX * U, YM = MVY_PIX * U;
    MB *A = mb_at(&S, x - 1, y), *B = mb_at(&S, x, y - 1), *C = mb_at(&S, x + 1, y - 1);
    int cand[5][2] = {{px, py}, {0, 0}, {nmv(A, L, 0), nmv(A, L, 1)}, {nmv(B, L, 0), nmv(B, L, 1)}, {nmv(C, L, 0), nmv(C, L, 1)}};
    int bx = 0, by = 0; double best = 1e30;
#define COST(mx, my) (sad16(ref, src, x, y, (mx), (my)) + ls * (mvbits((mx) - px) + mvbits((my) - py)))
    for (int i = 0; i < 5; i++) {
        int mx = tvc_clip(cand[i][0] & ~(U - 1), -XM, XM), my = tvc_clip(cand[i][1] & ~(U - 1), -YM, YM);
        double c = COST(mx, my); if (c < best) { best = c; bx = mx; by = my; }
    }
    for (int step = 8 * U; step >= U; step /= 2) {
        int improved = 1;
        while (improved) {
            improved = 0; int cx = bx, cy = by;
            for (int d = 0; d < 8; d++) {
                int mx = cx + DX8[d] * step, my = cy + DY8[d] * step;
                if (abs(mx) > XM || abs(my) > YM) continue;
                double c = COST(mx, my); if (c < best) { best = c; bx = mx; by = my; improved = 1; }
            }
            if (step > U) break;
        }
    }
    for (int sub = U / 2; subpel && sub >= 1; sub /= 2) {   /* half-sample, then quarter-sample refinement */
        int cx = bx, cy = by;
        for (int d = 0; d < 8; d++) { int mx = cx + DX8[d] * sub, my = cy + DY8[d] * sub; double c = COST(mx, my); if (c < best) { best = c; bx = mx; by = my; } }
    }
#undef COST
    *bx_ = bx; *by_ = by; return best;
}

static void pad_frame(const uint8_t *buf, Plane3 *dst) {
    int cw = (frame_w + 1) / 2, ch = (frame_h + 1) / 2;
    const uint8_t *pl[3] = {buf, buf + frame_w * frame_h, buf + frame_w * frame_h + cw * ch};
    int ww[3] = {frame_w, cw, cw}, hh[3] = {frame_h, ch, ch}, W[3] = {S.w, S.w / 2, S.w / 2}, H[3] = {S.h, S.h / 2, S.h / 2};
    for (int k = 0; k < 3; k++) for (int y = 0; y < H[k]; y++) for (int x = 0; x < W[k]; x++)
        dst->p[k][y * W[k] + x] = pl[k][tvc_min(y, hh[k] - 1) * ww[k] + tvc_min(x, ww[k] - 1)];
}

static void aq_offsets(const Plane3 *src, double strength, double *off) {
    int n = S.mbw * S.mbh; double mean = 0;
    for (int i = 0; i < n; i++) {
        int x = i % S.mbw, y = i / S.mbw; double s = 0, s2 = 0;
        for (int yy = 0; yy < 16; yy++) for (int xx = 0; xx < 16; xx++) { int v = src->p[0][(y * 16 + yy) * S.w + x * 16 + xx]; s += v; s2 += v * v; }
        off[i] = log2(s2 / 256 - (s / 256) * (s / 256) + 1.0); mean += off[i];
    }
    mean /= n;
    for (int i = 0; i < n; i++) off[i] = strength * (off[i] - mean);
}

/* Look-ahead analysis on the source, display order: per macroblock an intra cost (mean absolute deviation) and the
 * best integer-motion cost against the previous source frame (capped at the intra cost), plus that vector. */
typedef struct { double *intra, *inter; int *mvx, *mvy; } Look;
static Look look_costs(Plane3 *src, int n) {
    int nm = S.mbw * S.mbh; Look L;
    L.intra = malloc((size_t)n * nm * sizeof(double)); L.inter = malloc((size_t)n * nm * sizeof(double));
    L.mvx = calloc((size_t)n * nm, sizeof(int)); L.mvy = calloc((size_t)n * nm, sizeof(int));
    for (int t = 0; t < n; t++) for (int i = 0; i < nm; i++) {
        int x = i % S.mbw, y = i / S.mbw; double s = 0, a = 0;
        for (int yy = 0; yy < 16; yy++) for (int xx = 0; xx < 16; xx++) s += src[t].p[0][(y * 16 + yy) * S.w + x * 16 + xx];
        s /= 256;
        for (int yy = 0; yy < 16; yy++) for (int xx = 0; xx < 16; xx++) a += fabs(src[t].p[0][(y * 16 + yy) * S.w + x * 16 + xx] - s);
        size_t k = (size_t)t * nm + i;
        L.intra[k] = a + 64.0; L.inter[k] = L.intra[k];
        if (t == 0) continue;
        int mx, my;
        double c = motion_search(&src[t - 1], &src[t], x, y, 0, 0, 0, 0.0, 0, &mx, &my) + 64.0;
        if (c < L.inter[k]) { L.inter[k] = c; L.mvx[k] = mx; L.mvy[k] = my; }
    }
    return L;
}
/* frame-level ratio of motion-compensated cost to intra cost: near 1 at a scene cut */
static double look_ratio(const Look *L, int t) {
    int nm = S.mbw * S.mbh; double a = 0, b = 0;
    for (int i = 0; i < nm; i++) { a += L->inter[(size_t)t * nm + i]; b += L->intra[(size_t)t * nm + i]; }
    return a / b;
}

/* Look-ahead QP propagation: walk display order backwards; each macroblock of frame t passes the share of its
 * information that motion compensation takes from frame t-1 (1 - inter/intra cost) to the macroblocks it overlaps there.
 * A macroblock's QP offset is -strength * log2((intra + propagated) / intra).  An I frame does not propagate into the
 * frame before it. */
static void tree_offsets(const Look *L, const char *isI, int n, double strength, double **off) {
    int nm = S.mbw * S.mbh;
    /* Backward propagation only reads frame t and writes frame t-1. Once t
     * has been processed, its completed offsets can be retained and the
     * propagation buffer reused for the next preceding frame. */
    double *prop = calloc((size_t)2 * nm, sizeof *prop);
    double *cur = prop, *prev = prop + nm;
    const double *intra = L->intra;
    for (int t = n - 1; t >= 0; t--) {
        for (int i = 0; i < nm; i++) {
            double in = intra[(size_t)t * nm + i];
            off[t][i] = -strength * log2((in + cur[i]) / in);
        }
        if (t == 0) break;
        memset(prev, 0, (size_t)nm * sizeof *prev);
        if (!isI[t]) {
            for (int i = 0; i < nm; i++) {
                int x = i % S.mbw, y = i / S.mbw;
                size_t k = (size_t)t * nm + i;
                int mx = L->mvx[k], my = L->mvy[k];
                double in = intra[k], inter = L->inter[k];
                double amount = (in + cur[i]) * (1.0 - inter / in);
                if (amount <= 0) continue;
                /* overlapped macroblocks in t-1 (full-sample displacement mx/2, my/2) */
                int sh = S.qpel ? 2 : 1, px = x * 16 + (mx >> sh), py = y * 16 + (my >> sh);
                int bx0 = (int)floor(px / 16.0), by0 = (int)floor(py / 16.0);
                for (int dy = 0; dy < 2; dy++) for (int dx = 0; dx < 2; dx++) {
                    int bx = bx0 + dx, by = by0 + dy;
                    if (bx < 0 || by < 0 || bx >= S.mbw || by >= S.mbh) continue;
                    int ox = 16 - abs(px - bx * 16), oy = 16 - abs(py - by * 16);
                    if (ox <= 0 || oy <= 0) continue;
                    prev[by * S.mbw + bx] += amount * ox * oy / 256.0;
                }
            }
        }
        double *tmp = cur; cur = prev; prev = tmp;
    }
    free(prop);
}

static int SLICE_ROWS = 0;   /* macroblock rows per slice; 0 = one slice, no slice header (draft 4 layout) */
static const Level *LEV = NULL;
/* level rate window (TVC 8, 7b.19): picture lengths in decoding order, including those of an earlier segment (-P) */
static long *HIST; static int NHIST, WIN, RATEWIN = 1;   /* -R 0: no window enforcement (negative test vectors only) */

/* encode one picture into a malloc'd buffer (picture header + slice table + slice data); returns its length */
static size_t encode_picture_buf(uint8_t **outp, int ptype, int qp, const double *off, int aq_on) {
    S.ptype = ptype; S.qp = qp; S.aq = aq_on;
    int rows = SLICE_ROWS > 0 ? SLICE_ROWS : S.mbh, ns = (S.mbh + rows - 1) / rows;
    RC *rs = calloc(ns, sizeof *rs);
    static int16_t lev[6][64], blev[6][64], tlev[6][64];
    for (int k = 0; k < ns; k++) {
    RC *r = &rs[k];
    S.slice_y0 = k * rows; S.prev_qp = qp; S.prev_dq = 0;
    ctx_init(&S.ctx); rc_enc_init(r);
    for (int y = S.slice_y0; y < S.mbh && y < S.slice_y0 + rows; y++) for (int x = 0; x < S.mbw; x++) {
        int mqp = aq_on ? tvc_clip(qp + (int)lround(off[y * S.mbw + x]), 0, 51) : qp;
        int pv[2][2] = {{0, 0}, {0, 0}};
        double lam = lambda_of(mqp), ls = sqrt(lam), bestJ = 1e30;
        MB best = {0}, cand;
        if (ptype != PIC_I) {
            mv_pred(&S, x, y, 0, &pv[0][0], &pv[0][1]); mv_pred(&S, x, y, 1, &pv[1][0], &pv[1][1]);
            double f = ptype == PIC_B ? 1.0 / 8 : 1.0 / 6;
            memset(&cand, 0, sizeof cand); cand.type = MB_SKIP; cand.uses = ptype == PIC_B ? 3 : 1; cand.qp = (uint8_t)S.prev_qp;
            for (int l = 0; l < 2; l++) if ((cand.uses >> l) & 1) { cand.mv[l][0] = (int16_t)pv[l][0]; cand.mv[l][1] = (int16_t)pv[l][1]; }
            long d = inter_mb(x, y, mqp, 0, 0, &cand, tlev);
            double J = d + lam * mb_bits(x, y, &cand, tlev, pv);
            if (J < bestJ) { bestJ = J; best = cand; memcpy(blev, tlev, sizeof blev); }
            int mv[2][2]; int nl = ptype == PIC_B ? 2 : 1;
            for (int l = 0; l < nl; l++) motion_search(REFS[l], &SRC, x, y, l, pv[l][0], pv[l][1], ls, 1, &mv[l][0], &mv[l][1]);
            for (int uses = 1; uses <= (ptype == PIC_B ? 3 : 1); uses++) {
                memset(&cand, 0, sizeof cand); cand.type = MB_INTER; cand.uses = (uint8_t)uses; cand.qp = (uint8_t)mqp;
                for (int l = 0; l < 2; l++) if ((uses >> l) & 1) { cand.mv[l][0] = (int16_t)mv[l][0]; cand.mv[l][1] = (int16_t)mv[l][1]; }
                d = inter_mb(x, y, mqp, f, 1, &cand, tlev);
                J = d + lam * mb_bits(x, y, &cand, tlev, pv);
                if (J < bestJ) { bestJ = J; best = cand; memcpy(blev, tlev, sizeof blev); }
                /* A cheap vector can win with residuals even when SAD search selects another vector. */
                int different = 0;
                for (int l = 0; l < 2; l++) if ((uses >> l) & 1) {
                    different |= mv[l][0] != pv[l][0] || mv[l][1] != pv[l][1];
                    cand.mv[l][0] = (int16_t)pv[l][0]; cand.mv[l][1] = (int16_t)pv[l][1];
                }
                if (different) {
                    d = inter_mb(x, y, mqp, f, 1, &cand, tlev);
                    J = d + lam * mb_bits(x, y, &cand, tlev, pv);
                    if (J < bestJ) { bestJ = J; best = cand; memcpy(blev, tlev, sizeof blev); }
                }
            }
        }
        long di; eval_intra(x, y, mqp, &cand, tlev, &di);
        double J = di + lam * mb_bits(x, y, &cand, tlev, pv);
        if (J < bestJ) { best = cand; memcpy(blev, tlev, sizeof blev); }
        MB *m = &S.mb[y * S.mbw + x];
        *m = best; memcpy(lev, blev, sizeof lev);
        tvc_code_mb(&S, r, &S.ctx, x, y, m, lev, pv);
        tvc_recon_mb(&S, &CUR, REFS, x, y, m, lev);
    }
    rc_enc_flush(r);
    }
    S.slice_y0 = 0;
    tvc_deblock(&S, &CUR);
    int sliced = SLICE_ROWS > 0;
    size_t len = 3 + (sliced ? 1 + 4 * (ns - 1) : 0);
    for (int k = 0; k < ns; k++) len += rs[k].outlen;
    uint8_t *b = malloc(len), *p = b;
    *p++ = (uint8_t)ptype; *p++ = (uint8_t)qp; *p++ = (uint8_t)(aq_on | (S.qpel << 1) | (S.t4ok << 2) | (sliced << 3));
    if (sliced) {
        *p++ = (uint8_t)rows;
        for (int k = 0; k < ns - 1; k++) { uint32_t l = (uint32_t)rs[k].outlen; for (int i = 0; i < 4; i++) *p++ = (uint8_t)(l >> (8 * i)); }
    }
    for (int k = 0; k < ns; k++) { memcpy(p, rs[k].out, rs[k].outlen); p += rs[k].outlen; free(rs[k].out); }
    free(rs);
    *outp = b; return len;
}

/* Largest length the next picture may have under the level's window rule (TVC 8: any ceil(fps) consecutive pictures
 * at most maxrate bytes) while keeping room for the pictures after it: each future picture inside a window that
 * contains this one is reserved half the average budget, maxrate / (2 WIN). Without the reserve, large early pictures
 * could leave later ones no room at any QP. */
static long window_allowance(void) {
    long best = LEV->maxpic, m = LEV->maxrate / (2 * WIN);
    for (int k = 0; k < WIN; k++) {                  /* the window ending k pictures after this one */
        long sum = 0;
        for (int j = 1; j <= WIN - 1 - k && j <= NHIST; j++) sum += HIST[NHIST - j];
        long a = LEV->maxrate - sum - (long)k * m;
        if (a < best) best = a;
    }
    return best;
}
/* encode and write one picture; with a level, a picture over the allowance above is re-encoded at a higher QP */
static int NRAISED;
static void encode_picture(FILE *out, RC *r, int ptype, int qp, const double *off, int aq_on, size_t *total) {
    (void)r;
    uint8_t *b; size_t len;
    long allow = LEV ? (RATEWIN ? window_allowance() : LEV->maxpic) : 0;
    int qp0 = qp;
    for (;;) {
        len = encode_picture_buf(&b, ptype, qp, off, aq_on);
        if (!LEV || (long)len <= allow || qp >= 51) break;
        free(b); qp = tvc_clip(qp + 2, 0, 51);
    }
    NRAISED += qp != qp0;
    if (LEV && (long)len > allow)
        fprintf(stderr, "warning: picture %d is over its level allowance (%zu > %ld bytes) even at QP 51\n", NHIST, len, allow);
    HIST[NHIST++] = (long)len;
    put32(out, (uint32_t)len); fwrite(b, 1, len, out); *total += len + 4; free(b);
}

#define TVC_ENC_TOOL_VERSION "0.1.0"

int main(int argc, char **argv) {
    if (argc == 2 && !strcmp(argv[1], "--version")) { puts("tvc_enc " TVC_ENC_TOOL_VERSION); return 0; }
    if (argc < 3) { fprintf(stderr, "usage: tvc_enc in.y4m out.tvc [-q qp] [-g gop] [-b nb] [-i iqp_off] [-o bqp_off] [-t tree] [-a aq] [-n frames] [-r recon.y4m] [-l level [-S rows] [-P prev,lens]]\n"); return 1; }
    /* defaults: look-ahead 2.0 (V-188); quarter-sample motion (V-189); scene-cut I pictures at ratio 0.7 (V-190:
     * rate-neutral, gives seek points at cuts); 2 B pictures (1 is worse, 3 about equal on 60 s segments, V-190);
     * 4x4 transform option on (-2..-8%, best on anime, V-190) */
    Opt o = {30, 96, 2, 3, 2, 1 << 30, 0.0, 2.0, NULL, 1, 0.7, 1, 0};
    const char *prevlens = NULL;   /* -P l1,l2,...: lengths of the pictures just before this input (rate window) */
    for (int i = 3; i + 1 < argc; i += 2) {
        const char *a = argv[i], *v = argv[i + 1];
        if (!strcmp(a, "-q")) o.qp = atoi(v); else if (!strcmp(a, "-g")) o.gop = atoi(v); else if (!strcmp(a, "-b")) o.nb = atoi(v);
        else if (!strcmp(a, "-i")) o.iqoff = atoi(v); else if (!strcmp(a, "-o")) o.boff = atoi(v); else if (!strcmp(a, "-t")) o.tree = atof(v); else if (!strcmp(a, "-p")) o.qpel = atoi(v); else if (!strcmp(a, "-s")) o.cut = atof(v); else if (!strcmp(a, "-x")) o.t4 = atoi(v); else if (!strcmp(a, "-z")) RDOQ = atoi(v);
        else if (!strcmp(a, "-l")) o.level = atoi(v); else if (!strcmp(a, "-S")) SLICE_ROWS = atoi(v); else if (!strcmp(a, "-P")) prevlens = v; else if (!strcmp(a, "-R")) RATEWIN = atoi(v);
        else if (!strcmp(a, "-a")) o.aq = atof(v); else if (!strcmp(a, "-n")) o.nmax = atoi(v); else if (!strcmp(a, "-r")) o.recon = v;
    }
    tvc_init(); tvc_cost_init(); dct_init();
    FILE *in = fopen(argv[1], "rb"), *out = fopen(argv[2], "wb");
    int fn, fd;
    if (!in || !out || !y4m_read_header(in, &frame_w, &frame_h, &fn, &fd)) { fprintf(stderr, "input error\n"); return 1; }
    S.w = (frame_w + 15) & ~15; S.h = (frame_h + 15) & ~15; S.mbw = S.w / 16; S.mbh = S.h / 16;
    S.mb = calloc((size_t)S.mbw * S.mbh, sizeof(MB));
    size_t fsz = (size_t)frame_w * frame_h + 2 * (size_t)((frame_w + 1) / 2) * ((frame_h + 1) / 2);
    uint8_t *buf = malloc(fsz);
    int cap = 64, n = 0; Plane3 *src = malloc(cap * sizeof *src);
    while (n < o.nmax && y4m_read_frame(in, buf, fsz)) {
        if (n == cap) { cap *= 2; src = realloc(src, cap * sizeof *src); }
        src[n] = plane_alloc(S.w, S.h); pad_frame(buf, &src[n]); n++;
    }
    int nm = S.mbw * S.mbh;
    WIN = (fn + fd - 1) / fd;
    HIST = malloc((size_t)(n + WIN + 1) * sizeof *HIST);
    for (const char *q = prevlens; q && *q;) {           /* keep the last WIN-1 given lengths */
        char *e; long l = strtol(q, &e, 10);
        if (e == q) break;
        if (WIN > 1) { if (NHIST == WIN - 1) { memmove(HIST, HIST + 1, (size_t)(NHIST - 1) * sizeof *HIST); NHIST--; } HIST[NHIST++] = l; }
        q = *e == ',' ? e + 1 : e;
    }
    double **off = malloc(n * sizeof *off);
    for (int t = 0; t < n; t++) off[t] = calloc(nm, sizeof **off);
    S.qpel = o.qpel != 0; S.t4ok = o.t4 != 0;
    Look look = look_costs(src, n);
    /* picture types in display order: an I picture at frame 0, at a scene cut (motion-compensated cost above o.cut of
     * the intra cost, at least MIN_KEY frames after the last I) and when the GOP length is reached; anchors every
     * nb+1 frames counted from the last anchor, at every I and at the last frame */
    char *isI = calloc(n, 1), *anchor = calloc(n, 1);
    int lastI = 0, lastA = 0, ncut = 0;
    for (int t = 0; t < n; t++) {
        int cut = t > 0 && o.cut > 0 && t - lastI >= MIN_KEY && look_ratio(&look, t) > o.cut;
        isI[t] = t == 0 || cut || t - lastI >= o.gop;
        if (isI[t]) { lastI = t; ncut += cut; }
        anchor[t] = isI[t] || t - lastA >= o.nb + 1 || t == n - 1;
        if (anchor[t]) lastA = t;
    }
    if (o.tree > 0) tree_offsets(&look, isI, n, o.tree, off);
    /* Look-ahead data is no longer read once cuts and propagation are known. */
    free(look.intra); free(look.inter); free(look.mvx); free(look.mvy);
    if (o.aq > 0) { double *a = malloc(nm * sizeof *a); for (int t = 0; t < n; t++) { aq_offsets(&src[t], o.aq, a); for (int i = 0; i < nm; i++) off[t][i] += a[i]; } free(a); }
    int aq_on = o.tree > 0 || o.aq > 0;
    Plane3 *rec = o.recon ? calloc(n, sizeof *rec) : NULL;
    Plane3 bbuf = {0};
    if (!o.recon) bbuf = plane_alloc(S.w, S.h);
    Plane3 abuf[3] = {plane_alloc(S.w, S.h), plane_alloc(S.w, S.h), plane_alloc(S.w, S.h)};
    Plane3 *prev = NULL, *last = NULL; int last_t = -1;
    if (o.level) {                                   /* draft 5: "TVC1" header with the level; level checks up front */
        if (o.level < 1 || o.level > 3) { fprintf(stderr, "level must be 1..3\n"); return 1; }
        LEV = &TVC_LEVEL[o.level];
        if (S.w > LEV->maxw || S.h > LEV->maxh || S.mbw * S.mbh > LEV->maxmbs || fn > 30L * fd || (long)S.mbw * S.mbh * fn > (long)LEV->maxmbps * fd) {
            fprintf(stderr, "input %dx%d at %d/%d fps does not fit level %d\n", frame_w, frame_h, fn, fd, o.level); return 1; }
        if (LEV->minslices > 1 && SLICE_ROWS == 0) SLICE_ROWS = (S.mbh + LEV->minslices - 1) / LEV->minslices;
        if (SLICE_ROWS > 0 && (S.mbh + SLICE_ROWS - 1) / SLICE_ROWS < LEV->minslices) { fprintf(stderr, "too few slices for level %d\n", o.level); return 1; }
        fwrite("TVC1", 1, 4, out);
    } else fwrite("TVC0", 1, 4, out);
    put16(out, frame_w); put16(out, frame_h); put16(out, fn); put16(out, fd); put32(out, n);
    if (o.level) { fputc(o.level, out); fputc(0, out); fputc(0, out); fputc(0, out); }
    if (SLICE_ROWS > 255) SLICE_ROWS = 255;
    size_t total = 0; RC r;
    for (int t = 0; t < n; t++) {
        if (!anchor[t]) continue;
        /* anchor t */
        int ptype = isI[t] ? PIC_I : PIC_P;
        Plane3 *cur = &abuf[0]; while (cur == prev || cur == last) cur++;
        SRC = src[t]; CUR = *cur; REFS[0] = last; REFS[1] = last;
        double *zero = calloc(nm, sizeof *zero);
        encode_picture(out, &r, ptype, tvc_clip(ptype == PIC_I ? o.qp - o.iqoff : o.qp, 0, 51), off[t], aq_on, &total);
        free(zero);
        prev = last; last = cur;
        if (o.recon) {
            rec[t] = plane_alloc(S.w, S.h);
            for (int k = 0; k < 3; k++) memcpy(rec[t].p[k], cur->p[k], k ? S.w * S.h / 4 : S.w * S.h);
        }
        /* the B pictures between the two anchors */
        for (int b = last_t + 1; b < t && last_t >= 0; b++) {
            double *z = calloc(nm, sizeof *z);           /* B pictures are not referenced: no propagation offsets */
            if (o.aq > 0) aq_offsets(&src[b], o.aq, z);
            SRC = src[b];
            if (o.recon) { rec[b] = plane_alloc(S.w, S.h); CUR = rec[b]; }
            else CUR = bbuf;
            REFS[0] = prev; REFS[1] = last;
            encode_picture(out, &r, PIC_B, tvc_clip(o.qp + o.boff, 0, 51), z, aq_on, &total);
            free(z);
        }
        last_t = t;
    }
    fclose(out);
    if (o.recon) {
        FILE *rf = fopen(o.recon, "wb");
        fprintf(rf, "YUV4MPEG2 W%d H%d F%d:%d Ip A1:1 C420jpeg\n", frame_w, frame_h, fn, fd);
        for (int t = 0; t < n; t++) {
            fprintf(rf, "FRAME\n");
            for (int y = 0; y < frame_h; y++) fwrite(rec[t].p[0] + y * S.w, 1, frame_w, rf);
            for (int k = 1; k < 3; k++) for (int y = 0; y < (frame_h + 1) / 2; y++) fwrite(rec[t].p[k] + y * (S.w / 2), 1, (frame_w + 1) / 2, rf);
        }
        fclose(rf);
    }
    else plane_free(&bbuf);
    fprintf(stderr, "%d frames, %zu bytes, %.1f bytes/frame, %d scene-cut I pictures, %d pictures re-encoded for the level\n", n, total, n ? (double)total / n : 0, ncut, NRAISED);
    return 0;
}
