import json, math, pathlib, io, sys
S = pathlib.Path(__file__).parent
OUTF = S.parents[2] / "docs" / "codec_phase0" / "r0_rate_table.txt"
R = [r for r in json.load(open(S / "r0_results.json")) if "err" not in r]
CLIPS = ["dialog", "dark", "action", "anime_wide", "anime_dialog", "anime_motion", "bw_garden", "bw_closeup", "bw_room"]
GROUPS = {"live": CLIPS[0:3], "anime": CLIPS[3:6], "bw": CLIPS[6:9]}
METRICS = (("vmaf", (85, 90)), ("ssim", (0.95, 0.97)), ("psnr", (36, 38)))
CODECS = list(dict.fromkeys(r["codec"] for r in R))
out = io.StringIO()
def P(*a):
    print(*a, file=out)

def interp(pts, key, target):
    pts = sorted(pts, key=lambda p: p[key])
    for a, b in zip(pts, pts[1:]):
        if a[key] <= target <= b[key] and a[key] != b[key]:
            t = (target - a[key]) / (b[key] - a[key])
            return math.exp(math.log(a["kb_frame"]) * (1 - t) + math.log(b["kb_frame"]) * t)
    return None

def gm(v):
    return math.exp(sum(math.log(x) for x in v) / len(v)) if v and all(x is not None for x in v) else None

def f(x):
    return f"{x:7.2f}" if x is not None else "    n/a"

for key, targets in METRICS:
    for t in targets:
        P(f"\n=== kB/frame at equal {key} = {t} ===")
        P(f"{'codec':16s}" + "".join(f"{c[:9]:>10s}" for c in CLIPS) + "   live   anime      bw    all9  ratio_to_lite_cabac")
        tab = {}
        for c in CODECS:
            v = [interp([r for r in R if r["codec"] == c and r["clip"] == cl], key, t) for cl in CLIPS]
            g = [gm(v[0:3]), gm(v[3:6]), gm(v[6:9])]
            a = gm(v)
            tab[c] = (v, g, a)
        base = tab.get("lite_cabac", (None, None, None))[2]
        for c in CODECS:
            v, g, a = tab[c]
            ratio = f"{a / base:6.3f}" if a and base else "   n/a"
            P(f"{c:16s}" + "".join(f"{f(x):>10s}" for x in v) + "".join(f(x) for x in g) + f(a) + "   " + ratio)

OUTF.parent.mkdir(parents=True, exist_ok=True)
txt = out.getvalue()
OUTF.write_text(txt)
print(txt)
