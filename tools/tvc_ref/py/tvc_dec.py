#!/usr/bin/env python3
"""Independent TVC v0 decoder written from docs/TVC_BITSTREAM_V0.md only."""
import sys, struct, time
import numpy as np

QS = [10, 11, 13, 14, 16, 18, 20, 23, 25, 29, 32, 36, 40, 45, 51, 57, 64, 72, 81, 91, 102, 114, 128, 144, 161, 181,
      203, 228, 256, 287, 323, 362, 406, 456, 512, 575, 645, 724, 813, 912, 1024, 1149, 1290, 1448, 1625, 1825, 2048,
      2299, 2580, 2896, 3251, 3649]
QSA = np.array(QS, dtype=np.int64)
T = np.array([
    [64, 64, 64, 64, 64, 64, 64, 64],
    [89, 75, 50, 18, -18, -50, -75, -89],
    [84, 35, -35, -84, -84, -35, 35, 84],
    [75, -18, -89, -50, 50, 89, 18, -75],
    [64, -64, -64, 64, 64, -64, -64, 64],
    [50, -89, 18, 75, -75, -18, 89, -50],
    [35, -84, 84, -35, -35, 84, -84, 35],
    [18, -50, 75, -89, 89, -75, 50, -18]], dtype=np.int64)
PCLASS = [0, 1, 2, 3, 4, 5, 6, 6, 7, 7, 8, 8, 8, 8] + [9] * 6 + [10] * 10 + [11] * 12 + [12] * 22
assert len(PCLASS) == 64

SCAN = []
for s in range(15):
    pos = [(r, s - r) for r in range(8) if 0 <= s - r < 8]
    if s % 2 == 1:
        pos.sort(key=lambda p: p[0])
    else:
        pos.sort(key=lambda p: -p[0])
    SCAN.extend(pos)
assert len(SCAN) == 64

M32 = 0xFFFFFFFF

PCLASS4 = [0, 1, 2, 3, 4, 5, 6, 6, 7, 7, 7, 7, 8, 8, 8, 8]
SCAN4 = []
for _s in range(7):
    _pos = [(r, _s - r) for r in range(4) if 0 <= _s - r < 4]
    _pos.sort(key=(lambda p: p[0]) if _s % 2 == 1 else (lambda p: -p[0]))
    SCAN4.extend(_pos)
assert len(SCAN4) == 16
T4 = np.array([[64, 64, 64, 64], [84, 35, -35, -84], [64, -64, -64, 64], [35, -84, 84, -35]], dtype=np.int64)


def inv_transform4(lev, qp):
    C = np.zeros((4, 4), dtype=np.int64)
    qs = QS[qp]
    for i in range(16):
        l = lev[i]
        if l:
            r, c = SCAN4[i]
            v = min(32767, (abs(l) * qs + 8) >> 4)
            C[r, c] = -v if l < 0 else v
    e = np.clip((T4.T @ C + 64) >> 7, -32768, 32767)
    return (e @ T4 + 64) >> 7


class RC:
    def __init__(self, data, pos):
        self.d = data
        self.p = pos
        self.range = 0xFFFFFFFF
        self.code = 0
        for _ in range(5):
            self.code = ((self.code << 8) | self.nb()) & M32

    def nb(self):
        v = self.d[self.p] if self.p < len(self.d) else 0
        self.p += 1
        return v

    def norm(self):
        while self.range < (1 << 24):
            self.range = (self.range << 8) & M32
            self.code = ((self.code << 8) | self.nb()) & M32

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
        self.norm()
        return bit

    def byp(self):
        self.range >>= 1
        if self.code >= self.range:
            bit = 1
            self.code -= self.range
        else:
            bit = 0
        self.norm()
        return bit

    def eg(self, ctx, k):
        n = len(ctx)
        i = 0
        base = 0
        while self.bin(ctx, min(i, n - 1)) == 1:
            base += 1 << k
            k += 1
            i += 1
        s = 0
        for _ in range(k):
            s = (s << 1) | self.byp()
        return base + s

    def seg(self, zctx, zi, mctx):
        if self.bin(zctx, zi) == 0:
            return 0
        m = 1 + self.eg(mctx, 0)
        s = self.byp()
        return -m if s else m


