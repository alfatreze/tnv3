#!/usr/bin/env python3
"""One sequential decode of the damaged anime episode into two lossless FFV1 intermediates.

The source has holes and a broken seek index, so seeking inside it is unreliable.  Decoding it once
from the start is reliable; the intermediates (800x450 and 400x225, both scaled straight from the
decoded 10-bit picture) can then be cut by exact time.  Decoder messages are written to
work/codec_phase0/slime_decode.log with the stream time of each message.
"""
import pathlib, re, subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0"
SRC = ROOT / "slime_clean.mkv"
fc = ("[0:v]fps=24,split=2[a][b];[a]scale=800:450:flags=lanczos,format=yuv420p[x];"
      "[b]scale=400:225:flags=lanczos,format=yuv420p[y]")
p = subprocess.Popen(["ffmpeg", "-v", "error", "-stats", "-stats_period", "1", "-y", "-i", str(SRC), "-filter_complex", fc,
                      "-map", "[x]", "-c:v", "ffv1", "-level", "3", "-g", "1", str(ROOT / "slime_800.mkv"),
                      "-map", "[y]", "-c:v", "ffv1", "-level", "3", "-g", "1", str(ROOT / "slime_400.mkv")],
                     stderr=subprocess.PIPE, text=True, bufsize=1)
buf, t = "", 0.0
with open(ROOT / "slime_decode.log", "w") as log:
    while True:
        ch = p.stderr.read(1)
        if not ch:
            break
        if ch in "\r\n":
            m = re.search(r"time=(\d+):(\d+):([\d.]+)", buf)
            if m:
                t = int(m[1]) * 3600 + int(m[2]) * 60 + float(m[3])
            elif buf.strip():
                log.write(f"{t:8.1f} {buf}\n")
            buf = ""
        else:
            buf += ch
p.wait()
print("done rc", p.returncode, "last time", t)
