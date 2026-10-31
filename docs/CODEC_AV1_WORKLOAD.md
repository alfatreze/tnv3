# AV1 decoder workload from real streams (codec phase 1, 2026-10-21)

What an AV1-lean decoder has to do, measured on real streams instead of guessed. Every clip (9 clips: 3 live action,
3 anime, 3 black-and-white, 800 px wide, 10 s, 24 fps) was encoded with libaom (cpu-used 4, constrained quality 44,
about VMAF 88-94) in three tool sets: `full`, `lean0` (25 zero-value tools off) and `lean2` (also no loop restoration,
no rectangular partitions, blocks up to 32x32, 4-layer pyramid, 3 references; see `docs/CODEC_PHASE0B.md`). The streams
were decoded with a libaom build with `CONFIG_ACCOUNTING` and `CONFIG_INSPECTION` (study tool
`tools/codec_study/av1stats.c`, driver `av1_workload.py`), which reports every symbol read, its cost in bits and the
mode information of every block. Raw per-stream records for the `full` set: `docs/codec_phase0/av1/`; the tables:
`docs/codec_phase0/av1_workload_tables.md`, `av1_itx_load.md`.

## Symbol rate: the entropy engine has room

| | mean symbols/s | bits per symbol | worst frame (symbols) | worst 1 s window (symbols) |
|---|---|---|---|---|
| full | 109 k - 416 k | 0.83 - 0.91 | 77 k - 172 k | 0.23 M - 0.67 M |
| lean0 | 127 k - 435 k | 0.80 - 0.90 | 86 k - 184 k | 0.25 M - 0.70 M |
| lean2 | 210 k - 528 k | 0.54 - 0.83 | 86 k - 183 k | 0.32 M - 0.81 M |

A symbol is one read of the arithmetic decoder (binary or multi-symbol). The probability-adaptive decoder of
`docs/CODEC_AREA_PROBES.md` takes about 28 clocks a symbol (3.6 M/s at 100 MHz, est.): the worst second is 23% of a
100 MHz second, the mean 3-15%. The worst single frame (a key frame or a hidden reference frame, up to 184 k symbols) is
5.2 M clocks, 124% of one frame time, so it needs a decode-ahead queue or a prefetching variant of the engine; it does
not need a faster clock. lean2 reads more symbols for the same quality because its blocks are smaller and rectangles
are gone; it costs bits and symbols together.

## Where the bits and the work are (share of all bits)

* **Coefficients 58-76%** (full and lean alike), motion vectors 8-17%, inter modes + references + skip flags 8-17%,
  partition 2-5%, intra modes 1-10% (more with full tools: CfL, filter-intra, palette), loop-filter parameters and the
  rest about 2%. The coefficient parser, with its context derivation, is the block that matters most.

## Tool usage with all tools enabled (area-weighted, per clip)

| | range over the 9 clips |
|---|---|
| blocks skipped (no residual) | 73-97% |
| OBMC | **0.0%** of inter area |
| warped motion | 0.9-7.8% |
| non-average compound (distance, wedge, difference-weighted) | 0-11% |
| bi-prediction (two references) | **52-71%** of inter area |
| palette, intra block copy | negligible (a few thousand 4x4 units in total) |
| 64x64 blocks | 43-84% of the area; blocks of 16x16 and smaller 4-21% |
| vertical motion vector p50 / p90 / p99 / p99.9 (px) | 0-2 / 0-14 / 3-55 / 10-101 |
| horizontal motion vector p99 (px) | 3-48 |

* OBMC being 0.0% matches its 0% value in the tool study; masked compound and warped motion are used in a minority of
  blocks yet are worth about nothing in bits, which is why the lean profiles can drop them.
* **Bi-prediction is the norm in AV1** (52-71%), unlike x264 (2-6%): motion compensation reads two references for
  more than half of the inter area. Reference-fetch bandwidth, not arithmetic, is what compound prediction costs, and
  the memory system has to be sized for up to two reference reads per predicted pixel (about 30-50 MB/s for 800x450 at
  24 fps with 8-tap filters, est. from 540 KB a frame, 1.6 reference reads on average and an overfetch of 1.3-2; SDRAM is
  not the limit).
* Vertical vector reach: a +-16 row band covers at least 90% of the area on all nine clips and at least 99% on six; the
  three busiest (dark, action, black-and-white garden) need +-22 to +-55 for 99%, and +-64 covers 99.9% on eight of
  nine (action reaches 101). AV1 encoders have no vertical-range limit option (libaom, SVT), so a reference cache has to
  cope with misses or the transcoder has to clamp.

## Consequences for the design

1. One arithmetic decoder lane at about 28 clocks a symbol is enough on average and over a second; size the queue,
   not the clock.
2. Spend design effort on the coefficient parser and the transform/reconstruction path: they carry about two thirds of
   the bits.
3. Motion compensation must handle two references per block for the majority of inter blocks, with 8-tap sub-pel
   filters; OBMC, warped and masked compound can stay out of the decoder.
4. One transform lane is enough with decode-ahead (see the area report); a second doubles the margin cheaply.

## Limits

One quality point (cq 44), one encoder (libaom cpu-used 4), 800 px wide, 10 s clips; hidden reference frames are
counted in the per-frame numbers but "peak 1 s window" is taken over decode order, not display order; compound type
codes were read from libaom's enum (0 average, 1 distance, 2 wedge, 3 difference-weighted); transform blocks are
sized from the block's transform size, the number of coded rows per block is not available from the inspection hook
(two cases are bracketed instead).
