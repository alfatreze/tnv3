#!/usr/bin/env python3
"""Write the generated 2400-frame static B-encoder benchmark to stdout."""
import sys
w, h = 160, 96
y = bytes((x * 2 + y * 3) % 256 for y in range(h) for x in range(w))
c = bytes([128]) * (w * h // 4)
out = sys.stdout.buffer
out.write(f"YUV4MPEG2 W{w} H{h} F24:1 Ip A1:1 C420jpeg\n".encode())
for _ in range(2400):
    out.write(b"FRAME\n" + y + c + c)
