/* Encoder-only exact greedy RDOQ, reusing the counted high-scan prefix.
 * Requires tvc.h. Symbols follow its frozen coefficient syntax, in the same
 * addition order. A last-position change uses a fresh full-block count. */
#ifndef TVC_ENC_RDOQ_H
#define TVC_ENC_RDOQ_H
#ifdef TVC_VERIFY_RDOQ
#include <assert.h>
#endif
static void enc_rdoq_copy(Ctx *dst, const Ctx *src, int cc) {
    memcpy(dst->last[cc], src->last[cc], sizeof dst->last[cc]);
    memcpy(dst->sig[cc], src->sig[cc], sizeof dst->sig[cc]);
    memcpy(dst->gt1[cc], src->gt1[cc], sizeof dst->gt1[cc]);
    memcpy(dst->lrem[cc], src->lrem[cc], sizeof dst->lrem[cc]);
}
/* Suffix replay never codes the last-position header. */
static void enc_rdoq_copy_suffix(Ctx *dst, const Ctx *src, int cc) {
    memcpy(dst->sig[cc], src->sig[cc], sizeof dst->sig[cc]);
    memcpy(dst->gt1[cc], src->gt1[cc], sizeof dst->gt1[cc]);
    memcpy(dst->lrem[cc], src->lrem[cc], sizeof dst->lrem[cc]);
}
static double enc_rdoq_full(const Ctx *initial, int cc, int16_t *lev, int n) {
    Ctx c; enc_rdoq_copy(&c, initial, cc);
    RC r; rc_cnt_init(&r); code_block_n(&r, &c, cc, lev, n);
    return r.bits;
}
/* These calls only operate on an RC_CNT stream. Keep additions and probability
 * updates in the exact same order as the general-purpose range counter. */
static inline int enc_rdoq_bit(RC *r, uint16_t *p, int bit) {
    r->bits += TVC_COST[(bit ? PROB_ONE - *p : *p) >> 3];
    if (bit) *p -= *p >> PROB_RATE; else *p += (PROB_ONE - *p) >> PROB_RATE;
    return bit;
}
static void enc_rdoq_eg(RC *r, uint16_t *ctx, int v) {
    int i = 0, k = 0, vv = v;
    while (vv >= (1 << k)) { enc_rdoq_bit(r, &ctx[tvc_min(i, 5)], 1); vv -= 1 << k; k++; i++; }
    enc_rdoq_bit(r, &ctx[tvc_min(i, 5)], 0);
    for (int b = k - 1; b >= 0; b--) r->bits += 1.0;
}
static void enc_rdoq_header(RC *r, Ctx *c, const Ctx *initial, int cc, int last, int n) {
    rc_cnt_init(r); enc_rdoq_copy(c, initial, cc);
    int node = 1;
    for (int k = (n == 64 ? 6 : 4) - 1; k >= 0; k--) {
        int bit = enc_rdoq_bit(r, &c->last[cc][node], (last >> k) & 1);
        node = 2 * node + bit;
    }
}
static void enc_rdoq_symbol(RC *r, Ctx *c, int cc, int n, int last, int i,
                            const int16_t *lev, int *ngt1) {
    if (i < last) {
        int neighbors = (lev[i+1] != 0) + (i+2 <= last && lev[i+2] != 0);
        int pc = n == 64 ? TVC_PCLASS[i] : TVC_PCLASS4[i];
        if (!enc_rdoq_bit(r, &c->sig[cc][pc][neighbors], lev[i] != 0)) return;
    }
    int a = abs(lev[i]);
    int gc = n == 64 ? (i < 6 ? 0 : i < 20 ? 1 : 2) : (i < 3 ? 0 : i < 8 ? 1 : 2);
    if (enc_rdoq_bit(r, &c->gt1[cc][gc][tvc_min(*ngt1, 2)], a > 1)) {
        enc_rdoq_eg(r, c->lrem[cc], a - 2); (*ngt1)++;
    }
    r->bits += 1.0;
}
static int enc_rdoq(const double *cs, int16_t *lev, int n, int qp, int cc,
                    double lam, const Ctx *initial) {
    int nz = 0, last = 0;
    for (int i = 0; i < n; i++) if (lev[i]) { nz++; last = i; }
    if (!nz) return 0; /* Production callers pass nonzero quantized blocks. */
    /* A singleton needs at most one trial; a sole magnitude-one coefficient
     * cannot be removed by this greedy algorithm. Avoid prefix setup here. */
    if (nz == 1) {
        int a = abs(lev[last]);
        if (a > 1) {
            double cur = enc_rdoq_full(initial, cc, lev, n);
            double c = fabs(cs[last]), d_old = c - dequant(a, qp), d_new = c - dequant(a-1, qp);
            int16_t keep = lev[last];
            lev[last] = (int16_t)(keep < 0 ? -(a-1) : a-1);
            double nb = enc_rdoq_full(initial, cc, lev, n);
            if (!(d_new*d_new - d_old*d_old + lam*(nb-cur) < 0)) lev[last] = keep;
        }
        return 1;
    }
    double cur = enc_rdoq_full(initial, cc, lev, n);
    RC prefix; Ctx models; int ngt1 = 0;
    enc_rdoq_header(&prefix, &models, initial, cc, last, n);
    for (int i = n - 1; i >= 0; i--) {
        if (i > last) continue;
        int a = abs(lev[i]);
        if (a && !(a == 1 && nz == 1)) {
            int16_t keep = lev[i];
            double c = fabs(cs[i]), d_old = c - dequant(a, qp), d_new = c - (a > 1 ? dequant(a - 1, qp) : 0);
            lev[i] = (int16_t)(keep < 0 ? -(a - 1) : a - 1);
            double nb;
            int drops_last = i == last && a == 1;
            if (drops_last) nb = enc_rdoq_full(initial, cc, lev, n);
            else {
                RC trial; rc_cnt_init(&trial); trial.bits = prefix.bits;
                Ctx tc; enc_rdoq_copy_suffix(&tc, &models, cc); int nt = ngt1;
                for (int k = i; k >= 0; k--) enc_rdoq_symbol(&trial, &tc, cc, n, last, k, lev, &nt);
                nb = trial.bits;
            }
#ifdef TVC_VERIFY_RDOQ
            assert(nb == enc_rdoq_full(initial, cc, lev, n));
#endif
            if (d_new*d_new - d_old*d_old + lam*(nb-cur) < 0) {
                cur = nb; if (a == 1) nz--;
                if (drops_last) {
                    do { last--; } while (!lev[last]);
                    ngt1 = 0; enc_rdoq_header(&prefix, &models, initial, cc, last, n);
                }
            } else lev[i] = keep;
        }
        if (i <= last) enc_rdoq_symbol(&prefix, &models, cc, n, last, i, lev, &ngt1);
    }
#ifdef TVC_VERIFY_RDOQ
    assert(prefix.bits == cur);
#endif
    return nz;
}
#endif
