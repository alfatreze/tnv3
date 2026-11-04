#!/usr/bin/env python3
"""Tests for the TST authoring and rendering tools (TNV3 container draft 1.3, subtitle area).
Run from anywhere: python3 tools/tst_ref/test_tst.py. Needs Pillow and the built C tool tools/tnv3_ref/tnv3."""
import os
import struct
import subprocess
import sys
import tempfile

from PIL import Image, ImageChops

HERE = os.path.dirname(os.path.abspath(__file__))
REF = os.path.join(HERE, '..', 'tnv3_ref')
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REF, 'py'))
import tst_build as tb          # noqa: E402
import tst_render as tr         # noqa: E402
import tst_area                 # noqa: E402
import json                     # noqa: E402
import io                       # noqa: E402
import tnv3_demux               # noqa: E402

TNV3 = os.path.join(REF, 'tnv3')
VEC = os.path.join(HERE, '..', 'tvc_ref', 'vectors')
FONT = os.path.join(HERE, 'fonts', 'Roboto-Regular.ttf')
ITAL = os.path.join(HERE, 'fonts', 'Roboto-Italic.ttf')
DEMON = os.path.join(HERE, 'fonts', 'UnifrakturCook-Bold.ttf')
MULTI = ['Roboto=' + FONT, 'Roboto:italic=' + ITAL, 'Demon=' + DEMON + '@1.15']
SAMPLES = os.path.join(HERE, 'samples')
SIZES = [(16, 24), (20, 28), (26, 36)]
fails = []


def check(cond, msg):
    if not cond:
        fails.append(msg)
        print('FAIL:', msg)
    else:
        print('ok  :', msg)


def run(args):
    return subprocess.run([TNV3] + args, capture_output=True, text=True)


def sample(n):
    return os.path.join(SAMPLES, n)


def tracks_spec():
    return [dict(path=sample('sample.srt'), lang='eng', default=True),
            dict(path=sample('sample_sdh.srt'), lang='eng'),
            dict(path=sample('sample.ass'), lang='por'),
            dict(path=sample('sample_forced.srt'), lang='eng', forced=True)]


def mux_area(tmp, tvc, area, name, extra=()):
    ap, op = os.path.join(tmp, name + '.area'), os.path.join(tmp, name + '.tnv3')
    with open(ap, 'wb') as f:
        f.write(area)
    r = run(['mux', os.path.join(VEC, tvc), '-s', ap, '-o', op] + list(extra))
    return r, op


def differs(a, b):
    return ImageChops.difference(a, b).getbbox() is not None


