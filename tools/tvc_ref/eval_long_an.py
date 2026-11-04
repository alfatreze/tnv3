import json, math, pathlib, statistics
S = pathlib.Path(__file__).resolve().parent
OUTF = S.parents[1] / "docs" / "codec_phase0" / "tvc_long_eval_v5.txt"
R = [r for r in json.load(open(S / "tvc_long_results.json")) if "err" not in r]
CL = ["long_tears", "long_anime", "long_bw"]
CF = ["long_v3", "long_v3s", "long_v4", "long_v5"]
out = []


def P(s=""):
    out.append(s)


def interp(pts, key, target):
    pts = sorted(pts, key=lambda p: p[key])
    for a, b in zip(pts, pts[1:]):
        if a[key] <= target <= b[key] and a[key] != b[key]:
            t = (target - a[key]) / (b[key] - a[key])
            return math.exp(math.log(a["kb_frame"]) * (1 - t) + math.log(b["kb_frame"]) * t)


def rt(a, b):
    return f"{a/b:6.3f}" if a and b else "   n/a"


def f(x):
    return f"{x:7.3f}" if x else "    n/a"


for cl in CL:
    P(f"\n##### {cl}: raw points (q, kB/frame, vmaf, psnr, ssim, enc s)")
    for c in CF:
        for r in sorted([r for r in R if r["clip"] == cl and r["codec"] == c], key=lambda r: r["q"]):
            P(f"  {c:9s} q{r['q']} {r['kb_frame']:8.3f} {r['vmaf']:7.3f} {r['psnr']:7.3f} {r['ssim']:.4f} {r['enc_seconds']:6.0f}s")
    mid = [r for r in R if r["clip"] == cl and r["codec"] == "long_v3" and r["q"] == 30]
    if not mid:
        continue
    for key in ("vmaf", "psnr"):
        t = mid[0][key]
        P(f"  -- equal {key} = {t:.3f} (long_v3 q30)")
        v = {c: interp([r for r in R if r["clip"] == cl and r["codec"] == c], key, t) for c in CF}
        P("  kB/frame: " + "  ".join(f"{c} {f(v[c])}" for c in CF))
        P(f"  long_v3s/long_v3 {rt(v['long_v3s'], v['long_v3'])}   long_v4/long_v3s {rt(v['long_v4'], v['long_v3s'])}   long_v4/long_v3 {rt(v['long_v4'], v['long_v3'])}   long_v5/long_v4 {rt(v['long_v5'], v['long_v4'])}   long_v5/long_v3 {rt(v['long_v5'], v['long_v3'])}")
P("\nmean encode seconds: " + ", ".join(f"{c} {statistics.mean(r['enc_seconds'] for r in R if r['codec']==c):.0f}" for c in CF))
OUTF.write_text("\n".join(out))
print("\n".join(out))
