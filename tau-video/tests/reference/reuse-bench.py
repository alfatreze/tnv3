#!/usr/bin/env python3
"""Write a generated 2400-frame repeat/change sequence as Y4M to stdout."""
import sys
w, h = 160, 96
chroma = bytes([128]) * (w * h // 4)
out = sys.stdout.buffer
out.write(f"YUV4MPEG2 W{w} H{h} F24:1 Ip A1:1 C420jpeg\n".encode())
for i in range(2400):
    out.write(b"FRAME\n" + bytes([80 + ((i // 2) * 5) % 80]) * (w * h) + chroma + chroma)
