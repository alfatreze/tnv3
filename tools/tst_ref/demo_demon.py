#!/usr/bin/env python3
"""Build the demon demo area and render a contact sheet and a panel image.
    demo_demon.py OUT_DIR [FRAME.png] [--video WxH] [--demon FILE.ttf]
The demon font is the single --font line below (--demon swaps it, e.g. for another fantasy font)."""
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
F = os.path.join(HERE, 'fonts')
S = os.path.join(HERE, 'samples')


def main():
    args = sys.argv[1:]
    demon = os.path.join(F, 'UnifrakturCook-Bold.ttf')
    if '--demon' in args:
        i = args.index('--demon')
        demon = args[i + 1]
        del args[i:i + 2]
    video = '800x334'
    if '--video' in args:
        i = args.index('--video')
        video = args[i + 1]
        del args[i:i + 2]
    out = args[0]
    frame = args[1] if len(args) > 1 else None
    os.makedirs(out, exist_ok=True)
    area = os.path.join(out, 'demon.area')
    cmd = [sys.executable, os.path.join(HERE, 'tst_build.py'),
           '--track', os.path.join(S, 'demon.ass') + ':eng:default',
           '--sizes', '16x24,20x28,26x36',
           '--font', 'Roboto=' + os.path.join(F, 'Roboto-Regular.ttf'),
           '--font', 'Roboto:italic=' + os.path.join(F, 'Roboto-Italic.ttf'),
           '--font', 'Demon=' + demon + '@1.15',                     # the demon font: swap this one
           '--video', video, '-o', area]
    subprocess.run(cmd, check=True)
    r = [sys.executable, os.path.join(HERE, 'tst_render.py'), area, '--video', video]
    if frame:
        r += ['--frame', frame]
    for t in ('1.0', '4.0', '7.0', '10.5', '13.0', '16.0', '19.0'):
        subprocess.run(r + ['--time', t, '--preset', 'classic', '-o', os.path.join(out, 'demon_%s.png' % t)], check=True)
    subprocess.run(r + ['--time', '4.0', '--sheet', os.path.join(out, 'sheet_demon.png')], check=True)
    subprocess.run(r + ['--time', '16.0', '--preset', 'classic', '--panel', '-o', os.path.join(out, 'panel_demon.png')],
                   check=True)


if __name__ == '__main__':
    main()
