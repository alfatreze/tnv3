#!/usr/bin/env python3
"""Find corrupt frames in the 400x225 intermediate of the damaged episode.

The file has silent damage (magenta or garbage slices) that the decoder does not always report, so
every frame is scored from the pixels:
  * chroma row score: largest deviation of a row's mean U or V from the mean of the rows around it
    (a zeroed or garbage slice is a full-width band of wrong chroma)
  * temporal score: largest row-mean of |frame - (previous + next) / 2| where previous and next agree
    (a single bad frame in the middle of a good sequence)
Writes work/codec_phase0/slime_glitch.npy (per-frame score) and prints clean 10 s windows.
"""
import pathlib, subprocess, sys
import numpy as np

ROOT = pathlib.Path(__file__).resolve().parents[2] / "work" / "codec_phase0"
W, H = 400, 225
CH = (H + 1) // 2                      # 225 is odd: chroma has 113 rows
FS = W * H + 2 * (W // 2) * CH
p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", str(ROOT / "slime_400.mkv"), "-f", "rawvideo", "-pix_fmt", "yuv420p",
                      "-"], stdout=subprocess.PIPE)
scores, chroma, temporal = [], [], []
prev = cur = None
n = 0


def chroma_score(f):
    s = 0.0
    for off, (w, h) in ((W * H, (W // 2, (H + 1) // 2)), (W * H + (W // 2) * ((H + 1) // 2), (W // 2, (H + 1) // 2))):
        pl = f[off:off + w * h].reshape(h, w).astype(np.float32).mean(axis=1)
        k = np.ones(9, np.float32) / 9
        sm = np.convolve(np.pad(pl, 4, mode="edge"), k, mode="valid")
        s = max(s, float(np.abs(pl - sm).max()))
    return s


def luma_rows(f):
    return f[:W * H].reshape(H, W).astype(np.float32)


frames = []
while True:
    b = p.stdout.read(FS)
    if len(b) < FS:
        break
    f = np.frombuffer(b, np.uint8)
    chroma.append(chroma_score(f))
    frames.append(luma_rows(f))
    if len(frames) == 3:
        a, c, d = frames
        m = (a + d) / 2
        agree = np.abs(a - d).mean(axis=1)
        dev = np.abs(c - m).mean(axis=1)
        temporal.append(float(np.max(np.where(agree < 4, dev, 0))))
        frames.pop(0)
        if len(temporal) == 1:
            temporal.insert(0, 0.0)
    n += 1
temporal.append(0.0)
chroma = np.array(chroma)
temporal = np.array(temporal[:len(chroma)] + [0.0] * (len(chroma) - len(temporal)))
np.save(ROOT / "slime_glitch_chroma.npy", chroma)
np.save(ROOT / "slime_glitch_temporal.npy", temporal)
print("frames", n, "chroma p50/p99/max", np.percentile(chroma, [50, 99, 100]), "temporal p50/p99/max",
      np.percentile(temporal, [50, 99, 100]))
