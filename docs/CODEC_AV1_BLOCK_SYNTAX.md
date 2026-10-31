# AV1 block-level syntax: the model (phase 1, V-071)

`sim/av1_blk_model.py` is the executable specification of everything between the frame header and the coefficient parser: the
syntax of one tile, derived the way the RTL has to derive it. It asks for one symbol at a time (naming a CDF by syntax element and
context indices, and the alphabet size), derives mode info, motion vectors, transform sizes and types, and the contexts handed to
`tau_av1_coef`, and is checked against **libaom's own decoder**: a patched copy (`sim/av1/patches/blocktrace.patch`, a study aid,
BSD-2 like the file it patches) logs every arithmetic-decoder read of the block-level syntax with the word offset of the CDF inside
`FRAME_CONTEXT`, block markers, the mode info of every block and the coefficient parser's inputs and outputs. Any request that
differs from libaom's read, any derived value that differs, or any leftover record fails the run.

**Result:** the nine `lean2` streams (800x450, ten seconds each, nine clips: film, anime, B&W TV) run to the end: **1,381,000 blocks,
~15.1 M block-level symbols, 1.1 M transform blocks, every request and every derived value equal** (`make test-av1-blk-model-full`;
the committed regression trace, 10 frames, runs in `make test-av1-blk-model`). Frame types: key frames and inter frames with
single and compound references.

## What is modelled (lean2 tool set) and what is not
Modelled: partition (all ten partition types written, but see below), skip mode, skip, delta-q, intra frame and inter frame mode info,
intra modes with angle deltas, CfL, filter-intra symbols, the palette flags (not palettes), reference frames (single, unidirectional and
bidirectional compound, all their contexts), the reference-MV stack (row/column/point scans at 1, 3, 5 rows/columns, top-right and
top-left, weights, sorting, the extra search with sign bias, contexts, clamping), the inter modes and DRL, motion vector coding
(joints, classes, bits, fractions, high precision), the switchable interpolation filter, transform size (tx_depth, variable
transform trees with their split contexts), transform blocks in coding order (64x64 chunks, inter transform trees), transform types
(intra and inter sets), all-zero / DC-sign contexts and the above/left entropy-context updates, block-skip context resets.

Not modelled (a stream using them stops the run with a clear message): segmentation, CDEF syntax, loop restoration units, delta loop
filter, intra block copy, palettes in use, global motion (all references are identity in lean2), temporal motion vectors
(`use_ref_frame_mvs` is 0 in lean2), inter-intra, masked / distance-weighted compound, warped motion and OBMC (switchable motion
mode), dual filter. **Untested although written**: the rectangular and 4-way partition types (HORZ_A..VERT_4) and 128x128
superblocks (`lean2` has neither; `lean0`/full streams would exercise them but also need CDEF and restoration syntax first).

## Things the specification-level recollection got wrong, found by the trace (all fixed, none needed to read libaom's code except one)
* `delta_q_res` is a multiplier (1, 2, 4, 8), not a shift; transform-block context byte: libaom saturates the level at 7 and puts the
  DC category above it (the spec's 63 gives identical contexts, as only comparisons with 3 and zero are used).
* The inter transform-type alphabet for the 16-type set is ordered IDTX, V_DCT, H_DCT, V_ADST, H_ADST, V_FLIPADST, H_FLIPADST, then the
  nine two-dimensional types.
* The transform-split context reads the above/left transform size of **each column/row of the block**, not of the first neighbour.
* The extra search of the MV stack scans one extent for both passes, `min(16, bw4, bh4, frame columns left, frame rows left)`; this was
  the one place where libaom's source (`setup_ref_mv_list`, the extra-search tail) was read to find out the normative behaviour.
* libaom keeps `ref_mv_idx` of the near modes one lower than the specification's RefMvIdx (a representation difference, compared as such).

## Work counts, for sizing the RTL (nine lean2 streams, 90 s of video in total)
* Block-level symbols: ~168 k a second on average (coefficients not included). Share by element: reference frames 26%
  (single_ref alone 16%, the compound flags 10%), skip / skip mode / is_inter 18%, new/zero/ref-MV flags + DRL + compound mode 18%,
  motion vector components 16%, partition 11%, interpolation filter 3%, transform size and split 3%, transform type 2.5%, intra modes
  3%, delta q 0.1%. Binary decisions dominate; only partition (10-symbol), the MV class (11), the compound mode (8) and the intra
  modes (13/14) are wide.
* Reference-MV stack: 1.25 M searches (one per inter block), 9.5 M candidate evaluations (7.6 per search), 0.7 M extra searches (56% of
  the searches end with fewer than two candidates).
* Per-block state the RTL needs: above/left rows of mode info (block size, skip, skip mode, is_inter, reference pair, two motion
  vectors, mode, interpolation filter, transform width/height) and the entropy contexts of the three planes, stored per 4x4 column
  (800 px = 200 columns) and per row of the superblock; the MV stack of up to 8 entries with weights; no temporal state in lean2.

## RTL (V-072)
`src/fpga/core/tau_av1_blk.sv` implements this model as a serial unit and replays the same libaom traces: all nine lean2 streams pass in full (`make test-rtl-av1-blk`, `-mutation`, `-full`). Mode-info ring: 93-bit records, 64 rows x 256 columns (32 rows suffice with 64x64 superblocks only; half the memory). Open: attach the real symbol decoder / coefficient parser / CDF memory, area and timing numbers (probe), the 128x128 question (the profile could forbid it).

## Next
1. Decide the frame-level split: the frame header, tile and CDF-context handling (initial CDFs, averaging/saving, the symbol
   decoder's start per tile) and reference management are CPU-friendly and small; the block level is the hardware part.
2. RTL for the block syntax. The model is deliberately written as a request/response walker so that a micro-sequenced controller (one
   symbol request to `tau_av1_symdec` at a time, context logic in a few datapath units, MV-stack search as a small state machine) can be
   verified against the same trace: every request address and every derived value is available in the committed trace.
3. If rectangular partitions or 128 superblocks are wanted: model CDEF and restoration syntax, then check the `lean0` and full traces.
