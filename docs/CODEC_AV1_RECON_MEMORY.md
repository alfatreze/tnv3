# AV1-lean reconstruction memory map (V-087)

Where the reconstructed samples live, who reads and writes them, and what that costs. The on-chip part is implemented and verified
(`src/fpga/core/tau_av1_recon_buf.sv`, `make test-rtl-av1-rbuf`: against libaom's decode on 12 streams); the SDRAM part is the plan the
frame-store writer, the deblocking walker and the motion-compensation fetch will follow (numbers marked *estimate* are not measured).

## Three kinds of reconstructed samples

| What | Needed by | Where |
|---|---|---|
| the neighbours of an intra transform block (row above incl. above-right, column left incl. below-left, the corner) | the intra predictor | on chip, **pre-deblocking** |
| the finished frame before deblocking | the deblocking walker | SDRAM frame store (written by the superblock flush) |
| reference frames (after deblocking) | motion compensation | SDRAM frame store |

Intra prediction uses samples *before* the loop filter (verified: every neighbour sample libaom read equals the pre-deblocking frame), so
the filter can run later, over the whole frame, in place in the frame store; nothing intra needs to wait for it.

## On chip (`tau_av1_recon_buf`)

| Memory | Size | Content |
|---|---|---|
| `cur` | 24,576 B (128x128 luma + 2 x 64x64 chroma; a 64x64 superblock uses the low 6,144 B) = about 20 M10K | the superblock being reconstructed, row-major with the plane's superblock size as stride |
| `top` | 2 x MAXW + 256 B (4,352 B at MAXW = 2048) = 4 M10K | the bottom sample row of the superblock row above, whole frame width plus one superblock (above-right) |
| `left` | 3 x 128 B | the last column of the superblock to the left |
| `tl` | 3 B | the sample above-left of the superblock (the old `top` value the previous superblock's flush overwrote) |

Flow per superblock: `sb_start` (origin, size, frame area) -> for every intra transform block: `ft_start` streams `na` above and `nl` left samples
and the corner to the intra predictor's edge port (the counts come from `tau_av1_intra_avail`; nothing outside the decoded area is read) ->
the predicted + residual block (or an inter block) is written back sample by sample (`wr_*`, absolute plane coordinates) -> `fl_start` streams
the superblock out (only samples inside the frame), copies its bottom row to `top`, its last column to `left`, saves the corner.
A fetch reads one sample per clock (a 4x4 block: about 10 clocks, a 64x64 block: up to 256); a flush is S x S + 2 x (S/2)^2 clocks
(6,144 for 64x64: about 0.1 ms at 66 MHz *estimate*, about 10 ms per 800x450 frame if nothing overlaps; a ping-pong `cur` would hide it).

## SDRAM frame store (plan)

* A frame buffer = three planes, each `stride x height`, stride = frame width rounded up to 64 (luma) / 32 (chroma), height = mi rows x 4
  (/2), rows 64-byte aligned for burst writes. 800x450 -> luma 832 x 456 = 379 KB, chroma 416 x 228 = 95 KB each: **569 KB** per frame
  (1920x1080: luma 1920 x 1080 = 2.07 MB, chroma 0.52 MB each: **3.1 MB**).
* A pool of **9** buffers (8 reference slots + the frame being decoded; a frame that refreshes nothing is shown and dropped), the same
  rule as the entropy-context blocks (`fw/av1_hdr.h`); the lean streams use at most 3 references, so 5 would do for them. 9 x 569 KB = 5.1 MB
  for 800x450 (28 MB for 1080p) *estimate of space, no measurement*.
* Writer: the flush produces (plane, x, y, byte); row bursts of 64 or 128 bytes per superblock row go to `base + plane_off + y*stride + x`
  (the same burst-write DMA structure as `tau_mp4v_dma_wr`).
* Deblocking, in place after the frame: vertical pass then horizontal pass per plane (`tau_av1_lf_edge` decisions, `tau_av1_lf_line` kernel),
  fed by row windows read as bursts; needs the per-unit mode info (skip, intra, reference, mode, transform sizes) of the whole frame, which is
  NOT kept today (the mode-info ring holds 64 rows): a compact per-4x4 record (about 2 bytes: bsize/skip/intra/level inputs) written during
  decoding is the open item.
* Motion compensation reads reference blocks from the frame store with edge replication by clamped addresses (as `tau_mp4v_rowfetch`).
* Traffic per frame *estimate*: flush write 0.57 MB, deblocking read + write 1.1 MB, motion-compensation reads about 2-3x the frame for 8-tap
  compound prediction: roughly 4-5 MB per frame, about 100 MB/s at 24 fps; the SDRAM controller's burst rate has to be measured on the
  decoder clock before this is trusted.

## Choices that follow from this

* Superblock size drives the on-chip cost: 128x128 needs 24 KB for `cur`; 64x64 only 6 KB (the owner's decision on 64x64-only streams is
  still open). All code handles both (tested on streams with both).
* A tile boundary is free: the availability unit already forbids using samples across it; the `top` line keeps the last row per column, so
  tiles side by side in one superblock row work (tested with 2x2, 4x1 and 1x4 tile streams, `make test-av1-hdr`/`test-rtl-av1-rbuf` data).
* Frame width limit of the on-chip line buffer: MAXW (default 2048 luma samples), a parameter.