def main():
    check(os.path.exists(TNV3), 'C tool built')
    with tempfile.TemporaryDirectory() as tmp:
        # ---- multi-track, multi-size build, mux, check, parse, round trip
        area, info = tb.build_area(tracks_spec(), FONT, SIZES, (800, 334))
        check(len(info['tracks']) == 4 and all(len(t['variants']) == 3 for t in info['tracks']), '4 tracks x 3 sizes built')
        r, out = mux_area(tmp, 'v16_dialog_l2_3slices.tvc', area, 'multi')
        check(r.returncode == 0, 'tnv3 mux -s AREA (%s)' % r.stderr.strip())
        r = run(['check', out])
        check(r.returncode == 0, 'tnv3 check (%s)' % r.stderr.strip())
        with open(out, 'rb') as f:
            parsed = tnv3_demux.parse(f.read())
        check(parsed['subs'] == area, 'demuxed area equals built (%d B)' % len(area))
        check(tst_area.pack_area(tst_area.unpack_area(area)) == area, 'pack_area(unpack_area(area)) round trip')
        r = run(['demux', out, os.path.join(tmp, 'o.tvc'), os.path.join(tmp, 'o.tac'), os.path.join(tmp, 'o.area')])
        check(r.returncode == 0 and open(os.path.join(tmp, 'o.area'), 'rb').read() == area, 'tnv3 demux writes the same area')
        trk = tst_area.unpack_area(area)
        check([(t['lang'], t['default'], t['forced_track']) for t in trk] ==
              [('eng', True, False), ('eng', False, False), ('por', False, False), ('eng', False, True)],
              'track languages and flags')
        check(all([(v[1][2], v[1][3]) for v in t['variants']] == SIZES for t in trk), 'variant cell sizes 16x24, 20x28, 26x36')
        # per-size layout: the same text wraps differently
        ev = [tr.parse_variant(*v)[0] for v in trk[0]['variants']]
        check(len({len(e) for e in ev}) == 3, 'layout differs per size (records %s)' % [len(e) for e in ev])
        check(all(e == sorted(e, key=lambda r_: r_[0]) for e in ev), 'events sorted by start in every variant')
        # SDH and forced flags
        for v in range(3):
            e_sdh = tr.parse_variant(*trk[1]['variants'][v])[0]
            cnt = sum(1 for e in e_sdh if e[5] & 2)
            check(cnt == 4 and not any(e[5] & 1 for e in e_sdh), 'sdh track variant %d: 4 sdh records, none forced' % v)
            e_f = tr.parse_variant(*trk[3]['variants'][v])[0]
            check(all(e[5] == 1 for e in e_f) and len(e_f) == 2, 'forced track variant %d: every event forced, no sdh' % v)
            e_n = tr.parse_variant(*trk[0]['variants'][v])[0]
            check(all(e[5] == 0 for e in e_n), 'normal track variant %d: no flags' % v)
        # mixed line: "- [phone rings]" sdh, "- Hello?" not
        e5 = [e for e in tr.parse_variant(*trk[1]['variants'][1])[0] if 11 * 48000 <= e[0] < 13 * 48000]
        check(sorted(e[5] for e in e5) == [0, 2], 'mixed event: the bracketed line is sdh, the dialogue line is not')
        for s, want in (('[door slams]', True), ('(Marco)', True), ('(Marco) Come in.', False), ('♪ ♫', True),
                        ('[a] [b]', True), ('- [phone]', True), ('Hello [x]', False), ('', False), ('[a] b [c]', False)):
            check(tb.is_sdh_text(s) == want, 'is_sdh_text(%r) = %s' % (s, want))
        a2, _ = tb.build_area(tracks_spec()[1:2], FONT, [(20, 28)], (800, 334), sdh=False)
        check(not any(e[5] for e in tr.parse_variant(*tst_area.unpack_area(a2)[0]['variants'][0])[0]), '--no-sdh sets no flag')

        # ---- placement per size: bar at 800x334, over the picture at 800x600
        pic_bot = (720 - 334) // 2 + 334
        for v in range(3):
            evs, glyphs, cw, ch = tr.parse_variant(*trk[0]['variants'][v])
            boxes = [tr.event_box(e, glyphs, cw, ch) for e in evs]
            reg = info['tracks'][0]['variants'][v]['region']
            if reg == 'bar':
                ok = all(b[1] >= pic_bot and b[3] <= 720 for b in boxes)
            else:
                ok = all(b[1] >= 193 and b[3] <= pic_bot for b in boxes)
            check(ok, 'variant %d (%s): every event, including stacked ones, inside its region' % (v, reg))
        check([vi['region'] for vi in info['tracks'][0]['variants']] == ['bar', 'bar', 'picture'],
              'region decided per size from the tallest simultaneous stack (bar, bar, picture)')
        a600, i600 = tb.build_area(tracks_spec()[:1], FONT, SIZES, (800, 600))
        check(all(vi['region'] == 'picture' for vi in i600['tracks'][0]['variants']), '800x600 goes over the picture')
        r, o600 = mux_area(tmp, 'v17_bw_l3_2slices.tvc', a600, 'pic')
        check(r.returncode == 0 and run(['check', o600]).returncode == 0, '800x600 area muxes and checks (level 3 vector)')
        evs, glyphs, cw, ch = tr.parse_variant(*tst_area.unpack_area(a600)[0]['variants'][2])
        check(all(60 <= b[1] and b[3] <= 660 for b in (tr.event_box(e, glyphs, cw, ch) for e in evs)),
              'over-picture events stay inside the picture (largest size)')
        # fps snapping and determinism
        af, _ = tb.build_area(tracks_spec()[:1], FONT, [(20, 28)], (800, 334), fps=(24000, 1001))
        evs = tr.parse_variant(*tst_area.unpack_area(af)[0]['variants'][0])[0]
        check(all(e[0] % 2002 == 0 and e[1] % 2002 == 0 for e in evs), 'fps 24000/1001: times are multiples of 2002 samples')
        check(tb.build_area(tracks_spec(), FONT, SIZES, (800, 334))[0] == area, 'build is deterministic')
        check(evs[0][0] > 0 and tr.parse_variant(*tst_area.unpack_area(tb.build_area(
            tracks_spec()[:1], FONT, [(20, 28)], (800, 334))[0])[0]['variants'][0])[0][0][0:2] == (500 * 48, 2500 * 48),
            'times are ms * 48 samples without --fps')

        # ---- parser units
        base = dict(fam='default', bold=False, italic=False, rgb=None)
        ff = lambda n: n.lower()        # noqa: E731
        segs = tb.parse_markup('a <i>b</i> {\\i1}c{\\i0} <b>d</b> {\\p1}m 0 0{\\p0}e', True, base, ff)
        F_, T_ = False, True
        check(segs == [('a ', 'default', F_, F_, None), ('b', 'default', F_, T_, None), (' ', 'default', F_, F_, None),
                       ('c', 'default', F_, T_, None), (' ', 'default', F_, F_, None), ('d', 'default', T_, F_, None),
                       (' ', 'default', F_, F_, None), ('e', 'default', F_, F_, None)], 'markup: italics, bold, tags, drawing')
        segs = tb.parse_markup('{\\fnDemon\\c&H0000FF&}x{\\r}y{\\1c&H00FF00&\\b1}z', True, base, ff)
        check([(t, f, b, c) for t, f, b, i, c in segs] == [('x', 'demon', F_, (255, 0, 0)), ('y', 'default', F_, None),
                                                          ('z', 'default', T_, (0, 255, 0))],
              'ASS overrides: \\fn, \\c (BGR), \\r, \\1c, \\b')
        segs = tb.parse_markup('<font color="#40a0ff">a</font>b<font face="Demon" color=red>c</font>', False, base, ff)
        check([(t, f, c) for t, f, b, i, c in segs] == [('a', 'default', (64, 160, 255)), ('b', 'default', None),
                                                       ('c', 'demon', (255, 0, 0))], 'SRT <font color face>')
        st0 = ('d', False, False)
        check(tb.to_lines([('  a   b \n\n c ', 'd', False, False, None)]) ==
              [[('a', st0, None), (' ', st0, None), ('b', st0, None)], [('c', st0, None)]],
              'whitespace collapsed, empty lines dropped')
        check(tb.ass_rgb('&H00FF8000') == (0, 128, 255) and tb.ass_rgb('&HFF&') == (255, 0, 0), 'ASS colour is BGR')
        check(tb.parse_track_spec('x.srt') == dict(path='x.srt', lang='und', default=False, forced=False), 'track spec: path only')
        check(tb.parse_track_spec('x.srt:por:default:forced') == dict(path='x.srt', lang='por', default=True, forced=True),
              'track spec: lang default forced')
        check(tb.parse_track_spec('x.srt:forced')['forced'], 'track spec: flag without language')

        # ---- refusals: malformed variants (validate_blocks) and malformed areas (tnv3 check + Python demuxer)
        sub, gl = trk[0]['variants'][1]
        gc, cw, ch = struct.unpack('<HBB', gl[:4])

        def mutate(off, fmt, val):
            b = bytearray(sub)
            struct.pack_into(fmt, b, off, val)
            return bytes(b)
        first = 4
        n0 = struct.unpack_from('<H', sub, first + 14)[0]
        cases = {
            'glyph id out of range': (mutate(first + 16, '<H', gc), gl),
            'end <= start': (mutate(first + 4, '<I', struct.unpack_from('<I', sub, first)[0]), gl),
            'x >= 800': (mutate(first + 8, '<H', 800), gl),
            'y >= 720': (mutate(first + 10, '<H', 720), gl),
            'colour 16': (mutate(first + 12, '<B', 16), gl),
            'ref 0xFFF0': (mutate(first + 16, '<H', 0xFFF0), gl),
            'ref 0xFFFE': (mutate(first + 16, '<H', 0xFFFE), gl),
            'flag bit 2': (mutate(first + 13, '<B', 4), gl),
            'trailing byte': (sub + b'\0', gl),
            'unsorted': (mutate(first + 16 + 2 * n0, '<I', 0), gl),
            'advance 0': (sub, gl[:4] + b'\0' + gl[5:]),
            'advance > cell': (sub, gl[:4] + bytes([cw + 1]) + gl[5:]),
            'glyph table truncated': (sub, gl[:-1]),
        }
        for name, (s, g) in cases.items():
            try:
                tb.validate_blocks(s, g)
                check(False, 'validate_blocks rejects: ' + name)
            except tb.TstError:
                check(True, 'validate_blocks rejects: ' + name)
            bad = tst_area.pack_area([dict(lang='eng', variants=[(s, g)])])
            r, op = mux_area(tmp, 'v16_dialog_l2_3slices.tvc', bad, 'bad', ['-x'])
            rejected = r.returncode != 0 or run(['check', op]).returncode == 3
            if r.returncode == 0:
                with open(op, 'rb') as f:
                    try:
                        tnv3_demux.parse(f.read())
                        rejected = False
                    except tnv3_demux.TnvError:
                        pass
            check(rejected, 'tnv3 (C check and Python demuxer) rejects: ' + name)
        # padding bits need cell_w not a multiple of 4
        sub6, gl6 = trk[0]['variants'][0]          # 16x24: multiple of 4, so build a 14x28 table
        fs = tb.FontSet.make(FONT)
        s14, g14, _ = tb.build_variant(tb.load_track(sample('sample.srt'), fs), fs, (14, 28), (800, 334))
        bad = bytearray(g14)
        bad[4 + 1 + 4 - 1] |= 0xF0
        try:
            tb.validate_blocks(s14, bytes(bad))
            check(False, 'validate_blocks rejects padding bits')
        except tb.TstError:
            check(True, 'validate_blocks rejects padding bits')
        # area-level refusals
        good = tst_area.pack_area([dict(lang='eng', variants=[trk[0]['variants'][0], trk[0]['variants'][1]])])
        tnv3_demux.check_area(good)
        ab = bytearray(good)
        ab[4 + 56 + 20 + 2] = 1                                  # reserved field of variant 1
        area_bad = {
            'variant reserved': bytes(ab),
            'area trailing byte': good + b'\0',
            'language upper case': good[:4] + b'ENG\0' + good[8:],
            'decreasing cell height': tst_area.pack_area([dict(lang='eng', variants=[trk[0]['variants'][1], trk[0]['variants'][0]])]),
            'two default tracks': tst_area.pack_area([dict(lang='eng', default=True, variants=[trk[0]['variants'][0]]),
                                                      dict(lang='por', default=True, variants=[trk[2]['variants'][0]])]),
        }
        for name, ar in area_bad.items():
            try:
                tnv3_demux.check_area(ar)
                check(False, 'check_area rejects: ' + name)
            except tnv3_demux.TnvError:
                check(True, 'check_area rejects: ' + name)
            r, op = mux_area(tmp, 'v16_dialog_l2_3slices.tvc', ar, 'bad2', ['-x'])
            check(r.returncode != 0 or run(['check', op]).returncode == 3, 'tnv3 check rejects: ' + name)
        # builder refusals
        one = tracks_spec()[:1]
        for name, fn in (('video wider than grid', lambda: tb.build_area(one, FONT, [(20, 28)], (900, 334))),
                         ('cell too small for the font', lambda: tb.build_area(one, FONT, [(2, 2)], (800, 334))),
                         ('sizes not increasing', lambda: tb.build_area(one, FONT, [(20, 28), (16, 24)], (800, 334))),
                         ('two defaults', lambda: tb.build_area([dict(one[0], default=True), dict(one[0], default=True)],
                                                                FONT, [(20, 28)], (800, 334))),
                         ('bad language', lambda: tb.build_area([dict(one[0], lang='E1')], FONT, [(20, 28)], (800, 334))),
                         ('no tracks', lambda: tb.build_area([], FONT, [(20, 28)], (800, 334)))):
            try:
                fn()
                check(False, 'builder refuses: ' + name)
            except tb.TstError:
                check(True, 'builder refuses: ' + name)

        # ---- CLI build
        ap_, out_ = os.path.join(tmp, 'cli.area'), os.path.join(tmp, 'cli.tnv3')
        r = subprocess.run([sys.executable, os.path.join(HERE, 'tst_build.py'), '--track', sample('sample.srt') + ':eng:default',
                            '--track', sample('sample_forced.srt') + ':eng:forced', '--sizes', '16x24,20x28,26x36',
                            '--font', FONT, '--video', '800x334', '--fps', '24000/1001', '-o', ap_],
                           capture_output=True, text=True)
        check(r.returncode == 0, 'tst_build CLI (%s)' % r.stderr.strip())
        r2 = run(['mux', os.path.join(VEC, 'v16_dialog_l2_3slices.tvc'), '-s', ap_, '-o', out_])
        check(r2.returncode == 0 and run(['check', out_]).returncode == 0, 'CLI area muxes and checks')

        # ---- renderer: semantics
        base = tr.background((800, 334))
        P = tr.resolve()
        T = lambda s: int(s * 48000)         # noqa: E731

        def rnd(p, t, video=(800, 334), tracks=trk):
            return tr.render(tracks, p, video, t)[0]
        check(not differs(rnd(P, T(0.2)), base), 'no active event: picture only')
        plain = rnd(P, T(0.7))
        check(differs(plain, base), 'active event is drawn')
        # text at full strength: a coverage-3 pixel has exactly the palette colour
        evs, glyphs, cw, ch = tr.parse_variant(*trk[0]['variants'][1])
        solid = []
        for g, gx, gy, _c in tr.place(evs[0], glyphs, ch):
            for r_, row in enumerate(glyphs[g][1]):
                for c_, av in enumerate(row):
                    if av == 3:
                        solid.append((gx + c_, gy + r_))
        check(solid and all(plain.getpixel(q) == (255, 255, 255) for q in solid), 'coverage 3 = exact palette colour (no opacity)')
        col = tr.render(trk, tr.resolve(overrides=dict(text_color='FF0000')), (800, 334), T(0.7))[0]
        check(all(col.getpixel(q) == (255, 0, 0) for q in solid), '--text-color replaces palette entry 0')
        # italic without an italic face: the fallback colour is a palette entry, drawn as authored
        check(trk[0]['palette'][0] == (255, 255, 255) and (255, 224, 64) in trk[0]['palette'][1:4],
              'palette: entry 0 white, italic fallback colour authored into the palette')
        it = tr.render(trk, tr.resolve(), (800, 334), T(12))[0]
        check(any(px == (255, 224, 64) for px in it.get_flattened_data()), 'italic fallback colour drawn')
        # outline: disc dilation geometry
        d1 = tr.dilate({(10, 10): 3}, 1)
        d2 = tr.dilate({(10, 10): 3}, 2)
        d4 = tr.dilate({(10, 10): 2}, 4)
        check(len(d1) == 5 and len(d2) == 13 and len(d4) == 49 and set(d4.values()) == {2}, 'disc dilation sizes 5 / 13 / 49 for w = 1 / 2 / 4')
        check(tr.dilate({(0, 0): 3}, 2).get((-1, 0)) is None and (2, 0) in tr.dilate({(0, 0): 3}, 2), 'dilation clipped to the grid')
        o0 = rnd(tr.resolve(overrides=dict(outline_width=0)), T(0.7))
        o2 = rnd(tr.resolve(overrides=dict(outline_width=2, outline_color='FF00FF')), T(0.7))
        o4 = rnd(tr.resolve(overrides=dict(outline_width=4, outline_color='FF00FF')), T(0.7))
        dark = lambda im: sum(1 for px in im.get_flattened_data() if px == (0, 0, 0))   # noqa: E731
        check(o0 == plain, 'outline width 0 = plain text')
        check(differs(o2, o0) and any(px == (255, 0, 255) for px in o4.get_flattened_data()), 'outline width 2 and 4 draw, colour applies')
        # a text pixel stays on top of its outline
        check(all(o2.getpixel(q) == (255, 255, 255) for q in solid), 'text drawn over the outline')
        # box: solid colour inside the padded rectangle, nothing outside
        pb = tr.resolve(overrides=dict(box_color='0000FF', box_pad=6))
        bimg, act, _ = tr.render(trk, pb, (800, 334), T(0.7))
        bx = tr.event_box(act[0], glyphs, cw, ch)
        check(bimg.getpixel((bx[0] - 6, bx[1] - 6)) == (0, 0, 255) and bimg.getpixel((bx[2] + 5, bx[3] + 5)) == (0, 0, 255),
              'box covers the event rectangle grown by the padding (solid)')
        check(bimg.getpixel((bx[0] - 7, bx[1] - 7)) == base.getpixel((bx[0] - 7, bx[1] - 7)), 'nothing outside the padded box')
        # vertical offset
        voff = rnd(tr.resolve(overrides=dict(voffset=-30)), T(0.7))
        shifted = ImageChops.offset(plain, 0, -30)
        top = 720 - 30 - 24 - 2 * 30
        check(voff.crop((0, top, 800, 720 - 40)).tobytes() == shifted.crop((0, top, 800, 720 - 40)).tobytes(),
              'vertical offset moves every event by the given rows')
        # delay (positive = later): event 0.5-2.5 s; at t = 0.7 s with delay 400 ms p = 0.3 s: nothing
        check(not differs(rnd(tr.resolve(overrides=dict(delay=400)), T(0.7)), base), 'delay +400 ms hides an event that has not started')
        check(rnd(tr.resolve(overrides=dict(delay=-400)), T(0.2)) == rnd(P, T(0.6)), 'delay -400 ms shows it 400 ms early')
        # track and size selection
        por = rnd(tr.resolve(overrides=dict(track='por')), T(1.5))
        eng = rnd(tr.resolve(overrides=dict(track='eng')), T(1.5))
        check(differs(por, eng), 'track selection by language')
        check(eng == rnd(tr.resolve(overrides=dict(track='0')), T(1.5)), 'track selection by index; default track is the first eng')
        sm, lg = (rnd(tr.resolve(overrides=dict(size=s)), T(0.7)) for s in (0, -1))
        check(differs(sm, lg) and rnd(tr.resolve(overrides=dict(size='16x24')), T(0.7)) == sm, 'size by index, negative index and WxH')
        for bad_p in (dict(track='xxx'), dict(size='99x99'), dict(size=7), dict(outline_width=5), dict(box_pad=17)):
            try:
                tr.render(trk, tr.resolve(overrides=bad_p), (800, 334), T(0.7))
                check(False, 'renderer refuses %s' % bad_p)
            except tr.RenderError:
                check(True, 'renderer refuses %s' % bad_p)
        # subtitles off: only forced events, taken from the forced track of the same language
        offp = tr.resolve(overrides=dict(subs='off'))
        check(not differs(rnd(offp, T(0.7)), base), 'subs off: ordinary event hidden')
        check(differs(rnd(offp, T(2.0)), base), 'subs off: forced event shown (forced track, same language)')
        check(not differs(rnd(tr.resolve(overrides=dict(subs='off', track='por')), T(2.0)), base),
              'subs off with a language that has no forced track: nothing')
        check(rnd(tr.resolve(overrides=dict(track='off')), T(2.0)) == rnd(offp, T(2.0)), "track 'off' equals subs off")
        # hide-sdh
        shown = rnd(tr.resolve(overrides=dict(track='1')), T(1.5))
        hidden = rnd(tr.resolve(overrides=dict(track='1', hide_sdh=True)), T(1.5))
        check(differs(shown, base) and not differs(hidden, base), 'hide-sdh hides the bracketed event')
        both = rnd(tr.resolve(overrides=dict(track='1', hide_sdh=True)), T(11.5))
        check(differs(both, base), 'hide-sdh keeps the dialogue line of a mixed event')
        # unit: forced-flagged event inside an ordinary track while subs are off
        ev_bytes, gl_bytes = trk[0]['variants'][1]
        eb = bytearray(ev_bytes)
        struct.pack_into('<B', eb, 4 + 13, 1)                   # first event flagged forced
        ft = [dict(lang='eng', default=True, forced_track=False, palette=tst_area.DEFAULT_PALETTE, variants=[(bytes(eb), gl_bytes)])]
        check(differs(tr.render(ft, offp, (800, 334), T(0.7))[0], base) and
              not differs(tr.render(ft, offp, (800, 334), T(7.5))[0], base),
              'subs off: a forced-flagged event of the selected track still shows')
        # panel and window size
        check(rnd(P, T(0.7)).size == (800, 720) and tr.to_panel(plain).size == (1600, 1440), 'window 800x720, panel 1600x1440')

        # ---- draft 1.4: palette, colour switches, several fonts, authored colours
        S0, S1 = tb.SWITCH, tb.SWITCH + 15
        pal_custom = [(10, 20, 30)] + [(i, 2 * i, 3 * i) for i in range(1, 16)]
        pa = tst_area.pack_area([dict(lang='eng', palette=pal_custom, variants=[trk[0]['variants'][0]])])
        check(tst_area.unpack_area(pa)[0]['palette'] == pal_custom, 'palette round trip through pack_area / unpack_area')
        tnv3_demux.check_area(pa)
        logs = []
        mf = tb.FontSet.make(MULTI)
        darea, dinfo = tb.build_area([dict(path=sample('demon.ass'), lang='eng', default=True)], mf, SIZES, (800, 334),
                                     log=logs.append)
        dt = tst_area.unpack_area(darea)[0]
        check(dt['palette'][:5] == [(255, 255, 255), (255, 0, 0), (0, 255, 0), (0, 128, 255), (255, 255, 0)] and
              all(c == (160, 160, 160) for c in dt['palette'][5:]),
              'palette: entry 0 white, authored colours in order of first use, unused entries grey')
        r, od = mux_area(tmp, 'v16_dialog_l2_3slices.tvc', darea, 'demon')
        check(r.returncode == 0 and run(['check', od]).returncode == 0, 'demon area muxes and passes tnv3 check')
        dev, dgl, dcw, dch = tr.parse_variant(*dt['variants'][1])
        sign = [e for e in dev if 6 * 48000 <= e[0] < 7 * 48000]
        check(len(sign) == 1 and sum(1 for g in sign[0][6] if S0 <= g <= S1) == 2 and sign[0][4] == 0 and
              (S0 + 1) in sign[0][6] and S0 in sign[0][6], 'mixed-colour line is ONE event with two switches (red, back to 0)')
        glyphs_d = tr.parse_variant(*dt['variants'][1])[1]
        width = sum(glyphs_d[g][0] for g in sign[0][6] if g < S0)
        check(sign[0][2] == (800 - width) // 2, 'the mixed line is centred as a whole (x = %d)' % sign[0][2])
        mix = [e for e in dev if 14.5 * 48000 <= e[0] < 15 * 48000]
        check(len(mix) == 1 and (S0 + 1) in mix[0][6], 'font + colour run inside a line stays one event')
        demon_ev = [e for e in dev if 3 * 48000 <= e[0] < 4 * 48000]
        check(len(demon_ev) == 2 and all(e[4] == 1 and not any(S0 <= g <= S1 for g in e[6]) for e in demon_ev),
              'demon style: event colour 1 (red), no switch needed')
        # several fonts: same character, distinct glyph ids
        ltr = tb.load_track(sample('demon.ass'), mf)
        pairs = sorted({(it[1], it[0]) for _, _, ls in ltr['events'] for ln, _ in ls for it in ln})
        chars = {c for _, c in pairs}
        check(len({f for f, _ in pairs}) == 3 and len(pairs) > len(chars), 'three faces used; some characters exist in several faces')
        check(dinfo['tracks'][0]['variants'][1]['glyphs'] == len(pairs),
              'glyph table = one glyph per (face, character): %d glyphs, %d characters' % (len(pairs), len(chars)))
        c0 = [c for c in sorted(chars) if c != ' ' and sum(1 for f, cc in pairs if cc == c) > 1][0]
        ids_c0 = [i for i, (f, c) in enumerate(pairs) if c == c0]
        check(len(set(ids_c0)) >= 2 and glyphs_d[ids_c0[0]][1] != glyphs_d[ids_c0[1]][1],
              'character %r: distinct glyph ids %s with distinct bitmaps' % (c0, ids_c0))
        # italic font given: no fallback colour; without it the fallback colour appears
        wl0 = []
        a_it, _ = tb.build_area([dict(path=sample('demon.ass'))], 'Roboto=' + FONT, [(20, 28)], (800, 334), log=wl0.append)
        check((255, 224, 64) in tst_area.unpack_area(a_it)[0]['palette'], 'no italic face: italic fallback colour in the palette')
        check((255, 224, 64) not in dt['palette'], 'italic face given: no fallback colour')
        check(any('warning: font' in m and 'Demon' in m for m in wl0),
              'unknown font name falls back to the default font with a warning')
        # --font syntax
        fs = tb.FontSet()
        fs.add('x=' + FONT + '@1.1')
        fs.add('x:italic=' + ITAL)
        fs.add('Y:bold=' + DEMON)
        check(fs.default == 'x' and fs.faces[1][1] == 1.1 and fs.fam['x'][(False, True)] == 1 and fs.fam['y'][(True, False)] == 2,
              '--font syntax: name, style, scale (inherited by italic)')
        for name, spec, first in (('first font must be regular', 'a:italic=' + ITAL, True),
                                  ('unknown style', 'a:oblique=' + ITAL, False), ('missing file', 'nofile.ttf', False),
                                  ('scale out of range', 'a=' + FONT + '@9', False)):
            f2 = tb.FontSet()
            try:
                if not first:
                    f2.add('d=' + FONT)
                f2.add(spec)
                check(False, '--font refused: ' + name)
            except tb.TstError:
                check(True, '--font refused: ' + name)
        # speaker style sheet and SRT font tags
        sh = tb.load_styles(sample('demon_styles.json'))
        sr = tb.load_track(sample('demon.srt'), mf, sh)
        l0 = sr['events'][0][2][0][0]
        dface = mf.fam['demon'][(False, False)]
        check(''.join(it[0] for it in l0) == 'You should not have come here.' and sr['palette'][1] == (255, 32, 32) and
              all(it[2] == 1 and it[1] == dface for it in l0),
              'speaker prefix removed, line in the sheet font and colour (palette entry 1)')
        l1 = sr['events'][1][2][0][0]
        check(sr['palette'][2] == (64, 160, 255) and l1[0][2] == 2 and any(it[1] == dface for it in l1),
              'SRT <font color face>: colour authored, face used')
        # more than 15 authored colours: nearest existing entry with a warning
        many = os.path.join(tmp, 'many.srt')
        with open(many, 'w') as f:
            f.write('1\n00:00:01,000 --> 00:00:02,000\n' + ''.join(
                '<font color="#%02x%02x%02x">x</font>' % (i * 15, 255 - i * 10, 40 + i) for i in range(18)) + '\n')
        wl = []
        am, im = tb.build_area([dict(path=many)], FONT, [(20, 28)], (800, 334), log=wl.append)
        check(any('16 colours' in m for m in wl) and im['tracks'][0]['colours'] == 16,
              '>15 authored colours: warning, nearest entry used, 16 entries')
        # drawing rules with a hand-made glyph table
        S2, S3 = S0 + 2, S0 + 3
        gt = struct.pack('<HBB', 1, 4, 4) + bytes([2]) + bytes([0xFF] * 4)            # one glyph, advance 2, alpha 3
        pal3 = [(255, 255, 255), (255, 0, 0), (0, 255, 0), (0, 0, 255)] + [(0, 0, 0)] * 12

        def evb(items):
            b = struct.pack('<I', len(items))
            for st, en, x, y, col, ids in items:
                b += struct.pack('<IIHHBBH', st, en, x, y, col, 0, len(ids)) + b''.join(struct.pack('<H', i) for i in ids)
            return b
        evs_t = evb([(0, 480000, 100, 600, 1, [0]), (0, 480000, 100, 600, 2, [0]),
                     (0, 480000, 200, 600, 1, [0, S3, 0]), (0, 480000, 300, 600, 1, [0, S2, 0xFFFF, 0])])
        tarea = tst_area.pack_area([dict(lang='eng', palette=pal3, variants=[(evs_t, gt)])])
        tnv3_demux.check_area(tarea)
        timg = tr.render(tst_area.unpack_area(tarea), tr.resolve(), (800, 334), 0)[0]
        check(timg.getpixel((101, 601)) == (0, 255, 0), 'tie on equal alpha: the later event wins')
        check(timg.getpixel((200, 601)) == (255, 0, 0) and timg.getpixel((201, 601)) == (255, 0, 0) and
              timg.getpixel((202, 601)) == (0, 0, 255) and timg.getpixel((203, 601)) == (0, 0, 255),
              'switch inside an event: second glyph blue, pen advanced by 2; the later glyph wins the overlap column 2')
        pe, pg = tr.parse_variant(evs_t, gt)[0], tr.parse_variant(evs_t, gt)[1]
        check([(x, y, c) for _, x, y, c in tr.place(pe[2], pg, 4)] == [(200, 600, 1), (202, 600, 3)],
              'a switch does not move the pen (second glyph at x + advance)')
        check([(x, y, c) for _, x, y, c in tr.place(pe[3], pg, 4)] == [(300, 600, 1), (300, 604, 2)],
              'colour switch persists across a line break')
        check(timg.getpixel((300, 605)) == (0, 255, 0), 'second line drawn in the persisted colour')
        # override-all-colours and text colour
        dtracks = tst_area.unpack_area(darea)
        ov = tr.render(dtracks, tr.resolve(overrides=dict(override_colors=True, text_color='00FF00')), (800, 334), T(4))[0]
        nov = tr.render(dtracks, tr.resolve(), (800, 334), T(4))[0]
        check(any(px == (255, 0, 0) for px in nov.get_flattened_data()) and
              not any(px == (255, 0, 0) for px in ov.get_flattened_data()) and
              any(px == (0, 255, 0) for px in ov.get_flattened_data()), 'override-all-colours draws every entry in the text colour')
        px_ = list(tr.render(dtracks, tr.resolve(overrides=dict(text_color='00FF00')), (800, 334), T(6.5))[0].get_flattened_data())
        check((0, 255, 0) in px_ and (255, 0, 0) in px_, '--text-color replaces entry 0 only (red DANGER stays red)')
        check(not any(px == (255, 255, 255) for px in px_), 'no white left once entry 0 is replaced')

        # ---- renderer CLI: every preset, panel, preset override, sheet, .tnv3 input
        presets = tr.load_presets()
        check(len(presets) >= 6, '%d presets' % len(presets))
        for name in presets:
            png = os.path.join(tmp, name + '.png')
            r = subprocess.run([sys.executable, os.path.join(HERE, 'tst_render.py'), out, '--time', '2.2', '--preset', name,
                                '-o', png], capture_output=True, text=True)
            ok = r.returncode == 0 and os.path.exists(png) and Image.open(png).size == (800, 720)
            check(ok, 'preset %s renders (%s)' % (name, r.stderr.strip()))
        png = os.path.join(tmp, 'panel.png')
        r = subprocess.run([sys.executable, os.path.join(HERE, 'tst_render.py'), ap_, '--video', '800x334', '--time', '2.2',
                            '--preset', 'classic', '--outline-width', '0', '--panel', '-o', png], capture_output=True, text=True)
        check(r.returncode == 0 and Image.open(png).size == (1600, 1440), 'area file + --video, --panel (%s)' % r.stderr.strip())
        a_ = tr.render(trk, tr.resolve('classic', dict(outline_color='FF0000')), (800, 334), T(2.2))[0]
        b_ = tr.render(trk, tr.resolve('classic', dict(outline_width=0, outline_color='FF0000')), (800, 334), T(2.2))[0]
        check(differs(a_, b_), 'command line overrides the preset')
        sheet = os.path.join(tmp, 'sheet.png')
        r = subprocess.run([sys.executable, os.path.join(HERE, 'tst_render.py'), out, '--time', '2.2', '--sheet', sheet],
                           capture_output=True, text=True)
        check(r.returncode == 0 and Image.open(sheet).size[0] >= 1600, 'contact sheet over all presets (%s)' % r.stderr.strip())
        r = subprocess.run([sys.executable, os.path.join(HERE, 'tst_render.py'), ap_, '--time', '1', '-o', png],
                           capture_output=True, text=True)
        check(r.returncode == 2, 'area file without --video is refused')
        with open(os.path.join(tmp, 'junk.area'), 'wb') as f:
            f.write(b'\x01\0\0\0junk')
        r = subprocess.run([sys.executable, os.path.join(HERE, 'tst_render.py'), os.path.join(tmp, 'junk.area'), '--video',
                            '800x334', '--time', '1', '-o', png], capture_output=True, text=True)
        check(r.returncode == 2, 'invalid area refused by the renderer')

        # ---- demo script: multi --font CLI, styles.json CLI, sheet and panel
        dd = os.path.join(tmp, 'demo')
        r = subprocess.run([sys.executable, os.path.join(HERE, 'demo_demon.py'), dd], capture_output=True, text=True)
        check(r.returncode == 0 and os.path.exists(os.path.join(dd, 'sheet_demon.png')) and
              Image.open(os.path.join(dd, 'panel_demon.png')).size == (1600, 1440), 'demo_demon.py builds and renders (%s)' % r.stderr.strip()[-200:])
        sa = os.path.join(tmp, 'sty.area')
        r = subprocess.run([sys.executable, os.path.join(HERE, 'tst_build.py'), '--track', sample('demon.srt') + ':eng',
                            '--font', 'Roboto=' + FONT, '--font', 'Demon=' + DEMON, '--styles', sample('demon_styles.json'),
                            '--video', '800x334', '-o', sa], capture_output=True, text=True)
        check(r.returncode == 0 and tst_area.unpack_area(open(sa, 'rb').read())[0]['palette'][1] == (255, 32, 32),
              'tst_build CLI with --font NAME=FILE and --styles (%s)' % r.stderr.strip()[-200:])

    print('\n%d failure(s)' % len(fails))
    return 1 if fails else 0


if __name__ == '__main__':
    sys.exit(main())
