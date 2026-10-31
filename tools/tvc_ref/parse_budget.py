#!/usr/bin/env python3
"""How far behind the display can the TVC parser fall on a legal stream, and how many decoded pictures must be buffered ahead (docs/TVC_PARSER_DESIGN.md
section 6)?  The level rules (TVC_BITSTREAM_V0.md section 8) bound every picture (maxpic bytes) and any ceil(fps) consecutive pictures (window bytes).
The parser needs  clocks = A * bins + B * macroblocks  per picture (measured on the 21 vectors: A = 3.17 clocks per bin with the fast engine, B = 35), and a
legal stream has bins = bpb * bytes (measured 9.4 on average, 8.0 to 12.4).  The worst legal stream, for the lag, packs as many bytes as the rules allow into
the first pictures: after j pictures at most min(j * maxpic, window * ceil(j / N)) bytes.
  lag(j) = sum of the parse times of the first j pictures - j display periods;   pictures to buffer ahead = ceil(max lag / period) (+ 1 for the one being parsed).
Usage: parse_budget.py"""
import math

LEVELS = {1: dict(mbs=375, maxpic=96_000, window=200_000), 2: dict(mbs=1450, maxpic=256_000, window=400_000), 3: dict(mbs=2250, maxpic=384_000, window=600_000)}

def worst_lag(level, fps, clk_hz, a, b=35.0, bpb=9.4, parsers=1, slice_eff=1.0, tiny=400):
    L = LEVELS[level]; n = math.ceil(fps); period = 1.0 / fps
    per_byte = a * bpb / clk_hz / (parsers * slice_eff)              # seconds per byte of picture data
    per_mb = b * L['mbs'] / clk_hz / (parsers * slice_eff)           # per-macroblock overhead of a picture
    best = (0.0, 0)
    for j in range(1, 4 * n + 1):
        by = min(j * L['maxpic'], L['window'] * math.ceil(j / n))
        # the other pictures of the j are at least 'tiny' bytes: they cannot all be big, so use the byte bound as the total
        by = max(by, j * tiny)
        lag = j * per_mb + by * per_byte - j * period
        if lag > best[0]: best = (lag, j)
    sustained = (n * per_mb + L['window'] * per_byte) / (n * period)       # fraction of the time the parser is busy over a full window
    return best[0], best[1], sustained, per_byte

if __name__ == "__main__":
    print("level fps  parsers  A(clk/bin)  clk MHz | parse rate | sustained load | worst lag, after j pictures | pictures to buffer ahead | startup latency")
    for level in (3, 2, 1):
        for fps in (30, 24):
            for parsers, a, clk in ((1, 3.17, 66.667), (2, 3.17, 66.667), (1, 2.2, 66.667), (1, 3.17, 100.0)):
                lag, j, load, per_byte = worst_lag(level, fps, clk * 1e6, a, parsers=parsers)
                period = 1 / fps; ahead = math.ceil(lag / period) + 1 if lag > 0 else 1
                print(f"  {level}   {fps}    {parsers}       {a:4.2f}      {clk:6.1f}  | {1 / per_byte / 1e6:5.2f} MB/s | {100 * load:5.1f} %        | {lag * 1000:6.0f} ms (j = {j:2d})        | {ahead:2d}                       | {ahead * period * 1000:4.0f} ms")
