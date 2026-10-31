#!/usr/bin/env python3
"""Merge the 'size change' group means of the three metric reports into one table.

usage: combine.py <report prefix> [name filter]   (reads work/codec_phase0/<prefix>_{vmaf,ssim,psnr}.md)
"""
import pathlib, sys

ROOT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0"
prefix = sys.argv[1] if len(sys.argv) > 1 else "report"
flt = sys.argv[2] if len(sys.argv) > 2 else ""
rows, cols = {}, None
for m in ("vmaf", "ssim", "psnr"):
    txt = (ROOT / f"{prefix}_{m}.md").read_text()
    sec = txt.split("## Size change")[1].split("## Resolution")[0]
    for ln in sec.splitlines():
        c = [x.strip() for x in ln.strip("|").split("|")]
        if c and c[0] == "config":
            cols = c
        elif len(c) > 6 and c[1] in ("800", "400") and not c[0].startswith("-"):
            g = {k: v.strip("*") for k, v in zip(cols, c)}
            rows.setdefault((c[0], c[1]), {})[m] = g
print("| config | res | VMAF live / anime / bw / all | SSIM all | PSNR-Y all |")
print("|---|---|---|---|---|")
for (cfg, res), v in rows.items():
    if flt and flt not in cfg:
        continue
    a = v.get("vmaf", {})
    print(f"| {cfg} | {res} | {a.get('live mean', '-')} / {a.get('anime mean', '-')} / {a.get('bw mean', '-')} / **{a.get('all mean', '-')}** "
          f"| {v.get('ssim', {}).get('all mean', '-')} | {v.get('psnr', {}).get('all mean', '-')} |")
