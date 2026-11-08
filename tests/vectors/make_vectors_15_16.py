#!/usr/bin/env python3
"""Builds the TNV3 draft 1.5 / 1.6 conformance vectors owned by this repository (stdlib only).

Valid files ok*.tnv3 and must-reject files bad*.tnv3 each break exactly one rule of the container spec
(docs/TNV3_CONTAINER_V0.md section 6 and 7): the baked outline width and plane (draft 1.5), the event bottom bound
(draft 1.5) and the video codec id and reserved header bytes (draft 1.6). The expected verdict of every vector is written
here from the spec, not taken from a checker's output; `--verify` runs the C checker against it.

    python3 tests/vectors/make_vectors_15_16.py [--verify]      (needs tools/tnv3_ref/tnv3 and tools/tvc_ref/tvc_enc built)

Output: tests/vectors/tnv3_15_16/*.tnv3 and expected.txt (name, verdict, reason, SHA-256). The video is a tiny synthetic
gradient encoded by our tvc_enc and muxed by our tnv3, so the files depend on nothing from Tau V."""
import hashlib, os, struct, subprocess, sys, tempfile, zlib

ROOT = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(ROOT, '..', '..'))
TNV3 = os.path.join(REPO, 'tools', 'tnv3_ref', 'tnv3')
TVC = os.path.join(REPO, 'tools', 'tvc_ref', 'tvc_enc')
OUT = os.path.join(ROOT, 'tnv3_15_16')


def run(cmd, ok=(0,)):
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode not in ok:
        sys.exit('%s failed (%d): %s' % (' '.join(cmd), r.returncode, r.stderr))
    return r


def base_video():
    """Video-only TNV3 of 4 synthetic 32x32 pictures at 24 fps (8000 samples), as bytes."""
    with tempfile.TemporaryDirectory() as d:
        y4m = os.path.join(d, 'in.y4m')
        with open(y4m, 'wb') as f:
            f.write(b'YUV4MPEG2 W32 H32 F24:1 Ip A1:1 C420jpeg\n')
            for k in range(4):
                f.write(b'FRAME\n')
                f.write(bytes((x * 4 + y * 2 + k * 8) & 255 for y in range(32) for x in range(32)))
                f.write(bytes(128 + ((x + k) & 7) for y in range(16) for x in range(16)))
                f.write(bytes(128 - ((y + k) & 7) for y in range(16) for x in range(16)))
        tvc, out = os.path.join(d, 'v.tvc'), os.path.join(d, 'base.tnv3')
        run([TVC, y4m, tvc, '-q', '30', '-l', '1'])
        run([TNV3, 'mux', tvc, '-L', '1', '-o', out])
        return open(out, 'rb').read()


# ------------------------------------------------------------------------------------------------ area building
def pixel(cw, k, x, y):
    return ((x * 3 + y * 5 + k * 7) % 11 == 0) * 3 + ((x + y + k) % 5 == 0) * 2


def fill_rows(cw, ch, k):
    return [[min(3, pixel(cw, k, x, y)) for x in range(cw)] for y in range(ch)]


def dilate(rows, w):
    ch, cw = len(rows), len(rows[0])
    plane = [[0] * (cw + 2 * w) for _ in range(ch + 2 * w)]
    offs = [(dx, dy) for dx in range(-w, w + 1) for dy in range(-w, w + 1) if dx * dx + dy * dy <= w * w]
    for r in range(ch):
        for c in range(cw):
            a = rows[r][c]
            if a:
                for dx, dy in offs:
                    plane[r + w + dy][c + w + dx] = max(plane[r + w + dy][c + w + dx], a)
    return plane


