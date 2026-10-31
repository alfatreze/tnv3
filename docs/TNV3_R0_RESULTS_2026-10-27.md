# TNV3 step R0: the evidence re-run (2026-10-27)

Step R0 of the revised plan (`docs/TNV3_ADVERSARIAL_REVIEW_2026-10-27.md`): re-measure what the tool choices rest on, on more and longer material, with held-out fitting. All host-side, all **measured** unless marked. Raw data and scripts are in `tools/codec_study/clean_sheet/`, tables in `docs/codec_phase0/r0_*`.

| Part | Script | Output |
|---|---|---|
| Rate sweep: 9 clips x 10 s (240 frames; live 800x334, anime 800x450, B&W TV 800x600), 13 configurations x 6 points = 702 encodes; VMAF 0.6.1, SSIM, PSNR-Y | `r0_sweep.py`, `r0_an.py` | `r0_rate_table.txt`, `r0_results.json` |
| Held-out intra transform study (fit on 3 clips, test on 12 frames of 6 other clips), with a deblocked DCT and SSIM | `transform_study2.py` | `r0_transform_study.txt` |
| Intra share with real scene cuts: nine 60 s segments (3 per source), x264 with 3 B-frames, 4 GOP variants | `intra_share.py` | `r0_intra_share.md` |
| Which x264 switch disables deblocking | `check_deblock.py` | below |

## 1. Correction first: the deblocking switch was not broken
`check_deblock.py` reads x264's own settings record: `no-deblock=1` gives `deblock=0:0:0` (off); `deblock=0` gives `deblock=1:0:0` (still on, offsets 0). So the earlier `lite_nodb` runs were valid and only `lite_db0` was a no-op. The adversarial review's statement that "every x264 proxy had deblocking on, the switch failed" (C1) and the clean-sheet doc's caveat are **wrong**; C1's conclusion (keep a deblocking filter) is confirmed by better evidence in section 2.

## 2. Tool values on 9 clips (all-9 geometric mean; ratio to `lite_cabac` = x264, I/P, 16x16 blocks, 1 ref, 4x4 transform, CABAC, deblocking on)

| Configuration | VMAF 90 kB/frame | ratio | SSIM 0.97 | ratio | PSNR-Y 38 | ratio |
|---|---|---|---|---|---|---|
| `lite_cabac` (base) | 2.96 | 1.00 | 1.92 | 1.00 | 2.05 | 1.00 |
| VLC entropy instead of adaptive binary (`lite_cavlc`) | 3.61 | **1.22** | 2.24 | **1.16** | 2.40 | **1.17** |
| deblocking off (`lite_nodb`) | 3.04 | 1.03 | 2.42 | **1.26** | 2.40 | **1.17** |
| both off (`lite_cavlc_nodb`) | 3.68 | 1.24 | 2.78 | 1.45 | 2.77 | 1.35 |
| + B-frames (`lite_b`) | 2.68 | **0.91** | 1.73 | **0.90** | 1.85 | **0.90** |
| + 8x8 transform with 4x4/8x8 choice (`lite_8x8`) | 2.50 | **0.84** | 1.69 | **0.88** | 1.80 | **0.88** |
| + encoder look-ahead rate control (`lite_mbtree`, decoder-free) | 2.52 | **0.85** | 1.61 | **0.84** | 1.71 | **0.83** |
| everything x264 slow has (`lite_all`) | 2.08 | 0.70 | 1.38 | 0.72 | 1.46 | 0.71 |
| MPEG-2 I/P (`mpeg2_ld`) | 3.36 | 1.14 | 3.26 | 1.70 | 2.71 | 1.32 |
| x264 slow (`x264_ld`) | 2.00 | 0.68 | 1.31 | 0.68 | 1.40 | 0.68 |
| libaom cpu-used 6 (`aom_ld6`) | 1.56 | 0.53 | 0.85 | 0.44 | 1.00 | 0.49 |
| intra only: x264 / MPEG-2 | 14.81 / 19.29 | 5.0 / 6.5 | 11.78 / 21.15 | | 12.04 / 18.94 | |

Per group at VMAF 90 (live / anime / B&W, kB/frame): `lite_cabac` 2.75 / 1.75 / 5.40; `mpeg2_ld` 3.48 / 1.86 / 5.87; `x264_ld` 2.03 / 0.95 / 4.13; `aom_ld6` 1.40 / 0.79 / 3.44.

Reading:
* The three decoder tools that pay are confirmed on all content: **adaptive binary entropy coding (16-22%), a deblocking filter (17-26% by SSIM/PSNR; VMAF barely sees it), an 8x8 transform (12-16%)**, then **B-frames (about 10%)**. Encoder-only look-ahead is worth 15-17% at zero decoder cost.
* MPEG-2 is 14% above `lite_cabac` by VMAF but 70% by SSIM: VMAF under-weights blocking. Use SSIM and PSNR as gates next to VMAF, never VMAF alone.
* Against the clean-sheet sweep (3 clips x 2 s): ratios moved by a few points, the ordering did not change.

