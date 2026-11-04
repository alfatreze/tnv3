"""audcmp.py REF.wav TEST.wav: objective sanity numbers for a decoded audio file (NOT a perceptual measure):
overall SNR, segmental SNR (20 ms), worst 20 ms segment, high-frequency energy ratio above 15 kHz, sample offset check."""
import sys, wave
import numpy as np

def load(p):
    with wave.open(p) as w:
        d = np.frombuffer(w.readframes(w.getnframes()), np.int16).astype(np.float64)
        return d.reshape(-1, w.getnchannels()), w.getframerate()

a, sr = load(sys.argv[1]); b, _ = load(sys.argv[2])
n = min(len(a), len(b)); a, b = a[:n], b[:n]
# alignment check (decoders with delay would show a peak elsewhere)
x = a[:48000, 0]; y = b[:48000, 0]
lag = int(np.argmax([np.dot(x[1000:40000], y[1000 + k:40000 + k]) for k in range(-64, 65)])) - 64
e = a - b
snr = 10 * np.log10(np.sum(a ** 2) / max(np.sum(e ** 2), 1e-9))
seg = 960; segs = []
for i in range(0, n - seg, seg):
    s = np.sum(a[i:i + seg] ** 2); ee = np.sum(e[i:i + seg] ** 2)
    if s > 1e3 * seg: segs.append(10 * np.log10(s / max(ee, 1e-9)))
def hf(z):
    f = np.abs(np.fft.rfft(z[:, 0] if z.ndim > 1 else z)) ** 2; k = int(len(f) * 15000 / (sr / 2)); return f[k:].sum() / f.sum()
print(f"lag {lag} samples | SNR {snr:.1f} dB | segmental SNR mean {np.mean(segs):.1f} dB, worst {np.min(segs):.1f} dB | "
      f">15 kHz energy ref {hf(a):.2e} test {hf(b):.2e}")
