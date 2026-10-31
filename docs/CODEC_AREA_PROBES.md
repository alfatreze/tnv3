# Codec area probes (phase 1): what each block of an AV1-lean decoder costs on the Pocket's FPGA

Running table, newest block first. Method: each block gets a small wrapper (`src/fpga/probe/`), a standalone Quartus
project on the build VM (`tools/vm_probe.py`: Cyclone V 5CEBA4F23C8, speed grade 8, the same high-performance fitter
settings as the real core, multicorner timing at 100 MHz) and a full compile (map, fit, sta). Every port except the
clock is a virtual pin, so no pins or I/O logic are spent. These are **lower bounds on the cost inside the core**: no
routing congestion, no shared clocks, and anything the wrapper contains (a RAM) is counted. Compile time is about 11
minutes per probe on the VM (estimate for planning: 10-12 min).

The blocks come from the profile in `docs/CODEC_PHASE0B.md` section 7. Budget: the **whole device**, 18,480 ALMs, 308
M10K, 66 DSP. The existing Tau nu build (V-035: 10,263 ALMs, 251 M10K, 28 DSP) is NOT a constraint: owner decision
2026-10-21, the existing fabric can be deleted and redone, and the design aims at the optimum for a clean video core
(video decoder, audio, a minimal controller), not at fitting beside what is there today. Compare each probe against the
device, not against what is left over.

## 0. Block-level syntax unit (`tau_av1_blk`), first RTL (V-072)

Functionally complete for the lean2 tool set (key and inter frames, all of `docs/CODEC_AV1_BLOCK_SYNTAX.md`), verified against libaom on nine streams. **This first RTL is a golden implementation, not an optimised one**: probe `av1blk3` (32-row mode-info ring, 93-bit records):

| revision | ALMs | registers | M10K | DSP | Fmax slow 0C | slack at 100 MHz |
|---|---|---|---|---|---|---|
| 1 straightforward FSM | **8,680** (47%) | 6,266 | 94 (758 kbit) | 13 | **50.6 MHz** | **-9.8 ns** (fails) |
| 2 pipelined compare, MLAB stacks/maps, logic multipliers (`av1blk5`, 64-row ring) | 7,821 (42%) | 3,646 | 186 | 0 | 68.2 MHz | -4.7 ns |
| 3 + clamp bounds and transform-block decisions registered (`av1blk7`, probed at 66 MHz) | **7,755** (42%) | 3,706 | 186 (64-row ring: 1.5 Mbit) | 0 | **72.2 / 70.5 MHz** | **+0.97 ns at 66 MHz (closes)** |

* Critical path: a candidate record -> `lowmv` precision lowering -> eight parallel 64-bit comparisons against the stack -> weight add (19 ns). The ALM count is dominated by wide variable-index register arrays (MV stack of 8 x 2 x 32-bit entries with swaps, block-local transform maps, task stacks), eight parallel comparators and the one-hot-ish 300-state controller; the 13 DSPs are index multiplications by constants that should be shifts.
* Memory: the mode-info ring is 93 bits x 8192 records = 745 kbit; with 64 rows (needed for 128x128 superblocks) it doubles. Cheaper: a 64x64-superblock-only profile (32 rows), fewer bits per record (motion vectors 16 -> 12 bits, per-row line buffers for rows -3/-5 sampled at 8x8 granularity).
* Next revision (plan): register the candidate before comparing (two cycles per candidate; speed does not matter, ~100 clocks per symbol are available), compare serially or against a CAM-less index, turn the stack and local maps into small RAMs, replace constant multiplies by shifts. Target: well under 3,000 ALMs and 100 MHz.
* The unit needs only ~17 M cycles a second (about 170 k symbols/s x ~100 clocks), so **66 MHz is the target clock**, not 100: revision 3 closes there. The remaining problem is area: 7.7 k ALMs is far above the 3 k target; the logic is a 300-state controller with many 32-bit datapath muxes and ROM-function wires, not memory (the 186 M10K are the mode-info ring). Next: find the dominant parts (synthesise with sections removed), narrow the record (16-bit -> 12-bit vectors, drop the 128x128 ring half), and split the controller into smaller units.
* Verified after each round with the committed trace, the 15 mutants and full-stream replays of bw_garden, bw_closeup, bw_room, dark (all pass).

