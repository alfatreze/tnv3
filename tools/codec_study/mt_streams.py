#!/usr/bin/env python3
"""Short multi-tile lean2 test streams for sim/test_av1_hdr.py: 24 frames of the anime_dialog clip, 2x2 / 4x1 / 1x4 tiles.
  python3 tools/codec_study/mt_streams.py     (needs aomenc and ffmpeg; writes work/codec_phase0/av1mt/*_lean2.ivf and the libaom block traces)
"""
import os, pathlib, shlex, subprocess
ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / "work" / "codec_phase0" / "av1mt"
OUT.mkdir(parents=True, exist_ok=True)
y4m = ROOT / "build" / "mt_in.y4m"
subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(ROOT / "work/codec_phase0/seg/anime_dialog.mkv"), "-frames:v", "24", "-vf", "scale=800:450", "-pix_fmt", "yuv420p", str(y4m)], check=True)
s = (ROOT / "tools/codec_study/sweep.py").read_text()
g = {"AOM": {}}
exec(s[s.index("LEAN_ZERO = "):s.index('AOM["aom_lean2"]')], g)
lean2 = g["LEAN_ZERO"] + " --enable-restoration=0 --enable-rect-partitions=0 --max-partition-size=32 --gf-max-pyr-height=3 --max-reference-frames=3"
# 800x450 (the block unit handles frames up to 1024 samples wide); tiles 2x2 / 4x1 / 1x4 and forward key frames
for cols, rows, extra, name in ((1, 1, "", "t2x2"), (2, 0, "", "t4x1"), (0, 2, "", "t1x4"), (0, 0, "--enable-fwd-kf=1 --kf-min-dist=6 --kf-max-dist=12", "fwdkf")):
    ivf = OUT / f"{name}_lean2.ivf"
    subprocess.run(f"aomenc {shlex.quote(str(y4m))} --limit=24 --cpu-used=4 --end-usage=q --cq-level=30 --min-q=0 --max-q=63 --lag-in-frames=16 "
                   f"--tile-columns={cols} --tile-rows={rows} {extra} {lean2} -o {shlex.quote(str(ivf))}", shell=True, check=True, capture_output=True, input=b"y\n")
    subprocess.run([str(ROOT / "build/av1trace"), str(ivf)], env=dict(os.environ, AV1_BLK_TRACE=str(ivf.with_suffix(".blk"))),
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    print(name, ivf.stat().st_size)
