#!/usr/bin/env python3
"""Workload dump: what a hardware decoder would actually have to handle.

Encodes each study clip with x264 (Main, B-frames, refs 3, CRF chosen to sit near
VMAF 90) and reports, from the decoded stream:
  * vertical / horizontal vector reach (area-weighted percentiles) per clip
  * block-size mix, bi-prediction share, inter coverage
  * x264's own macroblock type mix (skip / intra / inter) and coded-coefficient share
  * per-frame size by type and the peak-to-mean ratios a decode queue must absorb
Result is written as work/codec_phase0/workload.json and printed as a table.
"""
import json, pathlib, re, statistics, subprocess, sys

ROOT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0"
MV = ROOT / "bin" / "mvdump"
CLIPS = ["dialog", "dark", "action", "anime_wide", "anime_dialog", "anime_motion", "bw_garden", "bw_closeup", "bw_room"]
CRF = {c: 26 for c in CLIPS}      # one setting for all clips; CRF 26 sits around VMAF 90-93 on this material


def pct(hist, p):
    tot = sum(hist)
    acc = 0
    for i, v in enumerate(hist):
        acc += v
        if tot and acc / tot >= p:
            return i
    return len(hist) - 1


def one(clip, crf, extra=()):
    ref = ROOT / "ref" / f"{clip}_800.y4m"
    out = ROOT / "enc" / f"wl_{clip}.h264"
    p = subprocess.run(["ffmpeg", "-v", "info", "-y", "-i", str(ref), "-an", "-c:v", "libx264", "-preset", "slow",
                        "-profile:v", "main", "-bf", "3", "-refs", "3", "-crf", str(crf), "-g", "96", "-threads", "2",
                        *extra, "-f", "h264", str(out)], capture_output=True, text=True)
    log = p.stderr
    mb = {}
    for key in ("mb I", "mb P", "mb B"):
        m = re.search(key + r"\s+(.*)", log)
        if m:
            mb[key] = m.group(1).strip()
    cod = re.search(r"coded y,uvDC,uvAC intra:\s*(.*)", log)
    inter = re.search(r"coded y,uvDC,uvAC inter:\s*(.*)", log)
    txt = subprocess.run([str(MV), str(out)], capture_output=True, text=True).stdout
    dy = [0.0] * 130
    dx = [0.0] * 130
    bs = {}
    frames = []
    for ln in txt.splitlines():
        t = ln.split("\t")
        if t[0] == "DY": dy[int(t[1])] = float(t[2])
        elif t[0] == "DX": dx[int(t[1])] = float(t[2])
        elif t[0] == "BS": bs[t[1]] = int(t[2])
        elif t[0] == "F": frames.append((t[2], float(t[3]), float(t[4]), float(t[5])))
    sizes = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v", "-show_entries", "frame=pict_type,pkt_size",
                            "-of", "csv=p=0", str(out)], capture_output=True, text=True).stdout.split()
    by = {}
    allsz = []
    for s in sizes:
        f = [x for x in s.split(",") if x]
        n = int(next(x for x in f if x.isdigit()))
        t = next(x for x in f if not x.isdigit())
        by.setdefault(t, []).append(n)
        allsz.append(n)
    past = sum(f[1] for f in frames if f[0] != "I")
    fut = sum(f[2] for f in frames)
    area = sum(f[3] for f in frames if f[0] != "I")
    sec = [sum(allsz[i:i + 24]) for i in range(0, len(allsz) - 23, 24)]
    nblk = sum(bs.values())
    r = {
        "clip": clip, "crf": crf, "kbps": sum(allsz) * 8 / 10 / 1000,
        "dy_p50": pct(dy, .5), "dy_p90": pct(dy, .9), "dy_p99": pct(dy, .99), "dy_p999": pct(dy, .999),
        "dy_max_bin_nonzero": max(i for i, v in enumerate(dy) if v) if any(dy) else 0,
        "dx_p99": pct(dx, .99), "dx_p999": pct(dx, .999),
        "bi_or_future_share": fut / (past + fut) if past + fut else 0,
        "inter_area_vs_nonI_frames": (past + fut) / area if area else 0,
        "block_mix_top": sorted(bs.items(), key=lambda kv: -kv[1])[:6],
        "block_count": nblk,
        "frame_bytes": {t: {"n": len(v), "mean": statistics.mean(v), "max": max(v), "p95": sorted(v)[int(.95 * len(v)) - 1]}
                        for t, v in by.items()},
        "frame_max_over_mean": max(allsz) / statistics.mean(allsz),
        "worst_second_over_mean": max(sec) / statistics.mean(sec) if sec else 0,
        "x264_mb": mb, "x264_coded_intra": cod.group(1) if cod else "", "x264_coded_inter": inter.group(1) if inter else "",
    }
    out.unlink()
    return r


def main():
    res = []
    crfs = dict(CRF)
    if len(sys.argv) > 1:
        crfs = json.loads(sys.argv[1])
    for c in CLIPS:
        r = one(c, crfs[c])
        res.append(r)
        print(json.dumps(r, indent=1))
    (ROOT / "workload.json").write_text(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
