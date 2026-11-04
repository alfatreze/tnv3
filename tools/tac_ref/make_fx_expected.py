#!/usr/bin/env python3
"""Write vectors/fx_expected.txt: the md5 of the integer model's PCM (tac_fx dec, section 10 of the TAC spec) for every
TAC vector. An implementation of section 10 (the RTL, or tools/tac_ref/py/tac_fx.py) must reproduce these exactly.
The md5 covers the raw interleaved little-endian 16-bit samples (the WAV data chunk only)."""
import glob, hashlib, os, subprocess, tempfile, wave

HERE = os.path.dirname(os.path.abspath(__file__))
tmp = tempfile.mkdtemp()
lines = []
for f in sorted(glob.glob(os.path.join(HERE, 'vectors', '*.tac'))):
    out = os.path.join(tmp, 'o.wav')
    subprocess.run([os.path.join(HERE, 'tac_fx'), 'dec', f, out], check=True)
    with wave.open(out) as w:
        pcm = w.readframes(w.getnframes())
    lines.append('%s %s' % (os.path.basename(f), hashlib.md5(pcm).hexdigest()))
    print(lines[-1])
open(os.path.join(HERE, 'vectors', 'fx_expected.txt'), 'w').write('\n'.join(lines) + '\n')
