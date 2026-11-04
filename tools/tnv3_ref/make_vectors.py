#!/usr/bin/env python3
"""Build the TNV3 container conformance vectors (docs/TNV3_CONTAINER_V0.md) with the C reference muxer.

Positive vectors p*.tnv3 mux TVC and TAC conformance streams at several frame rates; for each one the C demuxer's
.tvc/.tac (and subtitle/glyph) outputs are hashed into vectors/expected.txt. Negative vectors n*.tnv3 each break
one rule of section 7 and must be rejected. Run from the worktree root after building tools/tnv3_ref/tnv3."""
import hashlib, os, struct, subprocess, sys, tempfile, zlib
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'tst_ref'))
from tst_area import pack_area

ROOT = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(ROOT, 'tnv3')
OUT = os.path.join(ROOT, 'vectors')
TVC = os.path.join(ROOT, '..', 'tvc_ref', 'vectors')
TAC = os.path.join(ROOT, '..', 'tac_ref', 'vectors')


def run(args, ok=(0,)):
    r = subprocess.run([TOOL] + args, capture_output=True, text=True)
    if r.returncode not in ok:
        sys.exit('tnv3 %s failed (%d): %s' % (' '.join(args), r.returncode, r.stderr))
    return r


def glyph_table(cw, ch, n, seed=0):
    rowb = (cw + 3) // 4
    g = struct.pack('<HBB', n, cw, ch)
    for k in range(n):
        g += bytes([max(1, cw - 2 - (k % 3))])
        for r in range(ch):
            row = bytearray(rowb)
            for x in range(cw):
                row[x // 4] |= ((r + x + k + seed) % 4) << (2 * (x % 4))
            g += bytes(row)
    return g


def event_list(ev):
    s = struct.pack('<I', len(ev))
    for st, en, x, y, col, fl, ids in ev:
        s += struct.pack('<IIHHBBH', st, en, x, y, col, fl, len(ids)) + b''.join(struct.pack('<H', i) for i in ids)
    return s


EV_SMALL = [(0, 4000, 40, 600, 0, 0, [0, 1, 2]), (2000, 9000, 40, 640, 3, 2, [2, 0xFFFF, 1]), (2000, 2500, 700, 10, 1, 0, [])]
EV_LARGE = [(0, 4000, 30, 590, 0, 0, [0, 1]), (0, 4000, 30, 602, 0, 0, [2]), (2000, 9000, 30, 640, 3, 2, [2, 0xFFFF, 1]), (2000, 2500, 690, 10, 1, 0, [])]
EV_FORCED = [(1000, 3000, 300, 680, 0, 1, [1, 0])]
# draft 1.4: authored colour 9 and inline switches (0xFFE0 + k) that persist across a line break
EV_COLOURS = [(0, 5000, 50, 640, 9, 0, [0, 0xFFE5, 1, 0xFFFF, 2, 0xFFE0, 0]), (500, 900, 400, 100, 15, 2, [2])]
DEMON = [(255, 255, 255), (255, 230, 0)] + [(100, 100, 100)] * 7 + [(200, 20, 20)] + [(90, 90, 90)] * 5 + [(10, 200, 120)]


def subtitle_areas():
    """p06: one track, one size; p08: English (default, two sizes, an sdh event) and Portuguese forced-only"""
    one = pack_area([dict(lang='und', variants=[(event_list(EV_SMALL), glyph_table(6, 8, 3))])])
    two = pack_area([dict(lang='eng', default=True, variants=[(event_list(EV_SMALL), glyph_table(6, 8, 3)),
                                                              (event_list(EV_LARGE), glyph_table(9, 12, 3, 1))]),
                     dict(lang='la', palette=DEMON, variants=[(event_list(EV_COLOURS), glyph_table(8, 11, 3, 3))]),
                     dict(lang='por', forced_track=True, variants=[(event_list(EV_FORCED), glyph_table(7, 10, 2, 2))])])
    return one, two


def fix_header_crc(b):
    b[60:64] = struct.pack('<I', zlib.crc32(bytes(b[:60])) & 0xFFFFFFFF)


def md5(path):
    return hashlib.md5(open(path, 'rb').read()).hexdigest()


def main():
    os.makedirs(OUT, exist_ok=True)
    tmp = tempfile.mkdtemp()
    one, two = subtitle_areas()
    sp, sp2 = os.path.join(tmp, 's1.bin'), os.path.join(tmp, 's2.bin')
    open(sp, 'wb').write(one)
    open(sp2, 'wb').write(two)
    v = lambda n: os.path.join(TVC, n)
    a = lambda n: os.path.join(TAC, n)
    pos = [
        ('p01_ip_24_crc', ['v01_dialog_ip.tvc', '-L', '2', '-a', a('t02_film_action_96k.tac'), '-T', '-c']),
        ('p02_b2_23976_crc', ['v06_dialog_b2_tree.tvc', '-L', '2', '-r', '24000/1001', '-a', a('t02_film_action_96k.tac'), '-T', '-c']),
        ('p03_b3_gop7_2997', ['v07_anime_b3_gop7.tvc', '-L', '2', '-r', '30000/1001', '-a', a('t03_film_dialog_128k.tac'), '-T']),
        ('p04_slices_25_mono', ['v16_dialog_l2_3slices.tvc', '-r', '25/1', '-a', a('t04_tv_mono_64k.tac'), '-T', '-c']),
        ('p05_l1_video_only', ['v19_l1_crop_slices.tvc']),
        ('p06_crop_b2_subs', ['v09_odd_crop_b2.tvc', '-L', '1', '-a', a('t01_synth_stereo_96k.tac'), '-T', '-c', '-s', sp]),
        ('p07_long_audio_tail', ['v07_anime_b3_gop7.tvc', '-L', '2', '-a', a('t05_synth_lr_dense_clicks.tac')]),
        ('p09_seek_at_reset', ['v01_dialog_ip.tvc', '-L', '2', '-r', '375/128', '-a', a('t02_film_action_96k.tac'), '-T']),
        ('p08_l3_2slices_30', ['v17_bw_l3_2slices.tvc', '-r', '30/1', '-a', a('t06_synth_ms_all_320k.tac'), '-T', '-s', sp2]),
    ]
    lines = []
    for name, args in pos:
        args = [v(args[0])] + args[1:]
        f = os.path.join(OUT, name + '.tnv3')
        run(['mux'] + args + ['-o', f])
        has_a, has_s = '-a' in args, '-s' in args
        outs = [os.path.join(tmp, name + e) for e in ('.tvc', '.tac', '.subs')]
        run(['demux', f] + outs)
        parts = ['tvc=' + md5(outs[0])]
        if has_a:
            parts.append('tac=' + md5(outs[1]))
        if has_s:
            parts += ['subs=' + md5(outs[2])]
        lines.append('%s.tnv3 valid md5=%s %s' % (name, md5(f), ' '.join(parts)))
        print(lines[-1])
        print('   ', run(['info', f]).stdout.strip().replace('\n', '\n    '))

    # negative vectors: one broken rule each, built from p02 (crc on, B pictures) or p03 (crc off, several keys)
    base = bytearray(open(os.path.join(OUT, 'p02_b2_23976_crc.tnv3'), 'rb').read())
    base3 = bytearray(open(os.path.join(OUT, 'p03_b3_gop7_2997.tnv3'), 'rb').read())
    idx3 = struct.unpack_from('<I', base3, 40)[0]
    nf3 = struct.unpack_from('<I', base3, 28)[0]
    neg = []

    b = bytearray(base); b[61] ^= 1; neg.append(('n01_header_crc', b))
    b = bytearray(base); b[6] |= 8; fix_header_crc(b); neg.append(('n02_reserved_flag', b))
    b = bytearray(base); b[64 + 16 + 40] ^= 0x10; neg.append(('n03_chunk_crc', b))
    b = bytearray(base3); b[64 + 10] = 0; neg.append(('n04_key_flag', b))   # chunk 0 not marked key (crc off)
    b = bytearray(base3); e = idx3 + 16 * 1; struct.pack_into('<I', b, e + 4, struct.unpack_from('<I', b, e + 4)[0] + 1); neg.append(('n05_index_pts', b))
    b = bytearray(base3)
    keys = [n for n in range(nf3) if struct.unpack_from('<I', b, idx3 + 16 * n + 12)[0] & 1]
    e = idx3 + 16 * keys[-1] + 12
    struct.pack_into('<I', b, e, struct.unpack_from('<I', b, e)[0] ^ (1 << 8)); neg.append(('n06_seek_tac', b))
    b = bytearray(base) + b'\0'; neg.append(('n07_trailing_byte', b))
    b = bytearray(base); struct.pack_into('<I', b, 36, struct.unpack_from('<I', b, 36)[0] + 1); fix_header_crc(b); neg.append(('n08_tac_frames', b))
    for name, b in neg:
        open(os.path.join(OUT, name + '.tnv3'), 'wb').write(b)
    # built by the muxer with its self-check off
    mneg = [
        ('n09_audio_late', ['v06_dialog_b2_tree.tvc', '-L', '2', '-a', a('t02_film_action_96k.tac'), '-T', '-A', '-1']),
        ('n10_audio_early', ['v06_dialog_b2_tree.tvc', '-L', '2', '-a', a('t02_film_action_96k.tac'), '-T', '-A', '5']),
        ('n11_level1_size', ['v01_dialog_ip.tvc', '-L', '1']),
        ('n12_level_rate', ['v19_l1_crop_slices.tvc', '-r', '31/1']),
        ('n15_level_rate_window', ['../../tvc_ref/vectors/n02_level1_rate.tvc', '-r', '7/1']),   # 7 pictures > 200 kB, 6 fit
    ]
    for name, args in mneg:
        run(['mux', v(args[0])] + args[1:] + ['-x', '-o', os.path.join(OUT, name + '.tnv3')])
    sneg = [   # subtitle areas, each breaking one rule of section 6
        ('n13_glyph_id', [dict(lang='und', variants=[(event_list([(0, 10, 1, 1, 0, 0, [3])]), glyph_table(6, 8, 3))])]),
        ('n17_variant_order', [dict(lang='eng', variants=[(event_list(EV_SMALL), glyph_table(6, 8, 3)), (event_list(EV_SMALL), glyph_table(7, 8, 3))])]),   # equal heights
        ('n18_event_flags', [dict(lang='eng', variants=[(event_list([(0, 10, 1, 1, 0, 4, [0])]), glyph_table(6, 8, 3))])]),
        ('n20_reserved_code', [dict(lang='eng', variants=[(event_list([(0, 10, 1, 1, 0, 0, [0, 0xFFF0])]), glyph_table(6, 8, 3))])]),
        ('n21_colour_index', [dict(lang='eng', variants=[(event_list([(0, 10, 1, 1, 16, 0, [0])]), glyph_table(6, 8, 3))])]),
        ('n19_two_defaults', [dict(lang='eng', default=True, variants=[(event_list(EV_SMALL), glyph_table(6, 8, 3))]),
                              dict(lang='por', default=True, variants=[(event_list(EV_SMALL), glyph_table(6, 8, 3))])]),
    ]
    for name, tracks in sneg:
        bsp = os.path.join(tmp, name + '.bin'); open(bsp, 'wb').write(pack_area(tracks))
        run(['mux', v('v09_odd_crop_b2.tvc'), '-L', '1', '-s', bsp, '-x', '-o', os.path.join(OUT, name + '.tnv3')])
    b = bytearray(base); b[64 + 8] = 0xFF; neg.append(('n14_chunk_len', b))   # n_audio of chunk 0 inflated
    open(os.path.join(OUT, 'n14_chunk_len.tnv3'), 'wb').write(b)

    # n16: a level-1 stream that satisfies TVC's picture rate (6 intra pictures, about 193 kB) plus an untrimmed
    # 224 kbit/s audio stream whose tail lands in the last chunk: the chunks of one second exceed 265,536 bytes
    src = open(os.path.join(TVC, 'n02_level1_rate.tvc'), 'rb').read()
    pics, p = [], 20
    while p < len(src):
        l = struct.unpack_from('<I', src, p)[0]; pics.append(src[p + 4:p + 4 + l]); p += 4 + l
    keep = sorted(sorted(range(len(pics)), key=lambda i: -len(pics[i]))[:6])
    tv = bytearray(src[:20]); struct.pack_into('<I', tv, 12, len(keep))
    for i in keep:
        tv += struct.pack('<I', len(pics[i])) + pics[i]
    tvp = os.path.join(tmp, 'heavy.tvc'); open(tvp, 'wb').write(tv)
    run(['mux', tvp, '-a', a('t07_stress_fullscale_hi.tac'), '-x', '-o', os.path.join(OUT, 'n16_chunk_rate.tnv3')])

    for name in sorted(n for n in os.listdir(OUT) if n.startswith('n') and n.endswith('.tnv3')):
        r = run(['check', os.path.join(OUT, name)], ok=(3,))
        lines.append('%s invalid md5=%s  # C: %s' % (name, md5(os.path.join(OUT, name)), r.stderr.strip()))
        print(lines[-1])
    open(os.path.join(OUT, 'expected.txt'), 'w').write('\n'.join(lines) + '\n')


if __name__ == '__main__':
    main()