## 1. Sub-pel motion compensation, two references (`tau_av1_mc`, `tau_av1_mc2`)

One block prediction from one or two references, unscaled (8-bit): the separable 8-tap interpolation of the AV1 block inter
prediction process. A **lane** (`tau_av1_mc`) takes the reference window as a raster stream, one sample a clock, and returns the
block as a raster stream: horizontal 8-tap pass (`Round2(sum, 3)`), seven previous rows of intermediates in a line buffer (one wide
RAM word per column), vertical 8-tap pass (`Round2(sum, 11)` for a single prediction, `Round2(sum, 7)` raw for a lane of a
bi-prediction). `tau_av1_mc2` is two lanes plus the compound average `Clip1(Round2(p0 + p1, 5))`; the lanes have their own streams,
kernels and window geometry and stall each other through the handshake. Block sides 2-64 (2x2 chroma blocks included), the four
interpolation types, the 4-tap kernels the specification uses for sides <= 4, both directions independent. A zero fractional
position uses the identity kernel and needs none of the 7 extra columns (rows): the fetcher reads only `w + (fx ? 7 : 0)` columns, so
integer-position blocks cost no overfetch. Edge replication is the fetcher's job (not built yet). The kernel ROM
(`tau_av1_mc_rom.svh`) is the normative Subpel_Filters table read mechanically from libaom with its notice kept
(`tools/gen_av1_mc_rom.py`; every kernel sums to 128). Not in the lean set: distance-weighted and masked compound, OBMC, warped
motion, scaled references.

Verification (`make test-rtl-av1-mc`, `-stall`, `-mutation`, `-full`): the expected pixels come from **libaom's own
`av1_convolve_2d_facade`** with the ConvolveParams its decoder builds. The committed set is 90 blocks (sides 2-16, all interpolation
types, every integer / x-only / y-only / both position class, single and bi-prediction, random, 0/255, smooth, flat and
filter-adversarial reference patterns); `-full` runs 300 blocks up to 64x64 and diffs the regenerated kernel ROM. All pixels
equal, also with random gaps on both sample streams and random back-pressure on the output. Thirteen mutants (horizontal and
vertical rounding, tap order, 4-tap kernels ignored, smooth 4-tap, vertical type, compound shift, valid thresholds, no clip, row
order, bilinear kernel, average rounding) are all killed.

| revision | ALMs | registers | M10K | DSP | Fmax slow 0C | slack at 100 MHz (0C) |
|---|---|---|---|---|---|---|
| 1 (two lanes + average) | **581** (3.1%) | 717 | 6 | 16 | **117.9 MHz** | **+1.52 ns** (closes) |

* About 290 ALMs, 3 M10K and 8 DSP per lane. The multipliers are 8x9 (horizontal) and 14x9 (vertical) bit; coefficients are
  loaded once per block. Widths: horizontal intermediates fit 14 bits signed, the vertical sum 22 bits (checked against the filter
  tables: positive taps sum to at most 184, negative to at most 56).
* **Throughput:** one input sample a clock, latency about 14 clocks, so a block costs `(w + 7) x (h + 7)` clocks for a fractional
  position in both directions (64x64: 5,041 for 4,096 pixels; 8x8: 225 for 64; 4x4: 121 for 16). The worst case (everything in 4x4
  blocks, fractional both ways, bi-predicted; est.) is tens of millions of clocks a second at 400x176 and 24 fps; in real streams
  blocks of 16x16 and smaller cover only 4-21% of the area (`docs/CODEC_AV1_WORKLOAD.md`), so the typical cost is a small share
  of a 100 MHz second (est.). A second sample
  per clock per lane (two window columns) would double that for about 300 ALMs and 8 DSP, if ever needed.
* **The open question is the fetcher, not the arithmetic**: two reference streams of `(w + 7) x (h + 7)` samples with edge
  replication from SDRAM (row bursts, the vertical reach of the vectors, see `docs/CODEC_AV1_WORKLOAD.md`), the next piece.

## 2. Coefficient parser (`tau_av1_coef`)

