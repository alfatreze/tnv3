# Clean-sheet build: drop AV1 and the unneeded hardware, design the codec from first principles (2026-10-27)

Worktree `codec-alternatives`. Companion to `docs/CODEC_ALTERNATIVES_2026-10-27.md` (the first pass, which kept AV1 as the main track and missed MPEG-2; this document supersedes its
recommendation at the owner's request: fully drop AV1 and the surplus hardware, start from the requirements). Evidence per line: **measured** (this session's sweep or earlier project
measurements), **estimate**, **recalled** (background knowledge, not checked this session).

## 1. The measurement that changes the question

New sweep this session (`tools/codec_study/clean_sheet/`, raw results next to it, table in `docs/codec_phase0/clean_sheet_table.txt`): three Tears of Steel clips (dialog, dark, action),
800x334, 48 frames each, 24 fps; ffmpeg 9 encoders plus `aomenc` (libaom cpu-used 4); decode, then VMAF 0.6.1 and PSNR-Y against the source; size interpolated log-linearly at equal quality.
Kilobytes per frame, geometric mean of the three clips, at **VMAF 90** (PSNR-Y at 38 dB gives the same ordering):

| codec / tool set | kB per frame | x libaom low-delay | kB/s at 24 fps | 2 h film |
|---|---|---|---|---|
| **Intra only** | | | | |
| libaom all-intra | 7.4 | | 178 | 1.3 GB |
| VP9 all-intra | 9.7 | | 233 | 1.7 GB |
| x264 all-intra (all intra tools) | 12.3 | | 295 | 2.1 GB |
| x264 intra, 16x16 prediction only | 14.6 | | 350 | 2.5 GB |
| MJPEG | 14.3 | | 343 | 2.5 GB |
| **MPEG-2 all-intra** | 14.8 | | 355 | 2.6 GB |
| JPEG 2000 (ffmpeg, 9/7 wavelet, no rate-distortion optimisation) | 15.8 | | 380 | 2.7 GB |
| **Inter, low delay (I then P only, GOP 96 incl. the first key frame)** | | | | |
| libaom (full tools) | 1.6 | 1.0 | 38 | 277 MB |
| x264 slow (all tools, 3 refs, mbtree) | 2.7 | 1.7 | 65 | 467 MB |
| x264 restricted: 16x16 blocks, 1 ref, 4x4 transform, **CABAC** | 3.6 | 2.2 | 86 | 622 MB |
| x264 restricted, **CAVLC** | 4.4 | 2.7 | 106 | 760 MB |
| **MPEG-2 (ffmpeg `mpeg2video`, I/P, rd + trellis)** | **4.3** | **2.7** | **103** | **744 MB** |
| MPEG-2 with 2 B-frames | 4.4 | 2.7 | | (ffmpeg's B-frames gained nothing here) |

Two things follow.

1. **MPEG-2 is exactly "16x16 blocks, one vector, VLC entropy coding"**: it lands on top of the restricted x264 CAVLC point (4.3 vs 4.4). It needs 2.7x the bits of lean AV1 and 1.6x x264 slow,
   which is the "not space efficient" the owner expected. Yet in absolute terms it is **103 kB/s at 800x334**, 12% of the 850 KB/s the SD path sustains (measured, V-011) and 744 MB for a film on a 128 GB card.
2. **Bits are cheap, hardware is not.** Every candidate above, even all-intra MJPEG at 343 kB/s (40% of the SD rate, 2.5 GB a film), is within what the card can feed. The old question "which codec
   compresses best" is the wrong objective for this device; the right one is *which codec decodes at 800x334 and 24 fps in the least silicon with the least risk*. (This is the opposite of the AV1 track's premise,
   which chased -70% bits and paid with a 7.8 k-ALM decoder that could not hold more than 400x166.)

Caveats: three short clips of one CG film; ffmpeg's MPEG-2, MJPEG and JPEG 2000 encoders are plain (a tuned encoder, which runs offline in Tau Omega, would be 10-25% better for each); the first key frame is
inside the 48 frames, so low-delay figures are pessimistic for a long file; the x264 `deblock` switch had no effect in my harness, so **deblocking is not measured** (the earlier study gave 2-19% depending on metric).

### Tool-by-tool value in a simple hybrid (x264 restricted, I/P, 16x16, 1 ref, 4x4 transform = 3.6 kB/frame at VMAF 90; PSNR in brackets)

| change | kB/frame | effect |
|---|---|---|
| CAVLC instead of CABAC | 4.4 | **+22%** (+13%) |
| 8x8 transform (adaptive 4x4/8x8) and intra 8x8 | 3.0 | **-17%** (-13%) |
| B-frames (3, pyramid) | 3.1 | **-14%** (-17%) |
| encoder-only look-ahead rate control (mbtree) | 3.3 | -8% (free for the decoder) |
| intra 4x4 | 3.5 | -3% |
| 3 reference frames | 3.5 | -3% |
| inter partitions 16x8, 8x16, 8x8 | 3.6 | 0% (-3%) |
| everything together (= x264 slow) | 2.7 | -25% |

