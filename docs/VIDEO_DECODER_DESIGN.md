# Tau ν video decoder: design (2026-10-11)

Status: design only, nothing built. **Decision 2026-10-11 (owner): go straight to MPEG-4 Part 2 Simple Profile; no separate MJPEG stage. Audio is transcoded to 48 kHz (no resampling).** Figures marked [EST] are estimates, not measurements; everything else is measured or read from the repository. Companion: `docs/VIDEO_PROJECT_START.md` (scope, keep/drop lists).

## 1. Goal and non-goals
- Play a pre-converted video file with synchronised audio from the SD card on the Pocket, at the core's 400x360 output, letterboxed (2.4:1 film is 400x168).
- Target: 24 fps, no dropped frames, audio never starved, the existing UI (OSD, library, settings) usable while playing.
- Not a goal: playing arbitrary files. The Pocket does not decode what the user drops on the card; Tau Omega (later) transcodes to our container. No H.264/H.265, no B-frames, no quarter-pel, no interlace.

## 2. Measured inputs
| Fact | Value | Source |
|---|---|---|
| Source test film | Tears of Steel, 1920x800, 24 fps, 12:10, AAC 44.1 kHz | `media/tears-of-steel.mp4` |
| MJPEG at 400x168, quality 70, 4:2:0 | mean 11.6 KB, p95 18.7 KB, max 20.9 KB per frame; 0.28 MB/s mean; worst second 0.49 MB | `tools/video_convert.py`, full film |
| 90 s clip | mean 10.1 KB, max 14.8 KB, worst second 0.35 MB | same |
| MPEG-4 Simple Profile (I/P, qscale 6, 400x168, I frame every 48) | 90 s clip: mean 1,511 B/frame (6.7x smaller than MJPEG), max 12.2 KB (an I frame), 290 kbit/s, worst second 0.07 MB; average PSNR 35.7 dB (Y 34.5, U 40.0, V 40.4); ffprobe reports Simple Profile | `tools/video_convert.py --codec mpeg4` |
| Pruned bitstream (prune B: no spectrum bank, scope block, FLAC LPC, Cymo resampler, I2S diagnostic, Halcyon EQ, wide EQ input) | Quartus fit, both seeds, all corners positive (worst setup +1.396 ns, hold +0.110 ns): 8,107 ALMs (44%), 239/308 M10K, 14/66 DSP, i.e. 10.4 k ALMs, 69 M10K, 52 DSP free. **On a Pocket (`TAU DEV 203`, 2026-10-12): boots, plays MP3, menus fine, MP3 window unit intact (142,884 slots, 0 bad, 0 timeouts), VBLANK 60/s, SDRAM window OK, SD speed 850 KB/s (854 before the prune)** | V-016, V-021 |
| CPU already busy playing MP3 | about 44% | Tau Alpha B-593 |
| Uncached SDRAM access from the CPU | about 48-50 cycles; framebuffer writes by the CPU are not viable | Tau Alpha A-094, B-192 |
| SD/bridge sustained read rate | **854 KB/s sustained, slowest 4 KB read 13 ms** (Pocket, `TAU DEV 202`, Diagnostics > Tests > SD SPEED, 4 KB chunks, 2026-10-12; whether the track was playing during the run is not recorded) | V-001, V-011 |

The one number that gates everything is the SD rate. MPEG-4 needs about 0.04 MB/s average and 0.07 MB/s in the worst second plus 16 KB/s audio (MJPEG would need 0.3-0.5 MB/s), so even a poor SD rate is enough; the probe now confirms headroom rather than gating the design. Stage 1 below is therefore no longer a separate product: its blocks are built as the shared front of the MPEG-4 decoder.

## 3. Architecture
```
SD card (data slot) -> bridge read -> staging ring (PSRAM) -> decoder -> YUV frame store (SDRAM) -> colour convert -> back buffer (SDRAM) -> Talos / scanout
                                          ^ CPU: file feeding, headers, (MPEG-4: macroblock parsing), A/V sync, OSD
audio: MP3 stream -> existing MP3 path (window unit) -> pcm_fifo -> I2S;  video clock slaved to the audio sample counter
```
- **CPU does:** reading the file in chunks, the container index, frame scheduling, A/V sync, the OSD (Helios), and for MPEG-4 the bitstream parsing.
- **Hardware does:** everything that touches pixels. The CPU never writes a pixel (see section 2).
- **Frame pacing:** the display flips on vblank (existing H2 double buffer); a frame is decoded into the back buffer while the front one is shown. A frame that is late is repeated, never half-shown.
- **A/V sync:** the audio sample counter (already read for the meters) is the master clock. Video frame n is due at sample n * 48000/24; drift beyond one frame drops or repeats a frame.

