/* TNV3 container reference (docs/TNV3_CONTAINER_V0.md, draft 1.3): mux, demux, check, info.
 *   tnv3 mux VIDEO.tvc [-a AUDIO.tac] [-s SUBTITLE_AREA.bin] [-c] [-L level] [-r num/den] [-T] -o OUT.tnv3
 *     -c chunk CRCs; -L level for TVC0 input; -r frame rate override; -T trim the audio to the video's duration
 *     (drops trailing TAC frames and lowers sample_count: the kept output is unchanged, TAC section 2).
 *     Test only: -x skip the self-check, -A n add n to every non-last chunk's audio frame target (negative vectors).
 *   tnv3 demux IN.tnv3 OUT.tvc [OUT.tac [OUT_SUBTITLE_AREA.bin]]
 *   tnv3 check IN.tnv3      (exit 0 valid, 3 invalid, 1 usage / io)
 *   tnv3 info IN.tnv3
 * The muxer validates its own output with the checker before writing it. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { uint8_t *p; size_t n; } Buf;

static uint32_t crc_tab[256];
static void crc_init(void) {
    for (uint32_t i = 0; i < 256; i++) {
        uint32_t c = i;
        for (int k = 0; k < 8; k++) c = c & 1 ? 0xEDB88320u ^ (c >> 1) : c >> 1;
        crc_tab[i] = c;
    }
}
static uint32_t crc32(const uint8_t *p, size_t n) {
    uint32_t c = 0xFFFFFFFFu;
    while (n--) c = crc_tab[(c ^ *p++) & 0xFF] ^ (c >> 8);
    return c ^ 0xFFFFFFFFu;
}

static uint32_t rd16(const uint8_t *p) { return p[0] | p[1] << 8; }
static uint32_t rd32(const uint8_t *p) { return p[0] | p[1] << 8 | p[2] << 16 | (uint32_t)p[3] << 24; }
static void put(Buf *b, const void *s, size_t n) {
    b->p = realloc(b->p, b->n + n + 1);
    memcpy(b->p + b->n, s, n);
    b->n += n;
}
static void put8(Buf *b, uint32_t v) { uint8_t c = (uint8_t)v; put(b, &c, 1); }
static void put16(Buf *b, uint32_t v) { put8(b, v); put8(b, v >> 8); }
static void put32(Buf *b, uint32_t v) { put16(b, v & 0xFFFF); put16(b, v >> 16); }
static void pad4(Buf *b) { while (b->n & 3) put8(b, 0); }
static size_t align4(size_t x) { return (x + 3) & ~(size_t)3; }

static int load(const char *path, Buf *b) {
    FILE *f = fopen(path, "rb");
    if (!f) { perror(path); return 0; }
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    b->p = malloc(n + 1);
    b->n = fread(b->p, 1, n, f);
    fclose(f);
    return b->n == (size_t)n;
}
static int save(const char *path, const Buf *b) {
    FILE *f = fopen(path, "wb");
    if (!f || fwrite(b->p, 1, b->n, f) != b->n) { perror(path); return 0; }
    fclose(f);
    return 1;
}

/* TVC levels (TVC section 8): max coded w, h, MBs per picture, MBs per second, bytes per picture, bytes per window */
static const long LV[4][6] = {{0}, {400, 240, 375, 11250, 96000, 200000},
                               {800, 464, 1450, 43500, 256000, 400000}, {800, 720, 2250, 67500, 384000, 600000}};

/* TAC reset bit: the first bypass bin of the frame (section 5) */
static int tac_reset(const uint8_t *b, uint32_t len) {
    uint32_t c = 0;
    for (int i = 1; i <= 4; i++) c = c << 8 | (i < (int)len ? b[i] : 0);
    return c >= 0x7FFFFFFFu;
}
static uint64_t S(uint64_t k, uint32_t fn, uint32_t fd) { return k * 48000u * fd / fn; }

/* ------------------------------------------------------------------ parsed view of a file */
typedef struct {
    uint32_t flags, w, h, fn, fd, level, ch, rate, nf, nsamp, ntac, idx, so, sl;
    uint32_t *coff, *disp, *ftac, *ptype;   /* per chunk: offset, display index, first TAC frame, picture type */
    uint32_t *taco, *tacl;                  /* per TAC frame: offset in file, length */
    uint8_t *reset;
} Tnv;