The three tools that pay are **adaptive-context entropy coding, a larger transform, and bidirectional prediction**. Partitions, extra references and fine intra modes do not (in a long GOP the key frame is
about 1/96 of the frames: replacing MPEG-2 intra, 14.8 kB, by libaom intra, 7.4 kB, saves only 2% of the file).

## 2. Requirements derived from the device (not from a codec)

| Quantity | Value | Basis |
|---|---|---|
| Picture | 800x334 (2.4:1 film) at 24 fps; 1,050 macroblocks, 25 k macroblocks/s | arithmetic |
| Time per macroblock | 3,970 clocks at 100 MHz, 2,640 at 66 MHz | arithmetic |
| Stream rate to feed | 65-103 kB/s inter, 178-343 kB/s if all-intra; SD gives 850 KB/s | measured |
| Entropy work | about 35-45 k bins a mean frame, 0.2 M for a key frame: 2-9 ms at 100 MHz with a 4-clock bin | estimate |
| Transform work | at most 6,300 8x8 blocks a frame; 1 row/clock engine = 16 clocks a block = 1 ms | estimate |
| SDRAM traffic | about 2.5 MB a frame (write, reference read, output read, RGB write) = 60 MB/s of a ~200 MB/s part | estimate |
| What is NOT a budget | bitrate, storage, seek (key frame every 2-4 s is enough) | derived |

With a macroblock-serial hardware pipeline the decode time is 10-20% of a frame period for **any** of the simple hybrids. That removes the AV1 pressure (timing closure at 100 MHz, a 41.7 ms frame that
was already tight at 400 wide). Nothing here needs more than one lane of anything.

## 3. Inventory: what to drop, what to keep (from the file list and the fitted numbers of V-121; module roles to confirm in the first synthesis)

| Block | ALMs (fitted, nu-av1n) | Verdict |
|---|---|---|
| all `tau_av1_*` (decode subsystem) | about 7,800 + 70 M10K | **drop** |
| all `tau_mp4v_*` (MPEG-4 decoder: idct, mc, recon, dec, dma, out, regs, rowfetch, sdram_rd, yuv2rgb) | (earlier build 10.3 k total) | **drop the module set, reuse ideas**; the IDCT/MC/colour/DMA pieces are the verified starting point for a new MPEG-2-class core, redone for 1 row/clock (owner rule: optimal RTL over reuse) |
| `mp3_soc` (3,103) + VexRiscv (1,530) + 192 KB main RAM (about 190 M10K) | 4,600 + large RAM | **replace** by a minimal control core (RV32I serial or small, 4-16 KB RAM) |
| `mp3_fb` (1,426): display framebuffer, OSD, visualisers | 1,426 | **keep the scan-out and the SDRAM framebuffer path only**; drop the rest |
| spectrum bank, wave meter, Halcyon, Cymo resampler, EQ, gain stage, FLAC LPC, `mp3_poly` | several k | **drop** unless the audio choice (section 5) wants FLAC LPC or the MP3 window |
| `pcm_fifo`, `sound_i2s`, `sync_fifo` | small | keep |
| `data_loader`, `core_bridge_cmd`, `tgt_cmd`, PLL, constraints, `core_top` | small | keep (APF contract) |
| SDRAM controller and arbiter (`tau_sdram_*`) | small | keep (arbiter with the new decoder as a master) |
| PSRAM interface (`tau_psram_*`) | small | drop if code and tables live in SDRAM / on-chip RAM |
| CDC helpers, `font_rom`, `tau_clut_wr`, signaltap tap | small | keep what the new top needs |

Estimated end state (estimate): decoder 3.5-4.5 k ALMs + base 2-2.5 k + audio 0.3-0.8 k = **about 6-8 k ALMs (35-45%), 60-90 M10K**, against 16 k ALMs (87%) and 277 M10K today. The point is not the saving
itself but the consequence: timing closes at 66-100 MHz without three rounds of registering, power and fit time drop, and 800 wide fits.

## 4. The codec: "TNV3" proposal (clean sheet)

Skeleton: **MPEG-2-class**. It is the smallest design that sits on the measured curve, its tools are decades old (patents expired, recalled; ITU-T H.262, the same text as ISO 13818-2, is a free
download, recalled), and its syntax is regular enough for a table-driven hardware parser. Then add exactly the three tools the ablation says pay, each chosen to be cheap in silicon and free of H.264's patent pool:

| Layer | Choice | Why / cost | Expected gain (measured proxy) |
|---|---|---|---|
| Structure | I and P pictures, 16x16 macroblocks, one vector, GOP 2-4 s; B-pictures (bi-directional average, no direct modes) as a second step | simplest reference handling; 2-3 SDRAM buffers | B: -14% |
| Prediction | skip/zero-vector, half-pel bilinear (already verified in the project), intra DC + horizontal/vertical/plane per 16x16 and 8x8 | MC is a 17-sample-wide interpolator; no deblocking assumed | intra 4x4 would add -3%: skip |
| Transform | exact **integer 8x8 DCT** (no IEEE-1180 mismatch, bit-exact against a C model), 4x4 as a second size on a flag | one engine, row/column, 16 clocks a block | 8x8/4x4 adaptive: **-17%** |
| Entropy | **own adaptive binary range coder** (LZMA-style 12-bit probabilities, shift update, no table of CDFs), contexts per syntax element and per coefficient position/neighbourhood; syntax in a handful of binarisations (Exp-Golomb, unary) | about 150 ALMs for the coder and 1-2 k bits of context RAM; license-clean (not CABAC) | **-18..-22% vs VLC** (CABAC proxy; ours is simpler, so expect part of it) |
| Quantiser | uniform with dead-zone, per-picture and per-macroblock delta; optional frequency-weight matrix | encoder-side RDOQ is free | |
| In-loop filter | none in the first version; an H.263-Annex-J-style deblock as an optional later block | unmeasured (see caveat) | |
| Container | TNV with an index (key flags), as TNV2 | unchanged | |

Predicted size (estimate, from the ablation, not a prototype): 4.4 (MPEG-2-like) x 0.82 (adaptive binary coder) x 0.83 (adaptive transform) x 0.86 (B-pictures) x 0.92 (encoder RDO) = **about 2.4 kB/frame,
and 2.7 if the gains overlap as in the x264 "all tools" measurement**, i.e. 1.5-1.7x the bits of libaom and 55-65% of plain MPEG-2: 58-65 kB/s, a 2 h film of about 450 MB. An H.264-Main-sized stream
with an MPEG-2-sized decoder, if it holds.

### Why not the alternatives (after this measurement)
| Candidate | Verdict |
|---|---|
| AV1 | dropped by the owner; the data supports it for *this* device: its 40% advantage over the proposal buys storage we do not need and costs 2x the silicon |
| MPEG-2 as shipped | viable as the **fallback / stepping stone**: ships first, no new entropy coder, but 0 new ideas; keep its VLC tables out unless needed (ISO tables, same LGPL-isolation approach as V-020) |
| MJPEG / intra-only | the *simplest* possible decoder (343 kB/s, 40% of the SD rate, 2.5 GB a film; seek is trivial; no reference memory). A real contender for a "version zero" and for 800x334+ at higher frame rates; costs 3.3x the bits of the proposal |
| Wavelet (JPEG 2000, VC-2, custom) | 15.8 kB/frame in ffmpeg's plain encoder, no better than MJPEG; no temporal tool; the entropy coder (bit-plane) is the expensive part. Not pursued |
| H.264 / HEVC | the proposal reproduces their useful tools; their patent pools and syntax mass are the reason not to use the standards themselves |
| VP9 | intra 9.7 kB/frame vs 7.4 for AV1: no benefit once AV1 is dropped |

## 5. Audio (shared decision, hardware-first)

The CPU no longer decodes video, so the audio decoder decides whether a CPU is needed at all. Candidates by silicon and CPU cost (sizes at 48 kHz stereo; all estimates unless noted):

| Codec | Rate | Decoder | Notes |
|---|---|---|---|
| **FLAC (lossless)** | about 80-110 kB/s (PCM is 192 kB/s; FLAC typically 40-55%, recalled; the first draft said 85 kB/s "55%", which was inconsistent) | Rice decode + fixed/LPC predictor, a few hundred ALMs; `tau_flac_lpc` existed in Tau Alpha | no psychoacoustics, no tables, perfect quality; 550-760 MB for 2 h, i.e. **as large as the video or larger (50-60% of the file)**; fits the SD budget (about 190 kB/s total) but the file-size worry is justified: see section 5b |
| IMA ADPCM | 48 kB/s (4 bit) | trivial, tens of ALMs | audible noise floor; "good enough" for dialogue |
| MP3 mono/stereo | 8-16 kB/s | CPU 34% at stereo today (V-068) or the window unit (1.3 k ALMs) | patent-free now; heavy |
| Opus 64 kbit/s | 8 kB/s | a CPU-class decoder | the owner heard no difference vs MP3 128k (2026-10-21), but it is the most complex |

Recommendation: a **lossless-first audio path** (FLAC) so that the whole player is hardware; keep ADPCM as a cheap fallback. This is a decision for the owner (it multiplies the audio bytes by 6-10x against MP3).

## 5b. Audio revisited: file size (owner concern, 2026-10-27)

