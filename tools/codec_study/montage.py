#!/usr/bin/env python3
"""A 3x3 grid of frames (every 27th) from a clip's 400-wide reference, for a visual damage check.

usage: montage.py <clip>   ->  work/codec_phase0/sheet/<clip>_grid.png
"""
import pathlib, subprocess, sys

ROOT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0"
c = sys.argv[1]
out = ROOT / "sheet" / f"{c}_grid.png"
sel = "+".join(f"eq(n\\,{n})" for n in range(5, 240, 26))
subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(ROOT / "ref" / f"{c}_400.y4m"), "-vf",
                f"select={sel},scale=320:-2,tile=3x3", "-frames:v", "1", str(out)], check=True)
print(out)
