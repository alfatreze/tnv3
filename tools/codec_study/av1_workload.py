#!/usr/bin/env python3
"""AV1 decoder workload from real streams: encode every study clip with libaom (full tools, lean0, lean2) at one quality
setting, decode with the inspection build (build/av1stats, see av1stats.c) and tabulate what the hardware must do.

  python3 tools/codec_study/av1_workload.py run        # encode + decode all clips (needs aomenc, build/av1stats)
  python3 tools/codec_study/av1_workload.py table      # markdown tables from work/codec_phase0/av1/*.txt
"""
import json, pathlib, subprocess, sys
from collections import defaultdict

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import sweep
from clips import CLIPS

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / "work" / "codec_phase0" / "av1"
CQ = 44
CFGS = {"full": "aom_full", "lean0": "aom_lean0", "lean2": "aom_lean2"}
FPS = 24

BSIZE = ["4x4", "4x8", "8x4", "8x8", "8x16", "16x8", "16x16", "16x32", "32x16", "32x32", "32x64", "64x32", "64x64",
         "64x128", "128x64", "128x128", "4x16", "16x4", "8x32", "32x8", "16x64", "64x16"]
TXSIZE = ["4x4", "8x8", "16x16", "32x32", "64x64", "4x8", "8x4", "8x16", "16x8", "16x32", "32x16", "32x64", "64x32",
          "4x16", "16x4", "8x32", "32x8", "16x64", "64x16"]
# libaom enum order of intra prediction modes: DC V H D45 D135 D113 D157 D203 D67 SMOOTH SMOOTH_V SMOOTH_H PAETH, then UV_CFL
INTRA = {0: "DC", 1: "V", 2: "H", 3: "D45", 4: "D135", 5: "D113", 6: "D157", 7: "D203", 8: "D67", 9: "SMOOTH",
         10: "SMOOTH_V", 11: "SMOOTH_H", 12: "PAETH"}


def category(name):
    n = name.lower()
    if n.startswith("read_coeffs") or "golomb" in n or "tx_type" in n:
        return "coefficients"
    if "read_mv" in n or "drl" in n:
        return "motion vectors"
    if "partition" in n:
        return "partition"
    if "tx_size" in n:
        return "transform size"
    if "intra" in n or "angle" in n or "cfl" in n or "palette" in n or "filter_intra" in n:
        return "intra modes"
    if "inter" in n or "ref_frames" in n or "reference_mode" in n or "motion_mode" in n or "skip" in n or "interp" in n:
        return "inter modes, refs, skip"
    if "wiener" in n or "sgr" in n or "cdef" in n or "lr" in n or "delta" in n:
        return "filters and quantiser"
    return "other"


def run():
    OUT.mkdir(parents=True, exist_ok=True)
    for clip in CLIPS:
        src = ROOT / "work" / "codec_phase0" / "ref" / f"{clip}_800.y4m"
        for tag, cfg in CFGS.items():
            ivf = OUT / f"{clip}_{tag}.ivf"
            txt = OUT / f"{clip}_{tag}.txt"
            if txt.exists():
                continue
            build = sweep.CONFIGS[cfg][0]
            cmd = build(CQ, src, ivf)[0]
            subprocess.run(cmd, check=True, capture_output=True)
            with open(txt, "w") as fh:
                subprocess.run([str(ROOT / "build" / "av1stats"), str(ivf)], stdout=fh, check=True, stderr=subprocess.DEVNULL)
            print(clip, tag, ivf.stat().st_size, "bytes")


def load(path):
    d = {"F": [], "S": {}, "H": defaultdict(dict), "MV": {"y": {}, "x": {}}}
    for ln in path.read_text().splitlines():
        t = ln.split()
        if t[0] == "F":
            d["F"].append([int(x) for x in t[1:6]] + [float(t[6])])
        elif t[0] == "S":
            d["S"][t[1]] = (float(t[2]), float(t[3]))
        elif t[0] == "H":
            d["H"][t[1]][int(t[2])] = float(t[3])
        elif t[0] == "MV":
            d["MV"][t[1]][int(t[2])] = float(t[3])
    return d


