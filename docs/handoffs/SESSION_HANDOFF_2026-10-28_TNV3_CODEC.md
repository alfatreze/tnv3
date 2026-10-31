# Session handoff 2026-10-28: TNV3 custom codec (TVC video + TAC audio)

Worktree `tau-nu/.claude/worktrees/codec-alternatives`, branch `worktree-codec-alternatives` (from `codec-phase0` at 08f581d). All committed, **nothing pushed**. Audit V-181..V-196.

## Direction (owner decisions, 2026-10-27/28)
- Drop AV1; build a **fully custom** A/V/subtitle codec family **TNV3** (existing codecs only for learning; existing project RTL is not re-derived).
- Bar: a good fit for the Pocket with reasonably small files (budget about 400-450 MB/h at 800x450), not "beat MPEG-2" or "match H.264".
- One tool set with **levels** (L1 400x240, L2 800x464, L3 800x720, all up to 30 fps), not profiles.
- Source frame rates are kept (rational rate + audio samples per frame; 23.976 = 2002 samples).
- Audio: lossy MDCT codec, **locked at 96 kbit/s** (blind test: TAC 96k/128k, MP3 128k, Opus 64k/96k all indistinguishable from the reference to the owner).
- Commits always under the owner's identity, no assistant references; simple tasks go to Haiku/Sonnet subagents.

## What exists
| Piece | Where | State |
|---|---|---|
| Studies | `docs/CODEC_ALTERNATIVES_2026-10-27.md`, `CODEC_CLEAN_SHEET_…`, `TNV3_CODEC_SCOPE_…` (10b = decisions), `TNV3_ADVERSARIAL_REVIEW_…`, `TNV3_R0_RESULTS_…`; scripts `tools/codec_study/clean_sheet/` | done |
| **TVC video** | spec `docs/TVC_BITSTREAM_V0.md` **draft 5**; C `tools/tvc_ref/` (`tvc.h` shared syntax/recon, `tvc_dec.c`, `tvc_enc.c`); independent Python decoder `tools/tvc_ref/py/`; 19 vectors + 3 must-reject (`vectors/expected.txt`); `check_c_vectors.py`, `eval_tvc.py`, `eval_an.py`, `quick_try.py` | I/P/B, 16x16 MB, qpel own 4-tap, 8x8 + 4x4 option, 4 intra modes per 8x8, binary range coder, deblocking, slices, levels; encoder v5: RD modes, look-ahead QP, scene cuts, RDOQ |
| TVC results | `docs/TVC_V1_RESULTS_2026-10-27.md`, `docs/codec_phase0/tvc_v*_eval.txt` | 9 clips at VMAF 90 / SSIM 0.97 / PSNR 38: **1.00 / 1.04 / 0.91 x x264 slow**, 0.60 / 0.42 / 0.47 x MPEG-2; anime the remaining gap (1.09-1.16x); about 230 MB/h film, 140 MB/h anime incl. audio |
| **TAC audio** | spec `docs/TAC_BITSTREAM_V0.md` draft 1.1; C `tools/tac_ref/tac.c` (`tac enc/dec/test`); Python decoder `tools/tac_ref/py/`; 6 vectors with reference WAVs; `listening_pack.py` | MDCT 2048/8x256 block switching, 42/13 bands, M/S per band, noise filling; tonality psy model; independent decoder sample-identical |
| Rejected after measurement | lapped transform (2-6% over deblocked DCT), wavelet (no inter tool), contexts carried across pictures (-1%), variance AQ (hurts), B count 1/3 (2 is best), separable audio layers | recorded in the docs |

## Build / test commands (from the worktree root)
- `cc -O2 -Wno-unused-function -o tools/tvc_ref/tvc_enc tools/tvc_ref/tvc_enc.c -lm` (same for `tvc_dec`); `python3 tools/tvc_ref/check_c_vectors.py`; `python3 tools/tvc_ref/py/run_vectors.py`
- `cc -O2 -Wno-unused-function -Wno-unused-parameter -o tools/tac_ref/tac tools/tac_ref/tac.c -lm`; `tools/tac_ref/tac test`; `python3 tools/tac_ref/py/run_vectors.py`
- Reference clips (read-only): `tau-nu/.claude/worktrees/codec-phase0/work/codec_phase0/ref/{name}_800.y4m`; 60 s segments via `tools/tvc_ref/make_long_clips.py <dir>`.
- Gotchas: the sandbox refuses shell loops/variables in commands: write Python scripts; ffmpeg `psnr`/`libvmaf` repeat the last frame if lengths differ (compare equal lengths); the anime episode's audio is damaged.

