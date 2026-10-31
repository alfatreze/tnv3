import json, math, pathlib
S = pathlib.Path(__file__).parent
R = json.load(open(S / 'results_main.json')) + json.load(open(S / 'results_mpeg2.json')) + json.load(open(S / 'results_lite.json')) + json.load(open(S / 'results_lite2.json'))
R = [r for r in R if 'err' not in r]
def interp(pts, key, target):
    pts = sorted(pts, key=lambda p: p[key])
    for a, b in zip(pts, pts[1:]):
        if a[key] <= target <= b[key] and a[key] != b[key]:
            t = (target - a[key]) / (b[key] - a[key])
            return math.exp(math.log(a['kb_frame']) * (1 - t) + math.log(b['kb_frame']) * t)
    return None
codecs = list(dict.fromkeys(r['codec'] for r in R))
for key, targets in (('vmaf', (80, 90)), ('psnr', (35, 38))):
    print(f"\nkB/frame (800x334) at equal {key}; dialog/dark/action, geo-mean")
    for c in codecs:
        row = []
        for t in targets:
            v = [interp([r for r in R if r['codec'] == c and r['clip'] == cl], key, t) for cl in ('dialog', 'dark', 'action')]
            gm = math.exp(sum(math.log(x) for x in v) / 3) if all(v) else None
            row.append(f"{key}{t}: " + "/".join(f"{x:5.1f}" if x else "  n/a" for x in v) + (f" gm {gm:5.1f}" if gm else " gm   n/a"))
        print(f"{c:12s} " + " | ".join(row))
