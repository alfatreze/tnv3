#!/usr/bin/env python3
"""Generate the synthetic block-motion benchmark as Y4M on stdout."""
import sys

w, h = 160, 96
out = sys.stdout.buffer
out.write(f"YUV4MPEG2 W{w} H{h} F24:1 Ip A1:1 C420jpeg\n".encode())
for k in range(12):
    y = bytes(
        ((max(0, min(w - 1, x + ((k % 3) * 2 if (x // 16 + y // 16) % 2 == 0 else -(k % 3) * 2))) * 13 + y * 7 + ((x // 16) * y * 3)) % 180 + 30)
        for y in range(h) for x in range(w)
    )
    chroma = bytes([110 + k % 3]) * (w * h // 4)
    out.write(b"FRAME\n" + y + chroma + chroma)