## 4. Shared blocks (built first; MJPEG is not a product, only an optional later import path)
Scope: baseline JPEG, 4:2:0, no progressive, restart markers optional (our converter writes none), fixed Huffman tables allowed (the encoder's optimised tables are read from each frame, 4 DC + 4 AC tables parsed at frame start).

| Block | Function | Notes | Size [EST] |
|---|---|---|---|
| Bit reader + Huffman decoder | variable-length codes to (run, level) pairs | table-driven, 1 symbol per clock after the first lookup; Huffman tables in one M10K | 0.8-1.2k ALMs, 1-2 M10K |
| Dequantise + zigzag | multiply by the quantisation table, reorder | two quant tables in MLAB; one DSP time-multiplexed | 0.3k ALMs, 1 DSP |
| 8x8 IDCT | row-column, fixed point, accuracy per the JPEG/IEEE 1180 limits | **shared with MPEG-4**; transpose buffer 64x16 b in one M10K; 2-4 DSP | 1.5-2.5k ALMs |
| Colour convert | YCbCr 4:2:0 to RGB565, chroma replicated | 3 constants, 3 DSP or shift-add; writes the back buffer directly | 0.3-0.5k ALMs |
| Frame DMA | writes finished 8x8 / 16x16 blocks into SDRAM | burst writes through the existing arbiter; Talos-style row bursts | 0.3k ALMs |
Total [EST] 3-5k ALMs, 4-6 DSP, 3-5 M10K. Throughput target: one pixel per clock, so 400x168 = 67k pixels = about 1 ms per frame at 66 MHz; the 24 fps budget is 41 ms, so the decoder is idle more than 95% of the time. That headroom is the point: the design can be small and slow, one unit time-shared, not parallel.

MMIO: reuse the sticky index+data convention (`R_LPC_*`, `R_CLUT_*`). Registers [EST]: control (start, abort), source address, frame size, destination base and stride, table load (index + data), status (busy, done, error code), frame counter. One interrupt-free polling interface like the other units.

## 5. The decoder: MPEG-4 Part 2, Simple Profile (I and P only)
Why: measured 6.7x smaller than MJPEG at 400x168 (about 3 MB per minute instead of 17 MB for the same film, section 2). Encode with ffmpeg's `mpeg4` encoder restricted: no B-frames, no quarter-pel, no global motion, H.263 quantiser, fixed GOP with an I frame every 2 s.

| Stage | Where | Notes |
|---|---|---|
| VOL/VOP headers, macroblock layer, VLC decode (MCBPC, CBPY, MVD, DCT coefficients), intra AC/DC prediction, motion-vector prediction | CPU firmware | written from ISO/IEC 14496-2; cold code; about 6.5k macroblocks/s |
| Dequantise (H.263 type) | CPU or the shared dequant unit | decide after profiling |
| IDCT | hardware, shared with stage 1 | |
| Motion compensation | **new hardware unit** | 16x16 and 8x8 blocks, half-pel bilinear with the rounding-control bit, edge replication for vectors pointing outside the picture (unrestricted MVs), reads the reference frame from SDRAM, writes prediction + residual, clipped |
| Frame stores | SDRAM | reference and current YUV 4:2:0, 400x168 = 100.8 KB each |
| Colour convert + display | shared with stage 1 | |
Size [EST]: motion compensation 1-2k ALMs, no new M10K beyond a 17x17 block line buffer. CPU: parsing about 5-10% plus macroblock bookkeeping about 5% [EST]; with audio 44% the total is about 60-70%, to be measured with the headroom counters that already exist.

Cost to avoid: B-frames need a third reference and reordering; quarter-pel needs a 6-tap-style filter and doubles the fetch bandwidth; neither buys enough at this resolution.

## 6. Memory map [EST, to be fixed at the first RTL step]
| Region | Size | Where |
|---|---|---|
| Frame buffers (display, double) | existing H2 layout | SDRAM |
| YUV reference + current (stage 2) | 2 x 100.8 KB | SDRAM, above the display buffers |
| Input staging ring | 256 KB (about 0.5 s peak, 8 s mean) | PSRAM |
| Huffman/quant/IDCT working memory | 3-5 M10K | on chip |
| Firmware parser state, MB row | 2-4 KB | hot RAM (heap is scarce: cold code and PSRAM state first, see the RAM diet notes) |

## 7. Container (TNV1, test format, version 1)
Header 64 B (`TNV1`, version, size, fps, codec four-cc `MJPG`, frame count, audio offset/length/four-cc, index offset), then an index of (offset, size) per frame, then frames, then one MP3 stream. Defined and implemented in `tools/video_convert.py`. Stage 2 adds a codec four-cc `MP4V` and a keyframe flag (high bit of the size field), plus a thumbnail pointer and a chapter table when the library needs them. Not frozen; Tau Omega gets its own copy of the spec when transcoding is added (`docs/CROSS_PROJECT_INTERFACE.md`).

## 8. Verification (same discipline as the other units)
1. Host model in C of each block, written from the standards, never from other decoders' code (D-P01). For JPEG, compare against libjpeg-style reference output with a tolerance on the IDCT (+-1) and exact Huffman/bit behaviour; for MPEG-4, compare against ffmpeg's decoded frames of the same stream (tolerance on the IDCT, exact on motion compensation with the same rounding rule).
2. Independent integer twin for the hardware datapath (as `halcyon_engine_model.py`), bit-exact test with mutants (each stage's wrong rounding, wrong edge clamp, wrong pairing of Y/Cb/Cr).
3. RTL testbench replays the host vectors; mutation targets in `make test-rtl`.
4. Synthesis-only check, then a two-seed fit; all corners positive before any card.
5. Real-silicon gates: SD SPEED, then a still frame, then 10 s of video without audio, then with audio, then the full film; counters for late frames, underruns, worst decode time.

## 9. Build order (MPEG-4 direct)
0. SD SPEED on a Pocket (V-001) and the prune-B fit (V-003, running, ETA 01:30-01:35).
1. Review this document; fix the MMIO map and memory map.
2. **DONE (V-012)** Host reference decoder in C for MPEG-4 SP (headers, macroblock layer, VLC tables, AC/DC and MV prediction, half-pel motion compensation, IDCT) written from the standard, compared with ffmpeg's decode of `tears_90s_mp4v.tnv` (IDCT tolerance +-1, exact elsewhere). The CPU parser is this model's firmware twin.
3. RTL: IDCT, colour convert, frame DMA (shared blocks) with an I-frame-only test; first picture on the Pocket (stills only).
4. RTL: motion-compensation unit with golden model, mutants, synthesis check, two-seed fit; then P-frame playback of 10 s without audio.
5. Paced playback with 48 kHz MP3 audio and A/V sync, then the OSD, library entries and resume position, then the full film.

## 10. Risks and open decisions
| Risk or decision | Notes |
|---|---|
| SD rate | measured 854 KB/s: 3.5x the worst second of the whole MPEG-4 film (0.24 MB/s) and 14x its mean (0.06 MB/s); MJPEG's worst second (0.49 MB/s) would also fit but with only 1.7x margin. 4 KB reads cost about 4.8 ms each on average, so the rate is probably per-command latency, not raw bandwidth: larger reads (16-64 KB) should be faster (unmeasured). A 13 ms worst stall is about a third of a frame, covered by a small staging ring |
| ALM budget | prune B frees about 35% of registers [EST for ALMs]; the real fit decides; if short, drop Halcyon or the Cymo resampler (already out in prune B) |
| Audio rate | decided: the converter writes 48 kHz MP3, so no resampler is needed (Cymo is out in prune B) |
| Timing closure | the IDCT and the blend/blit paths have been the marginal ones before; pipeline the IDCT rows and register every DSP output, retiming per the Tau Alpha lessons |
| Licensing | MPEG-4 Part 2 and MPEG-1 patents believed largely expired, unverified; JPEG is free |
| Product name and card | working name Tau ν; the Pocket renders the glyph unconfirmed (fallback "Tau Nu") |
| Test content | Tears of Steel is CC BY 3.0 (Blender Foundation); keep the attribution with any file made from it |