static const char *why;
#define BAD(msg) do { why = msg; return 0; } while (0)

/* one variant: event list (section 6.2) against its glyph table */
static int check_variant(const uint8_t *s, uint32_t sl, const uint8_t *g, uint32_t gl, uint32_t vcw, uint32_t vch) {
    if (gl < 4) BAD("glyph table too short");
    uint32_t gc = rd16(g), cw = g[2], chh = g[3], rowb = (cw + 3) / 4;
    if (gc < 1 || gc > 0xFFE0 || cw < 1 || chh < 1) BAD("glyph table header");
    if (cw != vcw || chh != vch) BAD("variant cell size differs from its glyph table");
    if ((uint64_t)gl != 4 + (uint64_t)gc * (1 + chh * rowb)) BAD("glyph table length");
    for (uint32_t i = 0; i < gc; i++) {
        const uint8_t *q = g + 4 + i * (1 + chh * rowb);
        if (q[0] < 1 || q[0] > cw) BAD("glyph advance");
        if (cw & 3)
            for (uint32_t r = 0; r < chh; r++)
                if (q[1 + r * rowb + rowb - 1] >> (2 * (cw & 3))) BAD("glyph padding bits");
    }
    if (sl < 4) BAD("event list too short");
    uint32_t cnt = rd32(s), pos = 4, last = 0;
    for (uint32_t e = 0; e < cnt; e++) {
        if (pos + 16 > sl) BAD("event list truncated");
        const uint8_t *q = s + pos;
        uint32_t st = rd32(q), en = rd32(q + 4), x = rd16(q + 8), y = rd16(q + 10), n = rd16(q + 14);
        if (st >= en) BAD("subtitle event end <= start");
        if (e && st < last) BAD("subtitle events not sorted");
        if (x >= 800 || y >= 720 || q[12] > 15 || q[13] > 3) BAD("subtitle event position/colour/flags");
        last = st;
        pos += 16;
        if ((uint64_t)pos + 2 * n > sl) BAD("event list truncated");
        for (uint32_t k = 0; k < n; k++) {
            uint32_t id = rd16(s + pos + 2 * k);
            if (id >= 0xFFF0 && id != 0xFFFF) BAD("reserved glyph reference code");
            if (id < 0xFFE0 && id >= gc) BAD("glyph id out of range");
        }
        pos += 2 * n;
    }
    if (pos != sl) BAD("event list length");
    return 1;
}
/* the subtitle area (section 6.1): directory, then every variant's event list and glyph table in order */
static int check_area(const uint8_t *a, uint32_t al) {
    if (al < 4) BAD("subtitle area too short");
    uint32_t nt = a[0], pos = 4, ndef = 0;
    if (nt < 1 || nt > 15 || a[1] || a[2] || a[3]) BAD("track count / reserved");
    uint32_t dpos = 4;                               /* first pass: directory size */
    for (uint32_t t = 0; t < nt; t++) {
        if (dpos + 56 > al) BAD("directory truncated");
        uint32_t nv = a[dpos + 5];
        if (nv < 1 || nv > 8) BAD("variant count");
        dpos += 56 + 20 * nv;
    }
    if (dpos > al) BAD("directory truncated");
    size_t next = align4(dpos);
    for (size_t z = dpos; z < next && z < al; z++) if (a[z]) BAD("non-zero subtitle padding");
    for (uint32_t t = 0; t < nt; t++) {
        const uint8_t *e = a + pos;
        int letters = 0, ended = 0;
        for (int k = 0; k < 4; k++) {
            if (!e[k]) ended = 1;
            else if (ended || e[k] < 'a' || e[k] > 'z') BAD("track language");
            else letters++;
        }
        if (letters < 2) BAD("track language");
        if (e[4] >> 2 || e[6] || e[7]) BAD("track flags / reserved");
        ndef += e[4] & 1;
        uint32_t nv = e[5], prevh = 0;
        for (uint32_t v = 0; v < nv; v++) {
            const uint8_t *d = e + 56 + 20 * v;
            uint32_t cw = d[0], chh = d[1], eo = rd32(d + 4), el = rd32(d + 8), go = rd32(d + 12), gl = rd32(d + 16);
            if (rd16(d + 2)) BAD("variant reserved");
            if (chh <= prevh) BAD("variant cell heights not increasing");
            prevh = chh;
            if (eo != next || (uint64_t)eo + el > al) BAD("event list offset");
            size_t gpos = align4((size_t)eo + el);
            for (size_t z = (size_t)eo + el; z < gpos && z < al; z++) if (a[z]) BAD("non-zero subtitle padding");
            if (go != gpos || (uint64_t)go + gl > al) BAD("glyph table offset");
            if (!check_variant(a + eo, el, a + go, gl, cw, chh)) return 0;
            next = align4((size_t)go + gl);
            if (next > (size_t)go + gl) for (size_t z = (size_t)go + gl; z < next && z < al; z++) if (a[z]) BAD("non-zero subtitle padding");
            if (t + 1 == nt && v + 1 == nv && (size_t)go + gl != al) BAD("subtitle area length");
        }
        pos += 56 + 20 * nv;
    }
    if (ndef > 1) BAD("more than one default track");
    return 1;
}

