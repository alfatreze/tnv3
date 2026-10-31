# Codec alternatives to AV1 / TNV2: what is worth exploring (2026-10-27)

Worktree `codec-alternatives` (branch `worktree-codec-alternatives`, fast-forwarded to `codec-phase0` at `08f581d` so all codec docs are present). Desk study, no RTL, no
Pocket. Written from `CODEC_PHASE0.md`, `CODEC_PHASE0B.md`, `CODEC_AV1_WORKLOAD.md`, `CODEC_AV1_INTER_PLAN.md`, `CODEC_ROADMAP_2026-10-23.md` and the project memory.
Evidence per line: **measured** (from those docs), **estimate**, **recalled** (my background knowledge of a standard, NOT checked this session: verify before relying on it).

## 1. What the accumulated knowledge says the real constraints are

Bits are not the scarce resource; area, decode time and risk are.

| Fact | Evidence |
|---|---|
| SD sustains 850 KB/s; all-intra AV1 needs about 100 KB/s (4.2 KB/frame at 24 fps), inter about 22 KB/s (932 B/frame) | measured (V-011, `INTER_PLAN` s.1) |
| A 2 h film is about 730 MB all-intra, about 160 MB with P frames; the card is 128 GB | arithmetic |
| Decode time, not bitrate, bounds the picture: intra takes about 23 ms + 1.7 ms/KB at 400x166; 800 wide is 4x | measured / estimate |
| Free fabric next to the 160 KB build: 2,486 ALMs, 31 M10K, 29 DSP; the inter profile needs about 2,270 ALMs (98.8% of the device) | measured fit + estimate |
| `tau_av1_blk` is the area and timing problem: 3,284 ALMs after G2 (target 3,000), the MV stack and the transform tree were the critical paths | measured (probes) |
| Bits by syntax element: coefficients 58-76%, motion vectors 8-17%, inter modes/refs/skip 8-17%, partition 2-5% | measured (workload) |
| Tool values: CDF adaptation +10-12% if off, rectangular partitions +6-7%, rectangular transforms +5-9%, future references +32-42%; CDEF, OBMC, palette, CfL, 64-pt transforms, dual filter about 0 | measured (phase 0b) |
| Ladder vs tuned MPEG-4 at equal VMAF: H.264 Main+B -39%, High -45%, HEVC -52%, lean AV1 -66..-70%, full AV1 -73% | measured |
| AV1 hidden/pyramid streams are 52-71% bi-predicted by area; our profile 1 is single-reference low-delay | measured / plan |
| Profile 1 (P-only, low delay) forgoes a structure worth +32-42% bits if removed from a full encoder | measured (phase 0b), so profile 1 leaves a lot on the table |
| Rule: optimal RTL beats reuse; existing fabric is not a constraint; MPEG-4 is on hold and dropped by default | owner rules |

Consequence for what is worth exploring: an alternative must beat AV1-lean on **ALMs x decode-ms x verification effort x licence**, not on bitrate. Anything that
costs 30-50% more bits but removes the 3.3 k-ALM block syntax or the serial symbol wait is a real candidate; anything that merely compresses less is not.

## 2. Candidates, grouped by what they change

### Group A: keep AV1 reconstruction, change the syntax layer (highest value, cheapest to test)

**A1. TNV3 "syntax transcode": a lossless re-coding of libaom's decisions into a simpler bitstream.** The Tau Omega encoder runs libaom (all its RD quality), then re-writes
the same modes, vectors and coefficients with a decoder-friendly syntax. Reconstruction stays pixel-identical to libaom's decode of the original stream, so the existing oracle
(libaom trace and picture compare) stays valid; only the entropy/context layer changes. Targets, in order of area:
* **Motion vectors with a trivial predictor** (median of left/top/top-right, as in MPEG-4/H.264) instead of the AV1 ref-MV stack. The stack is 3.6 k ALMs in the golden blk and
  the critical path of every probe since G1. MVs are only 8-17% of the bits, so a 20% worse MV code costs about 2-3% total (estimate). **Experiment E2.**
* **Mandatory single reference, index 0** (the lean encoder already emits `ref_select = 0`): removes the reference-context machinery (2.1 k ALMs golden).
* **Fixed block and transform-size menu** (for example 64/32/16/8 square plus 2:1 rectangles only), no variable transform tree: the tree was 2.5 k ALMs golden. Rectangular partitions are
  worth 6-7% and rectangular transforms 5-9%, so keep those, drop the rest of the tree.
