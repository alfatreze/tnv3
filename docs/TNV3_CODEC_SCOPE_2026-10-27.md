# TNV3: a fully custom audio / video / subtitle codec family for the Analogue Pocket (scope, research and development plan, 2026-10-27)

Worktree `codec-alternatives`. Builds on `docs/CODEC_ALTERNATIVES_2026-10-27.md` and `docs/CODEC_CLEAN_SHEET_2026-10-27.md` (same day) and on the project's AV1 / MPEG-4 history. Owner brief: nothing
reused from an existing codec (existing codecs are read for learning only); good quality per byte; **scales from a small, less efficient core to a large, efficient core that does 800x720** (half the
1600x1440 panel) **with subtitles and enough room left to build a player**. Evidence tags: **measured** (this project, with run id), **source** (Pocket skill KB / Analogue docs), **literature**
(read this session, link in section 3, abstract-level only), **estimate**, **recalled** (my background knowledge, not checked).

## 0. Summary of the proposal

* **Video (TVC):** a deliberately conventional *shape* (16x16 macroblocks, I/P/B pictures, one or four vectors, block transform) built from tools chosen by measurement, with three non-conventional pieces:
  an **invertible time-domain lapped 8x8 transform** (no deblocking filter, -13% bits vs a plain DCT in this session's study), a **two-rate adaptive binary range coder** designed for a 2-clocks-per-bin hardware decoder,
  and a **slice-parallel bitstream** so a larger core simply instantiates more lanes of the same RTL.
* **Audio (TAC):** an MDCT transform codec (1024-long, 128-short, M/S, scale-factor bands, noise filling) sharing the video's range coder; **byte-separable layers** so a small core decodes the base layer only.
* **Subtitles (TST):** text events plus a per-file baked glyph table (any language/font, rendered offline), blitted by the scan-out overlay; optional RLE bitmap events for image subtitles. Placed in the letterbox bars where the picture has them.
* **Container (TNV3):** frame-locked A/V/S packets, per-packet CRC, key/layer flags in the index.
* **Scalability** is in the bitstream, not only in the silicon: three profiles (S / M / L) with *nested tool sets*, droppable temporal layers (a small core plays the same file at half rate), independently decodable slices,
  byte-separable audio layers.
* **Honest status:** a plan with two small host experiments behind it. The decisive measurements (does the whole design reach the size target, does the lapped transform survive inter coding, patent clearance) are NOT done: section 10.

## 1. Requirements (R) and what each implies

| ID | Requirement | Consequence |
|---|---|---|
| R1 | Top profile: 800x720 luma output window (Pocket scaler 2x to 1600x1440), 24 fps (30 fps stretch), content letterboxed inside (16:9 = 800x450, 2.4:1 = 800x334) | 2,250 macroblocks/frame at full window, 1,050 for 2.4:1; 54 k macroblocks/s worst case |
| R2 | Good quality per byte | target in section 9: at least 25% below MPEG-2 (measured 4.3 kB/frame at VMAF 90, 800x334) |
| R3 | Scales down to a small/slower core and up to a big one | profile ladder S/M/L, nested tools, temporal layers, parallel slices (section 6) |
| R4 | Subtitles at high pixel density | text with offline-baked glyphs, scaled to the panel, in letterbox when available |
| R5 | "Just enough leeway to build a player" | reserve (L profile) at least 30% of ALMs, 25% of M10K, 35% of SDRAM bandwidth, 40% of the decode time budget for UI, library, resume, OSD, thumbnails |
| R6 | Nothing copied from an existing codec | own syntax, tables, transform parameters, entropy coder; ideas from the literature are re-derived and measured; a separate freedom-to-operate review is the owner's call (section 10) |
| R7 | Verifiable | every block has a bit-exact C model, mutants, and a host reference codec (the project's existing method) |

## 2. Platform facts that bound the design

| Fact | Evidence |
|---|---|
| Core video: RGB888, 16x16 to 800x720, 47 to ~61 Hz, pixel clock up to ~50 MHz; scaler 2x to the panel; VS/HS/DE rules; zeros outside DE | source: skill `hardware-video-audio-input.md`, KB-014 |
| Audio: I2S 16-bit stereo, exactly 48 kHz, MCLK 12.288 MHz, no sample-rate adjustment | source |
| RAM: SDRAM 64 MB 32Mx16 (chip rated to 166 MHz; this project runs 100 MHz, 46-clock access measured), PSRAM 2x16 MB, SRAM 256 KB async, M10K 308, ALM 18,480, DSP 66 (one 27x27 or two 18x19 each) | source: skill, KB-147, KB-165; measured V-021 (SDRAM) |
| The SD file reaches the core through the APF bridge: a data loader pushes a byte every 8 bridge clocks and cannot be told to wait (needs a FIFO); sustained SD read measured 850 KB/s in this project | community-reported KB-028; measured V-011 |
| Bridge reads are asynchronous-ish; multi-bit CDC needs Gray/handshake, not per-bit synchronisers; vsync pulse is too short for a softcore to poll (count frames in hardware) | KB-008, KB-009, KB-060 |
| Fit results vary ~1.2 ns by seed (re-seed, read fast-corner hold); Quartus Lite or Standard, never Pro | KB-011, KB-049 |
| Frame-alternating output causes retention on the OLED; screenshots are lossless RGB565 (a free capture channel for tests) | KB-075, KB-103 |
| M10K / MLAB differ in port types and read-during-write; MLAB is simple dual port only | KB-073, KB-143, KB-144 |
| Panel density 615 ppi at 1600x1440: 800x720 is a 2x pixel-doubled picture, so artefacts are visible at the 800 grid | derived |

Derived budgets for the top profile (estimates): 800x720 4:2:0 = 864 kB a frame. Scan-out read straight from the YUV planes with YUV-to-RGB at the pixel clock (no RGB frame buffer) at 48 Hz = 41 MB/s;
decode write 21 MB/s; reference reads about 30 MB/s; total near 100 MB/s against about 200 MB/s peak at 100 MHz (332 MB/s at 166 MHz), i.e. **SDRAM bandwidth, not logic, is the limit of Profile L**
and an RGB565 intermediate buffer (the old path) would add 49 MB/s: do not keep it.

## 3. Literature read for this design (abstract level; I did not read the papers in full)

| Topic | What it says | Use here |
|---|---|---|
| Lapped transforms with time-domain pre/post filters ([TDLT page](https://www.sfu.ca/~jiel/TDLT.html), [IETF draft Egge/Terriberry](https://datatracker.ietf.org/doc/html/draft-egge-netvc-tdlt), [APSIPA 2014 integer TDLT](https://www.cs.tsukuba.ac.jp/~taizo/pdf/conference/201412_APSIPA_ASC.pdf)) | the filter is invertible, needs no conditional logic, reduces blocking and sits around an ordinary block DCT; integer versions use adders and shifts | the transform choice (section 4.1), supported by my own measurement |
| Daala ([arXiv 1608.01947](https://arxiv.org/pdf/1608.01947)) | lapped transforms replace deblocking but make *spatial* intra prediction hard, so it predicted in the frequency domain; perceptual vector quantisation was complex to integrate | **do not use spatial intra prediction**; frequency-domain DC/AC prediction only. Skip PVQ |
| JPEG XS ([UCLouvain](https://research.dial.uclouvain.be/handle/2078.5/224751), [entropy/profiles](https://research.dial.uclouvain.be/handle/2078.5/224474)) | designed for implementations of at most a quarter of a Cyclone V; rate-distortion was deliberately secondary to cheap, parallel hardware; profiles and levels as the scaling mechanism | profile/level ladder; slice-parallel design as a principle |
| Interleaved and FPGA rANS ([arXiv 1402.3392](https://arxiv.org/pdf/1402.3392), [FPGA cores arXiv 2404.10896](https://arxiv.org/pdf/2404.10896)) | static rANS decodes about one symbol per clock; interleaving buys throughput | rANS suits static tables; **our contexts adapt per bin**, so a binary range coder is the better fit; rANS stays an option for the audio coefficient escape tables |
| Adaptive binary models, multi-rate estimators ([CABAC overview](https://en.wikipedia.org/wiki/Context-adaptive_binary_arithmetic_coding), [VVC multi-estimator work](https://research.mpu.edu.mo/en/publications/double-bit-range-estimation-with-eight-estimators-for-cabac-in-vv-2/)) | state-table CABAC is patent-laden and mid-complexity; averaging a fast and a slow estimator gains several percent | own 16-bit two-rate counters; no state tables (patent distance) |
| CELT/Opus ([arXiv 1602.04845](https://arxiv.org/pdf/1602.04845)) | fixed-point CELT needs ~104 MIPS encode+decode on a 2010-class DSP; PVQ-based; low delay | too heavy for a 66 MHz CPU and not needed (delay is irrelevant for playback): **TAC is a plain long-window MDCT codec** |
| Noise filling and bandwidth extension ([AudioLabs tutorial](https://audiolabs-erlangen.de/content/resources/aesCodingTutorial/bwe.html)) | zero-quantised lines replaced by noise at a per-band RMS is cheap in bits; BWE copies LF content up; both add little computation | noise filling in base TAC; BWE as a Profile-L option |
| Blu-ray PGS bitmaps ([FFmpeg PGS decoder doc](https://ffmpeg.org/doxygen/6.1/pgssubdec_8c_source.html)) | RLE bitmap objects, palette of up to 256, windows and display sets | only as a *source* format for Tau Omega; our bitmap events are simpler (section 4.3) |

Not found: independent FPGA numbers for adaptive binary range decoders, a published PSNR/VMAF comparison of lapped vs deblocked DCT in an inter codec at this rate, and the cost of lapped transforms on residual pictures.
These are the gaps the experiments in section 8 close.

## 4. Codec design

### 4.1 Video (TVC)
* **Format:** 8-bit YCbCr 4:2:0, BT.709/601 flag. Output stage converts YUV to RGB888 at the pixel clock directly from the frame planes (no RGB buffer).
* **Structure:** I, P and B pictures; 16x16 macroblocks; pictures split into **independent slices** (horizontal stripes of macroblock rows, each with its own range-coder state and context reset). Temporal layers: B pictures
  are non-reference in profiles S/M (droppable), optionally reference in L.
* **Transform:** 8x8 integer DCT-like transform with an **invertible time-domain lapped pre/post filter on the 8x8 block edges** (4 samples each side). Measured this session
  (`tools/codec_study/clean_sheet/transform_study.py`, luma only, flat quantiser, one estimator for all, **no spatial intra prediction**, DC predicted by MED; `docs/codec_phase0/clean_sheet_transform_study.txt`), size at equal PSNR-Y relative to plain 8x8 DCT:

  | PSNR-Y | dct16 | dct8 + lapped | wavelet 9/7 (3 / 4 / 5 levels) |
  |---|---|---|---|
  | 36 dB | 0.95 | (n/a, one image outside range) | 1.02 / 0.91 / 0.85 |
  | 38 dB | 0.97 | **0.86** | 1.07 / 0.93 / 0.88 |
  | 40 dB | 0.97 | **0.87** | 1.07 / 0.94 / 0.90 |

  The lapped 8x8 is as good as the best wavelet (5 levels) at 38-40 dB, keeps the block pipeline (macroblock motion compensation, coefficient memories), and its post-filter replaces the deblocking loop filter
  (which the x264 ablation could not measure here). Wavelets were rejected: no efficient motion-compensated temporal tool, entropy coder cost, whole-frame level structure. **Caveats:** three frames, one film; absolute bits are not comparable to real
  codecs (no intra prediction, estimator contexts differ by transform); the lapped filter's matrix was fitted by a crude 6-angle search on two of the same frames; the interaction with inter prediction is unmeasured (section 8, E-B).
* **Intra:** no spatial prediction (Daala's finding, and a long GOP makes key frames ~1% of the file: replacing MPEG-2 intra by AV1 intra saves ~2%, section 1 of the clean-sheet doc). DC predicted from neighbours (MED), optional first-row/column AC prediction.
* **Inter:** one vector per macroblock (L adds four 8x8 vectors), median vector prediction, half-pel bilinear (S/M), quarter-pel 4-tap (L, unmeasured: E-C), skip and zero-vector modes, B pictures with single-direction and average bi-prediction. The reference residual picture is lapped-transformed as a whole picture.
* **Quantiser:** log-scale QP, per-macroblock delta, a perceptual weighting matrix per file; all rate-distortion work (trellis, multi-pass, adaptive QP, B structure, vector search) is **offline in the encoder**, where the decoder pays nothing.
* **Entropy coder:** binary range coder, 32-bit range, 16-bit probabilities each held as two estimators (fast and slow rates) averaged at use; context RAM in M10K; syntax binarised with unary/Exp-Golomb prefixes;
  coefficient coding = last position, significance, magnitude with contexts from the neighbourhood (the same neighbourhood set measured in the transform study). Target 2 clocks per bin, so a 100 MHz lane does 50 M bins/s.
* **No loop filter**; optional later: a very light de-ringing stage if viewing shows ringing from the lapped transform.

### 4.2 Audio (TAC)
MDCT, sine/KBD-like own window, 1024-line long blocks and 8 x 128 short blocks (transient flag), 48 kHz stereo with per-band M/S, scale-factor bands on a roughly Bark-spaced grid, uniform quantiser with per-band step,
**noise filling** of zeroed bands from a transmitted RMS, spectral coefficients through the shared range coder (contexts: previous-frame band energy, neighbouring lines). Layers: base (0-12 kHz, 64 kbit/s) and
enhancement (HF + finer quantisation, +32..64 kbit/s) as **separable byte ranges** inside every audio packet. Decoder (estimate): range decode + dequantise + 1024-point IMDCT through a 512-point FFT with one multiplier (the whole frame is ~7 k butterflies per channel against 1.4 M available clocks per 21 ms),
overlap-add RAM; about 600-900 ALMs, 1-3 DSP, 4-6 M10K. The hard part is the offline encoder (bit allocation, transient detection), not the decoder. Frames are locked to video frames (2000 samples at 24 fps, 2400 at 20, 3200 at 15; 23.976 fps is not supported).

### 4.3 Subtitles (TST)
* **Text events** (UTF-8, start/end in audio samples, position/alignment/colour class) plus a **glyph table baked per file**: Tau Omega renders exactly the used code points from any TTF to 4-level (2 bpp) bitmaps at the chosen size (e.g. 14x28 cell at the 800x720 grid = 28x56 panel pixels: legible at 615 ppi), so every language and a user-selected font cost nothing in the core. Typical sizes: 300 glyphs x ~100 B = 30 kB.
* **Layout in the encoder** (line breaking, shaping for complex scripts) so the core only blits glyph runs; the overlay unit composites at scan-out from a line buffer (alpha 0/33/66/100%, outline by the glyph bitmap itself).
* **Bitmap events** (image subtitles from DVD/PGS sources): palette of 4-16 colours, run-length coded rows, decoded during scan-out; an offline OCR is not needed.
* Placement: below the picture inside the letterbox if the free band is at least two text lines (16:9 at 800 wide leaves 270 rows, 2.4:1 leaves 386), otherwise over the picture with an outline.
* Cost (estimate): 300-500 ALMs, 2-3 M10K (line buffer + glyph cache); no CPU involvement. The same overlay path is the player's OSD, so the UI comes nearly free.

### 4.4 Container (TNV3)
Header (profile, size, frame rate, layer map), index with key and layer flags, then packets in decode order: video slices (one packet per slice, CRC32), audio frame packets (locked to the video frame), subtitle events, glyph table. Seeking lands on key frames; layer flags let a small core skip droppable pictures without parsing them.

## 5. Why not an existing codec (the learning-only list)
MPEG-2 / H.264 / AV1 / Opus / Vorbis / AAC / FLAC / PGS were *read for ideas and numbers* (sections 3 and the clean-sheet doc). Nothing above is a transcription: syntax, binarisations, transform parameters, window, entropy coder and container are new.
Concretely I used from them only: the shape of an MC hybrid, the idea of a lapped filter (and measured it myself), the idea of noise filling, and the PGS idea that subtitles are small objects.

## 6. Scalability

| Profile | Picture | Rate | Tools | Decoder shape (estimate) | ALMs |
|---|---|---|---|---|---|
| **S** (small / slow core) | up to 400x180 at 24 fps, or any size at a reduced rate by dropping the B layer | 12-24 fps | I/P, 1 vector, half-pel, 1 slice at a time, base audio layer | one lane everywhere, serial 1-D transform (8 clocks/row), entropy 1 bin per 4 clocks, 66 MHz | about 4-5 k |
| **M** | 800x334 at 24 fps | 24 | + B (non-reference), 1 vector, half-pel, full audio | one lane, parallel transform, entropy 2 clocks/bin, 66-100 MHz | about 5-6 k |
| **L** | 800x720 at 24 (30 stretch) | 24-30 | + 4 vectors, quarter-pel, reference B, BWE | two entropy lanes (slice parallel), two-reference MC, row-per-clock transform, 100 MHz (SDRAM 100 or 166 MHz) | about 8-9 k |

Scaling mechanisms, all in the format: (1) **nested tools**: S is a strict subset of M of L, a decoder refuses a stream whose profile exceeds it (cheap conformance test); (2) **slices** are independent so L = two lanes of the S/M RTL (a parameter, `LANES`);
(3) **temporal layer**: a profile-M file played on an S core runs at half rate by dropping non-reference B pictures; (4) **audio layers**; (5) the encoder produces one bitstream per target profile (a profile-M file also plays on L).
Area figures are bottom-up estimates and the decisive probes are in section 8.

**Leeway for the player** (R5), Profile L: decoder 8-9 k ALMs + base (bridge/loader, SDRAM controller and arbiter, scan-out, tiny control core ~1 k) 2-2.5 k = about 11 k of 18.5 k ALMs (60%), leaving 7 k for UI, OSD, library, thumbnails, resume, settings; M10K about 100-130 of 308; the control CPU keeps at least half of its cycles because it parses no video.

## 7. Encoder strategy (Tau Omega, offline)
All adaptivity is encoder-side: motion search (hierarchical, clamp to the decoder's vector window), mode decision with real bit costs from a model of the range coder, trellis quantisation, adaptive QP by visual activity, 2-pass rate control with a decoder-buffer model (SD sustained 850 KB/s,
a 1 s peak cap), B-structure decisions, scene-cut keys, GOP 2-4 s. The reference encoder is written in C first (it is also the executable specification), then productised in Tau Omega. A decoder-buffer rule (peak bytes per second and per frame, derived from the measured SD rate) is part of conformance.

## 8. Experiments that decide the open questions (host first, kill criteria stated before running)

| # | Question | Method | Proceed / kill |
|---|---|---|---|
| E-A | Does the whole design reach the size target? | write `tools/tvc_ref/` (C): I/P/B, 16x16 MBs, lapped 8x8, two-rate range coder, half-pel; run the 9 clips (3 Tears, 3 anime, 3 B&W); compare with the measured MPEG-2 / x264 / libaom curves of the clean-sheet sweep | at VMAF 90 and SSIM: at most 3.3 kB/frame at 800x334 (-25% vs MPEG-2) to proceed, 2.9 target; above 3.7: drop lapped/range coder tweaks, re-plan |
| E-B | Does the lapped transform survive inter residual pictures? | same reference codec, switchable filter, residual pictures with real motion; also with a plain loop deblocking filter instead | lapped must beat plain+deblock by 5% on PSNR and not lose by SSIM; otherwise plain DCT + own simple deblock |
| E-C | Quarter-pel vs half-pel value | switch in the reference codec | below 5% saving: half-pel only in L |
| E-D | Two-rate counters vs single-rate, binarisation and context sets | log bin costs in E-A | decide the context RAM size |
| E-F | Audio quality at 64 / 96 / 128 kbit/s with noise filling | reference MDCT encoder/decoder in Python+C; blind listening by the owner on the existing pack (`work/codec_phase0/audio`), plus band-wise noise-to-mask proxy | owner picks the base rate; reject if worse than 128k MP3 at 96k |
| E-G | Platform numbers: SD sustained with larger reads, SDRAM bandwidth at 100 vs 166 MHz with scan-out running, bridge loader behaviour (KB-028) | diagnostic firmware on the existing hardware core (no new RTL), Pocket run | tells if Profile L is feasible at 100 MHz |
| E-H | Area probes (`tools/vm_probe.py`): range-decoder lane + context RAM + syntax FSM; lapped 8x8 pipeline; two-reference MC; overlay/glyph unit; IMDCT | synthesis-only | decoder over 9 k ALMs for L or 6 k for M: re-plan |
| E-I | Patent / freedom-to-operate review of the lapped transform family, the binary coder and MDCT audio window choices | owner or counsel; I can only list the known families (JPEG XR's lapped transform under a Microsoft patent promise, Malvar lapped-transform patents, CABAC state machines, Dolby/Fraunhofer audio patents; recalled) | if the lapped filter is not clear: plain DCT + own deblock (costs the 13%) |

## 9. Quality/size targets
Pass thresholds are on the clean-sheet sweep's clip set at 800x334, VMAF 0.6.1 and SSIM-Y and PSNR-Y all agreeing: **minimum viable 3.3 kB/frame (-25% vs MPEG-2's 4.3), target 2.9, stretch 2.6 (x264 slow = 2.7)**; AV1-lean's 1.6 is explicitly not a target. Audio: indistinguishable from MP3 128k by the owner's listening at the rate chosen in E-F.
Absolute stream rates implied (800x334, 24 fps): 62-79 kB/s video, 8-16 kB/s audio, i.e. 8-11% of the SD rate; 800x720 L: about 2.15x those.

## 10. Development plan, gates and risks

| Phase | Work | Exit gate |
|---|---|---|
| P0 | Specifications (bitstream v0 draft, the section 4 tools) + reference codec E-A/E-B/E-C/E-D | G1: size target met on the 9 clips, else re-plan; freeze Profile M bitstream v0 |
| P1 | Skeleton core (E-G): new minimal top (bridge/loader FIFO, SDRAM controller/arbiter, YUV scan-out + overlay, I2S, tiny control core), plays a trivial intra stream with PCM audio and a baked-glyph subtitle | G2: A/V sync and sustained SD/SDRAM numbers on a Pocket; area of the base known |
| P2 | Video RTL by block, each with C model + mutants + probe: range decoder and syntax, transform + lapped filter, MC, reconstruction/DMA | G3: each block within its budget; frames equal the C model in simulation |
| P3 | Profile M integration, fit (two seeds), Pocket run | G4: 800x334 at 24 fps with audio on a Pocket |
| P4 | Audio RTL (TAC), subtitles RTL, container/demux in hardware | G5: film plays with sound and subtitles, CPU idle |
| P5 | Profile L (two lanes, 4-MV, quarter-pel, reference B), Profile S conformance, Tau Omega encoder | G6: 800x720 at 24 fps; S core plays an M file at half rate |
Effort estimate (working sessions of the kind used in this project): P0 8-12, P1 4-6, P2 15-20, P3 4-6, P4 8-10, P5 10-14: **about 50-70 sessions**, six or more fits. Risks, in order: (1) the design misses the size target (E-A); (2) lapped transform + inter interplay (E-B); (3) SDRAM bandwidth for Profile L (E-G);
(4) audio encoder quality (offline research effort); (5) timing closure of the entropy lane's context-update loop (two clocks per bin is a loop of read-modify-write on M10K: design it as a two-stage pipelined update with a bypass); (6) patent clearance (E-I); (7) scope: three codecs plus a container in one project: the phase gates exist so each can be dropped (subtitles and audio fall back to FLAC/ADPCM and text events with a fixed font).

## 10b. Owner decisions and resulting choices (2026-10-27, after the adversarial review)

| Topic | Owner said | Resulting choice |
|---|---|---|
| Quality bar | neither "beat MPEG-2" nor "close to H.264": a good balance for the Pocket's constraints with reasonably small files | the target becomes a **file-size budget at a fixed visual quality**, not a codec comparison: about **400-450 MB per hour at 800x450 (16:9), 300-350 MB per hour at 800x334 (2.39:1)**, video near VMAF 90, audio 12-16 kB/s. That is about 3.3 kB/frame at 800x334 plus audio (estimate derived from the sweep); to confirm by the owner after R0 |
| Audio | unsure; same size concern as video | **lossy MDCT transform codec**, conservative rate **96-128 kbit/s (12-16 kB/s)**, where a simple encoder can sound clean; 64 kbit/s needs a sophisticated encoder and is a later refinement. Near-lossless predictive coding (30-40 kB/s, 2-3x larger) and FLAC (80-110 kB/s) fail the size concern; ADPCM/PCM only for skeleton tests. Audio then is about 10-15% of the file. No separable audio layers |
| Levels or profiles | choose what fits the Pocket | **one tool set (a single profile) with levels.** The Pocket is one device: one encoder family, one conformance suite, no refused streams. Levels bound only size, rate and buffering, so a smaller hardware build (fewer lanes) declares a lower level and Tau Omega encodes to it. Proposed: **Level 1** up to 400x240 at 30 fps; **Level 2** up to 800x450 at 30 fps; **Level 3** up to 800x720 at 30 fps (4:3 TV at 800x600 is the real worst case). Each level also fixes the peak bytes per second (well under the measured 850 KB/s SD rate), the largest frame and the decode-ahead buffer |
| RTL | do not re-derive the existing RTL | the TNV3 decoder RTL is written new; review finding M6 is withdrawn. The existing project RTL stays untouched as history |
| Frame rates | not exact; any issue when compressing? | **none**: the format stores a rational frame rate and the audio samples per frame. At 48 kHz: 24 fps = 2000, **23.976 = exactly 2002**, 25 = 1920, 30 = 1600, 29.97 = 1601.6 (a 1601/1602 pattern repeating every 5 frames, 8008 samples). The encoder never re-times the source. Encoder duties: inverse telecine (29.97 film transfers back to 23.976: fewer frames, no judder), deinterlace interlaced TV, convert variable-frame-rate sources to constant by duplicating/dropping frames. Display: audio is the master clock; on a 48 or 60 Hz output a 23.976 or 29.97 file repeats one refresh about every 30-40 s, invisible in practice; the refresh can also be set per file (47.952 Hz is inside the Pocket's 47-61 Hz window) |

## 11. Owner decisions needed
1. Go for P0 (a C reference codec and the E-A/E-B experiments) as the next piece of work?
2. Nominal acceptance of the size targets in section 9 (minimum viable -25% vs MPEG-2)?
3. Frame rates: 24 only, or 20/15 flags as in the old player? (Frame-locked audio makes 23.976 unsupported.)
4. Subtitle scope: text + baked glyph table first, bitmap events later?
5. Who does the patent review (E-I), and is a lapped-transform fallback to plain DCT acceptable?
6. SDRAM clock: stay at 100 MHz, or test 166 MHz (E-G)?