## Next (in order of value)
1. **TNV3 container** (video/audio/subtitle packets, index, frame-locked audio sample counts, glyph table) with reference mux/demux.
2. **Fixed-point TAC decoder definition** (dequantiser tables, IMDCT) and validation of the conformance tolerance.
3. Encoder: per-second rate limit (only the decoder checks it now); anime gap (more vectors/partitions?), speed (7 fps).
4. Subtitle (TST) reference; then hardware: area probes of the TVC entropy lane, transform, MC, SDRAM/PSRAM bandwidth measurement on a Pocket (E-G of the scope).

## Addendum (same day, V-198..V-200)
- **TNV3 container done:** `docs/TNV3_CONTAINER_V0.md` draft 1.1; `tools/tnv3_ref/` (C mux/demux/check/info, 9 + 15 vectors, round trip, 10 mutants, independent Python demuxer 24/24). Audio-master timing S(k), seek audio frame in the index.
- **TAC fixed point done:** `docs/TAC_BITSTREAM_V0.md` draft 1.2 section 10; `tools/tac_ref/tac_fx.c` (integer model, oracle for the RTL), `tac_gen.c` (crafted vectors), vectors t07/t08, `fx_expected.txt`, `fx_mutants.py`, Python `py/tac_fx.py` bit-exact. Tolerance +-1 / 0.3 RMS.
- Next in order: (3) encoder per-second rate limit and the anime gap, (4) TST subtitle reference, then hardware probes (TVC entropy lane, transform, MC, IMDCT; SDRAM/PSRAM bandwidth on a Pocket).
- Build: `cc -O2 -Wno-unused-function -Wno-unused-parameter -o tools/tac_ref/tac_fx tools/tac_ref/tac_fx.c -lm` (same for `tac_gen`), `cc -O2 -Wall -o tools/tnv3_ref/tnv3 tools/tnv3_ref/tnv3.c`.

## Addendum 2 (same day, V-201..V-206)
- **Encoder level limits** enforced (per picture and per second, with a reserve for the rest of each window; `-P` carries a segment joint). **Film pipeline** `tools/tnv3_ref/encode_film.py` / `film_sweep.py`.
- **Whole-film sweep** (`docs/TNV3_FILM_SWEEP_2026-10-28.md`): 100-280 MB/h at VMAF 90, budget met; QP 30 files in `work/films/`.
- **Subtitles** (container drafts 1.3-1.4, `tools/tst_ref/`): tracks, 1-8 baked sizes, forced / SDH flags, multiple fonts, per-track authored palette with inline colour switches; player parameters (outline colour/width, box, position, delay, palette override); owner dropped shadow and opacity. Open owner choice: none pending (outline is player-drawn).
- **Memory study** (`docs/TNV3_MEMORY_STUDY_2026-10-28.md`, `tools/mem_study/`): the existing SDRAM controller (also Tau Alpha's, only renamed) cannot carry Level 3 (138-146% of 100 MHz); an open-page bank-parallel one needs about 60%. **Owner decision: Tau Alpha builds the new controller, Tau V copies it**; requirements handed over in `docs/SDRAM_CONTROLLER_REQUIREMENTS.md` (replay request files in `work/mem_study/`, SHA-256 in the doc).
- Tau Alpha state (surveyed): Talos 2 graphics rewrite, Player 2 firmware (`fw2/`, SD 2,433 KB/s), PSRAM async only, 800x720 output on its `test/720` branch (simulated only).
- **Next options:** Pocket test core (SD large reads, PSRAM, SDRAM soak at 150 MHz) once the new controller exists or before; TVC range-decoder RTL (first video block); TAC IMDCT RTL against `tac_fx.c`; encoder speed and a quality-targeted rate mode; anime gap.