## 3. Held-out transform study (luma, intra, estimate; ratio to plain 8x8 DCT, 12 test images)

| | PSNR-Y 38 | SSIM 0.97 | anime, SSIM 0.97 |
|---|---|---|---|
| 8x8 DCT + simple deblock (params fitted on train) | 0.90 | 0.86 | 0.85 |
| 8x8 + lapped pre/post filter (fitted on train) | 0.84 | 0.84 | 0.87 |
| 16x16 DCT | 0.91 | 0.94 | 1.00 |
| wavelet 9/7, 4 / 5 levels | 0.85 / 0.79 | 0.80 / 0.75 | 0.71 (5 levels) |

* Out of sample, the lapped transform beats a *deblocked* DCT by **only 2% (SSIM) to 6% (PSNR)**, and loses on anime by SSIM. The first study's -13% was mostly the missing deblocking filter. **Lapped transform: dropped from the baseline**; it is not worth its interaction risk with inter coding.
* The 5-level wavelet is the most efficient *intra* transform measured (about 10% better than the deblocked DCT) but has no good inter tool; noted for a possible key-frame-only use, not pursued now.
* No spatial intra prediction in any of these (the estimator compares transforms only).

## 4. Intra share with real scene cuts (x264, 3 B-frames, mean of nine 60 s segments)

| GOP | cuts/min | I-frame byte share | intra MB in P | estimated total intra share (lower bound) |
|---|---|---|---|---|
| 2 s, cut keys | 17.2 | 24% | 7.8% | **28%** |
| 4 s, cut keys | 17.2 | 16% | 7.8% | **20%** |
| 10 s, cut keys | 17.2 | 12% | 7.7% | **16%** |
| 4 s, no cut keys | 17.2 | 11% | 10.2% | **16%** |

Per source: Tears of Steel 16 cuts/min, anime 29, B&W TV 6; anime with 2 s GOP reaches 32-45% intra. **Intra is 16-28% of a real file, not 1% (plan) or 4-10% (review).** Intra efficiency is a first-order design target: x264's intra (spatial prediction + adaptive coding) is 23% smaller than MPEG-2 intra by VMAF and 44% by SSIM.

## 5. Consequences for the TNV3 baseline tool set (v0)

| Tool | Decision | Evidence |
|---|---|---|
| Adaptive binary range coder (own design, single-rate first) | **keep** | 16-22% |
| In-loop deblocking filter (own, simple edge filter) | **keep, mandatory** | 17-26% by SSIM/PSNR |
| 8x8 integer transform; 4x4 as a per-block option | **keep 8x8; 4x4 option to measure in R2** | 12-16% for the 8x8 option over 4x4-only |
| B pictures (non-reference) | **keep** | about 10% |
| Spatial intra prediction (DC, H, V, planar; 8x8 and 16x16) | **keep** | intra is 16-28% of the file |
| Lapped transform | **drop** (archive the study) | 2-6% held-out, inter risk |
| Wavelet | no (intra-only advantage) | section 3 |
| Partitions, extra references | no | 0-3% (clean-sheet ablation) |
| Encoder look-ahead / adaptive QP / RDO | **encoder must have it** | 15-17% free for the decoder |
| Quarter-pel | still unmeasured (every x264 config used it) | R2 experiment |

## 6. File-size budget check (owner's "reasonably small")
`lite_cabac` (which TNV3 v0 should match or beat, since it adds the 8x8 transform and B-frames) at VMAF 90 with audio at 14 kB/s:

| Content | Video kB/s | Per hour incl. audio |
|---|---|---|
| live-action film, 800x334 | 66 | about 290 MB |
| anime, 800x450 | 42 | about 200 MB |
| B&W TV (upscaled, grainy, 800x600) | 130 | about 520 MB |

The proposed budget of 400-450 MB/h at 800x450 is comfortable for film and anime even at `lite_cabac`'s efficiency, and tight only for grainy 4:3 sources. If the v0 tool set lands near `lite_8x8` x `lite_b` (estimate 0.76 of `lite_cabac`) and the home encoder costs 20-40% (estimate), the result is again about `lite_cabac`: **about 200-300 MB/h for typical films and anime**. A tighter budget of about 300 MB/h at 800x450 is realistic; the owner decides.

## 7. Limits
Sizes are 10 s clips (key frame inside each clip at GOP 96); one quality model (VMAF 0.6.1, TV viewing); x264 used as the proxy for "a codec with these tools", so the TNV3 numbers will differ; the transform study is luma-only with an ideal estimator; intra share measured with x264 only.
