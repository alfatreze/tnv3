#!/usr/bin/env python3
"""Reference current encode_film.py frame grouping; writes concatenated groups to stdout."""
import sys

per_group = int(sys.argv[1])
stream = sys.stdin.buffer
header = stream.readline()
fields = header.split()
width = int(next(value[1:] for value in fields if value.startswith(b'W')))
height = int(next(value[1:] for value in fields if value.startswith(b'H')))
frame_size = width * height + 2 * ((width + 1) // 2) * ((height + 1) // 2)
frames = 0
while True:
    metadata = stream.readline()
    if not metadata:
        break
    payload = stream.read(frame_size)
    if len(payload) < frame_size:
        break
    if frames % per_group == 0:
        sys.stdout.buffer.write(header)
    sys.stdout.buffer.write(metadata)
    sys.stdout.buffer.write(payload)
    frames += 1
