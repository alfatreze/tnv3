#!/usr/bin/env python3
"""Small streams for the reconstruction-buffer test (V-087): 3 frames of 200x120 (not a multiple of the superblock size), 64x64 and 128x128
superblocks, lean tools.   python3 tools/codec_study/rbuf_streams.py   -> work/codec_phase0/av1rb/{sb64,sb128}.ivf (+ traces under build/)"""
import os, pathlib, shlex, subprocess
ROOT = pathlib.Path(__file__).resolve().parents[2]
s = (ROOT / "tools/codec_study/sweep.py").read_text()
g = {"AOM": {}}
exec(s[s.index("LEAN_ZERO = "):s.index('AOM["aom_lean2"]')], g)
out = ROOT / "work/codec_phase0/av1rb"
out.mkdir(parents=True, exist_ok=True)
y4m = ROOT / "build" / "rb_in.y4m"
subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(ROOT / "work/codec_phase0/seg/anime_dialog.mkv"), "-frames:v", "3", "-vf", "scale=200:120", "-pix_fmt", "yuv420p", str(y4m)], check=True)
for name, sb, extra in (("sb64", 64, "--enable-rect-partitions=1"), ("sb128", 128, "")):
    ivf = out / f"{name}.ivf"
    lean = g["LEAN_ZERO"] + f" --enable-restoration=0 --gf-max-pyr-height=3 --max-reference-frames=3 --sb-size={sb} --enable-angle-delta=1 {extra}"
    subprocess.run(f"aomenc {shlex.quote(str(y4m))} --limit=3 --cpu-used=3 --end-usage=q --cq-level=36 --min-q=0 --max-q=63 --lag-in-frames=0 {lean} -o {shlex.quote(str(ivf))}",
                   shell=True, check=True, capture_output=True, input=b"y\n")
    env = dict(os.environ, AV1_INTRA_TRACE=str(ROOT / "build" / f"rb_{name}_intra.txt"), AV1_LF_TRACE=str(ROOT / "build" / f"rb_{name}_lf.bin"))
    subprocess.run([str(ROOT / "build/av1trace"), str(ivf)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    print(name, ivf.stat().st_size)
