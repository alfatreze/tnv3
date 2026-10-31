/* TVC v0 reference decoder: tvc_dec IN.tvc OUT.y4m
 * Anchors (I, P) and non-reference B pictures; output in display order: a B picture is output when decoded,
 * an anchor is held until the next anchor is decoded (or the stream ends).  Draft 5: slices (independent
 * row bands) and, for "TVC1" files, level limits; a level violation is reported and exits with status 3. */
#include "tvc.h"
#ifdef TVC_DBK_HOOK     /* tools/tvc_ref/tvc_dbk_trace.c: the picture before and after deblocking */
void tvc_dbk_hook(int end, Pic *s, Plane3 *cur);
#endif

static void write_frame(FILE *o, const Plane3 *p, int w, int h) {
    fprintf(o, "FRAME\n");
    for (int y = 0; y < h; y++) fwrite(p->p[0] + y * p->w, 1, w, o);
    for (int k = 1; k < 3; k++) for (int y = 0; y < (h + 1) / 2; y++) fwrite(p->p[k] + y * (p->w / 2), 1, (w + 1) / 2, o);
}
static int violation(const char *what, long v, long lim) {
    fprintf(stderr, "level violation: %s = %ld (limit %ld)\n", what, v, lim);
    exit(3);
}

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: tvc_dec in.tvc out.y4m\n"); return 1; }
    tvc_init();
    FILE *f = fopen(argv[1], "rb"), *o = fopen(argv[2], "wb");
    if (!f || !o) { perror("open"); return 1; }
    char magic[4];
    if (fread(magic, 1, 4, f) != 4 || (memcmp(magic, "TVC0", 4) && memcmp(magic, "TVC1", 4))) { fprintf(stderr, "not a TVC file\n"); return 1; }
    int w = get16(f), h = get16(f), fn = get16(f), fd = get16(f);
    uint32_t n = get32(f);
    int level = 0;
    if (magic[3] == '1') { level = fgetc(f); fgetc(f); fgetc(f); fgetc(f); }
    if (level < 0 || level > 3 || fn <= 0 || fd <= 0) { fprintf(stderr, "bad sequence header\n"); return 1; }
    Pic s = {0};
    s.w = (w + 15) & ~15; s.h = (h + 15) & ~15; s.mbw = s.w / 16; s.mbh = s.h / 16;
    const Level *L = level ? &TVC_LEVEL[level] : NULL;
    int win = (fn + fd - 1) / fd;                 /* pictures in one second, rounded up */
    long *lens = calloc(n ? n : 1, sizeof *lens);
    if (L) {
        if (s.w > L->maxw) violation("coded width", s.w, L->maxw);
        if (s.h > L->maxh) violation("coded height", s.h, L->maxh);
        if (s.mbw * s.mbh > L->maxmbs) violation("macroblocks per picture", s.mbw * s.mbh, L->maxmbs);
        if (fn > 30L * fd) violation("frame rate x den", fn, 30L * fd);
        if ((long)s.mbw * s.mbh * fn > (long)L->maxmbps * fd) violation("macroblocks per second", (long)s.mbw * s.mbh * fn / fd, L->maxmbps);
    }
    s.mb = calloc((size_t)s.mbw * s.mbh, sizeof(MB));
    Plane3 buf[3] = {plane_alloc(s.w, s.h), plane_alloc(s.w, s.h), plane_alloc(s.w, s.h)}, bpic = plane_alloc(s.w, s.h);
    Plane3 *prev = NULL, *last = NULL;       /* previous and latest anchors */
    int held = 0;                            /* latest anchor not yet output */
    fprintf(o, "YUV4MPEG2 W%d H%d F%d:%d Ip A1:1 C420jpeg\n", w, h, fn, fd);
    for (uint32_t i = 0; i < n; i++) {
        uint32_t len = get32(f);
        uint8_t *data = malloc(len ? len : 1);
        if (len < 3 || fread(data, 1, len, f) != len) { fprintf(stderr, "truncated picture %u\n", i); return 1; }
        s.ptype = data[0]; s.qp = data[1]; s.aq = data[2] & 1; s.qpel = (data[2] >> 1) & 1; s.t4ok = (data[2] >> 2) & 1;
        int sliced = (data[2] >> 3) & 1;
        if (s.ptype > PIC_B || s.qp > 51 || (data[2] >> 4) || (s.ptype == PIC_P && !last) || (s.ptype == PIC_B && !prev)) { fprintf(stderr, "bad picture header %u\n", i); return 1; }
        /* slice layout: rows per slice, then the byte length of every slice except the last */
        int rows = s.mbh, pos = 3;
        if (sliced) { if (len < 4 || data[3] == 0) { fprintf(stderr, "bad slice header %u\n", i); return 1; } rows = data[3]; pos = 4; }
        int ns = (s.mbh + rows - 1) / rows;
        uint32_t *slen = calloc(ns, sizeof *slen);
        for (int k = 0; k < ns - 1; k++) {
            if (pos + 4 > (int)len) { fprintf(stderr, "bad slice table %u\n", i); return 1; }
            slen[k] = data[pos] | data[pos + 1] << 8 | data[pos + 2] << 16 | (uint32_t)data[pos + 3] << 24; pos += 4;
        }
        uint32_t used = 0; for (int k = 0; k < ns - 1; k++) used += slen[k];
        if (pos + used > len) { fprintf(stderr, "bad slice table %u\n", i); return 1; }
        slen[ns - 1] = len - pos - used;
        if (L) {
            if ((long)len > L->maxpic) violation("bytes per picture", len, L->maxpic);
            if (ns < L->minslices) violation("slices per picture", ns, L->minslices);
            lens[i] = len; long sum = 0;
            for (int k = 0; k < win && k <= (int)i; k++) sum += lens[i - k];
            if (sum > L->maxrate) violation("bytes in one second of pictures", sum, L->maxrate);
        }
        Plane3 *cur;
        const Plane3 *ref[2];
        if (s.ptype == PIC_B) { cur = &bpic; ref[0] = prev; ref[1] = last; }
        else {
            cur = &buf[0]; while (cur == prev || cur == last) cur++;
            ref[0] = last; ref[1] = last;
        }
        static int16_t lev[6][64];
        for (int k = 0; k < ns; k++) {
            s.slice_y0 = k * rows;
            s.prev_qp = s.qp; s.prev_dq = 0;
            ctx_init(&s.ctx);
            RC r; rc_dec_init(&r, data + pos, slen[k]);
#ifdef TVC_RC_HOOK
            tvc_rc_hook_slice(s.ptype, s.aq, s.t4ok, s.qp, s.mbw, s.slice_y0, s.mbh < s.slice_y0 + rows ? s.mbh : s.slice_y0 + rows);
#endif
            for (int y = s.slice_y0; y < s.mbh && y < s.slice_y0 + rows; y++) for (int x = 0; x < s.mbw; x++) {
                int pv[2][2] = {{0, 0}, {0, 0}};
                if (s.ptype != PIC_I) { mv_pred(&s, x, y, 0, &pv[0][0], &pv[0][1]); mv_pred(&s, x, y, 1, &pv[1][0], &pv[1][1]); }
                MB *m = &s.mb[y * s.mbw + x];
                tvc_code_mb(&s, &r, &s.ctx, x, y, m, lev, pv);
#ifdef TVC_RC_HOOK
                tvc_rc_hook_mb(x, y, m, lev);
#endif
                if (L) {
                    int U = s.qpel ? 4 : 2;
                    for (int l = 0; l < 2; l++) if ((m->uses >> l) & 1) {
                        if (abs(m->mv[l][0]) > L->mvx * U) violation("vector x (picture units)", abs(m->mv[l][0]), L->mvx * U);
                        if (abs(m->mv[l][1]) > L->mvy * U) violation("vector y (picture units)", abs(m->mv[l][1]), L->mvy * U);
                    }
                }
                tvc_recon_mb(&s, cur, ref, x, y, m, lev);
            }
            pos += slen[k];
        }
        free(slen);
#ifdef TVC_DBK_HOOK
        tvc_dbk_hook(0, &s, cur);
#endif
        tvc_deblock(&s, cur);
#ifdef TVC_DBK_HOOK
        tvc_dbk_hook(1, &s, cur);
#endif
        if (s.ptype == PIC_B) write_frame(o, cur, w, h);
        else {
            if (held) write_frame(o, last, w, h);
            prev = last; last = cur; held = 1;
        }
        free(data);
    }
    if (held) write_frame(o, last, w, h);
    fclose(o); fclose(f);
    return 0;
}