def pctl(h, p):
    tot = sum(h.values())
    acc = 0
    for k in sorted(h):
        acc += h[k]
        if tot and acc / tot >= p:
            return k
    return 0


def table():
    rows = {}
    for clip in CLIPS:
        for tag in CFGS:
            p = OUT / f"{clip}_{tag}.txt"
            ivf = OUT / f"{clip}_{tag}.ivf"
            if not p.exists():
                continue
            d = load(p)
            frames = d["F"]
            syms = [f[4] for f in frames]
            bits = sum(v[0] for v in d["S"].values()) / 8.0
            # per-second symbol counts over shown-frame order is not tracked; use frames in decode order in groups of 24
            sec = [sum(syms[i:i + FPS]) for i in range(0, max(1, len(syms) - FPS + 1), FPS // 2)]
            cats = defaultdict(float)
            for n, (b, c) in d["S"].items():
                cats[category(n)] += b / 8.0
            area = sum(d["H"]["bsize"].values())
            rows[(clip, tag)] = dict(
                kbps=ivf.stat().st_size * 8 / 10 / 1000, symbols=sum(syms), sym_per_s=sum(syms) / 10,
                bits_per_symbol=bits / max(1, sum(syms)), peak_frame=max(syms), peak_second=max(sec),
                cats={k: v / bits for k, v in cats.items()}, d=d, area=area)
    return rows


def md(rows):
    out = []
    out.append("| clip | cfg | kbps | symbols/s (mean) | bits/symbol | peak frame (symbols) | peak 1 s window (symbols) | "
               "coef % | mv % | modes+refs+skip % | partition % | intra % | other % |")
    out.append("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for (clip, tag), r in rows.items():
        c = r["cats"]
        out.append(f"| {clip} | {tag} | {r['kbps']:.0f} | {r['sym_per_s'] / 1000:.0f} k | {r['bits_per_symbol']:.2f} | "
                   f"{r['peak_frame']:,} | {r['peak_second']:,} | {c.get('coefficients', 0) * 100:.0f} | "
                   f"{c.get('motion vectors', 0) * 100:.0f} | {c.get('inter modes, refs, skip', 0) * 100:.0f} | "
                   f"{c.get('partition', 0) * 100:.0f} | {c.get('intra modes', 0) * 100:.0f} | "
                   f"{(c.get('other', 0) + c.get('filters and quantiser', 0) + c.get('transform size', 0)) * 100:.0f} |")
    return "\n".join(out)


def tools_md(rows):
    out = ["| clip (full tools) | skip % | OBMC % | warped % | compound-masked % | bi-pred % of inter | palette | intrabc | "
           "vertical MV p50/p90/p99/p99.9 (px) | horizontal p99 (px) | 64x64 % | <=16x16 % |",
           "|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for (clip, tag), r in rows.items():
        if tag != "full":
            continue
        d, a = r["d"], r["area"]
        H = d["H"]
        inter = sum(H["motion_mode"].values()) or 1
        mm = H["motion_mode"]
        comp = H["compound_type"]
        masked = sum(v for k, v in comp.items() if k in (1, 2, 3, 4))      # wedge / diff-weighted / distance-weighted types
        bi = H["bi_pred"]
        big = H["bsize"].get(12, 0) + H["bsize"].get(13, 0) + H["bsize"].get(14, 0) + H["bsize"].get(15, 0)
        small = sum(v for k, v in H["bsize"].items() if k in (0, 1, 2, 3, 4, 5, 6, 16, 17))
        out.append(f"| {clip} | {H['skip'].get(1, 0) / sum(H['skip'].values()) * 100:.0f} | {mm.get(1, 0) / inter * 100:.1f} | "
                   f"{mm.get(2, 0) / inter * 100:.1f} | {masked / inter * 100:.1f} | "
                   f"{bi.get(1, 0) / max(1, sum(bi.values())) * 100:.0f} | {sum(v for k, v in H['palette'].items() if k):.0f} | "
                   f"{H['intrabc'].get(1, 0):.0f} | "
                   f"{pctl(d['MV']['y'], .5)} / {pctl(d['MV']['y'], .9)} / {pctl(d['MV']['y'], .99)} / {pctl(d['MV']['y'], .999)} | "
                   f"{pctl(d['MV']['x'], .99)} | {big / a * 100:.0f} | {small / a * 100:.0f} |")
    return "\n".join(out)


def restat():
    """Decode the streams again (after av1stats changed) without re-encoding."""
    for ivf in sorted(OUT.glob("*.ivf")):
        with open(ivf.with_suffix(".txt"), "w") as fh:
            subprocess.run([str(ROOT / "build" / "av1stats"), str(ivf)], stdout=fh, check=True, stderr=subprocess.DEVNULL)


# cycles of tau_av1_itx for a block of full density (every row coded, every coefficient position busy), from the
# sequencer: per 1D transform N loads + 5 + program + 5 + N stores + 1; program cycles from tools/gen_av1_itx_prog.py
PROG = {4: 11, 8: 26, 16: 74, 32: 194}
TXWH = {0: (4, 4), 1: (8, 8), 2: (16, 16), 3: (32, 32), 5: (4, 8), 6: (8, 4), 7: (8, 16), 8: (16, 8), 9: (16, 32),
        10: (32, 16), 13: (4, 16), 14: (16, 4), 15: (8, 32), 16: (32, 8)}


def itx_cycles(w, h, nz):
    """Engine cycles for one w x h block when the first nz rows hold coefficients (worst-case transform types)."""
    row = lambda n: 2 * n + 11 + PROG[n]
    return nz * row(w) + w * row(h) + 12


def load_frames_T(path):
    fr = defaultdict(dict)
    for ln in path.read_text().splitlines():
        t = ln.split()
        if t[0] == "T":
            fr[int(t[1])][int(t[2])] = float(t[3])
    return fr


def itx_table():
    """Transform engine load per frame from real streams: upper bound (every row coded) and a sparse case (nz = 4 rows)."""
    out = ["| clip | cfg | coded 4x4 units per frame: mean / worst | cycles per frame, all rows coded: mean / worst | "
           "cycles per frame, 4 rows coded: mean / worst | worst as % of a 41.7 ms frame at 100 MHz (all rows / 4 rows) |",
           "|---|---|---|---|---|---|"]
    for clip in CLIPS:
        for tag in ("full", "lean2"):
            p = OUT / f"{clip}_{tag}.txt"
            if not p.exists():
                continue
            fr = load_frames_T(p)
            full, sparse, units = [], [], []
            for f, d in fr.items():
                cf = cs = u = 0
                for t, area in d.items():
                    if t not in TXWH:
                        continue
                    w, h = TXWH[t]
                    nblk = area / (w // 4 * (h // 4))
                    cf += nblk * itx_cycles(w, h, h)
                    cs += nblk * itx_cycles(w, h, min(4, h))
                    u += area
                full.append(cf); sparse.append(cs); units.append(u)
            n = max(1, len(full))
            out.append(f"| {clip} | {tag} | {sum(units) / n:,.0f} / {max(units):,.0f} | {sum(full) / n / 1e6:.2f} M / {max(full) / 1e6:.2f} M | "
                       f"{sum(sparse) / n / 1e6:.2f} M / {max(sparse) / 1e6:.2f} M | "
                       f"{max(full) / 4.17e6 * 100:.0f}% / {max(sparse) / 4.17e6 * 100:.0f}% |")
    return "\n".join(out)


if __name__ == "__main__":
    if sys.argv[1] == "run":
        run()
    elif sys.argv[1] == "restat":
        restat()
    elif sys.argv[1] == "itx":
        print(itx_table())
    else:
        rows = table()
        print(md(rows))
        print()
        print(tools_md(rows))
