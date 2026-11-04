#!/usr/bin/env python3
import glob, os, sys, time, wave
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tac_dec

vd = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'vectors')
tot_t = 0
tot_s = 0
for f in sorted(glob.glob(os.path.join(vd, '*.tac'))):
    name = os.path.basename(f)
    t0 = time.time()
    try:
        rate, ch, pcm, st = tac_dec.decode(f)
    except Exception as e:
        print(name, 'FAIL exception', repr(e))
        continue
    dt = time.time() - t0
    with wave.open(f[:-4] + '.ref.wav') as w:
        rch = w.getnchannels()
        ref = np.frombuffer(w.readframes(w.getnframes()), '<i2').reshape(-1, rch)
    tot_t += dt
    tot_s += len(pcm) / rate
    if ref.shape != pcm.shape:
        print(name, 'FAIL shape', ref.shape, pcm.shape)
        continue
    d = pcm.astype(float) - ref.astype(float)
    mx = np.abs(d).max()
    rms = np.sqrt((d ** 2).mean())
    ok = mx <= 1 and rms <= 0.3   # tolerance of spec section 7 (draft 1.2)
    print('%s %s maxabs=%d rms=%.4f  %.2fs (%.1fx realtime)' % (name, 'PASS' if ok else 'FAIL', mx, rms, dt, len(pcm) / rate / dt))
    print('   frames LONG/START/SHORT/STOP =', st['wtype'], 'reset=1 frames', st['resets'],
          'msmode', st['msmode'], 'noise-filled bands', st['filled'], 'max|q|', st['maxq'], 'max sf', st['maxsf'])
print('total decode %.2fs for %.1fs audio' % (tot_t, tot_s))