def median(a, b, c):
    return sorted((a, b, c))[1]


def clip(v, lo, hi):
    return lo if v < lo else hi if v > hi else v


def inv_transform(lev, qp):
    C = np.zeros((8, 8), dtype=np.int64)
    qs = QS[qp]
    for i in range(64):
        l = lev[i]
        if l:
            r, c = SCAN[i]
            v = min(32767, (abs(l) * qs + 8) >> 4)
            C[r, c] = -v if l < 0 else v
    e = np.clip((T.T @ C + 64) >> 7, -32768, 32767)
    return (e @ T + 128) >> 8


def intra_pred(plane, bx, by, mode, top=0):
    Ta = by > top
    La = bx > 0
    Tn = plane[by - 1, bx:bx + 8].astype(np.int64) if Ta else None
    Ln = plane[by:by + 8, bx - 1].astype(np.int64) if La else None
    if mode == 0:
        if Ta and La:
            v = (int(Tn.sum()) + int(Ln.sum()) + 8) >> 4
        elif Ta:
            v = (int(Tn.sum()) + 4) >> 3
        elif La:
            v = (int(Ln.sum()) + 4) >> 3
        else:
            v = 128
        return np.full((8, 8), v, dtype=np.int64)
    if not Ta:
        Tn = np.full(8, int(Ln[0]) if La else 128, dtype=np.int64)
    if not La:
        Ln = np.full(8, int(Tn[0]) if Ta else 128, dtype=np.int64)
    if mode == 1:
        return np.tile(Tn, (8, 1))
    if mode == 2:
        return np.tile(Ln.reshape(8, 1), (1, 8))
    y = np.arange(8).reshape(8, 1)
    x = np.arange(8).reshape(1, 8)
    return ((7 - x) * Ln.reshape(8, 1) + (x + 1) * Tn[7] + (7 - y) * Tn.reshape(1, 8) + (y + 1) * Ln[7] + 8) >> 4


def inter_pred(ref, bx, by, bw, bh, mvx, mvy):
    h, w = ref.shape
    ix = mvx >> 1
    iy = mvy >> 1
    fx = mvx & 1
    fy = mvy & 1
    xs = np.arange(bx, bx + bw) + ix
    ys = np.arange(by, by + bh) + iy
    x0 = np.clip(xs, 0, w - 1)
    x1 = np.clip(xs + 1, 0, w - 1)
    y0 = np.clip(ys, 0, h - 1)
    y1 = np.clip(ys + 1, 0, h - 1)
    a = ref[np.ix_(y0, x0)]
    if not fx and not fy:
        return a
    b = ref[np.ix_(y0, x1)]
    c = ref[np.ix_(y1, x0)]
    d = ref[np.ix_(y1, x1)]
    if fx and not fy:
        return (a + b + 1) >> 1
    if fy and not fx:
        return (a + c + 1) >> 1
    return (a + b + c + d + 2) >> 2