static int parse(const Buf *b, Tnv *t) {
    const uint8_t *p = b->p;
    memset(t, 0, sizeof *t);
    if (b->n < 64 || memcmp(p, "TNV3", 4)) BAD("not a TNV3 file");
    if (rd16(p + 4) != 0) BAD("version");
    if (rd32(p + 60) != crc32(p, 60)) BAD("header CRC");
    t->flags = rd16(p + 6); t->w = rd16(p + 8); t->h = rd16(p + 10); t->fn = rd32(p + 12); t->fd = rd32(p + 16);
    t->level = p[20]; t->ch = p[21]; t->rate = rd32(p + 24); t->nf = rd32(p + 28); t->nsamp = rd32(p + 32);
    t->ntac = rd32(p + 36); t->idx = rd32(p + 40); t->so = rd32(p + 44); t->sl = rd32(p + 48);
    int audio = t->flags >> 1 & 1, subs = t->flags >> 2 & 1;
    if (t->flags >> 3 || rd16(p + 22) || rd32(p + 52) || rd32(p + 56)) BAD("reserved header bits");
    if (!t->w || !t->h || !t->fn || !t->fd || t->fn > 65535 || t->fd > 65535) BAD("size / frame rate");
    if (t->level < 1 || t->level > 3) BAD("level");
    if (t->nf < 1) BAD("frame count");
    if (audio) {
        if ((t->ch != 1 && t->ch != 2) || t->rate != 48000) BAD("audio channels / rate");
        if (t->ntac != (t->nsamp + 1023) / 1024 + 1) BAD("tac_frames formula");
    } else if (t->ch || t->rate || t->nsamp || t->ntac) BAD("audio fields without audio");
    if (!subs && (t->so || t->sl)) BAD("subtitle fields without subs");

    /* chunks */
    t->coff = calloc(t->nf, 4); t->disp = calloc(t->nf, 4); t->ftac = calloc(t->nf, 4); t->ptype = calloc(t->nf, 4);
    t->taco = calloc(t->ntac + 1, 4); t->tacl = calloc(t->ntac + 1, 4); t->reset = calloc(t->ntac + 1, 1);
    int crc = t->flags & 1;
    size_t pos = 64;
    uint32_t nt = 0, anchors = 0;
    for (uint32_t n = 0; n < t->nf; n++) {
        if (pos + 16 > b->n) BAD("chunk header outside the file");
        const uint8_t *c = p + pos;
        uint32_t clen = rd32(c), plen = rd32(c + 4), na = rd16(c + 8), fl = rd16(c + 10);
        if (clen < 16 || pos + clen > b->n) BAD("chunk length outside the file");
        if (plen < 3 || 16 + (uint64_t)plen > clen) BAD("picture length");
        uint32_t pt = c[16];
        if (pt > 2) BAD("picture type");
        if (pt == 1 && anchors < 1) BAD("P picture without an anchor");
        if (pt == 2 && anchors < 2) BAD("B picture without two anchors");
        if (pt != 2) anchors++;
        if (fl != (uint32_t)((pt == 0) | (pt == 2) << 1)) BAD("chunk flags");
        if (n == 0 && pt != 0) BAD("first chunk not key");
        if (!audio && na) BAD("audio frames without audio");
        size_t q = pos + 16 + plen;
        for (uint32_t i = 0; i < na; i++) {
            if (q + 2 > pos + clen) BAD("TAC frame outside the chunk");
            uint32_t l = rd16(p + q);
            if (l < 1 || q + 2 + l > pos + clen) BAD("TAC frame length");
            if (nt >= t->ntac) BAD("more TAC frames than tac_frames");
            t->taco[nt] = (uint32_t)(q + 2); t->tacl[nt] = l; t->reset[nt] = (uint8_t)tac_reset(p + q + 2, l);
            nt++;
            q += 2 + l;
        }
        if (q != pos + clen) BAD("chunk_len does not match its contents");
        if (crc ? rd32(c + 12) != crc32(c + 16, clen - 16) : rd32(c + 12) != 0) BAD("chunk CRC");
        t->coff[n] = (uint32_t)pos; t->ptype[n] = pt; t->ftac[n] = nt;   /* ftac = F(n) for now */
        size_t end = pos + clen;
        pos = align4(end);
        if (pos > b->n) BAD("chunk padding outside the file");
        for (size_t z = end; z < pos; z++) if (p[z]) BAD("non-zero padding");
    }
    if (nt != t->ntac) BAD("TAC frame total");
    if (audio && !t->reset[0]) BAD("TAC frame 0 without reset");

    /* display order (section 3) */
    uint32_t out = 0; int64_t held = -1;
    for (uint32_t n = 0; n < t->nf; n++) {
        if (t->ptype[n] == 2) t->disp[n] = out++;
        else { if (held >= 0) t->disp[held] = out++; held = n; }
    }
    t->disp[held] = out++;

    /* interleaving bounds (4.1) and TVC level limits (7.6) */
    uint32_t D = 0;
    for (uint32_t n = 0; n < t->nf; n++) {
        if (t->disp[n] > D) D = t->disp[n];
        if (S(t->disp[n], t->fn, t->fd) > 0xFFFFFFFFu) BAD("presentation sample beyond 32 bits");
        if (!audio) continue;
        uint64_t need = S(D + 1, t->fn, t->fd) / 1024 + 2;
        if (need > t->ntac) need = t->ntac;
        if (t->ftac[n] < need) BAD("audio interleaving: lower bound");
        if (n + 1 < t->nf && t->ftac[n] > need + 4) BAD("audio interleaving: upper bound");
    }
    const long *L = LV[t->level];
    long cw = (t->w + 15) & ~15, chh = (t->h + 15) & ~15, mbs = cw / 16 * (chh / 16);
    if (cw > L[0] || chh > L[1] || mbs > L[2]) BAD("level: picture size");
    if (t->fn > 30u * t->fd) BAD("level: frame rate");
    if ((uint64_t)mbs * t->fn > (uint64_t)L[3] * t->fd) BAD("level: macroblocks per second");
    uint32_t win = (t->fn + t->fd - 1) / t->fd;
    for (uint32_t n = 0; n < t->nf; n++) {
        long sum = 0, plen = rd32(p + t->coff[n] + 4);
        if (plen > L[4]) BAD("level: bytes per picture");
        for (uint32_t k = 0; k < win && k <= n; k++) sum += rd32(p + t->coff[n - k] + 4);
        if (sum > L[5]) BAD("level: bytes per second of pictures");
        long csum = 0;                    /* section 4.2: whole chunks, video + audio + headers */
        for (uint32_t k = 0; k < win && k <= n; k++) csum += rd32(p + t->coff[n - k]);
        if (csum > L[5] + 65536) BAD("chunk bytes per second");
    }

    /* index (section 5) */
    if (t->idx != pos) BAD("index offset");
    if ((uint64_t)pos + 16ull * t->nf > b->n) BAD("index outside the file");
    for (uint32_t n = 0; n < t->nf; n++) {
        const uint8_t *e = p + pos + 16 * n;
        uint32_t first = n ? t->ftac[n - 1] : 0, sk = (uint32_t)S(t->disp[n], t->fn, t->fd), fl = rd16(p + t->coff[n] + 10), seek = 0;
        if (audio && (fl & 1)) {
            for (uint32_t r = 0; r < t->ntac && (uint64_t)r * 1024 <= sk; r++) if (t->reset[r]) seek = r;
        }
        if (rd32(e) != t->coff[n] || rd32(e + 4) != sk || rd32(e + 8) != first || rd32(e + 12) != (fl | seek << 8)) BAD("index entry");
    }
    for (uint32_t n = t->nf; n-- > 0;) t->ftac[n] = n ? t->ftac[n - 1] : 0;   /* now first_tac */
    pos += 16ull * t->nf;

    /* subtitles (section 6) */
    if (subs) {
        if (t->so != pos || (uint64_t)t->so + t->sl > b->n) BAD("subtitle offset");
        if (!check_area(p + t->so, t->sl)) return 0;
        pos = t->so + t->sl;
    }
    if (pos != b->n) BAD("file size");
    return 1;
}

