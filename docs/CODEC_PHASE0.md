# Codec phase 0: host-side rate study and workload dump (2026-10-20)

> **Superseded in part by `docs/CODEC_PHASE0B.md`** (nine clips from three sources, libaom tool-by-tool results, lean
> profile candidates, anime and black-and-white workloads). Read that first; this file keeps the original method,
> the x264 / x265 / SVT-AV1 comparison and the first workload dump.

Branch `codec-phase0` (worktree). Host only: no RTL, no Pocket. Purpose: decide, from measurements, which coding
tools justify silicon before any format or RTL is designed. Rule in force: optimised purpose-built RTL beats reuse of
existing Tau nu blocks unless the existing block is shown to be optimal; this study therefore compares formats by
what they cost in bits, not by what the current MPEG-4 hardware happens to implement.

Everything below is MEASURED on the three clips unless marked "est.". Reproduce with the scripts in
`tools/codec_study/` (see the last section). Raw data: `docs/codec_phase0/`.

## Method

* Source: Tears of Steel (CC-BY, Blender Foundation), 1920x800 (2.4:1). The 800-wide target is therefore **800x334**
  (not 800x450); the 400-wide one is 400x166. Both are derived straight from the source with Lanczos.
* Three 10 s clips (240 frames, 24 fps), chosen from a contact sheet: `dialog` (outdoor close-up, static, fine texture),
  `dark` (candle-lit interior, low key), `action` (VFX tower, camera and object motion). One film only.
* Encoders (ffmpeg 9.0.2): `mpeg4` (ffmpeg, the current TNV1 source), x264 0.165 preset slow, x265 preset medium,
  SVT-AV1 preset 6 (and 4, 9). Key interval 96 frames. libaom is not installed.
* Quality: VMAF (model vmaf_v0.6.1), SSIM (float) and PSNR-Y of the decoded picture (400-wide encodes upscaled to
  800x334 with Lanczos) against the 800x334 reference. Bitrate is the elementary stream size (no container).
* Comparison is at **equal quality**: bitrate interpolated log-linearly between the two bracketing sweep points (never
  extrapolated), then a Bjontegaard-style mean over a window both curves cover (VMAF 78-95, SSIM 0.94-0.985, PSNR-Y
  35-42 dB). Seven quality points per curve, 567 encodes, zero errors.
* Three metrics are reported because VMAF alone can be gamed by smoothing and rewards little for deblocking.

## 1. Headline: kbps at equal VMAF 90 (800x334)

| config | dialog | dark | action |
|---|---|---|---|
| mpeg4 default (current TNV1 flags) | 747 | 664 | 785 |
| mpeg4 tuned (`-mbd rd -trellis 1 -cmp 2 -subcmp 2 -flags +aic`) | 601 | 523 | 723 |
| H.264 Baseline (CAVLC, no B) | 430 | 461 | 698 |
| H.264 Main, no B (CABAC) | 361 | 379 | 586 |
| H.264 Main + B | 288 | 319 | 553 |
| H.264 High | 264 | 295 | 493 |
| HEVC (x265 medium) | 231 | 225 | 424 |
| AV1 (SVT preset 6) | 135 | 153 | 315 |
| AV1 (SVT preset 4) | 130 | 144 | 296 |

Mean size change against tuned MPEG-4 (negative = smaller):

| config | VMAF | SSIM | PSNR-Y |
|---|---|---|---|
| mpeg4 default | +17% | +17% | +14% |
| H.264 Baseline | -16% | -33% | -22% |
| H.264 Main + B | -40% | -53% | -44% |
| H.264 High | -45% | -57% | -48% |
| HEVC medium | -55% | -67% | -61% |
| AV1 preset 6 | -70% | -76% | -72% |
| AV1 preset 4 | -72% | -78% | -73% |

AV1 is about **half the size of H.264 Main+B and about 45% smaller than H.264 High**, and the three metrics agree.
At this operating point (VMAF 90) AV1 needs 135-315 kbps where the tuned MPEG-4 needs 520-720. Not representative
of a whole film: three 10 s clips from one source.

Two things that are actionable today, independent of any new format:

