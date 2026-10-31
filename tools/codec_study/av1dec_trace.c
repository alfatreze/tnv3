/* av1dec_trace.c -- decode an IVF AV1 file with the trace-patched libaom (work/ref/aom-build-trace) through its public API only.
 * The patched decoder writes its traces itself (AV1_BLK_TRACE, AV1_INTRA_TRACE, AV1_INTER_TRACE, ... environment variables);
 * this driver additionally writes every SHOWN frame as raw 8-bit planar Y, U, V (stored size) to the file named by AV1_FRAME_DUMP.
 *
 *   cc -O2 -Iwork/ref/aom-build-trace -Iwork/ref/aom-src -o build/av1trace2 tools/codec_study/av1dec_trace.c work/ref/aom-build-trace/libaom.a -lm
 *   AV1_INTER_TRACE=inter.txt AV1_FRAME_DUMP=frames.yuv build/av1trace2 stream.ivf
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "aom/aom_decoder.h"
#include "aom/aomdx.h"

int main(int argc, char **argv) {
  if (argc < 2) return 2;
  FILE *f = fopen(argv[1], "rb");
  if (!f) return 2;
  unsigned char hdr[32];
  if (fread(hdr, 1, 32, f) != 32 || memcmp(hdr, "DKIF", 4)) return 2;
  aom_codec_ctx_t dec;
  if (aom_codec_dec_init(&dec, aom_codec_av1_dx(), NULL, 0)) return 3;
  const char *dump = getenv("AV1_FRAME_DUMP");
  FILE *fy = dump ? fopen(dump, "wb") : NULL;
  unsigned char fh[12];
  int n = 0;
  while (fread(fh, 1, 12, f) == 12) {
    unsigned sz = fh[0] | (fh[1] << 8) | (fh[2] << 16) | ((unsigned)fh[3] << 24);
    unsigned char *buf = malloc(sz);
    if (fread(buf, 1, sz, f) != sz) { free(buf); break; }
    if (aom_codec_decode(&dec, buf, sz, NULL)) { fprintf(stderr, "decode error at frame %d\n", n); free(buf); return 4; }
    free(buf);
    aom_codec_iter_t it = NULL;
    aom_image_t *img;
    while ((img = aom_codec_get_frame(&dec, &it)) != NULL) {
      if (fy) {
        for (int p = 0; p < 3; p++) {
          int w = aom_img_plane_width(img, p), h = aom_img_plane_height(img, p);
          for (int y = 0; y < h; y++) fwrite(img->planes[p] + (size_t)y * img->stride[p], 1, w, fy);
        }
      }
      n++;
    }
  }
  if (fy) fclose(fy);
  aom_codec_destroy(&dec);
  fprintf(stderr, "%d shown frames\n", n);
  return 0;
}
