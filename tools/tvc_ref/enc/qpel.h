/* Encoder-only 16x16 luma search predictor. Requires tvc.h first.
 * Return 0 without touching output if the filter footprint crosses an edge.
 * Uses TVC's frozen coefficients and exact rounding, not AV1 filters. */
#ifndef TVC_ENC_QPEL_H
#define TVC_ENC_QPEL_H
static int enc_pred_q16(const uint8_t *ref, int w, int h, int bx, int by,
                       int mvx, int mvy, uint8_t *out, int stride) {
    int rx = bx + (mvx >> 2), ry = by + (mvy >> 2), fx = mvx & 3, fy = mvy & 3;
    if (rx < (fx ? 1 : 0) || ry < (fy ? 1 : 0) ||
        rx + 16 + (fx ? 2 : 0) > w || ry + 16 + (fy ? 2 : 0) > h) return 0;
    const int *a = TVC_QF[fx], *b = TVC_QF[fy];
    const uint8_t *base = ref + ry * w + rx;
    if (!fx && !fy) {
        for (int y = 0; y < 16; y++) memcpy(out + y * stride, base + y * w, 16);
    } else if (!fy) {
        for (int y = 0; y < 16; y++) for (int x = 0; x < 16; x++) {
            const uint8_t *p = base + y * w + x;
            int v = a[0]*p[-1] + a[1]*p[0] + a[2]*p[1] + a[3]*p[2];
            out[y * stride + x] = (uint8_t)tvc_clip((v + 32) >> 6, 0, 255);
        }
    } else if (!fx) {
        for (int y = 0; y < 16; y++) for (int x = 0; x < 16; x++) {
            const uint8_t *p = base + y * w + x;
            int v = b[0]*p[-w] + b[1]*p[0] + b[2]*p[w] + b[3]*p[2*w];
            out[y * stride + x] = (uint8_t)tvc_clip((v + 32) >> 6, 0, 255);
        }
    } else {
        int tmp[19][16];
        for (int y = 0; y < 19; y++) for (int x = 0; x < 16; x++) {
            const uint8_t *p = base + (y - 1) * w + x;
            tmp[y][x] = a[0]*p[-1] + a[1]*p[0] + a[2]*p[1] + a[3]*p[2];
        }
        /* Horizontal intermediates stay unrounded and unclipped. */
        for (int y = 0; y < 16; y++) for (int x = 0; x < 16; x++) {
            int v = b[0]*tmp[y][x] + b[1]*tmp[y+1][x] + b[2]*tmp[y+2][x] + b[3]*tmp[y+3][x];
            out[y * stride + x] = (uint8_t)tvc_clip((v + 2048) >> 12, 0, 255);
        }
    }
    return 1;
}
#endif
