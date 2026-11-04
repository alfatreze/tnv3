#!/usr/bin/env python3
import hashlib, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import tac_fx

VEC = os.path.join(HERE, '..', 'vectors')
bad = 0
for line in open(os.path.join(VEC, 'fx_expected.txt')):
    p = line.split()
    if len(p) != 2:
        continue
    name, md5 = p
    try:
        _, _, pcm = tac_fx.decode(os.path.join(VEC, name))
        got = hashlib.md5(pcm.tobytes()).hexdigest()
    except Exception as e:
        got = 'error: %s' % e
    ok = got == md5
    bad += not ok
    print('%s %s %s' % ('PASS' if ok else 'FAIL', name, '' if ok else got))
sys.exit(1 if bad else 0)
