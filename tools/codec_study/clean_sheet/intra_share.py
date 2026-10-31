#!/usr/bin/env python3
"""Intra-coding share of x264 (CABAC, no partitions) on real film segments.

Per segment: scdet cut count, then 4 x264 variants (A..D); parses the x264
summary (frame I/P/B sizes, intra MB % in P and B) and cross-checks with
ffprobe -show_frames. Writes JSON results; markdown is built by --report.

Usage: intra_share.py run   (all segments, ~parallel)   |  intra_share.py report
"""
import json, os, re, subprocess, sys, shutil
from concurrent.futures import ThreadPoolExecutor

ROOT = "<tau-v-worktree>"
SCR = ("/private/tmp/claude-501/-Users-abel-santos-Downloads-DEV-PROJECTS-Tau-Alpha-"
       "tau-nu/6ee0a67e-dbcf-4484-9eb3-cee8ffa7a25e/scratchpad/r0intra")
OUT_JSON = os.path.join(SCR, "results.json")
OUT_MD = ("<tau-v-worktree>/.claude/worktrees/"
          "codec-alternatives/docs/codec_phase0/r0_intra_share.md")

SOURCES = {
    "tears": dict(path=ROOT + "/media/tears-of-steel.mp4", w=800, h=334, offs=[120, 300, 480], dur=734.1),
    "anime": dict(path=ROOT + "/.claude/worktrees/codec-phase0/work/codec_phase0/slime_800.mkv",
                  w=800, h=450, offs=[200, 700, 1200], dur=1439.8),
    "bw": dict(path=ROOT + "/media/MPEG-4 movie.mp4", w=800, h=600, offs=[200, 900, 1500], dur=1542.6),
}
SEG = 60
VARIANTS = {"A": (48, 24, 40), "B": (96, 24, 40), "C": (240, 24, 40), "D": (96, 96, 0)}
BASE = ("partitions=none:8x8dct=0:ref=1:weightp=0:mbtree=0:bframes=3:b-pyramid=normal:"
        "b-adapt=0:weightb=0:cabac=1:keyint=%d:min-keyint=%d:scenecut=%d")


def run(cmd):
    return subprocess.run(cmd, capture_output=True, text=True)


def do_segment(name, off):
    s = SOURCES[name]
    off = min(off, int(s["dur"]) - SEG - 1)
    tag = "%s_%d" % (name, off)
    y4m = os.path.join(SCR, tag + ".y4m")
    vf = "fps=24,scale=%d:%d:flags=lanczos,format=yuv420p" % (s["w"], s["h"])
    r = run(["ffmpeg", "-v", "error", "-y", "-ss", str(off), "-i", s["path"], "-t", str(SEG),
             "-an", "-vf", vf, y4m])
    if r.returncode:
        return dict(name=name, off=off, error="y4m: " + r.stderr[-300:])
    res = dict(name=name, off=off, variants={})
    sc = run(["ffmpeg", "-hide_banner", "-v", "info", "-i", y4m, "-vf", "scdet=threshold=10",
              "-f", "null", "-"])
    res["cuts"] = len(re.findall(r"lavfi\.scd\.time", sc.stderr))
    for v, (K, M, S) in VARIANTS.items():
        mkv = os.path.join(SCR, "%s_%s.mkv" % (tag, v))
        e = run(["ffmpeg", "-hide_banner", "-v", "info", "-y", "-i", y4m, "-c:v", "libx264",
                 "-preset", "slow", "-crf", "24", "-x264-params", BASE % (K, M, S), mkv])
        if e.returncode:
            res["variants"][v] = dict(error=e.stderr[-300:])
            continue
        err = e.stderr
        d = dict(K=K)
        for t in "IPB":
            m = re.search(r"frame %s:(\d+)\s+Avg QP:\s*([\d.]+)\s+size:\s*(\d+)" % t, err)
            d["n" + t] = int(m.group(1)) if m else 0
            d["sz" + t] = int(m.group(3)) if m else 0
        for t in "PB":
            m = re.search(r"mb %s\s+I16\.\.4:\s*([\d.]+)%%\s+([\d.]+)%%\s+([\d.]+)%%" % t, err)
            d["intra" + t] = sum(float(x) for x in m.groups()) if m else 0.0
        p = run(["ffprobe", "-v", "error", "-select_streams", "v", "-show_entries",
                 "frame=pict_type,pkt_size", "-of", "csv=p=0", mkv])
        tot = {"I": [0, 0], "P": [0, 0], "B": [0, 0]}
        for line in p.stdout.split():
            a = line.split(",")
            # csv field order from ffprobe is pkt_size,pict_type
            if len(a) >= 2 and a[1] in tot:
                tot[a[1]][0] += 1
                tot[a[1]][1] += int(a[0])
        d["probe"] = tot
        d["frames"] = sum(x[0] for x in tot.values())
        d["bytes"] = sum(x[1] for x in tot.values())
        res["variants"][v] = d
        os.remove(mkv)
    os.remove(y4m)
    return res


