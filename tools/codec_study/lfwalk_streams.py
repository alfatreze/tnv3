#!/usr/bin/env python3
"""Small intra streams with the loop filter ON for the deblocking walker test (V-093): different quality (filter level), sharpness and sizes, lean tool set, 64x64 superblocks.
   python3 tools/codec_study/lfwalk_streams.py     -> work/codec_phase0/av1lfw/NAME.ivf and build/lfw/NAME_lf.bin (libaom loop-filter trace, AV1_LF_TRACE)"""
import os, pathlib, shlex, subprocess
ROOT = pathlib.Path(__file__).resolve().parents[2]
s = (ROOT / "tools/codec_study/sweep.py").read_text()
g = {"AOM": {}}
exec(s[s.index("LEAN_ZERO = "):s.index('AOM["aom_lean2"]')], g)
out = ROOT / "work/codec_phase0/av1lfw"
tr = ROOT / "build/lfw"
out.mkdir(parents=True, exist_ok=True); tr.mkdir(parents=True, exist_ok=True)
src = ROOT / "work/codec_phase0/seg/anime_dialog.mkv"
# name: (width, height, cq, sharpness, seek)
CASES = {
    "q12": (208, 112, 12, 0, 2), "q30": (208, 112, 30, 0, 3), "q45": (208, 112, 45, 0, 4), "q58": (208, 112, 58, 0, 5),
    "s96": (96, 64, 50, 0, 10), "sh3": (208, 112, 40, 3, 6), "sh7": (208, 112, 40, 7, 7), "odd": (202, 118, 40, 0, 8), "w400": (400, 176, 40, 2, 9),
}
for name, (w, h, cq, sharp, ss) in CASES.items():
    y4m = ROOT / "build" / f"lfw_{name}.y4m"
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-ss", str(ss), "-i", str(src), "-frames:v", "2", "-vf", f"scale={w}:{h}", "-pix_fmt", "yuv420p", str(y4m)], check=True)
    ivf = out / f"{name}.ivf"
    lean = g["LEAN_ZERO"] + f" --enable-restoration=0 --sb-size=64 --enable-rect-partitions=1 --enable-angle-delta=1 --sharpness={sharp}"
    subprocess.run(f"aomenc {shlex.quote(str(y4m))} --limit=2 --cpu-used=3 --end-usage=q --cq-level={cq} --min-q=0 --max-q=63 --lag-in-frames=0 --kf-max-dist=0 {lean} -o {shlex.quote(str(ivf))}",
                   shell=True, check=True, capture_output=True, input=b"y\n")
    env = dict(os.environ, AV1_LF_TRACE=str(tr / f"{name}_lf.bin"))
    subprocess.run([str(ROOT / "build/av1trace"), str(ivf)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    print(name, ivf.stat().st_size, "bytes")