/* ------------------------------------------------------------------ mux */
static int mux(int argc, char **argv) {
    const char *vin = NULL, *ain = NULL, *sin = NULL, *outp = NULL;
    int crc = 0, level = 0, trim = 0, nocheck = 0, shift = 0;
    uint32_t fn = 0, fd = 0;
    for (int i = 2; i < argc; i++) {
        if (!strcmp(argv[i], "-a") && i + 1 < argc) ain = argv[++i];
        else if (!strcmp(argv[i], "-s") && i + 1 < argc) sin = argv[++i];
        else if (!strcmp(argv[i], "-o") && i + 1 < argc) outp = argv[++i];
        else if (!strcmp(argv[i], "-L") && i + 1 < argc) level = atoi(argv[++i]);
        else if (!strcmp(argv[i], "-r") && i + 1 < argc) { if (sscanf(argv[++i], "%u/%u", &fn, &fd) != 2) return 1; }
        else if (!strcmp(argv[i], "-c")) crc = 1;
        else if (!strcmp(argv[i], "-T")) trim = 1;
        else if (!strcmp(argv[i], "-x")) nocheck = 1;
        else if (!strcmp(argv[i], "-A") && i + 1 < argc) shift = atoi(argv[++i]);
        else if (!vin) vin = argv[i];
        else { fprintf(stderr, "unknown argument %s\n", argv[i]); return 1; }
    }
    if (!vin || !outp) { fprintf(stderr, "usage: tnv3 mux VIDEO.tvc [-a AUDIO.tac] [-s SUBTITLE_AREA] [-c] [-L l] [-r n/d] -o OUT\n"); return 1; }
    Buf v = {0}, a = {0}, s = {0};
    if (!load(vin, &v) || (ain && !load(ain, &a)) || (sin && !load(sin, &s))) return 1;
    if (v.n < 16 || (memcmp(v.p, "TVC0", 4) && memcmp(v.p, "TVC1", 4))) { fprintf(stderr, "not a TVC file\n"); return 1; }
    uint32_t w = rd16(v.p + 4), h = rd16(v.p + 6), nf = rd32(v.p + 12);
    size_t vp = 16;
    if (v.p[3] == '1') { if (!level) level = v.p[16]; vp = 20; }
    if (!fn) { fn = rd16(v.p + 8); fd = rd16(v.p + 10); }
    if (!level) { fprintf(stderr, "TVC0 input needs -L level\n"); return 1; }
    uint32_t *po = calloc(nf, 4), *pl = calloc(nf, 4);
    for (uint32_t i = 0; i < nf; i++) {
        if (vp + 4 > v.n) { fprintf(stderr, "truncated TVC\n"); return 1; }
        pl[i] = rd32(v.p + vp); po[i] = (uint32_t)(vp + 4); vp += 4 + (size_t)pl[i];
        if (vp > v.n) { fprintf(stderr, "truncated TVC\n"); return 1; }
    }
    uint32_t ch = 0, nsamp = 0, ntac = 0, *ao = NULL, *al = NULL;
    if (ain) {
        if (a.n < 20 || memcmp(a.p, "TAC0", 4) || rd32(a.p + 4) != 48000) { fprintf(stderr, "not a 48 kHz TAC0 file\n"); return 1; }
        ch = a.p[8]; ntac = rd32(a.p + 12); nsamp = rd32(a.p + 16);
        ao = calloc(ntac + 1, 4); al = calloc(ntac + 1, 4);
        size_t q = 20;
        for (uint32_t i = 0; i < ntac; i++) {
            if (q + 2 > a.n) { fprintf(stderr, "truncated TAC\n"); return 1; }
            al[i] = rd16(a.p + q); ao[i] = (uint32_t)(q + 2); q += 2 + al[i];
            if (q > a.n) { fprintf(stderr, "truncated TAC\n"); return 1; }
        }
        uint64_t dur = S(nf, fn, fd);
        if (trim && dur < nsamp) { nsamp = (uint32_t)dur; ntac = (nsamp + 1023) / 1024 + 1; }
    }
    /* display order, as in the checker */
    uint32_t *disp = calloc(nf, 4), out = 0; int64_t held = -1;
    for (uint32_t n = 0; n < nf; n++) {
        if (v.p[po[n]] == 2) disp[n] = out++;
        else { if (held >= 0) disp[held] = out++; held = n; }
    }
    if (held >= 0) disp[held] = out;

    Buf o = {0};
    uint8_t hdr[64] = {0};
    put(&o, hdr, 64);
    uint32_t *coff = calloc(nf, 4), *first = calloc(nf, 4), D = 0, F = 0;
    for (uint32_t n = 0; n < nf; n++) {
        if (disp[n] > D) D = disp[n];
        uint32_t target = F;
        if (ain) {
            uint64_t need = S(D + 1, fn, fd) / 1024 + 2;
            int64_t tg = n + 1 == nf ? (int64_t)ntac : (int64_t)need + shift;
            target = tg > (int64_t)ntac ? ntac : tg < 0 ? 0 : (uint32_t)tg;
            if (target < F) target = F;
        }
        uint32_t na = target - F, alen = 0;
        for (uint32_t i = F; i < target; i++) alen += 2 + al[i];
        coff[n] = (uint32_t)o.n; first[n] = F;
        uint32_t pt = v.p[po[n]];
        size_t c0 = o.n;
        put32(&o, 16 + pl[n] + alen); put32(&o, pl[n]); put16(&o, na); put16(&o, (pt == 0) | (pt == 2) << 1); put32(&o, 0);
        put(&o, v.p + po[n], pl[n]);
        for (uint32_t i = F; i < target; i++) { put16(&o, al[i]); put(&o, a.p + ao[i], al[i]); }
        if (crc) { uint32_t c = crc32(o.p + c0 + 16, o.n - c0 - 16); o.p[c0 + 12] = c; o.p[c0 + 13] = c >> 8; o.p[c0 + 14] = c >> 16; o.p[c0 + 15] = c >> 24; }
        F = target;
        pad4(&o);
    }
    uint32_t idx = (uint32_t)o.n;
    for (uint32_t n = 0; n < nf; n++) {
        uint32_t pt = v.p[po[n]], fl = (pt == 0) | (pt == 2) << 1, sk = (uint32_t)S(disp[n], fn, fd), seek = 0;
        if (ain && (fl & 1))
            for (uint32_t r = 0; r < ntac && (uint64_t)r * 1024 <= sk; r++) if (tac_reset(a.p + ao[r], al[r])) seek = r;
        put32(&o, coff[n]); put32(&o, sk); put32(&o, first[n]); put32(&o, fl | seek << 8);
    }
    uint32_t so = 0;
    if (sin) { so = (uint32_t)o.n; put(&o, s.p, s.n); }
    uint8_t *H = o.p;
    memcpy(H, "TNV3", 4);
    uint32_t flags = crc | (ain ? 2 : 0) | (sin ? 4 : 0);
    uint32_t f16[][2] = {{4, 0}, {6, flags}, {8, w}, {10, h}};
    for (int i = 0; i < 4; i++) { H[f16[i][0]] = f16[i][1]; H[f16[i][0] + 1] = f16[i][1] >> 8; }
    uint32_t f32[][2] = {{12, fn}, {16, fd}, {24, ain ? 48000 : 0}, {28, nf}, {32, nsamp}, {36, ntac}, {40, idx},
                         {44, so}, {48, (uint32_t)s.n}};
    for (int i = 0; i < 9; i++) for (int k = 0; k < 4; k++) H[f32[i][0] + k] = f32[i][1] >> (8 * k);
    H[20] = level; H[21] = ch;
    uint32_t hc = crc32(H, 60);
    for (int k = 0; k < 4; k++) H[60 + k] = hc >> (8 * k);
    Tnv t;
    if (!nocheck && !parse(&o, &t)) { fprintf(stderr, "muxed file is invalid: %s\n", why); return 3; }
    return save(outp, &o) ? 0 : 1;
}

