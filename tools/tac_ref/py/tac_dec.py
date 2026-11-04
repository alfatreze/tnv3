#!/usr/bin/env python3
"""Independent TAC v0 decoder written from docs/TAC_BITSTREAM_V0.md only."""
import struct, sys, wave
import numpy as np

M32 = 0xFFFFFFFF
BL = [0,4,8,12,16,20,24,28,32,36,40,44,48,52,57,62,67,73,79,86,93,101,110,120,131,143,157,172,188,206,225,246,268,292,319,349,384,426,478,546,642,793,853]
BS = [0,4,8,12,16,20,24,29,35,42,51,65,92,106]
LONG, START, SHORT, STOP = 0, 1, 2, 3
WNAMES = ['LONG', 'START', 'SHORT', 'STOP']
ALLOWED = {None: (0, 1, 2, 3), 0: (0, 1), 1: (2,), 2: (2, 3), 3: (0, 1)}


class RC:
    def __init__(self, data):
        self.d = data
        self.pos = 0
        self.range = M32
        self.code = 0
        for _ in range(5):
            self.code = ((self.code << 8) | self._nb()) & M32

    def _nb(self):
        p = self.pos
        self.pos = p + 1
        return self.d[p] if p < len(self.d) else 0

    def _norm(self):
        while self.range < (1 << 24):
            self.range = (self.range << 8) & M32
            self.code = ((self.code << 8) | self._nb()) & M32

    def bin(self, ctx, i):
        p = ctx[i]
        bound = (self.range >> 15) * p
        if self.code < bound:
            bit = 0
            self.range = bound
            ctx[i] = p + ((32768 - p) >> 5)
        else:
            bit = 1
            self.code -= bound
            self.range -= bound
            ctx[i] = p - (p >> 5)
        self._norm()
        return bit

    def byp(self):
        self.range >>= 1
        if self.code >= self.range:
            self.code -= self.range
            bit = 1
        else:
            bit = 0
        self._norm()
        return bit

    def B(self, n):
        v = 0
        for _ in range(n):
            v = (v << 1) | self.byp()
        return v

    def EG(self, ctx, k):
        n = len(ctx)
        i = 0
        base = 0
        while self.bin(ctx, min(i, n - 1)):
            base += 1 << k
            k += 1
            i += 1
            if k > 40:
                raise ValueError('EG runaway')
        return base + self.B(k)


def make_ctx():
    return dict(sfz=[16384]*2, sfm=[16384]*6, ms=[16384]*2,
                bz=[[16384]*2 for _ in range(3)],
                m0=[[16384]*5 for _ in range(3)],
                m1=[[16384]*5 for _ in range(3)],
                m2=[[16384]*3 for _ in range(3)],
                esc=[16384]*6)


# transform matrices
_n = np.arange(2048)[:, None]
_k = np.arange(1024)[None, :]
COS_L = np.cos(np.pi / 1024 * (_n + 0.5 + 512) * (_k + 0.5))
_n = np.arange(256)[:, None]
_k = np.arange(128)[None, :]
COS_S = np.cos(np.pi / 128 * (_n + 0.5 + 64) * (_k + 0.5))


def ws(m):
    return np.sin(np.pi * (m + 0.5) / 256)


def window(wt):
    n = np.arange(2048)
    sinw = np.sin(np.pi * (n + 0.5) / 2048)
    if wt == LONG:
        return sinw
    w = np.zeros(2048)
    if wt == START:
        w[:1024] = sinw[:1024]
        w[1024:1472] = 1
        m = np.arange(1472, 1600)
        w[1472:1600] = ws(128 + m - 1472)
    elif wt == STOP:
        m = np.arange(448, 576)
        w[448:576] = ws(m - 448)
        w[576:1024] = 1
        w[1024:] = sinw[1024:]
    return w


WIN = {t: window(t) for t in (LONG, START, STOP)}
WS = ws(np.arange(256))


def read_file(path):
    b = open(path, 'rb').read()
    if b[:4] != b'TAC0':
        raise ValueError('bad magic')
    rate, ch = struct.unpack_from('<IB', b, 4)
    nframes, nsamp = struct.unpack_from('<II', b, 12)
    pos = 20
    frames = []
    for _ in range(nframes):
        (ln,) = struct.unpack_from('<H', b, pos)
        pos += 2
        frames.append(b[pos:pos + ln])
        pos += ln
    return rate, ch, nsamp, frames