* The current TNV1 encode uses plain `mpeg4` flags. The tuned flags are **14-17% smaller at equal quality** (all
  three metrics). The existing decoder already implements AC prediction, which `+aic` needs. NOT yet checked
  against the project's own decoder (`test_mp4v_decode`), so confirm before relying on it.
* The size ladder H.264 Baseline -> Main -> High is mostly B-frames and CABAC (below).

## 2. Where the gain comes from

Relative to the stated reference, in % bits at equal quality (VMAF / SSIM / PSNR-Y; rounding noise about +-1-2 points,
the three metrics bracket the truth):

**H.264, against x264 Main+B**

| removed or changed | VMAF | SSIM | PSNR-Y |
|---|---|---|---|
| no B-frames | +17% | +26% | +20% |
| CAVLC instead of CABAC | +17% | +11% | +11% |
| no deblocking | +2% | +19% | +9% |
| 1 reference instead of 3 | +2% | +2% | +2% |
| vertical vector range limited to 16 px | +0-2% | 0% | 0% |
| High profile (8x8 transform etc.) | -8% | -9% | -7% |

**AV1 (SVT), against preset 6 with everything on**

| removed or changed | VMAF | SSIM | PSNR-Y |
|---|---|---|---|
| temporal filter (ALT-REF denoise) off | +3% | +4% | +4% |
| B pyramid 3 temporal layers (`hierarchical-levels=2`, 4-frame mini-GOP) | +3% | +12% | +11% |
| B pyramid 4 temporal layers (`hierarchical-levels=3`) | 0% | +4% | +4% |
| no 64-point transforms (max 32) | +3% | +4% | +4% |
| CDEF + loop restoration off | 0% | +4% | +4% |
| all in-loop filters off (incl. deblock) | +3% | +12% | +7% |
| B pyramid 4 layers + tx32 + no CDEF/LR (hardware-lean combo, run as one config) | +3% | +12% | +11% |
| low-delay structure (no future references) | +43% | +50% | +46% |
| preset 4 instead of 6 (slower encoder) | -7% | -8% | -4% |
| preset 9 instead of 6 (faster encoder) | +23% | +21% | +18% |
| film-grain synthesis on (clean source) | +17% | n/a | n/a |

Reading, with the caveats in the last section:

* **Reordered (hierarchical B) prediction is the biggest structural tool**: +43-50% when removed in AV1, +17-26% in
  H.264. It is cheap in memory here (64 MB SDRAM) and does not need the temporal filter. (SVT's low-delay mode may
  also change other encoder behaviour, so the +43-50% is an upper estimate of the structure alone.)
* **CDEF and loop restoration are worth 0-4%** on these clips. They are the AV1 filters that cost the most line buffer
  and logic; the metrics do not pay for them. H.264 deblocking is worth 2% by VMAF but 9-19% by SSIM/PSNR (and the
  eye may see blocking at 615 ppi when scaled 2x; not tested), so keep a deblocking filter.
* **64-point transforms (3-4%) and a shorter B pyramid (0-12%) are cheap to drop**: the hardware-lean combination
  (4 temporal layers, 32-point transforms, no CDEF/LR) was run as one configuration and lands at -69% / -73% / -69%
  against tuned MPEG-4, i.e. still **43-48% smaller than H.264 Main+B** (derived from the table values: 0.31/0.60,
  0.27/0.47, 0.31/0.56). The percentages are rounded to whole points, so differences of 1-3 points are noise.
* **Entropy coding is a large lever**: CABAC over CAVLC is worth 11-17% by itself in H.264.
* More than one reference frame is worth about 2%, and limiting vertical vectors to 16 px costs 0-2%.
* **Encoder effort matters as much as many tools**: preset 9 to 6 to 4 is a 23% then 7% change. The offline
  encoder in Tau Omega can use the slowest settings.
* Film-grain synthesis cannot be judged here: the source is clean and VMAF scores the grain-free output against the
  original. Needs a grainy source.

## 3. Resolution switching (code at 400x166, upscale to 800x334)

Size of the 400-wide encode at equal quality divided by the native 800-wide encode (below 1 = switching wins).
Lanczos upscaling only: a better upsampler would raise the ceiling but not the ordering.

