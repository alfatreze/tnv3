# Encoder lab brief (V-280, 2026-10-30)

Goal: improve the TVC encoder (smaller streams at equal quality, same or lower decoder cost) **without touching what the decoder, the RTL or the firmware depend on**. Worktree `encoder-lab`, branch `encoder-lab`, cut from tag `tvc-dec-draft5` (the decoder side as of spec draft 5). Nothing is pushed; commit under the owner's identity, no assistant references.

## The contract
`docs/TVC_BITSTREAM_V0.md` (draft 5) is the only normative description. Every stream you produce must decode, bit-exact with your own reconstruction, on the UNCHANGED reference decoder (`tools/tvc_ref/tvc_dec.c`) and, by construction, on the hardware decoder (RTL is verified against that reference). **A change that needs a new syntax element, flag or semantics is not an encoder change**: write it up (what, why, measured gain), stop, and ask the owner; it needs a new spec draft plus decoder, RTL and firmware work elsewhere.

## You may change (enforced by `tools/encoder_guard.py`)
`tools/tvc_ref/tvc_enc.c`, new files under `tools/tvc_ref/enc/`, `tools/tvc_ref/quick_try.py`, `eval_*.py`, `make_long_clips.py`, `tools/tnv3_ref/encode_film.py`, `film_sweep.py`, `tools/encoder_lab.py`, `docs/ENCODER_*.md`, `docs/encoder_*.json`, `work/`.
**Frozen**: everything else, in particular `tools/tvc_ref/tvc.h` (shared by the encoder, the reference decoder and the C models behind the RTL golden traces: changing it can silently move the RTL test goldens), `tvc_dec.c`, the spec, `src/`, `fw/`, `sim/`, `Makefile`. If the encoder needs a function that lives in `tvc.h`, copy it into `tools/tvc_ref/enc/` rather than editing it.

## Commands
```
python3 tools/encoder_lab.py build                       # compile tvc_enc and tvc_dec from this worktree
python3 tools/encoder_lab.py run OUT.json [--vmaf]       # corpus x 3 QPs: sizes, PSNR-Y, decoder match
python3 tools/encoder_lab.py compare docs/encoder_baseline.json OUT.json     # Bjontegaard rate change (negative = better)
python3 tools/encoder_lab.py gate                        # guard + frozen vectors + parse budget + run + compare with the baseline
python3 tools/tvc_ref/quick_try.py CLIP QP [encoder args]   # one clip, quick check (its ffmpeg PSNR is wrong on the *_400 clips: see below)
```
Clips: `work/encoder_lab/ref/*_400.y4m` (symlink to the study clips of the `codec-phase0` worktree, read-only; `*_800` are there too). The corpus uses the first 96 frames of dialog, action, anime_dialog and bw_room at QP 24, 30, 36. Run in about 50 s.

## Baseline (the encoder at tag tvc-dec-draft5; `docs/encoder_baseline.json`)
| clip | QP 24 | QP 30 | QP 36 |
|---|---|---|---|
| dialog | 2328 B/f, 41.79 dB | 1090 B/f, 38.00 dB | 473 B/f, 33.91 dB |
| action | 3727 B/f, 41.04 dB | 1856 B/f, 37.02 dB | 876 B/f, 33.26 dB |
| anime_dialog | 3992 B/f, 43.30 dB | 1958 B/f, 39.38 dB | 895 B/f, 35.50 dB |
| bw_room | 3154 B/f, 42.43 dB | 1394 B/f, 38.36 dB | 545 B/f, 34.26 dB |
PSNR-Y is the mean per-frame PSNR of the luma plane, computed in `encoder_lab.py`.

