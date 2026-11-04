import json, math, pathlib
S = pathlib.Path(__file__).resolve().parent
ROOT = S.parents[1]
OUTF = ROOT / "docs" / "codec_phase0" / "tvc_v5_eval.txt"
FILES = (ROOT / "tools/codec_study/clean_sheet/r0_results.json", S / "tvc_v1_results.json")
R = [r for f in FILES for r in json.load(open(f)) if "err" not in r]
CLIPS = ["dialog", "dark", "action", "anime_wide", "anime_dialog", "anime_motion", "bw_garden", "bw_closeup", "bw_room"]
CODECS = ["tvc_v1", "tvc_v1_aq", "tvc_v2", "tvc_v2_t1", "tvc_v2_t2", "tvc_v3", "tvc_v3s", "tvc_v4", "tvc_v5", "lite_cabac", "lite_b", "lite_cavlc", "lite_8x8", "mpeg2_ld", "x264_ld", "aom_ld6"]
TVC = CODECS[:9]
BASES = ("lite_cabac", "lite_b", "mpeg2_ld", "x264_ld")
METRICS = (("vmaf", (85, 90)), ("ssim", (0.97,)), ("psnr", (36, 38)))
out = []


def P(s=""):
    out.append(s)


def interp(pts, key, target):
    pts = sorted(pts, key=lambda p: p[key])
    for a, b in zip(pts, pts[1:]):
        if a[key] <= target <= b[key] and a[key] != b[key]:
            t = (target - a[key]) / (b[key] - a[key])
            return math.exp(math.log(a["kb_frame"]) * (1 - t) + math.log(b["kb_frame"]) * t)


def gm(v):
    return math.exp(sum(math.log(x) for x in v) / len(v)) if v and all(x is not None for x in v) else None


def f(x):
    return f"{x:7.2f}" if x is not None else "    n/a"


def rt(a, b):
    return f"{a/b:6.3f}" if a and b else "   n/a"


for key, targets in METRICS:
    for t in targets:
        P(f"\n=== kB/frame at equal {key} = {t} ===")
        P(f"{'codec':12s}" + "".join(f"{c[:9]:>10s}" for c in CLIPS) + "    live   anime      bw    all9  /lite_cabac  /mpeg2_ld")
        tab = {}
        for c in CODECS:
            v = [interp([r for r in R if r["codec"] == c and r["clip"] == cl], key, t) for cl in CLIPS]
            tab[c] = (v, [gm(v[0:3]), gm(v[3:6]), gm(v[6:9])], gm(v))
        for c in CODECS:
            v, g, a = tab[c]
            P(f"{c:12s}" + "".join(f"{f(x):>10s}" for x in v) + "".join(f(x) for x in g) + f(a) + "   " + "   ".join(rt(a, tab[b][2]) for b in BASES))
        P("  ratios per group (live anime bw all9):")
        for c in TVC:
            for base in BASES:
                gs = tab[c][1] + [tab[c][2]]
                bs = tab[base][1] + [tab[base][2]]
                P(f"  {c:10s} / {base:10s}: " + " ".join(rt(x, y) for x, y in zip(gs, bs)))
OUTF.write_text("\n".join(out))
print("\n".join(out))

print()
q = []
for key, t in (("vmaf", 90), ("ssim", 0.97), ("psnr", 38)):
    q.append(f"\ntvc_v5 / tvc_v4 kB/frame ratio at {key} = {t}")
    for cl in CLIPS + ["ALL"]:
        pass
    for cl in CLIPS:
        a = interp([r for r in R if r["codec"] == "tvc_v5" and r["clip"] == cl], key, t)
        b = interp([r for r in R if r["codec"] == "tvc_v4" and r["clip"] == cl], key, t)
        q.append(f"  {cl:14s} {rt(a, b)}")
txt = "\n".join(q)
print(txt)
OUTF.write_text("\n".join(out) + "\n" + txt)
