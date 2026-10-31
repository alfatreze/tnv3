"""R0 transform study: held-out version of transform_study.py (same transforms and estimator), plus a deblocked DCT.

Changes against the first study (see docs/TNV3_ADVERSARIAL_REVIEW_2026-10-27.md, M1):
  * the lapped filter and the deblocking parameters are fitted on TRAIN clips only (frame 30 of dialog, anime_wide,
    bw_garden) and every number is reported on six OTHER clips (frames 60 and 180 of dark, action, anime_dialog,
    anime_motion, bw_closeup, bw_room): 12 held-out images from three content types;
  * frequency classes are no longer capped at 11 (the cap handicapped the 16x16 DCT);
  * `dct8db`: plain 8x8 DCT with a simple edge filter on the reconstruction (H.264-like weak filter, strength tied to
    the quantiser step), so the lapped transform is compared against a deblocked DCT, not a bare one;
  * SSIM (8x8 windows, stride 4, luma) next to PSNR-Y: SSIM sees blocking, PSNR barely does.
Still luma only, still an entropy ESTIMATE (ideal per-image conditional entropy), still no spatial intra prediction.
Usage: python3 transform_study2.py
"""
import sys, math, pathlib, subprocess
import numpy as np
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import transform_study as ts

REF = pathlib.Path("<study-worktree>/work/codec_phase0/ref")
TRAIN = [("dialog", 30), ("anime_wide", 30), ("bw_garden", 30)]
TEST = [(c, f) for c in ("dark", "action", "anime_dialog", "anime_motion", "bw_closeup", "bw_room") for f in (60, 180)]

def y4m_size(path):
    with open(path, "rb") as fh:
        hdr = fh.readline().decode()
    w = int(next(t[1:] for t in hdr.split() if t.startswith("W")))
    h = int(next(t[1:] for t in hdr.split() if t.startswith("H")))
    return w, h