def decode_frame(t, data, ch, ctx_holder, prev_wt, st):
    rc = RC(data)
    if rc.B(1):
        ctx_holder[0] = make_ctx()
        st['resets'] += 1
    elif ctx_holder[0] is None:
        raise ValueError('frame without reset at start')
    c = ctx_holder[0]
    wt = rc.B(2)
    if wt not in ALLOWED[prev_wt]:
        raise ValueError('window sequence violation frame %d: %s -> %s' % (t, prev_wt, wt))
    st['wtype'][wt] += 1
    msmode = rc.B(2) if ch == 2 else 0
    st['msmode'][msmode] = st['msmode'].get(msmode, 0) + 1
    maxband = rc.B(6)
    short = wt == SHORT
    if maxband > (13 if short else 42):
        raise ValueError('maxband too large')
    nfill = rc.B(1)
    msflags = [0] * maxband
    if msmode == 2:
        prev = 0
        for b in range(maxband):
            prev = msflags[b] = rc.bin(c['ms'], prev)
    elif msmode == 1:
        msflags = [1] * maxband
    X = np.zeros((ch, 1024))
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
                raise ValueError('sf out of range')
        st['maxsf'] = max(st['maxsf'], max(sf))
        q1 = q2 = 0
        prevz = 0
        q = np.zeros(1024)
        bl = BS if short else BL
        zero_bands = []
        for b in range(maxband):
            if short:
                cls = 0 if b < 4 else (1 if b < 9 else 2)
            else:
                cls = 0 if b < 12 else (1 if b < 28 else 2)
            z = rc.bin(c['bz'][cls], prevz)
            prevz = z
            if short:
                idx = [w * 128 + k for w in range(8) for k in range(bl[b], bl[b + 1])]
            else:
                idx = list(range(bl[b], bl[b + 1]))
            if z:
                zero_bands.append((b, idx))
                continue
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
                    raise ValueError('magnitude > 8191')
                if mag:
                    allz = False
                    q[i] = -mag if rc.byp() else mag
                    if mag > st['maxq']:
                        st['maxq'] = mag
                q2 = q1
                q1 = mag
            if allz:
                zero_bands.append((b, idx))
        # dequantise
        x = np.sign(q) * np.abs(q) ** (4.0 / 3.0)
        for b in range(maxband):
            sc = 2.0 ** ((sf[b] - 100) / 4.0)
            if short:
                for w in range(8):
                    x[w*128+bl[b]:w*128+bl[b+1]] *= sc
            else:
                x[bl[b]:bl[b+1]] *= sc
        if nfill:
            s = (t * 2 + chn + 1) & M32
            for b, idx in zero_bands:
                if b >= (3 if short else 8):
                    sc = 2.0 ** ((sf[b] - 100) / 4.0)
                    st['filled'] += 1
                    for i in idx:
                        s = (s * 1664525 + 1013904223) & M32
                        u = ((s >> 8) & 0xFFFF) / 32768.0 - 1.0
                        x[i] = sc * u
        X[chn] = np.clip(x, -2.0 ** 26, 2.0 ** 26)   # draft 1.2: values limited to +-2^26 (section 6)
    if ch == 2:
        for b in range(maxband):
            if msflags[b]:
                if short:
                    sl = np.concatenate([np.arange(w*128+BS[b], w*128+BS[b+1]) for w in range(8)])
                else:
                    sl = np.arange(BL[b], BL[b+1])
                M = X[0, sl].copy()
                S = X[1, sl].copy()
                X[0, sl] = np.clip(M + S, -2.0 ** 26, 2.0 ** 26)
                X[1, sl] = np.clip(M - S, -2.0 ** 26, 2.0 ** 26)
    # inverse transform
    out = np.zeros((ch, 2048))
    for chn in range(ch):
        if short:
            y = np.zeros(2048)
            for w in range(8):
                v = (2.0 / 128) * WS * (COS_S @ X[chn, w*128:(w+1)*128])
                y[448 + 128*w: 448 + 128*w + 256] += v
            out[chn] = y
        else:
            out[chn] = (2.0 / 1024) * WIN[wt] * (COS_L @ X[chn])
    return wt, out


def decode(path):
    rate, ch, nsamp, frames = read_file(path)
    st = dict(wtype=[0]*4, resets=0, msmode={}, filled=0, maxq=0, maxsf=0)
    total = (len(frames) + 1) * 1024
    acc = np.zeros((ch, total))
    holder = [None]
    prev = None
    for t, f in enumerate(frames):
        prev, y = decode_frame(t, f, ch, holder, prev, st)
        pos = (t - 1) * 1024
        if pos < 0:
            acc[:, 0:pos + 2048] += y[:, -pos:]
        else:
            acc[:, pos:pos + 2048] += y
    a = acc[:, :nsamp]
    r = np.where(a >= 0, np.floor(a + 0.5), np.ceil(a - 0.5))
    r = np.clip(r, -32768, 32767).astype('<i2')
    return rate, ch, r.T.copy(), st


def write_wav(path, rate, ch, pcm):
    with wave.open(path, 'wb') as w:
        w.setnchannels(ch)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm.tobytes())


if __name__ == '__main__':
    rate, ch, pcm, st = decode(sys.argv[1])
    write_wav(sys.argv[2], rate, ch, pcm)