| encoder | VMAF 70 | 75 | 80 | 85 | VMAF ceiling reached at 400 |
|---|---|---|---|---|---|
| tuned MPEG-4 (dialog / dark / action) | .97/.93/.76 | 1.09/1.04/.81 | 1.24/1.20/.88 | 1.49/1.46/.95 | 90 / 90 / 97 |
| H.264 Main+B | .78/.79/.77 | .86/.87/.78 | .97/.97/.83 | 1.27/1.23/.87 | 87 / 87 / 92 |
| HEVC | 1.02/1.06/1.01 | 1.08/1.13/1.03 | 1.21/1.25/1.08 | 1.48/1.52/1.14 | 87 / 86 / 90 |
| AV1 | -/1.09/1.07 | 1.02/1.12/1.12 | 1.10/1.24/1.15 | 1.36/1.50/1.21 | 87 / 87 / 91 |

With the old codecs, coding at half resolution wins on motion-heavy shots and at low quality. **With HEVC and AV1
it never wins** (ratio at or above 1 everywhere measured), and the 400-wide stream cannot even reach VMAF 90 on two
of the three clips. The stronger the codec, the less half-resolution coding helps.

## 4. Workload dump (what a hardware decoder has to handle)

x264 Main+B at CRF 24-26 (VMAF roughly 90-93; the CRFs were not tuned per clip), decoded with libavcodec `export_mvs` (`tools/codec_study/mvdump.c`).
This describes H.264, not AV1: per-block AV1 tool usage is not measured (needs libaom or an instrumented dav1d).

| | dialog | dark | action |
|---|---|---|---|
| stream rate (kbps) | 299 | 425 | 629 |
| vertical vector |dy| p50 / p90 / p99 / p99.9 (px) | 0 / 2 / 5 / 8 | 0 / 4 / 12 / 26 | 2 / 13 / 36 / 72 |
| horizontal |dx| p99 / p99.9 (px) | 6 / 13 | 12 / 28 | 35 / 87 |
| skip share, P / B macroblocks | 37% / 64% | 31% / 60% | 22% / 48% |
| true bi-prediction share of B macroblocks | 2.1% | 6.5% | 6.1% |
| 16x16 share of exported blocks (by count) | 87% | 80% | 72% |
| key frame bytes (mean) / P / B | 26.9 k / 2.8 k / 410 | 24.3 k / 4.5 k / 875 | 14.4 k / 5.7 k / 1.9 k |
| biggest frame / mean frame | 20x | 14x | 6x |
| worst 1-second window / mean | 1.48x | 1.41x | 1.43x |

Consequences (the sizing questions raised in the review):

* A row-band reference cache of +-16 vertical pixels covers 99% of the area for dialog and dark but **not for action**
  (p99 = 36 px, p99.9 = 72 px). An encoder-side limit of 16 px costs 0-2% in x264, so the limit is nearly free
  *if the encoder can enforce it*: x264 and x265 expose it, libaom and SVT-AV1 do not (to be checked), so an AV1-class
  design may need a transcoder or a cache-miss path.
* Key frames are 6-20 times a mean frame but the worst second is only 1.4-1.5 times the mean: a decode-ahead queue of
  about one second (a few MB of the 64 MB SDRAM) absorbs the spikes, so the engines can be sized near the mean.
* In x264, true bi-prediction is only 2-6% of B macroblocks; B-frames help through single-direction references from
  both sides (skip 48-64%). Whether AV1's compound modes earn their logic is unmeasured.

## 5. Verdict against the pre-registered kill criteria

| criterion (from the review) | result | verdict |
|---|---|---|
| AV1 at least about 10% better than x264 at equal VMAF, else choose H.264 | about 50% smaller than x264 Main+B (also 45% than High), same on SSIM and PSNR | **AV1-class passes by a wide margin** |
| a half-resolution switch must win at least about 10% on some shots | wins only with MPEG-4/H.264 on motion-heavy shots; never with HEVC/AV1; ceiling VMAF 87-92 | **fails for a modern codec; drop it** |
| context-mixing entropy recode saves at least about 4% | NOT MEASURED (needs a syntax-level recoder). Proxy: CABAC over CAVLC is 11-17% | **open**; entropy coding is a real lever |

