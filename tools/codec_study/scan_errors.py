#!/usr/bin/env python3
"""Decode a whole source once and attribute decoder errors to 10 s buckets (via ffmpeg's -stats time).

Prints the error-free windows, to pick study clips from a damaged file.
usage: scan_errors.py <source>
"""
import re, subprocess, sys

src = sys.argv[1]
p = subprocess.Popen(["ffmpeg", "-v", "error", "-stats", "-stats_period", "0.5", "-i", src, "-map", "0:v:0",
                      "-vf", "scale=160:-2", "-f", "null", "-"], stderr=subprocess.PIPE, text=True, bufsize=1)
buf, t, bad = "", 0.0, {}
total = 0
while True:
    ch = p.stderr.read(1)
    if not ch:
        break
    if ch in "\r\n":
        m = re.search(r"time=(\d+):(\d+):([\d.]+)", buf)
        if m:
            t = int(m[1]) * 3600 + int(m[2]) * 60 + float(m[3])
        elif buf.strip() and "hevc" in buf:
            bad[int(t // 10)] = bad.get(int(t // 10), 0) + 1
            total += 1
        buf = ""
    else:
        buf += ch
p.wait()
print("total error lines", total, "last time", t)
clean = [b * 10 for b in range(int(t // 10)) if b not in bad and b + 1 not in bad]
print("error buckets (10 s):", sorted(bad)[:80], "...")
print("clean 10 s starts:", clean[:60])
