#!/usr/bin/env python3
"""Mutation test of the integer model (tac_fx.c, TAC spec section 10): each mutant must change the PCM of at least one
vector against vectors/fx_expected.txt, so the vectors pin down every rounding and table of the model."""
import hashlib, os, subprocess, sys, tempfile, wave

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = open(os.path.join(HERE, 'tac_fx.c')).read()
MUTANTS = [
    ('round half up -> truncate', 'return s <= 0 ? v * ((int64_t)1 << -s) : (v + ((int64_t)1 << (s - 1))) >> s;',
     'return s <= 0 ? v * ((int64_t)1 << -s) : v >> s;'),
    ('butterfly without +1', 're[s + k] = (ar + br + 1) >> 1;', 're[s + k] = (ar + br) >> 1;'),
    ('|q|^(4/3) table 15 bits', '#define PB 16 ', '#define PB 15 '),
    ('2^(b/4) table 18 bits', '#define P2B 22 ', '#define P2B 18 '),
    ('no interpolation above 1024', '16 * (POW43[a >> 3] + rshift((POW43[(a >> 3) + 1] - POW43[a >> 3]) * (a & 7), 3))', '16 * POW43[a >> 3]'),
    ('noise rounded with its sign', 'int64_t m = clampx(rshift((u < 0 ? -u : u) * P2[b], 15 + P2B - FX - ex));\n    return u < 0 ? -m : m;',
     'return clampx(rshift(u * P2[b], 15 + P2B - FX - ex));'),
    ('clamp 2^27', '#define XLIM (((int64_t)1) << (26 + FX))', '#define XLIM (((int64_t)1) << (27 + FX))'),
    ('output rounds half up', 'v = acc < 0 ? -v : v;', 'v = (acc + h) >> FX;'),
    ('post-twiddle conjugate dropped', 'cmul(&a, &c, tw[2][k], -tw[3][k]);', 'cmul(&a, &c, tw[2][k], tw[3][k]);'),
    ('twiddles 19 bits', 'static int FX = 4, TW = 20;', 'static int FX = 4, TW = 19;'),
    ('M/S without clamp', 'coef[0][idx[i]] = clampx(m + s); coef[1][idx[i]] = clampx(m - s);', 'coef[0][idx[i]] = m + s; coef[1][idx[i]] = m - s;'),
]


def digests(exe, tmp):
    out = {}
    for line in open(os.path.join(HERE, 'vectors', 'fx_expected.txt')):
        name, md5 = line.split()
        w = os.path.join(tmp, 'o.wav')
        subprocess.run([exe, 'dec', os.path.join(HERE, 'vectors', name), w], check=True, capture_output=True)
        with wave.open(w) as f:
            out[name] = (hashlib.md5(f.readframes(f.getnframes())).hexdigest(), md5)
    return out


tmp = tempfile.mkdtemp()
survived = 0
for i, (desc, a, b) in enumerate([('original', '', '')] + MUTANTS):
    src = SRC
    if a:
        assert SRC.count(a) == 1, desc
        src = SRC.replace(a, b)
    c, exe = os.path.join(HERE, '_mut%d.c' % i), os.path.join(tmp, 'm%d' % i)
    open(c, 'w').write(src)                       # next to tac.c, which it includes
    try:
        subprocess.run(['cc', '-O2', '-w', '-o', exe, c, '-lm'], check=True)
    finally:
        os.remove(c)
    bad = [n for n, (got, want) in digests(exe, tmp).items() if got != want]
    if not a:
        print('original:', 'all vectors match' if not bad else 'MISMATCH on %s' % bad)
        if bad:
            sys.exit(1)
        continue
    print('%-34s %s' % (desc, 'killed by ' + ', '.join(bad[:3]) if bad else 'SURVIVED'))
    survived += not bad
sys.exit(1 if survived else 0)