/* ------------------------------------------------------------------ demux, info */
static int demux(int argc, char **argv, const Buf *b, const Tnv *t) {
    Buf v = {0};
    put(&v, "TVC1", 4); put16(&v, t->w); put16(&v, t->h); put16(&v, t->fn); put16(&v, t->fd); put32(&v, t->nf);
    put8(&v, t->level); put8(&v, 0); put16(&v, 0);
    for (uint32_t n = 0; n < t->nf; n++) { uint32_t pl = rd32(b->p + t->coff[n] + 4); put32(&v, pl); put(&v, b->p + t->coff[n] + 16, pl); }
    if (!save(argv[3], &v)) return 1;
    if (argc > 4 && (t->flags & 2)) {       /* outputs for absent streams are not written */
        Buf a = {0};
        put(&a, "TAC0", 4); put32(&a, 48000); put8(&a, t->ch); put8(&a, 0); put16(&a, 0); put32(&a, t->ntac); put32(&a, t->nsamp);
        for (uint32_t i = 0; i < t->ntac; i++) { put16(&a, t->tacl[i]); put(&a, b->p + t->taco[i], t->tacl[i]); }
        if (!save(argv[4], &a)) return 1;
    }
    if (argc > 5 && (t->flags & 4)) {
        Buf s = {b->p + t->so, t->sl};
        if (!save(argv[5], &s)) return 1;
    }
    return 0;
}