* Keep the CDF-adapting multi-symbol coder (10-12% is too much to lose) and the coefficient contexts (58-76% of bits).
Risk: this is a new format with no external decoder to test against (mitigated by the pixel oracle), and the transcoder needs the libaom trace as input (the trace hooks exist). It
also detaches us from "any AV1 file plays". Given that TNV2 already wraps the OBUs in our own container and plays only our own encodes, that loss is small.

**A2. AV1 profile 1 extended with look-ahead structure (hidden frames, `show_existing_frame`) but still one reference per block.** Phase 0 measured +32-42% bits for losing future references; profile 1 is
exactly the low-delay case. A mini-GOP with a hidden forward frame, each block predicting from ONE reference (either side), needs two more SDRAM buffers (64 MB is plentiful) and a reorder in
the driver, but no compound unit. Caveat: libaom uses compound for 52-71% of inter area in free-running encodes, so how much of the gain survives a single-reference restriction is **unmeasured**
(x264 B-frames gain mostly through single-direction references, but that is H.264 evidence). **Experiment E4.** This is an extension of the main track rather than an alternative, but it is the
largest bit saving still on the table.

**A3. Zero-MV / integer-MV inter.** An inter profile where a block is either intra, "copy from the reference at the same place" (+ residual), or an integer-vector copy. No 8-tap filter, no subpel
syntax; the MC lane (290 ALMs, 8 DSP, small) shrinks to an address generator. The MV stack remains unless combined with A1. Value depends on static content (skip blocks are 73-97% of the
area in the workload, but skipped is not the same as zero-vector). **Experiment E3** (histogram of zero / integer / fractional vectors from the existing traces).

### Group B: different transform and entropy family (intra-first, throughput play)

**B1. Wavelet intra codec (VC-2 / Dirac Pro, JPEG 2000, or a custom LeGall 5/3 plus bit-plane coder).** Properties (recalled): line-based streaming, no blocking, no loop filter, no
partition tree or mode contexts, fixed work per pixel, resolution scalability for free (thumbnails, fast seek preview). Hardware: a 1-sample/clock lifting datapath is tiny; the
cost sits in the entropy coder. Throughput is the interesting number: 800x334x1.5 = 400 k samples a frame, 2-3 clocks each is about 10 ms at 100 MHz, against AV1 intra's serial symbol decode
(23 ms + 1.7 ms/KB at 400 wide, estimate 4x at 800). Weakness: intra-only compresses worse (MJPEG measured 11.6 KB/frame vs AV1 intra 4.2 KB at the same size, V-002; a wavelet codec should sit between, estimate
6-8 KB) and a temporal extension (motion-compensated wavelet) is research, not engineering. Role: **an 800-wide, higher-frame-rate intra option**, not a replacement for P frames.
**Experiment E1.**

**B2. Intra-only block codecs with a lighter entropy layer** (H.264 intra CAVLC, JPEG XL VarDCT, HEIF-like). Worse than AV1 intra and no simpler to build than what already works (AV1 intra is bit-exact on a
Pocket, V-106). Include in E1 only as rate reference points; do not design for.

### Group C: other standards

| Candidate | Why consider | Why probably not | Status |
|---|---|---|---|
| **VP9** | royalty-free; 64x64 superblocks, bool coder (binary), transforms and 8-tap MC close to AV1's, so intra/transform/MC RTL transfers; no CDEF/LR | backward adaptation by per-frame counts adds a frame-end pass and storage; between H.264 and AV1 in bits (recalled, roughly 15-25% worse than AV1); lose the working AV1 chain and its libaom oracle for a smaller step | rate point in E5 only |
| **EVC Baseline (MPEG-5 Part 1)** | designed as royalty-free baseline: simple tools, no deblock-heavy pipeline; reference encoder xeve (BSD) | recalled as roughly H.264 class in bits; small ecosystem; no evidence it is simpler than AV1-lean in silicon; xeve not installed | rate point in E5 if xeve builds |
| **H.264 Baseline/Main** | simplest mainstream hybrid; measured -16% (Baseline) to -39% (Main+B) vs tuned MPEG-4 | measured 2-3x larger than AV1-lean; patent pool still an issue for an MIT public project (earlier note: no evidence supports it); CABAC engine is not much simpler than the AV1 symbol decoder | do not pursue |
| **HEVC / VVC** | strong compression | patent pools, large syntax and CTU machinery; HEVC measured -52% vs AV1's -66..-73% | do not pursue |
| **AV2** | newer AOM codec | unfrozen/complex, no hardware-lean subset known | revisit if a lean subset study appears |
| **MJPEG / MPEG-4** | exists | MJPEG 2.8x larger than AV1 intra (V-002); MPEG-4 is CPU-bound and on hold (V-114) | no |
| **LCEVC, learned/neural codecs, JPEG XS** | novelty | enhancement layer needs a base codec; neural decode is far beyond 18 k ALMs; XS targets high bitrate | no |