## 6. What this changes in the plan

1. The compression-first candidate is an **AV1-class reconstruction** (hierarchical B, rich partitioning, adaptive
   arithmetic coding), not an H.264-class one: H.264 Main+B would give about -40% on the current stream, AV1-class
   about -70%. H.264-class is only preferable if the AV1-class decoder costs more than about twice the silicon: that
   is the open measurement.
2. A hardware-lean subset costs only about 3-12% by our measures: 4-layer B pyramid, 32-point transforms, no CDEF, no
   loop restoration (measured together); one or two references and a vertical range limit cost about 2% each in x264
   (not yet measured in AV1). Keep deblocking.
3. Resolution switching and "code low, synthesise high" are dropped for this codec class.
4. Under the RTL-over-reuse rule the current MPEG-4 IDCT, MC and parser are not carried over by default: the new
   decoder is designed from the table above. (The tuned-MPEG-4 flags are still a free 14-17% for the existing TNV1
   pipeline until then.)
5. The memory system, not arithmetic, remains the thing to size: vertical reach, bi-prediction share and 8-tap
   interpolation windows decide whether a row-band cache works.

## 7. Limits of this study (read before relying on any number)

* One film, three 10 s clips, 24 fps, 800x334. No grain, animation, or documentary content.
* VMAF 0.6.1 is a TV-viewing model, not a handheld one; the three metrics agree on the ordering of formats but
  disagree on deblocking and the loop filters. No subjective viewing was done.
* Encoder preset matters (see preset rows); x264 slow, x265 medium and SVT preset 6 are not equal-effort. SVT-AV1
  at preset 4 is 4-8% better still.
* "AV1 subset" figures are SVT-AV1 with the flags it exposes (temporal filter, pyramid depth, transform size, in-loop
  filters, preset). OBMC, warped motion, compound masks, palette, CfL, filter-intra, intra edge filter and
  switchable filters could not be turned off with this encoder: their value, and therefore the cost of dropping
  them, is unmeasured. libaom exposes most of them.
* The workload numbers are from H.264, not AV1.
* Bitrates exclude audio. The audio listening pack is in `work/codec_phase0/audio/` (local, not tracked) and has not
  been listened to.
* Nothing here says anything about area, timing or power: those need synthesis-only runs.

## 8. Next measurements (phase 0b)

1. Install libaom (`brew install aom`: needs the owner's go-ahead, it is a download) and sweep the AV1 tool flags
   individually to build the tool-by-tool cost table that decides the subset.
2. Instrument dav1d (BSD-2) to histogram per-block tool usage, transform sizes and vectors on the SVT/libaom streams.
3. Add a grainy live-action source and an animation (downloads: need approval) before the film-grain and the
   palette/screen-content questions.
4. Synthesis-only area probes (the V-003 method, `tools/vm_synth.py`) for the three blocks that decide the cost: an
   adaptive multi-symbol arithmetic decoder, the 4-32 point transform engine, and the 8-tap interpolation unit.
5. Audio: listen to the blind pack (`key.json` stays unopened until ranking), then measure fixed-point Opus decode
   cost on the simulated CPU.

## Reproduce

```
python3 tools/codec_study/prep.py            # cut clips, write y4m references
python3 tools/codec_study/sweep.py 8         # rate study, about 15 min on 14 cores (resumable)
python3 tools/codec_study/report.py vmaf     # also ssim, psnr_y
python3 tools/codec_study/combine.py         # one table across the three metrics
cc -O2 -o work/codec_phase0/bin/mvdump tools/codec_study/mvdump.c $(pkg-config --cflags --libs libavformat libavcodec libavutil)
python3 tools/codec_study/workload.py        # vector reach and frame-size statistics
python3 tools/codec_study/audio_pack.py      # blind Opus/MP3 listening pack
```

The source film (`media/tears-of-steel.mp4`) is git-ignored and lives in the main checkout; `prep.py` reads it by its
absolute path.
