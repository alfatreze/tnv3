#!/usr/bin/env python3
"""Turn results.jsonl into the phase-0 tables (markdown on stdout).

Rate comparison is done at equal VMAF: kbps is interpolated (log-linear in VMAF
between the two bracketing sweep points) and never extrapolated.  The headline
number per configuration is the mean log-bitrate ratio over a VMAF window both
curves cover (a Bjontegaard-style average), reported as a percent size change.
"""
import json, math, pathlib, sys
from collections import defaultdict

ROOT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0"
CLIPS = ["dialog", "dark", "action", "anime_wide", "anime_dialog", "anime_motion", "bw_garden", "bw_closeup", "bw_room"]


METRIC = sys.argv[1] if len(sys.argv) > 1 else "vmaf"
# (window lo, hi, report targets) per quality metric
RANGE = {"vmaf": (78, 95, (85, 90, 93)), "ssim": (0.940, 0.985, (0.95, 0.97, 0.98)),
         "psnr_y": (35.0, 42.0, (37, 39, 41))}


def load():
    d = defaultdict(list)
    for ln in (ROOT / "results.jsonl").read_text().splitlines():
        r = json.loads(ln)
        if "error" in r:
            continue
        d[(r["clip"], r["cfg"], r["res"])].append((r[METRIC], r["kbps"], r))
    for v in d.values():
        v.sort()
    return d


def kbps_at(pts, target):
    for (v0, k0, _), (v1, k1, _) in zip(pts, pts[1:]):
        if v0 <= target <= v1 and v1 > v0:
            t = (target - v0) / (v1 - v0)
            return math.exp(math.log(k0) + t * (math.log(k1) - math.log(k0)))
    return None


def window(a, b, lo=None, hi=None, n=60):
    lo = RANGE[METRIC][0] if lo is None else lo
    hi = RANGE[METRIC][1] if hi is None else hi
    xs = []
    for i in range(n + 1):
        t = lo + (hi - lo) * i / n
        ka, kb = kbps_at(a, t), kbps_at(b, t)
        if ka and kb:
            xs.append(math.log(kb / ka))
    return (sum(xs) / len(xs), len(xs) / (n + 1)) if xs else (None, 0)


def main():
    d = load()
    cfgs = sorted({(c, r) for (_, c, r) in d})
    base = (sys.argv[2] if len(sys.argv) > 2 else "mpeg4_tuned", "800")
    print(f"# Quality metric: {METRIC}\n")
    print(f"## kbps at equal {METRIC} (800x334 output), per clip\n")
    for T in RANGE[METRIC][2]:
        print(f"\n### VMAF {T}\n")
        print("| config | res | " + " | ".join(CLIPS) + " |")
        print("|---|---|" + "---|" * len(CLIPS))
        for c, r in cfgs:
            row = []
            for cl in CLIPS:
                k = kbps_at(d.get((cl, c, r), []), T)
                row.append(f"{k:.0f}" if k else "-")
            print(f"| {c} | {r} | " + " | ".join(row) + " |")
    print(f"\n## Size change vs {base[0]} {base[1]} (mean over the {METRIC} window {RANGE[METRIC][0]}-{RANGE[METRIC][1]}, per clip and group)\n")
    print("| config | res | " + " | ".join(CLIPS) + " | live mean | anime mean | bw mean | all mean | window coverage |")
    print("|---|---|" + "---|" * (len(CLIPS) + 5))
    pct = lambda vs: f"{(math.exp(sum(vs) / len(vs)) - 1) * 100:+.0f}%" if vs else "-"
    for c, r in cfgs:
        vals, cov = {}, []
        row = []
        for cl in CLIPS:
            a, b = d.get((cl, *base), []), d.get((cl, c, r), [])
            m, cv = window(a, b) if a and b else (None, 0)
            row.append(f"{(math.exp(m) - 1) * 100:+.0f}%" if m is not None else "-")
            if m is not None:
                vals[cl] = m; cov.append(cv)
        live = [v for k, v in vals.items() if not k.startswith(("anime", "bw"))]
        anim = [v for k, v in vals.items() if k.startswith("anime")]
        bw = [v for k, v in vals.items() if k.startswith("bw")]
        print(f"| {c} | {r} | " + " | ".join(row) + f" | {pct(live)} | {pct(anim)} | {pct(bw)} | **{pct(list(vals.values()))}** | "
              f"{sum(cov) / max(len(cov), 1):.0%} |")
    print("\n## Resolution switch: coded 400x166 upscaled vs native 800x334 (same encoder)\n")
    print("| encoder | clip | best VMAF at 400 | kbps there | native kbps at same VMAF | 400 / native |")
    print("|---|---|---|---|---|---|")
    for enc in ("mpeg4_tuned", "x264_main_b", "x265_medium", "svt_full", "aom_full"):
        for cl in CLIPS:
            lo, hi = d.get((cl, enc, "400"), []), d.get((cl, enc, "800"), [])
            if not lo or not hi:
                continue
            for T in (70, 75, 80, 85):
                a, b = kbps_at(lo, T), kbps_at(hi, T)
                if a and b:
                    print(f"| {enc} | {cl} | {max(v for v, _, _ in lo):.0f} (ceiling) | {a:.0f} @ {T} | {b:.0f} | {a / b:.2f} |")
    print("\n## Encode speed (mean seconds per 10 s clip, informational)\n")
    sp = defaultdict(list)
    for (cl, c, r), pts in d.items():
        for _, _, rec in pts:
            sp[(c, r)].append(rec["enc_s"])
    for k, v in sorted(sp.items()):
        print(f"- {k[0]} {k[1]}: {sum(v) / len(v):.1f} s")


if __name__ == "__main__":
    main()
