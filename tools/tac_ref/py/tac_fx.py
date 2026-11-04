#!/usr/bin/env python3
"""TAC integer decoder model, written from docs/TAC_BITSTREAM_V0.md section 10.
Parsing is reused from tac_dec.py (RC, contexts, constants); everything from
dequantisation onwards is implemented here with integers."""
import math, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
from tac_dec import RC, make_ctx, read_file, BL, BS, LONG, START, SHORT, STOP, ALLOWED, M32

FX = 4
TW = 20
CLAMP = 1 << (26 + FX)   # draft 1.2: +-2^26 in real units, inclusive


def rnd(x):  # round to nearest, halves away from zero
    return int(math.floor(abs(x) + 0.5)) * (1 if x >= 0 else -1)


def R(v, s):
    if s > 0:
        return (v + (1 << (s - 1))) >> s
    return v << (-s)


P43 = [rnd(i ** (4.0 / 3.0) * 65536) for i in range(1025)]
P2 = [rnd(2.0 ** (b / 4.0) * (1 << 22)) for b in range(4)]


def clamp(v):
    return max(-CLAMP, min(CLAMP, v))


def deq_mag(a):
    if a <= 1024:
        return P43[a]
    h = a >> 3
    return 16 * (P43[h] + R((P43[h + 1] - P43[h]) * (a & 7), 3))


def qarr(x):
    return (np.sign(x) * np.floor(np.abs(x) * (1 << TW) + 0.5)).astype(np.int64)