The coefficients of one transform block, from the arithmetic decoder to dequantised values in the transform engine's RAM:
all_zero, then (through a handshake with the block-level syntax) the transform type, eob_pt and eob_extra, the coefficient
at the end of the scan, every other coefficient in reverse scan order (a base-level symbol with a context from five
neighbouring levels, then up to four range symbols), then signs, Golomb tails and dequantisation in forward order. Serial by
design: one request at a time to `tau_av1_symdec`, neighbour levels one per clock from a 4-bit RAM padded like libaom's (stride
height + 4) that is zero between blocks because every entry written in the first pass is cleared when the second pass
consumes it (no per-block clearing). 2D scans from a generated 3,344-entry ROM (rule-generated by
`tools/gen_av1_coef_rom.py` and checked against libaom's tables), 1D scans by formula, context offsets by rule. Written from the
AV1 specification; libaom (BSD-2) is the oracle.

Verification, four layers:
1. a **Python model** (`sim/av1_coef_model.py`) of the whole coefficient syntax against a trace of libaom's own decoder
   (`sim/av1/patches/decodetxb_trace.patch` logs every symbol read, the block's contexts and its results): **570,000 real
   transform blocks** from 27 streams (9 clips x 3 tool sets), every request address, alphabet size, bit and every dequantised
   coefficient equal (64-point sizes included; a 2D context offset that uses the original block shape was found this way);
2. the **RTL** against 174 blocks (every size/plane/class bucket present in the streams, 10 with Golomb tails, 6 all-zero, 50
   synthetic stress blocks with extreme quantisers and long tails) with the arithmetic decoder replaced by a replay of libaom's
   symbols: every request must equal libaom's read and every coefficient write its result;
3. **end to end**: libaom's range ENCODER codes the same symbol sequences (random valid initial CDFs, libaom's adaptation), the
   RTL range decoder and the parser decode them back: all 174 blocks (70 committed) give libaom's coefficients, context byte,
   rows used **and the adapted CDF words**;
4. fifteen mutants (neighbour clipping, 2D offset shape, range-context threshold, Golomb length, dc-sign context, dequant shift,
   dc/ac swap, eob bit weight, dc sign read as a bit, rows used, range-symbol limit, 1D scan, dequant clamp, CDF set, level clip)
   are all killed; one needed the synthetic stress blocks (the dequantisation clamp never triggers on real streams).

| revision | ALMs | registers | M10K | DSP | Fmax slow 85C / 0C | slack at 100 MHz (0C) |
|---|---|---|---|---|---|---|
| 1 | 664 | 483 | 6 | 5 | 78.8 / 76.2 MHz | -3.13 ns (neighbour sums -> context -> CDF address in one clock) |
| 2 neighbour sums, context and address on three clocks | **666** (3.6%) | 505 | 6 | 5 | **98.0 / 92.7 MHz** | -0.79 ns |

* Memory is 46 Kbit: the scan ROM (33 Kbit, inferred into block RAM) and the level RAM. The five DSPs are constant multiplies in the CDF
  address arithmetic and the dequantisation multiplier; they can be moved to logic.
* **Speed:** with a 4-clock decoder the parser itself adds about 15 clocks a symbol (19 measured); with the real range decoder the
  end-to-end rate is **32.7 clocks a symbol** over the 174 blocks (218,813 clocks for 6,690 symbols, block overheads included), i.e.
  about 3.0 M symbols a second at 100 MHz (est.). Against the real stream load (`docs/CODEC_AV1_WORKLOAD.md`: at most 0.81 M symbols
  in the worst second, 184 k in the worst frame): 27% of the worst second, and the worst frame is 6.0 M clocks, 144% of a frame
  time, so the decode-ahead queue is needed, not a faster clock.
* The parser, the range decoder and the transform engine together are **1,962 ALMs (10.6% of the device)** and about 32 block RAMs
  (17 of them the full CDF set).

## 3. Inverse transform engine (`tau_av1_itx`)

DCT 4-32, ADST 4-16 and identity 4-32 in all 14 block shapes up to 32x32 (4x4 .. 32x32, 4x8, 8x4, 8x16, 16x8, 16x32,
32x16, 4x16, 16x4, 8x32, 32x8), the 16 transform types with flips, rows of zeros skipped. **Micro-coded**: one datapath with
two multipliers does every step (a coefficient load, an add, a rotation, a rounding shift, a store) as the same
operation `(w0*A + w1*B [+ acc] + 2048) >>> 12`, optionally clamped to 16 bits; the 1D transforms are programs in a
506-word ROM generated mechanically from the normative butterfly structure (`tools/gen_av1_itx_prog.py`: parse, value
graph, latency-aware list scheduling, liveness-based slot allocation). Written from the AV1 specification's algorithm;
libaom (BSD-2) is the oracle and the source the structure is read from, with its notice kept in the generated file.

Verification, three layers (`make test-rtl-av1-itx`, `-mutation`, `-full`):
1. the generated programs against libaom's 1D functions on 3,300 vectors incl. the +-32768 extremes (cycle-exact
   model of the pipeline, no hazards): 0 mismatches. Widest stored result 19 bits, widest unrounded sum 31 bits;