def derive(d):
    n = d["frames"]
    nI, bI = d["probe"]["I"]
    nP, bP = d["probe"]["P"]
    nB, bB = d["probe"]["B"]
    tot = d["bytes"]
    fixed = -(-n // d["K"])
    o = dict(nI=nI, extra=nI - fixed, ishare=bI / tot, mI=bI / max(nI, 1), mP=bP / max(nP, 1),
             mB=bB / max(nB, 1), ipP=d["intraP"], ipB=d["intraB"], mean=tot / n,
             est=(bI + d["intraP"] / 100 * bP + d["intraB"] / 100 * bB) / tot, n=n)
    return o


def report():
    R = json.load(open(OUT_JSON))
    L = ["# R0 intra share: how much of a film's size goes to intra coding", "",
         "Method: three 60 s segments per source, scaled with `fps=24,scale=WxH:flags=lanczos,format=yuv420p` to a "
         "lossless y4m; libx264 `-preset slow -crf 24`, params `partitions=none:8x8dct=0:ref=1:weightp=0:mbtree=0:"
         "bframes=3:b-pyramid=normal:b-adapt=0:weightb=0:cabac=1` with keyint/min-keyint/scenecut per variant "
         "(A 48/24/40, B 96/24/40, C 240/24/40, D 96/96/0).",
         "Cuts: ffmpeg `scdet` threshold 10 on the y4m. Frame counts and byte totals per picture type come from "
         "ffprobe `-show_frames` (pkt_size); intra MB % in P/B from the x264 summary (I16 + I4 + I8 columns of the "
         "`mb P`/`mb B` lines).",
         "I-frame bytes include the whole picture. 'Est. total intra' = I-frame bytes + intra MB % x (P bytes) + "
         "intra MB % x (B bytes), divided by all bytes. It is an ESTIMATE and a lower bound (intra MBs are larger "
         "than the average MB of their frame). 'Extra I' = I frames beyond ceil(frames/K).",
         "Source: x264 via ffmpeg 9.0.2; source segment offsets as listed per row.", ""]
    summ = {v: [] for v in VARIANTS}
    cutsrc = {}
    for name in SOURCES:
        L += ["## " + name, "",
              "| segment (s) | cuts | var | I frames | extra I | I byte share | mean I | mean P | mean B | intra MB% P | "
              "intra MB% B | est. total intra | mean B/frame |",
              "|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
        for r in [x for x in R if x["name"] == name]:
            if "error" in r:
                L.append("| %d | ERROR %s |" % (r["off"], r["error"][:60]))
                continue
            cutsrc.setdefault(name, []).append(r["cuts"])
            for v in VARIANTS:
                d = r["variants"].get(v)
                if not d or "error" in d:
                    L.append("| %d | %d | %s | failed |" % (r["off"], r["cuts"], v))
                    continue
                o = derive(d)
                summ[v].append(dict(o, cuts=r["cuts"]))
                L.append("| %d | %d | %s | %d | %d | %.1f%% | %.0f | %.0f | %.0f | %.1f | %.1f | %.1f%% | %.0f |" % (
                    r["off"], r["cuts"], v, o["nI"], o["extra"], 100 * o["ishare"], o["mI"], o["mP"], o["mB"],
                    o["ipP"], o["ipB"], 100 * o["est"], o["mean"]))
        L.append("")
    L += ["## Cuts per minute (scdet threshold 10, mean of the segments)", "",
          "| source | cuts per segment | cuts/min |", "|---|---|---|"]
    allc = []
    for n, c in cutsrc.items():
        allc += c
        L.append("| %s | %s | %.1f |" % (n, ", ".join(map(str, c)), sum(c) / len(c)))
    L.append("| all | | %.1f |" % (sum(allc) / max(len(allc), 1)))
    L += ["", "## Summary: mean over all segments", "",
          "| var | segments | cuts/min | I byte share | extra I frames | intra MB% P | intra MB% B | est. total intra | mean B/frame |",
          "|---|---|---|---|---|---|---|---|---|"]
    for v, lst in summ.items():
        if not lst:
            continue
        m = lambda k: sum(x[k] for x in lst) / len(lst)
        L.append("| %s | %d | %.1f | %.1f%% | %.1f | %.1f | %.1f | %.1f%% | %.0f |" % (
            v, len(lst), m("cuts"), 100 * m("ishare"), m("extra"), m("ipP"), m("ipB"), 100 * m("est"), m("mean")))
    open(OUT_MD, "w").write("\n".join(L) + "\n")
    print("\n".join(L))


def main():
    os.makedirs(SCR, exist_ok=True)
    if len(sys.argv) > 1 and sys.argv[1] == "report":
        return report()
    jobs = [(n, o) for n in SOURCES for o in SOURCES[n]["offs"]]
    with ThreadPoolExecutor(3) as ex:
        out = list(ex.map(lambda j: do_segment(*j), jobs))
    json.dump(out, open(OUT_JSON, "w"), indent=1)
    print("done", OUT_JSON)


if __name__ == "__main__":
    main()
