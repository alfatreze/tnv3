#!/usr/bin/env python3
"""Exercise spatial luma measurement and strict flag boundaries with Y4M frames."""
import importlib.util
import pathlib
import tempfile

import numpy as np

root = pathlib.Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location('encoder_lab', root / 'tools/encoder_lab.py')
lab = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lab)
w, h = 400, 176

def write(path, frames):
    with path.open('wb') as f:
        f.write(f'YUV4MPEG2 W{w} H{h} F24:1 Ip C420jpeg\n'.encode())
        for y in frames:
            f.write(b'FRAME\n'); f.write(y.tobytes()); f.write(bytes([128]) * (w * h // 2))

zero = np.zeros((h, w), dtype=np.uint8)
flag = zero.copy(); flag[-40:] = 10     # bottom MSE 100, top MSE 0
boundary = zero.copy(); boundary[:100] = 10; boundary[-40:] = 10  # ratio 1
with tempfile.TemporaryDirectory() as td:
    a, b = pathlib.Path(td) / 'a.y4m', pathlib.Path(td) / 'b.y4m'
    write(a, [zero, zero, zero]); write(b, [zero, flag, boundary])
    _, rows = lab.quality_y(a, b, 3)
    assert [r['flagged'] for r in rows] == [False, True, False]
    assert rows[1]['top_mse'] == 0 and rows[1]['bottom_mse'] == 100
    assert rows[2]['top_mse'] == rows[2]['bottom_mse'] == 100
print('PASS spatial luma regions, frame order and diagnostic threshold')
