import os, sys, hashlib, subprocess, tempfile
here = os.path.dirname(os.path.abspath(__file__))
vec = os.path.join(here, '..', 'vectors')
out = tempfile.mkdtemp()
ok = True
for line in open(os.path.join(vec, 'expected.txt')):
    if not line.strip():
        continue
    name, md5 = line.split()
    o = os.path.join(out, name + '.y4m')
    r = subprocess.run([sys.executable, os.path.join(here, 'tvc_dec.py'), os.path.join(vec, name), o],
                       stderr=subprocess.DEVNULL)
    if md5 == 'REJECT':
        good = r.returncode == 3 and not os.path.exists(o)
        print('PASS' if good else 'FAIL', name, 'REJECT (status %d)' % r.returncode, flush=True)
    else:
        got = hashlib.md5(open(o, 'rb').read()).hexdigest() if r.returncode == 0 and os.path.exists(o) else 'error'
        good = got == md5
        print('PASS' if good else 'FAIL', name, got, flush=True)
    ok &= good
sys.exit(0 if ok else 1)
