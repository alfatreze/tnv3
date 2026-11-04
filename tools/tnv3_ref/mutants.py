#!/usr/bin/env python3
"""Mutation test of the C reference checker: each mutant of tnv3.c must disagree with vectors/expected.txt on at least one
vector (a valid file rejected, an invalid one accepted, or a different demuxed output)."""
import hashlib, os, re, subprocess, sys, tempfile

ROOT = os.path.dirname(os.path.abspath(__file__))
SRC = open(os.path.join(ROOT, 'tnv3.c')).read()
MUTANTS = [
    ('B pictures counted as anchors', 'if (t->ptype[n] == 2) t->disp[n] = out++;', 'if (t->ptype[n] == 1) t->disp[n] = out++;'),
    ('S(k) rounding order', 'return k * 48000u * fd / fn;', 'return k * 48000u / fn * fd;'),
    ('lower bound +1', 'uint64_t need = S(D + 1, t->fn, t->fd) / 1024 + 2;', 'uint64_t need = S(D + 1, t->fn, t->fd) / 1024 + 1;'),
    ('upper bound +5', 't->ftac[n] > need + 4', 't->ftac[n] > need + 5'),
    ('reset bit from byte 0', 'for (int i = 1; i <= 4; i++)', 'for (int i = 0; i < 4; i++)'),
    ('CRC polynomial', '0xEDB88320u', '0x04C11DB7u'),
    ('seek point strict', 'for (uint32_t r = 0; r < t->ntac && (uint64_t)r * 1024 <= sk; r++) if (t->reset[r]) seek = r;',
     'for (uint32_t r = 0; r < t->ntac && (uint64_t)r * 1024 < sk; r++) if (t->reset[r]) seek = r;'),
    ('D(n) = disp(n)', 'if (t->disp[n] > D) D = t->disp[n];\n        if (S(t->disp', 'D = t->disp[n];\n        if (S(t->disp'),
    ('glyph id bound', 'id >= gc', 'id > gc'),
    ('rate window one short', 'k < win && k <= n; k++) sum +=', 'k + 1 < win && k <= n; k++) sum +='),
    ('chunk rate margin doubled', 'if (csum > L[5] + 65536)', 'if (csum > L[5] + 131072)'),
    ('variant order not strict', 'if (chh <= prevh)', 'if (chh < prevh)'),
    ('two default tracks allowed', 'if (ndef > 1)', 'if (ndef > 2)'),
    ('event flag bit 2 allowed', 'q[13] > 3', 'q[13] > 7'),
    ('reserved codes allowed', 'if (id >= 0xFFF0 && id != 0xFFFF)', 'if (id > 0xFFFF)'),
    ('colour index 31', 'q[12] > 15', 'q[12] > 31'),
    ('colour switches as glyph ids', 'if (id < 0xFFE0 && id >= gc)', 'if (id != 0xFFFF && id >= gc)'),
]


def outcome(exe, tmp, line):
    name = line.split()[0]
    f = os.path.join(ROOT, 'vectors', name)
    r = subprocess.run([exe, 'check', f], capture_output=True)
    want_valid = line.split()[1] == 'valid'
    if (r.returncode == 0) != want_valid:
        return False
    if not want_valid:
        return True
    outs = [os.path.join(tmp, name + e) for e in ('.tvc', '.tac', '.subs', '.glyphs')]
    subprocess.run([exe, 'demux', f] + outs, capture_output=True)
    for key, o in zip(('tvc=', 'tac=', 'subs=', 'glyphs='), outs):
        m = re.search(key + '([0-9a-f]+)', line)
        if m and hashlib.md5(open(o, 'rb').read()).hexdigest() != m.group(1):
            return False
    return True


lines = [l for l in open(os.path.join(ROOT, 'vectors', 'expected.txt')) if l.strip()]
tmp = tempfile.mkdtemp()
survived = 0
for i, (desc, a, b) in enumerate([('original', '', '')] + MUTANTS):
    src = SRC
    if a:
        assert SRC.count(a) == 1, desc
        src = SRC.replace(a, b)
    c, exe = os.path.join(tmp, 'm%d.c' % i), os.path.join(tmp, 'm%d' % i)
    open(c, 'w').write(src)
    subprocess.run(['cc', '-O2', '-o', exe, c], check=True)
    bad = [l.split()[0] for l in lines if not outcome(exe, tmp, l)]
    if not a:
        print('original:', 'all vectors agree' if not bad else 'DISAGREES on %s' % bad)
        if bad:
            sys.exit(1)
        continue
    print('%-30s %s' % (desc, 'killed by ' + ', '.join(bad[:3]) if bad else 'SURVIVED'))
    survived += not bad
sys.exit(1 if survived else 0)
