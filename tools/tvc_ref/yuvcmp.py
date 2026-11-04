"""yuvcmp.py A.y4m B.y4m [N]: per-frame PSNR of Y, U, V between two 4:2:0 y4m files (first N frames)."""
import sys, math
import numpy as np

def frames(path, n):
    with open(path, "rb") as f:
        hdr = f.readline().split()
        w = int(next(t[1:] for t in hdr if t.startswith(b"W"))); h = int(next(t[1:] for t in hdr if t.startswith(b"H")))
        cw, ch = (w + 1) // 2, (h + 1) // 2
        for _ in range(n):
            if not f.readline(): return
            y = np.frombuffer(f.read(w * h), np.uint8).reshape(h, w)
            u = np.frombuffer(f.read(cw * ch), np.uint8).reshape(ch, cw)
            v = np.frombuffer(f.read(cw * ch), np.uint8).reshape(ch, cw)
            yield y, u, v

def psnr(a, b):
    m = np.mean((a.astype(float) - b.astype(float)) ** 2)
    return 99.0 if m == 0 else 10 * math.log10(255 ** 2 / m)

n = int(sys.argv[3]) if len(sys.argv) > 3 else 1 << 30
for i, (fa, fb) in enumerate(zip(frames(sys.argv[1], n), frames(sys.argv[2], n))):
    print(i, " ".join(f"{psnr(a, b):6.2f}" for a, b in zip(fa, fb)), f"mean Y {fa[0].mean():.1f} vs {fb[0].mean():.1f}")
