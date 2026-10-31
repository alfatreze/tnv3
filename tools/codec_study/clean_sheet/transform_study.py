"""Intra transform study for a clean-sheet Pocket codec (luma only, numpy only).

Compares, with ONE entropy estimator and uniform quantisation (orthonormal transforms, flat step):
  dct8, dct16, dct8+pot (time-domain lapped pre/post filter, 4x4 orthogonal V fitted on training frames),
  dct16+pot, wavelet 9/7 (CDF lifting, 3..5 levels).
Bits are an ESTIMATE: conditional empirical entropy H(q | frequency class, neighbourhood context) plus an
escape cost; the same machinery for every transform, but the neighbourhoods differ (block transforms use
in-block and neighbour-block magnitudes, the wavelet uses same-subband neighbours), so context quality is
part of what is measured.  Relative numbers matter, absolute ones do not (no chroma, no headers).
Usage: python3 transform_study.py CLIP.y4m [CLIP.y4m ...]
"""
import sys, subprocess, math
import numpy as np

def load_frames(path, idx):
    w, h = 800, 334
    out = []
    for i in idx:
        r = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-vf", f"select=eq(n\\,{i})", "-frames:v", "1",
                            "-pix_fmt", "gray", "-f", "rawvideo", "-"], capture_output=True)
        a = np.frombuffer(r.stdout, np.uint8).reshape(h, w).astype(np.float64)
        a = np.pad(a, ((0, 352 - h), (0, 0)), mode="edge")  # 352 = 11*32, 800 = 25*32
        out.append(a)
    return out

def dct_matrix(n):
    k = np.arange(n)[:, None]; m = np.arange(n)[None, :]
    c = np.cos(np.pi * (2 * m + 1) * k / (2 * n)) * math.sqrt(2.0 / n)
    c[0] /= math.sqrt(2)
    return c