2. a Python 2D model (input scaling of 2:1 shapes, clamps, rounding shifts, flips, zero-row skipping) against libaom's
   `av1_inv_txfm2d_add_*_c` pixel for pixel on 400 blocks, before any RTL;
3. the RTL against libaom's 2D transform plus reconstruction: the committed coverage set (146 blocks: every valid
   shape/type pair, big shapes with three types) and 400 random blocks, **all pixels equal**. Thirteen mutants (program
   clamps, 1/sqrt2 scaling, both flips, both rounding shifts, zero-row skipping, row-store clamp, accumulator,
   rounding constant, load-to-program wait, weight sign, rect detection) are all killed.
Two bugs were found on the way and fixed: the RTL weight table lacked five entries (the Python model had hidden it, the
RTL test caught it) and the generator's slot allocation depended on hash order (the ROM is now deterministic and `-full`
diffs it against a fresh generation).

| revision | ALMs | registers | M10K | DSP | Fmax slow 85C / 0C | slack at 100 MHz (0C) |
|---|---|---|---|---|---|---|
| 1 | **848** (4.6%) | 348 | 9 | 2 | **132.5 / 122.7 MHz** | **+1.85 ns** (closes) |

* Memory is 53.8 Kbit: coefficients 16 Kbit, intermediate 16 Kbit, residual 18 Kbit, two 64x20 slot RAMs. The ROM is in logic
  (about 300-400 of the ALMs, est.); an M10K ROM would save most of that.
* **Cycles**: per 1D transform `N + 5 + program + 5 + N + 1`, programs 11 / 26 / 74 / 194 cycles for 4 / 8 / 16 / 32
  points; a dense 8x8 block is about 850 cycles, a dense 32x32 12,043, the coverage mean 1,172.
* **Against real streams** (libaom AV1, 800 px wide, `docs/CODEC_AV1_WORKLOAD.md` and `docs/codec_phase0/av1_itx_load.md`):
  coded blocks cost on average 0.43-1.61 M cycles a frame with every row of every block coded (an upper bound), 10-39% of
  a 41.7 ms frame at 100 MHz; the worst frames (key frames, hidden reference frames) 3.7-7.8 M, 89-186%. One lane is
  enough with a decode-ahead queue of a few frames; a second lane doubles the margin for about 400 ALMs and 2 DSP (est.).
  With only four rows coded per block the mean is 0.27-0.98 M and the worst 2.5-4.6 M.

## 4. Adaptive multi-symbol range decoder (`tau_av1_symdec`)

Written from the AV1 specification (symbol decoder, CDF adaptation). Serial: one CDF entry per few clocks, one small
multiplier, CDFs in an external one-word-wide RAM, bool at probability 1/2 without memory access.

Verification (`make test-rtl-av1-symdec`, `-mutation`, `-full`): streams made by **libaom's own range coder and CDF
adaptation** (libaom v3.15.2 as a test oracle only, nothing copied). 200 random streams, 66,260 symbols (2-16 symbol
alphabets, binary and ternary favoured, adaptation on in 80% of streams, saturating counters): every decoded symbol
and every CDF word after the stream equals the reference. The committed small set is 40 streams, 2,445 symbols. Ten
mutants (adaptation rate, counter, comparison, minimum-probability term, renormalisation shift, initial value, last
adapted entry, bool probability, counter saturation, alphabet threshold) are all killed; one first mutant survived
because it was equivalent (the probability is truncated to 10 bits) and was replaced by a distinguishable one.

