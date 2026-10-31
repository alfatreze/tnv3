# Adversarial review of the TNV3 plan (2026-10-27)

Reviewer stance: A/V codec developer for embedded hardware and the Analogue Pocket, asked to attack `docs/TNV3_CODEC_SCOPE_2026-10-27.md`, `docs/CODEC_CLEAN_SHEET_2026-10-27.md` and the evidence in
`tools/codec_study/clean_sheet/`. The plan was written in this same session; the Codex review could not run (model not available on the account), so this is a self-review by the same author and
should be read with that bias in mind. Findings are ranked by how much they change the plan. Each states the claim, why it fails, and the recommended change.

## Verdict

The *direction* is right: decoder-simple and encoder-heavy, all syntax and reconstruction in hardware, the CPU out of the data path, bits traded for silicon. The *plan* over-promises in four places that
decide success: it removes the deblocking filter and spatial intra prediction on evidence that does not support it, it assumes a home-made encoder will perform like x264, it sizes the top profile for a
picture no film has, and it splits one codec into three profiles that multiply verification work without buying anything the device needs. The evidence base (one CG film, 2-second clips, one
in-sample transform fit) is too thin to freeze tools on. Fix those before P0 starts.

## Critical findings (change the plan)

### C1. "Lapped transform replaces the deblocking filter" is unsupported, and wrong for inter frames
> **R0 update:** the bullet saying the x264 deblock switch failed is wrong (`no-deblock=1` worked; only `deblock=0` was a no-op). The conclusion stands on better evidence: deblocking is worth 17-26% by SSIM/PSNR on 9 clips, and held out the lapped transform beats a deblocked DCT by only 2-6%. Intra share is 16-28% (C2's 4-10% was also too low). See `docs/TNV3_R0_RESULTS_2026-10-27.md`.
* The -13% for the lapped 8x8 was measured on **intra luma only**, against a plain DCT **without** a deblocking filter, by PSNR. It says nothing about blocking *visibility* and nothing about inter frames.
* In a P/B-heavy stream (more than 95% of frames) the visible blocks come from **motion-compensated prediction discontinuities and skipped macroblocks**, not from residual quantisation. A post-filter that runs on the
  *residual* picture cannot smooth an edge that exists in the *prediction*. That is exactly why codecs pair block MC with a loop filter or OBMC. Daala, the one serious lapped-transform video codec, never shipped and AV1 did not adopt lapping.
* Lapping the residual also makes macroblock decisions non-local: a skipped macroblock is no longer a copy (its neighbours' post-filter leaks into it), which complicates encoder rate-distortion and the "skip" semantics.
* Every x264 "lite" proxy the size estimate was built on **had deblocking on** (the switch failed in the harness: `lite_nodb` equals `lite_cabac` to the last digit), so the plan removed a tool from the measured baseline and then counted the lapped gain on top.
* **Recommendation:** baseline = plain integer 8x8 transform + a simple, own in-loop deblocking filter (edge-strength from mode/coded flags, 1-3 pixel modification, one pass, line-buffer based: a few hundred ALMs). Keep the lapped transform as an *experiment* (E-B) that must beat deblocked DCT on inter streams by SSIM and by eye before it replaces anything.

### C2. "No spatial intra prediction: key frames are about 1% of the file" is miscalculated
* With the measured numbers, MPEG-2 intra 14.8 kB vs P 4.3 kB: GOP 96 gives key frames 3.5% of the bytes; GOP 48 (2 s) 7%; once P frames shrink to the target 2.9 kB, a 2 s GOP puts **10% of the file in key frames**.
* Films cut every 3-5 s on average: every cut needs an intra picture or intra macroblocks regardless of GOP, so the real intra share is higher still. Intra quality also propagates: P frames inherit the key frame's errors.
* Seeking and scrubbing want short GOPs, which raise the share further.
* **Recommendation:** give intra the cheap spatial predictors (DC, horizontal, vertical, planar on 8x8, from reconstructed neighbours; simple with a plain DCT, awkward with lapping: another reason for C1). Measure intra share on real films with scene cuts in E-A.

### C3. The size prediction assumes x264's encoder; the project will have a home-made one
* The 2.4-2.7 kB/frame estimate multiplies x264 ablation ratios. x264's numbers include decades of encoder work (psy-RD, trellis, mbtree, adaptive quantisation); the phase-0 study measured encoder effort alone at 7-23%.
  A first in-house encoder typically lands 20-40% above a mature one on the same bitstream (estimate from the preset spread measured in phase 0).
* The "minimum viable 3.3 kB/frame (-25% vs MPEG-2)" gate compares the home encoder against **ffmpeg's `mpeg2video`, a weak encoder**; passing it is easy and proves little. The owner cares about file size (the FLAC question), so the gate should be against a strong reference.
* **Recommendation:** make the encoder a first-class deliverable with its own effort line (it is at least as large as the decoder RTL). Gate G1 against **x264 slow restricted to the same tools** (same partitions, same entropy class, deblock on): within 15% = pass. Report also against libaom so the owner sees the real price of "custom" in bytes.

### C4. Profile L is sized for a picture films do not have, and three profiles triple verification for no device benefit
* 800x720 is the 10:9 output window. A 16:9 film is 800x450, 2.39:1 is 800x334, 4:3 TV is 800x600 (fits) or 960x720 (does not). Sizing decode throughput and SDRAM traffic for 800x720 active at 24-30 fps overstates the real worst case by 1.2-2.2x.
* The Pocket is one device. "Small core / large core" matters only as *area vs. features inside one bitstream*. Three nested tool profiles (S/M/L) need three conformance suites, three encoders settings families and decoders that refuse streams; the measured value of the L-only tools is small (extra partitions 0%, 3 refs 3%, quarter-pel unmeasured).
* The claim "an S core plays an M file at half rate by dropping B pictures" is inconsistent: S is sized for 400x180 line buffers and throughput; the remaining I/P pictures of an M file are still 800x334.
* **Recommendation:** **one tool set** (I/P + non-reference B, one vector, half-pel unless E-C proves quarter-pel at 5%+), **levels** only for size/rate limits (Level 1 up to 400x240, Level 2 up to 800x450 at 24, Level 3 up to 800x600 at 30). Scale the *hardware* by RTL parameters (lanes, pipeline width) that all decode the same syntax. Drop the S-plays-M claim or size S line buffers for 800.

### C5. SDRAM bandwidth is the real limit, the budget has no margin once efficiency is counted
* The plan's ~100 MB/s of 200 MB/s at 100 MHz uses the *peak* rate. A 16-bit SDR SDRAM with refresh, row switches, short random MC bursts, planar 4:2:0 scan-out (separate luma and chroma rows) and the loader's writes delivers roughly 50-65% of peak in practice (estimate; the project measured a 46-clock random access and a controller that refreshes before every read, V-021/V-026). That is 100-130 MB/s usable: the budget is consumed, not 35% free (R5 fails).
* Scan-out is hard real time: the arbiter must guarantee it, so decode traffic gets what is left in bursts. B bi-prediction doubles reference reads; quarter-pel 4-tap adds overfetch.
* **Recommendations:** (1) run E-G *first* (SDRAM throughput with a synthetic scan-out + random-burst load at 100 and 133/166 MHz, CL3 per KB-021); (2) use the **second memory channel**: the two PSRAM chips (sync burst to 133 MHz, own bus) can hold the display frames and scan-out while SDRAM holds references and the decode-ahead queue, roughly doubling bandwidth; (3) fetch MC references in macroblock-row bands to turn random fetches into bursts.

### C6. Frame-locked audio that refuses 23.976 fps breaks most real content
> **Correction (same day):** 23.976 fps at 48 kHz is exactly 2002 samples per frame, so the plan's "23.976 is not supported" was itself an error: storing the samples-per-frame figure (and a 1601/1602 pattern for 29.97) solves it without re-timing. Resolution recorded in `docs/TNV3_CODEC_SCOPE_2026-10-27.md` section 10b.
* Film masters are 23.976 (24000/1001), TV is 29.97 and 25. Forcing 24.000 shifts duration by 0.1% (audio must be resampled or sped up) and 29.97 has the same issue; 25 fps sources need 1920 samples per frame, fine, but the plan lists only 24/20/15.
* **Recommendation:** keep sync structural but carry a **per-packet sample count** (e.g. 2002/2002/2002/2001-style patterns for 24000/1001); the display refresh is chosen per file (47.952 Hz is inside the 47-61 Hz window) and generated from the same PLL reference as the audio MCLK so the two clocks keep an exact ratio (core-generated 12.288 MHz and pixel clock from one VCO, KB-081/KB-148; feasibility to check in the PLL tool). Audio stays master for presentation; a rare repeated field is the fallback.

### C7. A from-scratch psychoacoustic audio encoder is a research project hiding in one table row
* The decoder is small; the encoder (masking model, bit allocation, transient detection, stereo decisions, noise-fill levels) is where audio codecs win or lose, and "indistinguishable from MP3 128k" against a 30-year-tuned family is a high bar for a first encoder. The earlier listening test was not blind.
* The audio "byte-separable layers" add encoder and container complexity for nothing: audio decode costs about 1 k ALMs at every profile; there is no small core that cannot afford it.
* **Recommendations:** stage it. Stage 1: a **custom predictive near-lossless coder** (adaptive linear prediction + noise-shaped quantisation of the residual + the shared range coder), about 2.5-3.5 bits/sample (30-42 kB/s stereo, estimate), no psychoacoustics, easy to verify; it already halves FLAC. Stage 2: the MDCT codec, gated by a **blind ABX** test against MP3 128k and Opus 64k. Drop audio layers.

## Major findings (weaken evidence or schedule)

### M1. The evidence base is too thin to freeze tools
* The clean-sheet sweep used **one CG film** (Tears of Steel), three **2-second** clips, 48 frames, with one key frame per clip (key-frame weight 1/48, not 1/96 as in a real GOP). Anime and black-and-white clips from phase 0b were not run. MJPEG was encoded full-range (`yuvj420p`) and converted back for scoring.
* The transform study fitted the lapped filter on **two of the three test images** (in-sample), lumps all 16x16 frequency classes above 11 into one context (a handicap for dct16), uses an ideal two-pass static entropy without model cost, and reports PSNR-Y only. The -13% may shrink out of sample.
* **Recommendation:** before any tool decision: rerun the sweep on all nine phase-0b clips at 10 s, with held-out training for any fitted parameter, SSIM/VMAF on decoded colour, and record the estimator's limits in the result file.

### M2. The entropy decoder speed (2 clocks per bin) is optimistic
* An adaptive binary decoder's loop is: context address (often depends on the previous bin) -> M10K read -> range x probability -> compare -> renormalise -> probability update -> write-back. At 100 MHz on Cyclone V that is 3-5 clocks per bin without speculation; the project's own AV1 symbol decoder was planned at ~28 clocks per multi-symbol read.
* Budgets still close at 4-5 clocks/bin for Level 2 (a 20-36 kB key frame is 0.2-0.4 M bins, 1-2 M clocks), but only with a **decode-ahead queue** sized for key frames.
* **Recommendation:** design for 4 clocks/bin, speculate the two next contexts where the binarisation allows, and keep slices for Level 3 (slice count costs 1-3% bits: measure).

### M3. Patent distance is asserted, not checked, and two chosen tools sit close to known families
* Two-rate probability estimators were standardised in VVC's CABAC; noise filling has many patents (one appeared in this session's search); lapped pre/post filters are close to JPEG XR's transform (Microsoft covenant) and older Malvar patents (recalled; expiry dates not checked).
* "Written from scratch" does not mean "free to use".
* **Recommendation:** default to tools whose prior art is old and public: single-rate shift-adapted binary models (1970s-90s range coding), plain DCT + deblock, MDCT with sine window, no noise filling in stage 1. Add the newer tools only after E-I clears them.

### M4. No independent oracle
* The AV1 and MPEG-4 work was trustworthy because libaom/ffmpeg were external oracles. A custom codec has none: an encoder and decoder written by the same hand agree on the same misunderstanding, and a real encoder exercises only a fraction of the syntax (the MPEG-4 decoder found ffmpeg never emitted several modes).
* **Recommendation:** two independent decoder models (C reference + a short Python model from the written spec), a **random-syntax stream generator** that covers every element and value range, conformance streams frozen with checksums, and the RTL replayed against both models with mutants (the project's existing discipline).

### M5. Schedule realism: a product is far away and nothing ships in between
* 50-70 sessions with the first playable film at P3-P4. The AV1 track showed how the hard part (inter path, area, timing) expands late.
* **Recommendation:** define a **minimum viable codec** that plays a whole film early: Level 2, I/P only, plain 8x8 DCT, simple deblock, spatial intra (DC/H/V), half-pel, one vector, single-rate range coder, stage-1 audio, text subtitles with baked glyphs. Ship that through the skeleton (P1) in roughly 15-20 sessions; add B pictures, then experiments, after a film plays on a Pocket.

### M6. Reuse of the project's *own* RTL is not "reusing a codec"
* The verified IDCT, half-pel MC, row-burst fetch, SDRAM read client, DMA and register patterns in `tau_mp4v_*` are the project's own work, already on hardware. The owner's rule is "no existing codec" and "optimal RTL over reuse unless optimal".
* **Recommendation:** treat those blocks as starting points to re-derive and re-optimise for the new syntax (keep their tests and mutant method); it removes perhaps 5-8 sessions of P2 (estimate). Confirm with the owner.

## Minor findings
* **Subtitles:** ASS/anime styling (positioning, karaoke, signs) cannot be expressed as text events; the encoder should rasterise those to bitmap events automatically. Overlay blending must happen after YUV-to-RGB, at 1 pixel/clock: fine.
* **Container CRC:** say what the player does on a bad packet (conceal by repeating the previous picture; mute the audio frame); the SD path is reliable, so keep CRCs for development and make them optional.
* **Power:** continuous SDRAM scan-out at 48-60 Hz and racing the decoder are the main fabric draw; measure battery hours on the skeleton (the skill notes higher scaler/display modes raise draw). Not in the plan.
* **Output timing:** 800x720 at 60 Hz needs a pixel clock near 38-40 MHz (inside the ~50 MHz limit); at 48 Hz about 30-32 MHz; pick refresh per file (C6).
* **Frame buffers:** with B pictures and decode-ahead: 2 references + 1 B + display + queue of 4-8 = up to 12 frames of 864 kB = 10 MB: fine in 64 MB, but state it.
* **Chroma** with any lapped filter, and 4:2:0 scan-out upsampling, are unspecified.

## Revised plan (recommended)

| Step | Work | Gate |
|---|---|---|
| R0 (1-2 sessions) | Re-run the sweep on 9 clips x 10 s; held-out transform study; deblock switch fixed; intra share on films with cuts | numbers, not opinions |
| R1 (2-3) | Platform measurements E-G: SDRAM throughput with scan-out load at 100/133/166 MHz, PSRAM burst bandwidth, SD sustained with large reads, PLL ratio for locked audio/video clocks | decides Level 3 and the memory split |
| R2 (6-10) | Minimum viable codec reference (C) + independent Python decoder + syntax fuzzer + stage-1 audio + text subtitles; encoder v1 | within 15% of x264-restricted at VMAF 90 / SSIM |
| R3 (8-12) | Skeleton core and MVC decoder RTL (plain DCT, deblock, intra, half-pel, range decoder), on a Pocket | a film plays with sound and subtitles, CPU idle |
| R4 | B pictures; then experiments one at a time (lapped vs deblock, quarter-pel, two-rate models, MDCT audio), each adopted only on measured gain and patent check | per experiment |
| R5 | Level 3 (lanes, PSRAM display channel), encoder quality work in Tau Omega | 800x600 at 30 / 800x450 at 24 with margin |

## Questions the owner should answer before R0
1. Is the bar "beat MPEG-2" or "close to H.264 with our own codec"? (It changes how much encoder work is worth it.)
2. Accept a staged audio path (custom near-lossless first, MDCT later), or MDCT from the start?
3. One tool set with levels instead of three profiles?
4. May the project's own MPEG-4 RTL blocks be re-derived as starting points?
5. Which frame rates must be exact (23.976, 25, 29.97)?
