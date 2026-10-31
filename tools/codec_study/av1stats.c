/* av1stats.c -- what an AV1 decoder has to do for a stream: symbol counts, bits per syntax element, per-block tool usage.
 *
 * Study tool, links a libaom decoder built with CONFIG_ACCOUNTING=1 and CONFIG_INSPECTION=1 (work/ref/aom-build, v3.15.2).
 * Not part of the product and nothing here is copied from libaom: it only calls its public decode API and its inspection
 * hook, and reads what they report.
 *
 *   cc -O2 -Iwork/ref/aom-build -Iwork/ref/aom-src -o build/av1stats tools/codec_study/av1stats.c work/ref/aom-build/libaom.a -lm
 *   build/av1stats stream.ivf > stats.txt
 *
 * Output, one record per line:
 *   F <index> <type> <show> <qindex> <symbols> <bits>         one per decoded frame (symbols = binary + multi-symbol reads)
 *   S <name> <bits> <count>                                    totals per syntax element, bits in 1/8 bit units
 *   H <name> <bin> <area>                                      histograms, area in 4x4-pixel units over all frames
 *   MV <axis> <bin> <area>                                     |motion vector| in whole pixels, 0..127 (127 = overflow)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include "aom/aom_decoder.h"
#include "aom/aomdx.h"
#include "av1/av1_iface_common.h"
#include "av1/decoder/inspection.h"

#define MAXSYM 300
static char symname[MAXSYM][64];
static double symbits[MAXSYM], symcnt[MAXSYM];
static int nsym_names;
static double hbsize[32], hmode[64], huv[16], hmm[4], hcomp[8], htx[20], hfilt[16], href[16], hskip[2], hpal[2], hibc[2], hbi[2];
static double mvy[128], mvx[128];
static int frames_seen;
static insp_frame_data fd;

static void cb(void *pbi, void *ctx)
{
    (void)ctx;
    ifd_inspect(&fd, pbi, 0);
    if (fd.show_existing_frame) return;
    const Accounting *acc = fd.accounting;
    double bits = 0;
    int syms = acc->syms.num_binary_syms + acc->syms.num_multi_syms;
    for (int i = 0; i < acc->syms.num_syms; i++) {
        const AccountingSymbol *s = &acc->syms.syms[i];
        if (s->id >= (uint32_t)acc->syms.dictionary.num_strs) continue;
        const char *nm = acc->syms.dictionary.strs[s->id];
        int k;
        for (k = 0; k < nsym_names; k++) if (!strcmp(symname[k], nm)) break;
        if (k == nsym_names && nsym_names < MAXSYM) { snprintf(symname[k], 64, "%s", nm); nsym_names++; }
        if (k < MAXSYM) { symbits[k] += s->bits; symcnt[k] += s->samples; }
        bits += s->bits;
    }
    printf("F %d %d %d %d %d %.0f\n", frames_seen, fd.frame_type, fd.show_frame, fd.base_qindex, syms, bits / 8.0);
    {   /* per frame: area (4x4 units) of NON-skipped blocks by transform size, for transform engine sizing */
        double ns[20] = { 0 };
        for (int j = 0; j < fd.mi_rows; j++)
            for (int i = 0; i < fd.mi_cols; i++) {
                const insp_mi_data *m = &fd.mi_grid[j * fd.mi_cols + i];
                if (!m->skip && m->tx_size >= 0 && m->tx_size < 20) ns[m->tx_size]++;
            }
        for (int t = 0; t < 20; t++) if (ns[t] > 0) printf("T %d %d %.0f\n", frames_seen, t, ns[t]);
    }
    frames_seen++;
    for (int j = 0; j < fd.mi_rows; j++)
        for (int i = 0; i < fd.mi_cols; i++) {
            const insp_mi_data *m = &fd.mi_grid[j * fd.mi_cols + i];
            if (m->bsize >= 0 && m->bsize < 32) hbsize[m->bsize]++;
            hskip[m->skip != 0]++;
            if (m->tx_size >= 0 && m->tx_size < 20) htx[m->tx_size]++;
            int inter = m->ref_frame[0] > 0;
            if (m->ref_frame[0] >= 0 && m->ref_frame[0] < 16) href[m->ref_frame[0]]++;
            if (!inter) {
                if (m->mode >= 0 && m->mode < 64) hmode[m->mode]++;
                if (m->uv_mode >= 0 && m->uv_mode < 16) huv[m->uv_mode]++;
                hpal[m->palette > 0]++;
                hibc[m->intrabc != 0]++;
            } else {
                if (m->motion_mode >= 0 && m->motion_mode < 4) hmm[m->motion_mode]++;
                if (m->compound_type >= 0 && m->compound_type < 8) hcomp[m->compound_type]++;
                if (m->dual_filter_type >= 0 && m->dual_filter_type < 16) hfilt[m->dual_filter_type]++;
                hbi[m->ref_frame[1] > 0]++;
                int ay = abs(m->mv[0].row) / 8, ax = abs(m->mv[0].col) / 8;
                mvy[ay > 127 ? 127 : ay]++;
                mvx[ax > 127 ? 127 : ax]++;
            }
        }
}