| revision | ALMs | registers | M10K (wrapper RAM) | DSP | Fmax slow 85C / 0C | slack at 100 MHz (0C) | cycles per symbol |
|---|---|---|---|---|---|---|---|
| 1 first RTL | 434 | 172 | 2 | 1 | 89.3 / 88.1 MHz | -1.35 ns | 21.0 mean, 85 worst |
| 2 write data registered | 437 | 189 | 2 | 1 | 93.7 / 91.1 MHz | -0.97 ns | 25.4 mean, 100 worst |
| 3 CDF read registered | **448** | 206 | 2 | 1 | **97.3 / 93.6 MHz** | -0.69 ns | **27.9 mean, 116 worst** |

* **Area: 448 ALMs, 2.4% of the device.** The entropy engine is not a sizeable cost. The one DSP is the 8x10-bit
  range multiplier; it could be forced into logic if DSPs are needed elsewhere (not tried).
* **Speed: about 28 clocks per symbol on average**, so 3.6 million symbols a second at 100 MHz (est.); the worst case
  is 116 clocks (16-symbol alphabet that decodes the last symbol, with adaptation). **Measured on real streams**
  (`docs/CODEC_AV1_WORKLOAD.md`): 109 k - 528 k symbols a second on average and at most 0.81 M in the worst second, so the
  engine runs at 3-15% load on average and 23% in the worst second; only the single worst frame (a key or hidden reference
  frame, up to 184 k symbols = 5.2 M clocks, 124% of a frame) needs a decode-ahead queue. The engine can still be sped up
  (prefetch the next CDF entry, adapt while the next symbol starts) at a cost of a few dozen ALMs if the queue is unwanted.
* **Timing:** closes at about 93 MHz in the slowest corner, not 100. The remaining critical path is RAM read to
  register; it would close with the RAM's own output register. Fine for the 66 MHz clock domain, one more pipeline
  step for 100 MHz.
* **CDF memory is the real cost.** libaom's `FRAME_CONTEXT` (the full AV1 CDF set plus a few small fields) is 21,244
  bytes = 170 Kbit = about 17 M10K blocks at full AV1, with its padded 16-entry alphabets. A lean profile that drops
  palette, intra block copy, CfL, filter-intra, compound masks and the other zero-value tools removes a share of
  them (the coefficient CDFs, which dominate, stay). Eight saved contexts per frame set would sit in SDRAM. So the
  entropy decoder logic is tiny and its tables are the larger part (about 5% of the device's block RAM at full AV1);
  that is affordable on a clean core, and a lean profile shrinks it.

## Next probes

Done: the range decoder, the transform engine, the coefficient parser and the 8-tap motion compensation arithmetic. In order of how
much the rest decides (compound prediction is the norm in real AV1 streams): the **reference fetch** for the motion compensation
(edge replication, row bursts from SDRAM), the directional intra predictor, the deblocking filter, then the **RTL of the block-level syntax** (its model is done and verified: `docs/CODEC_AV1_BLOCK_SYNTAX.md`) and the frame
level (headers, tile and frame CDF contexts). Each follows the same recipe: RTL from the specification, vectors from libaom as
an oracle, mutants, standalone probe.

## tau_av1_blk area ablations (V-074, map-only, ALM estimate; baseline av1blk7 = 7,825 ALMs, 3,706 regs)

One group of controller states emptied per variant (`build/abl/`). Drops overlap (removing a group also removes logic only it fed), so they do not add up.

| variant (group emptied) | ALMs | drop | regs | block bits |
|---|---|---|---|---|
| abl_stack (reference-MV stack) | 4,176 | 3,649 | 2,351 | 413,920 (mode-info ring unused, so part of the drop is the ring read-out) |
| abl_vt (variable transform tree) | 5,296 | 2,529 | 2,367 | 1,511,648 |
| abl_ref (reference-frame syntax and contexts) | 5,702 | 2,123 | 2,964 | 1,511,648 |
| abl_part (partition) | 6,682 | 1,143 | 3,406 | 1,511,648 |
| abl_tr (transform-block walk) | 7,071 | 754 | 3,665 | 1,511,648 |
| abl_misc | 7,414 | 411 | 3,633 | 1,511,648 |
| abl_mvc (motion-vector coding) | 7,536 | 289 | 3,649 | 1,511,648 |
| abl_coefctx | 7,269 | 252 (556 incl. 4,320 bits) | 3,573 | 1,507,328 |

Reading: the area is in the MV-stack micro-sequence, the variable transform tree and the reference-frame contexts (about 7.3 k of 7.8 k between them, with overlap). Target < 3 k ALMs needs those three restructured, not trimmed.