Sizes for a 2 h stereo 48 kHz film (rates of the lossy codecs from the nominal bitrates; FLAC range recalled):

| Option | kB/s | 2 h | Share of file with 65-103 kB/s video | Decoder |
|---|---|---|---|---|
| FLAC | 80-110 | 550-760 MB | 45-62% | few hundred ALMs |
| FLAC of lossyWAV-style pre-processed audio (encoder-only; decoder unchanged) | about 45-60 (recalled, unverified) | 320-430 MB | 30-45% | unchanged |
| IMA ADPCM | 48 | 345 MB | 32-42% | tens of ALMs |
| MP3 128 kbit/s | 16 | 115 MB | 10-20% | CPU + 1.3 k-ALM window unit |
| Opus / custom MDCT 64-96 kbit/s | 8-12 | 58-86 MB | 6-12% | see below |

**Custom MDCT codec ("TNA", joint with TNV3 only in container, entropy coder and hardware front end):** long MDCT (1024), short blocks for transients, scalefactor bands, uniform quantiser,
the same adaptive binary range coder with per-band contexts. Decoder = entropy decode, dequantise, IMDCT (FFT-based), window overlap-add: estimate 1-1.5 k ALMs, 4-6 M10K, a few DSP, no CPU
(the same order as today's MP3 window unit, which still needs the CPU for Huffman). The hard part is the encoder (bit allocation / masking model), offline in Tau Omega; the owner's listening result
(Opus 64k vs MP3 128k, not blind) says 64-96 kbit/s is the right target. Joint coding of audio and video gives no compression gain (unrelated signals); the gain of a combined design is shared hardware and frame-locked packets.
Proposed order: FLAC (or ADPCM) for the first-light skeleton because it needs no encoder research, then the custom MDCT as a host reference codec (experiment X6b) judged by listening.

## 6. What would make this wrong (kill criteria and the experiments)

| # | Experiment (host first) | Done when | Kill / proceed |
|---|---|---|---|
| X1 | **Reference codec in C/Python** (`tools/tnv3_ref/`): I/P, 16x16, integer DCT 8x8/4x4, deadzone quantiser, half-pel ME (diamond), pluggable entropy back ends: (a) MPEG-2 VLC bit counts, (b) Exp-Golomb, (c) adaptive binary range coder with contexts | size and PSNR on the same 9 clips (3 Tears, 3 anime, 3 B&W) vs ffmpeg MPEG-2 and x264 curves | back end (c) not at least 15% below (a): drop the coder and ship MPEG-2-VLC-class |
| X2 | Transform study inside X1: 8x8 only vs 4x4/8x8 vs 16x16 | numbers | 4x4/8x8 not worth 10% of size: keep 8x8 |
| X3 | Deblocking filter value on 800x334 at the Pocket's pixel density: add a simple loop filter to X1 and score by metric and by eye | numbers + montage | under 5% and invisible: omit forever |
| X4 | B-pictures: X1 plus a bi-predictive mode with reorder | size vs P-only on all clips | under 8%: omit |
| X5 | Area probes (`tools/vm_probe.py`, 11 min each): binary range decoder + context RAM + syntax FSM; 1 row/clock integer IDCT; half-pel MC; skeleton base (bridge + SDRAM + scan-out + tiny CPU) | ALMs, M10K, Fmax per block | decoder over 5 k ALMs or base over 3 k: re-plan |
| X6 | Audio: FLAC LPC decode cost at 800x334 playback (DMA from SD, hardware) vs ADPCM; blind listening | owner decision | |
| X7 | Intra-only version zero: JPEG-class baseline decoder on the new skeleton, 800x334, to validate the base (SD streaming, display, A/V sync) before the inter path exists | plays a 2 min file | the skeleton is the real risk; build it first |

Order: X1 and X7 in parallel (host codec and hardware skeleton), X5 once X1 fixes the syntax.

## 7. Owner decisions needed
1. **Clean-sheet scope:** new top level (new worktree/branch from `codec-phase0`, AV1/MPEG-4 RTL archived on its own branch and deleted from the new build), or keep a legacy target alongside? (Recommended: archive, delete from the new tree, keep Tau Omega tools.)
2. **Licence stance:** use only tools whose patents have expired (MPEG-2-era) plus our own entropy coder, no CABAC/H.264 mechanics; ISO MPEG-2 tables, if used, isolated under `third_party/` like the FFmpeg tables?
3. **Target:** 800x334 at 24 fps first (the arithmetic says it is easy); is 1:1 pixel doubling to the Pocket screen as today?
4. **Audio:** FLAC-first (hardware-only player) or keep MP3/CPU?
5. **Version zero:** is an intra-only first light (X7) acceptable as the milestone before inter coding?
6. **Approve X1** (a host reference codec, a few sessions) as the next piece of work.
