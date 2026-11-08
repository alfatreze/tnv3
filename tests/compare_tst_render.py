#!/usr/bin/env python3
"""Pixel comparison of our Rust subtitle reader/renderer with Tau V's reference renderer (tools/tst_ref/tst_render.py).

    python3 -I tests/compare_tst_render.py --tau-v ROOT --cli tau-video-cli/target/debug/tau-codecs-cli

Builds the same subtitle areas with Tau V's Python builder and with our native one (their sample.srt at three sizes and
demon.ass with three faces), then at every event start, middle, last sample and just after the end compares, for each
variant, the three layers (text coverage, palette index of the winning glyph, baked outline) of both renderers over the whole
800x720 grid. Needs Python 3 with Pillow and the C-free parts of Tau V's tools; read-only use of Tau V's files.
Exit 0 only if every frame is identical."""
import argparse, os, subprocess, sys, tempfile

GW, GH = 800, 720


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--tau-v', required=True)
    ap.add_argument('--cli', required=True)
    a = ap.parse_args()
    V = os.path.join(a.tau_v, 'tools', 'tst_ref')
    sys.path.insert(0, V)
    sys.path.insert(0, os.path.join(V, '..', 'tnv3_ref', 'py'))
    import tst_render as R
    from tst_area import unpack_area
    S, F = os.path.join(V, 'samples'), os.path.join(V, 'fonts')
    roboto, italic, fraktur = (os.path.join(F, n) for n in ('Roboto-Regular.ttf', 'Roboto-Italic.ttf', 'UnifrakturCook-Bold.ttf'))
    jobs = [
        ('srt', 3, ['--track', os.path.join(S, 'sample.srt'), '--sizes', '16x24,20x28,26x36', '--font', roboto, '--video', '800x334']),
        ('demon', 1, ['--track', os.path.join(S, 'demon.ass'), '--sizes', '20x28', '--font', roboto, '--font', 'default:italic=' + italic,
                      '--font', 'demon=' + fraktur, '--styles', os.path.join(S, 'demon_styles.json'), '--video', '800x334']),
    ]
    bad = checked = 0
    with tempfile.TemporaryDirectory() as tmp:
        for name, variants, args in jobs:
            py, rs = os.path.join(tmp, 'py_%s.bin' % name), os.path.join(tmp, 'rs_%s.bin' % name)
            subprocess.run([sys.executable, os.path.join(V, 'tst_build.py')] + args + ['-o', py], check=True, capture_output=True)
            subprocess.run([a.cli, 'build-subtitles', rs] + args, check=True, capture_output=True)
            for path in (py, rs):
                area = open(path, 'rb').read()
                trk = unpack_area(area)[0]
                for vi in range(variants):
                    ow = trk['outline_w'][vi]
                    planes = []
                    events, glyphs, cw, ch = R.parse_variant(*trk['variants'][vi], w=ow, planes=planes)
                    times = sorted({e[0] for e in events} | {(e[0] + e[1]) // 2 for e in events} | {e[1] - 1 for e in events} | {e[1] + 7 for e in events})
                    for t in times:
                        act = [e for e in events if e[0] <= t < e[1]]
                        cov = {}
                        for ev in act:
                            for g, x, y, colr in R.place(ev, glyphs, ch):
                                for r, row in enumerate(glyphs[g][1]):
                                    yy = y + r
                                    if not 0 <= yy < GH:
                                        continue
                                    for c, v in enumerate(row):
                                        xx = x + c
                                        if v and 0 <= xx < GW and cov.get((xx, yy), (0, 0))[0] <= v:
                                            cov[(xx, yy)] = (v, colr)
                        outline = R.baked_outline_layer(act, glyphs, ch, ow, planes) if ow else {}
                        layers, ppm = os.path.join(tmp, 'l.bin'), os.path.join(tmp, 'x.ppm')
                        for f in (layers, ppm):
                            if os.path.exists(f):
                                os.remove(f)
                        subprocess.run([a.cli, 'subtitles', path, '--variant', str(vi), '--render', str(t / 48000), ppm, '--layers', layers, '--json'],
                                       check=True, capture_output=True)
                        raw = open(layers, 'rb').read()
                        n = GW * GH
                        text, col, out = raw[:n], raw[n:2 * n], raw[2 * n:]
                        diff = 0
                        for y in range(GH):
                            for x in range(GW):
                                i = y * GW + x
                                v, c = cov.get((x, y), (0, 0))
                                diff += text[i] != v or (v and col[i] != c) or out[i] != outline.get((x, y), 0)
                        checked += 1
                        if diff:
                            bad += 1
                            print('DIFF %s %s variant %d t=%d: %d pixels' % (name, os.path.basename(path)[:2], vi, t, diff))
    print('compared %d frames (3 layers, %d pixels each); frames with a difference: %d' % (checked, GW * GH, bad))
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
