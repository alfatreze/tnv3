# TVC reference codec v1: first results (R2, 2026-10-27)

Bitstream: `docs/TVC_BITSTREAM_V0.md` draft 1.1 (I/P pictures, 16x16 macroblocks, one half-sample vector, four intra modes per 8x8 block, 8x8 integer transform, adaptive binary range coder, in-loop deblocking, optional macroblock QP deltas).
Code: `tools/tvc_ref/` (`tvc.h` shared decoder side with ONE syntax function for decode/encode/bit counting; `tvc_dec.c`; `tvc_enc.c` encoder v1; `test_tx.c`; `make_vectors.py`; `py/tvc_dec.py` independent decoder; `eval_tvc.py`, `eval_an.py`). All **measured**.

## Verification
* Unit tests (`test_tx`): range coder round trip on 20,000 mixed context/bypass bins, 0 mismatches; transform round trip at step 1: max error 4 (rounded integer matrix; acceptable, could be tightened).
* Five conformance vectors (`tools/tvc_ref/vectors/`, incl. QP deltas, low and high QP, an odd 99x71 crop): encoder reconstruction = C decoder output bit for bit.
* **Independent decoder:** written by a separate agent from the specification only, without access to the C code: all five vectors decode bit-identical on the first run. Its list of 29 interpretation points produced the normative clarifications in section 7b of the spec. No spec error was found.
* 90 evaluation encodes: encoder reconstruction = decoder output on every one.

## Rate at equal quality (9 clips x 10 s; kB/frame, geometric mean; same method as R0)

| | VMAF 90 | SSIM 0.97 | PSNR-Y 38 |
|---|---|---|---|
| **TVC v1** (no AQ) | **3.16** | **2.09** | **1.82** |
| ratio to x264 restricted (`lite_cabac`: 16x16, 1 ref, 4x4, CABAC, deblock, **quarter-pel**) | 1.07 | 1.09 | **0.89** |
| ratio to MPEG-2 I/P | **0.94** | **0.64** | **0.67** |
| TVC v1 with variance AQ (strength 1.0) | 3.70 | 2.05 | 2.07 |
| x264 slow / libaom (cpu-used 6), for scale | 2.00 / 1.56 | 1.31 / 0.85 | 1.40 / 1.00 |

Per group at VMAF 90 (live / anime / B&W): TVC v1 3.43 / 1.62 / 5.65 kB/frame; vs `lite_cabac` 1.25 / 0.93 / 1.05.