static void info(const Buf *b, const Tnv *t) {
    uint32_t keys = 0, bs = 0, maxa = 0;
    for (uint32_t n = 0; n < t->nf; n++) {
        uint32_t na = rd16(b->p + t->coff[n] + 8);
        keys += t->ptype[n] == 0; bs += t->ptype[n] == 2; if (na > maxa) maxa = na;
    }
    printf("TNV3 v0  %ux%u  %u/%u fps  level %u  flags crc=%u audio=%u subs=%u\n", t->w, t->h, t->fn, t->fd, t->level,
           t->flags & 1, t->flags >> 1 & 1, t->flags >> 2 & 1);
    printf("pictures %u (key %u, B %u)  duration %llu samples\n", t->nf, keys, bs, (unsigned long long)S(t->nf, t->fn, t->fd));
    if (t->flags & 2) printf("audio %u ch, %u samples, %u TAC frames, at most %u per chunk\n", t->ch, t->nsamp, t->ntac, maxa);
    if (t->flags & 4) printf("subtitles: %u track(s), %u bytes\n", b->p[t->so], t->sl);
    printf("file %zu bytes, index at %u\n", b->n, t->idx);
}

#define TNV3_TOOL_VERSION "0.1.0"

int main(int argc, char **argv) {
    if (argc == 2 && !strcmp(argv[1], "--version")) { puts("tnv3 " TNV3_TOOL_VERSION); return 0; }
    crc_init();
    if (argc >= 2 && !strcmp(argv[1], "mux")) return mux(argc, argv);
    if (argc < 3) { fprintf(stderr, "usage: tnv3 mux|demux|check|info ...\n"); return 1; }
    Buf b = {0};
    if (!load(argv[2], &b)) return 1;
    Tnv t;
    if (!parse(&b, &t)) { fprintf(stderr, "invalid: %s\n", why); return 3; }
    if (!strcmp(argv[1], "check")) { printf("valid\n"); return 0; }
    if (!strcmp(argv[1], "info")) { info(&b, &t); return 0; }
    if (!strcmp(argv[1], "demux") && argc >= 4) return demux(argc, argv, &b, &t);
    fprintf(stderr, "usage: tnv3 mux|demux|check|info ...\n");
    return 1;
}