def ctrunc_div2(v):
    return -((-v) // 2) if v < 0 else v // 2


INTRA, INTER, SKIP = 0, 1, 2


QF = [(0, 64, 0, 0), (-4, 56, 14, -2), (-4, 36, 36, -4), (-2, 14, 56, -4)]


def luma_q(ref, bx, by, bw, bh, mvx, mvy):
    h, w = ref.shape
    ix, iy, fx, fy = mvx >> 2, mvy >> 2, mvx & 3, mvy & 3
    xs = np.arange(bx, bx + bw) + ix
    ys = np.arange(by, by + bh) + iy

    def R(dx, dy):
        return ref[np.ix_(np.clip(ys + dy, 0, h - 1), np.clip(xs + dx, 0, w - 1))]
    if fx == 0 and fy == 0:
        return R(0, 0)
    if fy == 0:
        v = sum(QF[fx][k] * R(k - 1, 0) for k in range(4))
        return np.clip((v + 32) >> 6, 0, 255)
    if fx == 0:
        v = sum(QF[fy][k] * R(0, k - 1) for k in range(4))
        return np.clip((v + 32) >> 6, 0, 255)
    t = [sum(QF[fx][k] * R(k - 1, j - 1) for k in range(4)) for j in range(4)]
    v = sum(QF[fy][j] * t[j] for j in range(4))
    return np.clip((v + 2048) >> 12, 0, 255)


def chroma_q(ref, bx, by, mvx, mvy):
    h, w = ref.shape
    ix, iy, fx, fy = mvx >> 3, mvy >> 3, mvx & 7, mvy & 7
    xs = np.arange(bx, bx + 8) + ix
    ys = np.arange(by, by + 8) + iy

    def R(dx, dy):
        return ref[np.ix_(np.clip(ys + dy, 0, h - 1), np.clip(xs + dx, 0, w - 1))]
    return ((8 - fx) * (8 - fy) * R(0, 0) + fx * (8 - fy) * R(1, 0) + (8 - fx) * fy * R(0, 1)
            + fx * fy * R(1, 1) + 32) >> 6


def predict_mb(refs, mx, my, uses, mvs, qpel=0):
    ys = []
    cbs = []
    crs = []
    for L in (0, 1):
        if uses >> L & 1:
            ref = refs[L]
            mv = mvs[L]
            if qpel:
                ys.append(luma_q(ref[0], mx * 16, my * 16, 16, 16, mv[0], mv[1]))
                cbs.append(chroma_q(ref[1], mx * 8, my * 8, mv[0], mv[1]))
                crs.append(chroma_q(ref[2], mx * 8, my * 8, mv[0], mv[1]))
                continue
            ys.append(inter_pred(ref[0], mx * 16, my * 16, 16, 16, mv[0], mv[1]))
            cx, cy = ctrunc_div2(mv[0]), ctrunc_div2(mv[1])
            cbs.append(inter_pred(ref[1], mx * 8, my * 8, 8, 8, cx, cy))
            crs.append(inter_pred(ref[2], mx * 8, my * 8, 8, 8, cx, cy))
    if len(ys) == 2:
        return (ys[0] + ys[1] + 1) >> 1, (cbs[0] + cbs[1] + 1) >> 1, (crs[0] + crs[1] + 1) >> 1
    return ys[0], cbs[0], crs[0]


class LevelViolation(Exception):
    pass


def reset_ctx(x):
    for i in range(len(x)):
        if isinstance(x[i], list):
            reset_ctx(x[i])
        else:
            x[i] = 16384


def chk_mv(mvs, vx, vy):
    for mv in mvs:
        if abs(mv[0]) > vx or abs(mv[1]) > vy:
            raise LevelViolation('vector')


def decode_picture(data, width, height, refs, vlimit=False):
    ptype, pqp, flags = data[0], data[1], data[2]
    aq = flags & 1
    qpel = (flags >> 1) & 1
    t4ok = (flags >> 2) & 1
    W = (width + 15) & ~15
    H = (height + 15) & ~15
    mbw, mbh = W // 16, H // 16
    sliced = (flags >> 3) & 1
    if sliced:
        rows = data[3]
        if rows < 1:
            raise ValueError('rows')
        ns = -(-mbh // rows)
        lens = [struct.unpack('<I', data[4 + 4 * k:8 + 4 * k])[0] for k in range(ns - 1)]
        sdata = []
        p0 = 4 + 4 * (ns - 1)
        for ln_ in lens:
            sdata.append(data[p0:p0 + ln_])
            p0 += ln_
        sdata.append(data[p0:])
        if vlimit and mbh >= 2 and ns < 2 and vlimit == 3:
            raise LevelViolation('slices')
    else:
        rows = mbh
        ns = 1
        sdata = [data[3:]]
        if vlimit == 3 and mbh >= 2:
            raise LevelViolation('slices')
    rc = None
    vx = 256 * (4 if qpel else 2)
    vy = 128 * (4 if qpel else 2)
    I16 = 16384
    skip = [I16] * 3
    bdir = [I16] * 2
    intra = [I16] * 3
    mpm = [I16]
    rem = [I16] * 2
    cmode = [I16] * 3
    cbp = [[I16] * 3 for _ in range(3)]
    dqz = [I16] * 2
    dqm = [I16] * 3
    mvz = [[I16] * 3 for _ in range(2)]
    mvm = [[I16] * 6 for _ in range(2)]
    last_c = [[I16] * 64 for _ in range(3)]
    sig = [[[I16] * 3 for _ in range(13)] for _ in range(3)]
    gt1 = [[[I16] * 3 for _ in range(3)] for _ in range(3)]
    lrem = [[I16] * 6 for _ in range(3)]
    t4c = [I16] * 3
    cbf4 = [I16] * 4
    mt4 = [[0] * mbw for _ in range(mbh)]

    Y = np.zeros((H, W), dtype=np.int64)
    Cb = np.zeros((H // 2, W // 2), dtype=np.int64)
    Cr = np.zeros((H // 2, W // 2), dtype=np.int64)
    mtype = [[None] * mbw for _ in range(mbh)]
    Z2 = ((0, 0), (0, 0))
    mmv = [[Z2] * mbw for _ in range(mbh)]
    mmvd = [[Z2] * mbw for _ in range(mbh)]
    muses = [[0] * mbw for _ in range(mbh)]
    mcoded = [[0] * mbw for _ in range(mbh)]
    mimode = [[[0] * 4 for _ in range(mbw)] for _ in range(mbh)]
    mqp = [[0] * mbw for _ in range(mbh)]
    prev_qp = pqp
    prev_dq = 0
    ctx_all = [skip, bdir, intra, mpm, rem, cmode, cbp, dqz, dqm, mvz, mvm, last_c, sig, gt1, lrem, t4c, cbf4]

    def parse_block(cc):
        lev = [0] * 64
        node = 1
        if cc == 2:
            for _ in range(4):
                node = 2 * node + rc.bin(last_c[cc], node)
            last = node - 16
        else:
            for _ in range(6):
                node = 2 * node + rc.bin(last_c[cc], node)
            last = node - 64
        ngt1 = 0
        for i in range(last, -1, -1):
            if i < last:
                n = (1 if lev[i + 1] != 0 else 0) + (1 if (i + 2 <= last and lev[i + 2] != 0) else 0)
                if rc.bin(sig[cc][(PCLASS4 if cc == 2 else PCLASS)[i]], n) == 0:
                    continue
            if cc == 2:
                gc = 0 if i < 3 else 1 if i < 8 else 2
            else:
                gc = 0 if i < 6 else 1 if i < 20 else 2
            g = rc.bin(gt1[cc][gc], min(ngt1, 2))
            if g == 0:
                a = 1
            else:
                a = 2 + rc.eg(lrem[cc], 0)
                ngt1 += 1
            s = rc.byp()
            lev[i] = -a if s else a
        return lev

    stats = {'intra': 0, 'inter': 0, 'skip': 0}
    for my in range(mbh):
        for mx in range(mbw):
            if mx == 0 and my % rows == 0:
                rc = RC(sdata[my // rows], 0)
                for c_ in ctx_all:
                    reset_ctx(c_)
                prev_qp = pqp
                prev_dq = 0
            sfirst = (my // rows) * rows
            availL = mx > 0
            availT = my > sfirst
            preds = [(0, 0), (0, 0)]
            if ptype != 0:
                for L in (0, 1):
                    A = mmv[my][mx - 1][L] if availL else None
                    if not availT:
                        preds[L] = A if A is not None else (0, 0)
                    else:
                        B = mmv[my - 1][mx][L]
                        if mx + 1 < mbw:
                            C = mmv[my - 1][mx + 1][L]
                        elif mx - 1 >= 0:
                            C = mmv[my - 1][mx - 1][L]
                        else:
                            C = None
                        a = A if A is not None else (0, 0)
                        c = C if C is not None else (0, 0)
                        preds[L] = (median(a[0], B[0], c[0]), median(a[1], B[1], c[1]))
            typ = INTRA
            if ptype != 0:
                n = (availL and mtype[my][mx - 1] == SKIP) + (availT and mtype[my - 1][mx] == SKIP)
                if rc.bin(skip, int(n)):
                    typ = SKIP
                else:
                    n = (availL and mtype[my][mx - 1] == INTRA) + (availT and mtype[my - 1][mx] == INTRA)
                    typ = INTRA if rc.bin(intra, int(n)) else INTER
            mtype[my][mx] = typ
            if typ == SKIP:
                uses = 1 if ptype == 1 else 3
                mvs = tuple(preds[L] if uses >> L & 1 else (0, 0) for L in (0, 1))
                mmv[my][mx] = mvs
                if vlimit:
                    chk_mv(mvs, vx, vy)
                mmvd[my][mx] = Z2
                muses[my][mx] = uses
                mcoded[my][mx] = 0
                mqp[my][mx] = prev_qp
                stats['skip'] += 1
                py_, pcb, pcr = predict_mb(refs, mx, my, uses, mvs, qpel)
                Y[my * 16:my * 16 + 16, mx * 16:mx * 16 + 16] = py_
                Cb[my * 8:my * 8 + 8, mx * 8:mx * 8 + 8] = pcb
                Cr[my * 8:my * 8 + 8, mx * 8:mx * 8 + 8] = pcr
                continue
            if aq:
                d = rc.seg(dqz, 1 if prev_dq != 0 else 0, dqm)
                qp = clip(prev_qp + d, 0, 51)
                prev_qp = qp
                prev_dq = d
            else:
                qp = pqp
            mqp[my][mx] = qp
            imodes = [0] * 4
            cm = 0
            if typ == INTRA:
                stats['intra'] += 1
                mmv[my][mx] = Z2
                mmvd[my][mx] = Z2
                muses[my][mx] = 0
                for b in range(4):
                    if b in (1, 3):
                        ml = imodes[b - 1]
                    else:
                        ml = mimode[my][mx - 1][b + 1] if (availL and mtype[my][mx - 1] == INTRA) else 0
                    if b in (2, 3):
                        mt = imodes[b - 2]
                    else:
                        mt = mimode[my - 1][mx][b + 2] if (availT and mtype[my - 1][mx] == INTRA) else 0
                    m = min(ml, mt)
                    if rc.bin(mpm, 0):
                        imodes[b] = m
                    else:
                        others = [x for x in range(4) if x != m]
                        if rc.bin(rem, 0) == 0:
                            idx = 0
                        else:
                            idx = 1 + rc.bin(rem, 1)
                        imodes[b] = others[idx]
                if rc.bin(cmode, 0) == 0:
                    cm = 0
                elif rc.bin(cmode, 1) == 0:
                    cm = 1
                else:
                    cm = 2 + rc.bin(cmode, 2)
                mimode[my][mx] = imodes
            else:
                stats['inter'] += 1
                if ptype == 1:
                    uses = 1
                elif rc.bin(bdir, 0) == 0:
                    uses = 3
                else:
                    uses = 1 + rc.bin(bdir, 1)
                muses[my][mx] = uses
                mvds = [(0, 0), (0, 0)]
                mvs = [(0, 0), (0, 0)]
                for L in (0, 1):
                    if not (uses >> L & 1):
                        continue
                    mvd = [0, 0]
                    for comp in range(2):
                        s_ = 0
                        if availL and mtype[my][mx - 1] == INTER and muses[my][mx - 1] >> L & 1:
                            s_ += abs(mmvd[my][mx - 1][L][comp])
                        if availT and mtype[my - 1][mx] == INTER and muses[my - 1][mx] >> L & 1:
                            s_ += abs(mmvd[my - 1][mx][L][comp])
                        cls = 0 if s_ < 2 else 1 if s_ < 16 else 2
                        mvd[comp] = rc.seg(mvz[comp], cls, mvm[comp])
                    mvds[L] = (mvd[0], mvd[1])
                    mvs[L] = (preds[L][0] + mvd[0], preds[L][1] + mvd[1])
                mmvd[my][mx] = tuple(mvds)
                mmv[my][mx] = tuple(mvs)
                if vlimit:
                    chk_mv(mvs, vx, vy)
            coded = 0
            for b in range(4):
                if b in (1, 3):
                    lc = (coded >> (b - 1)) & 1
                else:
                    lc = (mcoded[my][mx - 1] >> (b + 1)) & 1 if availL else 0
                if b in (2, 3):
                    tc = (coded >> (b - 2)) & 1
                else:
                    tc = (mcoded[my - 1][mx] >> (b + 2)) & 1 if availT else 0
                coded |= rc.bin(cbp[0], lc + tc) << b
            for b in (4, 5):
                lc = (mcoded[my][mx - 1] >> b) & 1 if availL else 0
                tc = (mcoded[my - 1][mx] >> b) & 1 if availT else 0
                coded |= rc.bin(cbp[b - 3], lc + tc) << b
            mcoded[my][mx] = coded
            resid = {}
            t4mask = 0
            for b in range(6):
                if (coded >> b) & 1:
                    if b < 4 and t4ok:
                        if b in (1, 3):
                            lt = (t4mask >> (b - 1)) & 1
                        else:
                            lt = (mt4[my][mx - 1] >> (b + 1)) & 1 if availL else 0
                        if b in (2, 3):
                            tt = (t4mask >> (b - 2)) & 1
                        else:
                            tt = (mt4[my - 1][mx] >> (b + 2)) & 1 if availT else 0
                        if rc.bin(t4c, lt + tt):
                            t4mask |= 1 << b
                            R = np.zeros((8, 8), dtype=np.int64)
                            for k in range(4):
                                if rc.bin(cbf4, k):
                                    lev = parse_block(2)
                                    R[4 * (k >> 1):4 * (k >> 1) + 4, 4 * (k & 1):4 * (k & 1) + 4] = inv_transform4(lev, qp)
                            resid[b] = R
                            continue
                    lev = parse_block(0 if b < 4 else 1)
                    resid[b] = inv_transform(lev, qp)
            mt4[my][mx] = t4mask
            planes = [(Y, mx * 16, my * 16), (Y, mx * 16 + 8, my * 16), (Y, mx * 16, my * 16 + 8),
                      (Y, mx * 16 + 8, my * 16 + 8), (Cb, mx * 8, my * 8), (Cr, mx * 8, my * 8)]
            if typ == INTRA:
                for b in range(6):
                    pl, bx, by = planes[b]
                    mode = imodes[b] if b < 4 else cm
                    p = intra_pred(pl, bx, by, mode, sfirst * (16 if b < 4 else 8))
                    if b in resid:
                        p = p + resid[b]
                    pl[by:by + 8, bx:bx + 8] = np.clip(p, 0, 255)
            else:
                py_, pcb, pcr = predict_mb(refs, mx, my, muses[my][mx], mmv[my][mx], qpel)
                Y[my * 16:my * 16 + 16, mx * 16:mx * 16 + 16] = py_
                Cb[my * 8:my * 8 + 8, mx * 8:mx * 8 + 8] = pcb
                Cr[my * 8:my * 8 + 8, mx * 8:mx * 8 + 8] = pcr
                for b in resid:
                    pl, bx, by = planes[b]
                    pl[by:by + 8, bx:bx + 8] = np.clip(pl[by:by + 8, bx:bx + 8] + resid[b], 0, 255)

    deblock(Y, Cb, Cr, mbw, mbh, mtype, mmv, mcoded, mqp, muses, qpel)
    return (Y, Cb, Cr), stats, rc


def filt(P1, P0, Q0, Q1, bs, qp):
    qs = QSA[qp]
    alpha = ((5 * qs) >> 5) + 1
    beta = (qs >> 4) + 1
    tc = np.maximum(1, (bs * qs) >> 6)
    cond = (bs > 0) & (np.abs(P0 - Q0) < alpha) & (np.abs(P1 - P0) < beta) & (np.abs(Q1 - Q0) < beta)
    d = np.clip(((Q0 - P0) * 4 + (P1 - Q1) + 4) >> 3, -tc, tc)
    nP0 = np.where(cond, np.clip(P0 + d, 0, 255), P0)
    nQ0 = np.where(cond, np.clip(Q0 - d, 0, 255), Q0)
    return nP0, nQ0


def deblock(Y, Cb, Cr, mbw, mbh, mtype, mmv, mcoded, mqp, muses, qpel=0):
    thr = 4 if qpel else 2
    def differs(pm, qm):
        up = muses[pm[1]][pm[0]]
        uq = muses[qm[1]][qm[0]]
        if up != uq:
            return True
        for L in (0, 1):
            if up >> L & 1:
                a = mmv[pm[1]][pm[0]][L]
                b = mmv[qm[1]][qm[0]][L]
                if abs(a[0] - b[0]) >= thr or abs(a[1] - b[1]) >= thr:
                    return True
        return False

    W = mbw * 16
    H = mbh * 16

    def bs_luma(pbx, pby, qbx, qby):
        pm = (pbx >> 1, pby >> 1)
        qm = (qbx >> 1, qby >> 1)
        if mtype[pm[1]][pm[0]] == INTRA or mtype[qm[1]][qm[0]] == INTRA:
            return 2
        pi = 2 * (pby & 1) + (pbx & 1)
        qi = 2 * (qby & 1) + (qbx & 1)
        if (mcoded[pm[1]][pm[0]] >> pi) & 1 or (mcoded[qm[1]][qm[0]] >> qi) & 1:
            return 1
        if pm != qm and differs(pm, qm):
            return 1
        return 0

    def bs_chroma(pm, qm):
        if mtype[pm[1]][pm[0]] == INTRA or mtype[qm[1]][qm[0]] == INTRA:
            return 2
        if mcoded[pm[1]][pm[0]] != 0 or mcoded[qm[1]][qm[0]] != 0:
            return 1
        if differs(pm, qm):
            return 1
        return 0

    def qpavg(pm, qm):
        return (mqp[pm[1]][pm[0]] + mqp[qm[1]][qm[0]] + 1) >> 1

    def apply_v(pl, x, bs, qp):
        bs = np.repeat(np.array(bs, dtype=np.int64), 8)
        qp = np.repeat(np.array(qp, dtype=np.int64), 8)
        nP0, nQ0 = filt(pl[:, x - 2].copy(), pl[:, x - 1].copy(), pl[:, x].copy(), pl[:, x + 1].copy(), bs, qp)
        pl[:, x - 1] = nP0
        pl[:, x] = nQ0

    def apply_h(pl, y, bs, qp):
        bs = np.repeat(np.array(bs, dtype=np.int64), 8)
        qp = np.repeat(np.array(qp, dtype=np.int64), 8)
        nP0, nQ0 = filt(pl[y - 2, :].copy(), pl[y - 1, :].copy(), pl[y, :].copy(), pl[y + 1, :].copy(), bs, qp)
        pl[y - 1, :] = nP0
        pl[y, :] = nQ0

    for x in range(8, W - 7, 8):
        u = x // 8
        n = H // 8
        apply_v(Y, x, [bs_luma(u - 1, j, u, j) for j in range(n)],
                [qpavg(((u - 1) >> 1, j >> 1), (u >> 1, j >> 1)) for j in range(n)])
    for y in range(8, H - 7, 8):
        u = y // 8
        n = W // 8
        apply_h(Y, y, [bs_luma(i, u - 1, i, u) for i in range(n)],
                [qpavg((i >> 1, (u - 1) >> 1), (i >> 1, u >> 1)) for i in range(n)])
    for pl in (Cb, Cr):
        for x in range(8, W // 2 - 7, 8):
            u = x // 8
            apply_v(pl, x, [bs_chroma((u - 1, j), (u, j)) for j in range(mbh)],
                    [qpavg((u - 1, j), (u, j)) for j in range(mbh)])
        for y in range(8, H // 2 - 7, 8):
            u = y // 8
            apply_h(pl, y, [bs_chroma((i, u - 1), (i, u)) for i in range(mbw)],
                    [qpavg((i, u - 1), (i, u)) for i in range(mbw)])


LEVELS = {1: (400, 240, 375, 11250, 96000, 200000), 2: (800, 464, 1450, 43500, 256000, 400000),
          3: (800, 720, 2250, 67500, 384000, 600000)}


def decode_file(path, out, verbose=False):
    d = open(path, 'rb').read()
    magic, w, h, fn, fd, n = struct.unpack('<4sHHHHI', d[:16])
    level = 0
    if magic == b'TVC1':
        level = d[16]
        pos = 20
        if level not in LEVELS:
            raise LevelViolation('level byte')
    else:
        assert magic == b'TVC0'
        pos = 16
    Wc, Hc = (w + 15) & ~15, (h + 15) & ~15
    if level:
        mw, mh, mbp, mbs, bpp, bps = LEVELS[level]
        mbn = (Wc // 16) * (Hc // 16)
        if Wc > mw or Hc > mh or mbn > mbp or fn > 30 * fd or mbn * fn > mbs * fd:
            raise LevelViolation('size/rate')
        # first pass over the picture lengths
        lens = []
        q = pos
        for i in range(n):
            ln = struct.unpack('<I', d[q:q + 4])[0]
            lens.append(ln)
            q += 4 + ln
        if any(x > bpp for x in lens):
            raise LevelViolation('bytes per picture')
        win = -(-fn // fd)
        run = 0
        for i in range(n):
            run += lens[i]
            if i >= win:
                run -= lens[i - win]
            if run > bps:
                raise LevelViolation('bytes per second')
    o = open(out, 'wb')
    o.write(('YUV4MPEG2 W%d H%d F%d:%d Ip A1:1 C420jpeg\n' % (w, h, fn, fd)).encode())
    latest = prev = None
    held = None
    cw, ch = (w + 1) // 2, (h + 1) // 2

    def emit(pl):
        o.write(b'FRAME\n')
        o.write(pl[0][:h, :w].astype(np.uint8).tobytes())
        o.write(pl[1][:ch, :cw].astype(np.uint8).tobytes())
        o.write(pl[2][:ch, :cw].astype(np.uint8).tobytes())

    for i in range(n):
        ln = struct.unpack('<I', d[pos:pos + 4])[0]
        pos += 4
        pic = d[pos:pos + ln]
        pos += ln
        t0 = time.time()
        if pic[0] == 1:
            refs = (latest, None)
        elif pic[0] == 2:
            refs = (prev, latest)
        else:
            refs = (None, None)
        out_pl, stats, rc = decode_picture(pic, w, h, refs, level)
        if verbose:
            print('pic %d type %d len %d rc_pos %d  %s  %.2fs' % (i, pic[0], ln, rc.p - 3, stats, time.time() - t0),
                  file=sys.stderr)
        if pic[0] == 2:
            emit(out_pl)
        else:
            if held is not None:
                emit(held)
            held = out_pl
            prev = latest
            latest = out_pl
    if held is not None:
        emit(held)
    o.close()


if __name__ == '__main__':
    try:
        decode_file(sys.argv[1], sys.argv[2], verbose=len(sys.argv) > 3)
    except LevelViolation as e:
        import os
        print('level violation:', e, file=sys.stderr)
        try:
            os.remove(sys.argv[2])
        except OSError:
            pass
        sys.exit(3)