def pack_plane(rows):
    rb = (len(rows[0]) + 3) // 4
    out = bytearray()
    for row in rows:
        b = bytearray(rb)
        for x, v in enumerate(row):
            b[x // 4] |= v << (2 * (x % 4))
        out += b
    return out


def glyph_table(cw, ch, n, ow, mutate=None):
    g = bytearray(struct.pack('<HBB', n, cw, ch))
    for k in range(n):
        rows = fill_rows(cw, ch, k)
        g.append(max(1, cw - 1 - (k % 2)))
        g += pack_plane(rows)
        if ow:
            plane = dilate(rows, ow)
            if mutate:
                mutate(k, plane)
            g += pack_plane(plane)
    return bytes(g)


def event_list(ev):
    s = struct.pack('<I', len(ev))
    for st, en, x, y, col, fl, ids in ev:
        s += struct.pack('<IIHHBBH', st, en, x, y, col, fl, len(ids)) + b''.join(struct.pack('<H', i) for i in ids)
    return s


def align4(x):
    return (x + 3) & ~3


def pack_area(lang, variants):
    """variants: list of (cell_w, cell_h, outline_w_in_directory, reserved_byte, events_bytes, glyph_bytes)."""
    dir_len = 4 + 56 + 20 * len(variants)
    pos = align4(dir_len)
    body = bytearray(pos - dir_len)
    pal = [(255, 255, 255), (255, 230, 0)] + [(160, 160, 160)] * 14
    entries = bytearray(struct.pack('<B3x', 1))
    entries += lang.ljust(4, b'\0') + struct.pack('<BBxx', 0, len(variants)) + bytes(c for rgb in pal for c in rgb)
    last = 0
    for cw, ch, ow, res, ev, gl in variants:
        eo = pos
        body += ev; pos += len(ev)
        pad = align4(pos) - pos; body += bytes(pad); pos += pad
        go = pos
        body += gl; pos += len(gl)
        entries += struct.pack('<BBBBIIII', cw, ch, ow, res, eo, len(ev), go, len(gl))
        last = go + len(gl)
        pad = align4(pos) - pos; body += bytes(pad); pos += pad
    return (bytes(entries) + bytes(body))[:last]


def attach(base, area, codec=0, byte23=0):
    h = bytearray(base)
    so = len(base)
    h[6] |= 4
    struct.pack_into('<I', h, 44, so)
    struct.pack_into('<I', h, 48, len(area))
    h[22], h[23] = codec, byte23
    struct.pack_into('<I', h, 60, zlib.crc32(bytes(h[:60])) & 0xFFFFFFFF)
    return bytes(h) + area


def with_header(base, codec=0, byte23=0):
    h = bytearray(base)
    h[22], h[23] = codec, byte23
    struct.pack_into('<I', h, 60, zlib.crc32(bytes(h[:60])) & 0xFFFFFFFF)
    return bytes(h)


EV = [(0, 4000, 4, 600, 0, 0, [0, 1]), (2000, 8000, 4, 640, 1, 2, [1, 0xFFFF, 0])]


def variant(cw, ch, ow, events=EV, n=3, dir_ow=None, reserved=0, mutate=None, drop_plane=False, cut=0):
    gl = glyph_table(cw, ch, n, 0 if drop_plane else ow, mutate)
    if cut:
        gl = gl[:-cut]
    return (cw, ch, ow if dir_ow is None else dir_ow, reserved, event_list(events), gl)


def flip_pixel(k, plane):
    if k == 1:
        plane[0][0] = (plane[0][0] + 1) % 4         # breaks equality with the dilation of the fill


def set_padding(k, plane):
    pass                                             # padding is patched after packing (below)


def vectors(base):
    v = []   # (name, verdict, reason, bytes)
    ok = lambda n, why, data: v.append((n, 'valid', why, data))
    bad = lambda n, why, data: v.append((n, 'invalid', why, data))
    A = lambda variants: attach(base, pack_area(b'eng', variants))
    # valid
    ok('ok15_w0_draft14_compatible', 'outline_w 0: no outline plane (draft 1.4 layout)', A([variant(6, 8, 0)]))
    ok('ok15_w1_w2_two_sizes', 'outline widths 1 and 2 on two sizes, planes equal the dilation', A([variant(6, 8, 1), variant(9, 12, 2)]))
    ok('ok15_w4_largest', 'outline_w 4, the largest allowed', A([variant(6, 8, 4)]))
    ev_bottom = [(0, 4000, 4, 720 - 8, 0, 0, [0, 1])]
    ok('ok15_event_bottom_exact', 'one line ending exactly on row 720 (y + 1 * cell_h = 720)', A([variant(6, 8, 1, ev_bottom)]))
    ev_two = [(0, 4000, 4, 720 - 16, 0, 0, [0, 0xFFFF, 1])]
    ok('ok15_two_lines_bottom_exact', 'two lines ending exactly on row 720 (y + 2 * cell_h = 720)', A([variant(6, 8, 1, ev_two)]))
    ok('ok16_codec_id_zero', 'header byte 22 = 0 (TVC) with subtitles present', A([variant(6, 8, 1)]))
    # draft 1.5 must-reject
    bad('bad15_outline_w5', 'outline_w 5 is above 4 (plane built consistently for w = 5)', A([variant(6, 8, 5)]))
    bad('bad15_plane_not_dilation', 'one outline pixel differs from the dilation of the fill', A([variant(6, 8, 1, mutate=flip_pixel)]))
    good = A([variant(6, 8, 1)])
    # padding bits: cell 6 + 2w(=2) = 8 pixels per outline row -> no padding; use w = 1 with cw 7 (9 px -> 3 bytes, padding 3 px)
    pad = bytearray(A([variant(7, 8, 1)]))
    area0 = len(base)
    eo, el, go, gl = struct.unpack_from('<IIII', pad, area0 + 4 + 56 + 8)
    rb, orb = (7 + 3) // 4, (7 + 2 + 3) // 4
    first_outline_row_end = area0 + go + 4 + 1 + 8 * rb + orb - 1       # last byte of the first outline row of glyph 0
    pad[first_outline_row_end] |= 0b11000000                              # pixel 11 of a 9-pixel row: padding
    bad('bad15_outline_padding_bits', 'a padding bit of an outline row is set', bytes(pad))
    bad('bad15_variant_reserved_byte', 'variant reserved byte is 1', A([variant(6, 8, 1, reserved=1)]))
    bad('bad15_plane_missing', 'outline_w 1 but the glyph table has no outline planes (length is fill-only)', A([variant(6, 8, 1, drop_plane=True)]))
    bad('bad15_table_one_byte_short', 'glyph table one byte shorter than 4 + n * record', A([variant(6, 8, 1, cut=1)]))
    ev_below = [(0, 4000, 4, 720 - 8 + 1, 0, 0, [0, 1])]
    bad('bad15_event_below_grid', 'one line ends on row 721 (y + cell_h > 720)', A([variant(6, 8, 1, ev_below)]))
    ev_below2 = [(0, 4000, 4, 720 - 16 + 1, 0, 0, [0, 0xFFFF, 1])]
    bad('bad15_event_below_grid_break', 'two lines end on row 721 (y + 2 * cell_h > 720)', A([variant(6, 8, 1, ev_below2)]))
    # draft 1.6 must-reject
    area = pack_area(b'eng', [variant(6, 8, 1)])
    bad('bad16_codec_id_1', 'header byte 22 (video_codec) = 1, header CRC fixed', attach(base, area, codec=1))
    bad('bad16_codec_id_255', 'header byte 22 (video_codec) = 255, header CRC fixed', attach(base, area, codec=255))
    bad('bad16_reserved_byte_23', 'header byte 23 (reserved) = 1, header CRC fixed', attach(base, area, byte23=1))
    bad('bad16_codec_id_without_subtitles', 'header byte 22 = 1 on a video-only file, header CRC fixed', with_header(base, codec=1))
    return v


def check_c(path):
    r = subprocess.run([TNV3, 'check', path], capture_output=True, text=True)
    return ('valid' if r.stdout.startswith('valid') else 'invalid'), (r.stdout + r.stderr).strip()


def main():
    verify = '--verify' in sys.argv
    for t in (TNV3, TVC):
        if not os.path.exists(t):
            sys.exit('build the C tools first (make all): missing %s' % t)
    os.makedirs(OUT, exist_ok=True)
    lines, wrong = [], 0
    for name, verdict, why, data in vectors(base_video()):
        path = os.path.join(OUT, name + '.tnv3')
        open(path, 'wb').write(data)
        sha = hashlib.sha256(data).hexdigest()
        lines.append('%s %s sha256=%s  # %s' % (name, verdict, sha, why))
        if verify:
            got, msg = check_c(path)
            flag = 'ok ' if got == verdict else 'MISMATCH'
            wrong += got != verdict
            print('%-8s %-34s expected %-7s C says: %s' % (flag, name, verdict, msg.splitlines()[0] if msg else ''))
    open(os.path.join(OUT, 'expected.txt'), 'w').write('\n'.join(lines) + '\n')
    print('%d vectors written to %s' % (len(lines), OUT))
    return 1 if wrong else 0


if __name__ == '__main__':
    sys.exit(main())