Reading:
* The **first encoder on a minimal tool set already matches the x264 proxy** (better by PSNR, equal by SSIM, 7% behind by VMAF) and is 33-36% below MPEG-2 by SSIM/PSNR. The R0 decision (adaptive binary coder + deblocking + 8x8 transform) holds up in a real implementation.
* The gap is **live action by VMAF** (dialog 1.53x `lite_cabac`): fine texture, where x264 has quarter-sample motion and psycho-visual tuning that VMAF rewards.
* The simple variance AQ **hurts** VMAF (+17%) and PSNR (+14%) and helps SSIM only 2%: as implemented it is wrong for this content and stays off. (A propagation-based QP scheme like x264's look-ahead is the measured 15% lever, not variance AQ.)
* Encoder speed: about 9 s per 240-frame clip (27 fps at 800x334-800x600, one core).

## File size against the owner's budget (VMAF 90, audio at 14 kB/s added)
| Content | Video kB/s | Per hour |
|---|---|---|
| live action, 800x334 | 82 | about 350 MB |
| anime, 800x450 | 39 | about 190 MB |
| B&W TV, 800x600 (grainy upscale) | 136 | about 540 MB |
Within the proposed 400-450 MB/h for film and anime already; grainy 4:3 TV is over.

## Update: encoder v2 with B pictures and look-ahead QP (bitstream draft 2, V-188)
Draft 2 adds non-reference B pictures (list 0 = previous anchor, list 1 = latest anchor, bi-prediction by averaging, display-order output by an anchor-hold rule). Encoder v2: 2 B pictures between anchors (B at QP+2), and look-ahead QP propagation (`-t`, now default 2.0). Verification: draft-1 vectors still decode bit-identically; 4 new B vectors (v06-v09); the independent Python decoder, updated from the draft-2 text only, passes all 9; 137 evaluation encodes with recon = decoder output.

| all 9 clips, kB/frame | VMAF 90 | SSIM 0.97 | PSNR-Y 38 |
|---|---|---|---|
| TVC v1 (I/P) | 3.16 | 2.09 | 1.82 |
| TVC v2 (+2 B) | 2.83 | 1.76 | 1.60 |
| TVC v2 + look-ahead 1.0 | 2.67 | 1.62 | 1.49 |
| **TVC v2 + look-ahead 2.0** | **2.64** | **1.56** | **1.48** |
| ratio v2-t2 to: x264 restricted `lite_cabac` / with B `lite_b` | 0.89 / 0.99 | 0.81 / 0.90 | 0.72 / 0.80 |
| ratio v2-t2 to: MPEG-2 I/P / x264 slow | 0.79 / 1.32 | 0.48 / 1.19 | 0.55 / 1.06 |

* B pictures: -10% (VMAF) to -16% (SSIM); look-ahead QP: a further -6..-11%, mostly on anime (-16% at VMAF 90), about neutral on live action.
* TVC v2 now beats the x264 proxy with the same tool class (16x16, 1 vector, B) by SSIM and PSNR and equals it by VMAF; it needs half the bytes of MPEG-2 by SSIM/PSNR; it is 6-32% above x264 slow (which has quarter-pel, partitions, 3 references, 8x8 adaptive transform, psy tuning).
* Remaining gap is **live action by VMAF** (1.43x x264 slow).
* File size at VMAF 90 with 14 kB/s audio: live action about 300 MB/h, anime about 160 MB/h, grainy 4:3 TV about 475 MB/h.
* Encoder speed: about 18 s per 240-frame clip (13 fps, one core).

## Update: quarter-sample motion (bitstream draft 3, encoder v3, V-189)
Per-picture `qpel` flag: vectors in quarter luma samples, own 4-tap luma filters (Catmull-Rom cubic: (-4,56,14,-2), (-4,36,36,-4), (-2,14,56,-4)), 1/8-sample bilinear chroma, deblocking threshold kept at one sample. Encoder: quarter-sample refinement after the half-sample step (now on by default). Verification: 12 vectors (v10-v12 qpel, incl. an odd 83x61 crop); independent Python decoder updated from the draft-3 text only passes all 12; 47 evaluation encodes with recon = decoder output.

| all 9 clips, kB/frame | VMAF 90 | SSIM 0.97 | PSNR-Y 38 |
|---|---|---|---|
| TVC v2 + look-ahead (half-sample) | 2.64 | 1.56 | 1.48 |
| **TVC v3 (+ quarter-sample)** | **2.19** | **1.43** | **1.34** |
| v3 / x264 restricted proxy (`lite_cabac`) / with B (`lite_b`) | 0.74 / 0.82 | 0.74 / 0.83 | 0.66 / 0.73 |
| v3 / MPEG-2 I/P | 0.65 | 0.44 | 0.50 |
| v3 / x264 slow (all tools) | **1.09** | **1.10** | **0.96** |

* Quarter-sample motion saves 17% (VMAF), 8% (SSIM), 9% (PSNR) overall; most on slow, smooth motion (dialog -37% by VMAF, B&W close-up -30%, dark -24%), little on anime and fast action (0.5-9%).
* **Live action, the previous gap, is now 1.09x x264 slow by VMAF (was 1.43x)**, and 0.96x by PSNR; B&W is below x264 slow on all three metrics. Anime remains 1.10-1.30x x264 slow (x264 has more partitions and references, which anime's flat areas and sharp edges reward).
* File size at VMAF 90 with 14 kB/s audio: live action about 240 MB/h, anime about 160 MB/h, grainy 4:3 TV about 380 MB/h: all inside the 400-450 MB/h budget.
* Encoder: about 24 s per 240-frame clip (10 fps, one core).

## Update: scene-cut I pictures, B count, 4x4 transform (encoder v4, bitstream draft 4, V-190)
* **Scene-cut I pictures** (encoder only): look-ahead frame ratio of motion-compensated to intra cost above 0.7 starts an I picture (at least 8 frames after the last I; the B pattern restarts after it). Three 60 s segments with real cuts (20 / 15 / 8 cuts detected), equal-quality size vs no cut detection: -0.4..-0.9% (live), -2.8..-3.1% (anime), -1.0..+0.1% (B&W). On the 10 s clips: -3% (VMAF) / -3.5% (SSIM). Now default on (also gives seek points at cuts).
* **B count:** 1 B picture clearly worse than 2; 3 about equal to 2 on the long segments: 2 stays; an adaptive per-group choice is deferred (expected under a few percent).
* **4x4 transform option** (draft 4, per-picture flag; luma 8x8 blocks may use four 4x4 transforms with their own coded flags; even rows of the 8x8 matrix; no deblocking inside the 8x8 block): 9 clips -2% live, -6..-7% anime, about 0 (+-1.5%) B&W; long segments -2..-3% live, -8.5% anime, +0.2..0.4% B&W. Now default on. Verified: 15 vectors bit-exact in C and in the independent Python decoder (updated from the draft-4 text only); 121 evaluation encodes with recon = decoder output.

| all 9 clips, kB/frame | VMAF 90 | SSIM 0.97 | PSNR-Y 38 |
|---|---|---|---|
| TVC v3 | 2.19 | 1.43 | 1.34 |
| TVC v3 + scene cuts | 2.12 | 1.38 | (not bracketed on one clip) |
| **TVC v4 (+ 4x4)** | **2.06** | **1.37** | (not bracketed on one clip; live 1.50, B&W 2.16) |
| v4 / x264 restricted proxy | 0.70 | 0.71 | |
| v4 / x264 slow | **1.03** | **1.05** | |

Anime: 1.13 / 1.17 x x264 slow (VMAF / SSIM; was 1.29 / 1.30 for v3); live 1.05 / 1.04; B&W 0.93 / 0.95. File size at VMAF 90 with 14 kB/s audio: live about 230 MB/h, anime about 140 MB/h, grainy 4:3 TV about 380 MB/h.

## Update: rate-distortion quantisation (encoder v5) and a rejected experiment (V-191)
* **RD quantisation** (encoder only; after the dead-zone quantiser, each level from the last scan position down is lowered by one when distortion + lambda x bits, counted on the live contexts, decreases): 9 clips -2.9% (VMAF), -0.7% (SSIM), about -2% (PSNR); long segments -3.1..-3.7% (VMAF), -1.6..-2.2% (PSNR). Encode time +24..37%. Default on.
* **Contexts carried from the latest anchor into the next picture** (bitstream experiment): about -1% at QP 30 on three clips; **rejected** (pictures would depend on another picture's entropy state: worse for seeking and error recovery, more hardware state for 1%). Removed from the code; the bitstream stays at draft 4.

| all 9 clips, kB/frame | VMAF 90 | SSIM 0.97 | PSNR-Y 38 |
|---|---|---|---|
| TVC v4 | 2.06 | 1.37 | n/a (one clip not bracketed) |
| **TVC v5** | **2.00** | **1.36** | **1.28** |
| v5 / x264 restricted proxy | 0.68 | 0.71 | 0.62 |
| v5 / MPEG-2 I/P | 0.60 | 0.42 | 0.47 |
| v5 / x264 slow | **1.00** | **1.04** | **0.91** |

By group vs x264 slow at VMAF 90 / SSIM / PSNR: live 1.01 / 1.03 / 0.92, anime 1.09 / 1.16 / 0.99, B&W 0.90 / 0.94 / 0.84. File size at VMAF 90 with 14 kB/s audio: live about 230 MB/h, anime about 140 MB/h, grainy 4:3 TV about 370 MB/h. Encoder: about 35 s per 240-frame clip (7 fps, one core).

## Next steps (ordered by measured or expected gain)
1. **B pictures** (non-reference, single-direction and averaged prediction): about -10% (R0).
2. **Encoder look-ahead QP propagation** (lower QP for macroblocks that later pictures reference): about -15% (R0, x264 mbtree); encoder-only.
3. **Quarter-sample motion** (decoder change; measure on the dialog/dark/action clips where the VMAF gap is).
4. Scene-cut key frames and a picture-type decision (intra share is 16-28% of real files).
5. 4x4 transform option, a 16x16 intra mode set, trellis quantisation, contexts carried between pictures.
6. Then slices and level limits; then the audio codec (TAC) reference.