class Tables:
    def __init__(self, n):
        self.n = n
        h = n // 2
        j = np.arange(h)
        ang = np.pi * (4 * j + 1) / (4 * n)
        self.pre_c = qarr(np.cos(ang)); self.pre_s = qarr(np.sin(ang))
        ang = np.pi * j / n
        self.post_c = qarr(np.cos(ang)); self.post_s = qarr(np.sin(ang))
        j = np.arange(n // 4)
        ang = 2 * np.pi * j / (n // 2)
        self.f_c = qarr(np.cos(ang)); self.f_s = qarr(np.sin(ang))
        bits = int(round(math.log2(h)))
        self.br = np.array([int(format(i, '0%db' % bits)[::-1], 2) for i in range(h)])


T_L = Tables(1024)
T_S = Tables(128)


def build_windows():
    n = np.arange(2048)
    sinw = np.sin(np.pi * (n + 0.5) / 2048)
    ws = lambda m: np.sin(np.pi * (m + 0.5) / 256)
    W = {}
    W[LONG] = sinw.copy()
    w = np.zeros(2048); w[:1024] = sinw[:1024]; w[1024:1472] = 1
    w[1472:1600] = ws(128 + np.arange(128))
    W[START] = w
    w = np.zeros(2048); w[448:576] = ws(np.arange(128)); w[576:1024] = 1; w[1024:] = sinw[1024:]
    W[STOP] = w
    W = {k: qarr(v) for k, v in W.items()}
    W[SHORT] = qarr(ws(np.arange(256)))
    return W


WIN = build_windows()


def cmul_conj(xr, xi, c, s):
    """(xr + i xi) * (c - i s), each component rounded once."""
    return R(xr * c + xi * s, TW), R(xi * c - xr * s, TW)


def dct4(X, T):
    """X: (B, n) int64 -> (B, n) int64, (2/n) DCT-IV in Q(FX)."""
    n = T.n
    h = n // 2
    j = np.arange(h)
    xr = X[:, 2 * j]
    xi = X[:, n - 1 - 2 * j]
    zr, zi = cmul_conj(xr, xi, T.pre_c, T.pre_s)
    Zr = np.zeros_like(zr); Zi = np.zeros_like(zi)
    Zr[:, T.br] = zr; Zi[:, T.br] = zi
    B = X.shape[0]
    span = 2
    while span <= h:
        hs = span // 2
        idx = (np.arange(hs) * h // span)
        c = T.f_c[idx]; s = T.f_s[idx]
        Zr = Zr.reshape(B, h // span, span); Zi = Zi.reshape(B, h // span, span)
        ar = Zr[:, :, :hs]; ai = Zi[:, :, :hs]
        tr, ti = cmul_conj(Zr[:, :, hs:], Zi[:, :, hs:], c, s)
        Zr = np.concatenate([(ar + tr + 1) >> 1, (ar - tr + 1) >> 1], axis=2).reshape(B, h)
        Zi = np.concatenate([(ai + ti + 1) >> 1, (ai - ti + 1) >> 1], axis=2).reshape(B, h)
        span *= 2
    vr, vi = cmul_conj(Zr, Zi, T.post_c, T.post_s)
    u = np.zeros((B, n), dtype=np.int64)
    k = np.arange(h)
    u[:, 2 * k] = vr
    u[:, n - 1 - 2 * k] = -vi
    return u


def unfold(u, n):
    m = np.arange(2 * n)
    y = np.empty((u.shape[0], 2 * n), dtype=np.int64)
    a = m < n // 2
    b = (m >= n // 2) & (m < 3 * n // 2)
    c = m >= 3 * n // 2
    y[:, a] = u[:, m[a] + n // 2]
    y[:, b] = -u[:, 3 * n // 2 - 1 - m[b]]
    y[:, c] = -u[:, m[c] - 3 * n // 2]
    return y


def decode_frame(t, data, ch, holder, prev_wt):
    rc = RC(data)
    if rc.B(1):
        holder[0] = make_ctx()
    elif holder[0] is None:
        raise ValueError('no reset')
    c = holder[0]
    wt = rc.B(2)
    if wt not in ALLOWED[prev_wt]:
        raise ValueError('window sequence')
    msmode = rc.B(2) if ch == 2 else 0
    maxband = rc.B(6)
    short = wt == SHORT
    if maxband > (13 if short else 42):
        raise ValueError('maxband')
    nfill = rc.B(1)
    msflags = [0] * maxband
    if msmode == 2:
        prev = 0
        for b in range(maxband):
            prev = msflags[b] = rc.bin(c['ms'], prev)
    elif msmode == 1:
        msflags = [1] * maxband
    bl = BS if short else BL
    X = [[0] * 1024 for _ in range(ch)]
    for chn in range(ch):
        sf = [0] * maxband
        sf[0] = rc.B(8)
        pd = 0
        for b in range(1, maxband):
            if rc.bin(c['sfz'], 1 if pd != 0 else 0):
                m = 1 + rc.EG(c['sfm'], 0)
                d = -m if rc.byp() else m
            else:
                d = 0
            pd = d
            sf[b] = sf[b - 1] + d
            if not 0 <= sf[b] <= 255:
                raise ValueError('sf')
        q1 = q2 = 0
        prevz = 0
        x = X[chn]
        zero_bands = []
        for b in range(maxband):
            cls = (0 if b < 4 else 1 if b < 9 else 2) if short else (0 if b < 12 else 1 if b < 28 else 2)
            z = rc.bin(c['bz'][cls], prevz)
            prevz = z
            if short:
                idx = [w * 128 + k for w in range(8) for k in range(bl[b], bl[b + 1])]
            else:
                idx = list(range(bl[b], bl[b + 1]))
            if z:
                zero_bands.append((b, idx))
                continue
            e = sf[b] - 100
            ex = e >> 2
            bb = e & 3
            allz = True
            for i in idx:
                nc = min(q1, 2) + min(q2, 2)
                if rc.bin(c['m0'][cls], nc) == 0:
                    mag = 0
                elif rc.bin(c['m1'][cls], nc) == 0:
                    mag = 1
                elif rc.bin(c['m2'][cls], min(nc, 2)) == 0:
                    mag = 2
                else:
                    mag = 3 + rc.EG(c['esc'], 0)
                if mag > 8191:
                    raise ValueError('mag')
                if mag:
                    allz = False
                    neg = rc.byp()
                    val = clamp(R(deq_mag(mag) * P2[bb], 38 - FX - ex))
                    x[i] = -val if neg else val
                q2 = q1
                q1 = mag
            if allz:
                zero_bands.append((b, idx))
        if nfill:
            s = (t * 2 + chn + 1) & M32
            for b, idx in zero_bands:
                if b >= (3 if short else 8):
                    e = sf[b] - 100
                    ex = e >> 2
                    bb = e & 3
                    for i in idx:
                        s = (s * 1664525 + 1013904223) & M32
                        r = (s >> 8) & 0xFFFF
                        u = r - 32768
                        val = clamp(R(abs(u) * P2[bb], 37 - FX - ex))
                        x[i] = -val if u < 0 else val
    if ch == 2:
        for b in range(maxband):
            if msflags[b]:
                if short:
                    sl = [w * 128 + k for w in range(8) for k in range(BS[b], BS[b + 1])]
                else:
                    sl = range(BL[b], BL[b + 1])
                for i in sl:
                    M = X[0][i]; S = X[1][i]
                    X[0][i] = clamp(M + S)
                    X[1][i] = clamp(M - S)
    out = np.zeros((ch, 2048), dtype=np.int64)
    for chn in range(ch):
        xa = np.array(X[chn], dtype=np.int64)
        if short:
            u = dct4(xa.reshape(8, 128), T_S)
            y = unfold(u, 128)  # (8, 256)
            y = R(y * WIN[SHORT][None, :], TW)
            for w in range(8):
                out[chn, 448 + 128 * w: 448 + 128 * w + 256] += y[w]
        else:
            u = dct4(xa.reshape(1, 1024), T_L)
            y = unfold(u, 1024)
            out[chn] = R(y * WIN[wt][None, :], TW)[0]
    return wt, out


def decode(path):
    rate, ch, nsamp, frames = read_file(path)
    acc = np.zeros((ch, (len(frames) + 1) * 1024), dtype=np.int64)
    holder = [None]
    prev = None
    for t, f in enumerate(frames):
        prev, y = decode_frame(t, f, ch, holder, prev)
        pos = (t - 1) * 1024
        if pos < 0:
            acc[:, 0:pos + 2048] += y[:, -pos:]
        else:
            acc[:, pos:pos + 2048] += y
    a = acc[:, :nsamp]
    half = 1 << (FX - 1)
    r = np.where(a >= 0, (a + half) >> FX, -((-a + half) >> FX))
    r = np.clip(r, -32768, 32767).astype('<i2')
    return rate, ch, r.T.copy()


if __name__ == '__main__':
    import wave
    rate, ch, pcm = decode(sys.argv[1])
    with wave.open(sys.argv[2], 'wb') as w:
        w.setnchannels(ch); w.setsampwidth(2); w.setframerate(rate)
        w.writeframes(pcm.tobytes())
