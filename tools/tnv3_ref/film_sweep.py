#!/usr/bin/env python3
"""Whole-film size/quality sweep with encode_film.py: each film at several QPs, quality measured, reports collected in
OUTDIR/sweep.json and interpolated to VMAF 90 (MB/h at the QP where the film's mean VMAF crosses 90).
Usage: python3 tools/tnv3_ref/film_sweep.py OUTDIR [QP ...]"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
MEDIA = os.path.join(HERE, '..', '..', '..', '..', '..', 'media')     # tau-nu/media from the worktree
FILMS = [('tears', 'tears-of-steel.mp4', ['--width', '800']),                         # CG/live action 2.4:1, 24 fps
         ('bwtv', 'MPEG-4 movie.mp4', ['--width', 'native']),                         # 4:3 B&W TV, 640x480, 23.976
         ('anime', 'Tensei Shitara Slime Datta Ken 4th Season - 18.mkv', ['--width', '800'])]   # 16:9 anime (damaged container)

out = sys.argv[1]
qps = [int(q) for q in sys.argv[2:]] or [26, 30, 34]
os.makedirs(out, exist_ok=True)
res = []
for name, f, extra in FILMS:
    for qp in qps:
        dst = os.path.join(out, '%s_q%d.tnv3' % (name, qp))
        if not os.path.exists(dst[:-5] + '.json'):
            subprocess.run([sys.executable, os.path.join(HERE, 'encode_film.py'), os.path.join(MEDIA, f), dst, '--qp', str(qp), '--quality'] + extra, check=True)
        r = json.load(open(dst[:-5] + '.json'))
        r['name'] = name
        res.append(r)
        if os.path.exists(dst) and qp != 30:
            os.remove(dst)                       # keep only the QP 30 files (test material); the reports stay
summary = {}
for name, _, _ in FILMS:
    pts = sorted((r['vmaf'], r['mb_per_hour'], r['qp']) for r in res if r['name'] == name)
    at90 = None
    for (v0, m0, _), (v1, m1, _) in zip(pts, pts[1:]):
        if v0 <= 90 <= v1 and v1 > v0:
            at90 = round(m0 + (m1 - m0) * (90 - v0) / (v1 - v0), 1)
    summary[name] = dict(points=[dict(qp=q, vmaf=v, mb_per_hour=m) for v, m, q in pts], mb_per_hour_at_vmaf90=at90)
json.dump(dict(runs=res, summary=summary), open(os.path.join(out, 'sweep.json'), 'w'), indent=1)
print(json.dumps(summary, indent=1))