def load(clip, idx):
    p = REF / f"{clip}_800.y4m"; w, h = y4m_size(p)
    r = subprocess.run(["ffmpeg", "-v", "error", "-i", str(p), "-vf", f"select=eq(n\\,{idx})", "-frames:v", "1",
                        "-pix_fmt", "gray", "-f", "rawvideo", "-"], capture_output=True, check=True)
    a = np.frombuffer(r.stdout, np.uint8).reshape(h, w).astype(np.float64)
    H = -(-h // 32) * 32
    return np.pad(a, ((0, H - h), (0, 0)), mode="edge"), h

# ---- estimator fix: no class cap ---------------------------------------------------------------------------------
def bits_block(Q, n):
    Q = ts.dc_dpcm(Q, n)
    a = np.abs(Q); ii, jj = np.indices(Q.shape); u, v = ii % n, jj % n
    left = ts.shift(a, 0, n); up = ts.shift(a, n, 0)
    inu = np.where(u > 0, ts.shift(a, 1, 0), 0.0); inv = np.where(v > 0, ts.shift(a, 0, 1), 0.0)
    m = left + up + 2 * (inu + inv)
    cls = np.minimum(u + v, 15)
    ctx = cls * 12 + ts.qlog(m)
    sym = np.clip(Q, -20, 20).astype(np.int64)
    esc = float(np.sum(np.where(a > 20, 2 * np.log2(a - 19 + 1) + 1, 0.0)))
    return ts.cond_entropy(sym, ctx, esc)

# ---- simple deblocking of 8x8 edges (decoder-side, intra here) ---------------------------------------------------
def deblock(img, step, k):
    ka, kb, kt = k
    alpha, beta, tc = ka * step + 1, kb * step + 1, kt * step
    out = img.copy()
    for axis in (1, 0):
        a = out if axis == 1 else out.T.copy()
        for b in range(8, a.shape[1], 8):
            p1, p0, q0, q1 = a[:, b - 2], a[:, b - 1], a[:, b], a[:, b + 1]
            on = (np.abs(p0 - q0) < alpha) & (np.abs(p1 - p0) < beta) & (np.abs(q1 - q0) < beta)
            d = np.clip(((q0 - p0) * 4 + (p1 - q1)) / 8.0, -tc, tc) * on
            a[:, b - 1] = p0 + d; a[:, b] = q0 - d
        out = a if axis == 1 else a.T
    return out

# ---- SSIM: 8x8 windows, stride 4 ---------------------------------------------------------------------------------
def ssim(x, y):
    C1, C2 = (0.01 * 255) ** 2, (0.03 * 255) ** 2
    H, W = x.shape; vals = []
    xs = np.lib.stride_tricks.sliding_window_view(x, (8, 8))[::4, ::4]
    ys = np.lib.stride_tricks.sliding_window_view(y, (8, 8))[::4, ::4]
    mx, my = xs.mean((-1, -2)), ys.mean((-1, -2))
    vx, vy = xs.var((-1, -2)), ys.var((-1, -2))
    cxy = ((xs - mx[..., None, None]) * (ys - my[..., None, None])).mean((-1, -2))
    s = ((2 * mx * my + C1) * (2 * cxy + C2)) / ((mx ** 2 + my ** 2 + C1) * (vx + vy + C2))
    return float(s.mean())

def run(name, img, h, step, P=None, wt=None, dbk=None):
    if name.startswith("dct"):
        n = 16 if name.startswith("dct16") else 8
        x = ts.pot_apply(img, P, n) if "pot" in name else img
        co = ts.blocks_fwd(x - 128.0, n)
        q = ts.quant(co, step); rec = ts.blocks_inv(ts.dequant(q, step), n) + 128.0
        if "pot" in name: rec = ts.pot_apply(rec, P, n, inverse=True)
        if name == "dct8db": rec = deblock(rec, step, dbk)
        bits = bits_block(q, n)
    else:
        lv = int(name[-1])
        co = ts.dwt2(img - 128.0, lv)
        q = ts.quant(co, step * wt); rec = ts.idwt2(ts.dequant(q, step * wt), lv) + 128.0
        bits = ts.bits_wavelet(q, lv)
    rec = np.clip(np.round(rec), 0, 255)
    mse = np.mean((rec[:h] - img[:h]) ** 2)
    return bits / 8.0, 10 * math.log10(255.0 ** 2 / mse), ssim(rec[:h], img[:h])

def fit_deblock(train):
    best = None
    for ka in (1.0, 1.5, 2.5):
        for kb in (0.25, 0.5, 1.0):
            for kt in (0.25, 0.5, 1.0):
                gain = 0.0
                for im, h in train:
                    for s in (10, 20, 30):
                        _, p0, _ = run("dct8", im, h, s)
                        _, p1, _ = run("dct8db", im, h, s, dbk=(ka, kb, kt))
                        gain += p1 - p0
                if best is None or gain > best[0]:
                    best = (gain, (ka, kb, kt))
    return best

def at(pts, k, target):
    pts = sorted(pts, key=lambda p: p[k])
    for a, b in zip(pts, pts[1:]):
        if a[k] <= target <= b[k] and a[k] != b[k]:
            t = (target - a[k]) / (b[k] - a[k])
            return math.exp(math.log(a[0]) * (1 - t) + math.log(b[0]) * t)
    return None

if __name__ == "__main__":
    train = [load(c, f) for c, f in TRAIN]
    test = [load(c, f) for c, f in TEST]
    th, _ = ts.fit_pot([im for im, _ in train], 8)
    P8 = ts.pot_matrix(th, 8)
    g, dbk = fit_deblock(train)
    print("lapped filter angles (fitted on TRAIN):", np.round(th, 3))
    print(f"deblock params (fitted on TRAIN, PSNR gain sum {g:.2f} dB): alpha={dbk[0]}*step beta={dbk[1]}*step tc={dbk[2]}*step")
    cfgs = [("dct8", None), ("dct8db", None), ("dct16", None), ("dct8pot", P8), ("wav4", None), ("wav5", None)]
    steps = [4, 5.5, 7.5, 10, 14, 20, 28, 40]
    res = {n: [] for n, _ in cfgs}
    for im, h in test:
        wts = {lv: ts.wavelet_weights(im.shape, lv) for lv in (4, 5)}
        for name, P in cfgs:
            wt = wts[int(name[-1])] if name.startswith("wav") else None
            res[name].append([run(name, im, h, s, P, wt, dbk) for s in steps])
    labels = [f"{c}@{f}" for c, f in TEST]
    for k, targets, unit in ((1, (36, 38, 40), "PSNR-Y dB"), (2, (0.95, 0.97, 0.98), "SSIM")):
        for t in targets:
            print(f"\nHELD-OUT luma estimate, kB per frame at {unit} {t} (12 images; ratio = geo-mean vs dct8 over images where all configs have a value)")
            vals = {n: [at(p, k, t) for p in res[n]] for n, _ in cfgs}
            ok = [i for i in range(len(TEST)) if all(vals[n][i] for n, _ in cfgs)]
            base = math.exp(sum(math.log(vals["dct8"][i]) for i in ok) / len(ok)) if ok else None
            for n, _ in cfgs:
                if ok:
                    gm = math.exp(sum(math.log(vals[n][i]) for i in ok) / len(ok))
                    print(f"  {n:8s} gm {gm / 1000:7.2f} kB  x{gm / base:.3f}   (n={len(ok)})")
            groups = {"live": ("dark", "action"), "anime": ("anime_dialog", "anime_motion"), "bw": ("bw_closeup", "bw_room")}
            line = []
            for gname, members in groups.items():
                idx = [i for i in ok if TEST[i][0] in members]
                if idx:
                    r = {n: math.exp(sum(math.log(vals[n][i] / vals["dct8"][i]) for i in idx) / len(idx)) for n in ("dct8db", "dct8pot", "wav5", "dct16")}
                    line.append(f"{gname}: db {r['dct8db']:.3f} lapped {r['dct8pot']:.3f} wav5 {r['wav5']:.3f} dct16 {r['dct16']:.3f}")
            print("  per group (ratio to dct8): " + " | ".join(line))
