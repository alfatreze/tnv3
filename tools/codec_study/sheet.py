#!/usr/bin/env python3
"""Contact sheet of a source video, to choose study clips by eye.

usage: sheet.py <source> <name> <t1> <t2> ... (up to 9 times in seconds)
"""
import subprocess, pathlib, sys

SRC = pathlib.Path(sys.argv[1])
NAME = sys.argv[2]
times = [int(t) for t in sys.argv[3:12]]
OUT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0" / "sheet" / NAME
OUT.mkdir(parents=True, exist_ok=True)
for t in times:
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-ss", str(t), "-i", str(SRC), "-map", "0:v:0", "-frames:v", "1",
                    "-vf", "scale=480:-2", str(OUT / f"f{t:04d}.png")], check=True)
args = ["ffmpeg", "-v", "error", "-y"]
for t in times:
    args += ["-i", str(OUT / f"f{t:04d}.png")]
n = len(times)
layout = "|".join(f"{'+'.join(['w0'] * (i % 3)) or '0'}_{'+'.join(['h0'] * (i // 3)) or '0'}" for i in range(n))
args += ["-filter_complex", f"xstack=inputs={n}:layout={layout}", str(OUT / "sheet.png")]
subprocess.run(args, check=True)
print(OUT / "sheet.png")