## The gate (what "better" means)
1. `encoder_guard.py` passes (nothing frozen touched).
2. Every stream decodes on the frozen decoder, and the encoder's own reconstruction (`-r`) equals the decoder's output (md5).
3. `check_c_vectors.py` passes (the frozen decoder still produces the expected output on the committed vectors).
4. The Bjontegaard rate change against the baseline is negative on the corpus mean and not worse than +1 % on any clip.
5. The decoder cost does not rise: `tools/tvc_ref/parse_budget.py` states the budget (hardware parser: about 3.2 clocks per range-coder bin, 35 per macroblock; bins per byte measured 9.4 on average, 8.0 to 12.4; level limits per picture and per second in spec section 8). **Gap:** the gate only runs the model, it does not measure the bins per byte of your streams. Until it does, report bins per byte (or bytes and bins per macroblock) next to every claimed gain; an encoder change that raises bins per macroblock costs hardware decode time.

## Known hazards
* `eval_tvc.py`, `quick_try.py`, `make_long_clips.py` have absolute paths into the `codec-phase0` worktree and an old scratchpad: use `encoder_lab.py`, or fix the paths inside your own copies.
* The source clips are tagged limited-range, the decoder output is not: ffmpeg's `psnr` filter then rescales one side and reports nonsense (about 12 to 23 dB instead of 33 to 43). `encoder_lab.py` computes PSNR itself; with `--vmaf` it forces the same range tag on both inputs (VMAF path not validated against a trusted number).
* `tvc_enc` is single-threaded and holds its whole input in memory (it is a reference tool); `encode_film.py` parallelises by segments.
* Not yet in the encoder (its own header): scene-cut keys, trellis quantisation, quarter-pel. The decoder side already supports quarter-sample motion (draft 3), the 4x4 luma transform (draft 4) and slices (draft 5); check which of these the encoder uses before spending time on them.
* Level compliance (per-picture and per-second byte limits, spec section 8) is enforced by the encoder with `-l LEVEL`; the corpus run does not use it. Run the full film path (`encode_film.py`, `tnv3 check`) before claiming a result for real content.
* Keep an execution log in `docs/ENCODER_LOG.md` (new entries `E-NNN`: change, baseline against new, gate output).

## First quality target, from the first Pocket run (2026-10-30)
The film played on a Pocket (400 x 176, level 1, QP 30) and its output equals the reference decoder's, so the hardware is not the problem here; but the STREAM has artifacts. In the Tears of Steel excerpt (a 90 s cut starting at 300 s of the film, scaled to 400 wide, 2,160 frames) 17 frames (0.8 %) have a bottom-region error more than twice the top region's and above an MSE of 80, found by comparing the reference decode with the source (luma, bottom 40 rows against the top 100): frames 66-67, 70, 81, 87, 129-132, 148, 156, 159-160, 165, 172-175, 178 (2.8 s to 7.4 s: the fast-moving hologram scene), mostly isolated frames (probably non-reference B pictures; not confirmed). Frame 157 shows a magenta / green blocky band over its lower quarter. The level cap is not involved (no pictures were raised for the level). The encoder's own reconstruction equals the decoder's, so this is the encoder's choices (motion search near the bottom edge, mode or QP decisions, B-picture prediction), not a decode error. To reproduce: cut the excerpt with ffmpeg (`-ss 300 -t 90` of media/tears-of-steel.mp4), pad the source to a height that scales to a multiple of 16 (1920 x 844 gives 400 x 176), then `python3 tools/tnv3_ref/encode_film.py SRC OUT.tnv3 --width 400 --qp 30 --seconds 90 --quality`, demux with `tools/tnv3_ref/tnv3 demux`, decode with `tvc_dec` and compare per frame against the source. A good first task: add a per-frame bottom-versus-top error check to the gate (a regression test for this class), then find the cause. Also noted: the padding rows of a height that is not a multiple of 16 are filled by repeating the last row (`pad_frame` in tvc_enc.c); black padding would be invisible on the Pocket (which shows the whole coded picture), without any spec change.

**Later clarification (2026-10-31):** The owner suspects the visible band was
player/window padding outside the video. This has not been reproduced here. The
luma-error heuristic and the coloured display artifact are separate observations;
neither establishes the other's cause. The per-frame luma check is now a
reporting diagnostic, not a confirmed encoder-defect gate. See E-030 in
`docs/ENCODER_LOG.md` for evidence and limitations.
