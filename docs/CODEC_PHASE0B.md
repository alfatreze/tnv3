# Codec phase 0b: libaom tool-by-tool study on nine clips from three sources (2026-10-20/21)

Continues `docs/CODEC_PHASE0.md` (phase 0a: three live-action clips, x264/x265/SVT-AV1). Where the two disagree, this
file wins: it has three times the material and the real AV1 reference encoder. Host only, no RTL, no Pocket.
Everything is MEASURED unless marked "est.". Raw data and tables: `docs/codec_phase0/`; scripts: `tools/codec_study/`.

## What was added

* **libaom 3.15.2** (`aomenc`, Homebrew; installed with the owner's permission). It exposes an encoder switch for almost
  every AV1 coding tool, which SVT-AV1 does not, so each tool's value could be measured by switching it off.
* **Two more sources** supplied by the owner (never committed, `media/` is git-ignored):
  * an anime episode (1080p HEVC 10-bit) -> three 800x450 clips;
  * a 1960s black-and-white TV episode (640x480 H.264, 0.7 Mbit/s, so already soft and lossy) -> three 800x600 clips
    (the source is upscaled 1.25x for the 800 tag; its own grain is partly smoothed by that earlier encode).
* Result: **9 clips x 7 quality points per configuration, 3,000+ encodes**, all decoded and scored against the
  reference with VMAF, SSIM and PSNR-Y exactly as in phase 0a. Same equal-quality comparison method.

| clip | source | output | what it exercises |
|---|---|---|---|
| dialog / dark / action | Tears of Steel | 800x334 | outdoor close-up / dark interior / VFX motion |
| anime_wide | anime | 800x450 | courtroom, several characters, flat colour |
| anime_dialog | anime | 800x450 | bright magic circles, then a talking head on a flat sky |
| anime_motion | anime | 800x450 | sword draw, fast character motion |
| bw_garden | B&W TV | 800x600 | greenhouse foliage, figures moving, camera drift |
| bw_closeup | B&W TV | 800x600 | close-ups, wicker texture, little motion |
| bw_room | B&W TV | 800x600 | interior, two characters |

### The anime file is damaged (and why it matters for the method)

The episode has zero-filled holes, a broken seek index and **silent slice corruption** that the decoder does not
report: a seek-and-decode of any window returned the same scene or corrupt pictures, and one window that passed a
decoder-error scan and a chroma/temporal glitch detector still showed a persistent garbage band. What works: one
sequential decode of the whole file into lossless intermediates (`slime_intermediate.py`), then **visual inspection of
a 3x3 frame grid of every candidate window** (`montage.py`). The three anime windows were chosen that way. The pixel
detector (`scan_glitch.py`) is kept in the repo but must not be trusted on its own. Any new damaged source needs the
same visual check.

## 1. Codec ladder at equal quality (nine clips)

Mean size change against tuned MPEG-4 (the best the existing decoder family can do). Negative = smaller.

| config | VMAF live / anime / B&W | VMAF all | SSIM all | PSNR-Y all |
|---|---|---|---|---|
| mpeg4 with today's TNV1 flags | +17 / +19 / +16% | +17% | +12% | +13% |
| H.264 Main, no B (x264 slow) | -30 / - / -21% (6 clips) | -25% | -39% | -30% |
| H.264 Main + B | -40 / -45 / -31% | -39% | -52% | -46% |
| H.264 High | -45 / -51 / -39% | -45% | -56% | -50% |
| HEVC (x265 medium) | -55 / -54 / -47% | -52% | -64% | -60% |
| AV1, SVT-AV1 preset 6 | -70 / -68 / -68% | -69% | -75% | -74% |
| AV1, SVT-AV1 preset 4 | -72 / -71 / -69% | -71% | -77% | -76% |
| **AV1, libaom cpu-used 4 (all tools)** | -74 / -73 / -71% | **-73%** | -77% | -76% |
| AV1 libaom, 25 zero-value tools off (`lean0`) | -71 / -70 / -69% | -70% | -75% | -74% |
| `lean1` (lean0 + no restoration, blocks <= 32x32, 4-layer pyramid, 3 refs) | -68 / -67 / -65% | -67% | -72% | -71% |
| `lean2` (lean1 + no rectangular partitions) | -68 / -66 / -64% | -66% | -71% | -70% |

The ordering is the same on live action, anime and black-and-white, and on all three metrics. kbps needed for VMAF 90:

| clip | tuned MPEG-4 | H.264 High | HEVC | libaom full | lean0 | lean1 | lean2 |
|---|---|---|---|---|---|---|---|
| dialog | 601 | 264 | 231 | 122 | 131 | 148 | 151 |
| dark | 523 | 295 | 225 | 131 | 141 | 163 | 167 |
| action | 723 | 493 | 424 | 269 | 293 | 314 | 317 |
| anime_wide | 296 | 136 | 145 | 81 | 91 | 101 | 106 |
| anime_dialog | 476 | 261 | 243 | 140 | 152 | 169 | 170 |
| anime_motion | 276 | 135 | 114 | 66 | 77 | 82 | 84 |
| bw_garden | 1163 | 789 | 729 | 462 | 490 | 539 | 543 |
| bw_closeup | 1081 | 615 | 528 | 256 | 273 | 323 | 326 |
| bw_room | 1022 | 656 | 633 | 359 | 373 | 428 | 434 |

Even the leanest AV1 subset tested (`lean2`) is **about 38% smaller than H.264 High** (0.34 / 0.55) and 44% smaller than
H.264 Main+B (0.34 / 0.61), by VMAF; SSIM and PSNR agree to within a few points.

## 2. What each AV1 tool is worth

Size increase when one tool is switched off, libaom cpu-used 4, mean over the six colour clips (the single-tool runs
were not repeated on the B&W clips): VMAF / SSIM / PSNR-Y. Rounding is to whole points, so differences of 1-2 points
are noise. The combinations at the bottom ran on all nine clips.

| tool or setting | VMAF / SSIM / PSNR | verdict |
|---|---|---|
| CDEF | 0 / +1 / +1% | worth nothing here |
| OBMC, palette, intra block copy, filter-intra, chroma-from-luma, smooth and Paeth intra, intra edge filter, inter-intra, masked compound (wedge, difference-weighted), distance-weighted compound, dual filter, flip/identity transforms, 64-point transforms, 4:1 and AB partitions, 64 vs 128 superblock | 0 / 0 / 0% each | worth nothing here |
| warped motion, global motion, ref-frame MV projection, reduced transform-type set, 4-layer vs deeper pyramid | +1 / 0-1 / 0-1% each | negligible |
| 3 reference frames instead of 7 | +3 / +3 / +3% | small |
| 3-layer pyramid (min height) | +2% | small |
| intra angle delta | +3 / +4 / +3% | small |
| delta-q, ALT-REF temporal filter, trellis quantisation, TPL model | +3-5% | small, **encoder-only** (free for the decoder) |
| loop restoration | +5 / +1 / +1% | VMAF only; keep or drop is a judgement call |
| rectangular partitions | +6 / +7 / +7% | worth keeping |
| partitions limited to 32x32 | +6 / +6 / +6% | cost of a smaller decoder |
| partitions limited to 16x16 | +16 / +17 / +17% | too expensive |
| rectangular transforms | +9 / +5 / +9% | worth keeping |
| **adaptive probability (CDF) update off** | **+10 / +12 / +11%** | the arithmetic decoder must adapt |
| future references (low-delay structure) | +42 / +37 / +32% | the structure is essential |
| encoder effort: cpu-used 3 / 6 | -3% / +13% | offline, so use the slowest practical |
| `lean0` (25 zero-value tools off together) | +9 / +9 / +9% | the small ones add up |
| `lean1` / `lean2` | +21 / +24% (SSIM, PSNR within 1-2 points) | see section 1 |

`lean0` switches off CDEF, OBMC, palette, intrabc, filter-intra, CfL, smooth and Paeth intra, intra edge filter,
inter-intra (with smooth and wedge), masked compound (with wedge and diff-weighted), distance-weighted compound,
dual filter, flip/identity transforms, 64-point transforms, warped and global motion, angle delta, ref-frame MVs, and
the 1:4 and AB partitions.

## 3. Resolution switching, retested with libaom on nine clips

Coded at half size and upscaled with Lanczos, against native, at equal VMAF (ratio below 1 = switching wins). It won in
**2 of 30 comparisons** (dialog and bw_closeup at VMAF 70: 0.96 and 0.98); at VMAF 80 the ratio is 1.03-1.26 and at
VMAF 85 it is 1.05-1.62, and the half-size stream tops out at VMAF 84-87. This confirms phase 0a: with a modern codec
the idea is dead. (The odd 225-row anime half-size clips cannot be encoded by x264 or x265, so those two are missing
for anime; libaom and SVT handle them.)

## 4. Film-grain synthesis

libaom estimates the grain, codes a denoised picture and sends a parametric model (`--denoise-noise-level`). At the
same quality setting the result was **larger, not smaller** (+1% to +30%) with a VMAF drop of 0.3-2.4, on all nine
clips including the black-and-white ones (`docs/codec_phase0/film_grain_table.md`). The cause is not understood and
was not investigated, and the metrics compare against the grainy original so they cannot judge whether the result
looks better. No evidence of a saving; leave it out of the baseline and judge by eye on a grainy source if it is
wanted.

## 5. Workload on the new clips (x264 Main+B at CRF 26, as in phase 0a)

| | anime_wide | anime_dialog | anime_motion | bw_garden | bw_closeup | bw_room |
|---|---|---|---|---|---|---|
| kbps | 237 | 360 | 240 | 897 | 600 | 609 |
| vertical vector p99 / p99.9 (px) | 2 / 9 | 4 / 20 | 6 / 26 | 26 / 68 | 5 / 17 | 5 / 19 |
| horizontal p99 / p99.9 (px) | 3 / 29 | 5 / 23 | 4 / 32 | 28 / 71 | 5 / 17 | 6 / 19 |
| skip share of B / P macroblocks | 95% / 84% | 85% / 57% | 89% / 67% | 57% / 27% | 67% / 32% | 66% / 34% |
| true bi-prediction, % of B MBs | 1.6 | 3.8 | 1.8 | 6.0 | 2.0 | 3.2 |
| key frame mean (KB) | 31.3 | 20.1 | 19.9 | 32.5 | 48.8 | 40.9 |
| biggest frame / mean frame | 27x | 12x | 19x | 8.6x | 20x | 15x |
| worst second / mean | **2.9x** | **3.1x** | **3.2x** | 1.5x | 1.7x | 1.4x |

(The dark clip was re-run at CRF 26 instead of 24: 333 kbps, vertical p99 12 / p99.9 26, worst second 1.4x.)

What changes in the sizing conclusions:

* **Anime is the worst case for burstiness**: the worst one-second window is about 3x the mean (scene cuts, flat then
  complex), against 1.4-1.7x for live action and the B&W show. A decode-ahead queue sized for 1.5x is not enough; size
  it for 3x (a few MB of SDRAM) or the engine must run 3x faster than the mean demands for a second.
* The B&W garden shot has the **largest vertical reach** (p99 26 px, p99.9 68): a +-16 row cache covers only the 90th
  percentile there. Slow zooms and camera drift are common in old TV material, not just in VFX action.
* In flat anime 85-95% of B macroblocks are skipped, so the average decode work per frame is very low and a peak-sized
  engine idles most of the time (relevant for power).
* True bi-prediction stays at 2-6% of B macroblocks.

## 6. Pre-registered criteria, final status

| criterion | status |
|---|---|
| AV1 at least about 10% better than x264, else choose H.264 | **passed, about 55% smaller than H.264 Main+B and 50% than High** (libaom, nine clips) |
| half-resolution switching wins at least about 10% on some shots | **failed again** (2 of 30 comparisons, by 2-4%) |
| context-mixing recode at least about 4% | still NOT MEASURED. New evidence only that adaptive probabilities are worth 10-12% over static ones, so the arithmetic decoder's adaptation is a real lever |

## 7. What the evidence says about a Pocket profile (bits only: area is still unmeasured)

* **Keep** (each costs at least 3% to drop, or is structural): adaptive multi-symbol arithmetic coding, hierarchical B
  with future references and about 3 reference frames, partitions from 8x8 to at least 32x32 including rectangles,
  4-32 point DCT/ADST including rectangular sizes, intra directional modes with angle delta, deblocking (phase 0a),
  8-tap sub-pel interpolation, delta-q (cheap, helps rate control).
* **Drop** (cost under 1% each, 9% together): the 25 `lean0` tools. Dropping them removes the compound masks, OBMC,
  warped/global motion, palette/intrabc, CfL, most intra extras, CDEF and the 64-point transform. This is by far the
  largest hardware saving per bit lost.
* **Optional**: loop restoration (5% by VMAF, 1% by SSIM/PSNR), 64x64 and larger blocks (6%).
* **Encoder-only, free**: trellis, TPL, ALT-REF temporal filter, slowest practical preset. Run libaom (or a better
  encoder) at its slowest setting in Tau Omega.
* A **family** falls out naturally: three nested tool sets, `lean0` (largest), `lean1`, `lean2` (smallest), which cost
  0.30, 0.33 and 0.34 of tuned-MPEG-4 size. A core built for `lean2` decodes only `lean2` streams; the `lean0` core
  decodes all three. Nothing here says what each costs in ALMs: that is the next measurement.
* Not tested and therefore not claimed: whether libaom can be forced to a vertical vector range (it has no such flag
  that I found; x264 and x265 do), what share of the bitstream is motion vectors and mode syntax (decides whether a
  custom, simpler syntax could save bits or only silicon), and 4:4:4 / 10-bit.

## 8. Limits

* Nine clips from three sources; one anime episode and one TV episode, one film. The B&W source was already a 0.7 Mbit/s
  H.264 encode, so its "original" is soft; absolute bitrates there are lower than a clean scan would give, ratios
  between codecs should still hold.
* VMAF 0.6.1, SSIM and PSNR-Y only. No subjective viewing. The metrics disagree about deblocking and restoration.
* Single-tool runs: six colour clips only, cpu-used 4, two-pass constrained quality, key interval 96. `aom_refs2` is
  invalid in libaom (minimum 3 references): that run is empty.
* Per-block AV1 tool usage and syntax bit shares were not measured (needs an instrumented decoder).
* Rate numbers describe the encoder as well as the format. Where a tool shows 0%, the encoder may also simply not
  choose it at this operating point; a different content class (screen content, sports) could change that.

## 9. Next

1. **Area before more bits.** Synthesis-only area probes (the V-003 method, `tools/vm_synth.py`) for the blocks that
   `lean0..lean2` imply: multi-symbol adaptive arithmetic decoder, 4-32 point DCT/ADST engine, 8-tap interpolation unit,
   deblocking filter. Without these the profile above is a bit budget, not a design.
2. **Syntax cost.** Instrument libaom or dav1d to split bits into coefficients, motion vectors, modes and side
   information. Decides standard AV1 syntax (decode libaom streams directly) versus a simpler transcoded syntax.
3. **Vertical reach and AV1.** Measure vector distributions on the AV1 streams themselves and test whether a
   transcoder-side clamp costs more than the 0-2% seen in x264.
4. Listen to the audio pack (`work/codec_phase0/audio/`); measure fixed-point Opus decode cost on the simulated CPU.
5. A grainy live-action source and a screen-content-like source (credits, subtitles) before dropping palette/intrabc
   and film-grain for good.

## Reproduce

```
python3 tools/codec_study/prep.py                        # cut clips (needs media/ and the slime intermediates)
python3 tools/codec_study/slime_intermediate.py          # one sequential decode of the damaged anime episode
python3 tools/codec_study/sweep.py 7 [--clips a,b] [config names...]   # resumable
python3 tools/codec_study/report.py vmaf [base config]   # also ssim, psnr_y
python3 tools/codec_study/combine.py report|aomrep [filter]
python3 tools/codec_study/workload.py ; python3 tools/codec_study/fg_table.py
```