### Group D: system-level moves that change which codec wins

* **Hybrid by content:** all-intra AV1 for scenes where P frames do not pay, P frames elsewhere (the player already handles both file kinds, V-177). Free in the format; needs a per-GOP choice in the encoder.
* **Frame-rate / resolution ladder per file** (24/20/15 fps flags exist from V-068): lets the decoder run at 800 wide at lower rate. Phase 0 killed *spatial* switching with modern codecs; temporal reduction is untested.
* **Audio is a separate axis:** Opus 64 kbit/s vs MP3 128 kbit/s was inaudible to the owner; MP3 costs 34% of the CPU at stereo (V-068), mono much less. Candidates: MP3 mono (now), IMA-ADPCM or mu-law (2-3% CPU, 4:1 / 2:1), Opus (needs fixed-point CPU cost measured; a hardware MDCT already exists in the MP3 window unit). Keep this as the shared item in the roadmap.

## 3. Recommendation

1. **Do not abandon AV1.** No alternative has shown a path to smaller silicon at comparable bits, and the AV1 intra chain is verified on hardware. VP9, EVC, H.264 and HEVC are rate points to measure, not designs to start.
2. **The most promising new direction is A1 (syntax transcode),** because it attacks the one block that is blocking the inter path (`tau_av1_blk`, MV stack and transform tree) while keeping the reconstruction oracle.
   Cheap to test on the host with the traces already in the repo.
3. **A2 (look-ahead structure with single-reference blocks) is the biggest remaining bit saving** and is an extension of the current plan; measure how much survives the single-reference restriction.
4. **B1 (wavelet intra) is the only genuinely different architecture worth a small study,** pitched at 800 wide / higher fps rather than as a replacement.

## 4. Experiments (host only, pre-registered kill criteria)

| # | Question | Method | Kill / proceed |
|---|---|---|---|
| E1 | Intra rate ladder: libaom all-intra (lean), JPEG 2000 (openjpeg via ffmpeg), VC-2 (ffmpeg `vc2`), x264 intra, JPEG XL, MJPEG, equal VMAF | frames of the nine phase-0b clips, extend `tools/codec_study/sweep.py` | wavelet within 1.6x of AV1 intra: carry B1 to an area/throughput estimate; above 2x: drop |
| E2 | Cost of an explicit/simple-predictor MV code vs AV1's stack | MV and mode records from the libaom trace of the GOP clips; entropy of MV residuals vs median predictor; also count bits of the stack-derived contexts | total P-frame size increase under 4%: adopt A1 MV part and re-scope blk G3 |
| E3 | Share of zero, integer and fractional vectors, and of blocks that are skip + zero vector | same trace | zero+integer over 60% of inter area: A3 is worth an MC-lane simplification |
| E4 | How much of the hierarchical-B gain survives one reference per block | libaom with look-ahead, then count compound blocks; re-encode restricted if a flag allows, otherwise estimate from per-block reference use | under 40% of the +32% retained: drop A2 |
| E5 | Rate points for VP9 (libvpx) and EVC (xeve) on the same clips | install via Homebrew (downloads: owner approval) | not within 25% of lean AV1: closed |
| E6 | Area and clocks per sample of a 1 sample/clock 5/3 lifting engine plus a simple context coder | synthesis-only probe (`tools/vm_probe.py`), only if E1 passes | decides B1 |

E1-E4 need no installs except what Homebrew already has (aomenc, ffmpeg, x264 are present; vpxenc and xeve are not). Effort estimate: E2-E3 about one session each, E1 about one, E4 about one.

## 5. Owner decisions needed

1. Approve running E1-E4 (host only, no card writes, no fits) before any further blk work on the inter path, or run them alongside A7-G3?
2. Is a non-libaom-decodable TNV3 syntax acceptable if E2 passes (the file would play only on Tau V)? TNV2 already only comes from `tools/tnv2.py`.
3. Approve downloading libvpx / xeve for E5 (Homebrew installs).
4. Is an 800-wide, intra-only fast-codec option (B1) of interest, or is 400x166 final?
