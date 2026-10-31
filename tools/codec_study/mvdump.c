/* Motion-vector workload dump for an H.264 / MPEG-4 stream (libavcodec export_mvs).
 *
 * Output (stdout, tab separated):
 *   F  <display idx> <type> <area past> <area future> <frame area>
 *   DY <bin px> <area>     histogram of |vertical vector| in whole pixels (last bin = overflow)
 *   DX <bin px> <area>     the same for horizontal
 *   BS <w>x<h> <count>     block size mix
 * Areas are in pixels (sum of w*h of the exported blocks); intra blocks export nothing.
 */
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/motion_vector.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

#define NB 130
static double dy_hist[NB], dx_hist[NB];
static long bs[17][17];

static void handle(AVFrame *f, long idx)
{
    double past = 0, fut = 0;
    AVFrameSideData *sd = av_frame_get_side_data(f, AV_FRAME_DATA_MOTION_VECTORS);
    if (sd) {
        const AVMotionVector *mv = (const AVMotionVector *)sd->data;
        int n = sd->size / sizeof(*mv);
        for (int i = 0; i < n; i++) {
            double a = (double)mv[i].w * mv[i].h;
            double sc = mv[i].motion_scale ? mv[i].motion_scale : 1;
            int ady = (int)floor(fabs(mv[i].motion_y / sc) + 0.5);
            int adx = (int)floor(fabs(mv[i].motion_x / sc) + 0.5);
            dy_hist[ady >= NB - 1 ? NB - 1 : ady] += a;
            dx_hist[adx >= NB - 1 ? NB - 1 : adx] += a;
            if (mv[i].w <= 16 && mv[i].h <= 16)
                bs[mv[i].w][mv[i].h]++;
            if (mv[i].source < 0) past += a; else fut += a;
        }
    }
    printf("F\t%ld\t%c\t%.0f\t%.0f\t%.0f\n", idx, av_get_picture_type_char(f->pict_type), past, fut,
           (double)f->width * f->height);
}

int main(int argc, char **argv)
{
    if (argc < 2) return 2;
    AVFormatContext *fc = NULL;
    if (avformat_open_input(&fc, argv[1], NULL, NULL) < 0) return 1;
    avformat_find_stream_info(fc, NULL);
    int vi = av_find_best_stream(fc, AVMEDIA_TYPE_VIDEO, -1, -1, NULL, 0);
    if (vi < 0) return 1;
    const AVCodec *dec = avcodec_find_decoder(fc->streams[vi]->codecpar->codec_id);
    AVCodecContext *cc = avcodec_alloc_context3(dec);
    avcodec_parameters_to_context(cc, fc->streams[vi]->codecpar);
    AVDictionary *o = NULL;
    av_dict_set(&o, "flags2", "+export_mvs", 0);
    if (avcodec_open2(cc, dec, &o) < 0) return 1;
    AVPacket *p = av_packet_alloc();
    AVFrame *f = av_frame_alloc();
    long idx = 0;
    while (av_read_frame(fc, p) >= 0) {
        if (p->stream_index == vi && avcodec_send_packet(cc, p) >= 0)
            while (avcodec_receive_frame(cc, f) >= 0) { handle(f, idx++); av_frame_unref(f); }
        av_packet_unref(p);
    }
    avcodec_send_packet(cc, NULL);
    while (avcodec_receive_frame(cc, f) >= 0) { handle(f, idx++); av_frame_unref(f); }
    for (int i = 0; i < NB; i++) printf("DY\t%d\t%.0f\n", i, dy_hist[i]);
    for (int i = 0; i < NB; i++) printf("DX\t%d\t%.0f\n", i, dx_hist[i]);
    for (int w = 1; w <= 16; w++) for (int h = 1; h <= 16; h++) if (bs[w][h]) printf("BS\t%dx%d\t%ld\n", w, h, bs[w][h]);
    return 0;
}
