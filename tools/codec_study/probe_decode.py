#!/usr/bin/env python3
"""Decode 10 s windows of a source and report decoder errors, to find clean study clips.

usage: probe_decode.py <source> <t1> <t2> ...
"""
import subprocess, sys

src = sys.argv[1]
for t in sys.argv[2:]:
    p = subprocess.run(["ffmpeg", "-v", "error", "-ss", t, "-t", "10", "-i", src, "-map", "0:v:0", "-f", "null", "-"],
                       capture_output=True, text=True)
    errs = [ln for ln in p.stderr.splitlines() if "hevc" in ln or "Invalid" in ln or "error" in ln.lower()]
    print(t, "errors:", len(errs), errs[:2])