static void dump(const char *n, double *h, int len)
{
    for (int i = 0; i < len; i++) if (h[i] > 0) printf("H %s %d %.0f\n", n, i, h[i]);
}

int main(int argc, char **argv)
{
    if (argc < 2) return 2;
    FILE *f = fopen(argv[1], "rb");
    if (!f) return 1;
    unsigned char hdr[32];
    if (fread(hdr, 1, 32, f) != 32 || memcmp(hdr, "DKIF", 4)) return 1;
    aom_codec_ctx_t dec;
    aom_codec_dec_cfg_t cfg = { 1, 0, 0, 0 };
    /* the inspection decoder decodes one frame per call (hidden ones too) and calls the hook after each */
    if (aom_codec_dec_init(&dec, &aom_codec_av1_inspect_algo, &cfg, 0)) return 1;
    aom_inspect_init ii = { cb, NULL };
    if (aom_codec_control(&dec, AV1_SET_INSPECTION_CALLBACK, &ii)) { fprintf(stderr, "no inspection support\n"); return 1; }
    unsigned char fh[12], *buf = malloc(1 << 22);
    int ivf_frames = 0;
    while (fread(fh, 1, 12, f) == 12) {
        ivf_frames++;
        unsigned sz = fh[0] | fh[1] << 8 | fh[2] << 16 | (unsigned)fh[3] << 24;
        if (sz > (1u << 22) || fread(buf, 1, sz, f) != sz) break;
        const unsigned char *p = buf, *end = buf + sz;
        while (p < end) {                                  /* a temporal unit can hold several frames */
            Av1DecodeReturn adr;
            memset(&adr, 0, sizeof adr);
            if (aom_codec_decode(&dec, p, (size_t)(end - p), &adr)) {
                fprintf(stderr, "decode error: %s\n", aom_codec_error_detail(&dec)); return 1;
            }
            if (adr.buf <= p) break;
            p = adr.buf;
        }
        aom_codec_iter_t it = NULL;
        while (aom_codec_get_frame(&dec, &it)) {}
    }
    fprintf(stderr, "ivf frames %d, inspected %d\n", ivf_frames, frames_seen);
    for (int k = 0; k < nsym_names; k++) printf("S %s %.0f %.0f\n", symname[k], symbits[k], symcnt[k]);
    dump("bsize", hbsize, 32); dump("mode", hmode, 64); dump("uvmode", huv, 16); dump("motion_mode", hmm, 4);
    dump("compound_type", hcomp, 8); dump("txsize", htx, 20); dump("filter", hfilt, 16); dump("refframe", href, 16);
    dump("skip", hskip, 2); dump("palette", hpal, 2); dump("intrabc", hibc, 2); dump("bi_pred", hbi, 2);
    for (int i = 0; i < 128; i++) if (mvy[i] > 0) printf("MV y %d %.0f\n", i, mvy[i]);
    for (int i = 0; i < 128; i++) if (mvx[i] > 0) printf("MV x %d %.0f\n", i, mvx[i]);
    return 0;
}
