#!/usr/bin/env python3
"""Run tac_fx cmp (fixed-point model vs the double reference) over the TAC vectors and any extra .tac files given.
Usage: python3 fx_sweep.py [-f FX] [-t TW] [extra.tac ...]"""
import glob, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
args = sys.argv[1:]
opts, files = [], []
i = 0
while i < len(args):
    if args[i] in ('-f', '-t'):
        opts += args[i:i + 2]; i += 2
    else:
        files.append(args[i]); i += 1
files = sorted(glob.glob(os.path.join(HERE, 'vectors', '*.tac'))) + files
for f in files:
    r = subprocess.run([os.path.join(HERE, 'tac_fx'), 'cmp', f] + opts, capture_output=True, text=True)
    print('%-34s %s' % (os.path.basename(f), (r.stdout + r.stderr).strip()))