def blocks_fwd(img, n):
    c = dct_matrix(n); H, W = img.shape
    b = img.reshape(H // n, n, W // n, n).transpose(0, 2, 1, 3)
    t = np.einsum("ij,abjk,lk->abil", c, b, c)
    return t.transpose(0, 2, 1, 3).reshape(H, W)

def blocks_inv(co, n):
    c = dct_matrix(n); H, W = co.shape
    b = co.reshape(H // n, n, W // n, n).transpose(0, 2, 1, 3)
    t = np.einsum("ji,abjk,kl->abil", c, b, c)
    return t.transpose(0, 2, 1, 3).reshape(H, W)

# ---- time-domain lapped pre/post filter (POT): P = W diag(I,V) W, W = [[I,J],[J,-I]]/sqrt2 -------------------
def givens(theta):
    n = 4; V = np.eye(n); idx = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]
    for t, (i, j) in zip(theta, idx):
        G = np.eye(n); G[i, i] = G[j, j] = math.cos(t); G[i, j] = -math.sin(t); G[j, i] = math.sin(t)
        V = G @ V
    return V

def pot_matrix(theta, n):
    h = n // 2; I = np.eye(h); J = np.eye(h)[::-1]
    W = np.block([[I, J], [J, -I]]) / math.sqrt(2)
    D = np.eye(n); D[h:, h:] = givens(theta) if h == 4 else np.eye(h)
    return W @ D @ W

def pot_apply(img, P, n, inverse=False):
    M = P.T if inverse else P
    h = n // 2; out = img.copy()
    for axis in (1, 0):
        a = out if axis == 1 else out.T
        a = a.copy()
        for b in range(n, a.shape[1], n):          # boundary between block b/n-1 and b/n
            seg = a[:, b - h:b + h]
            a[:, b - h:b + h] = seg @ M.T
        out = a if axis == 1 else a.T
    return out

# ---- CDF 9/7 lifting wavelet (even sizes, reflect extension) ---------------------------------------------------
A, B, G, D = -1.586134342059924, -0.052980118572961, 0.882911075530934, 0.443506852043971
K = 1.230174104914001

def _lift(x, c, first):  # x: (..., n) ; update odd from even (first) or even from odd
    x = x.copy()
    if first:  # odd += c*(even[i]+even[i+1])
        e = x[..., 0::2]; e2 = np.concatenate([e[..., 1:], e[..., -1:]], -1)
        x[..., 1::2] += c * (e + e2)
    else:      # even += c*(odd[i-1]+odd[i])
        o = x[..., 1::2]; o0 = np.concatenate([o[..., :1], o[..., :-1]], -1)
        x[..., 0::2] += c * (o0 + o)
    return x

def dwt1(x):
    x = _lift(x, A, True); x = _lift(x, B, False); x = _lift(x, G, True); x = _lift(x, D, False)
    return np.concatenate([x[..., 0::2] * (1 / K) * 1, x[..., 1::2] * K / 2 * 2], -1) if False else \
        np.concatenate([x[..., 0::2] / K, x[..., 1::2] * K], -1)

def idwt1(y):
    n = y.shape[-1]; x = np.empty_like(y)
    x[..., 0::2] = y[..., :n // 2] * K; x[..., 1::2] = y[..., n // 2:] / K
    x = _lift(x, -D, False); x = _lift(x, -G, True); x = _lift(x, -B, False); x = _lift(x, -A, True)
    return x

def dwt2(img, levels):
    out = img.copy(); h, w = img.shape
    for _ in range(levels):
        sub = out[:h, :w]
        sub = dwt1(sub); sub = dwt1(sub.T).T
        out[:h, :w] = sub; h //= 2; w //= 2
    return out

def idwt2(co, levels):
    out = co.copy(); H, W = co.shape
    for l in range(levels - 1, -1, -1):
        h, w = H >> l, W >> l
        sub = out[:h, :w]
        sub = idwt1(sub.T).T; sub = idwt1(sub)
        out[:h, :w] = sub
    return out

def wavelet_weights(shape, levels):
    """Per-coefficient quantiser step multiplier = 1/L2 norm of the synthesis basis (so MSE is flat)."""
    H, W = shape; wt = np.ones(shape)
    # measure synthesis gain per subband numerically with an impulse per subband
    for lev in range(levels):
        h, w = H >> (lev + 1), W >> (lev + 1)
        for (r0, c0) in [(0, w), (h, 0), (h, w)]:
            z = np.zeros(shape); z[r0 + h // 2, c0 + w // 2] = 1.0
            g = math.sqrt(np.sum(idwt2(z, levels) ** 2))
            wt[r0:r0 + h, c0:c0 + w] = 1.0 / g
    h, w = H >> levels, W >> levels
    z = np.zeros(shape); z[h // 2, w // 2] = 1.0
    wt[:h, :w] = 1.0 / math.sqrt(np.sum(idwt2(z, levels) ** 2))
    return wt

# ---- entropy estimate --------------------------------------------------------------------------------------------
def cond_entropy(sym, ctx, esc_bits):
    """sum over contexts of empirical entropy of sym (ints, clipped) + escape bits."""
    bits = esc_bits
    key = ctx.astype(np.int64) * 64 + (sym + 20)
    cnt = np.bincount(key.ravel(), minlength=(int(ctx.max()) + 1) * 64).reshape(-1, 64).astype(np.float64)
    tot = cnt.sum(1, keepdims=True)
    with np.errstate(divide="ignore", invalid="ignore"):
        p = np.where(cnt > 0, cnt / tot, 1.0)
        bits += float(-(cnt * np.log2(p)).sum())
    return bits

def qlog(m):
    return np.minimum(11, np.floor(np.log2(1.0 + m) * 2.0)).astype(np.int64)

def shift(a, di, dj):
    o = np.zeros_like(a)
    H, W = a.shape
    si = slice(max(di, 0), H + min(di, 0)); sj = slice(max(dj, 0), W + min(dj, 0))
    ti = slice(max(-di, 0), H + min(-di, 0)); tj = slice(max(-dj, 0), W + min(-dj, 0))
    o[si, sj] = a[ti, tj]
    return o

def dc_dpcm(Q, n):
    """Replace each block DC by its residual against the MED (LOCO-I) predictor of left/up/up-left block DCs."""
    D = Q[::n, ::n].copy(); R = D.copy()
    L = np.zeros_like(D); U = np.zeros_like(D); UL = np.zeros_like(D)
    L[:, 1:] = D[:, :-1]; U[1:, :] = D[:-1, :]; UL[1:, 1:] = D[:-1, :-1]
    mx = np.maximum(L, U); mn = np.minimum(L, U)
    pred = np.where(UL >= mx, mn, np.where(UL <= mn, mx, L + U - UL))
    R = D - pred
    out = Q.copy(); out[::n, ::n] = R
    return out

def bits_block(Q, n):
    Q = dc_dpcm(Q, n)
    H, W = Q.shape; a = np.abs(Q)
    ii, jj = np.indices(Q.shape); u, v = ii % n, jj % n
    left = shift(a, 0, n); up = shift(a, n, 0)
    inu = np.where(u > 0, shift(a, 1, 0), 0.0); inv = np.where(v > 0, shift(a, 0, 1), 0.0)
    m = left + up + 2 * (inu + inv)
    cls = np.minimum(u + v, 11 if n == 8 else 15) * 1
    cls = np.minimum(cls, 11)
    ctx = cls * 12 + qlog(m)
    sym = np.clip(Q, -20, 20).astype(np.int64)
    esc = float(np.sum(np.where(a > 20, 2 * np.log2(a - 19 + 1) + 1, 0.0)))
    return cond_entropy(sym, ctx, esc)

def bits_wavelet(Q, levels):
    H, W = Q.shape; a = np.abs(Q)
    cls = np.zeros(Q.shape, np.int64); ox = np.zeros(Q.shape); oy = np.zeros(Q.shape)
    c = 0
    for lev in range(levels):
        h, w = H >> (lev + 1), W >> (lev + 1)
        for (r0, c0) in [(0, w), (h, 0), (h, w)]:
            c += 1; cls[r0:r0 + h, c0:c0 + w] = c; ox[r0:r0 + h, c0:c0 + w] = c0; oy[r0:r0 + h, c0:c0 + w] = r0
    h, w = H >> levels, W >> levels
    cls[:h, :w] = 0
    ii, jj = np.indices(Q.shape)
    hasL = (jj - ox) > 0; hasU = (ii - oy) > 0
    L = np.where(hasL, shift(a, 0, 1), 0.0); U = np.where(hasU, shift(a, 1, 0), 0.0)
    UL = np.where(hasL & hasU, shift(a, 1, 1), 0.0)
    LL = np.where((jj - ox) > 1, shift(a, 0, 2), 0.0)
    m = 2 * (L + U) + UL + LL
    # parent magnitude (same orientation, next coarser level), averaged: add into context
    par = np.zeros(Q.shape)
    for lev in range(levels - 1):
        h, w = H >> (lev + 1), W >> (lev + 1)
        h2, w2 = H >> (lev + 2), W >> (lev + 2)
        for (r0, c0), (pr, pc) in zip([(0, w), (h, 0), (h, w)], [(0, w2), (h2, 0), (h2, w2)]):
            blk = a[pr:pr + h2, pc:pc + w2]
            par[r0:r0 + h, c0:c0 + w] = np.kron(blk, np.ones((2, 2)))[:h, :w]
    m = m + 2 * par
    ctx = np.minimum(cls, 15) * 12 + qlog(m)
    sym = np.clip(Q, -20, 20).astype(np.int64)
    esc = float(np.sum(np.where(a > 20, 2 * np.log2(a - 19 + 1) + 1, 0.0)))
    return cond_entropy(sym, ctx, esc)

# ---- codec models ----------------------------------------------------------------------------------------------
OFFSET = 0.38  # dead-zone rounding offset (RDOQ-like)
def quant(c, step):
    return np.sign(c) * np.floor(np.abs(c) / step + OFFSET)

def dequant(q, step):
    return q * step

def run(name, img, step, P=None, wt=None):
    if name.startswith("dct"):
        n = 16 if name.startswith("dct16") else 8
        x = pot_apply(img, P, n) if "pot" in name else img
        co = blocks_fwd(x - 128.0, n)
        q = quant(co, step); rec = blocks_inv(dequant(q, step), n) + 128.0
        if "pot" in name: rec = pot_apply(rec - 0.0, P, n, inverse=True)
        bits = bits_block(q, n)
    else:
        lv = int(name[-1])
        co = dwt2(img - 128.0, lv)
        q = quant(co, step * wt); rec = idwt2(dequant(q, step * wt), lv) + 128.0
        bits = bits_wavelet(q, lv)
    rec = np.clip(np.round(rec), 0, 255)
    mse = np.mean((rec[:334] - img[:334]) ** 2)
    return bits / 8.0, 10 * math.log10(255.0 ** 2 / mse)

def fit_pot(train, n):
    rng = np.random.default_rng(1)
    def cost(th):
        P = pot_matrix(th, n); tot = 0.0
        for im in train:
            co = blocks_fwd(pot_apply(im, P, n) - 128.0, n)
            tot += np.sum(np.log2(1.0 + np.abs(co) / 14.0))
        return tot
    th = np.zeros(6); best = cost(th); stepa = 0.4
    for it in range(14):
        improved = False
        for k in range(6):
            for s in (stepa, -stepa):
                t = th.copy(); t[k] += s; c = cost(t)
                if c < best - 1e-6: th, best, improved = t, c, True
        if not improved: stepa /= 2
    return th, best

if __name__ == "__main__":
    imgs = []
    for p in sys.argv[1:]:
        imgs += load_frames(p, [12])
    th8, c8 = fit_pot(imgs[:2], 8)
    print("POT8 angles", np.round(th8, 3))
    th16, c16 = fit_pot(imgs[:2], 16) if False else (th8, 0)
    P8 = pot_matrix(th8, 8); P16 = pot_matrix(th8, 16)  # 16: same V on the central 4 (approximation, see doc)
    wts = {lv: wavelet_weights(imgs[0].shape, lv) for lv in (3, 4, 5)}
    cfgs = [("dct8", None), ("dct16", None), ("dct8pot", P8), ("wav3", None), ("wav4", None), ("wav5", None)]
    steps = [5, 7, 10, 14, 20, 28, 40]
    res = {c[0]: [] for c in cfgs}
    for im in imgs:
        for name, P in cfgs:
            pts = []
            for s in steps:
                wt = wts[int(name[-1])] if name.startswith("wav") else None
                pts.append(run(name, im, s, P, wt))
            res[name].append(pts)
    def at(pts, target):
        pts = sorted(pts, key=lambda p: p[1])
        for a, b in zip(pts, pts[1:]):
            if a[1] <= target <= b[1]:
                t = (target - a[1]) / (b[1] - a[1])
                return math.exp(math.log(a[0]) * (1 - t) + math.log(b[0]) * t)
        return float("nan")
    for target in (34, 36, 38, 40):
        print(f"\nluma-only estimated kB per 800x334 frame at PSNR-Y {target} dB (per image, geo-mean, ratio to dct8)")
        base = None
        for name, _ in cfgs:
            v = [at(p, target) / 1000 for p in res[name]]
            gm = math.exp(sum(math.log(x) for x in v) / len(v))
            if name == "dct8": base = gm
            print(f"{name:8s} " + " ".join(f"{x:6.2f}" for x in v) + f"  gm {gm:6.2f}  x{gm / base:.3f}")
